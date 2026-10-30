//! `/link`: the phone's encrypted WebSocket. Handshake (desktop role), then
//! sealed binary frames. Everything here is attacker input until the
//! handshake proves otherwise, and even after: every parse is checked,
//! every frame is size-capped, and any violation just closes.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;
use tokio::sync::{mpsc, oneshot};
use uwupulse_proto::{
    b64_decode_32, b64_encode, limits, refusal, DeskMsg, DesktopHandshake, ErrorCode,
    HandshakeError, LinkCipher, Mode, PeerHello, PhoneMsg, WatchState,
};

use crate::hub::{source_for_phone, Inner, LinkCmd, SasSlot};
use crate::types::{Event, HrSample, PairDone, PairRequest};

/// From the upgrade to `auth` (`known`, `qr`) or to the `reveal` (`sas`).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a closing peer gets to answer our close.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) async fn handler(
    State(inner): State<Arc<Inner>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    // Browsers always send `Origin` with a WebSocket upgrade, the app never
    // does: no web page gets to talk to `/link`.
    if headers.contains_key(header::ORIGIN) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    if !inner.allow_link_attempt(addr.ip()) {
        return (StatusCode::TOO_MANY_REQUESTS, "slow down").into_response();
    }
    let Ok(permit) = inner.link_slots.clone().try_acquire_owned() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "too many connections").into_response();
    };
    let Some(pre) = PreAuth::take(&inner, addr.ip()) else {
        return (StatusCode::SERVICE_UNAVAILABLE, "too many connections").into_response();
    };
    upgrade
        .max_message_size(limits::MAX_FRAME_BYTES)
        .max_frame_size(limits::MAX_FRAME_BYTES)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            run(inner, socket, addr, pre).await;
        })
}

/// A connection that has not reached `ready`: holds one of the few slots for
/// those until `ready` goes out or the connection ends.
struct PreAuth {
    inner: Arc<Inner>,
    ip: IpAddr,
    held: bool,
}

impl PreAuth {
    fn take(inner: &Arc<Inner>, ip: IpAddr) -> Option<Self> {
        inner.acquire_pre_auth(ip).then(|| Self {
            inner: inner.clone(),
            ip,
            held: true,
        })
    }

    fn release(&mut self) {
        if std::mem::take(&mut self.held) {
            self.inner.release_pre_auth(self.ip);
        }
    }
}

impl Drop for PreAuth {
    fn drop(&mut self) {
        self.release();
    }
}

/// Text frames during the handshake; pings/pongs are skipped.
async fn next_text(ws: &mut WebSocket) -> Option<String> {
    loop {
        match ws.recv().await? {
            Ok(Message::Text(t)) => return Some(t.as_str().to_owned()),
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            _ => return None,
        }
    }
}

async fn next_binary(ws: &mut WebSocket) -> Option<Vec<u8>> {
    loop {
        match ws.recv().await? {
            Ok(Message::Binary(b)) => return Some(b.to_vec()),
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            _ => return None,
        }
    }
}

/// A plain-text refusal before the keys exist. The connection is over, so
/// its pre-`ready` slot goes back before the closing handshake.
async fn refuse(ws: &mut WebSocket, pre: &mut PreAuth, code: ErrorCode) {
    pre.release();
    let _ = ws.send(Message::Text(refusal(code).into())).await;
    close(ws).await;
}

/// A sealed refusal once the keys exist; like [`refuse`], the slot goes back
/// first.
async fn reject(ws: &mut WebSocket, cipher: &mut LinkCipher, pre: &mut PreAuth, code: ErrorCode) {
    pre.release();
    let _ = send_sealed(ws, cipher, &DeskMsg::Error { code }).await;
    close(ws).await;
}

/// Closing handshake: send a close and read until the peer answers. Dropping
/// the socket with unread frames would reset the connection, and the peer
/// could lose what was sent before the close.
async fn close(ws: &mut WebSocket) {
    if ws.send(Message::Close(None)).await.is_err() {
        return;
    }
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, async {
        while let Some(Ok(m)) = ws.recv().await {
            if matches!(m, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
}

/// Holds the single SAS slot; frees it however the attempt ends.
struct SasClaim<'a>(&'a Inner);

impl<'a> SasClaim<'a> {
    fn take(inner: &'a Inner) -> Option<Self> {
        let mut p = inner.pairing.lock();
        if p.sas.is_some() {
            return None;
        }
        p.sas = Some(SasSlot {
            request_id: String::new(),
            answer: None,
        });
        Some(Self(inner))
    }
}

impl Drop for SasClaim<'_> {
    fn drop(&mut self) {
        self.0.pairing.lock().sas = None;
    }
}

/// Unregisters a running link and its source.
struct LinkGuard<'a> {
    inner: &'a Inner,
    conn: u64,
    device_id: String,
}

