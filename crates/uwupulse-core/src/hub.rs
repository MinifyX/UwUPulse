//! The shared state behind [`crate::Pulse`] and everything that changes it:
//! samples coming in, sources coming and going, live/idle, recording.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc, oneshot, watch, Semaphore};
use uwupulse_proto::{limits, DeskMsg, StaticKeypair};

use crate::config::{PairedRecord, Store};
use crate::net::{lan_ips, now_ms};
use crate::recorder::Recorder;
use crate::server::Running;
use crate::types::{
    AppState, Event, HrSample, PairedDevice, ServerInfo, Settings, Source, SourceKind,
};

/// A value newer than this is "live".
pub const LIVE_WINDOW: Duration = Duration::from_secs(10);
/// Phone timestamps further off than this from the desktop's clock are
/// replaced by the desktop's.
const MAX_CLOCK_SKEW_MS: u64 = 5 * 60 * 1000;
/// API WebSocket clients at the same time (overlays, bots, …).
pub const MAX_API_SOCKETS: usize = 64;
/// New link connections per address and minute.
const LINK_ATTEMPTS_PER_MINUTE: u32 = 30;

pub(crate) struct LiveState {
    pub current: Option<HrSample>,
    pub last_at: Option<Instant>,
    pub live: bool,
    pub sources: BTreeMap<String, Source>,
    /// A sample came in since the last `State` event.
    pub dirty: bool,
}

pub(crate) struct ServerStatus {
    pub port: u16,
    pub lan: bool,
    pub error: Option<String>,
}

pub(crate) enum LinkCmd {
    Send(DeskMsg),
    Close,
}

pub(crate) struct LinkEntry {
    pub device_id: String,
    pub tx: mpsc::Sender<LinkCmd>,
}

pub(crate) struct Offer {
    pub secret: [u8; 32],
    pub expires: Instant,
}

pub(crate) struct SasSlot {
    pub request_id: String,
    pub answer: Option<oneshot::Sender<bool>>,
}

/// Link connections that have not reached `ready` yet.
#[derive(Default)]
pub(crate) struct PreAuthCount {
    pub total: usize,
    pub per_ip: HashMap<IpAddr, usize>,
}

#[derive(Default)]
pub(crate) struct PairingState {
    pub offer: Option<Offer>,
    pub sas: Option<SasSlot>,
}

/// The `/ui` transport of this run.
pub(crate) struct UiState {
    pub dir: std::path::PathBuf,
    pub host: Arc<dyn crate::commands::Host>,
    /// Required on `/ui/rpc` and `/ui/events`; new with every start.
    pub secret: String,
}

pub(crate) struct Inner {
    pub rt: Handle,
    pub store: Store,
    pub app_version: String,
    pub desk_id: String,
    pub desk_name: String,
    pub keypair: StaticKeypair,
    pub token: Mutex<String>,
    pub settings: Mutex<Settings>,
    pub paired: Mutex<Vec<PairedRecord>>,
    pub widgets: Mutex<Vec<Value>>,
    pub events: broadcast::Sender<Event>,
    pub live: Mutex<LiveState>,
    pub recorder: Recorder,
    pub server: tokio::sync::Mutex<Option<Running>>,
    pub server_status: Mutex<ServerStatus>,
    /// Bumped to end every open WebSocket (server restart, shutdown).
    pub generation: watch::Sender<u64>,
    /// Bumped when the token changes: API sockets opened with the old one end.
    pub token_generation: watch::Sender<u64>,
    /// Tests bind port 0; the app leaves this `None` and uses the settings.
    pub port_override: Option<u16>,
    pub header_timeout: Duration,
    pub pairing: Mutex<PairingState>,
    pub links: Mutex<HashMap<u64, LinkEntry>>,
    pub next_conn: AtomicU64,
    pub link_slots: Arc<Semaphore>,
    pub api_slots: Arc<Semaphore>,
    pub link_attempts: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    pub pre_auth: Mutex<PreAuthCount>,
    pub bonjour: bool,
    /// [`crate::PulseOptions::ble`].
    pub ble_allowed: bool,
    pub advert: Mutex<Option<crate::mdns::Advert>>,
    pub ticker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// `/ui/`: only when the host asked for it.
    pub ui: Option<UiState>,
    #[cfg(feature = "ble")]
    pub ble: crate::ble::Ble,
}

impl Inner {
    pub fn emit(&self, ev: Event) {
        // No receivers is fine.
        let _ = self.events.send(ev);
    }

