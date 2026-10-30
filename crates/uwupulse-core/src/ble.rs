//! Chest straps straight at the desktop: scan for the Heart Rate service
//! (0x180D), subscribe to Heart Rate Measurement (0x2A37), and reconnect
//! while the user wants the strap.
//!
//! The Bluetooth manager starts on first use, not at launch: on macOS the
//! first touch triggers the permission prompt, which should come when the
//! user opens the BLE page, not when the app starts.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use btleplug::api::bleuuid::uuid_from_u16;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use futures_util::StreamExt;
use parking_lot::Mutex;
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;
use uwupulse_proto::{hrm, limits, Src};

use crate::hub::Inner;
use crate::net::now_ms;
use crate::types::{BleDevice, HrSample, Source, SourceKind};
use crate::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Waits between reconnect attempts; the last one repeats.
const BACKOFF: [u64; 4] = [1, 2, 5, 10];
/// Scanned straps kept in the list. Anyone nearby can advertise new
/// addresses without end; past this the longest-unseen idle one goes.
const MAX_DEVICES: usize = 64;

struct Dev {
    pid: PeripheralId,
    name: String,
    rssi: Option<i16>,
    connected: bool,
    connecting: bool,
    /// Last advertisement or update.
    seen: Instant,
}

#[derive(Default)]
struct State {
    devices: BTreeMap<String, Dev>,
    /// Straps the user connected and hasn't disconnected.
    wanted: HashSet<String>,
    tasks: HashMap<String, JoinHandle<()>>,
}

#[derive(Default)]
pub(crate) struct Ble {
    /// The manager is kept alive next to its adapter.
    adapter: OnceCell<(Manager, Adapter)>,
    state: Mutex<State>,
    events_task: Mutex<Option<JoinHandle<()>>>,
}

fn source_id(id: &str) -> String {
    format!("ble:{id}")
}

