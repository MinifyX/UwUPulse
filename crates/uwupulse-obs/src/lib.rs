//! UwUPulse inside OBS Studio: the core ([`uwupulse_core::Pulse`]) behind a
//! small C ABI for the plugin in `plugins/obs`. The header is
//! `include/uwupulse_obs.h`; keep both in step.
//!
//! The hub runs on its own tokio runtime (two worker threads). Every
//! function here returns quickly, apart from [`uwupulse_obs_start`] (reads
//! the config, binds the port) and [`uwupulse_obs_stop`] (ends the server
//! and the running session, at most a few seconds). Callbacks come from the
//! runtime's threads, never from the caller's: the plugin moves them to its
//! UI thread itself. No panic crosses the boundary.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::ptr;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde_json::{json, Value};
use tokio::runtime::Runtime;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use uwupulse_core::commands::SaveRequest;
use uwupulse_core::{Event, Host, HostInfo, HostKind, Pulse, PulseOptions, ServerInfo, UiOptions};

#[cfg(test)]
mod tests;

/// The plugin's port on a fresh install; the desktop app has 8746.
pub const DEFAULT_PORT: u16 = 8747;

/// OBS's `blog` levels, which the log callback gets.
pub const LOG_ERROR: c_int = 100;
pub const LOG_WARNING: c_int = 200;
pub const LOG_INFO: c_int = 300;
pub const LOG_DEBUG: c_int = 400;

/// `save_file` answers: a path was written to `out`, the person cancelled,
/// or something failed (no dialog, path too long).
pub const SAVE_PICKED: c_int = 1;
pub const SAVE_CANCELLED: c_int = 0;
pub const SAVE_FAILED: c_int = -1;

/// Room for the path the save dialog returns, NUL included.
const SAVE_PATH_BYTES: usize = 32 * 1024;
/// How often the change watcher looks even without an event (a deleted
/// widget sends none).
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
/// The longest [`uwupulse_obs_stop`] waits for the hub and its threads.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

pub type LogFn = unsafe extern "C" fn(ctx: *mut c_void, level: c_int, msg: *const c_char);
pub type OpenUrlFn = unsafe extern "C" fn(ctx: *mut c_void, url: *const c_char) -> bool;
pub type SaveFileFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    file_name: *const c_char,
    filter_name: *const c_char,
    extension: *const c_char,
    out: *mut c_char,
    out_len: usize,
) -> c_int;
pub type ChangedFn = unsafe extern "C" fn(ctx: *mut c_void);

/// `uwupulse_obs_config` in the header.
#[repr(C)]
pub struct UwupulseObsConfig {
    /// Keys, settings, paired phones, widgets (UTF-8, created if missing).
    pub config_dir: *const c_char,
    /// `pulse.db` (UTF-8, created if missing).
    pub data_dir: *const c_char,
    /// The built desktop frontend for the dock (`index.html`, `assets/`).
    pub ui_dir: *const c_char,
    /// The name phones see; NULL for "UwUPulse OBS (<host name>)".
    pub device_name: *const c_char,
    /// The port of a fresh install; 0 for [`DEFAULT_PORT`].
    pub default_port: u16,
    /// Negative: the port from the settings. 0 or more: this port, not
    /// saved (0 = any free one; tests).
    pub port_override: c_int,
    /// Announce over Bonjour (while LAN access is on).
    pub bonjour: bool,
    /// Chest straps at this computer (see [`PulseOptions::ble`]).
    pub ble: bool,
    /// Handed back to every callback.
    pub ctx: *mut c_void,
    pub log: Option<LogFn>,
    pub open_url: Option<OpenUrlFn>,
    pub save_file: Option<SaveFileFn>,
    /// The dock URL, the port, the token or the widget list changed.
    pub changed: Option<ChangedFn>,
}

/// The callbacks with their context. The plugin promises they may be called
/// from any thread while the hub runs.
#[derive(Clone, Copy)]
struct Callbacks {
    ctx: *mut c_void,
    open_url: Option<OpenUrlFn>,
    save_file: Option<SaveFileFn>,
    changed: Option<ChangedFn>,
}

// SAFETY: the header requires `ctx` and the callbacks to be usable from any
// thread until `uwupulse_obs_stop` returns.
unsafe impl Send for Callbacks {}
unsafe impl Sync for Callbacks {}

impl Callbacks {
    fn changed(&self) {
        if let Some(f) = self.changed {
            // SAFETY: see `impl Send`.
            unsafe { f(self.ctx) }
        }
    }
}

// ── Logging ──────────────────────────────────────────────

#[derive(Clone, Copy)]
struct LogSink {
    f: LogFn,
    ctx: *mut c_void,
}

// SAFETY: as for `Callbacks`.
unsafe impl Send for LogSink {}
unsafe impl Sync for LogSink {}

