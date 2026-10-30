//! The one HTTP server: `/link` for phones, `/overlay/*` for OBS,
//! `/api/v1/*` for everything else. Started at boot and again whenever the
//! port or LAN access changes.

mod api;
mod link;

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::pin::{pin, Pin};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::routing::get;
use axum::{middleware, Router};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, watch, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tower_service::Service;

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

/// Open connections at most, WebSockets included.
const MAX_CONNECTIONS: usize = 128;
/// Open connections per client address. OBS with a handful of overlays and
/// a phone or two stay far below it.
const MAX_CONNECTIONS_PER_IP: u32 = 32;

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
    let app = router(inner.clone());
    let header_timeout = inner.header_timeout;
    let task = inner
        .rt
        .spawn(serve(listener, app, header_timeout, async move {
            let _ = stopped.await;
        }));
    *inner.server.lock().await = Some(Running { stop, task });
}

/// The accept loop: hyper with a timer (so the header timeout applies), a
/// global and a per-address connection cap, and graceful shutdown. Each
/// connection's slot lives in its IO, so an upgraded WebSocket keeps it
/// until it closes.
async fn serve(
    listener: TcpListener,
    app: Router,
    header_timeout: Duration,
    shutdown: impl Future<Output = ()>,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let per_ip: Arc<Mutex<HashMap<IpAddr, u32>>> = Arc::default();
    // Every connection task holds a receiver; `closed()` on the sender
    // resolves once all of them are done. A send asks them to wind down.
    let (drain, watcher) = watch::channel(());
    let mut shutdown = pin!(shutdown);
    loop {
        let (tcp, addr) = tokio::select! {
            res = listener.accept() => match res {
                Ok(v) => v,
                Err(e) => {
                    // Out of FDs and the like: back off instead of spinning.
                    log::warn!("accept: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
            () = &mut shutdown => break,
        };
        let Some(slot) = Slot::take(&slots, &per_ip, addr.ip()) else {
            // Dropping the stream closes it.
            continue;
        };
        let _ = tcp.set_nodelay(true);
        let io = TokioIo::new(Guarded {
            io: tcp,
            _slot: slot,
        });
        let app = app.clone();
        let svc = service_fn(move |mut req: Request<Incoming>| {
            req.extensions_mut().insert(ConnectInfo(addr));
            let mut app = app.clone();
            app.call(req.map(Body::new))
        });
        let mut watcher = watcher.clone();
        tokio::spawn(async move {
            let conn = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(header_timeout)
                .serve_connection(io, svc)
                .with_upgrades();
            let mut conn = pin!(conn);
            tokio::select! {
                res = conn.as_mut() => {
                    if let Err(e) = res {
                        log::debug!("connection {addr}: {e}");
                    }
                    return;
                }
                _ = watcher.changed() => conn.as_mut().graceful_shutdown(),
            }
            if let Err(e) = conn.await {
                log::debug!("connection {addr}: {e}");
            }
            drop(watcher);
        });
    }
    // Stop listening right away, so a restart can bind the port again.
    drop(listener);
    let _ = drain.send(());
    drop(watcher);
    drain.closed().await;
}

/// A connection's place under both caps; freed on drop.
struct Slot {
    _permit: OwnedSemaphorePermit,
    per_ip: Arc<Mutex<HashMap<IpAddr, u32>>>,
    ip: IpAddr,
}

impl Slot {
    fn take(
        slots: &Arc<Semaphore>,
        per_ip: &Arc<Mutex<HashMap<IpAddr, u32>>>,
        ip: IpAddr,
    ) -> Option<Self> {
        let permit = slots.clone().try_acquire_owned().ok()?;
        let mut map = per_ip.lock();
        let n = map.entry(ip).or_insert(0);
        if *n >= MAX_CONNECTIONS_PER_IP {
            return None;
        }
        *n += 1;
        Some(Self {
            _permit: permit,
            per_ip: per_ip.clone(),
            ip,
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut map = self.per_ip.lock();
        if let Some(n) = map.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                map.remove(&self.ip);
            }
        }
    }
}

/// A TCP stream that carries its [`Slot`].
struct Guarded {
    io: TcpStream,
    _slot: Slot,
}

impl AsyncRead for Guarded {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl AsyncWrite for Guarded {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
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
