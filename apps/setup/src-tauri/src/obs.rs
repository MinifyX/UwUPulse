//! The UwUPulse OBS plugin: whether OBS Studio is there, where OBS 31 loads
//! plugins from, and putting the plugin there or taking it away again.
//!
//! The folders are the ones OBS itself searches for one user's plugins
//! (`AddExtraModulePaths` in OBS's `obs-app.cpp`):
//!
//! - **macOS:** `~/Library/Application Support/obs-studio/plugins/uwupulse.plugin`
//! - **Windows:** `%ProgramData%\obs-studio\plugins\uwupulse` — OBS 31 has no
//!   per-user folder on Windows. The setup runs without administrator rights;
//!   ProgramData lets every user create folders. So every folder the setup
//!   makes there is created with an access list that lets other users only
//!   read it (never protected afterwards, which would leave a gap), working
//!   folders get random names and must not exist yet, and folders that are
//!   already there must belong to this user, SYSTEM, Administrators or
//!   TrustedInstaller, or the setup stops.
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
        self.prepare_plugins()?;
        let staging =
            payload::fresh_dir(&self.plugins, ".uwupulse-setup-", create_new).map_err(|e| {
                denied(format!(
                    "Couldn't create a folder in {}: {e}",
                    self.plugins.display()
                ))
            })?;
        let result = check_shared(&staging)
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
            .and_then(|()| protect(&staging.join(PLUGIN)))
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

    /// Makes sure the plugins folder is there. On Windows `obs-studio` and
    /// `plugins` are created protected when missing, and checked when not.
    #[cfg(windows)]
    fn prepare_plugins(&self) -> Result<(), String> {
        if let Some(obs_studio) = self.plugins.parent() {
            ensure_shared(obs_studio)?;
        }
        ensure_shared(&self.plugins)
    }

    #[cfg(not(windows))]
    fn prepare_plugins(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.plugins)
            .map_err(|e| format!("Couldn't create {}: {e}", self.plugins.display()))
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

/// Creates a folder that must not exist yet; on Windows protected from the
/// start (see [`protect`]).
fn create_new(dir: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        system::create_dir_shared(dir)
    }
    #[cfg(not(windows))]
    {
        std::fs::create_dir(dir)
    }
}

/// Windows: `dir` exists afterwards, and either the setup made it (protected)
/// or it belongs to someone trusted.
#[cfg(windows)]
fn ensure_shared(dir: &Path) -> Result<(), String> {
    match system::create_dir_shared(dir) {
        Ok(()) => check_shared(dir),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => check_shared(dir),
        Err(e) => Err(denied(format!("Couldn't create {}: {e}", dir.display()))),
    }
}

/// Windows: `dir` is a real folder (no link or junction pointing elsewhere)
/// owned by this user, SYSTEM, Administrators or TrustedInstaller.
#[cfg(windows)]
fn check_shared(dir: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let meta = std::fs::symlink_metadata(dir)
        .map_err(|e| format!("Couldn't read {}: {e}", dir.display()))?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(foreign_message(dir, "link", system::is_german()));
    }
    let owner = system::owner_sid(dir)?;
    if trusted_owner(&owner, &system::current_user_sid()?) {
        Ok(())
    } else {
        Err(foreign_message(dir, &owner, system::is_german()))
    }
}

#[cfg(not(windows))]
fn check_shared(_dir: &Path) -> Result<(), String> {
    Ok(())
}

/// Owners whose folders in ProgramData the setup builds on: this user and
/// the system's own accounts. Anyone else could change the plugin.
#[cfg_attr(not(windows), allow(dead_code))]
fn trusted_owner(owner: &str, user: &str) -> bool {
    const SYSTEM: &[&str] = &[
        "S-1-5-18",                                                       // SYSTEM
        "S-1-5-32-544",                                                   // Administrators
        "S-1-5-80-956008885-3418522649-1831038044-1858292187-2669553620", // TrustedInstaller
    ];
    owner == user || SYSTEM.contains(&owner)
}

