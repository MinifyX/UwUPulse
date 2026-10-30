//! The UwUPulse hub: heart rates come in from phones (over `/link`) and
//! chest straps (BLE), go out to the app (events), to overlays and bots (the
//! local API) and into recorded sessions.
//!
//! [`Pulse`] is the one handle to all of it. It is cheap to clone; every
//! clone talks to the same hub. Start it inside a tokio runtime; it keeps a
//! handle to that runtime, so its sync methods also work from threads
//! without one (Tauri's sync commands).

mod config;
mod hub;
mod mdns;
mod net;
mod recorder;
mod server;
pub mod types;
mod widgets;

#[cfg(feature = "ble")]
mod ble;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use qrcode::render::svg;
use qrcode::QrCode;
use serde_json::Value;
use tokio::sync::{broadcast, watch, Semaphore};
use uwupulse_proto::{b64_encode, limits, random_bytes, DeskMsg, PairingUri, WatchAction};

pub use crate::types::*;
pub use crate::widgets::default_widget;
pub use uwupulse_proto as proto;

use crate::config::{check_settings, Identity, PairedRecord, Store};
use crate::hub::{Inner, LinkCmd, LiveState, Offer, PairingState, ServerStatus};
use crate::net::{desktop_name, lan_ips, now_ms, pairing_hosts};
use crate::recorder::Recorder;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("the device is not connected")]
    NotConnected,
    #[error("Bluetooth: {0}")]
    Ble(String),
    #[error("{0}")]
    Server(String),
    #[error("not available in this build")]
    Unavailable,
}

/// Where the hub keeps its files and how it runs.
#[derive(Debug, Clone)]
pub struct PulseOptions {
    /// Keys, identity, settings, paired phones, widgets.
    pub config_dir: PathBuf,
    /// `pulse.db`.
    pub data_dir: PathBuf,
    /// Overrides the port from the settings; tests pass `Some(0)`.
    pub port_override: Option<u16>,
    /// Announce the desktop over Bonjour (only while LAN access is on).
    pub bonjour: bool,
    /// The name phones see; the host name when `None`.
    pub device_name: Option<String>,
    /// Sent to API clients in `hello`.
    pub app_version: String,
}

