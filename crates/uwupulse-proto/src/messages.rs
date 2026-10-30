//! The JSON messages of the link.
//!
//! Every message is an object whose `t` names its type. The two `hello`s, the
//! phone's `reveal` (only in `sas`) and a refusal travel as text frames before
//! the keys exist ([`Handshake`]);
//! everything else is sealed ([`PhoneMsg`] one way, [`DeskMsg`] the other).
//! Types neither side knows yet parse as `Unknown`, so a newer phone can add
//! a message without breaking an older desktop.

use serde::{Deserialize, Serialize};

use crate::handshake::Mode;

/// The body of both `hello`s. `mode` is only in the phone's. Exactly one
/// of `epk` and `commit` is set: `commit` in the phone's hello in `sas`,
/// `epk` everywhere else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub v: u32,
    pub id: String,
    pub name: String,
    /// The sender's static X25519 public key, b64.
    pub spk: String,
    /// The sender's ephemeral X25519 public key, b64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epk: Option<String>,
    /// `sas` only: b64 SHA-256("UwUPulse-commit-v1" ‖ epk) of the phone's
    /// ephemeral key, revealed in `m3` once the desktop's `m2` is out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
}

/// The text frames before the keys exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Handshake {
    Hello(Hello),
    /// `sas` only, phone → desktop after `m2`: the ephemeral key behind the
    /// commitment in `m1`.
    Reveal {
        epk: String,
    },
    /// The desktop's refusal; it closes the connection right after.
    Error {
        code: ErrorCode,
    },
}

/// Why the desktop refused or ended a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The phone speaks another protocol version.
    Version,
    /// `known`, but the desktop does not know this device (or its key).
    UnknownDevice,
    /// `qr`, but there is no open pairing offer.
    NoOffer,
    /// `sas`, but another code comparison is already waiting.
    Busy,
    /// The user declined the code or did not answer in time, the `sas`
    /// commitment did not match, or the phone's ID is paired with another key.
    Rejected,
    #[serde(other)]
    Unknown,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Version => "version",
            ErrorCode::UnknownDevice => "unknown_device",
            ErrorCode::NoOffer => "no_offer",
            ErrorCode::Busy => "busy",
            ErrorCode::Rejected => "rejected",
            ErrorCode::Unknown => "unknown",
        }
    }
}

/// Where a heart rate comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Src {
    Watch,
    Ble,
}

impl Src {
    pub fn as_str(self) -> &'static str {
        match self {
            Src::Watch => "watch",
            Src::Ble => "ble",
        }
    }

    pub fn parse(s: &str) -> Option<Src> {
        match s {
            "watch" => Some(Src::Watch),
            "ble" => Some(Src::Ble),
            _ => None,
        }
    }
}

/// The workout on the watch, as the phone sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchState {
    Unavailable,
    Idle,
    Running,
    #[serde(other)]
    Unknown,
}

impl WatchState {
    pub fn as_str(self) -> &'static str {
        match self {
            WatchState::Unavailable => "unavailable",
            WatchState::Idle => "idle",
            WatchState::Running => "running",
            WatchState::Unknown => "unknown",
        }
    }
}

/// Starting or stopping the workout on the watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchAction {
    Start,
    Stop,
}

/// Phone → desktop, sealed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum PhoneMsg {
    /// The first sealed message: proves the phone has the keys.
    Auth,
    Hr {
        /// Unix milliseconds on the phone.
        ts: i64,
        /// Wide on purpose: an out-of-range value is dropped, not a parse
        /// error that ends the link.
        bpm: i64,
        src: Src,
        dev: String,
        /// RR intervals in milliseconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rr: Option<Vec<u32>>,
    },
    Status {
        watch: WatchState,
        #[serde(default)]
        ble: Vec<String>,
        /// The watch's battery, 0..1.
        #[serde(default)]
        battery: Option<f64>,
    },
    Ping,
    #[serde(other)]
    Unknown,
}

/// Desktop → phone, sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum DeskMsg {
    /// The phone is now paired with this desktop (`qr` and `sas`).
    Paired {
        id: String,
        name: String,
    },
    /// Heart rates may flow.
    Ready,
    Pong,
    Cmd {
        action: WatchAction,
    },
    Error {
        code: ErrorCode,
    },
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        let hr = PhoneMsg::Hr {
            ts: 1_700_000_000_000,
            bpm: 72,
            src: Src::Watch,
            dev: "Apple Watch".into(),
            rr: None,
        };
        assert_eq!(
            serde_json::to_string(&hr).unwrap(),
            r#"{"t":"hr","ts":1700000000000,"bpm":72,"src":"watch","dev":"Apple Watch"}"#
        );
        assert_eq!(
            serde_json::to_string(&PhoneMsg::Auth).unwrap(),
            r#"{"t":"auth"}"#
        );
        assert_eq!(
            serde_json::to_string(&DeskMsg::Cmd {
                action: WatchAction::Start
            })
            .unwrap(),
            r#"{"t":"cmd","action":"start"}"#
        );
        assert_eq!(
            serde_json::to_string(&Handshake::Error {
                code: ErrorCode::UnknownDevice
            })
            .unwrap(),
            r#"{"t":"error","code":"unknown_device"}"#
        );
        assert_eq!(
            serde_json::to_string(&Handshake::Reveal { epk: "AA".into() }).unwrap(),
            r#"{"t":"reveal","epk":"AA"}"#
        );
    }

    #[test]
    fn unknown_types_parse() {
        let m: PhoneMsg = serde_json::from_str(r#"{"t":"future","x":1}"#).unwrap();
        assert_eq!(m, PhoneMsg::Unknown);
        let s: PhoneMsg =
            serde_json::from_str(r#"{"t":"status","watch":"sleeping","ble":[],"battery":null}"#)
                .unwrap();
        assert!(matches!(
            s,
            PhoneMsg::Status {
                watch: WatchState::Unknown,
                ..
            }
        ));
        let rr: PhoneMsg = serde_json::from_str(
            r#"{"t":"hr","ts":1,"bpm":300,"src":"ble","dev":"Strap","rr":[800,812]}"#,
        )
        .unwrap();
        assert!(matches!(rr, PhoneMsg::Hr { bpm: 300, .. }));
    }
}
