//! Installing, updating and removing UwUPulse for the current Windows user.
//!
//! Everything lives under the user's profile: the app in
//! `%LOCALAPPDATA%\Programs\UwUPulse`, shortcuts in the Start menu (and on the
//! desktop if wanted), and registry entries under `HKEY_CURRENT_USER`. No
//! administrator rights are needed.
//!
//! The OBS plugin, when picked, goes to `%ProgramData%\obs-studio\plugins`
//! (`obs.rs`), the one folder OBS 31 loads plugins from that needs no
//! administrator; the registry notes it next to the app.
//!
//! `UWUPULSE_SETUP_SANDBOX=<folder>` redirects all of it (files, shortcuts,
//! registry under `HKCU\Software\UwUPulse-Setup-Sandbox`) for testing.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

use crate::obs::{self, Obs};
use crate::system;

pub const APP_EXE: &str = "UwUPulse.exe";
pub const UNINSTALL_EXE: &str = "uninstall.exe";
pub const APP_ID: &str = "app.uwupulse.desktop";
const SHORTCUT: &str = "UwUPulse.lnk";
const HOMEPAGE: &str = "https://github.com/MinifyX/UwUPulse";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\UwUPulse";
const SETUP_KEY: &str = r"Software\UwUPulse\Setup";

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

/// Where things go on this machine.
pub struct Layout {
    pub default_dir: PathBuf,
    pub start_menu: PathBuf,
    pub desktop: PathBuf,
    pub roaming_data: PathBuf,
    pub local_data: PathBuf,
    registry_prefix: String,
    pub obs: Obs,
    pub sandbox: bool,
}

impl Layout {
    pub fn detect() -> Self {
        match std::env::var_os("UWUPULSE_SETUP_SANDBOX").filter(|dir| !dir.is_empty()) {
            Some(dir) => Self::sandbox(Path::new(&dir)),
            None => {
                let folders = system::folders();
                Self {
                    default_dir: folders.user_programs.join("UwUPulse"),
                    start_menu: folders.start_menu,
                    desktop: folders.desktop,
                    roaming_data: folders.roaming.join(APP_ID),
                    local_data: folders.local.join(APP_ID),
                    registry_prefix: String::new(),
                    obs: Obs::real(),
                    sandbox: false,
                }
            }
        }
    }

    pub fn sandbox(root: &Path) -> Self {
        Self {
            default_dir: root.join(r"Programs\UwUPulse"),
            start_menu: root.join("StartMenu"),
            desktop: root.join("Desktop"),
            roaming_data: root.join("Roaming").join(APP_ID),
            local_data: root.join("Local").join(APP_ID),
            registry_prefix: format!(
                r"Software\UwUPulse-Setup-Sandbox\{}\",
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            obs: Obs::sandbox(root),
            sandbox: true,
        }
    }

    fn key(&self, path: &str) -> String {
        format!("{}{path}", self.registry_prefix)
    }

    fn open(&self, path: &str) -> Option<RegKey> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(self.key(path), KEY_READ)
            .ok()
    }

    fn create(&self, path: &str) -> Result<RegKey, String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(self.key(path))
            .map(|(key, _)| key)
            .map_err(|e| format!("Couldn't write to the registry ({path}): {e}"))
    }

    fn remove_tree(&self, path: &str) {
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(self.key(path));
    }

    /// What is installed right now, if anything.
    pub fn installed(&self) -> Option<Installed> {
        let key = self.open(SETUP_KEY)?;
        let dir = key.get_value::<String, _>("InstallDir").ok()?;
        Path::new(&dir).join(APP_EXE).exists().then(|| Installed {
            dir,
            version: key.get_value("Version").ok(),
        })
    }

    /// The OBS plugin the setup installed, if it is still there.
    pub fn obs_installed(&self) -> Option<obs::Installed> {
        let key = self.open(SETUP_KEY)?;
        let dir = key.get_value::<String, _>("ObsPlugin").ok()?;
        Path::new(&dir).exists().then(|| obs::Installed {
            dir,
            version: key.get_value("ObsVersion").unwrap_or_default(),
        })
    }

    /// Options from the last install, or the defaults: the app unless only
    /// the plugin was installed, the plugin if it was or OBS is there.
    pub fn remembered_options(&self) -> Options {
        let key = self.open(SETUP_KEY);
        let app = key.is_none() || self.installed().is_some();
        let obs_wanted =
            obs::has_payload() && (self.obs_installed().is_some() || self.obs.detected());
        let flag = |name: &str, default: bool| {
            key.as_ref()
                .and_then(|k| k.get_value::<u32, _>(name).ok())
                .map_or(default, |v| v != 0)
        };
        let dir = self.installed().map_or_else(
            || self.default_dir.display().to_string(),
            |installed| installed.dir,
        );
        Options {
            dir,
            desktop_shortcut: flag("DesktopShortcut", true),
            app,
            obs: obs_wanted,
        }
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

/// Writes a file next to its destination first, then swaps it in. A running
/// program keeps its file locked for a moment after it ends, hence the retries.
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < 19 => std::thread::sleep(Duration::from_millis(250)),
            Err(e) => return Err(format!("Couldn't replace {}: {e}", to.display())),
        }
        attempt += 1;
    }
}