static LOG_SINK: RwLock<Option<LogSink>> = RwLock::new(None);

/// `log` → OBS's log. Ours at info and up, other crates' (mdns, btleplug,
/// hyper) only warnings and errors.
struct ObsLogger;

impl log::Log for ObsLogger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        let max = if meta.target().starts_with("uwupulse") {
            log::Level::Info
        } else {
            log::Level::Warn
        };
        meta.level() <= max
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // The read lock stays held across the callback, so `uwupulse_obs_stop`
        // (which takes the write lock) waits for a log line in flight
        // instead of returning while the plugin's code still runs.
        // `read_recursive`: a log line from inside the callback can't
        // deadlock against a waiting writer.
        let sink_guard = LOG_SINK.read_recursive();
        let Some(sink) = *sink_guard else {
            return;
        };
        let level = match record.level() {
            log::Level::Error => LOG_ERROR,
            log::Level::Warn => LOG_WARNING,
            log::Level::Info => LOG_INFO,
            log::Level::Debug | log::Level::Trace => LOG_DEBUG,
        };
        let text = format!("[uwupulse] {}", record.args()).replace('\0', " ");
        if let Ok(msg) = CString::new(text) {
            // SAFETY: see `impl Send for LogSink`.
            unsafe { (sink.f)(sink.ctx, level, msg.as_ptr()) }
        }
        drop(sink_guard);
    }

    fn flush(&self) {}
}

static LOGGER: ObsLogger = ObsLogger;

fn set_log_sink(sink: Option<LogSink>) {
    // Fails when a logger is set already (a second start, tests): ours it is.
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    *LOG_SINK.write() = sink;
}

// ── Host ─────────────────────────────────────────────────

/// What the page asks of OBS: the browser and the save dialog.
struct ObsHost(Callbacks);

impl Host for ObsHost {
    fn info(&self) -> HostInfo {
        HostInfo {
            kind: HostKind::Obs,
            updater: false,
        }
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        let f = self.0.open_url.ok_or("not available in OBS")?;
        let url = CString::new(url).map_err(|e| e.to_string())?;
        // SAFETY: see `impl Send for Callbacks`; `url` lives across the call.
        if unsafe { f(self.0.ctx, url.as_ptr()) } {
            Ok(())
        } else {
            Err("OBS could not open it".into())
        }
    }

    fn save_file(&self, request: SaveRequest) -> Result<Option<PathBuf>, String> {
        let f = self.0.save_file.ok_or("Exporting is not available here.")?;
        let c = |s: &str| CString::new(s).map_err(|e| e.to_string());
        let (name, filter, ext) = (
            c(&request.file_name)?,
            c(request.filter_name)?,
            c(request.extension)?,
        );
        let mut out = vec![0u8; SAVE_PATH_BYTES];
        // SAFETY: see `impl Send for Callbacks`; the strings and `out` live
        // across the call and `out_len` is the buffer's length.
        let rc = unsafe {
            f(
                self.0.ctx,
                name.as_ptr(),
                filter.as_ptr(),
                ext.as_ptr(),
                out.as_mut_ptr().cast(),
                out.len(),
            )
        };
        match rc {
            SAVE_CANCELLED => Ok(None),
            SAVE_PICKED => {
                let path = CStr::from_bytes_until_nul(&out)
                    .map_err(|_| "the save dialog returned no path")?
                    .to_str()
                    .map_err(|_| "the path is not UTF-8")?;
                if path.is_empty() {
                    return Err("the save dialog returned no path".into());
                }
                Ok(Some(PathBuf::from(path)))
            }
            _ => Err("The save dialog failed.".into()),
        }
    }
}

// ── The handle ───────────────────────────────────────────

/// `uwupulse_obs` in the header: a running hub.
pub struct UwupulseObs {
    rt: Option<Runtime>,
    pulse: Pulse,
    watcher: Option<JoinHandle<()>>,
}

impl UwupulseObs {
    fn start(cfg: &UwupulseObsConfig) -> Result<Self, String> {
        let config_dir = path_arg(cfg.config_dir, "config_dir")?;
        let data_dir = path_arg(cfg.data_dir, "data_dir")?;
        let ui_dir = path_arg(cfg.ui_dir, "ui_dir")?;
        let device_name = opt_str_arg(cfg.device_name, "device_name")?
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("UwUPulse OBS ({})", uwupulse_core::host_name()));
        let callbacks = Callbacks {
            ctx: cfg.ctx,
            open_url: cfg.open_url,
            save_file: cfg.save_file,
            changed: cfg.changed,
        };