    pub fn server_info(&self) -> ServerInfo {
        let st = self.server_status.lock();
        let mut urls = vec![format!("http://127.0.0.1:{}", st.port)];
        if st.lan {
            urls.extend(
                lan_ips()
                    .iter()
                    .map(|ip| format!("http://{ip}:{}", st.port)),
            );
        }
        ServerInfo {
            port: st.port,
            lan_enabled: st.lan,
            token: self.token.lock().clone(),
            urls,
            error: st.error.clone(),
        }
    }

    pub fn app_state(&self) -> AppState {
        let (current, live, sources) = {
            let l = self.live.lock();
            (
                l.current.clone(),
                l.live,
                l.sources.values().cloned().collect(),
            )
        };
        AppState {
            current,
            live,
            sources,
            server: self.server_info(),
            recording: self.recorder.recording(),
        }
    }

    pub fn emit_state(&self) {
        self.live.lock().dirty = false;
        self.emit(Event::State(Box::new(self.app_state())));
    }

    pub fn paired_devices(&self) -> Vec<PairedDevice> {
        let connected: Vec<String> = self
            .links
            .lock()
            .values()
            .map(|l| l.device_id.clone())
            .collect();
        self.paired
            .lock()
            .iter()
            .map(|p| PairedDevice {
                id: p.id.clone(),
                name: p.name.clone(),
                paired_at: p.paired_at,
                last_seen: p.last_seen,
                connected: connected.contains(&p.id),
            })
            .collect()
    }

    pub fn emit_paired(&self) {
        self.emit(Event::Paired(self.paired_devices()));
    }

    pub fn save_paired(&self) {
        let list = self.paired.lock().clone();
        if let Err(e) = self.store.write("paired.json", &list, true) {
            log::error!("saving paired devices failed: {e}");
        }
    }

    /// Stores a newly paired phone. Pairing the same device with the same
    /// key again replaces the old record; an ID that is already paired with
    /// another key is refused (false) and the old pairing stays.
    pub fn add_paired(&self, id: &str, name: &str, spk_b64: String) -> bool {
        {
            let mut p = self.paired.lock();
            if p.iter().any(|d| d.id == id && d.spk != spk_b64) {
                return false;
            }
            p.retain(|d| d.id != id);
            p.push(PairedRecord {
                id: id.to_owned(),
                name: name.to_owned(),
                spk: spk_b64,
                paired_at: now_ms(),
                last_seen: Some(now_ms()),
            });
        }
        self.save_paired();
        true
    }

    pub fn touch_paired(&self, id: &str) {
        let found = {
            let mut p = self.paired.lock();
            match p.iter_mut().find(|d| d.id == id) {
                Some(d) => {
                    d.last_seen = Some(now_ms());
                    true
                }
                None => false,
            }
        };
        if found {
            self.save_paired();
        }
    }

    pub fn set_source(&self, source: Source) {
        self.live.lock().sources.insert(source.id.clone(), source);
        self.emit_state();
    }

    pub fn update_source(&self, id: &str, f: impl FnOnce(&mut Source)) {
        let changed = match self.live.lock().sources.get_mut(id) {
            Some(s) => {
                f(s);
                true
            }
            None => false,
        };
        if changed {
            self.emit_state();
        }
    }

    pub fn remove_source(&self, id: &str) {
        let removed = self.live.lock().sources.remove(id).is_some();
        if removed {
            self.emit_state();
        }
    }

    /// One heart rate from any source.
    pub fn ingest(&self, source_id: &str, mut sample: HrSample) {
        if !limits::valid_bpm(sample.bpm as i64) {
            return;
        }
        let now = now_ms();
        if sample.ts.abs_diff(now) > MAX_CLOCK_SKEW_MS {
            sample.ts = now;
        }
        let became_live = {
            let mut l = self.live.lock();
            l.current = Some(sample.clone());
            l.last_at = Some(Instant::now());
            l.dirty = true;
            if let Some(s) = l.sources.get_mut(source_id) {
                s.last_bpm = Some(sample.bpm);
                s.last_ts = Some(sample.ts);
            }
            !std::mem::replace(&mut l.live, true)
        };
        let record = self.settings.lock().record_sessions;
        let started = record
            && match self.recorder.push(&sample) {
                Ok(p) => p.started,
                Err(e) => {
                    log::error!("recording a sample failed: {e}");
                    false
                }
            };
        self.emit(Event::Hr(sample));
        if became_live {
            self.emit(Event::Live(true));
        }
        if became_live || started {
            self.emit_state();
        }
        if started {
            self.emit(Event::Sessions);
        }
    }