/// Unpack the packed program to `target`, reporting progress through the copy
/// step.
fn extract(payload: &[u8], size: &str, target: &Path, progress: Progress) -> Result<(), String> {
    if payload.is_empty() {
        return Err("This setup was built without UwUPulse inside (a development build).".into());
    }
    let total: u64 = size.parse().unwrap_or(1).max(1);
    let mut decoder =
        zstd::Decoder::new(payload).map_err(|e| format!("The packed app is damaged: {e}"))?;
    let mut file = std::fs::File::create(target)
        .map_err(|e| format!("Couldn't write {}: {e}", target.display()))?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut written = 0u64;
    loop {
        let read = decoder
            .read(&mut buffer)
            .map_err(|e| format!("The packed app is damaged: {e}"))?;
        if read == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..read])
            .map_err(|e| format!("Couldn't write {}: {e}", target.display()))?;
        written += read as u64;
        progress(Step::Copy, written as f64 / total as f64);
    }
    file.sync_all()
        .map_err(|e| format!("Couldn't write {}: {e}", target.display()))?;
    if written != total {
        return Err("The packed app is incomplete.".into());
    }
    Ok(())
}

fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn dir_size_kb(dir: &Path) -> u32 {
    let bytes: u64 = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum();
    (bytes / 1024).min(u32::MAX as u64) as u32
}

/// Stops UwUPulse if it runs from `dir`. Skipped in the sandbox, which must never
/// touch real processes.
pub fn stop_app(layout: &Layout, dir: &Path) -> Result<(), String> {
    if layout.sandbox {
        return Ok(());
    }
    system::stop_processes(&dir.join(APP_EXE))
}

/// UwUPulse is open. Closing it stops the live heart rate, the overlay and a
/// running recording, so the setup asks first.
pub fn app_running(layout: &Layout, dir: &Path) -> bool {
    !layout.sandbox && !system::processes_of(&dir.join(APP_EXE)).is_empty()
}

/// The folder the setup installs into for a folder the person picked: a
/// "UwUPulse" folder inside it, unless it is one already.
pub fn folder_for(mut picked: PathBuf) -> PathBuf {
    if !picked
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("UwUPulse"))
    {
        picked.push("UwUPulse");
    }
    picked
}

/// Starts the installed UwUPulse. Nothing in the sandbox.
pub fn launch(layout: &Layout, dir: &Path) -> Result<(), String> {
    if layout.sandbox {
        return Ok(());
    }
    system::spawn_detached(&dir.join(APP_EXE), &[])
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
    // The uninstaller and the registry entry live here even when only the
    // plugin is installed: that is how it comes off again.
    let dir = PathBuf::from(options.dir.trim());
    if !dir.is_absolute() {
        return Err("Please pick a full folder path.".into());
    }
    let split = match (options.app, options.obs) {
        (true, true) => 0.85,
        (true, false) => 1.0,
        _ => 0.0,
    };
    progress(Step::Prepare, 0.0);
    let existed = dir.exists();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    // Outside the user's own folders (say D:\Apps\UwUPulse) the folder would
    // inherit an access list that lets other users replace UwUPulse.exe. Only
    // a folder the setup made, or an earlier UwUPulse install: never rewrite
    // the permissions of some existing folder full of other things.
    if !layout.sandbox
        && !system::in_user_folders(&dir, &system::folders())
        && (!existed || dir.join(APP_EXE).exists())
    {
        system::restrict_to_user(&dir)?;
    }
    if options.app {
        stop_app(layout, &dir)?;
    }
    progress(Step::Prepare, 1.0);

    let app = dir.join(APP_EXE);
    if options.app {
        let incoming = dir.join("UwUPulse.exe.new");
        extract(PAYLOAD, PAYLOAD_SIZE, &incoming, &mut |step, f| {
            progress(step, f * split)
        })?;
        replace_file(&incoming, &app)?;
    }
    // OBS may be open: it keeps the plugin it loaded until it restarts.
    let plugin = if options.obs {
        Some(layout.obs.install(version, &mut |f| {
            progress(Step::Copy, split + f * (1.0 - split))
        })?)
    } else {
        layout.obs_installed()
    };

    let uninstaller = dir.join(UNINSTALL_EXE);
    let me =
        std::env::current_exe().map_err(|e| format!("Couldn't find the setup program: {e}"))?;
    if !me.eq(&uninstaller) {
        let copy = dir.join("uninstall.exe.new");
        std::fs::copy(&me, &copy).map_err(|e| format!("Couldn't write {}: {e}", copy.display()))?;
        replace_file(&copy, &uninstaller)?;
    }

    progress(Step::Shortcuts, 0.0);
    if options.app {
        let shortcut = system::Shortcut {
            target: &app,
            arguments: "",
            description: "UwUPulse",
            app_id: APP_ID,
        };
        system::create_shortcut(&layout.start_menu.join(SHORTCUT), &shortcut)?;
        let on_desktop = layout.desktop.join(SHORTCUT);
        if options.desktop_shortcut {
            system::create_shortcut(&on_desktop, &shortcut)?;
        } else {
            let _ = std::fs::remove_file(&on_desktop);
        }
    }
    progress(Step::Shortcuts, 1.0);

    progress(Step::Register, 0.0);
    register(layout, &dir, options, version, plugin.as_ref())?;
    progress(Step::Register, 1.0);
    progress(Step::Done, 1.0);
    Ok(())
}

