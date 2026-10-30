//! What survives a restart, as small files in the config folder.
//!
//! - `link.key` — the desktop's static X25519 secret, 32 raw bytes, 0600
//! - `identity.json` — the desktop's ID and the API token, 0600
//! - `settings.json` — [`Settings`]
//! - `paired.json` — paired phones with their static keys
//! - `widgets.json` — overlay widgets, opaque JSON beyond `id`/`name`
//!
//! Every write goes to a temporary file first and is renamed over the old
//! one, so a crash mid-write leaves the previous version, never half a file.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uwupulse_proto::StaticKeypair;

use crate::types::Settings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Identity {
    pub id: String,
    pub token: String,
}

/// A paired phone as stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PairedRecord {
    pub id: String,
    pub name: String,
    /// Static public key, b64.
    pub spk: String,
    pub paired_at: i64,
    pub last_seen: Option<i64>,
}

pub(crate) struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
        })
    }

    /// Reads a JSON file. A missing file is `None`; a broken one is moved
    /// aside as `<name>.broken` (so nothing is lost) and also `None`.
    pub fn read<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        let path = self.dir.join(name);
        let bytes = fs::read(&path).ok()?;
        match serde_json::from_slice(&bytes) {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("{name} is not readable ({e}); starting over, old file kept as .broken");
                let _ = fs::rename(&path, self.dir.join(format!("{name}.broken")));
                None
            }
        }
    }

    pub fn write<T: Serialize>(&self, name: &str, value: &T, private: bool) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
        write_atomic(&self.dir.join(name), &bytes, private)
    }

    /// The desktop's static key, created on first start.
    pub fn link_key(&self) -> io::Result<StaticKeypair> {
        let path = self.dir.join("link.key");
        match fs::read(&path) {
            Ok(bytes) => match <[u8; 32]>::try_from(bytes.as_slice()) {
                Ok(secret) => return Ok(StaticKeypair::from_secret_bytes(secret)),
                Err(_) => {
                    // Paired phones will no longer recognise this desktop;
                    // they have to pair again.
                    log::warn!("link.key is damaged; creating a new key");
                    let _ = fs::rename(&path, self.dir.join("link.key.broken"));
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let key = StaticKeypair::generate();
        write_atomic(&path, &key.secret_bytes(), true)?;
        Ok(key)
    }
}

fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Settings with every value inside its range.
pub(crate) fn check_settings(s: &Settings) -> Result<(), String> {
    if s.port < 1024 {
        return Err("port must be between 1024 and 65535".into());
    }
    if !(10..=86_400).contains(&s.idle_timeout_secs) {
        return Err("idle timeout must be between 10 seconds and 24 hours".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let a = store.link_key().unwrap();
        let b = store.link_key().unwrap();
        assert_eq!(a.public_bytes(), b.public_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join("link.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn broken_json_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        fs::write(dir.path().join("settings.json"), b"{nope").unwrap();
        assert!(store.read::<Settings>("settings.json").is_none());
        assert!(dir.path().join("settings.json.broken").exists());
        store
            .write("settings.json", &Settings::default(), false)
            .unwrap();
        assert_eq!(
            store.read::<Settings>("settings.json"),
            Some(Settings::default())
        );
    }
}
