//! The desktop shell: the hub from `uwupulse-core`, its commands for the
//! page, and its events as `pulse://…`. Everything with substance lives in
//! the core; this file only translates.

#[cfg(feature = "self-update")]
mod updates;

use chrono::TimeZone;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use uwupulse_core::proto::WatchAction;
use uwupulse_core::{
    AppState, BleDevice, Event, ExportFormat, PairedDevice, PairingOffer, Pulse, PulseOptions,
    ServerInfo, SessionDetail, SessionSummary, Settings,
};

/// Errors reach the page as plain strings; the page shows them as they are.
type CmdResult<T> = Result<T, String>;

fn err(e: uwupulse_core::Error) -> String {
    e.to_string()
}

#[tauri::command]
fn get_state(pulse: State<'_, Pulse>) -> AppState {
    pulse.state()
}

#[tauri::command]
fn list_paired(pulse: State<'_, Pulse>) -> Vec<PairedDevice> {
    pulse.list_paired()
}

#[tauri::command]
fn remove_paired(pulse: State<'_, Pulse>, id: String) -> CmdResult<()> {
    pulse.remove_paired(&id).map_err(err)
}

#[tauri::command]
fn create_pairing(pulse: State<'_, Pulse>) -> CmdResult<PairingOffer> {
    pulse.create_pairing().map_err(err)
}

#[tauri::command]
fn cancel_pairing(pulse: State<'_, Pulse>) {
    pulse.cancel_pairing();
}

