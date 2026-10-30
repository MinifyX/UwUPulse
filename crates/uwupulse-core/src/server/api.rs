//! Overlay page and the token-protected API.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use tokio::sync::broadcast::error::RecvError;

use crate::hub::Inner;
use crate::types::{Event, ExportFormat};
use crate::widgets::{id_of, valid_widget_id};
use crate::Error;

const OVERLAY_HTML: &str = include_str!("../../overlay/index.html");
const OVERLAY_JS: &str = include_str!("../../overlay/overlay.js");
const OVERLAY_CSS: &str = include_str!("../../overlay/overlay.css");

/// `?token=` or `Authorization: Bearer`, compared in constant time.
pub(super) async fn auth(State(inner): State<Arc<Inner>>, req: Request, next: Next) -> Response {
    let from_query = req.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix("token="))
            .map(str::to_owned)
    });
    let from_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.trim().to_owned());
    let ok = {
        let token = inner.token.lock();
        [from_query, from_header]
            .into_iter()
            .flatten()
            .any(|given| bool::from(given.as_bytes().ct_eq(token.as_bytes())))
    };
    if !ok {
        return (StatusCode::UNAUTHORIZED, "token missing or wrong").into_response();
    }
    next.run(req).await
}

/// GET from any origin (the token is the protection); answers preflights.
pub(super) async fn cors(req: Request, next: Next) -> Response {
    if req.method() == Method::OPTIONS {
        let mut res = StatusCode::NO_CONTENT.into_response();
        let h = res.headers_mut();
        h.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, OPTIONS"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization"),
        );
        h.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
        return res;
    }
    let mut res = next.run(req).await;
    res.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    res
}

fn error_response(e: Error) -> Response {
    match e {
        Error::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
        e => {
            log::error!("API: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

pub(super) async fn overlay_page(Path(id): Path<String>) -> Response {
    if !valid_widget_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            // The token is in the URL; it shouldn't leak into fetched fonts
            // or links.
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        OVERLAY_HTML,
    )
        .into_response()
}

pub(super) async fn asset(Path(file): Path<String>) -> Response {
    let (body, ty) = match file.as_str() {
        "overlay.js" => (OVERLAY_JS, "text/javascript; charset=utf-8"),
        "overlay.css" => (OVERLAY_CSS, "text/css; charset=utf-8"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, ty),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

fn current_json(inner: &Inner) -> Value {
    let l = inner.live.lock();
    match &l.current {
        Some(s) => json!({"bpm": s.bpm, "ts": s.ts, "src": s.src, "dev": s.dev, "live": l.live}),
        None => json!({"bpm": null, "ts": null, "src": null, "dev": null, "live": false}),
    }
}

pub(super) async fn current(State(inner): State<Arc<Inner>>) -> Json<Value> {
    Json(current_json(&inner))
}

pub(super) async fn widget(State(inner): State<Arc<Inner>>, Path(id): Path<String>) -> Response {
    let found = inner
        .widgets
        .lock()
        .iter()
        .find(|w| id_of(w) == id)
        .cloned();
    match found {
        Some(w) => ([(header::CACHE_CONTROL, "no-store")], Json(w)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(super) async fn sessions(State(inner): State<Arc<Inner>>) -> Response {
    match inner.recorder.list() {
        Ok(l) => Json(l).into_response(),
        Err(e) => error_response(e),
    }
}

pub(super) async fn session(State(inner): State<Arc<Inner>>, Path(id): Path<String>) -> Response {
    match inner.recorder.get(&id) {
        Ok(d) => Json(d).into_response(),
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
pub(super) struct ExportQuery {
    format: Option<ExportFormat>,
}

pub(super) async fn export(
    State(inner): State<Arc<Inner>>,
    Path(id): Path<String>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let format = q.format.unwrap_or(ExportFormat::Csv);
    let bytes = match inner.recorder.export(&id, format) {
        Ok(b) => b,
        Err(e) => return error_response(e),
    };
    let (ext, ty) = match format {
        ExportFormat::Csv => ("csv", "text/csv; charset=utf-8"),
        ExportFormat::Json => ("json", "application/json"),
    };
    // Only reached for an ID that exists, so it's one of ours (a UUID);
    // filter anyway, it goes into a header.
    let safe: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(64)
        .collect();
    let disposition = format!("attachment; filename=\"uwupulse-{safe}.{ext}\"");
    let mut res = Response::new(Body::from(bytes));
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ty));
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    res
}

pub(super) async fn ws(State(inner): State<Arc<Inner>>, upgrade: WebSocketUpgrade) -> Response {
    let Ok(permit) = inner.api_slots.clone().try_acquire_owned() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "too many connections").into_response();
    };
    upgrade
        // Clients don't send anything we read.
        .max_message_size(4096)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            feed(inner, socket).await;
        })
}

/// Pushes `hr`, `live` and `widget` until the client leaves, the server
/// restarts or the token changes.
async fn feed(inner: Arc<Inner>, mut socket: WebSocket) {
    // Subscribe before the hello, so nothing between the two is lost.
    let mut events = inner.events.subscribe();
    let mut generation = inner.generation.subscribe();
    let mut token_gen = inner.token_generation.subscribe();
    let hello = json!({
        "t": "hello",
        "version": inner.app_version,
        "current": current_json(&inner),
    });
    if send(&mut socket, &hello).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            ev = events.recv() => {
                let msg = match ev {
                    Ok(Event::Hr(s)) => {
                        json!({"t": "hr", "ts": s.ts, "bpm": s.bpm, "src": s.src, "dev": s.dev})
                    }
                    Ok(Event::Live(live)) => json!({"t": "live", "live": live}),
                    Ok(Event::Widget { id, config }) => {
                        json!({"t": "widget", "id": id, "config": config})
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                };
                if send(&mut socket, &msg).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                // Pings are answered by the library; anything else is ignored.
                Some(Ok(_)) => {}
            },
            _ = generation.changed() => break,
            _ = token_gen.changed() => break,
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn send(socket: &mut WebSocket, v: &Value) -> Result<(), axum::Error> {
    socket.send(Message::Text(v.to_string().into())).await
}