/// Why the setup stops at a folder it can't trust, in the system's language.
#[cfg_attr(not(windows), allow(dead_code))]
fn foreign_message(dir: &Path, owner: &str, german: bool) -> String {
    let dir = dir.display();
    if german {
        format!(
            "Der Ordner {dir} gehört einem anderen Benutzer ({owner}) oder ist eine Verknüpfung. \
             Das OBS-Plugin wird dort nicht installiert, weil dieser Benutzer es verändern könnte.\n\n\
             Bitte einen Administrator, den Ordner zu prüfen, oder installiere nur die Desktop-App."
        )
    } else {
        format!(
            "The folder {dir} belongs to another user ({owner}) or is a link. \
             The OBS plugin isn't installed there, because that user could change it.\n\n\
             Ask an administrator to check the folder, or install only the desktop app."
        )
    }
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
    fn working_folders_get_new_random_names() {
        let root = tempfile::tempdir().unwrap();
        let a = payload::fresh_dir(root.path(), ".uwupulse-setup-", create_new).unwrap();
        let b = payload::fresh_dir(root.path(), ".uwupulse-setup-", create_new).unwrap();
        assert_ne!(a, b);
        for dir in [&a, &b] {
            let name = dir.file_name().unwrap().to_str().unwrap();
            let random = name.strip_prefix(".uwupulse-setup-").unwrap();
            assert_eq!(random.len(), 32, "{name}");
            assert!(random.bytes().all(|c| c.is_ascii_hexdigit()), "{name}");
            assert!(dir.is_dir());
        }
    }

    #[test]
    fn a_folder_that_is_there_already_is_never_taken_over() {
        let root = tempfile::tempdir().unwrap();
        let there = root.path().join("there");
        std::fs::create_dir(&there).unwrap();
        let error = create_new(&there).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        // fresh_dir picks another name instead.
        let names = std::cell::RefCell::new(Vec::new());
        let dir = payload::fresh_dir(root.path(), "x-", |dir| {
            names.borrow_mut().push(dir.to_path_buf());
            if names.borrow().len() == 1 {
                Err(std::io::ErrorKind::AlreadyExists.into())
            } else {
                create_new(dir)
            }
        })
        .unwrap();
        let names = names.into_inner();
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], dir);
        // … and gives up after a few tries.
        let error = payload::fresh_dir(root.path(), "x-", |_| {
            Err(std::io::ErrorKind::AlreadyExists.into())
        })
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        // Other errors stop at once.
        let error = payload::fresh_dir(&root.path().join("missing"), "x-", create_new).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn only_the_user_and_the_system_are_trusted_owners() {
        let user = "S-1-5-21-1-2-3-1001";
        assert!(trusted_owner(user, user));
        assert!(trusted_owner("S-1-5-18", user));
        assert!(trusted_owner("S-1-5-32-544", user));
        assert!(trusted_owner(
            "S-1-5-80-956008885-3418522649-1831038044-1858292187-2669553620",
            user
        ));
        assert!(!trusted_owner("S-1-5-21-1-2-3-1002", user));
        assert!(!trusted_owner("S-1-5-32-545", user), "Users");
        assert!(!trusted_owner("S-1-1-0", user), "Everyone");
    }

    #[test]
    fn a_foreign_folder_is_explained_in_both_languages() {
        let dir = Path::new("plugins");
        let de = foreign_message(dir, "S-1-5-21-9", true);
        assert!(
            de.contains("gehört einem anderen Benutzer (S-1-5-21-9)"),
            "{de}"
        );
        assert!(de.contains("Administrator"), "{de}");
        let en = foreign_message(dir, "S-1-5-21-9", false);
        assert!(en.contains("belongs to another user (S-1-5-21-9)"), "{en}");
        assert!(en.contains("install only the desktop app"), "{en}");
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
