//! `/ui`: the page, its commands and events for a host without Tauri.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use uwupulse_core::{Host, HostInfo, HostKind, HrSample, PulseOptions, Src, UiOptions};

use tempfile::TempDir;

use crate::{
    http, http_body, next_msg, refused_status, start, start_with, ws_connect_with, Env, Ws,
};

struct ObsHost;

impl Host for ObsHost {
    fn info(&self) -> HostInfo {
        HostInfo {
            kind: HostKind::Obs,
            updater: false,
        }
    }

    fn open_url(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }
}

struct Ui {
    env: Env,
    secret: String,
    _files: TempDir,
}

impl std::ops::Deref for Ui {
    type Target = Env;
    fn deref(&self) -> &Env {
        &self.env
    }
}

/// A hub with `/ui` on, serving `<tmp>/ui`.
async fn start_ui() -> Ui {
    let files = tempfile::tempdir().unwrap();
    let ui_dir = files.path().join("ui");
    std::fs::create_dir_all(ui_dir.join("assets")).unwrap();
    std::fs::write(
        ui_dir.join("index.html"),
        "<!doctype html><title>UwUPulse</title>",
    )
    .unwrap();
    std::fs::write(ui_dir.join("assets/app.js"), "console.log(1)").unwrap();
    // Next to the page's folder, never reachable through it.
    std::fs::write(files.path().join("ui.secret"), "nope").unwrap();
    let env = start_with(|o: &mut PulseOptions| {
        o.ui = Some(UiOptions {
            dir: ui_dir.clone(),
            host: Arc::new(ObsHost),
        })
    })
    .await;
    let url = env.pulse.ui_url().unwrap();
    let prefix = format!("http://127.0.0.1:{}/ui/#s=", env.port);
    let secret = url.strip_prefix(&prefix).expect(&url).to_owned();
    assert_eq!(secret.len(), 64);
    Ui {
        env,
        secret,
        _files: files,
    }
}

async fn rpc(env: &Env, secret: &str, command: &str, args: Value) -> (u16, Value) {
    let body = args.to_string();
    let r = http_body(
        env.port,
        "POST",
        &format!("/ui/rpc/{command}"),
        &[
            ("X-UwUPulse-UI", secret),
            ("Content-Type", "application/json"),
        ],
        body.as_bytes(),
    )
    .await;
    (r.status, serde_json::from_slice(&r.body).unwrap())
}

#[tokio::test]
async fn off_by_default() {
    let env = start().await;
    assert!(env.pulse.ui_url().is_none());
    assert_eq!(http(env.port, "GET", "/ui/", &[]).await.status, 404);
    assert_eq!(
        http(env.port, "POST", "/ui/rpc/get_state", &[])
            .await
            .status,
        404
    );
}

