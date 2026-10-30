//! The desktop shell: the hub from `uwupulse-core`, its commands for the
//! page, and its events as `pulse://…`. Everything with substance lives in
//! the core; this file only translates.

use std::path::PathBuf;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
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

/// `path` comes from the page's save dialog.
#[tauri::command]
fn export_session(
    pulse: State<'_, Pulse>,
    id: String,
    format: ExportFormat,
    path: PathBuf,
) -> CmdResult<()> {
    pulse.export_session_to(&id, format, &path).map_err(err)
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
    let app = tauri::Builder::default()
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
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let pulse = start_pulse(app.handle())?;
            forward_events(app.handle().clone(), pulse.clone());
            app.manage(pulse);
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
        ])
        .build(tauri::generate_context!())
        .expect("error while building UwUPulse");

    app.run(|app, ev| {
        if let RunEvent::Exit = ev {
            // Close the recording, the sockets and the straps before leaving.
            if let Some(pulse) = app.try_state::<Pulse>() {
                tauri::async_runtime::block_on(pulse.shutdown());
            }
        }
    });
}