#[tauri::command(rename_all = "camelCase")]
fn answer_pair_request(pulse: State<'_, Pulse>, request_id: String, accept: bool) -> CmdResult<()> {
    pulse.answer_pair_request(&request_id, accept).map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
async fn send_watch_command(
    pulse: State<'_, Pulse>,
    device_id: String,
    action: WatchAction,
) -> CmdResult<()> {
    pulse
        .send_watch_command(&device_id, action)
        .await
        .map_err(err)
}

#[tauri::command]
async fn ble_set_scanning(pulse: State<'_, Pulse>, on: bool) -> CmdResult<()> {
    pulse.ble_set_scanning(on).await.map_err(err)
}

#[tauri::command]
fn ble_list(pulse: State<'_, Pulse>) -> Vec<BleDevice> {
    pulse.ble_list()
}

#[tauri::command]
async fn ble_connect(pulse: State<'_, Pulse>, id: String) -> CmdResult<()> {
    pulse.ble_connect(&id).await.map_err(err)
}

#[tauri::command]
async fn ble_disconnect(pulse: State<'_, Pulse>, id: String) -> CmdResult<()> {
    pulse.ble_disconnect(&id).await.map_err(err)
}

#[tauri::command]
fn list_widgets(pulse: State<'_, Pulse>) -> Vec<Value> {
    pulse.list_widgets()
}

#[tauri::command]
fn save_widget(pulse: State<'_, Pulse>, widget: Value) -> CmdResult<Value> {
    pulse.save_widget(widget).map_err(err)
}

#[tauri::command]
fn delete_widget(pulse: State<'_, Pulse>, id: String) -> CmdResult<()> {
    pulse.delete_widget(&id).map_err(err)
}

#[tauri::command]
fn list_sessions(pulse: State<'_, Pulse>) -> CmdResult<Vec<SessionSummary>> {
    pulse.list_sessions().map_err(err)
}

#[tauri::command]
fn get_session(pulse: State<'_, Pulse>, id: String) -> CmdResult<SessionDetail> {
    pulse.get_session(&id).map_err(err)
}

#[tauri::command]
fn delete_session(pulse: State<'_, Pulse>, id: String) -> CmdResult<()> {
    pulse.delete_session(&id).map_err(err)
}

/// Asks where to save, then writes the export there. The page names only the
/// session and the format: the path comes from the save dialog shown here,
/// never from the page. `None` when the dialog was cancelled.
#[tauri::command]
async fn export_session(
    app: AppHandle,
    pulse: State<'_, Pulse>,
    id: String,
    format: ExportFormat,
) -> CmdResult<Option<String>> {
    let started_at = pulse
        .list_sessions()
        .map_err(err)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or("session not found")?
        .started_at;
    let (name, ext) = match format {
        ExportFormat::Csv => ("CSV", "csv"),
        ExportFormat::Json => ("JSON", "json"),
    };
    let file_name = chrono::Local
        .timestamp_millis_opt(started_at)
        .single()
        .map(|t| export_file_name(&t.naive_local(), ext))
        .unwrap_or_else(|| format!("uwupulse.{ext}"));
    let dialog = app
        .dialog()
        .file()
        .set_file_name(file_name)
        .add_filter(name, &[ext]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|e| e.to_string())?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    pulse.export_session_to(&id, format, &path).map_err(err)?;
    Ok(Some(path.display().to_string()))
}

/// `uwupulse-2026-10-30-1405.csv`: the session's local start time.
fn export_file_name(t: &chrono::NaiveDateTime, ext: &str) -> String {
    format!("uwupulse-{}.{ext}", t.format("%Y-%m-%d-%H%M"))
}

#[tauri::command]
fn get_settings(pulse: State<'_, Pulse>) -> Settings {
    pulse.settings()
}

#[tauri::command]
async fn set_settings(pulse: State<'_, Pulse>, settings: Settings) -> CmdResult<Settings> {
    pulse.set_settings(settings).await.map_err(err)
}

#[tauri::command]
fn regenerate_token(pulse: State<'_, Pulse>) -> CmdResult<ServerInfo> {
    pulse.regenerate_token().map_err(err)
}

// ── App: project pages, quitting, updates ───────────────

/// The project pages the About card links to. The page names one; it never
/// hands in an address of its own.
#[tauri::command]
fn open_project_page(app: AppHandle, page: String) -> CmdResult<()> {
    let url = match page.as_str() {
        "source" => "https://github.com/MinifyX/UwUPulse",
        "releases" => "https://github.com/MinifyX/UwUPulse/releases",
        "issues" => "https://github.com/MinifyX/UwUPulse/issues",
        "license" => "https://github.com/MinifyX/UwUPulse/blob/main/LICENSE",
        _ => return Err(format!("unknown page: {page}")),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("Couldn't open the browser: {e}"))
}

/// The page's answer to `quit-requested` (macOS: ⌘Q, the Dock, logging out).
#[tauri::command]
fn finish_quit(proceed: bool) {
    #[cfg(target_os = "macos")]
    uwu_macos::reply_quit(proceed);
    #[cfg(not(target_os = "macos"))]
    let _ = proceed;
}

/// The update commands exist in every build; without `self-update` they say
/// there is nothing to update.
#[cfg(feature = "self-update")]
mod update_commands {
    use super::updates::{self, Channel, UpdateInfo};
    use tauri::AppHandle;

    #[tauri::command]
    pub fn set_update_channel(app: AppHandle, channel: Channel) {
        updates::set_channel(&app, channel);
    }

    #[tauri::command]
    pub fn set_auto_update(app: AppHandle, on: bool) {
        updates::set_auto(&app, on);
    }

    #[tauri::command]
    pub fn update_status(app: AppHandle) -> Option<UpdateInfo> {
        updates::ready(&app)
    }

    #[tauri::command]
    pub async fn check_for_updates(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
        updates::check(&app).await
    }

    #[tauri::command]
    pub async fn install_update(app: AppHandle) -> Result<(), String> {
        updates::install_now(&app).await
    }
}

#[cfg(not(feature = "self-update"))]
mod update_commands {
    use serde_json::Value;

    #[tauri::command]
    pub fn set_update_channel(channel: Value) {
        let _ = channel;
    }

    #[tauri::command]
    pub fn set_auto_update(on: bool) {
        let _ = on;
    }

    #[tauri::command]
    pub fn update_status() -> Option<Value> {
        None
    }

    #[tauri::command]
    pub fn check_for_updates() -> Result<Option<Value>, String> {
        Ok(None)
    }

    #[tauri::command]
    pub fn install_update() -> Result<(), String> {
        Err("This copy of UwUPulse doesn't update itself.".into())
    }
}

use update_commands::{
    check_for_updates, install_update, set_auto_update, set_update_channel, update_status,
};

/// Hub events → `pulse://…` for the page. `Widget` only concerns the
/// overlays; `Live` changes are part of the state.
fn forward_events(app: AppHandle, pulse: Pulse) {
    let mut rx = pulse.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                // Missed some: the next state catches the page up.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = app.emit("pulse://state", pulse.state());
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let sent = match ev {
                Event::Hr(s) => app.emit("pulse://hr", s),
                Event::Live(_) => app.emit("pulse://state", pulse.state()),
                Event::State(s) => app.emit("pulse://state", s),
                Event::PairRequest(r) => app.emit("pulse://pair-request", r),
                Event::PairDone(d) => app.emit("pulse://pair-done", d),
                Event::Paired(list) => app.emit("pulse://paired", list),
                Event::Ble(list) => app.emit("pulse://ble", list),
                Event::Sessions => app.emit("pulse://sessions", ()),
                Event::Widget { .. } => Ok(()),
            };
            if let Err(e) = sent {
                log::warn!("emit: {e}");
            }
        }
    });
}

