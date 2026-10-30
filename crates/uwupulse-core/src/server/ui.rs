//! `/ui`: the app's page for a host without a webview of its own (the OBS
//! plugin's dock), with the commands and events the desktop app gets over
//! Tauri. Only routed when the host asked for it ([`crate::PulseOptions::ui`]).
//!
//! - `GET /ui/`, `/ui/<file>`: the built frontend from the host's folder. No
//!   secret: there is nothing in it that isn't in the release.
//! - `POST /ui/rpc/<command>` (JSON args → JSON result, 4xx `{"error"}`) and
//!   `GET /ui/events` (WebSocket, `{"event", "payload"}`): only from loopback,
//!   only without an `Origin` or with the server's own, and only with the
//!   secret of this run (`X-UwUPulse-UI` / `?s=`).

use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;

use crate::commands::{self, UiEvents};
use crate::hub::Inner;
use crate::Pulse;

/// The secret on `/ui/rpc`.
pub const SECRET_HEADER: &str = "x-uwupulse-ui";
/// Command arguments are small; a widget with its CSS is the largest
/// (≤ 64 KiB as JSON, a little more escaped).
const MAX_RPC_BODY: usize = 256 * 1024;

/// What the page may load and talk to: like the Tauri window's CSP, with the
/// server itself instead of Tauri's IPC.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data:; font-src 'self' data:; \
connect-src 'self' http://127.0.0.1:* ws://127.0.0.1:*; \
frame-src http://127.0.0.1:* http://localhost:*; object-src 'none'; base-uri 'none'; \
form-action 'none'; frame-ancestors 'none'";

pub(super) fn router(inner: Arc<Inner>) -> Router<Arc<Inner>> {
    let guarded = Router::new()
        .route("/ui/rpc/{command}", post(rpc))
        .route("/ui/events", get(events))
        .layer(DefaultBodyLimit::max(MAX_RPC_BODY))
        .route_layer(middleware::from_fn_with_state(inner, guard));
    Router::new()
        .route("/ui", get(|| async { Redirect::permanent("/ui/") }))
        .route("/ui/", get(index))
        .route("/ui/{*path}", get(file))
        .merge(guarded)
}

/// Why a request to `/ui/rpc` or `/ui/events` is refused, if it is.
pub(crate) fn check_access(
    peer: IpAddr,
    origin: Option<&str>,
    given: Option<&str>,
    secret: &str,
    port: u16,
) -> Result<(), StatusCode> {
    if !peer.is_loopback() {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(origin) = origin {
        let own = [
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
        ];
        if !own.iter().any(|o| o == origin) {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    match given {
        Some(g) if bool::from(g.as_bytes().ct_eq(secret.as_bytes())) => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn guard(
    State(inner): State<Arc<Inner>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let Some(ui) = inner.ui.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let headers = req.headers();
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let from_header = headers.get(SECRET_HEADER).and_then(|v| v.to_str().ok());
    let from_query = req
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("s=")));
    let port = inner.server_status.lock().port;
    let peer = match peer.ip() {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        ip => ip,
    };
    if let Err(status) = check_access(peer, origin, from_header.or(from_query), &ui.secret, port) {
        let msg = match status {
            StatusCode::UNAUTHORIZED => "secret missing or wrong",
            _ => "not allowed",
        };
        return (status, Json(json!({ "error": msg }))).into_response();
    }
    next.run(req).await
}

async fn rpc(
    State(inner): State<Arc<Inner>>,
    Path(command): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(ui) = inner.ui.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !commands::COMMANDS.contains(&command.as_str()) {
        return error(StatusCode::NOT_FOUND, format!("unknown command: {command}"));
    }
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    let args = if body.is_empty() {
        Value::Null
    } else if !is_json {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "expected application/json".into(),
        );
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(v @ (Value::Object(_) | Value::Null)) => v,
            Ok(_) => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "arguments must be an object".into(),
                )
            }
            Err(e) => return error(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")),
        }
    };
    let host = ui.host.clone();
    let pulse = Pulse(inner.clone());
    match commands::dispatch(&pulse, &host, &command, args).await {
        Ok(v) => ([(header::CACHE_CONTROL, "no-store")], Json(v)).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}

fn error(status: StatusCode, msg: String) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

async fn events(State(inner): State<Arc<Inner>>, upgrade: WebSocketUpgrade) -> Response {
    let Ok(permit) = inner.api_slots.clone().try_acquire_owned() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "too many connections").into_response();
    };
    upgrade
        // The page sends nothing we read.
        .max_message_size(4096)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            feed(inner, socket).await;
        })
}