        let mut opts = PulseOptions::new(config_dir, data_dir);
        opts.default_port = if cfg.default_port == 0 {
            DEFAULT_PORT
        } else {
            cfg.default_port
        };
        opts.port_override = u16::try_from(cfg.port_override).ok();
        opts.bonjour = cfg.bonjour;
        opts.ble = cfg.ble;
        opts.device_name = Some(device_name);
        opts.ui = Some(UiOptions {
            dir: ui_dir,
            host: Arc::new(ObsHost(callbacks)),
        });

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("uwupulse")
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        let pulse = rt.block_on(Pulse::start(opts)).map_err(|e| e.to_string())?;
        let info = pulse.server_info();
        match &info.error {
            // Not fatal: the dock shows it and the port can be changed there.
            Some(e) => log::warn!("server on port {}: {e}", info.port),
            None => log::info!(
                "listening on port {} as \"{}\"",
                info.port,
                pulse.device_name()
            ),
        }
        let watcher = rt.spawn(watch_changes(pulse.clone(), callbacks));
        Ok(Self {
            rt: Some(rt),
            pulse,
            watcher: Some(watcher),
        })
    }

    fn stop(&mut self) {
        if let Some(w) = self.watcher.take() {
            w.abort();
        }
        let Some(rt) = self.rt.take() else {
            return;
        };
        let pulse = self.pulse.clone();
        let done = rt.block_on(async move {
            tokio::time::timeout(STOP_TIMEOUT, pulse.shutdown())
                .await
                .is_ok()
        });
        if !done {
            log::warn!("shutdown took too long; leaving the rest behind");
        }
        // A save dialog still open would hold a blocking thread: don't wait
        // for it forever.
        rt.shutdown_timeout(STOP_TIMEOUT);
        log::info!("stopped");
    }
}

impl Drop for UwupulseObs {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Calls `changed` whenever [`snapshot`] differs: on hub events, and every
/// [`WATCH_INTERVAL`] for what sends none.
async fn watch_changes(pulse: Pulse, callbacks: Callbacks) {
    let mut events = pulse.subscribe();
    let mut tick = tokio::time::interval(WATCH_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last = snapshot(&pulse);
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                // Heart rates change nothing the plugin shows itself.
                Ok(Event::Hr(_)) => continue,
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            },
            _ = tick.tick() => {}
        }
        let now = snapshot(&pulse);
        if now != last {
            last = now;
            callbacks.changed();
        }
    }
}

/// Everything a change of which the plugin must hear about.
fn snapshot(pulse: &Pulse) -> (Option<String>, String) {
    (pulse.ui_url(), widgets_json(pulse))
}

/// `{"port", "error", "widgets": [{"id", "name", "url"}]}`.
fn widgets_json(pulse: &Pulse) -> String {
    let info = pulse.server_info();
    let widgets: Vec<Value> = pulse
        .list_widgets()
        .iter()
        .filter_map(|w| {
            let id = w.get("id")?.as_str()?;
            let name = w
                .get("name")
                .and_then(Value::as_str)
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(id);
            Some(json!({ "id": id, "name": name, "url": overlay_url(&info, id)? }))
        })
        .collect();
    json!({ "port": info.port, "error": info.error, "widgets": widgets }).to_string()
}

/// A widget's overlay on loopback, with the token. `None` for an ID that
/// can't be one (it goes into the path unescaped).
fn overlay_url(info: &ServerInfo, id: &str) -> Option<String> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    // The token is base64url without padding: nothing to escape.
    valid.then(|| {
        format!(
            "http://127.0.0.1:{}/overlay/{id}?token={}",
            info.port, info.token
        )
    })
}

// ── C ABI ────────────────────────────────────────────────

/// Runs `f`, turning a panic into `fallback`.
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        log::error!("internal error (panic) in the plugin's core");
        fallback
    })
}

fn str_arg(p: *const c_char, what: &str) -> Result<String, String> {
    opt_str_arg(p, what)?.ok_or_else(|| format!("{what} is missing"))
}

fn opt_str_arg(p: *const c_char, what: &str) -> Result<Option<String>, String> {
    if p.is_null() {
        return Ok(None);
    }
    // SAFETY: the header requires NUL-terminated strings.
    let s = unsafe { CStr::from_ptr(p) };
    s.to_str()
        .map(|s| Some(s.to_owned()))
        .map_err(|_| format!("{what} is not UTF-8"))
}

fn path_arg(p: *const c_char, what: &str) -> Result<PathBuf, String> {
    let s = str_arg(p, what)?;
    if s.is_empty() {
        return Err(format!("{what} is empty"));
    }
    Ok(PathBuf::from(s))
}

fn into_c(s: String) -> *mut c_char {
    CString::new(s).map_or(ptr::null_mut(), CString::into_raw)
}

