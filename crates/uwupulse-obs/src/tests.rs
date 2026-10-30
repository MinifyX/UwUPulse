//! The C ABI as the plugin uses it: start on 127.0.0.1:0 in a temp folder,
//! read the URLs and widgets, get the callbacks, stop.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

use super::*;

/// The log sink is process-wide: one test at a time owns it.
static SERIAL: Mutex<()> = Mutex::new(());

const WAIT: Duration = Duration::from_secs(5);

/// What the callbacks saw.
#[derive(Default)]
struct Seen {
    logs: Mutex<Vec<(c_int, String)>>,
    changed: AtomicUsize,
    opened: Mutex<Vec<String>>,
    save_requests: Mutex<Vec<String>>,
    save_to: Mutex<Option<String>>,
}

fn seen(ctx: *mut c_void) -> &'static Seen {
    // SAFETY: every test passes a leaked `Seen`.
    unsafe { &*(ctx as *const Seen) }
}

unsafe extern "C" fn on_log(ctx: *mut c_void, level: c_int, msg: *const c_char) {
    let msg = unsafe { CStr::from_ptr(msg) }
        .to_string_lossy()
        .into_owned();
    seen(ctx).logs.lock().push((level, msg));
}

unsafe extern "C" fn on_changed(ctx: *mut c_void) {
    seen(ctx).changed.fetch_add(1, Ordering::SeqCst);
}

unsafe extern "C" fn on_open_url(ctx: *mut c_void, url: *const c_char) -> bool {
    let url = unsafe { CStr::from_ptr(url) }
        .to_string_lossy()
        .into_owned();
    seen(ctx).opened.lock().push(url);
    true
}

unsafe extern "C" fn on_save_file(
    ctx: *mut c_void,
    file_name: *const c_char,
    _filter_name: *const c_char,
    _extension: *const c_char,
    out: *mut c_char,
    out_len: usize,
) -> c_int {
    let s = seen(ctx);
    let name = unsafe { CStr::from_ptr(file_name) }.to_string_lossy();
    s.save_requests.lock().push(name.into_owned());
    let Some(path) = s.save_to.lock().clone() else {
        return SAVE_CANCELLED;
    };
    let bytes = path.as_bytes();
    if bytes.len() + 1 > out_len {
        return SAVE_FAILED;
    }
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len());
        *out.add(bytes.len()) = 0;
    }
    SAVE_PICKED
}

struct Started {
    h: *mut UwupulseObs,
    seen: &'static Seen,
    dir: tempfile::TempDir,
}

impl Started {
    fn port(&self) -> u16 {
        unsafe { &*self.h }.pulse.server_info().port
    }

    fn pulse(&self) -> &Pulse {
        &unsafe { &*self.h }.pulse
    }
}

fn start() -> Started {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    // Loopback only: tests must not open ports on the LAN.
    std::fs::write(config.join("settings.json"), r#"{"lanEnabled":false}"#).unwrap();
    let ui = dir.path().join("ui");
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(
        ui.join("index.html"),
        "<!doctype html><title>UwUPulse</title>",
    )
    .unwrap();
    let seen: &'static Seen = Box::leak(Box::default());
    let c = |p: &Path| CString::new(p.to_str().unwrap()).unwrap();
    let (config, data, ui) = (c(&config), c(&dir.path().join("data")), c(&ui));
    let cfg = UwupulseObsConfig {
        config_dir: config.as_ptr(),
        data_dir: data.as_ptr(),
        ui_dir: ui.as_ptr(),
        device_name: ptr::null(),
        default_port: 0,
        port_override: 0,
        bonjour: false,
        ble: false,
        ctx: seen as *const Seen as *mut c_void,
        log: Some(on_log),
        open_url: Some(on_open_url),
        save_file: Some(on_save_file),
        changed: Some(on_changed),
    };
    let h = unsafe { uwupulse_obs_start(&cfg) };
    assert!(!h.is_null(), "start failed: {:?}", seen.logs.lock());
    Started { h, seen, dir }
}

fn take(s: *mut c_char) -> String {
    assert!(!s.is_null());
    let out = unsafe { CStr::from_ptr(s) }.to_str().unwrap().to_owned();
    unsafe { uwupulse_obs_free_string(s) };
    out
}

/// A plain HTTP/1.1 request; the status code and the body.
fn http(port: u16, method: &str, path: &str, headers: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(WAIT)).unwrap();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n{headers}\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out[9..12].parse().unwrap();
    let body = out.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    (status, body.to_owned())
}