/// The state first (so a reconnecting page catches up), then every event
/// until the page leaves or the server restarts.
async fn feed(inner: Arc<Inner>, mut socket: WebSocket) {
    let pulse = Pulse(inner.clone());
    // Subscribe before the state, so nothing between the two is lost.
    let mut events = UiEvents::new(&pulse);
    let mut generation = inner.generation.subscribe();
    let first = serde_json::to_value(pulse.state()).map(|s| ("pulse://state", s));
    if let Ok(first) = first {
        if send(&mut socket, first).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            ev = events.next() => {
                let Some(ev) = ev else { break };
                if send(&mut socket, ev).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            _ = generation.changed() => break,
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn send(socket: &mut WebSocket, (event, payload): (&str, Value)) -> Result<(), axum::Error> {
    let msg = json!({ "event": event, "payload": payload });
    socket.send(Message::Text(msg.to_string().into())).await
}

// ── Static files ────────────────────────────────────────

async fn index(State(inner): State<Arc<Inner>>) -> Response {
    serve_file(&inner, "index.html").await
}

async fn file(State(inner): State<Arc<Inner>>, Path(path): Path<String>) -> Response {
    serve_file(&inner, &path).await
}

/// `rel` inside `dir`, or `None` for anything that could leave it: `..`,
/// absolute paths, drive letters, backslashes, hidden files.
pub(crate) fn safe_path(dir: &FsPath, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.contains(['\\', ':', '\0']) {
        return None;
    }
    let mut out = dir.to_path_buf();
    for part in rel.split('/') {
        if part.is_empty() || part.starts_with('.') {
            return None;
        }
        let mut comps = FsPath::new(part).components();
        match (comps.next(), comps.next()) {
            (Some(Component::Normal(c)), None) => out.push(c),
            _ => return None,
        }
    }
    Some(out)
}

/// The file's real path and bytes, if it is a file inside `dir` (a symlink
/// must not lead out of the folder either).
fn read_inside(dir: &FsPath, path: &FsPath) -> Option<(PathBuf, Vec<u8>)> {
    let root = std::fs::canonicalize(dir).ok()?;
    let real = std::fs::canonicalize(path).ok()?;
    if !real.starts_with(&root) || !std::fs::metadata(&real).ok()?.is_file() {
        return None;
    }
    let bytes = std::fs::read(&real).ok()?;
    Some((real, bytes))
}

fn content_type(path: &FsPath) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn serve_file(inner: &Inner, rel: &str) -> Response {
    let Some(ui) = inner.ui.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(path) = safe_path(&ui.dir, rel) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let dir = ui.dir.clone();
    let read = tokio::task::spawn_blocking(move || read_inside(&dir, &path)).await;
    let Ok(Some((real, bytes))) = read else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ty = content_type(&real);
    let mut res = Response::new(Body::from(bytes));
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ty));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if ty.starts_with("text/html") {
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        );
        h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn access_needs_loopback_origin_and_secret() {
        let lo = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let lan = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7));
        let ok = |peer, origin, given| check_access(peer, origin, given, "s3cret", 8746);
        assert_eq!(ok(lo, None, Some("s3cret")), Ok(()));
        assert_eq!(
            ok(lo, Some("http://127.0.0.1:8746"), Some("s3cret")),
            Ok(())
        );
        assert_eq!(
            ok(lo, Some("http://localhost:8746"), Some("s3cret")),
            Ok(())
        );
        assert_eq!(ok(lan, None, Some("s3cret")), Err(StatusCode::FORBIDDEN));
        assert_eq!(
            ok(lo, Some("http://example.com"), Some("s3cret")),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            ok(lo, Some("http://127.0.0.1:9999"), Some("s3cret")),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(ok(lo, None, None), Err(StatusCode::UNAUTHORIZED));
        assert_eq!(ok(lo, None, Some("s3cre")), Err(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn paths_stay_inside() {
        let dir = FsPath::new("/srv/ui");
        assert_eq!(
            safe_path(dir, "assets/index.js"),
            Some(PathBuf::from("/srv/ui/assets/index.js"))
        );
        for bad in [
            "",
            "../secret",
            "assets/../../x",
            "/etc/passwd",
            "assets//x",
            ".hidden",
            "a\\b",
            "C:x",
            "a\0b",
        ] {
            assert_eq!(safe_path(dir, bad), None, "{bad:?}");
        }
    }
}