fn start_pulse(app: &AppHandle) -> Result<Pulse, Box<dyn std::error::Error>> {
    let paths = app.path();
    let mut opts = PulseOptions::new(paths.app_config_dir()?, paths.app_data_dir()?);
    opts.app_version = app.package_info().version.to_string();
    // Development and tests can move the server off the default port.
    opts.port_override = std::env::var("UWUPULSE_PORT")
        .ok()
        .and_then(|p| p.parse().ok());
    let pulse = tauri::async_runtime::block_on(Pulse::start(opts))?;
    if let Some(e) = pulse.server_info().error {
        // Not fatal: the page shows it and the port can be changed there.
        log::warn!("server: {e}");
    }
    Ok(pulse)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // First, so a second start never gets as far as binding the port.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init());
    #[cfg(feature = "self-update")]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    let app = builder
        .setup(|app| {
            // ⌘Q, the Dock and logging out ask the page first (`onMacQuit` in
            // App.tsx, answered through `finish_quit`).
            #[cfg(target_os = "macos")]
            {
                let handle = app.handle().clone();
                if let Err(error) = uwu_macos::install_quit_guard(move || {
                    handle.emit(uwu_macos::QUIT_EVENT, ()).is_ok()
                }) {
                    log::warn!("quit guard: {error}");
                }
            }
            // A waiting update is installed before the server takes its port.
            #[cfg(feature = "self-update")]
            if updates::apply_pending_on_start(app.handle()) {
                std::process::exit(0);
            }
            let pulse = start_pulse(app.handle())?;
            forward_events(app.handle().clone(), pulse.clone());
            app.manage(pulse);
            #[cfg(feature = "self-update")]
            updates::start(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            list_paired,
            remove_paired,
            create_pairing,
            cancel_pairing,
            answer_pair_request,
            send_watch_command,
            ble_set_scanning,
            ble_list,
            ble_connect,
            ble_disconnect,
            list_widgets,
            save_widget,
            delete_widget,
            list_sessions,
            get_session,
            delete_session,
            export_session,
            get_settings,
            set_settings,
            regenerate_token,
            open_project_page,
            finish_quit,
            set_update_channel,
            set_auto_update,
            update_status,
            check_for_updates,
            install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building UwUPulse");

    app.run(|app, ev| {
        // A click on the Dock icon brings the hidden window back; the server
        // kept running meanwhile.
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = ev
        {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        if let RunEvent::Exit = ev {
            // Close the recording, the sockets and the straps before leaving.
            if let Some(pulse) = app.try_state::<Pulse>() {
                tauri::async_runtime::block_on(pulse.shutdown());
            }
        }
    });
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
}
