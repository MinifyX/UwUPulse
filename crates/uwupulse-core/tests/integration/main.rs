//! End-to-end tests against a real server on 127.0.0.1:0, one temp folder
//! per test. One binary (one link step) for all of them, to keep CI fast.

mod api;
mod link;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use uwupulse_core::proto::{DeskMsg, PhoneMsg};
use uwupulse_core::proto::{
    HandshakeError, Keys, LinkCipher, PeerHello, PhoneHandshake, PhoneTarget, StaticKeypair,
};
use uwupulse_core::{Event, Pulse, PulseOptions};

/// Upper bound for anything we wait on; a hang fails instead of blocking CI.
pub const WAIT: Duration = Duration::from_secs(5);

pub struct Env {
    pub pulse: Pulse,
    pub port: u16,
    pub token: String,
    pub dir: TempDir,
}

pub async fn start() -> Env {
    start_with(|_| {}).await
}

/// Like [`start`], with options adjusted by `f`.
pub async fn start_with(f: impl FnOnce(&mut PulseOptions)) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    // Loopback only: tests must not open ports on the LAN.
    std::fs::write(config.join("settings.json"), r#"{"lanEnabled":false}"#).unwrap();
    let mut opts = PulseOptions::new(config, dir.path().join("data"));
    opts.port_override = Some(0);
    opts.bonjour = false;
    opts.device_name = Some("Test Desk".into());
    f(&mut opts);
    let pulse = Pulse::start(opts).await.unwrap();
    let info = pulse.server_info();
    assert!(info.error.is_none(), "{:?}", info.error);
    Env {
        port: info.port,
        token: info.token,
        pulse,
        dir,
    }
}

/// The first event `f` accepts, within [`WAIT`].
pub async fn wait_for<T>(
    rx: &mut broadcast::Receiver<Event>,
    mut f: impl FnMut(&Event) -> Option<T>,
) -> T {
    tokio::time::timeout(WAIT, async {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Some(v) = f(&ev) {
                        return v;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("event channel: {e}"),
            }
        }
    })
    .await
    .expect("event did not arrive")
}

pub type Ws = WebSocketStream<TcpStream>;

pub async fn ws_connect(
    port: u16,
    path: &str,
) -> Result<Ws, tokio_tungstenite::tungstenite::Error> {
    ws_connect_with(port, path, &[]).await
}

/// Like [`ws_connect`], with extra request headers.
pub async fn ws_connect_with(
    port: u16,
    path: &str,
    headers: &[(&'static str, &str)],
) -> Result<Ws, tokio_tungstenite::tungstenite::Error> {
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut req = format!("ws://127.0.0.1:{port}{path}")
        .into_client_request()
        .unwrap();
    for (k, v) in headers {
        req.headers_mut().insert(*k, v.parse().unwrap());
    }
    tokio_tungstenite::client_async(req, tcp)
        .await
        .map(|(ws, _)| ws)
}

/// The HTTP status a refused WebSocket upgrade got.
pub fn refused_status(r: Result<Ws, tokio_tungstenite::tungstenite::Error>) -> Option<u16> {
    match r {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => Some(resp.status().as_u16()),
        _ => None,
    }
}

/// The next data message, skipping pings; `None` once the socket closes.
pub async fn next_msg(ws: &mut Ws) -> Option<Message> {
    tokio::time::timeout(WAIT, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return None,
                Some(Ok(m)) => return Some(m),
            }
        }
    })
    .await
    .expect("socket neither answered nor closed")
}

/// A phone on the link, after `auth`.
pub struct Phone {
    pub ws: Ws,
    pub cipher: LinkCipher,
    pub desk: PeerHello,
    pub keys: Keys,
}

pub fn phone_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Runs the phone side of the handshake and sends `auth`. `Err` carries the
/// refusal code (or a short reason).
pub async fn link(
    port: u16,
    key: &StaticKeypair,
    id: &str,
    target: PhoneTarget,
) -> Result<Phone, String> {
    let mut phone = handshake(port, key, id, target).await?;
    phone.send(&PhoneMsg::Auth).await;
    Ok(phone)
}

/// The handshake up to the keys (in `sas` including the `reveal`), without
/// `auth`.
pub async fn handshake(
    port: u16,
    key: &StaticKeypair,
    id: &str,
    target: PhoneTarget,
) -> Result<Phone, String> {
    let mut ws = ws_connect(port, "/link").await.map_err(|e| e.to_string())?;
    let hs = PhoneHandshake::new(key, id, "Test iPhone", target);
    ws.send(Message::text(hs.m1())).await.unwrap();
    let m2 = match next_msg(&mut ws).await {
        Some(Message::Text(t)) => t.as_str().to_owned(),
        other => return Err(format!("no hello: {other:?}")),
    };
    let done = match hs.finish(&m2) {
        Ok(v) => v,
        Err(HandshakeError::Refused(code)) => return Err(code.as_str().to_owned()),
        Err(e) => return Err(format!("{e:?}")),
    };
    if let Some(m3) = &done.m3 {
        ws.send(Message::text(m3.as_str())).await.unwrap();
    }
    Ok(Phone {
        ws,
        cipher: LinkCipher::phone(&done.keys),
        desk: done.desk,
        keys: done.keys,
    })
}

impl Phone {
    pub async fn send(&mut self, msg: &PhoneMsg) {
        let frame = self.cipher.seal_msg(msg).unwrap();
        self.ws.send(Message::binary(frame)).await.unwrap();
    }

    /// The next sealed message; `None` once closed.
    pub async fn recv(&mut self) -> Option<DeskMsg> {
        match next_msg(&mut self.ws).await? {
            Message::Binary(b) => Some(self.cipher.open_msg(&b).expect("desk frame")),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// True once the desktop has closed the socket (within [`WAIT`]).
    pub async fn closed(&mut self) -> bool {
        tokio::time::timeout(WAIT, async {
            loop {
                match self.ws.next().await {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return true,
                    Some(Ok(_)) => {}
                }
            }
        })
        .await
        .unwrap_or(false)
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub head: String,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap()
    }
}

/// A bare HTTP/1.1 request; enough for our handlers, which all send a
/// `Content-Length`.
pub async fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)]) -> HttpResponse {
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    tokio::time::timeout(WAIT, tcp.read_to_end(&mut raw))
        .await
        .expect("HTTP response")
        .unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("header end");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap();
    HttpResponse {
        status,
        head,
        body: raw[split + 4..].to_vec(),
    }
}