fn wait_until(what: &str, f: impl Fn() -> bool) {
    let until = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn null_arguments_are_harmless() {
    let _serial = SERIAL.lock();
    unsafe {
        assert!(uwupulse_obs_start(ptr::null()).is_null());
        uwupulse_obs_stop(ptr::null_mut());
        assert!(uwupulse_obs_ui_url(ptr::null()).is_null());
        assert!(uwupulse_obs_widgets(ptr::null()).is_null());
        assert!(uwupulse_obs_overlay_url(ptr::null(), ptr::null()).is_null());
        uwupulse_obs_free_string(ptr::null_mut());
    }
    let v = unsafe { CStr::from_ptr(uwupulse_obs_version()) };
    assert_eq!(v.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn missing_dirs_fail_with_a_log_line() {
    let _serial = SERIAL.lock();
    let seen: &'static Seen = Box::leak(Box::default());
    let cfg = UwupulseObsConfig {
        config_dir: ptr::null(),
        data_dir: ptr::null(),
        ui_dir: ptr::null(),
        device_name: ptr::null(),
        default_port: 0,
        port_override: 0,
        bonjour: false,
        ble: false,
        ctx: seen as *const Seen as *mut c_void,
        log: Some(on_log),
        open_url: None,
        save_file: None,
        changed: None,
    };
    assert!(unsafe { uwupulse_obs_start(&cfg) }.is_null());
    let logs = seen.logs.lock();
    assert!(
        logs.iter()
            .any(|(l, m)| *l == LOG_ERROR && m.contains("config_dir is missing")),
        "{logs:?}"
    );
}

#[test]
fn serves_the_dock_and_lists_widgets() {
    let _serial = SERIAL.lock();
    let s = start();
    let port = s.port();
    assert_ne!(port, 0);
    assert!(s.pulse().device_name().starts_with("UwUPulse OBS ("));
    assert!(s
        .seen
        .logs
        .lock()
        .iter()
        .any(|(l, m)| *l == LOG_INFO && m.starts_with("[uwupulse] listening on port")));

    let ui = take(unsafe { uwupulse_obs_ui_url(s.h) });
    let prefix = format!("http://127.0.0.1:{port}/ui/#s=");
    assert!(ui.starts_with(&prefix), "{ui}");
    let (status, body) = http(port, "GET", "/ui/", "", "");
    assert_eq!(status, 200);
    assert!(body.contains("<title>UwUPulse</title>"));

    // The page's host is OBS, without an updater.
    let secret = &ui[prefix.len()..];
    let (status, body) = http(
        port,
        "POST",
        "/ui/rpc/get_host_info",
        &format!("X-UwUPulse-UI: {secret}\r\nContent-Type: application/json\r\n"),
        "null",
    );
    assert_eq!(status, 200, "{body}");
    let info: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info, serde_json::json!({ "kind": "obs", "updater": false }));

    let token = s.pulse().server_info().token;
    let list: Value = serde_json::from_str(&take(unsafe { uwupulse_obs_widgets(s.h) })).unwrap();
    assert_eq!(list["port"], port);
    assert_eq!(list["widgets"][0]["id"], "classic");
    assert_eq!(list["widgets"][0]["name"], "Classic");
    let url = format!("http://127.0.0.1:{port}/overlay/classic?token={token}");
    assert_eq!(list["widgets"][0]["url"], url.as_str());
    let id = CString::new("classic").unwrap();
    assert_eq!(
        take(unsafe { uwupulse_obs_overlay_url(s.h, id.as_ptr()) }),
        url
    );
    let bad = CString::new("../x").unwrap();
    assert!(unsafe { uwupulse_obs_overlay_url(s.h, bad.as_ptr()) }.is_null());
    let (status, _) = http(port, "GET", &url[url.find("/overlay").unwrap()..], "", "");
    assert_eq!(status, 200);

    // Bluetooth is off when the plugin says so.
    let (status, body) = http(
        port,
        "POST",
        "/ui/rpc/ble_set_scanning",
        &format!("X-UwUPulse-UI: {secret}\r\nContent-Type: application/json\r\n"),
        r#"{"on":true}"#,
    );
    assert!(status >= 400, "{status} {body}");

    unsafe { uwupulse_obs_stop(s.h) };
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    drop(s.dir);
}

#[test]
fn reports_changes_and_answers_the_page() {
    let _serial = SERIAL.lock();
    let s = start();
    let before = s.seen.changed.load(Ordering::SeqCst);

    // A new token changes every overlay URL.
    s.pulse().regenerate_token().unwrap();
    wait_until("changed after a new token", || {
        s.seen.changed.load(Ordering::SeqCst) > before
    });
    let token = s.pulse().server_info().token;
    let list = take(unsafe { uwupulse_obs_widgets(s.h) });
    assert!(list.contains(&token));

    // So does a new widget.
    let before = s.seen.changed.load(Ordering::SeqCst);
    s.pulse()
        .save_widget(serde_json::json!({ "id": "big", "name": "Big" }))
        .unwrap();
    wait_until("changed after a new widget", || {
        s.seen.changed.load(Ordering::SeqCst) > before
    });

    // The page's requests reach the callbacks.
    let host = ObsHost(Callbacks {
        ctx: s.seen as *const Seen as *mut c_void,
        open_url: Some(on_open_url),
        save_file: Some(on_save_file),
        changed: None,
    });
    host.open_url("https://github.com/MinifyX/UwUPulse")
        .unwrap();
    assert_eq!(
        s.seen.opened.lock().as_slice(),
        ["https://github.com/MinifyX/UwUPulse"]
    );
    let req = || SaveRequest {
        file_name: "uwupulse-2026-10-30-1405.csv".into(),
        filter_name: "CSV",
        extension: "csv",
    };
    assert_eq!(host.save_file(req()).unwrap(), None);
    let target = s.dir.path().join("export.csv");
    *s.seen.save_to.lock() = Some(target.to_str().unwrap().to_owned());
    assert_eq!(host.save_file(req()).unwrap(), Some(target));
    assert_eq!(
        s.seen.save_requests.lock()[0],
        "uwupulse-2026-10-30-1405.csv"
    );

    unsafe { uwupulse_obs_stop(s.h) };
}
