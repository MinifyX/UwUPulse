//! The one HTTP server: `/link` for phones, `/overlay/*` for OBS,
//! `/api/v1/*` for everything else. Started at boot and again whenever the
//! port or LAN access changes.

mod api;
mod link;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::{middleware, Router};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::hub::Inner;

pub(crate) struct Running {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

fn router(inner: Arc<Inner>) -> Router {
    let protected = Router::new()
        .route("/overlay/{id}", get(api::overlay_page))
        .route("/api/v1/current", get(api::current))
        .route("/api/v1/ws", get(api::ws))
        .route("/api/v1/widgets/{id}", get(api::widget))
        .route("/api/v1/sessions", get(api::sessions))
        .route("/api/v1/sessions/{id}", get(api::session))
        .route("/api/v1/sessions/{id}/export", get(api::export))
        .route_layer(middleware::from_fn_with_state(inner.clone(), api::auth));
    Router::new()
        .route("/link", get(link::handler))
        .route("/overlay/assets/{file}", get(api::asset))
        .merge(protected)
        .layer(middleware::from_fn(api::cors))
        .with_state(inner)
}

/// Binds and serves. A failed bind lands in `ServerInfo.error` instead of
/// failing: the app still opens and says what's wrong.
pub(crate) async fn start(inner: &Arc<Inner>) {
    let lan = inner.settings.lock().lan_enabled;
    let port = inner
        .port_override
        .unwrap_or_else(|| inner.settings.lock().port);
    let ip = if lan {
        Ipv4Addr::UNSPECIFIED
    } else {
        Ipv4Addr::LOCALHOST
    };
    let listener = match TcpListener::bind((ip, port)).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("cannot listen on {ip}:{port}: {e}");
            *inner.server_status.lock() = crate::hub::ServerStatus {
                port,
                lan,
                error: Some(format!("Port {port} is not available: {e}")),
            };
            return;
        }
    };
    let bound = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    *inner.server_status.lock() = crate::hub::ServerStatus {
        port: bound,
        lan,
        error: None,
    };
    log::info!("listening on {ip}:{bound}");
    let (stop, stopped) = oneshot::channel::<()>();
    let app = router(inner.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let task = inner.rt.spawn(async move {
        let res = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await;
        if let Err(e) = res {
            log::error!("server stopped: {e}");
        }
    });
    *inner.server.lock().await = Some(Running { stop, task });
}

/// Stops accepting, ends every open WebSocket and waits (briefly) for the
/// rest to finish.
pub(crate) async fn stop(inner: &Arc<Inner>) {
    let Some(running) = inner.server.lock().await.take() else {
        return;
    };
    let _ = running.stop.send(());
    inner.close_connections();
    let mut task = running.task;
    if tokio::time::timeout(Duration::from_secs(3), &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
}
