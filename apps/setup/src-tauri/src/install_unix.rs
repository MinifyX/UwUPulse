//! Installing, updating and removing UwUPulse on macOS and Linux, for the
//! current user.
//!
//! - **macOS:** `UwUPulse.app` into `/Applications`, or `~/Applications` when
//!   this user may not write there. No shortcuts: the app shows up in
//!   Launchpad and Spotlight by being there.
//! - **Linux:** the app as an AppDir — an AppImage, already unpacked — under
//!   `~/.local/share/uwupulse`, with menu entries in
//!   `~/.local/share/applications` and, if wanted, on the desktop. Unpacked,
//!   because an AppImage that runs as one needs FUSE, which many systems no
//!   longer ship; everything else it brings (WebKitGTK included) stays inside.
//!
//! No administrator rights either way. What was installed where is kept in a
//! small file (`install.json`) in the setup's own config folder, which
//! UwUPulse's updater reads too: an app installed some other way — a `.deb`, a
//! copy dragged out of a disk image — is not one the setup updates.
//!
//! The OBS plugin, when picked, goes where OBS loads one user's plugins
//! (`obs.rs`), and is noted in `install.json` too.
//!
//! `UWUPULSE_SETUP_SANDBOX=<folder>` redirects all of it for testing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::obs::{self, Obs};
use crate::payload::{self, remove_tree, swap_in};
use crate::system;

#[cfg(target_os = "macos")]
mod names {
    pub const APP: &str = "UwUPulse.app";
}
#[cfg(not(target_os = "macos"))]
mod names {
    pub const APP: &str = "UwUPulse";
}
use names::APP;

pub const APP_ID: &str = "app.uwupulse.desktop";
const SETUP_ID: &str = "app.uwupulse.setup";

/// The app in a tar archive (see `build.rs`).
static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.zst"));
const PAYLOAD_SIZE: &str = env!("UWUPULSE_SETUP_PAYLOAD_SIZE");

pub fn has_payload() -> bool {
    !PAYLOAD.is_empty()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub dir: String,
    pub desktop_shortcut: bool,
    /// Install (or update) the desktop app. Left alone when not picked.
    #[serde(default = "yes")]
    pub app: bool,
    /// Install (or update) the OBS plugin. Left alone when not picked.
    #[serde(default)]
    pub obs: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub dir: String,
    pub version: Option<String>,
}

/// What `install.json` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    /// The app's folder; UwUPulse's updater reads it. When only the plugin
    /// was ever installed, the folder the app would go to.
    dir: String,
    version: String,
    desktop_shortcut: bool,
    /// The OBS plugin, if the setup installed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    obs: Option<obs::Installed>,
}

/// Where things go on this machine.
pub struct Layout {
    pub default_dir: PathBuf,
    /// The setup's `install.json`.
    state: PathBuf,
    /// Linux: where menu entries go. Unused on macOS.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    applications: PathBuf,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    desktop: PathBuf,
    /// Everything UwUPulse keeps: removed on request.
    data: Vec<PathBuf>,
    pub obs: Obs,
    pub sandbox: bool,
}

impl Layout {
    pub fn detect() -> Self {
        match std::env::var_os("UWUPULSE_SETUP_SANDBOX").filter(|dir| !dir.is_empty()) {
            Some(dir) => Self::sandbox(Path::new(&dir)),
            None => Self::real(),
        }
    }

