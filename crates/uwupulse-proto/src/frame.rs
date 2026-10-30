//! Sealed frames after the handshake.
//!
//! Each direction has its own key and its own counter, starting at 0. The
//! nonce is four zero bytes and the counter as u64 little-endian; there is no
//! associated data. Both sides count, nothing goes over the wire but the
//! ciphertext and its tag, so a replayed, dropped or reordered frame fails to
//! open and ends the connection.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::handshake::Keys;
use crate::limits::MAX_FRAME_BYTES;

/// Poly1305 tag length.
pub const TAG_LEN: usize = 16;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame too large")]
    TooLarge,
    #[error("frame does not open")]
    Decrypt,
    #[error("frame is not a valid message")]
    Message,
    #[error("counter exhausted")]
    Exhausted,
}

/// The nonce for a counter value.
pub fn nonce(counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_le_bytes());
    n
}

struct Direction {
    aead: ChaCha20Poly1305,
    counter: u64,
}

impl Direction {
    fn new(key: &[u8; 32]) -> Self {
        Self {
            aead: ChaCha20Poly1305::new(Key::from_slice(key)),
            counter: 0,
        }
    }

    fn next_nonce(&mut self) -> Result<[u8; 12], FrameError> {
        let n = nonce(self.counter);
        self.counter = self.counter.checked_add(1).ok_or(FrameError::Exhausted)?;
        Ok(n)
    }
}

/// Both directions of one link, from one side's point of view.
pub struct LinkCipher {
    send: Direction,
    recv: Direction,
}

impl LinkCipher {
    /// The phone sends with `k_p2d` and receives with `k_d2p`.
    pub fn phone(keys: &Keys) -> Self {
        Self {
            send: Direction::new(&keys.k_p2d),
            recv: Direction::new(&keys.k_d2p),
        }
    }

    /// The desktop the other way round.
    pub fn desktop(keys: &Keys) -> Self {
        Self {
            send: Direction::new(&keys.k_d2p),
            recv: Direction::new(&keys.k_p2d),
        }
    }

    /// Seals the next outgoing frame.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, FrameError> {
        if plaintext.len() + TAG_LEN > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge);
        }
        let n = self.send.next_nonce()?;
        self.send
            .aead
            .encrypt(Nonce::from_slice(&n), plaintext)
            .map_err(|_| FrameError::TooLarge)
    }

    /// Opens the next incoming frame. The counter only moves on success, but
    /// any failure is meant to end the connection anyway.
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, FrameError> {
        if frame.len() > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge);
        }
        if frame.len() < TAG_LEN {
            return Err(FrameError::Decrypt);
        }
        let n = nonce(self.recv.counter);
        let plain = self
            .recv
            .aead
            .decrypt(Nonce::from_slice(&n), frame)
            .map_err(|_| FrameError::Decrypt)?;
        self.recv.next_nonce()?;
        Ok(plain)
    }

    /// Seals a message as JSON.
    pub fn seal_msg<T: Serialize>(&mut self, msg: &T) -> Result<Vec<u8>, FrameError> {
        let json = serde_json::to_vec(msg).map_err(|_| FrameError::Message)?;
        self.seal(&json)
    }

    /// Opens a frame and parses the JSON inside.
    pub fn open_msg<T: DeserializeOwned>(&mut self, frame: &[u8]) -> Result<T, FrameError> {
        let plain = self.open(frame)?;
        serde_json::from_slice(&plain).map_err(|_| FrameError::Message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{DeskMsg, PhoneMsg};

    fn pair() -> (LinkCipher, LinkCipher) {
        let keys = Keys {
            th: [0; 32],
            k_p2d: [1; 32],
            k_d2p: [2; 32],
            sas: "000000".into(),
        };
        (LinkCipher::phone(&keys), LinkCipher::desktop(&keys))
    }

    #[test]
    fn round_trip_both_ways() {
        let (mut phone, mut desk) = pair();
        for _ in 0..3 {
            let f = phone.seal_msg(&PhoneMsg::Ping).unwrap();
            assert_eq!(desk.open_msg::<PhoneMsg>(&f).unwrap(), PhoneMsg::Ping);
            let f = desk.seal_msg(&DeskMsg::Pong).unwrap();
            assert_eq!(phone.open_msg::<DeskMsg>(&f).unwrap(), DeskMsg::Pong);
        }
    }

    #[test]
    fn replay_reorder_and_tamper_fail() {
        let (mut phone, mut desk) = pair();
        let a = phone.seal(b"a").unwrap();
        let b = phone.seal(b"b").unwrap();
        assert_eq!(desk.open(&b), Err(FrameError::Decrypt));
        assert_eq!(desk.open(&a).unwrap(), b"a");
        assert_eq!(desk.open(&a), Err(FrameError::Decrypt));
        let mut bad = b.clone();
        bad[0] ^= 1;
        assert_eq!(desk.open(&bad), Err(FrameError::Decrypt));
        assert_eq!(desk.open(&[]), Err(FrameError::Decrypt));
        assert_eq!(desk.open(&[0; 15]), Err(FrameError::Decrypt));
        assert_eq!(
            desk.open(&vec![0; MAX_FRAME_BYTES + 1]),
            Err(FrameError::TooLarge)
        );
        assert_eq!(desk.open(&b).unwrap(), b"b");
        // A frame that is not JSON opens but is no message.
        let junk = phone.seal(b"\xff").unwrap();
        assert_eq!(desk.open_msg::<PhoneMsg>(&junk), Err(FrameError::Message));
    }

    #[test]
    fn oversized_plaintext_is_refused() {
        let (mut phone, _) = pair();
        assert_eq!(
            phone.seal(&vec![0; MAX_FRAME_BYTES]),
            Err(FrameError::TooLarge)
        );
    }
}
