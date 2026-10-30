//! The app's commands and events, independent of who shows the page.
//!
//! The desktop app hands them to its Tauri window (`invoke`, `pulse://…`
//! events); a host without Tauri (the OBS plugin) serves the same page from
//! the core's server and reaches them over `/ui/rpc` and `/ui/events`. Both
//! go through [`dispatch`] and [`UiEvents`], so the page sees the same
//! names, arguments and payloads either way.
//!
//! What only the embedding app can do (a save dialog, opening the browser,
//! updating itself, quitting) goes through [`Host`].

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use chrono::TimeZone;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::types::{Event, ExportFormat, Settings};
use crate::Pulse;

/// A boxed future, so [`Host`] stays object safe.
pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Who shows the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    /// The Tauri desktop app.
    Desktop,
    /// The OBS plugin's dock.
    Obs,
}

/// `get_host_info`: lets the page hide what its host can't do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HostInfo {
    pub kind: HostKind,
    /// Whether the update commands do anything.
    pub updater: bool,
}

/// What the save dialog for an export suggests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    /// `uwupulse-2026-10-30-1405.csv`
    pub file_name: String,
    /// The filter's label: "CSV", "JSON".
    pub filter_name: &'static str,
    /// Without the dot: "csv", "json".
    pub extension: &'static str,
}

/// The update commands of the page (`set_update_channel`, …).
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateCommand {
    /// The channel as the page sent it (`"stable"`, `"beta"`).
    SetChannel(Value),
    SetAuto(bool),
    Status,
    Check,
    Install,
}

/// The embedding app: everything the page asks for that the core can't do
/// on its own.
pub trait Host: Send + Sync + 'static {
    fn info(&self) -> HostInfo;

    /// Asks where to save an export. Blocking (a modal dialog); called from a
    /// blocking thread. `None` when the person cancelled.
    fn save_file(&self, request: SaveRequest) -> Result<Option<PathBuf>, String> {
        let _ = request;
        Err("Exporting is not available here.".into())
    }

    /// Opens one of the project's pages in the browser. Only ever called
    /// with the fixed addresses in [`project_page_url`].
    fn open_url(&self, url: &str) -> Result<(), String>;

    /// The page's answer to a quit request (macOS: ⌘Q, the Dock).
    fn finish_quit(&self, proceed: bool) {
        let _ = proceed;
    }

    /// The update commands. A host without an updater keeps the default,
    /// which answers like the desktop app built without `self-update`.
    fn update(&self, command: UpdateCommand) -> HostFuture<'_, Result<Value, String>> {
        Box::pin(std::future::ready(no_updater(&command)))
    }
}

/// How a copy without an updater answers the update commands: nothing
/// waiting, nothing found, nothing to install.
pub fn no_updater(command: &UpdateCommand) -> Result<Value, String> {
    match command {
        UpdateCommand::SetChannel(_)
        | UpdateCommand::SetAuto(_)
        | UpdateCommand::Status
        | UpdateCommand::Check => Ok(Value::Null),
        UpdateCommand::Install => Err("This copy of UwUPulse doesn't update itself.".into()),
    }
}

/// Every command [`dispatch`] knows; the Tauri app registers exactly these.
pub const COMMANDS: &[&str] = &[
    "get_state",
    "get_host_info",
    "list_paired",
    "remove_paired",
    "create_pairing",
    "cancel_pairing",
    "answer_pair_request",
    "send_watch_command",
    "ble_set_scanning",
    "ble_list",
    "ble_connect",
    "ble_disconnect",
    "list_widgets",
    "save_widget",
    "delete_widget",
    "list_sessions",
    "get_session",
    "delete_session",
    "export_session",
    "get_settings",
    "set_settings",
    "regenerate_token",
    "open_project_page",
    "finish_quit",
    "set_update_channel",
    "set_auto_update",
    "update_status",
    "check_for_updates",
    "install_update",
];

/// The project pages the About card links to. The page names one; it never
/// hands in an address of its own.
pub fn project_page_url(page: &str) -> Option<&'static str> {
    Some(match page {
        "source" => "https://github.com/MinifyX/UwUPulse",
        "releases" => "https://github.com/MinifyX/UwUPulse/releases",
        "issues" => "https://github.com/MinifyX/UwUPulse/issues",
        "license" => "https://github.com/MinifyX/UwUPulse/blob/main/LICENSE",
        _ => return None,
    })
}