impl Ble {
    pub fn list(&self) -> Vec<BleDevice> {
        let st = self.state.lock();
        let mut out: Vec<BleDevice> = st
            .devices
            .iter()
            .map(|(id, d)| BleDevice {
                id: id.clone(),
                name: d.name.clone(),
                rssi: d.rssi,
                connected: d.connected,
                connecting: d.connecting,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        out
    }

    fn set_flags(&self, id: &str, connected: bool, connecting: bool) {
        if let Some(d) = self.state.lock().devices.get_mut(id) {
            d.connected = connected;
            d.connecting = connecting;
        }
    }

    fn wanted(&self, id: &str) -> bool {
        self.state.lock().wanted.contains(id)
    }
}

impl State {
    /// Makes room for one more device: drops the longest-unseen ones that are
    /// neither connected, connecting nor wanted. False when nothing can go.
    fn make_room(&mut self) -> bool {
        while self.devices.len() >= MAX_DEVICES {
            let oldest = self
                .devices
                .iter()
                .filter(|(id, d)| !d.connected && !d.connecting && !self.wanted.contains(*id))
                .min_by_key(|(_, d)| d.seen)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => {
                    self.devices.remove(&id);
                }
                None => return false,
            }
        }
        true
    }
}

fn emit_list(inner: &Inner) {
    inner.emit(crate::types::Event::Ble(inner.ble.list()));
}

/// The adapter, set up on first use, with a task that collects discovered
/// heart rate devices.
async fn adapter(inner: &Arc<Inner>) -> Result<Adapter, Error> {
    let (_, adapter) = inner
        .ble
        .adapter
        .get_or_try_init(|| async {
            let manager = Manager::new().await.map_err(ble_err)?;
            let adapter = manager
                .adapters()
                .await
                .map_err(ble_err)?
                .into_iter()
                .next()
                .ok_or_else(|| Error::Ble("no Bluetooth adapter found".into()))?;
            Ok::<_, Error>((manager, adapter))
        })
        .await?;
    let adapter = adapter.clone();
    if inner.ble.events_task.lock().is_none() {
        let mut events = adapter.events().await.map_err(ble_err)?;
        let weak = Arc::downgrade(inner);
        let a = adapter.clone();
        let handle = inner.rt.spawn(async move {
            while let Some(ev) = events.next().await {
                let Some(inner) = weak.upgrade() else { break };
                on_event(&inner, &a, ev).await;
            }
        });
        let mut task = inner.ble.events_task.lock();
        match task.as_ref() {
            // Another caller won the race; keep theirs.
            Some(_) => handle.abort(),
            None => *task = Some(handle),
        }
    }
    Ok(adapter)
}

fn ble_err(e: btleplug::Error) -> Error {
    Error::Ble(e.to_string())
}

async fn on_event(inner: &Arc<Inner>, adapter: &Adapter, ev: CentralEvent) {
    let hrs = uuid_from_u16(hrm::HEART_RATE_SERVICE);
    match ev {
        CentralEvent::DeviceDiscovered(pid)
        | CentralEvent::DeviceUpdated(pid)
        | CentralEvent::ServicesAdvertisement { id: pid, .. } => {
            let id = pid.to_string();
            let Ok(p) = adapter.peripheral(&pid).await else {
                return;
            };
            let Ok(Some(props)) = p.properties().await else {
                return;
            };
            let known = inner.ble.state.lock().devices.contains_key(&id);
            if !known && !props.services.contains(&hrs) {
                return;
            }
            let name = props
                .local_name
                .or(props.advertisement_name)
                .map(|n| limits::truncate_chars(n.trim(), limits::MAX_NAME_CHARS).to_owned())
                .filter(|n| limits::valid_name(n))
                .unwrap_or_else(|| "Heart rate sensor".to_owned());
            let changed = {
                let mut st = inner.ble.state.lock();
                // Room for a new one first, while nothing else is borrowed.
                let room = st.devices.contains_key(&id) || st.make_room();
                match st.devices.get_mut(&id) {
                    Some(d) => {
                        let changed = d.name != name
                            || d.rssi
                                .zip(props.rssi)
                                .is_none_or(|(a, b)| a.abs_diff(b) >= 5);
                        d.name = name;
                        d.rssi = props.rssi.or(d.rssi);
                        d.seen = Instant::now();
                        changed
                    }
                    None if room => {
                        st.devices.insert(
                            id,
                            Dev {
                                pid,
                                name,
                                rssi: props.rssi,
                                connected: false,
                                connecting: false,
                                seen: Instant::now(),
                            },
                        );
                        true
                    }
                    None => false,
                }
            };
            if changed {
                emit_list(inner);
            }
        }
        CentralEvent::DeviceDisconnected(pid) => {
            let id = pid.to_string();
            let was = inner
                .ble
                .state
                .lock()
                .devices
                .get(&id)
                .is_some_and(|d| d.connected);
            if was {
                inner.ble.set_flags(&id, false, false);
                emit_list(inner);
            }
        }
        _ => {}
    }
}

pub(crate) async fn set_scanning(inner: &Arc<Inner>, on: bool) -> Result<(), Error> {
    let adapter = adapter(inner).await?;
    if on {
        adapter
            .start_scan(ScanFilter {
                services: vec![uuid_from_u16(hrm::HEART_RATE_SERVICE)],
            })
            .await
            .map_err(ble_err)
    } else {
        adapter.stop_scan().await.map_err(ble_err)
    }
}

pub(crate) fn connect(inner: &Arc<Inner>, id: &str) -> Result<(), Error> {
    let mut st = inner.ble.state.lock();
    if !st.devices.contains_key(id) {
        return Err(Error::NotFound);
    }
    st.wanted.insert(id.to_owned());
    if st.tasks.get(id).is_some_and(|t| !t.is_finished()) {
        return Ok(());
    }
    let task = inner
        .rt
        .spawn(keep_connected(Arc::downgrade(inner), id.to_owned()));
    st.tasks.insert(id.to_owned(), task);
    Ok(())
}

pub(crate) async fn disconnect(inner: &Arc<Inner>, id: &str) -> Result<(), Error> {
    let (task, pid) = {
        let mut st = inner.ble.state.lock();
        st.wanted.remove(id);
        let pid = st.devices.get(id).map(|d| d.pid.clone());
        (st.tasks.remove(id), pid)
    };
    let pid = pid.ok_or(Error::NotFound)?;
    if let Some(t) = task {
        t.abort();
    }
    if let Some((_, adapter)) = inner.ble.adapter.get() {
        if let Ok(p) = adapter.peripheral(&pid).await {
            let _ = p.disconnect().await;
        }
    }
    inner.ble.set_flags(id, false, false);
    inner.remove_source(&source_id(id));
    emit_list(inner);
    Ok(())
}

pub(crate) async fn shutdown(inner: &Arc<Inner>) {
    let (tasks, pids): (Vec<_>, Vec<_>) = {
        let mut st = inner.ble.state.lock();
        st.wanted.clear();
        let pids = st
            .devices
            .values()
            .filter(|d| d.connected)
            .map(|d| d.pid.clone())
            .collect();
        (st.tasks.drain().map(|(_, t)| t).collect(), pids)
    };
    for t in tasks {
        t.abort();
    }
    if let Some(t) = inner.ble.events_task.lock().take() {
        t.abort();
    }
    if let Some((_, adapter)) = inner.ble.adapter.get() {
        let _ = adapter.stop_scan().await;
        for pid in pids {
            if let Ok(p) = adapter.peripheral(&pid).await {
                let _ = p.disconnect().await;
            }
        }
    }
}

/// Connects, streams, and on a drop tries again until the user lets go.
async fn keep_connected(weak: Weak<Inner>, id: String) {
    let mut attempt = 0usize;
    loop {
        let Some(inner) = weak.upgrade() else { return };
        if !inner.ble.wanted(&id) {
            return;
        }
        inner.ble.set_flags(&id, false, true);
        emit_list(&inner);
        match stream(&inner, &id).await {
            // It was connected for a while: start the backoff over.
            Ok(()) => attempt = 0,
            Err(e) => log::warn!("BLE {id}: {e}"),
        }
        inner.ble.set_flags(&id, false, false);
        inner.remove_source(&source_id(&id));
        emit_list(&inner);
        drop(inner);
        let wait = BACKOFF[attempt.min(BACKOFF.len() - 1)];
        attempt += 1;
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}

/// One connection: until the strap goes away.
async fn stream(inner: &Arc<Inner>, id: &str) -> Result<(), Error> {
    let adapter = adapter(inner).await?;
    let (pid, name) = {
        let st = inner.ble.state.lock();
        let d = st.devices.get(id).ok_or(Error::NotFound)?;
        (d.pid.clone(), d.name.clone())
    };
    let p = adapter.peripheral(&pid).await.map_err(ble_err)?;
    if !p.is_connected().await.unwrap_or(false) {
        tokio::time::timeout(CONNECT_TIMEOUT, p.connect())
            .await
            .map_err(|_| Error::Ble("connecting timed out".into()))?
            .map_err(ble_err)?;
    }
    p.discover_services().await.map_err(ble_err)?;
    let hrm_uuid = uuid_from_u16(hrm::HEART_RATE_MEASUREMENT);
    let ch = p
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == hrm_uuid)
        .ok_or_else(|| Error::Ble("no heart rate measurement characteristic".into()))?;
    p.subscribe(&ch).await.map_err(ble_err)?;
    let mut notes = p.notifications().await.map_err(ble_err)?;

    let src_id = source_id(id);
    inner.ble.set_flags(id, true, false);
    emit_list(inner);
    inner.set_source(Source {
        id: src_id.clone(),
        kind: SourceKind::Ble,
        name: name.clone(),
        connected: true,
        last_bpm: None,
        last_ts: None,
        detail: None,
    });
    while let Some(n) = notes.next().await {
        if n.uuid != hrm_uuid {
            continue;
        }
        if let Some(hr) = hrm::parse(&n.value) {
            inner.ingest(
                &src_id,
                HrSample {
                    ts: now_ms(),
                    bpm: hr.bpm,
                    src: Src::Ble,
                    dev: name.clone(),
                },
            );
        }
    }
    Ok(())
}
