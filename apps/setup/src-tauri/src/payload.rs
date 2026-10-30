//! Folders packed into the setup (see `build.rs`): the app on macOS and
//! Linux, the OBS plugin everywhere. Each is one tar archive compressed with
//! zstd, unpacked next to where it goes and then renamed into place.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The compressor's window (`WINDOW_LOG` in `build.rs`), which the
/// decompressor has to allow.
const WINDOW_LOG: u32 = 28;

/// A reader that tells how far it got.
struct Counting<'a, R> {
    inner: R,
    read: u64,
    report: &'a mut dyn FnMut(u64),
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.report)(self.read);
        Ok(n)
    }
}

/// Unpacks `payload` (`size` bytes once decompressed) into `staging`. Every
/// entry has to sit under the one top-level folder `top`; `what` names the
/// payload in errors ("app", "OBS plugin"). `progress` gets 0..1.
pub fn unpack(
    payload: &[u8],
    size: &str,
    top: &str,
    what: &str,
    staging: &Path,
    progress: &mut dyn FnMut(f64),
) -> Result<(), String> {
    if payload.is_empty() {
        return Err(format!(
            "This setup was built without the {what} inside (a development build)."
        ));
    }
    let damaged = |e: std::io::Error| format!("The packed {what} is damaged: {e}");
    let total: u64 = size.parse().unwrap_or(1).max(1);
    let mut decoder = zstd::Decoder::new(payload).map_err(damaged)?;
    decoder.window_log_max(WINDOW_LOG).map_err(damaged)?;
    let mut report = |read: u64| progress(read.min(total) as f64 / total as f64);
    let counting = Counting {
        inner: decoder,
        read: 0,
        report: &mut report,
    };
    let mut archive = tar::Archive::new(counting);
    archive.set_preserve_permissions(true);
    archive.set_preserve_mtime(false);
    archive.set_overwrite(true);
    for entry in archive.entries().map_err(damaged)? {
        let mut entry = entry.map_err(damaged)?;
        let first = entry
            .path()
            .map_err(damaged)?
            .components()
            .next()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        if first != top {
            return Err(format!(
                "The packed {what} holds something unexpected: {first}"
            ));
        }
        // `unpack_in` keeps every entry inside `staging`: absolute paths and
        // `..` in an archive are refused rather than followed.
        let placed = entry
            .unpack_in(staging)
            .map_err(|e| format!("Couldn't unpack {first}: {e}"))?;
        if !placed {
            return Err(format!("The packed {what} tried to write outside {top}."));
        }
    }
    if !staging.join(top).is_dir() {
        return Err(format!("The packed {what} has no {top} in it."));
    }
    Ok(())
}

/// 16 random bytes as hex: names nobody can guess ahead of the setup.
pub fn random_name() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// A new folder `<prefix><random>` in `parent`, made by `create`, which must
/// fail with `AlreadyExists` for a folder that is there already (as
/// `std::fs::create_dir` does). So a folder someone else put in place, even
/// under a guessed name, is never taken over.
pub fn fresh_dir(
    parent: &Path,
    prefix: &str,
    create: impl Fn(&Path) -> std::io::Result<()>,
) -> std::io::Result<PathBuf> {
    let mut last = None;
    for _ in 0..4 {
        let path = parent.join(format!("{prefix}{}", random_name()?));
        match create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::ErrorKind::AlreadyExists.into()))
}

/// Puts `incoming` where `target` is, and what was there before away. The old
/// version is moved aside first (under a random name, so nothing planted in
/// advance can block it), so a failure leaves one of the two in place.
pub fn swap_in(incoming: &Path, target: &Path) -> Result<(), String> {
    let aside = target.with_extension(format!(
        "old-{}",
        random_name().map_err(|e| format!("Couldn't pick a name: {e}"))?
    ));
    if std::fs::symlink_metadata(&aside).is_ok() {
        return Err(format!("{} is in the way.", aside.display()));
    }
    let had_old = std::fs::symlink_metadata(target).is_ok();
    if had_old {
        std::fs::rename(target, &aside)
            .map_err(|e| format!("Couldn't move the old {} aside: {e}", target.display()))?;
    }
    if let Err(e) = std::fs::rename(incoming, target) {
        if had_old {
            let _ = std::fs::rename(&aside, target);
        }
        return Err(format!("Couldn't put {} in place: {e}", target.display()));
    }
    if had_old {
        remove_tree(&aside);
    }
    Ok(())
}

/// Removes a folder UwUPulse owns. A symlink in its place is only unlinked,
/// never followed.
pub fn remove_tree(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed(entries: &[(&str, &[u8])]) -> (Vec<u8>, String) {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *data).unwrap();
        }
        let raw = builder.into_inner().unwrap();
        let size = raw.len().to_string();
        (zstd::encode_all(raw.as_slice(), 3).unwrap(), size)
    }

    #[test]
    fn unpacks_one_folder_and_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (payload, size) = packed(&[("uwupulse/data/locale/en-US.ini", b"x")]);
        let mut last = 0.0;
        unpack(
            &payload,
            &size,
            "uwupulse",
            "OBS plugin",
            dir.path(),
            &mut |f| last = f,
        )
        .unwrap();
        assert!(dir.path().join("uwupulse/data/locale/en-US.ini").exists());
        assert!(last > 0.0 && last <= 1.0, "{last}");
    }

    #[test]
    fn refuses_anything_beside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (payload, size) = packed(&[("uwupulse/a", b"x"), ("other/b", b"y")]);
        let error = unpack(
            &payload,
            &size,
            "uwupulse",
            "OBS plugin",
            dir.path(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.contains("unexpected: other"), "{error}");
        assert!(!dir.path().join("other").exists());
    }

    #[test]
    fn an_empty_payload_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let error =
            unpack(&[], "0", "uwupulse", "OBS plugin", dir.path(), &mut |_| {}).unwrap_err();
        assert!(error.contains("without the OBS plugin"));
    }

    #[test]
    fn swapping_keeps_one_version_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let target = dir.join("UwUPulse");
        let incoming = dir.join("incoming");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("old"), b"old").unwrap();
        std::fs::create_dir_all(&incoming).unwrap();
        std::fs::write(incoming.join("new"), b"new").unwrap();
        swap_in(&incoming, &target).unwrap();
        assert!(target.join("new").exists());
        assert!(!target.join("old").exists());
        assert_eq!(
            std::fs::read_dir(dir).unwrap().count(),
            1,
            "nothing left aside"
        );
    }

    #[test]
    fn random_names_are_new_each_time() {
        let a = random_name().unwrap();
        let b = random_name().unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