impl PulseOptions {
    pub fn new(config_dir: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
            data_dir: data_dir.into(),
            port_override: None,
            bonjour: true,
            device_name: None,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct Pulse(Arc<Inner>);

impl Pulse {
    /// Loads (or creates) everything, binds the server and starts the
    /// once-a-second ticker. A port that can't be bound is not an error here:
    /// the app should still open and show it in [`ServerInfo::error`].
    pub async fn start(opts: PulseOptions) -> Result<Self, Error> {
        let store = Store::new(&opts.config_dir)?;
        let keypair = store.link_key()?;
        let identity = match store.read::<Identity>("identity.json") {
            Some(i) if limits::valid_device_id(&i.id) && !i.token.is_empty() => i,
            _ => {
                let i = Identity {
                    id: uuid::Uuid::new_v4().to_string(),
                    token: new_token(),
                };
                store.write("identity.json", &i, true)?;
                i
            }
        };
        let settings = store
            .read::<Settings>("settings.json")
            .filter(|s| check_settings(s).is_ok())
            .unwrap_or_default();
        let paired: Vec<PairedRecord> = store.read("paired.json").unwrap_or_default();
        let widgets: Vec<Value> = match store.read::<Vec<Value>>("widgets.json") {
            Some(w) => w,
            None => {
                let w = vec![default_widget()];
                store.write("widgets.json", &w, false)?;
                w
            }
        };
        std::fs::create_dir_all(&opts.data_dir)?;
        let recorder = Recorder::open(&opts.data_dir.join("pulse.db"))?;
        let desk_name = opts
            .device_name
            .as_deref()
            .map(|n| limits::truncate_chars(n.trim(), limits::MAX_NAME_CHARS).to_owned())
            .filter(|n| limits::valid_name(n))
            .unwrap_or_else(desktop_name);
        let (events, _) = broadcast::channel(256);
        let inner = Arc::new(Inner {
            rt: tokio::runtime::Handle::current(),
            store,
            app_version: opts.app_version,
            desk_id: identity.id,
            desk_name,
            keypair,
            token: Mutex::new(identity.token),
            settings: Mutex::new(settings),
            paired: Mutex::new(paired),
            widgets: Mutex::new(widgets),
            events,
            live: Mutex::new(LiveState {
                current: None,
                last_at: None,
                live: false,
                sources: Default::default(),
                dirty: false,
            }),
            recorder,
            server: tokio::sync::Mutex::new(None),
            server_status: Mutex::new(ServerStatus {
                port: opts.port_override.unwrap_or(settings.port),
                lan: settings.lan_enabled,
                error: None,
            }),
            generation: watch::channel(0).0,
            token_generation: watch::channel(0).0,
            port_override: opts.port_override,
            pairing: Mutex::new(PairingState::default()),
            links: Mutex::new(Default::default()),
            next_conn: AtomicU64::new(1),
            link_slots: Arc::new(Semaphore::new(limits::MAX_LINK_CONNECTIONS)),
            api_slots: Arc::new(Semaphore::new(hub::MAX_API_SOCKETS)),
            link_attempts: Mutex::new(Default::default()),
            pre_auth: Mutex::new(Default::default()),
            bonjour: opts.bonjour,
            advert: Mutex::new(None),
            ticker: Mutex::new(None),
            #[cfg(feature = "ble")]
            ble: ble::Ble::default(),
        });
        server::start(&inner).await;
        inner.advertise();
        hub::spawn_ticker(&inner);
        Ok(Self(inner))
    }

    /// Every hub event from now on. A slow receiver misses events
    /// (`RecvError::Lagged`) rather than holding the hub up.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.0.events.subscribe()
    }

    pub fn state(&self) -> AppState {
        self.0.app_state()
    }

    pub fn server_info(&self) -> ServerInfo {
        self.0.server_info()
    }

    /// The desktop's ID and static public key (b64), as phones see them.
    pub fn identity(&self) -> (String, String) {
        (self.0.desk_id.clone(), self.0.keypair.public_b64())
    }

    pub fn device_name(&self) -> &str {
        &self.0.desk_name
    }

    // --- Pairing -----------------------------------------------------------

    pub fn list_paired(&self) -> Vec<PairedDevice> {
        self.0.paired_devices()
    }

    /// Forgets a phone and drops its connection.
    pub fn remove_paired(&self, id: &str) -> Result<(), Error> {
        let removed = {
            let mut p = self.0.paired.lock();
            let before = p.len();
            p.retain(|d| d.id != id);
            p.len() != before
        };
        if !removed {
            return Err(Error::NotFound);
        }
        self.0.save_paired();
        for l in self.0.links.lock().values().filter(|l| l.device_id == id) {
            let _ = l.tx.try_send(LinkCmd::Close);
        }
        self.0.emit_paired();
        Ok(())
    }

    /// A fresh QR offer (5 minutes, single use); replaces an older one. The
    /// URI lists the private LAN addresses (the phone ignores anything else)
    /// — none while LAN access is off, so the app should ask to turn it on
    /// first.
    pub fn create_pairing(&self) -> Result<PairingOffer, Error> {
        let secret = random_bytes::<32>();
        let info = self.0.server_info();
        let hosts = if info.lan_enabled {
            pairing_hosts(lan_ips())
        } else {
            Vec::new()
        };
        let uri = PairingUri {
            id: self.0.desk_id.clone(),
            name: self.0.desk_name.clone(),
            pk: self.0.keypair.public_bytes(),
            secret,
            port: info.port,
            hosts,
        }
        .to_uri();
        let qr_svg = QrCode::new(uri.as_bytes())
            .map_err(|e| Error::Invalid(format!("QR code: {e}")))?
            .render::<svg::Color>()
            .min_dimensions(256, 256)
            .quiet_zone(true)
            .build();
        self.0.pairing.lock().offer = Some(Offer {
            secret,
            expires: Instant::now() + limits::OFFER_TTL,
        });
        Ok(PairingOffer {
            uri,
            qr_svg,
            expires_at: now_ms() + limits::OFFER_TTL.as_millis() as i64,
        })
    }

    pub fn cancel_pairing(&self) {
        self.0.pairing.lock().offer = None;
    }

    /// The user's answer to a code comparison (`PairRequest`).
    pub fn answer_pair_request(&self, request_id: &str, accept: bool) -> Result<(), Error> {
        let mut p = self.0.pairing.lock();
        let slot = p
            .sas
            .as_mut()
            .filter(|s| s.request_id == request_id)
            .ok_or(Error::NotFound)?;
        let tx = slot.answer.take().ok_or(Error::NotFound)?;
        let _ = tx.send(accept);
        Ok(())
    }

    /// Starts or stops the workout on the watch behind a connected phone.
    pub async fn send_watch_command(
        &self,
        device_id: &str,
        action: WatchAction,
    ) -> Result<(), Error> {
        let tx = self
            .0
            .links
            .lock()
            .values()
            .find(|l| l.device_id == device_id)
            .map(|l| l.tx.clone())
            .ok_or(Error::NotConnected)?;
        tx.send(LinkCmd::Send(DeskMsg::Cmd { action }))
            .await
            .map_err(|_| Error::NotConnected)
    }

    // --- BLE ---------------------------------------------------------------

    pub async fn ble_set_scanning(&self, on: bool) -> Result<(), Error> {
        #[cfg(feature = "ble")]
        return ble::set_scanning(&self.0, on).await;
        #[cfg(not(feature = "ble"))]
        {
            let _ = on;
            Err(Error::Unavailable)
        }
    }

    pub fn ble_list(&self) -> Vec<BleDevice> {
        #[cfg(feature = "ble")]
        return self.0.ble.list();
        #[cfg(not(feature = "ble"))]
        Vec::new()
    }

    pub async fn ble_connect(&self, id: &str) -> Result<(), Error> {
        #[cfg(feature = "ble")]
        return ble::connect(&self.0, id);
        #[cfg(not(feature = "ble"))]
        {
            let _ = id;
            Err(Error::Unavailable)
        }
    }

    pub async fn ble_disconnect(&self, id: &str) -> Result<(), Error> {
        #[cfg(feature = "ble")]
        return ble::disconnect(&self.0, id).await;
        #[cfg(not(feature = "ble"))]
        {
            let _ = id;
            Err(Error::Unavailable)
        }
    }

    // --- Widgets -----------------------------------------------------------

    pub fn list_widgets(&self) -> Vec<Value> {
        self.0.widgets.lock().clone()
    }

    /// Creates (no or empty `id`) or replaces a widget; open overlays get it
    /// live.
    pub fn save_widget(&self, widget: Value) -> Result<Value, Error> {
        let widget = widgets::normalize(widget).map_err(Error::Invalid)?;
        let id = widgets::id_of(&widget).to_owned();
        let list = {
            let mut w = self.0.widgets.lock();
            match w.iter().position(|x| widgets::id_of(x) == id) {
                Some(i) => w[i] = widget.clone(),
                None if w.len() >= widgets::MAX_WIDGETS => {
                    return Err(Error::Invalid("too many widgets".into()))
                }
                None => w.push(widget.clone()),
            }
            w.clone()
        };
        self.0.store.write("widgets.json", &list, false)?;
        self.0.emit(Event::Widget {
            id,
            config: widget.clone(),
        });
        Ok(widget)
    }

    pub fn delete_widget(&self, id: &str) -> Result<(), Error> {
        let list = {
            let mut w = self.0.widgets.lock();
            let before = w.len();
            w.retain(|x| widgets::id_of(x) != id);
            if w.len() == before {
                return Err(Error::NotFound);
            }
            w.clone()
        };
        self.0.store.write("widgets.json", &list, false)?;
        Ok(())
    }

    // --- Sessions ----------------------------------------------------------

    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>, Error> {
        self.0.recorder.list()
    }

    pub fn get_session(&self, id: &str) -> Result<SessionDetail, Error> {
        self.0.recorder.get(id)
    }

    pub fn delete_session(&self, id: &str) -> Result<(), Error> {
        self.0.recorder.delete(id)?;
        self.0.emit(Event::Sessions);
        self.0.emit_state();
        Ok(())
    }

    pub fn export_session(&self, id: &str, format: ExportFormat) -> Result<Vec<u8>, Error> {
        self.0.recorder.export(id, format)
    }

    pub fn export_session_to(
        &self,
        id: &str,
        format: ExportFormat,
        path: &Path,
    ) -> Result<(), Error> {
        let bytes = self.export_session(id, format)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    // --- Settings ----------------------------------------------------------

    pub fn settings(&self) -> Settings {
        *self.0.settings.lock()
    }

    /// Saves the settings. A new port or a LAN toggle restarts the server;
    /// if the new address can't be bound, the old settings come back and
    /// the error says why.
    pub async fn set_settings(&self, new: Settings) -> Result<Settings, Error> {
        check_settings(&new).map_err(Error::Invalid)?;
        let old = std::mem::replace(&mut *self.0.settings.lock(), new);
        let rebind = old.lan_enabled != new.lan_enabled
            || (self.0.port_override.is_none() && old.port != new.port)
            // Retry a server that couldn't start (port was taken).
            || self.0.server_status.lock().error.is_some();
        if rebind {
            self.restart_server().await;
            let failed = self.0.server_status.lock().error.clone();
            if let Some(err) = failed {
                *self.0.settings.lock() = old;
                self.restart_server().await;
                return Err(Error::Server(err));
            }
        }
        self.0.store.write("settings.json", &new, false)?;
        if !new.record_sessions && old.record_sessions && self.0.recorder.end()? {
            self.0.emit(Event::Sessions);
        }
        self.0.emit_state();
        Ok(new)
    }

    async fn restart_server(&self) {
        server::stop(&self.0).await;
        server::start(&self.0).await;
        self.0.advertise();
    }

    /// A new API token; overlays and bots with the old one are cut off.
    pub fn regenerate_token(&self) -> Result<ServerInfo, Error> {
        let token = new_token();
        self.0.store.write(
            "identity.json",
            &Identity {
                id: self.0.desk_id.clone(),
                token: token.clone(),
            },
            true,
        )?;
        *self.0.token.lock() = token;
        self.0.token_generation.send_modify(|g| *g += 1);
        self.0.emit_state();
        Ok(self.0.server_info())
    }

    // --- Misc ----------------------------------------------------------------

    /// Feeds a sample in as if it came from `source_id` (BLE uses this; so
    /// do tests and demos).
    pub fn push_sample(&self, source_id: &str, sample: HrSample) {
        self.0.ingest(source_id, sample);
    }

    /// Writes buffered samples now.
    pub fn flush(&self) -> Result<(), Error> {
        self.0.recorder.flush()
    }

    /// Stops the server, ends the running session and lets go of Bluetooth.
    pub async fn shutdown(&self) {
        if let Some(t) = self.0.ticker.lock().take() {
            t.abort();
        }
        *self.0.advert.lock() = None;
        server::stop(&self.0).await;
        #[cfg(feature = "ble")]
        ble::shutdown(&self.0).await;
        if let Err(e) = self.0.recorder.end() {
            log::error!("ending the session failed: {e}");
        }
    }
}

fn new_token() -> String {
    b64_encode(&random_bytes::<32>())
}
