//! The desktop shell: the hub from `uwupulse-core`, its commands for the
//! page, and its events as `pulse://…`. Everything with substance lives in
//! the core (`uwupulse_core::commands`, shared with the OBS plugin's `/ui`);
//! this file only translates and supplies what only a Tauri app can do.

#[cfg(feature = "self-update")]
mod updates;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use uwupulse_core::commands::{self, SaveRequest, UiEvents};
#[cfg(feature = "self-update")]
use uwupulse_core::commands::{HostFuture, UpdateCommand};
use uwupulse_core::{Host, HostInfo, HostKind, Pulse, PulseOptions};

/// The hub and this app as its host, for the commands.
struct Shell {
    pulse: Pulse,
    host: Arc<dyn Host>,
}

/// What the page asks of the app itself: save dialogs, the browser, quitting,
/// updates.
struct TauriHost {
    app: AppHandle,
}

impl Host for TauriHost {
    fn info(&self) -> HostInfo {
        HostInfo {
            kind: HostKind::Desktop,
            updater: cfg!(feature = "self-update"),
        }
    }

    fn save_file(&self, request: SaveRequest) -> Result<Option<PathBuf>, String> {
        let picked = self
            .app
            .dialog()
            .file()
            .set_file_name(request.file_name)
            .add_filter(request.filter_name, &[request.extension])
            .blocking_save_file();
        picked
            .map(|p| p.into_path().map_err(|e| e.to_string()))
            .transpose()
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    }

    fn finish_quit(&self, proceed: bool) {
        #[cfg(target_os = "macos")]
        uwu_macos::reply_quit(proceed);
        #[cfg(not(target_os = "macos"))]
        let _ = proceed;
    }

    #[cfg(feature = "self-update")]
    fn update(&self, command: UpdateCommand) -> HostFuture<'_, Result<Value, String>> {
        use updates::Channel;
        fn out<T: serde::Serialize>(v: T) -> Result<Value, String> {
            serde_json::to_value(v).map_err(|e| e.to_string())
        }
        let app = &self.app;
        Box::pin(async move {
            match command {
                UpdateCommand::SetChannel(channel) => {
                    let channel: Channel = serde_json::from_value(channel)
                        .map_err(|e| format!("invalid args `channel`: {e}"))?;
                    updates::set_channel(app, channel);
                    Ok(Value::Null)
                }
                UpdateCommand::SetAuto(on) => {
                    updates::set_auto(app, on);
                    Ok(Value::Null)
                }
                UpdateCommand::Status => out(updates::ready(app)),
                UpdateCommand::Check => out(updates::check(app).await?),
                UpdateCommand::Install => out(updates::install_now(app).await?),
            }
        })
    }
}

/// One Tauri command per core command, each handing its arguments over
/// unchanged, so names and arguments stay exactly what the page sends.
macro_rules! core_commands {
    ($($name:ident),* $(,)?) => {
        $(
            #[tauri::command]
            async fn $name(shell: State<'_, Shell>, request: Request<'_>) -> Result<Value, String> {
                let args = match request.body() {
                    InvokeBody::Json(v) => v.clone(),
                    InvokeBody::Raw(_) => Value::Null,
                };
                commands::dispatch(&shell.pulse, &shell.host, stringify!($name), args).await
            }
        )*

        /// The commands registered here, checked against the core's list.
        #[cfg(test)]
        const REGISTERED: &[&str] = &[$(stringify!($name)),*];

        fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($name),*]
        }
    };
}

core_commands![
    get_state,
    get_host_info,
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
];

/// Hub events → `pulse://…` for the page.
fn forward_events(app: AppHandle, pulse: Pulse) {
    let mut events = UiEvents::new(&pulse);
    tauri::async_runtime::spawn(async move {
        while let Some((name, payload)) = events.next().await {
            if let Err(e) = app.emit(name, payload) {
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
            app.manage(pulse.clone());
            app.manage(Shell {
                pulse,
                host: Arc::new(TauriHost {
                    app: app.handle().clone(),
                }),
            });
            #[cfg(feature = "self-update")]
            updates::start(app.handle());
            Ok(())
        })
        .invoke_handler(handler())
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
    fn registers_every_core_command() {
        let mut ours = super::REGISTERED.to_vec();
        let mut core = uwupulse_core::commands::COMMANDS.to_vec();
        ours.sort_unstable();
        core.sort_unstable();
        assert_eq!(ours, core);
    }
}