    /// Once a second: live → idle, flushing and ending sessions, a `State`
    /// while samples flow.
    pub fn tick(&self) {
        let (went_idle, dirty) = {
            let mut l = self.live.lock();
            let idle = l.live && l.last_at.is_none_or(|t| t.elapsed() > LIVE_WINDOW);
            if idle {
                l.live = false;
            }
            (idle, l.dirty)
        };
        let idle_secs = self.settings.lock().idle_timeout_secs;
        let ended = match self.recorder.tick(Duration::from_secs(idle_secs as u64)) {
            Ok(e) => e,
            Err(e) => {
                log::error!("writing samples failed: {e}");
                false
            }
        };
        if went_idle {
            self.emit(Event::Live(false));
        }
        if went_idle || ended || dirty {
            self.emit_state();
        }
        if ended {
            self.emit(Event::Sessions);
        }
        let mut pairing = self.pairing.lock();
        if pairing
            .offer
            .as_ref()
            .is_some_and(|o| o.expires <= Instant::now())
        {
            pairing.offer = None;
        }
    }

    /// Ends every open WebSocket of the current server generation.
    pub fn close_connections(&self) {
        self.generation.send_modify(|g| *g += 1);
    }

    /// Counts a link attempt from an address; false once it's over the limit.
    pub fn allow_link_attempt(&self, ip: IpAddr) -> bool {
        let mut map = self.link_attempts.lock();
        let now = Instant::now();
        if map.len() > 256 {
            map.retain(|_, (t, _)| now.duration_since(*t) < Duration::from_secs(60));
        }
        let entry = map.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= Duration::from_secs(60) {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= LINK_ATTEMPTS_PER_MINUTE
    }

    /// Takes a slot for a link connection before `ready`; false when there
    /// are too many already, in total or from this address.
    pub fn acquire_pre_auth(&self, ip: IpAddr) -> bool {
        let mut c = self.pre_auth.lock();
        let from_ip = c.per_ip.get(&ip).copied().unwrap_or(0);
        if c.total >= limits::MAX_PREAUTH_CONNECTIONS || from_ip >= limits::MAX_PREAUTH_PER_IP {
            return false;
        }
        c.total += 1;
        c.per_ip.insert(ip, from_ip + 1);
        true
    }

    pub fn release_pre_auth(&self, ip: IpAddr) {
        let mut c = self.pre_auth.lock();
        c.total = c.total.saturating_sub(1);
        if let Some(n) = c.per_ip.get_mut(&ip) {
            *n -= 1;
            if *n == 0 {
                c.per_ip.remove(&ip);
            }
        }
    }

    pub fn register_link(&self, device_id: &str, tx: mpsc::Sender<LinkCmd>) -> u64 {
        let id = self.next_conn.fetch_add(1, Ordering::Relaxed);
        let mut links = self.links.lock();
        // A phone that reconnects before the old connection timed out: the
        // new one wins.
        for l in links.values().filter(|l| l.device_id == device_id) {
            let _ = l.tx.try_send(LinkCmd::Close);
        }
        links.insert(
            id,
            LinkEntry {
                device_id: device_id.to_owned(),
                tx,
            },
        );
        id
    }

    pub fn unregister_link(&self, conn: u64) {
        self.links.lock().remove(&conn);
    }

    pub fn advertise(&self) {
        let mut advert = self.advert.lock();
        *advert = None;
        if !self.bonjour {
            return;
        }
        let (port, lan, running) = {
            let st = self.server_status.lock();
            (st.port, st.lan, st.error.is_none())
        };
        // Without LAN access no phone could connect anyway.
        if !lan || !running {
            return;
        }
        match crate::mdns::advertise(
            &self.desk_name,
            &self.desk_id,
            &self.keypair.public_b64(),
            port,
        ) {
            Ok(a) => *advert = Some(a),
            Err(e) => log::warn!("Bonjour advertisement failed: {e}"),
        }
    }
}

/// The once-a-second loop. Holds only a weak reference, so it ends with the
/// hub.
pub(crate) fn spawn_ticker(inner: &Arc<Inner>) {
    let weak: Weak<Inner> = Arc::downgrade(inner);
    let handle = inner.rt.spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_secs(1));
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            iv.tick().await;
            let Some(inner) = weak.upgrade() else { break };
            inner.tick();
        }
    });
    *inner.ticker.lock() = Some(handle);
}

pub(crate) fn source_for_phone(id: &str, name: &str) -> Source {
    Source {
        id: id.to_owned(),
        kind: SourceKind::Phone,
        name: name.to_owned(),
        connected: true,
        last_bpm: None,
        last_ts: None,
        detail: None,
    }
}