impl Drop for LinkGuard<'_> {
    fn drop(&mut self) {
        self.inner.unregister_link(self.conn);
        // A reconnect of the same phone may already own the source.
        let replaced = self
            .inner
            .links
            .lock()
            .values()
            .any(|l| l.device_id == self.device_id);
        if !replaced {
            self.inner.remove_source(&self.device_id);
        }
        self.inner.emit_paired();
    }
}

async fn send_sealed(ws: &mut WebSocket, cipher: &mut LinkCipher, msg: &DeskMsg) -> bool {
    match cipher.seal_msg(msg) {
        Ok(frame) => ws.send(Message::Binary(frame.into())).await.is_ok(),
        Err(_) => false,
    }
}

/// The first sealed frame: it proves the phone derived the same keys.
async fn read_auth(
    ws: &mut WebSocket,
    cipher: &mut LinkCipher,
    deadline: tokio::time::Instant,
) -> bool {
    match tokio::time::timeout_at(deadline, next_binary(ws)).await {
        Ok(Some(b)) => matches!(cipher.open_msg::<PhoneMsg>(&b), Ok(PhoneMsg::Auth)),
        _ => false,
    }
}

// `pre` comes last so it is dropped first: its slot is free by the time the
// socket closes.
async fn run(inner: Arc<Inner>, mut ws: WebSocket, addr: SocketAddr, mut pre: PreAuth) {
    let mut generation = inner.generation.subscribe();
    let deadline = tokio::time::Instant::now() + HANDSHAKE_TIMEOUT;
    let m1 = match tokio::time::timeout_at(deadline, next_text(&mut ws)).await {
        Ok(Some(t)) => t,
        _ => return,
    };
    let hs = match DesktopHandshake::receive(&m1) {
        Ok(h) => h,
        Err(HandshakeError::Version) => return refuse(&mut ws, &mut pre, ErrorCode::Version).await,
        Err(e) => {
            log::debug!("link from {addr}: bad hello ({e:?})");
            return;
        }
    };
    let peer = hs.hello().clone();
    let mode = hs.mode();

    // Which psk applies, and is this attempt allowed at all?
    let mut sas = None;
    let mut qr_secret = None;
    let psk = match mode {
        Mode::Known => {
            let known = inner.paired.lock().iter().any(|p| {
                p.id == peer.id
                    && b64_decode_32(&p.spk).is_some_and(|k| bool::from(k.ct_eq(&peer.spk)))
            });
            if !known {
                return refuse(&mut ws, &mut pre, ErrorCode::UnknownDevice).await;
            }
            [0u8; 32]
        }
        Mode::Qr => {
            let secret = inner
                .pairing
                .lock()
                .offer
                .as_ref()
                .filter(|o| o.expires > Instant::now())
                .map(|o| o.secret);
            match secret {
                Some(s) => {
                    qr_secret = Some(s);
                    s
                }
                None => return refuse(&mut ws, &mut pre, ErrorCode::NoOffer).await,
            }
        }
        Mode::Sas => match SasClaim::take(&inner) {
            Some(c) => {
                sas = Some(c);
                [0u8; 32]
            }
            None => return refuse(&mut ws, &mut pre, ErrorCode::Busy).await,
        },
    };

    let Ok((m2, pending)) = hs.respond(&inner.keypair, &inner.desk_id, &inner.desk_name, &psk)
    else {
        return;
    };
    if ws.send(Message::Text(m2.into())).await.is_err() {
        return;
    }
    // `sas`: the phone now reveals the ephemeral key it committed to in `m1`.
    let m3 = if pending.needs_reveal() {
        match tokio::time::timeout_at(deadline, next_text(&mut ws)).await {
            Ok(Some(t)) => Some(t),
            _ => return,
        }
    } else {
        None
    };
    let keys = match pending.finish(m3.as_deref()) {
        Ok(k) => k,
        Err(e) => {
            log::debug!("link from {addr}: handshake failed ({e:?})");
            if m3.is_some() {
                // Free the code comparison before the phone sees the refusal.
                drop(sas.take());
                return refuse(&mut ws, &mut pre, ErrorCode::Rejected).await;
            }
            return;
        }
    };
    let mut cipher = LinkCipher::desktop(&keys);

    match mode {
        Mode::Known => {
            if !read_auth(&mut ws, &mut cipher, deadline).await {
                log::debug!("link from {addr}: auth failed");
                return;
            }
            pre.release();
            if !send_sealed(&mut ws, &mut cipher, &DeskMsg::Ready).await {
                return;
            }
        }
        Mode::Qr => {
            if !read_auth(&mut ws, &mut cipher, deadline).await {
                log::debug!("link from {addr}: auth failed");
                return;
            }
            // Single use: whoever authenticates first takes the offer. A new
            // offer created meanwhile has another secret and stays.
            let consumed = {
                let mut p = inner.pairing.lock();
                let valid = match (&p.offer, &qr_secret) {
                    (Some(o), Some(s)) => {
                        o.expires > Instant::now() && bool::from(o.secret.ct_eq(s))
                    }
                    _ => false,
                };
                if valid {
                    p.offer = None;
                }
                valid
            };
            if !consumed {
                return reject(&mut ws, &mut cipher, &mut pre, ErrorCode::NoOffer).await;
            }
            if !inner.add_paired(&peer.id, &peer.name, b64_encode(&peer.spk)) {
                log::warn!("phone {}: ID already paired with another key", peer.name);
                return reject(&mut ws, &mut cipher, &mut pre, ErrorCode::Rejected).await;
            }
            inner.emit_paired();
            if !announce(&inner, &mut ws, &mut cipher, &mut pre).await {
                return;
            }
        }
        Mode::Sas => {
            let (request_id, confirmed) =
                confirm_sas(&inner, &peer, &keys.sas, &mut ws, &mut cipher).await;
            let stored = confirmed && inner.add_paired(&peer.id, &peer.name, b64_encode(&peer.spk));
            if confirmed && !stored {
                log::warn!("phone {}: ID already paired with another key", peer.name);
            }
            inner.emit(Event::PairDone(PairDone {
                request_id,
                ok: stored,
            }));
            drop(sas.take());
            if !stored {
                return reject(&mut ws, &mut cipher, &mut pre, ErrorCode::Rejected).await;
            }
            inner.emit_paired();
            if !announce(&inner, &mut ws, &mut cipher, &mut pre).await {
                return;
            }
        }
    }
    drop(sas);

    session(&inner, peer, ws, cipher, &mut generation).await;
}

