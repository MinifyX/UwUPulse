//! The handshake and the key schedule.
//!
//! ```text
//! phone → desktop   m1 = {"t":"hello","v":1,"id","name","spk","epk","mode"}        known, qr
//!                   m1 = {"t":"hello","v":1,"id","name","spk","commit","mode"}     sas
//! desktop → phone   m2 = {"t":"hello","v":1,"id","name","spk","epk"}
//! phone → desktop   m3 = {"t":"reveal","epk"}                                      sas only
//!
//! commit = SHA-256("UwUPulse-commit-v1" ‖ epk_phone)
//! th  = SHA-256("UwUPulse-link-v1" ‖ m1 ‖ m2)          known, qr
//! th  = SHA-256("UwUPulse-link-v1" ‖ m1 ‖ m2 ‖ m3)     sas
//! ikm = X25519(e_phone, e_desk) ‖ X25519(s_phone, s_desk) ‖ psk
//! okm = HKDF-SHA256(salt = th, ikm, info = "uwupulse link keys", L = 68)
//! k_p2d = okm[0..32], k_d2p = okm[32..64], sas = u32_be(okm[64..68]) mod 10^6
//! ```
//!
//! `m1`, `m2` and `m3` enter the transcript exactly as they went over the
//! wire, so neither side ever re-serialises the other's JSON. The static DH
//! binds the keys to both long-term keys (`known`), the QR secret as `psk`
//! binds them to the person who scanned the code (`qr`), and in `sas` the
//! person comparing the six digits on both screens does the same.
//!
//! In `sas` the phone commits to its ephemeral key before it sees the
//! desktop's and reveals it only after `m2`. Neither side can then pick its
//! key after seeing the other's, so a machine in the middle gets one try at
//! matching the six digits (1 in 10^6), not as many as it can compute.

use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::limits::{valid_device_id, valid_name};
use crate::messages::{ErrorCode, Handshake, Hello};
use crate::{b64_decode_32, b64_encode, random_bytes, PROTOCOL_VERSION};

/// Length of every key, secret and DH output.
pub const KEY_LEN: usize = 32;
/// Prefix of the transcript hash.
pub const TRANSCRIPT_LABEL: &[u8] = b"UwUPulse-link-v1";
/// HKDF `info`.
pub const HKDF_INFO: &[u8] = b"uwupulse link keys";
/// Prefix of the `sas` commitment to the phone's ephemeral key.
pub const COMMIT_LABEL: &[u8] = b"UwUPulse-commit-v1";

/// SHA-256("UwUPulse-commit-v1" ‖ epk): what the phone's `sas` hello carries
/// instead of its ephemeral key.
pub fn commitment(epk: &[u8; KEY_LEN]) -> [u8; 32] {
    Sha256::new()
        .chain_update(COMMIT_LABEL)
        .chain_update(epk)
        .finalize()
        .into()
}

/// How the phone authenticates this time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Paired before: the static keys authenticate.
    Known,
    /// Pairing with a scanned QR code: its secret is the `psk`.
    Qr,
    /// Pairing by comparing a six-digit code.
    Sas,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Known => "known",
            Mode::Qr => "qr",
            Mode::Sas => "sas",
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandshakeError {
    #[error("malformed hello")]
    Malformed,
    #[error("unsupported protocol version")]
    Version,
    #[error("the peer's static key is not the expected one")]
    KeyMismatch,
    #[error("the peer sent a low-order public key")]
    WeakKey,
    #[error("the revealed ephemeral key does not match the commitment")]
    CommitMismatch,
    #[error("the desktop refused: {}", .0.as_str())]
    Refused(ErrorCode),
}

/// A device's long-term X25519 key pair.
#[derive(Clone)]
pub struct StaticKeypair {
    secret: StaticSecret,
    public: PublicKey,
}

impl StaticKeypair {
    pub fn generate() -> Self {
        Self::from_secret_bytes(random_bytes())
    }

