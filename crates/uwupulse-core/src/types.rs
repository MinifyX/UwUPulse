//! The types the app and the API see. All camelCase on the wire, exactly as
//! `docs/plan.md` lists them for the TypeScript side.

use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use uwupulse_proto::Src;

/// One heart rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HrSample {
    /// Unix milliseconds.
    pub ts: i64,
    pub bpm: u16,
    pub src: Src,
    /// The device that measured it ("Apple Watch", "Polar H10 1234ABCD").
    pub dev: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Phone,
    Ble,
}

/// Something heart rates come from right now: a connected phone or a strap
/// at this desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub kind: SourceKind,
    pub name: String,
    pub connected: bool,
    pub last_bpm: Option<u16>,
    pub last_ts: Option<i64>,
    /// e.g. "Watch: running".
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub port: u16,
    pub lan_enabled: bool,
    pub token: String,
    /// Base URLs: `http://127.0.0.1:<port>`, then one per LAN address.
    pub urls: Vec<String>,
    /// Why the server isn't running (port taken, …); `None` while it runs.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub session_id: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub current: Option<HrSample>,
    pub live: bool,
    pub sources: Vec<Source>,
    pub server: ServerInfo,
    pub recording: Option<Recording>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub paired_at: i64,
    pub last_seen: Option<i64>,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingOffer {
    pub uri: String,
    pub qr_svg: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequest {
    pub request_id: String,
    pub device_name: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairDone {
    pub request_id: String,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BleDevice {
    pub id: String,
    pub name: String,
    pub rssi: Option<i16>,
    pub connected: bool,
    pub connecting: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub min_bpm: u16,
    pub max_bpm: u16,
    pub avg_bpm: f64,
    pub sample_count: u64,
    /// Comma-separated, e.g. "watch,ble".
    pub sources: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    #[serde(flatten)]
    pub summary: SessionSummary,
    /// `[ts, bpm]` pairs, oldest first.
    pub samples: Vec<(i64, u16)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub port: u16,
    pub lan_enabled: bool,
    pub idle_timeout_secs: u32,
    pub record_sessions: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: 8746,
            lan_enabled: true,
            idle_timeout_secs: 120,
            record_sessions: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Json,
}

/// What the hub tells its listeners. The Tauri app turns these into
/// `pulse://…` events; the API's WebSocket forwards `Hr`, `Live` and `Widget`.
#[derive(Debug, Clone)]
pub enum Event {
    Hr(HrSample),
    Live(bool),
    /// Sources, server or recording changed (also once a second while
    /// samples flow, so `lastBpm` in the sources stays fresh).
    State(Box<AppState>),
    PairRequest(PairRequest),
    PairDone(PairDone),
    Paired(Vec<PairedDevice>),
    Ble(Vec<BleDevice>),
    Sessions,
    Widget {
        id: String,
        config: Value,
    },
}