    #[cfg(target_os = "macos")]
    fn real() -> Self {
        let home = system::home();
        let library = home.join("Library");
        let system_apps = PathBuf::from("/Applications");
        let default_dir = if system::writable(&system_apps) {
            system_apps
        } else {
            home.join("Applications")
        };
        let data = vec![
            library.join("Application Support").join(APP_ID),
            library.join("Caches").join(APP_ID),
            library.join("WebKit").join(APP_ID),
            library
                .join("Saved Application State")
                .join(format!("{APP_ID}.savedState")),
        ];
        Self {
            default_dir,
            state: library
                .join("Application Support")
                .join(SETUP_ID)
                .join("install.json"),
            applications: PathBuf::new(),
            desktop: PathBuf::new(),
            data,
            obs: Obs::real(),
            sandbox: false,
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn real() -> Self {
        let data_home = system::xdg("XDG_DATA_HOME", ".local/share");
        let config_home = system::xdg("XDG_CONFIG_HOME", ".config");
        let cache_home = system::xdg("XDG_CACHE_HOME", ".cache");
        let data = vec![
            data_home.join(APP_ID),
            config_home.join(APP_ID),
            cache_home.join(APP_ID),
        ];
        Self {
            default_dir: data_home.join("uwupulse"),
            state: config_home.join(SETUP_ID).join("install.json"),
            applications: data_home.join("applications"),
            desktop: system::desktop_dir(),
            data,
            obs: Obs::real(),
            sandbox: false,
        }
    }

    pub fn sandbox(root: &Path) -> Self {
        Self {
            default_dir: root.join("Programs"),
            state: root.join("Setup").join("install.json"),
            applications: root.join("Applications"),
            desktop: root.join("Desktop"),
            data: vec![root.join("Data").join(APP_ID)],
            obs: Obs::sandbox(root),
            sandbox: true,
        }
    }

    fn record(&self) -> Option<Record> {
        let text = std::fs::read_to_string(&self.state).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// What is installed right now, if anything.
    pub fn installed(&self) -> Option<Installed> {
        if let Some(record) = self.record() {
            if Path::new(&record.dir).join(APP).exists() {
                return Some(Installed {
                    dir: record.dir,
                    version: Some(record.version),
                });
            }
        }
        // Put there by hand — out of a disk image, say. Its version is not
        // known, so an update will not touch it, but a reinstall can.
        self.default_dir.join(APP).exists().then(|| Installed {
            dir: self.default_dir.display().to_string(),
            version: None,
        })
    }

    /// The OBS plugin the setup installed, if it is still there.
    pub fn obs_installed(&self) -> Option<obs::Installed> {
        self.record()?
            .obs
            .filter(|plugin| Path::new(&plugin.dir).exists())
    }

    /// Options from the last install, or the defaults: the app unless only
    /// the plugin was installed, the plugin if it was or OBS is there.
    pub fn remembered_options(&self) -> Options {
        let app = self.installed().is_some();
        let obs_wanted =
            obs::has_payload() && (self.obs_installed().is_some() || self.obs.detected());
        match self.record() {
            Some(record) => Options {
                dir: record.dir,
                desktop_shortcut: record.desktop_shortcut,
                app,
                obs: obs_wanted,
            },
            None => Options {
                dir: self.default_dir.display().to_string(),
                desktop_shortcut: !cfg!(target_os = "macos"),
                app: true,
                obs: obs_wanted,
            },
        }
    }
}

/// On macOS the app goes straight into the picked folder; elsewhere into an
/// `uwupulse` folder inside it, unless it is one already.
pub fn folder_for(picked: PathBuf) -> PathBuf {
    if cfg!(target_os = "macos")
        || picked
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("uwupulse"))
    {
        picked
    } else {
        picked.join("uwupulse")
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Prepare,
    Copy,
    Shortcuts,
    Register,
    Cleanup,
    Done,
}

pub type Progress<'a> = &'a mut dyn FnMut(Step, f64);

/// Stops UwUPulse if it runs from `dir`. Skipped in the sandbox, which must
/// never touch real processes.
pub fn stop_app(layout: &Layout, dir: &Path) -> Result<(), String> {
    if layout.sandbox {
        return Ok(());
    }
    system::stop_processes_under(&dir.join(APP), "UwUPulse")
}

/// UwUPulse is open. Closing it stops the live heart rate, the overlay and a
/// running recording, so the setup asks first.
pub fn app_running(layout: &Layout, dir: &Path) -> bool {
    !layout.sandbox && !system::processes_under(&dir.join(APP)).is_empty()
}

/// Starts the installed UwUPulse. Nothing in the sandbox.
pub fn launch(layout: &Layout, dir: &Path) -> Result<(), String> {
    if layout.sandbox {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        system::spawn_detached(
            Path::new("/usr/bin/open"),
            &[&dir.join(APP).to_string_lossy()],
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        system::spawn_detached(&dir.join(APP).join("AppRun"), &[])
    }
}

/// Unpack the app into `staging`, reporting progress over 0..1.
fn unpack(staging: &Path, progress: &mut dyn FnMut(f64)) -> Result<(), String> {
    payload::unpack(PAYLOAD, PAYLOAD_SIZE, APP, "app", staging, progress)
}

/// Unpacks everything into `dir` (which must not exist yet) and checks that
/// the app arrived with an executable to start — `--check-payload`, for CI.
pub fn check_payload(dir: &Path) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;
    if dir.exists() {
        return Err(format!("{} exists already.", dir.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    unpack(dir, &mut |_| {})?;
    // macOS: the one program in Contents/MacOS; Linux: the AppDir's AppRun.
    let path = if cfg!(target_os = "macos") {
        let programs = dir.join(APP).join("Contents/MacOS");
        std::fs::read_dir(&programs)
            .map_err(|e| format!("{} is missing: {e}", programs.display()))?
            .flatten()
            .map(|entry| entry.path())
            .next()
            .ok_or_else(|| format!("{} is empty.", programs.display()))?
    } else {
        dir.join(APP).join("AppRun")
    };
    let meta =
        std::fs::metadata(&path).map_err(|e| format!("{} is missing: {e}", path.display()))?;
    if meta.permissions().mode() & 0o111 == 0 {
        return Err(format!("{} is not executable.", path.display()));
    }
    let mut report = format!("ok  {} ({} bytes)", path.display(), meta.len());
    // The OBS plugin, where there is one: its library where OBS looks.
    if obs::has_payload() {
        let plugins = dir.join("obs");
        std::fs::create_dir_all(&plugins)
            .map_err(|e| format!("Couldn't create {}: {e}", plugins.display()))?;
        let plugin = Obs::sandbox(&plugins);
        plugin.install("check", &mut |_| {})?;
        let library = if cfg!(target_os = "macos") {
            plugin.plugin_dir().join("Contents/MacOS/uwupulse")
        } else {
            plugin.plugin_dir().join("bin/64bit/uwupulse.so")
        };
        let meta = std::fs::metadata(&library)
            .map_err(|e| format!("{} is missing: {e}", library.display()))?;
        report.push_str(&format!(
            "\nok  {} ({} bytes)",
            library.display(),
            meta.len()
        ));
    }
    Ok(report)
}

pub fn install(
    layout: &Layout,
    options: &Options,
    version: &str,
    progress: Progress,
) -> Result<(), String> {
    if !options.app && !options.obs {
        return Err("Please pick the desktop app, the OBS plugin or both.".into());
    }
    let dir = PathBuf::from(options.dir.trim());
    if options.app && !dir.is_absolute() {
        return Err("Please pick a full folder path.".into());
    }
    let before = layout.record();
    // The copy step: the app, then the plugin, by their usual sizes.
    let split = match (options.app, options.obs) {
        (true, true) => 0.8,
        (true, false) => 1.0,
        _ => 0.0,
    };

    progress(Step::Prepare, 0.0);
    if options.app {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
        stop_app(layout, &dir)?;
        // Unpacked next to where it goes, so the final move is a rename on
        // the same disk, never a copy.
        let staging = dir.join(format!(".uwupulse-setup-{}", std::process::id()));
        remove_tree(&staging);
        std::fs::create_dir_all(&staging)
            .map_err(|e| format!("Couldn't create {}: {e}", staging.display()))?;
        progress(Step::Prepare, 1.0);
        let result = unpack(&staging, &mut |f| progress(Step::Copy, f * split))
            .and_then(|()| swap_in(&staging.join(APP), &dir.join(APP)));
        remove_tree(&staging);
        result?;
    }
    progress(Step::Prepare, 1.0);

    // OBS may be open: it keeps the plugin it loaded until it restarts.
    let plugin = if options.obs {
        Some(layout.obs.install(version, &mut |f| {
            progress(Step::Copy, split + f * (1.0 - split))
        })?)
    } else {
        before.as_ref().and_then(|record| record.obs.clone())
    };

    progress(Step::Shortcuts, 0.0);
    if options.app {
        shortcuts(layout, &dir, options)?;
    }
    progress(Step::Shortcuts, 1.0);

    progress(Step::Register, 0.0);
    let record = match before {
        // The app stays as it was.
        Some(before) if !options.app => Record {
            obs: plugin,
            ..before
        },
        _ => Record {
            dir: dir.display().to_string(),
            version: version.to_string(),
            desktop_shortcut: options.desktop_shortcut,
            obs: plugin,
        },
    };
    if let Some(parent) = layout.state.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    std::fs::write(&layout.state, json)
        .map_err(|e| format!("Couldn't write {}: {e}", layout.state.display()))?;
    progress(Step::Register, 1.0);
    progress(Step::Done, 1.0);
    Ok(())
}

/// macOS: tell Launch Services the app is there, so Spotlight and Launchpad
/// find it at once.
#[cfg(target_os = "macos")]
fn shortcuts(layout: &Layout, dir: &Path, _options: &Options) -> Result<(), String> {
    if layout.sandbox {
        return Ok(());
    }
    const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
    let app = dir.join(APP);
    if app.exists() {
        system::best_effort(LSREGISTER, &["-f", &app.to_string_lossy()]);
    }
    Ok(())
}

/// Linux: menu entries, and a desktop icon if wanted.
#[cfg(not(target_os = "macos"))]
fn shortcuts(layout: &Layout, dir: &Path, options: &Options) -> Result<(), String> {
    let app_dir = dir.join(APP);
    let desktop_file = format!("{APP_ID}.desktop");
    let menu_entry = layout.applications.join(&desktop_file);
    let on_desktop = layout.desktop.join(&desktop_file);
    if !app_dir.exists() {
        let _ = std::fs::remove_file(&menu_entry);
        let _ = std::fs::remove_file(&on_desktop);
        return Ok(());
    }
    let entry = desktop_entry(
        &app_dir,
        "UwUPulse",
        "Live heart rate overlay for streams",
        "uwupulse-desktop",
        "Utility;AudioVideo;",
    );
    write_entry(&menu_entry, &entry)?;
    if options.desktop_shortcut {
        if write_entry(&on_desktop, &entry).is_ok() && !layout.sandbox {
            // GNOME only starts a desktop file someone trusted.
            system::best_effort(
                "gio",
                &[
                    "set",
                    &on_desktop.to_string_lossy(),
                    "metadata::trusted",
                    "true",
                ],
            );
        }
    } else {
        let _ = std::fs::remove_file(&on_desktop);
    }
    if !layout.sandbox {
        system::best_effort(
            "update-desktop-database",
            &[&layout.applications.to_string_lossy()],
        );
    }
    Ok(())
}

/// A freedesktop entry for an AppDir. The icon is the AppDir's own PNG.
#[cfg(not(target_os = "macos"))]
fn desktop_entry(
    app_dir: &Path,
    name: &str,
    comment: &str,
    class: &str,
    categories: &str,
) -> String {
    let icon = std::fs::read_dir(app_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "png"))
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let exec = app_dir.join("AppRun");
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment={comment}\nExec={}\nIcon={icon}\nTerminal=false\nCategories={categories}\nStartupWMClass={class}\n",
        quote_exec(&exec.to_string_lossy())
    )
}

/// A path as the `Exec` key wants it: quoted, with the characters the spec
/// reserves escaped.
#[cfg(not(target_os = "macos"))]
fn quote_exec(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        if c == '%' {
            out.push('%');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(not(target_os = "macos"))]
fn write_entry(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    Ok(())
}

pub fn uninstall(
    layout: &Layout,
    dir: &Path,
    keep_data: bool,
    progress: Progress,
) -> Result<(), String> {
    progress(Step::Prepare, 0.0);
    stop_app(layout, dir)?;
    progress(Step::Prepare, 1.0);

    progress(Step::Shortcuts, 0.0);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = std::fs::remove_file(layout.applications.join(format!("{APP_ID}.desktop")));
        let _ = std::fs::remove_file(layout.desktop.join(format!("{APP_ID}.desktop")));
    }
    progress(Step::Shortcuts, 1.0);

    progress(Step::Register, 0.0);
    let plugin = layout.record().and_then(|record| record.obs);
    let _ = std::fs::remove_file(&layout.state);
    if let Some(parent) = layout.state.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    progress(Step::Register, 1.0);

    progress(Step::Copy, 0.0);
    remove_tree(&dir.join(APP));
    if dir.join(APP).exists() {
        return Err(format!(
            "Couldn't remove {}. Is UwUPulse still open?",
            dir.join(APP).display()
        ));
    }
    // Only the folder the setup made for itself, and only when empty — never
    // /Applications or a folder the person chose that holds other things.
    if !cfg!(target_os = "macos") {
        let _ = std::fs::remove_dir(dir);
    }
    layout.obs.uninstall(plugin.as_ref(), keep_data)?;
    progress(Step::Copy, 1.0);

    if !keep_data {
        progress(Step::Cleanup, 0.0);
        for folder in &layout.data {
            if std::fs::symlink_metadata(folder).is_ok() {
                remove_tree(folder);
                if folder.exists() {
                    return Err(format!("Couldn't delete {}.", folder.display()));
                }
            }
        }
        progress(Step::Cleanup, 1.0);
    }
    progress(Step::Done, 1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox {
        _dir: tempfile::TempDir,
        layout: Layout,
    }

    impl Sandbox {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let layout = Layout::sandbox(dir.path());
            Self { _dir: dir, layout }
        }
    }

    #[test]
    fn remembers_defaults_until_something_is_installed() {
        let sandbox = Sandbox::new();
        assert!(sandbox.layout.installed().is_none());
        let options = sandbox.layout.remembered_options();
        assert!(options.dir.ends_with("Programs"));
    }

    #[test]
    fn reads_a_record_from_before_the_obs_plugin() {
        let record: Record = serde_json::from_str(
            r#"{"dir":"/Applications","version":"1.0.0","desktopShortcut":false}"#,
        )
        .unwrap();
        assert_eq!(record.obs, None);
        let json = serde_json::to_string(&record).unwrap();
        assert!(
            !json.contains("obs"),
            "the 1.0 updater reads it too: {json}"
        );
    }

    #[test]
    fn the_plugin_alone_leaves_the_app_as_it_was() {
        let sandbox = Sandbox::new();
        let layout = &sandbox.layout;
        let dir = layout.default_dir.clone();
        // An app the setup installed before.
        std::fs::create_dir_all(dir.join(APP)).unwrap();
        std::fs::create_dir_all(layout.state.parent().unwrap()).unwrap();
        let before = Record {
            dir: dir.display().to_string(),
            version: "1.0.0".into(),
            desktop_shortcut: false,
            obs: None,
        };
        std::fs::write(&layout.state, serde_json::to_vec(&before).unwrap()).unwrap();
        let options = Options {
            dir: "/elsewhere".into(),
            desktop_shortcut: true,
            app: false,
            obs: true,
        };
        let result = install(layout, &options, "1.1.0", &mut |_, _| {});
        if !obs::has_payload() {
            assert!(result.unwrap_err().contains("without the OBS plugin"));
            return;
        }
        result.unwrap();
        let record = layout.record().unwrap();
        assert_eq!(record.dir, before.dir, "the updater still finds the app");
        assert_eq!(record.version, "1.0.0");
        let plugin = layout.obs_installed().unwrap();
        assert_eq!(PathBuf::from(&plugin.dir), layout.obs.plugin_dir());
        assert_eq!(plugin.version, "1.1.0");
        assert!(layout.remembered_options().obs, "picked again next time");
        assert!(layout.remembered_options().app);

        uninstall(layout, &dir, true, &mut |_, _| {}).unwrap();
        assert!(!layout.obs.plugin_dir().exists(), "uninstalling removes it");
    }

    #[test]
    fn nothing_picked_is_refused() {
        let sandbox = Sandbox::new();
        let options = Options {
            dir: sandbox.layout.default_dir.display().to_string(),
            desktop_shortcut: false,
            app: false,
            obs: false,
        };
        assert!(install(&sandbox.layout, &options, "1.1.0", &mut |_, _| {}).is_err());
    }

    #[test]
    fn the_plugin_is_preselected_once_obs_is_there() {
        let sandbox = Sandbox::new();
        let layout = &sandbox.layout;
        assert!(!layout.remembered_options().obs);
        std::fs::create_dir_all(sandbox._dir.path().join("OBS/obs-studio")).unwrap();
        assert_eq!(layout.remembered_options().obs, obs::has_payload());
        assert!(layout.remembered_options().app);
    }

    #[test]
    fn registers_and_unregisters_everything() {
        let sandbox = Sandbox::new();
        let layout = &sandbox.layout;
        let dir = layout.default_dir.clone();
        std::fs::create_dir_all(dir.join(APP)).unwrap();
        std::fs::create_dir_all(&layout.data[0]).unwrap();
        std::fs::write(layout.data[0].join("uwupulse.db"), b"sessions").unwrap();
        let options = Options {
            dir: dir.display().to_string(),
            desktop_shortcut: true,
            app: true,
            obs: false,
        };
        shortcuts(layout, &dir, &options).unwrap();
        #[cfg(not(target_os = "macos"))]
        {
            let entry =
                std::fs::read_to_string(layout.applications.join(format!("{APP_ID}.desktop")))
                    .unwrap();
            assert!(entry.contains("\nName=UwUPulse\n"));
            assert!(entry.contains("/AppRun\"\n"), "the entry starts the AppDir");
        }
        std::fs::create_dir_all(layout.state.parent().unwrap()).unwrap();
        std::fs::write(
            &layout.state,
            serde_json::to_vec(&Record {
                dir: options.dir.clone(),
                version: "0.1.0".into(),
                desktop_shortcut: true,
                obs: None,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            layout.installed().unwrap().version.as_deref(),
            Some("0.1.0")
        );

        uninstall(layout, &dir, true, &mut |_, _| {}).unwrap();
        assert!(!dir.join(APP).exists());
        assert!(layout.installed().is_none());
        assert!(
            layout.data[0].join("uwupulse.db").exists(),
            "sessions and settings are kept"
        );

        std::fs::create_dir_all(dir.join(APP)).unwrap();
        uninstall(layout, &dir, false, &mut |_, _| {}).unwrap();
        assert!(
            !layout.data[0].exists(),
            "sessions and settings are deleted on request"
        );
    }

    #[test]
    fn a_folder_the_person_picked_gets_a_folder_of_ours() {
        let picked = PathBuf::from("/opt/tools");
        if cfg!(target_os = "macos") {
            assert_eq!(folder_for(picked.clone()), picked);
        } else {
            assert_eq!(folder_for(picked), PathBuf::from("/opt/tools/uwupulse"));
            assert_eq!(
                folder_for(PathBuf::from("/opt/uwupulse")),
                PathBuf::from("/opt/uwupulse")
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn exec_paths_are_quoted_for_the_desktop_file() {
        assert_eq!(
            quote_exec("/home/nyu/a b/AppRun"),
            "\"/home/nyu/a b/AppRun\""
        );
        assert_eq!(quote_exec("/x/$y/100%"), "\"/x/\\$y/100%%\"");
    }
}