    pub fn from_secret_bytes(bytes: [u8; KEY_LEN]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// The secret, for saving it. Clamping happens on use, so these are the
    /// bytes that went in.
    pub fn secret_bytes(&self) -> [u8; KEY_LEN] {
        self.secret.to_bytes()
    }

    pub fn public_bytes(&self) -> [u8; KEY_LEN] {
        self.public.to_bytes()
    }

    pub fn public_b64(&self) -> String {
        b64_encode(self.public.as_bytes())
    }

    pub(crate) fn dh(&self, peer: &[u8; KEY_LEN]) -> Result<[u8; KEY_LEN], HandshakeError> {
        let shared = self.secret.diffie_hellman(&PublicKey::from(*peer));
        // An all-zero result means a low-order point: no secret at all.
        if !shared.was_contributory() {
            return Err(HandshakeError::WeakKey);
        }
        Ok(shared.to_bytes())
    }
}

impl std::fmt::Debug for StaticKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticKeypair")
            .field("public", &self.public_b64())
            .finish_non_exhaustive()
    }
}

/// What both sides know once the handshake is through.
#[derive(Clone, PartialEq, Eq)]
pub struct Keys {
    /// The transcript hash.
    pub th: [u8; 32],
    pub k_p2d: [u8; 32],
    pub k_d2p: [u8; 32],
    /// Six digits, leading zeros kept.
    pub sas: String,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keys")
            .field("sas", &self.sas)
            .finish_non_exhaustive()
    }
}

/// The key schedule's intermediate values, for the test vectors.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Schedule {
    pub ikm: [u8; 96],
    pub okm: [u8; 68],
    pub keys: Keys,
}

/// `m3` is the phone's `reveal`, `sas` only.
pub(crate) fn schedule(
    m1: &[u8],
    m2: &[u8],
    m3: Option<&[u8]>,
    dh_e: &[u8; 32],
    dh_s: &[u8; 32],
    psk: &[u8; 32],
) -> Schedule {
    let mut hash = Sha256::new()
        .chain_update(TRANSCRIPT_LABEL)
        .chain_update(m1)
        .chain_update(m2);
    if let Some(m3) = m3 {
        hash.update(m3);
    }
    let th: [u8; 32] = hash.finalize().into();
    let mut ikm = [0u8; 96];
    ikm[..32].copy_from_slice(dh_e);
    ikm[32..64].copy_from_slice(dh_s);
    ikm[64..].copy_from_slice(psk);
    let mut okm = [0u8; 68];
    Hkdf::<Sha256>::new(Some(&th), &ikm)
        .expand(HKDF_INFO, &mut okm)
        .expect("68 bytes is far below HKDF-SHA256's limit");
    let mut k_p2d = [0u8; 32];
    let mut k_d2p = [0u8; 32];
    k_p2d.copy_from_slice(&okm[..32]);
    k_d2p.copy_from_slice(&okm[32..64]);
    let n = u32::from_be_bytes([okm[64], okm[65], okm[66], okm[67]]);
    let sas = format!("{:06}", n % 1_000_000);
    Schedule {
        ikm,
        okm,
        keys: Keys {
            th,
            k_p2d,
            k_d2p,
            sas,
        },
    }
}

/// A hello whose keys decoded and whose fields are within the limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerHello {
    pub id: String,
    pub name: String,
    pub spk: [u8; KEY_LEN],
    /// The ephemeral key. `None` only in the phone's `sas` hello, where it
    /// comes later in `m3`.
    pub epk: Option<[u8; KEY_LEN]>,
    /// The commitment to the ephemeral key: the phone's `sas` hello only.
    pub commit: Option<[u8; 32]>,
    /// Only in the phone's hello.
    pub mode: Option<Mode>,
}

