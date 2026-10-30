//! The UwUPulse OBS plugin: whether OBS Studio is there, where OBS 31 loads
//! plugins from, and putting the plugin there or taking it away again.
//!
//! The folders are the ones OBS itself searches for one user's plugins
//! (`AddExtraModulePaths` in OBS's `obs-app.cpp`):
//!
//! - **macOS:** `~/Library/Application Support/obs-studio/plugins/uwupulse.plugin`
//! - **Windows:** `%ProgramData%\obs-studio\plugins\uwupulse` — OBS 31 has no
//!   per-user folder on Windows. The setup runs without administrator rights;
//!   ProgramData lets every user create folders, and the setup gives the
//!   plugin's folder an access list that lets other users only read it.
//! - **Linux:** `$XDG_CONFIG_HOME/obs-studio/plugins/uwupulse`
//!
//! A running OBS is never touched: the plugin it loaded stays in use until
//! OBS starts again.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::payload;
use crate::system;

/// The plugin's folder, as OBS loads it.
pub const PLUGIN: &str = if cfg!(target_os = "macos") {
    "uwupulse.plugin"
} else {
    "uwupulse"
};

/// The plugin folder in a tar archive (see `build.rs`); empty where there is
/// no plugin (Linux on ARM).
static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/obs.zst"));
const PAYLOAD_SIZE: &str = env!("UWUPULSE_SETUP_OBS_PAYLOAD_SIZE");

pub fn has_payload() -> bool {
    !PAYLOAD.is_empty()
}

/// The installed plugin, as the setup remembers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    /// The plugin's own folder (`…/plugins/uwupulse`).
    pub dir: String,
    pub version: String,
}

/// What the setup page shows about OBS.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// The plugin is packed in this setup.
    pub available: bool,
    /// OBS Studio seems to be installed.
    pub detected: bool,
    /// OBS Studio is open right now: the new plugin loads when it restarts.
    pub running: bool,
    /// Where the plugin goes.
    pub dir: String,
    pub installed: Option<Installed>,
}