#[tokio::test]
async fn rpc_runs_commands() {
    let ui = start_ui().await;
    let (env, secret) = (&ui.env, ui.secret.clone());
    let (status, state) = rpc(env, &secret, "get_state", Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(state["server"]["port"], env.port);

    let (_, info) = rpc(env, &secret, "get_host_info", json!({})).await;
    assert_eq!(info, json!({"kind": "obs", "updater": false}));

    let (status, saved) = rpc(
        env,
        &secret,
        "save_widget",
        json!({"widget": {"id": "wtest0001", "name": "Test"}}),
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["name"], "Test");

    // Without an updater: nothing found, nothing to install.
    assert_eq!(
        rpc(env, &secret, "check_for_updates", Value::Null).await,
        (200, Value::Null)
    );
    let (status, e) = rpc(env, &secret, "install_update", Value::Null).await;
    assert_eq!(status, 400);
    assert!(e["error"].as_str().unwrap().contains("doesn't update"));

    // Command errors and bad arguments are 400 with the text.
    let (status, e) = rpc(env, &secret, "remove_paired", json!({"id": "nobody"})).await;
    assert_eq!((status, e["error"].as_str()), (400, Some("not found")));
    let (status, e) = rpc(env, &secret, "remove_paired", json!({})).await;
    assert_eq!(status, 400);
    assert!(e["error"].as_str().unwrap().contains("`id`"));

    let (status, _) = rpc(env, &secret, "no_such_command", Value::Null).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn rpc_needs_secret_and_own_origin() {
    let ui = start_ui().await;
    let (env, secret) = (&ui.env, ui.secret.clone());
    let p = env.port;
    assert_eq!(http(p, "POST", "/ui/rpc/get_state", &[]).await.status, 401);
    let wrong = "0".repeat(64);
    assert_eq!(
        http(p, "POST", "/ui/rpc/get_state", &[("X-UwUPulse-UI", &wrong)])
            .await
            .status,
        401
    );
    // The secret in the query only counts for the WebSocket… and here too
    // only from our own origin.
    let own = format!("http://127.0.0.1:{p}");
    let ok = http(
        p,
        "POST",
        "/ui/rpc/get_settings",
        &[("X-UwUPulse-UI", &secret), ("Origin", &own)],
    )
    .await;
    assert_eq!(ok.status, 200);
    assert_eq!(ok.header("access-control-allow-origin"), None);
    let foreign = http(
        p,
        "POST",
        "/ui/rpc/get_settings",
        &[("X-UwUPulse-UI", &secret), ("Origin", "http://example.com")],
    )
    .await;
    assert_eq!(foreign.status, 403);
}

#[tokio::test]
async fn events_push_hr() {
    let ui = start_ui().await;
    let (env, secret) = (&ui.env, ui.secret.clone());
    let own = format!("http://127.0.0.1:{}", env.port);
    let refused = ws_connect_with(env.port, "/ui/events?s=wrong", &[]).await;
    assert_eq!(refused_status(refused), Some(401));
    let foreign = ws_connect_with(
        env.port,
        &format!("/ui/events?s={secret}"),
        &[("Origin", "http://example.com")],
    )
    .await;
    assert_eq!(refused_status(foreign), Some(403));

    let mut ws: Ws = ws_connect_with(
        env.port,
        &format!("/ui/events?s={secret}"),
        &[("Origin", &own)],
    )
    .await
    .unwrap();
    let first = event(&mut ws).await;
    assert_eq!(first["event"], "pulse://state");
    assert_eq!(first["payload"]["server"]["port"], env.port);

    env.pulse.push_sample(
        "strap",
        HrSample {
            ts: 1_700_000_000_000,
            bpm: 123,
            src: Src::Ble,
            dev: "Strap".into(),
        },
    );
    loop {
        let ev = event(&mut ws).await;
        if ev["event"] == "pulse://hr" {
            assert_eq!(ev["payload"]["bpm"], 123);
            break;
        }
    }
}

async fn event(ws: &mut Ws) -> Value {
    match next_msg(ws).await {
        Some(Message::Text(t)) => serde_json::from_str(t.as_str()).unwrap(),
        other => panic!("expected an event, got {other:?}"),
    }
}

#[tokio::test]
async fn static_files() {
    let ui = start_ui().await;
    let env = &ui.env;
    let p = env.port;
    let index = http(p, "GET", "/ui/", &[]).await;
    assert_eq!(index.status, 200);
    assert_eq!(
        index.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert!(index.header("content-security-policy").is_some());
    assert!(index.text().contains("UwUPulse"));

    let js = http(p, "GET", "/ui/assets/app.js", &[]).await;
    assert_eq!(js.status, 200);
    assert_eq!(
        js.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(js.header("x-content-type-options"), Some("nosniff"));

    assert_eq!(http(p, "GET", "/ui", &[]).await.status, 308);
    assert_eq!(http(p, "GET", "/ui/missing.js", &[]).await.status, 404);
    assert_eq!(http(p, "GET", "/ui/assets", &[]).await.status, 404);
    for path in [
        "/ui/../ui.secret",
        "/ui/%2e%2e/ui.secret",
        "/ui/assets/..%2f..%2fui.secret",
        "/ui/%2e%2e%5cui.secret",
        "/ui/.%2e/ui.secret",
    ] {
        let r = http(p, "GET", path, &[]).await;
        assert!(r.status == 404 || r.status == 400, "{path}: {}", r.status);
        assert!(!r.text().contains("nope"), "{path}");
    }
}
