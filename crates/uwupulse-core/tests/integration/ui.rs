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
    start_ui_with(|_| {}).await
}

/// Like [`start_ui`], with options adjusted by `f`.
async fn start_ui_with(f: impl FnOnce(&mut PulseOptions)) -> Ui {
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
        });
        f(o);
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
    rpc_at(env.port, secret, command, args).await
}

async fn rpc_at(port: u16, secret: &str, command: &str, args: Value) -> (u16, Value) {
    let body = args.to_string();
    let r = http_body(
        port,
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

/// `POST /ui/ticket` with `ticket`, from the page's own origin.
async fn redeem(env: &Env, ticket: &str) -> (u16, Value) {
    let own = format!("http://127.0.0.1:{}", env.port);
    let body = json!({ "ticket": ticket }).to_string();
    let r = http_body(
        env.port,
        "POST",
        "/ui/ticket",
        &[("Content-Type", "application/json"), ("Origin", &own)],
        body.as_bytes(),
    )
    .await;
    (
        r.status,
        serde_json::from_slice(&r.body).unwrap_or(Value::Null),
    )
}

fn ticket_of(env: &Env) -> String {
    let url = env.pulse.ui_ticket_url().unwrap();
    let prefix = format!("http://127.0.0.1:{}/ui/#t=", env.port);
    let ticket = url.strip_prefix(&prefix).expect(&url).to_owned();
    assert_eq!(ticket.len(), 64);
    ticket
}

#[tokio::test]
async fn a_ticket_buys_the_secret_once() {
    let ui = start_ui().await;
    let env = &ui.env;
    let ticket = ticket_of(env);
    assert!(!ticket.contains(&ui.secret));

    // Wrong: refused, and the real one still works afterwards.
    assert_eq!(redeem(env, &"0".repeat(64)).await.0, 401);
    assert_eq!(redeem(env, "").await.0, 401);
    let (status, body) = redeem(env, &ticket).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["secret"], ui.secret.as_str());
    // Used twice: refused.
    assert_eq!(redeem(env, &ticket).await.0, 401);

    // Only from loopback pages of this server.
    let other = ticket_of(env);
    let body = json!({ "ticket": other }).to_string();
    let foreign = http_body(
        env.port,
        "POST",
        "/ui/ticket",
        &[("Origin", "http://example.com")],
        body.as_bytes(),
    )
    .await;
    assert_eq!(foreign.status, 403);
    assert_eq!(redeem(env, &other).await.0, 200);
}

#[tokio::test]
async fn an_expired_ticket_is_refused() {
    let ui = start_ui_with(|o| o.ui_ticket_ttl = std::time::Duration::ZERO).await;
    let ticket = ticket_of(&ui.env);
    assert_eq!(redeem(&ui.env, &ticket).await.0, 401);
}

#[tokio::test]
async fn a_new_port_is_answered_before_the_restart() {
    let free = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let (first, second) = (free(), free());
    let ui = start_ui_with(|o| {
        o.port_override = None;
        std::fs::write(
            o.config_dir.join("settings.json"),
            format!(r#"{{"lanEnabled":false,"port":{first}}}"#),
        )
        .unwrap();
    })
    .await;
    let env = &ui.env;
    assert_eq!(env.port, first);
    let (_, mut settings) = rpc(env, &ui.secret, "get_settings", Value::Null).await;

    // An invalid port is refused right away, without a restart.
    settings["port"] = json!(80);
    let (status, e) = rpc(
        env,
        &ui.secret,
        "set_settings",
        json!({ "settings": settings }),
    )
    .await;
    assert_eq!(status, 400, "{e}");

    let mut events = env.pulse.subscribe();
    settings["port"] = json!(second);
    // Restarting inline held the answer until the old server gave up on
    // this very connection (3 s).
    let (status, saved) = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        rpc(
            env,
            &ui.secret,
            "set_settings",
            json!({ "settings": settings }),
        ),
    )
    .await
    .expect("answered before the server went away");
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["port"], second);

    if env.pulse.server_info().port != second {
        crate::wait_for(&mut events, |_| {
            (env.pulse.server_info().port == second).then_some(())
        })
        .await;
    }
    let (status, now) = rpc_at(second, &ui.secret, "get_settings", Value::Null).await;
    assert_eq!((status, now["port"].clone()), (200, json!(second)));
}
