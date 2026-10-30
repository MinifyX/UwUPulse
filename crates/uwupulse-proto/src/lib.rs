//! The UwUPulse link protocol, version 1, without any I/O.
//!
//! An iPhone talks to the desktop over one WebSocket. Plain-text JSON frames
//! (`hello` both ways, in `sas` also the phone's `reveal`) set up the keys; everything after that is a
//! binary frame sealed with ChaCha20-Poly1305. `docs/plan.md` is the spec,
//! `docs/protocol-vectors.json` pins it down byte for byte for the Swift side.
//!
//! - [`handshake`] — both roles of the handshake and the key schedule
//! - [`frame`] — sealing and opening frames with per-direction counters
//! - [`messages`] — the JSON messages, before and after the handshake
//! - [`pairing`] — the `uwupulse://pair` URI the QR code carries
//! - [`limits`] — the protocol's limits and the checks for them
//! - [`hrm`] — the BLE Heart Rate Measurement characteristic (0x2A37)
//!
//! Every parser here takes attacker-controlled bytes: none of them panics,
//! they return an error instead.

pub mod frame;
pub mod handshake;
pub mod hrm;
pub mod limits;
pub mod messages;
pub mod pairing;

#[cfg(test)]
mod vectors;

pub use frame::{FrameError, LinkCipher};
pub use handshake::{
    commitment, refusal, DesktopHandshake, DesktopPending, HandshakeError, Keys, Mode, PeerHello,
    PhoneFinish, PhoneHandshake, PhoneTarget, StaticKeypair, KEY_LEN,
};
pub use messages::{DeskMsg, ErrorCode, Hello, PhoneMsg, Src, WatchAction, WatchState};
pub use pairing::PairingUri;

use base64::Engine;

/// The protocol version both `hello`s carry.
pub const PROTOCOL_VERSION: u32 = 1;

/// base64url without padding, the only base64 the protocol uses.
pub fn b64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Decodes base64url without padding. Padding or the standard alphabet are
/// errors, so one key has exactly one spelling.
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()
}

/// Decodes base64url into exactly 32 bytes (a key or the QR secret).
pub fn b64_decode_32(s: &str) -> Option<[u8; 32]> {
    // 32 bytes are 43 characters; anything longer is not worth decoding.
    if s.len() != 43 {
        return None;
    }
    b64_decode(s)?.try_into().ok()
}

/// Fills a buffer from the operating system's random source.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    // The OS RNG failing is not something to continue from: every key and
    // secret would be predictable.
    getrandom::fill(&mut out).expect("operating system random source failed");
    out
}