/// Tells a newly stored phone it is paired and ready.
async fn announce(
    inner: &Inner,
    ws: &mut WebSocket,
    cipher: &mut LinkCipher,
    pre: &mut PreAuth,
) -> bool {
    pre.release();
    let paired = DeskMsg::Paired {
        id: inner.desk_id.clone(),
        name: inner.desk_name.clone(),
    };
    send_sealed(ws, cipher, &paired).await && send_sealed(ws, cipher, &DeskMsg::Ready).await
}

/// Shows the code comparison right away and waits (at most 120 s) until both
/// sides confirmed: the user here accepted, and the phone's `auth` came in,
/// which it sends only once the person tapped "codes match" there. A "no",
/// the timeout or the phone leaving ends it. Returns the request ID and
/// whether both confirmed.
async fn confirm_sas(
    inner: &Inner,
    peer: &PeerHello,
    code: &str,
    ws: &mut WebSocket,
    cipher: &mut LinkCipher,
) -> (String, bool) {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = oneshot::channel();
    if let Some(slot) = inner.pairing.lock().sas.as_mut() {
        slot.request_id = request_id.clone();
        slot.answer = Some(tx);
    }
    inner.emit(Event::PairRequest(PairRequest {
        request_id: request_id.clone(),
        device_name: peer.name.clone(),
        code: code.to_owned(),
    }));
    let mut generation = inner.generation.subscribe();
    let deadline = tokio::time::sleep(limits::SAS_TIMEOUT);
    tokio::pin!(deadline);
    let mut window = RateWindow::default();
    let mut accepted = false;
    let mut authed = false;
    let ok = loop {
        if accepted && authed {
            break true;
        }
        tokio::select! {
            answer = &mut rx, if !accepted => {
                if answer.unwrap_or(false) {
                    accepted = true;
                } else {
                    break false;
                }
            }
            _ = &mut deadline => break false,
            _ = generation.changed() => break false,
            msg = ws.recv() => match msg {
                Some(Ok(Message::Binary(b))) => {
                    if !window.allow() {
                        break false;
                    }
                    match cipher.open_msg::<PhoneMsg>(&b) {
                        // The first sealed frame must be `auth`; after it,
                        // the phone may ping while the user here decides.
                        Ok(PhoneMsg::Auth) if !authed => authed = true,
                        Ok(_) if authed => {}
                        _ => break false,
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => break false,
            },
        }
    };
    (request_id, ok)
}

/// At most [`limits::MAX_MSGS_PER_SEC`] messages per one-second window.
struct RateWindow {
    start: Instant,
    count: u32,
}

impl Default for RateWindow {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            count: 0,
        }
    }
}