fn check_hello(h: Hello) -> Result<PeerHello, HandshakeError> {
    if h.v != PROTOCOL_VERSION {
        return Err(HandshakeError::Version);
    }
    if !valid_device_id(&h.id) || !valid_name(&h.name) {
        return Err(HandshakeError::Malformed);
    }
    let spk = b64_decode_32(&h.spk).ok_or(HandshakeError::Malformed)?;
    // A commitment in `sas` (phone), the key itself everywhere else.
    let (epk, commit) = match (h.mode, &h.epk, &h.commit) {
        (Some(Mode::Sas), None, Some(c)) => (
            None,
            Some(b64_decode_32(c).ok_or(HandshakeError::Malformed)?),
        ),
        (mode, Some(e), None) if mode != Some(Mode::Sas) => (
            Some(b64_decode_32(e).ok_or(HandshakeError::Malformed)?),
            None,
        ),
        _ => return Err(HandshakeError::Malformed),
    };
    Ok(PeerHello {
        id: h.id,
        name: h.name,
        spk,
        epk,
        commit,
        mode: h.mode,
    })
}

/// Parses a `hello` text frame. A version other than 1 is reported as such
/// even when the rest of the object has a shape this code doesn't know.
fn parse_hello(text: &str) -> Result<PeerHello, HandshakeError> {
    #[derive(Deserialize)]
    struct Probe {
        t: String,
        v: Option<serde_json::Value>,
    }
    let probe: Probe = serde_json::from_str(text).map_err(|_| HandshakeError::Malformed)?;
    if probe.t != "hello" {
        return Err(HandshakeError::Malformed);
    }
    if probe.v.as_ref().and_then(|v| v.as_u64()) != Some(PROTOCOL_VERSION as u64) {
        return Err(HandshakeError::Version);
    }
    match serde_json::from_str(text) {
        Ok(Handshake::Hello(h)) => check_hello(h),
        _ => Err(HandshakeError::Malformed),
    }
}

/// Parses the phone's `reveal` (`m3`) into its ephemeral key.
fn parse_reveal(text: &str) -> Result<[u8; KEY_LEN], HandshakeError> {
    match serde_json::from_str(text) {
        Ok(Handshake::Reveal { epk }) => b64_decode_32(&epk).ok_or(HandshakeError::Malformed),
        _ => Err(HandshakeError::Malformed),
    }
}