/// Starts the hub. NULL on failure (the reason goes to the log callback).
///
/// # Safety
/// `cfg` is NULL or points to a valid config whose strings are
/// NUL-terminated (only read during the call). `ctx` and the callbacks
/// must stay usable, from any thread, until [`uwupulse_obs_stop`] returns.
#[no_mangle]
pub unsafe extern "C" fn uwupulse_obs_start(cfg: *const UwupulseObsConfig) -> *mut UwupulseObs {
    guard(ptr::null_mut(), || {
        // SAFETY: the caller's promise.
        let Some(cfg) = (unsafe { cfg.as_ref() }) else {
            return ptr::null_mut();
        };
        set_log_sink(cfg.log.map(|f| LogSink { f, ctx: cfg.ctx }));
        match UwupulseObs::start(cfg) {
            Ok(h) => Box::into_raw(Box::new(h)),
            Err(e) => {
                log::error!("could not start: {e}");
                ptr::null_mut()
            }
        }
    })
}

/// Stops the hub and frees the handle; NULL is fine. No callback runs
/// after it returns, apart from a save dialog that is still open (the
/// plugin cancels those first) and a log line in flight, which it waits for.
///
/// # Safety
/// `h` is NULL or a handle from [`uwupulse_obs_start`] not stopped yet.
#[no_mangle]
pub unsafe extern "C" fn uwupulse_obs_stop(h: *mut UwupulseObs) {
    if h.is_null() {
        return;
    }
    guard((), || {
        // SAFETY: the caller's promise; the handle came from `Box::into_raw`.
        drop(unsafe { Box::from_raw(h) });
    });
    *LOG_SINK.write() = None;
}

/// The dock's address, `http://127.0.0.1:<port>/ui/#s=<secret>`. Free with
/// [`uwupulse_obs_free_string`]; NULL for a NULL handle.
///
/// # Safety
/// `h` is NULL or a running handle.
#[no_mangle]
pub unsafe extern "C" fn uwupulse_obs_ui_url(h: *const UwupulseObs) -> *mut c_char {
    guard(ptr::null_mut(), || {
        // SAFETY: the caller's promise.
        let Some(h) = (unsafe { h.as_ref() }) else {
            return ptr::null_mut();
        };
        h.pulse.ui_url().map_or(ptr::null_mut(), into_c)
    })
}

/// The dock's address for the system browser,
/// `http://127.0.0.1:<port>/ui/#t=<ticket>`: a one-time ticket (30 s) the
/// page trades for the secret, so the secret never shows up in the
/// browser's command line or history. Free with [`uwupulse_obs_free_string`];
/// NULL for a NULL handle.
///
/// # Safety
/// `h` is NULL or a running handle.
#[no_mangle]
pub unsafe extern "C" fn uwupulse_obs_ui_ticket_url(h: *const UwupulseObs) -> *mut c_char {
    guard(ptr::null_mut(), || {
        // SAFETY: the caller's promise.
        let Some(h) = (unsafe { h.as_ref() }) else {
            return ptr::null_mut();
        };
        h.pulse.ui_ticket_url().map_or(ptr::null_mut(), into_c)
    })
}

/// The widgets as JSON: `{"port": 8747, "error": null, "widgets": [{"id",
/// "name", "url"}]}`, `url` being the overlay with the token. Free with
/// [`uwupulse_obs_free_string`].
///
/// # Safety
/// `h` is NULL or a running handle.
#[no_mangle]
pub unsafe extern "C" fn uwupulse_obs_widgets(h: *const UwupulseObs) -> *mut c_char {
    guard(ptr::null_mut(), || {
        // SAFETY: the caller's promise.
        let Some(h) = (unsafe { h.as_ref() }) else {
            return ptr::null_mut();
        };
        into_c(widgets_json(&h.pulse))
    })
}

/// The overlay address of widget `id` with the current port and token, also
/// for a widget that doesn't exist (any more). NULL for an ID that can't be
/// one. Free with [`uwupulse_obs_free_string`].
///
/// # Safety
/// `h` is NULL or a running handle; `id` is NULL or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn uwupulse_obs_overlay_url(
    h: *const UwupulseObs,
    id: *const c_char,
) -> *mut c_char {
    guard(ptr::null_mut(), || {
        // SAFETY: the caller's promise.
        let Some(h) = (unsafe { h.as_ref() }) else {
            return ptr::null_mut();
        };
        let Ok(id) = str_arg(id, "id") else {
            return ptr::null_mut();
        };
        overlay_url(&h.pulse.server_info(), &id).map_or(ptr::null_mut(), into_c)
    })
}

/// Frees a string from this library; NULL is fine.
///
/// # Safety
/// `s` is NULL or a string returned here, not freed yet.
#[no_mangle]
pub unsafe extern "C" fn uwupulse_obs_free_string(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: the caller's promise; it came from `CString::into_raw`.
        drop(unsafe { CString::from_raw(s) });
    }
}

/// The version, e.g. "1.0.0" (static, don't free).
#[no_mangle]
pub extern "C" fn uwupulse_obs_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}