/// Runs one command. `args` is the object the page passed (camelCase keys,
/// as with Tauri's `invoke`); `null` when there are none. Errors are the
/// text the page shows.
pub async fn dispatch(
    pulse: &Pulse,
    host: &Arc<dyn Host>,
    command: &str,
    args: Value,
) -> Result<Value, String> {
    let a = Args { command, args };
    match command {
        "get_state" => out(pulse.state()),
        "get_host_info" => out(host.info()),

        "list_paired" => out(pulse.list_paired()),
        "remove_paired" => unit(pulse.remove_paired(&a.get::<String>("id")?)),
        "create_pairing" => out(pulse.create_pairing().map_err(err)?),
        "cancel_pairing" => {
            pulse.cancel_pairing();
            Ok(Value::Null)
        }
        "answer_pair_request" => {
            unit(pulse.answer_pair_request(&a.get::<String>("requestId")?, a.get("accept")?))
        }
        "send_watch_command" => unit(
            pulse
                .send_watch_command(&a.get::<String>("deviceId")?, a.get("action")?)
                .await,
        ),

        "ble_set_scanning" => unit(pulse.ble_set_scanning(a.get("on")?).await),
        "ble_list" => out(pulse.ble_list()),
        "ble_connect" => unit(pulse.ble_connect(&a.get::<String>("id")?).await),
        "ble_disconnect" => unit(pulse.ble_disconnect(&a.get::<String>("id")?).await),

        "list_widgets" => out(pulse.list_widgets()),
        "save_widget" => out(pulse.save_widget(a.get("widget")?).map_err(err)?),
        "delete_widget" => unit(pulse.delete_widget(&a.get::<String>("id")?)),

        "list_sessions" => out(pulse.list_sessions().map_err(err)?),
        "get_session" => out(pulse.get_session(&a.get::<String>("id")?).map_err(err)?),
        "delete_session" => unit(pulse.delete_session(&a.get::<String>("id")?)),
        "export_session" => export_session(pulse, host, a.get("id")?, a.get("format")?).await,

        "get_settings" => out(pulse.settings()),
        "set_settings" => out(pulse
            .set_settings(a.get::<Settings>("settings")?)
            .await
            .map_err(err)?),
        "regenerate_token" => out(pulse.regenerate_token().map_err(err)?),

        "open_project_page" => {
            let page: String = a.get("page")?;
            let url = project_page_url(&page).ok_or_else(|| format!("unknown page: {page}"))?;
            host.open_url(url)
                .map_err(|e| format!("Couldn't open the browser: {e}"))?;
            Ok(Value::Null)
        }
        "finish_quit" => {
            host.finish_quit(a.get("proceed")?);
            Ok(Value::Null)
        }

        "set_update_channel" => {
            host.update(UpdateCommand::SetChannel(a.get("channel")?))
                .await
        }
        "set_auto_update" => host.update(UpdateCommand::SetAuto(a.get("on")?)).await,
        "update_status" => host.update(UpdateCommand::Status).await,
        "check_for_updates" => host.update(UpdateCommand::Check).await,
        "install_update" => host.update(UpdateCommand::Install).await,

        _ => Err(format!("unknown command: {command}")),
    }
}

/// Asks the host where to save, then writes the export there. The page
/// names only the session and the format: the path comes from the host's
/// dialog, never from the page. `null` when the dialog was cancelled.
async fn export_session(
    pulse: &Pulse,
    host: &Arc<dyn Host>,
    id: String,
    format: ExportFormat,
) -> Result<Value, String> {
    let started_at = pulse
        .list_sessions()
        .map_err(err)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or("session not found")?
        .started_at;
    let (filter_name, extension) = match format {
        ExportFormat::Csv => ("CSV", "csv"),
        ExportFormat::Json => ("JSON", "json"),
    };
    let file_name = chrono::Local
        .timestamp_millis_opt(started_at)
        .single()
        .map(|t| export_file_name(&t.naive_local(), extension))
        .unwrap_or_else(|| format!("uwupulse.{extension}"));
    let request = SaveRequest {
        file_name,
        filter_name,
        extension,
    };
    let dialog_host = host.clone();
    let picked = tokio::task::spawn_blocking(move || dialog_host.save_file(request))
        .await
        .map_err(|e| e.to_string())??;
    let Some(path) = picked else {
        return Ok(Value::Null);
    };
    pulse.export_session_to(&id, format, &path).map_err(err)?;
    Ok(json!(path.display().to_string()))
}