/// The ephemeral part of a hello: the key, or (phone, `sas`) its commitment.
enum Ephemeral<'a> {
    Key(&'a [u8; KEY_LEN]),
    Commit(&'a [u8; 32]),
}

fn hello_json(id: &str, name: &str, spk: &[u8; 32], eph: Ephemeral, mode: Option<Mode>) -> String {
    let (epk, commit) = match eph {
        Ephemeral::Key(k) => (Some(b64_encode(k)), None),
        Ephemeral::Commit(c) => (None, Some(b64_encode(c))),
    };
    serde_json::to_string(&Handshake::Hello(Hello {
        v: PROTOCOL_VERSION,
        id: id.to_owned(),
        name: name.to_owned(),
        spk: b64_encode(spk),
        epk,
        commit,
        mode,
    }))
    .expect("a hello always serialises")
}

/// Which desktop the phone expects, and how it authenticates.
#[derive(Clone)]
pub enum PhoneTarget {
    /// A paired desktop with the static key saved at pairing.
    Known { desk_spk: [u8; KEY_LEN] },
    /// A scanned QR code: the desktop's key and the offer's secret.
    Qr {
        desk_spk: [u8; KEY_LEN],
        secret: [u8; KEY_LEN],
    },
    /// Code comparison: any desktop key, the person checks the code.
    Sas,
}

impl PhoneTarget {
    pub fn mode(&self) -> Mode {
        match self {
            PhoneTarget::Known { .. } => Mode::Known,
            PhoneTarget::Qr { .. } => Mode::Qr,
            PhoneTarget::Sas => Mode::Sas,
        }
    }

    fn psk(&self) -> [u8; KEY_LEN] {
        match self {
            PhoneTarget::Qr { secret, .. } => *secret,
            _ => [0; KEY_LEN],
        }
    }

    fn expected_spk(&self) -> Option<&[u8; KEY_LEN]> {
        match self {
            PhoneTarget::Known { desk_spk } | PhoneTarget::Qr { desk_spk, .. } => Some(desk_spk),
            PhoneTarget::Sas => None,
        }
    }
}

/// The phone's side. The desktop never runs it; it is here for the tests and
/// as the reference for the Swift port.
pub struct PhoneHandshake {
    keypair: StaticKeypair,
    ephemeral: StaticKeypair,
    target: PhoneTarget,
    m1: String,
}

/// What the phone has once the desktop's hello is in.
#[derive(Debug)]
pub struct PhoneFinish {
    /// The desktop's hello.
    pub desk: PeerHello,
    pub keys: Keys,
    /// `sas` only: the `reveal` text frame to send right away, before `auth`.
    pub m3: Option<String>,
}

impl PhoneHandshake {
    pub fn new(keypair: &StaticKeypair, id: &str, name: &str, target: PhoneTarget) -> Self {
        Self::with_ephemeral(keypair, id, name, target, random_bytes())
    }

    /// With a fixed ephemeral secret, for test vectors.
    pub fn with_ephemeral(
        keypair: &StaticKeypair,
        id: &str,
        name: &str,
        target: PhoneTarget,
        ephemeral: [u8; KEY_LEN],
    ) -> Self {
        let ephemeral = StaticKeypair::from_secret_bytes(ephemeral);
        let epk = ephemeral.public_bytes();
        let commit = commitment(&epk);
        let eph = match target {
            PhoneTarget::Sas => Ephemeral::Commit(&commit),
            _ => Ephemeral::Key(&epk),
        };
        let m1 = hello_json(id, name, &keypair.public_bytes(), eph, Some(target.mode()));
        Self {
            keypair: keypair.clone(),
            ephemeral,
            target,
            m1,
        }
    }

    /// The text frame to send first.
    pub fn m1(&self) -> &str {
        &self.m1
    }

    /// Takes the desktop's answer: its hello, the keys and, in `sas`, the
    /// `reveal` to send next.
    pub fn finish(self, m2: &str) -> Result<PhoneFinish, HandshakeError> {
        if let Ok(Handshake::Error { code }) = serde_json::from_str::<Handshake>(m2) {
            return Err(HandshakeError::Refused(code));
        }
        let desk = parse_hello(m2)?;
        let desk_epk = match (desk.mode, desk.epk) {
            (None, Some(e)) => e,
            _ => return Err(HandshakeError::Malformed),
        };
        if let Some(expected) = self.target.expected_spk() {
            if expected != &desk.spk {
                return Err(HandshakeError::KeyMismatch);
            }
        }
        let dh_e = self.ephemeral.dh(&desk_epk)?;
        let dh_s = self.keypair.dh(&desk.spk)?;
        let m3 = matches!(self.target, PhoneTarget::Sas).then(|| {
            serde_json::to_string(&Handshake::Reveal {
                epk: self.ephemeral.public_b64(),
            })
            .expect("a reveal always serialises")
        });
        let keys = schedule(
            self.m1.as_bytes(),
            m2.as_bytes(),
            m3.as_deref().map(str::as_bytes),
            &dh_e,
            &dh_s,
            &self.target.psk(),
        )
        .keys;
        Ok(PhoneFinish { desk, keys, m3 })
    }
}

/// The desktop's side: parse `m1`, decide (outside, with the stored
/// devices and offers) which `psk` applies, answer with `m2`, then (`sas`:
/// after the phone's `m3`) derive the keys.
pub struct DesktopHandshake {
    m1: String,
    hello: PeerHello,
    mode: Mode,
}

impl DesktopHandshake {
    /// Parses the phone's hello. [`HandshakeError::Version`] is the one error
    /// worth answering (`{"t":"error","code":"version"}`); everything else
    /// just closes.
    pub fn receive(m1: &str) -> Result<Self, HandshakeError> {
        let hello = parse_hello(m1)?;
        let mode = hello.mode.ok_or(HandshakeError::Malformed)?;
        Ok(Self {
            m1: m1.to_owned(),
            hello,
            mode,
        })
    }

    pub fn hello(&self) -> &PeerHello {
        &self.hello
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Answers with `m2`. `psk` is the QR secret for `qr` and 32 zero bytes
    /// otherwise. The keys come from [`DesktopPending::finish`].
    pub fn respond(
        &self,
        keypair: &StaticKeypair,
        id: &str,
        name: &str,
        psk: &[u8; KEY_LEN],
    ) -> Result<(String, DesktopPending), HandshakeError> {
        self.respond_with_ephemeral(keypair, id, name, psk, random_bytes())
    }

    /// With a fixed ephemeral secret, for test vectors.
    pub fn respond_with_ephemeral(
        &self,
        keypair: &StaticKeypair,
        id: &str,
        name: &str,
        psk: &[u8; KEY_LEN],
        ephemeral: [u8; KEY_LEN],
    ) -> Result<(String, DesktopPending), HandshakeError> {
        let ephemeral = StaticKeypair::from_secret_bytes(ephemeral);
        let dh_s = keypair.dh(&self.hello.spk)?;
        // Known now in `known`/`qr`; in `sas` only after the reveal.
        let dh_e = match &self.hello.epk {
            Some(epk) => Some(ephemeral.dh(epk)?),
            None => None,
        };
        let m2 = hello_json(
            id,
            name,
            &keypair.public_bytes(),
            Ephemeral::Key(&ephemeral.public_bytes()),
            None,
        );
        let pending = DesktopPending {
            m1: self.m1.clone(),
            m2: m2.clone(),
            ephemeral,
            dh_e,
            dh_s,
            psk: *psk,
            commit: self.hello.commit,
        };
        Ok((m2, pending))
    }
}

/// The desktop after `m2`: waiting for nothing (`known`, `qr`) or for the
/// phone's `reveal` (`sas`).
pub struct DesktopPending {
    m1: String,
    m2: String,
    ephemeral: StaticKeypair,
    dh_e: Option<[u8; KEY_LEN]>,
    dh_s: [u8; KEY_LEN],
    psk: [u8; KEY_LEN],
    commit: Option<[u8; 32]>,
}

impl std::fmt::Debug for DesktopPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopPending")
            .field("needs_reveal", &self.needs_reveal())
            .finish_non_exhaustive()
    }
}

