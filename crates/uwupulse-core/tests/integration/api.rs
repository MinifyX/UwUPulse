//! The token-protected API, the overlay routes and recorded sessions.

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use uwupulse_core::{ExportFormat, HrSample, Src};

use crate::{http, next_msg, start, ws_connect};

fn sample(bpm: u16, ts: i64) -> HrSample {
    HrSample {
        ts,
        bpm,
        src: Src::Ble,
        dev: "Strap".into(),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test]
async fn token_and_cors() {
    let env = start().await;
    let p = env.port;
    assert_eq!(http(p, "GET", "/api/v1/current", &[]).await.status, 401);
    assert_eq!(
        http(p, "GET", "/api/v1/current?token=wrong", &[])
            .await
            .status,
        401
    );
    let ok = http(
        p,
        "GET",
        &format!("/api/v1/current?token={}", env.token),
        &[],
    )
    .await;
    assert_eq!(ok.status, 200);
    assert_eq!(ok.header("access-control-allow-origin"), Some("*"));
    let v: Value = serde_json::from_slice(&ok.body).unwrap();
    assert_eq!(
        v,
        json!({"bpm": null, "ts": null, "src": null, "dev": null, "live": false})
    );

    let bearer = format!("Bearer {}", env.token);
    let r = http(
        p,
        "GET",
        "/api/v1/widgets/classic",
        &[("Authorization", &bearer)],
    )
    .await;
    assert_eq!(r.status, 200);
    let w: Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(w["name"], "Classic");
    let r = http(
        p,
        "GET",
        "/api/v1/widgets/none",
        &[("Authorization", &bearer)],
    )
    .await;
    assert_eq!(r.status, 404);

    let pre = http(
        p,
        "OPTIONS",
        "/api/v1/current",
        &[("Origin", "http://example.com")],
    )
    .await;
    assert_eq!(pre.status, 204);
    assert!(pre
        .header("access-control-allow-headers")
        .is_some_and(|h| h.contains("Authorization")));

    // Overlay page needs the token, its assets don't.
    assert_eq!(http(p, "GET", "/overlay/classic", &[]).await.status, 401);
    let page = http(
        p,
        "GET",
        &format!("/overlay/classic?token={}", env.token),
        &[],
    )
    .await;
    assert_eq!(page.status, 200);
    assert_eq!(page.header("referrer-policy"), Some("no-referrer"));
    assert!(page
        .header("content-type")
        .unwrap()
        .starts_with("text/html"));
    for asset in ["overlay.js", "overlay.css"] {
        let r = http(p, "GET", &format!("/overlay/assets/{asset}"), &[]).await;
        assert_eq!(r.status, 200, "{asset}");
    }
    assert_eq!(
        http(p, "GET", "/overlay/assets/secret", &[]).await.status,
        404
    );

    // A new token locks the old one out.
    let info = env.pulse.regenerate_token().unwrap();
    assert_ne!(info.token, env.token);
    let old = http(
        p,
        "GET",
        &format!("/api/v1/current?token={}", env.token),
        &[],
    )
    .await;
    assert_eq!(old.status, 401);
    let new = http(
        p,
        "GET",
        &format!("/api/v1/current?token={}", info.token),
        &[],
    )
    .await;
    assert_eq!(new.status, 200);
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn sessions_are_recorded_and_exported() {
    let env = start().await;
    let t = now();
    for (i, bpm) in [70u16, 80, 90].into_iter().enumerate() {
        env.pulse
            .push_sample("ble:test", sample(bpm, t + i as i64 * 1000));
    }
    assert!(env.pulse.state().live);
    assert!(env.pulse.state().recording.is_some());

    let q = format!("?token={}", env.token);
    let list = http(env.port, "GET", &format!("/api/v1/sessions{q}"), &[]).await;
    assert_eq!(list.status, 200);
    let list: Value = serde_json::from_slice(&list.body).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    let s = &list[0];
    assert_eq!(s["minBpm"], 70);
    assert_eq!(s["maxBpm"], 90);
    assert_eq!(s["avgBpm"], 80.0);
    assert_eq!(s["sampleCount"], 3);
    assert_eq!(s["sources"], "ble");
    let id = s["id"].as_str().unwrap().to_owned();

    let detail = http(env.port, "GET", &format!("/api/v1/sessions/{id}{q}"), &[]).await;
    let detail: Value = serde_json::from_slice(&detail.body).unwrap();
    assert_eq!(
        detail["samples"],
        json!([[t, 70], [t + 1000, 80], [t + 2000, 90]])
    );

    let csv = http(
        env.port,
        "GET",
        &format!("/api/v1/sessions/{id}/export{q}&format=csv"),
        &[],
    )
    .await;
    assert_eq!(csv.status, 200);
    assert!(csv
        .header("content-disposition")
        .is_some_and(|h| h.starts_with("attachment;") && h.contains(".csv")));
    let lines: Vec<&str> = csv.text().lines().collect();
    assert_eq!(lines[0], "timestamp_ms,iso8601,bpm,source");
    assert_eq!(lines.len(), 4);
    assert!(lines[1].ends_with(",70,ble"));

    let missing = http(env.port, "GET", &format!("/api/v1/sessions/nope{q}"), &[]).await;
    assert_eq!(missing.status, 404);

    let path = env.dir.path().join("out.json");
    env.pulse
        .export_session_to(&id, ExportFormat::Json, &path)
        .unwrap();
    let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json["sampleCount"], 3);

    env.pulse.delete_session(&id).unwrap();
    assert!(env.pulse.list_sessions().unwrap().is_empty());
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn overlay_feed() {
    let env = start().await;
    assert!(
        ws_connect(env.port, "/api/v1/ws").await.is_err(),
        "no token"
    );

    let mut ws = ws_connect(env.port, &format!("/api/v1/ws?token={}", env.token))
        .await
        .unwrap();
    let hello = match next_msg(&mut ws).await {
        Some(Message::Text(t)) => serde_json::from_str::<Value>(t.as_str()).unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(hello["t"], "hello");
    assert_eq!(hello["current"]["bpm"], Value::Null);

    env.pulse.push_sample("ble:test", sample(101, now()));
    let mut saw_hr = false;
    let mut saw_live = false;
    while !(saw_hr && saw_live) {
        let Some(Message::Text(t)) = next_msg(&mut ws).await else {
            panic!("closed")
        };
        let v: Value = serde_json::from_str(t.as_str()).unwrap();
        match v["t"].as_str() {
            Some("hr") => {
                assert_eq!(v["bpm"], 101);
                assert_eq!(v["src"], "ble");
                assert_eq!(v["dev"], "Strap");
                saw_hr = true;
            }
            Some("live") => {
                assert_eq!(v["live"], true);
                saw_live = true;
            }
            other => panic!("{other:?}"),
        }
    }

    let mut w = env.pulse.list_widgets()[0].clone();
    w["color"] = json!("#00ff00");
    env.pulse.save_widget(w).unwrap();
    let Some(Message::Text(t)) = next_msg(&mut ws).await else {
        panic!("closed")
    };
    let v: Value = serde_json::from_str(t.as_str()).unwrap();
    assert_eq!(v["t"], "widget");
    assert_eq!(v["id"], "classic");
    assert_eq!(v["config"]["color"], "#00ff00");

    // A new token ends sockets opened with the old one.
    env.pulse.regenerate_token().unwrap();
    assert!(next_msg(&mut ws).await.is_none());
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn settings_restart_the_server() {
    let env = start().await;
    let mut s = env.pulse.settings();
    assert!(!s.lan_enabled);
    s.idle_timeout_secs = 5;
    assert!(env.pulse.set_settings(s).await.is_err(), "too short");

    let mut ws = ws_connect(env.port, &format!("/api/v1/ws?token={}", env.token))
        .await
        .unwrap();
    assert!(next_msg(&mut ws).await.is_some());
    // Turning LAN access on rebinds (port 0 again in tests).
    s.idle_timeout_secs = 60;
    s.lan_enabled = true;
    let saved = env.pulse.set_settings(s).await.unwrap();
    assert!(saved.lan_enabled);
    assert!(next_msg(&mut ws).await.is_none(), "old sockets end");
    let info = env.pulse.server_info();
    assert!(info.lan_enabled && info.error.is_none());
    let r = http(
        info.port,
        "GET",
        &format!("/api/v1/current?token={}", info.token),
        &[],
    )
    .await;
    assert_eq!(r.status, 200);

    // Persisted.
    let file = std::fs::read_to_string(env.dir.path().join("config/settings.json")).unwrap();
    let v: Value = serde_json::from_str(&file).unwrap();
    assert_eq!(v["idleTimeoutSecs"], 60);
    env.pulse.shutdown().await;
}