/// `uwupulse-2026-10-30-1405.csv`: the session's local start time.
pub fn export_file_name(t: &chrono::NaiveDateTime, ext: &str) -> String {
    format!("uwupulse-{}.{ext}", t.format("%Y-%m-%d-%H%M"))
}

/// The arguments of one command.
struct Args<'a> {
    command: &'a str,
    args: Value,
}

impl Args<'_> {
    fn get<T: DeserializeOwned>(&self, key: &str) -> Result<T, String> {
        let value = self.args.get(key).cloned().unwrap_or(Value::Null);
        serde_json::from_value(value)
            .map_err(|e| format!("invalid args `{key}` for command `{}`: {e}", self.command))
    }
}

fn err(e: crate::Error) -> String {
    e.to_string()
}

fn out<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn unit(result: Result<(), crate::Error>) -> Result<Value, String> {
    result.map(|()| Value::Null).map_err(err)
}

// ── Events ──────────────────────────────────────────────

/// A hub event as the page gets it: `pulse://…` and its payload. `Widget`
/// only concerns the overlays (`None`); a `Live` change goes out as the
/// whole state.
pub fn ui_event(pulse: &Pulse, ev: Event) -> Option<(&'static str, Value)> {
    let (name, payload) = match ev {
        Event::Hr(s) => ("pulse://hr", serde_json::to_value(s)),
        Event::Live(_) => ("pulse://state", serde_json::to_value(pulse.state())),
        Event::State(s) => ("pulse://state", serde_json::to_value(s)),
        Event::PairRequest(r) => ("pulse://pair-request", serde_json::to_value(r)),
        Event::PairDone(d) => ("pulse://pair-done", serde_json::to_value(d)),
        Event::Paired(list) => ("pulse://paired", serde_json::to_value(list)),
        Event::Ble(list) => ("pulse://ble", serde_json::to_value(list)),
        Event::Sessions => ("pulse://sessions", Ok(Value::Null)),
        Event::Widget { .. } => return None,
    };
    match payload {
        Ok(p) => Some((name, p)),
        Err(e) => {
            log::warn!("event {name}: {e}");
            None
        }
    }
}

/// The page's events from now on, for one page.
pub struct UiEvents {
    pulse: Pulse,
    rx: broadcast::Receiver<Event>,
}

impl UiEvents {
    pub fn new(pulse: &Pulse) -> Self {
        Self {
            rx: pulse.subscribe(),
            pulse: pulse.clone(),
        }
    }

    /// The next event; `None` once the hub is gone. A receiver that fell
    /// behind gets the current state instead of what it missed.
    pub async fn next(&mut self) -> Option<(&'static str, Value)> {
        loop {
            match self.rx.recv().await {
                Ok(ev) => {
                    if let Some(out) = ui_event(&self.pulse, ev) {
                        return Some(out);
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    if let Ok(state) = serde_json::to_value(self.pulse.state()) {
                        return Some(("pulse://state", state));
                    }
                }
                Err(RecvError::Closed) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_file_name() {
        let t = chrono::NaiveDate::from_ymd_opt(2026, 10, 30)
            .unwrap()
            .and_hms_opt(14, 5, 59)
            .unwrap();
        assert_eq!(
            super::export_file_name(&t, "csv"),
            "uwupulse-2026-10-30-1405.csv"
        );
    }

    #[test]
    fn project_pages_are_fixed() {
        assert_eq!(
            super::project_page_url("license"),
            Some("https://github.com/MinifyX/UwUPulse/blob/main/LICENSE")
        );
        assert_eq!(super::project_page_url("https://example.com"), None);
    }
}