impl DesktopPending {
    /// Whether the phone's `m3` (`reveal`) has to come in first (`sas`).
    pub fn needs_reveal(&self) -> bool {
        self.commit.is_some()
    }

    /// The keys. `m3` is the phone's `reveal` text frame in `sas` and `None`
    /// otherwise; a revealed key that doesn't match the commitment is
    /// [`HandshakeError::CommitMismatch`].
    pub fn finish(self, m3: Option<&str>) -> Result<Keys, HandshakeError> {
        let dh_e = match (self.commit, self.dh_e, m3) {
            (None, Some(dh_e), None) => dh_e,
            (Some(commit), None, Some(m3)) => {
                let epk = parse_reveal(m3)?;
                if !bool::from(commitment(&epk).ct_eq(&commit)) {
                    return Err(HandshakeError::CommitMismatch);
                }
                self.ephemeral.dh(&epk)?
            }
            _ => return Err(HandshakeError::Malformed),
        };
        Ok(schedule(
            self.m1.as_bytes(),
            self.m2.as_bytes(),
            m3.map(str::as_bytes),
            &dh_e,
            &self.dh_s,
            &self.psk,
        )
        .keys)
    }
}

/// The text frame that refuses a hello.
pub fn refusal(code: ErrorCode) -> String {
    serde_json::to_string(&Handshake::Error { code }).expect("an error always serialises")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE_ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    const DESK_ID: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";

    fn run(target: PhoneTarget, desk: &StaticKeypair, psk: [u8; 32]) -> (Keys, Keys) {
        let phone_key = StaticKeypair::generate();
        let sas = matches!(target, PhoneTarget::Sas);
        let phone = PhoneHandshake::new(&phone_key, PHONE_ID, "iPhone", target);
        let d = DesktopHandshake::receive(phone.m1()).unwrap();
        assert_eq!(d.hello().spk, phone_key.public_bytes());
        let (m2, pending) = d.respond(desk, DESK_ID, "Desk", &psk).unwrap();
        assert_eq!(pending.needs_reveal(), sas);
        let done = phone.finish(&m2).unwrap();
        assert_eq!(done.m3.is_some(), sas);
        let dk = pending.finish(done.m3.as_deref()).unwrap();
        (done.keys, dk)
    }

    #[test]
    fn all_modes_agree() {
        let desk = StaticKeypair::generate();
        let (a, b) = run(
            PhoneTarget::Known {
                desk_spk: desk.public_bytes(),
            },
            &desk,
            [0; 32],
        );
        assert_eq!(a, b);
        let secret = random_bytes();
        let (a, b) = run(
            PhoneTarget::Qr {
                desk_spk: desk.public_bytes(),
                secret,
            },
            &desk,
            secret,
        );
        assert_eq!(a, b);
        let (a, b) = run(PhoneTarget::Sas, &desk, [0; 32]);
        assert_eq!(a, b);
        assert_eq!(a.sas.len(), 6);
    }

    #[test]
    fn wrong_psk_gives_other_keys() {
        let desk = StaticKeypair::generate();
        let (a, b) = run(
            PhoneTarget::Qr {
                desk_spk: desk.public_bytes(),
                secret: [1; 32],
            },
            &desk,
            [2; 32],
        );
        assert_ne!(a.k_p2d, b.k_p2d);
    }

    #[test]
    fn phone_rejects_other_desktop_key() {
        let desk = StaticKeypair::generate();
        let phone_key = StaticKeypair::generate();
        let phone = PhoneHandshake::new(
            &phone_key,
            PHONE_ID,
            "iPhone",
            PhoneTarget::Known {
                desk_spk: StaticKeypair::generate().public_bytes(),
            },
        );
        let d = DesktopHandshake::receive(phone.m1()).unwrap();
        let (m2, _) = d.respond(&desk, DESK_ID, "Desk", &[0; 32]).unwrap();
        assert_eq!(phone.finish(&m2).unwrap_err(), HandshakeError::KeyMismatch);
    }

    #[test]
    fn sas_hello_commits_instead_of_epk() {
        let phone = PhoneHandshake::new(
            &StaticKeypair::generate(),
            PHONE_ID,
            "iPhone",
            PhoneTarget::Sas,
        );
        let v: serde_json::Value = serde_json::from_str(phone.m1()).unwrap();
        assert!(v.get("epk").is_none());
        assert_eq!(v["commit"].as_str().unwrap().len(), 43);
        let d = DesktopHandshake::receive(phone.m1()).unwrap();
        assert_eq!(d.hello().epk, None);
        assert!(d.hello().commit.is_some());
    }

    #[test]
    fn sas_commitment_mismatch_is_refused() {
        let desk = StaticKeypair::generate();
        let phone = PhoneHandshake::new(
            &StaticKeypair::generate(),
            PHONE_ID,
            "iPhone",
            PhoneTarget::Sas,
        );
        let d = DesktopHandshake::receive(phone.m1()).unwrap();
        let respond = || d.respond(&desk, DESK_ID, "Desk", &[0; 32]).unwrap().1;
        // Another key than the committed one.
        let other = StaticKeypair::generate().public_b64();
        let forged = format!(r#"{{"t":"reveal","epk":"{other}"}}"#);
        assert_eq!(
            respond().finish(Some(&forged)).unwrap_err(),
            HandshakeError::CommitMismatch
        );
        // No reveal, or junk instead of one.
        assert_eq!(
            respond().finish(None).unwrap_err(),
            HandshakeError::Malformed
        );
        for junk in [
            "",
            "{}",
            r#"{"t":"reveal"}"#,
            r#"{"t":"reveal","epk":"AAAA"}"#,
            r#"{"t":"hello","epk":"AAAA"}"#,
        ] {
            assert_eq!(
                respond().finish(Some(junk)).unwrap_err(),
                HandshakeError::Malformed,
                "{junk}"
            );
        }
        // The real one still works, and an `m3` outside `sas` doesn't.
        let (m2, pending) = d.respond(&desk, DESK_ID, "Desk", &[0; 32]).unwrap();
        let done = phone.finish(&m2).unwrap();
        assert_eq!(pending.finish(done.m3.as_deref()).unwrap(), done.keys);

        let known = PhoneHandshake::new(
            &StaticKeypair::generate(),
            PHONE_ID,
            "iPhone",
            PhoneTarget::Known {
                desk_spk: desk.public_bytes(),
            },
        );
        let d = DesktopHandshake::receive(known.m1()).unwrap();
        let (_, pending) = d.respond(&desk, DESK_ID, "Desk", &[0; 32]).unwrap();
        assert_eq!(
            pending.finish(done.m3.as_deref()).unwrap_err(),
            HandshakeError::Malformed
        );
    }

    #[test]
    fn transcript_covers_m3() {
        let m = b"x";
        let a = schedule(m, m, None, &[1; 32], &[2; 32], &[0; 32]).keys;
        let b = schedule(m, m, Some(b"y"), &[1; 32], &[2; 32], &[0; 32]).keys;
        assert_ne!(a.th, b.th);
    }

    #[test]
    fn refusal_reaches_phone() {
        let phone = PhoneHandshake::new(
            &StaticKeypair::generate(),
            PHONE_ID,
            "iPhone",
            PhoneTarget::Sas,
        );
        assert_eq!(
            phone.finish(&refusal(ErrorCode::Busy)).unwrap_err(),
            HandshakeError::Refused(ErrorCode::Busy)
        );
    }

    #[test]
    fn hostile_hellos() {
        let k = b64_encode(&StaticKeypair::generate().public_bytes());
        let ok = format!(
            r#"{{"t":"hello","v":1,"id":"{PHONE_ID}","name":"x","spk":"{k}","epk":"{k}","mode":"qr"}}"#
        );
        assert!(DesktopHandshake::receive(&ok).is_ok());
        let sas = format!(
            r#"{{"t":"hello","v":1,"id":"{PHONE_ID}","name":"x","spk":"{k}","commit":"{k}","mode":"sas"}}"#
        );
        assert!(DesktopHandshake::receive(&sas).is_ok());
        let cases = [
            ("", HandshakeError::Malformed),
            ("null", HandshakeError::Malformed),
            ("[]", HandshakeError::Malformed),
            (r#"{"t":"hello"}"#, HandshakeError::Version),
            (
                r#"{"t":"hello","v":2,"whatever":true}"#,
                HandshakeError::Version,
            ),
            (r#"{"t":"hello","v":1}"#, HandshakeError::Malformed),
            (r#"{"t":"auth","v":1}"#, HandshakeError::Malformed),
        ];
        for (text, err) in cases {
            assert_eq!(DesktopHandshake::receive(text).err(), Some(err), "{text}");
        }
        let bad = [
            ok.replace("\"qr\"", "\"nope\""),
            ok.replace(",\"mode\":\"qr\"", ""),
            // `sas` needs the commitment and no key; the others the reverse.
            ok.replace("\"qr\"", "\"sas\""),
            sas.replace("\"sas\"", "\"known\""),
            sas.replace("\"commit\"", "\"epk\""),
            sas.replace(",\"mode\"", &format!(",\"epk\":\"{k}\",\"mode\"")),
            sas.replace(&format!("\"commit\":\"{k}\""), "\"commit\":\"AAAA\""),
            ok.replace(PHONE_ID, "not-an-id"),
            ok.replace("\"x\"", "\"\""),
            ok.replace(&format!("\"spk\":\"{k}\""), "\"spk\":\"AAAA\""),
            ok.replace(&format!("\"epk\":\"{k}\""), &format!("\"epk\":\"{k}=\"")),
        ];
        for text in bad {
            assert!(DesktopHandshake::receive(&text).is_err(), "{text}");
        }
    }

    #[test]
    fn low_order_key_is_refused() {
        let zero = b64_encode(&[0; 32]);
        let m1 = format!(
            r#"{{"t":"hello","v":1,"id":"{PHONE_ID}","name":"x","spk":"{zero}","epk":"{zero}","mode":"qr"}}"#
        );
        let d = DesktopHandshake::receive(&m1).unwrap();
        assert_eq!(
            d.respond(&StaticKeypair::generate(), DESK_ID, "Desk", &[0; 32])
                .unwrap_err(),
            HandshakeError::WeakKey
        );
    }
}