impl RateWindow {
    fn allow(&mut self) -> bool {
        if self.start.elapsed() >= Duration::from_secs(1) {
            self.start = Instant::now();
            self.count = 0;
        }
        self.count += 1;
        self.count <= limits::MAX_MSGS_PER_SEC
    }
}

/// After `ready`: heart rates in, commands out, until either side leaves.
async fn session(
    inner: &Inner,
    peer: PeerHello,
    mut ws: WebSocket,
    mut cipher: LinkCipher,
    generation: &mut tokio::sync::watch::Receiver<u64>,
) {
    let (tx, mut rx) = mpsc::channel(16);
    let conn = inner.register_link(&peer.id, tx);
    let guard = LinkGuard {
        inner,
        conn,
        device_id: peer.id.clone(),
    };
    inner.touch_paired(&peer.id);
    inner.set_source(source_for_phone(&peer.id, &peer.name));
    inner.emit_paired();
    log::info!("phone {} connected", peer.name);

    let mut window = RateWindow::default();
    loop {
        tokio::select! {
            incoming = tokio::time::timeout(limits::IDLE_TIMEOUT, ws.recv()) => {
                let msg = match incoming {
                    Ok(Some(Ok(m))) => m,
                    // Silent for 30 s, gone, or broken.
                    _ => break,
                };
                let frame = match msg {
                    Message::Binary(b) => b,
                    Message::Ping(_) | Message::Pong(_) => continue,
                    // Text after the handshake, or a close.
                    _ => break,
                };
                if !window.allow() {
                    log::warn!("phone {} sends too fast; closing", peer.name);
                    break;
                }
                let Ok(msg) = cipher.open_msg::<PhoneMsg>(&frame) else { break };
                match msg {
                    PhoneMsg::Hr { ts, bpm, src, dev, .. } => {
                        if !limits::valid_bpm(bpm) {
                            continue;
                        }
                        let dev = if limits::valid_name(&dev) { dev } else { peer.name.clone() };
                        inner.ingest(&peer.id, HrSample { ts, bpm: bpm as u16, src, dev });
                    }
                    PhoneMsg::Status { watch, ble, battery } => {
                        let detail = status_detail(watch, &ble, battery);
                        inner.update_source(&peer.id, |s| s.detail = Some(detail));
                    }
                    PhoneMsg::Ping => {
                        if !send_sealed(&mut ws, &mut cipher, &DeskMsg::Pong).await {
                            break;
                        }
                    }
                    PhoneMsg::Auth | PhoneMsg::Unknown => {}
                }
            }
            cmd = rx.recv() => match cmd {
                Some(LinkCmd::Send(msg)) => {
                    if !send_sealed(&mut ws, &mut cipher, &msg).await {
                        break;
                    }
                }
                Some(LinkCmd::Close) | None => break,
            },
            _ = generation.changed() => break,
        }
    }
    // Free the source before waiting for the close.
    drop(guard);
    close(&mut ws).await;
    log::info!("phone {} disconnected", peer.name);
}

/// "Watch: running · battery 80% · BLE: Polar H10".
fn status_detail(watch: WatchState, ble: &[String], battery: Option<f64>) -> String {
    let mut out = format!("Watch: {}", watch.as_str());
    if let Some(b) = battery.filter(|b| b.is_finite() && (0.0..=1.0).contains(b)) {
        out.push_str(&format!(" · battery {:.0}%", b * 100.0));
    }
    let names: Vec<&str> = ble
        .iter()
        .take(4)
        .map(|n| limits::truncate_chars(n, limits::MAX_NAME_CHARS))
        .filter(|n| limits::valid_name(n))
        .collect();
    if !names.is_empty() {
        out.push_str(" · BLE: ");
        out.push_str(&names.join(", "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail() {
        assert_eq!(
            status_detail(WatchState::Running, &["Polar H10".into()], Some(0.8)),
            "Watch: running · battery 80% · BLE: Polar H10"
        );
        assert_eq!(
            status_detail(WatchState::Idle, &["\u{7}".into()], Some(f64::NAN)),
            "Watch: idle"
        );
    }
}