/// Where OBS and its plugins are on this machine.
pub struct Obs {
    /// The folder OBS searches for plugins.
    plugins: PathBuf,
    /// The plugin's data (key, paired phones, widgets): removed on request.
    data: PathBuf,
    /// Any of these existing means OBS is installed.
    probes: Vec<PathBuf>,
    /// … as does this program in one of the `PATH` folders.
    program: Option<(&'static str, OsString)>,
}

impl Obs {
    pub fn sandbox(root: &Path) -> Self {
        let obs = root.join("OBS");
        Self {
            plugins: obs.join("plugins"),
            data: obs.join("plugin_config").join("uwupulse"),
            probes: vec![obs.join("obs-studio")],
            program: None,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn real() -> Self {
        let home = system::home();
        let obs_studio = home.join("Library/Application Support/obs-studio");
        Self {
            plugins: obs_studio.join("plugins"),
            data: obs_studio.join("plugin_config").join("uwupulse"),
            probes: vec![
                PathBuf::from("/Applications/OBS.app"),
                home.join("Applications/OBS.app"),
                obs_studio.join("plugins"),
            ],
            program: None,
        }
    }

    #[cfg(windows)]
    pub fn real() -> Self {
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let program_data = env("ProgramData").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        let roaming = system::folders().roaming;
        let mut probes = vec![program_data.join("obs-studio")];
        probes.extend(env("ProgramFiles").map(|dir| dir.join("obs-studio")));
        probes.extend(system::obs_install_dir());
        Self {
            plugins: program_data.join("obs-studio").join("plugins"),
            data: roaming.join(r"obs-studio\plugin_config\uwupulse"),
            probes,
            program: None,
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub fn real() -> Self {
        let obs_studio = system::xdg("XDG_CONFIG_HOME", ".config").join("obs-studio");
        Self {
            plugins: obs_studio.join("plugins"),
            data: obs_studio.join("plugin_config").join("uwupulse"),
            probes: vec![PathBuf::from("/usr/share/obs")],
            program: std::env::var_os("PATH").map(|path| ("obs", path)),
        }
    }

    /// Where the plugin goes.
    pub fn plugin_dir(&self) -> PathBuf {
        self.plugins.join(PLUGIN)
    }

    /// Whether OBS Studio seems to be installed.
    pub fn detected(&self) -> bool {
        self.probes.iter().any(|probe| probe.exists())
            || self
                .program
                .as_ref()
                .is_some_and(|(name, path)| in_path(name, path))
    }

    pub fn info(&self, installed: Option<Installed>) -> Info {
        Info {
            available: has_payload(),
            detected: self.detected(),
            running: running(),
            dir: self.plugin_dir().display().to_string(),
            installed,
        }
    }

    /// Unpacks the plugin next to its place and swaps it in. `progress` gets
    /// 0..1.
    pub fn install(
        &self,
        version: &str,
        progress: &mut dyn FnMut(f64),
    ) -> Result<Installed, String> {
        let target = self.plugin_dir();
        std::fs::create_dir_all(&self.plugins)
            .map_err(|e| denied(format!("Couldn't create {}: {e}", self.plugins.display())))?;
        let staging = self
            .plugins
            .join(format!(".uwupulse-setup-{}", std::process::id()));
        payload::remove_tree(&staging);
        std::fs::create_dir_all(&staging)
            .map_err(|e| denied(format!("Couldn't create {}: {e}", staging.display())))?;
        let result = protect(&staging)
            .and_then(|()| {
                payload::unpack(
                    PAYLOAD,
                    PAYLOAD_SIZE,
                    PLUGIN,
                    "OBS plugin",
                    &staging,
                    progress,
                )
            })
            .and_then(|()| {
                payload::swap_in(&staging.join(PLUGIN), &target).map_err(|e| {
                    if cfg!(windows) && running() {
                        format!("{e}\n\nPlease close OBS Studio and try again.")
                    } else {
                        e
                    }
                })
            });
        payload::remove_tree(&staging);
        result?;
        Ok(Installed {
            dir: target.display().to_string(),
            version: version.to_string(),
        })
    }

    /// Removes an installed plugin and, unless kept, its data.
    pub fn uninstall(&self, installed: Option<&Installed>, keep_data: bool) -> Result<(), String> {
        if let Some(dir) = installed.and_then(|installed| owned_plugin_dir(&installed.dir)) {
            payload::remove_tree(&dir);
            if dir.exists() {
                return Err(format!(
                    "Couldn't remove {}. Is OBS Studio still open?",
                    dir.display()
                ));
            }
        }
        if !keep_data && installed.is_some() {
            payload::remove_tree(&self.data);
        }
        Ok(())
    }
}

/// A remembered plugin folder, if it is one the setup may delete: named like
/// the plugin and inside a `plugins` folder. A damaged or edited record never
/// makes the setup remove some other folder.
fn owned_plugin_dir(dir: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(dir);
    let named = dir.file_name().is_some_and(|name| name == PLUGIN);
    let in_plugins = dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "plugins");
    (dir.is_absolute() && named && in_plugins).then_some(dir)
}

/// Whether `name` is a program in one of the folders of `path` (`PATH`).
fn in_path(name: &str, path: &OsString) -> bool {
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .any(|dir| dir.join(name).is_file())
}

/// Whether an executable is OBS Studio's.
fn is_obs(exe: &Path) -> bool {
    let name = exe
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        name == "OBS" && exe.to_string_lossy().contains(".app/Contents/MacOS/")
    } else if cfg!(windows) {
        name.eq_ignore_ascii_case("obs64.exe")
    } else {
        name == "obs"
    }
}

/// Whether OBS Studio is open. Only looked at, never closed.
pub fn running() -> bool {
    system::executables().iter().any(|exe| is_obs(exe))
}

/// Windows: ProgramData is shared by every user of the computer. Other users
/// may read and run the plugin (OBS does, for them too) but not change it.
#[cfg(windows)]
fn protect(dir: &Path) -> Result<(), String> {
    system::restrict_to_user_shared(dir)
}

#[cfg(not(windows))]
fn protect(_dir: &Path) -> Result<(), String> {
    Ok(())
}

/// A clearer word for the one failure the plugin folder has that the app's
/// folder hasn't: on Windows it is outside the user's own folders.
fn denied(error: String) -> String {
    if cfg!(windows) && error.contains("(os error 5)") {
        format!("{error}\n\nWindows didn't allow writing to ProgramData. Ask an administrator to install the OBS plugin, or install only the desktop app.")
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_obs_by_its_folders() {
        let root = tempfile::tempdir().unwrap();
        let obs = Obs::sandbox(root.path());
        assert!(!obs.detected());
        std::fs::create_dir_all(root.path().join("OBS/obs-studio")).unwrap();
        assert!(obs.detected());
    }

    #[test]
    fn finds_obs_by_its_program_in_path() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let path = std::env::join_paths([root.path().join("missing"), bin.clone()]).unwrap();
        let mut obs = Obs::sandbox(root.path());
        obs.program = Some(("obs", path.clone()));
        assert!(!obs.detected());
        std::fs::write(bin.join("obs"), b"").unwrap();
        assert!(obs.detected());
        // A folder by that name is no program.
        assert!(!in_path(
            "bin",
            &std::env::join_paths([root.path()]).unwrap()
        ));
        // Relative PATH entries are not searched.
        assert!(!in_path("obs", &OsString::from("bin")));
    }

    #[test]
    fn the_plugin_goes_into_the_plugins_folder() {
        let root = tempfile::tempdir().unwrap();
        let obs = Obs::sandbox(root.path());
        assert_eq!(
            obs.plugin_dir(),
            root.path().join("OBS").join("plugins").join(PLUGIN)
        );
        let real = Obs::real().plugin_dir();
        assert!(real.ends_with(Path::new("obs-studio").join("plugins").join(PLUGIN)));
    }

    #[test]
    fn only_a_plugin_folder_is_ever_removed() {
        let root = tempfile::tempdir().unwrap();
        let plugin = root.path().join("plugins").join(PLUGIN);
        assert_eq!(
            owned_plugin_dir(&plugin.display().to_string()),
            Some(plugin)
        );
        for other in [
            root.path().join("plugins"),
            root.path().join("Documents").join(PLUGIN),
            root.path().join("plugins").join("other"),
            PathBuf::from("plugins").join(PLUGIN),
        ] {
            assert_eq!(owned_plugin_dir(&other.display().to_string()), None);
        }
    }

    #[test]
    fn tells_obs_apart_from_other_programs() {
        if cfg!(target_os = "macos") {
            assert!(is_obs(Path::new(
                "/Applications/OBS.app/Contents/MacOS/OBS"
            )));
            assert!(!is_obs(Path::new("/usr/local/bin/OBS")));
            assert!(!is_obs(Path::new(
                "/Applications/UwUPulse.app/Contents/MacOS/UwUPulse"
            )));
        } else if cfg!(windows) {
            assert!(is_obs(Path::new(
                r"C:\Program Files\obs-studio\bin\64bit\OBS64.EXE"
            )));
            assert!(!is_obs(Path::new(r"C:\Tools\obs.exe")));
        } else {
            assert!(is_obs(Path::new("/usr/bin/obs")));
            assert!(!is_obs(Path::new("/usr/bin/obs-ffmpeg-mux")));
        }
    }

    #[test]
    fn installs_and_removes_the_plugin() {
        let root = tempfile::tempdir().unwrap();
        let obs = Obs::sandbox(root.path());
        let target = obs.plugin_dir();
        if has_payload() {
            let installed = obs.install("1.0.0", &mut |_| {}).unwrap();
            assert_eq!(PathBuf::from(&installed.dir), target);
        } else {
            std::fs::create_dir_all(target.join("data")).unwrap();
        }
        std::fs::create_dir_all(&obs.data).unwrap();
        let installed = Installed {
            dir: target.display().to_string(),
            version: "1.0.0".into(),
        };
        obs.uninstall(Some(&installed), true).unwrap();
        assert!(!target.exists());
        assert!(obs.data.exists(), "paired phones and widgets are kept");
        obs.uninstall(Some(&installed), false).unwrap();
        assert!(!obs.data.exists(), "and deleted on request");
        assert_eq!(
            std::fs::read_dir(&obs.plugins).unwrap().count(),
            0,
            "nothing left in the plugins folder"
        );
    }

    #[test]
    fn a_setup_without_the_plugin_says_so() {
        if has_payload() {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let error = Obs::sandbox(root.path())
            .install("1.0.0", &mut |_| {})
            .unwrap_err();
        assert!(error.contains("without the OBS plugin"), "{error}");
    }
}