fn register(
    layout: &Layout,
    dir: &Path,
    options: &Options,
    version: &str,
    plugin: Option<&obs::Installed>,
) -> Result<(), String> {
    let app = dir.join(APP_EXE);
    // Only the plugin this time, next to an app from before: the app's
    // version and shortcut choice stay what they were.
    let app_before = !options.app && app.exists();
    let before = layout.open(SETUP_KEY);
    let kept_version = app_before
        .then(|| before.as_ref()?.get_value::<String, _>("Version").ok())
        .flatten();
    let kept_shortcut = app_before
        .then(|| before.as_ref()?.get_value::<u32, _>("DesktopShortcut").ok())
        .flatten();
    let icon = if app.exists() {
        app.clone()
    } else {
        dir.join(UNINSTALL_EXE)
    };
    let uninstaller = dir.join(UNINSTALL_EXE);
    let write = |key: &RegKey, name: &str, value: &str| {
        key.set_value(name, &value)
            .map_err(|e| format!("Couldn't write to the registry ({name}): {e}"))
    };
    let write_dword = |key: &RegKey, name: &str, value: u32| {
        key.set_value(name, &value)
            .map_err(|e| format!("Couldn't write to the registry ({name}): {e}"))
    };

    // Start from a clean entry: an older setup may have written other values.
    layout.remove_tree(UNINSTALL_KEY);
    let entry = layout.create(UNINSTALL_KEY)?;
    write(&entry, "DisplayName", "UwUPulse")?;
    write(&entry, "DisplayVersion", version)?;
    write(&entry, "Publisher", "UwUPulse")?;
    write(&entry, "DisplayIcon", &format!("{},0", icon.display()))?;
    write(&entry, "InstallLocation", &dir.display().to_string())?;
    write(
        &entry,
        "UninstallString",
        &format!("{} --uninstall", quoted(&uninstaller)),
    )?;
    write(&entry, "URLInfoAbout", HOMEPAGE)?;
    write(&entry, "HelpLink", HOMEPAGE)?;
    write_dword(&entry, "EstimatedSize", dir_size_kb(dir))?;
    write_dword(&entry, "NoModify", 1)?;
    write_dword(&entry, "NoRepair", 1)?;

    let setup = layout.create(SETUP_KEY)?;
    write(&setup, "InstallDir", &dir.display().to_string())?;
    write(
        &setup,
        "Version",
        kept_version.as_deref().unwrap_or(version),
    )?;
    write_dword(
        &setup,
        "DesktopShortcut",
        kept_shortcut.unwrap_or(options.desktop_shortcut.into()),
    )?;
    match plugin {
        Some(plugin) => {
            write(&setup, "ObsPlugin", &plugin.dir)?;
            write(&setup, "ObsVersion", &plugin.version)?;
        }
        None => {
            let _ = setup.delete_value("ObsPlugin");
            let _ = setup.delete_value("ObsVersion");
        }
    }
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
    let _ = std::fs::remove_file(layout.start_menu.join(SHORTCUT));
    let _ = std::fs::remove_file(layout.desktop.join(SHORTCUT));
    progress(Step::Shortcuts, 1.0);

    progress(Step::Register, 0.0);
    let plugin = layout.obs_installed();
    layout.remove_tree(UNINSTALL_KEY);
    layout.remove_tree(r"Software\UwUPulse");
    progress(Step::Register, 1.0);

    progress(Step::Copy, 0.0);
    for file in [
        APP_EXE,
        UNINSTALL_EXE,
        "UwUPulse.exe.new",
        "uninstall.exe.new",
    ] {
        let path = dir.join(file);
        if path.exists() && std::fs::remove_file(&path).is_err() && file == APP_EXE {
            return Err(format!(
                "Couldn't remove {}. Is UwUPulse still open?",
                path.display()
            ));
        }
    }
    let _ = std::fs::remove_dir(dir);
    layout.obs.uninstall(plugin.as_ref(), keep_data)?;
    progress(Step::Copy, 1.0);

    if !keep_data {
        progress(Step::Cleanup, 0.0);
        for folder in [&layout.roaming_data, &layout.local_data] {
            if folder.exists() {
                std::fs::remove_dir_all(folder)
                    .map_err(|e| format!("Couldn't delete {}: {e}", folder.display()))?;
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

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let key = self
                .layout
                .registry_prefix
                .trim_end_matches('\\')
                .to_string();
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(key);
        }
    }

    #[test]
    fn remembers_defaults_until_something_is_installed() {
        let sandbox = Sandbox::new();
        assert!(sandbox.layout.installed().is_none());
        let options = sandbox.layout.remembered_options();
        assert!(options.dir.ends_with(r"Programs\UwUPulse"));
        assert!(options.desktop_shortcut);
        assert!(options.app);
    }

    #[test]
    fn notes_the_plugin_next_to_the_app() {
        let sandbox = Sandbox::new();
        let layout = &sandbox.layout;
        let dir = layout.default_dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(APP_EXE), b"app").unwrap();
        let mut options = Options {
            dir: dir.display().to_string(),
            desktop_shortcut: false,
            app: true,
            obs: false,
        };
        register(layout, &dir, &options, "1.0.0", None).unwrap();
        assert!(layout.obs_installed().is_none());

        // Later only the plugin: the app keeps its version.
        let plugin_dir = layout.obs.plugin_dir();
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let plugin = obs::Installed {
            dir: plugin_dir.display().to_string(),
            version: "1.1.0".into(),
        };
        options.app = false;
        options.obs = true;
        options.desktop_shortcut = true;
        register(layout, &dir, &options, "1.1.0", Some(&plugin)).unwrap();
        assert_eq!(layout.obs_installed(), Some(plugin));
        assert_eq!(
            layout.installed().unwrap().version.as_deref(),
            Some("1.0.0")
        );
        assert!(!layout.remembered_options().desktop_shortcut);
        assert!(layout.remembered_options().obs);

        uninstall(layout, &dir, true, &mut |_, _| {}).unwrap();
        assert!(!plugin_dir.exists(), "uninstalling removes the plugin");
    }

    #[test]
    fn registers_and_unregisters_everything() {
        let sandbox = Sandbox::new();
        let layout = &sandbox.layout;
        let dir = layout.default_dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(APP_EXE), b"app").unwrap();
        std::fs::write(dir.join(UNINSTALL_EXE), b"setup").unwrap();
        let options = Options {
            dir: dir.display().to_string(),
            desktop_shortcut: false,
            app: true,
            obs: false,
        };
        register(layout, &dir, &options, "0.1.0", None).unwrap();

        let installed = layout.installed().unwrap();
        assert_eq!(installed.version.as_deref(), Some("0.1.0"));
        assert!(!layout.remembered_options().desktop_shortcut);
        let command: String = layout
            .open(UNINSTALL_KEY)
            .unwrap()
            .get_value("UninstallString")
            .unwrap();
        assert!(command.ends_with("\" --uninstall"));
        std::fs::create_dir_all(&layout.roaming_data).unwrap();
        std::fs::write(layout.roaming_data.join("uwupulse.db"), b"sessions").unwrap();
        uninstall(layout, &dir, true, &mut |_, _| {}).unwrap();
        assert!(!dir.exists());
        assert!(
            layout.roaming_data.join("uwupulse.db").exists(),
            "sessions and settings are kept"
        );
        assert!(layout.open(UNINSTALL_KEY).is_none());
        assert!(layout.open(SETUP_KEY).is_none());

        std::fs::create_dir_all(&dir).unwrap();
        uninstall(layout, &dir, false, &mut |_, _| {}).unwrap();
        assert!(
            !layout.roaming_data.exists(),
            "sessions and settings are deleted on request"
        );
    }
}
