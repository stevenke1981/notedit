//! Durable file writes for user documents.
//!
//! We never write directly into the destination path. The new contents are
//! written and flushed to a sibling temporary file first, then installed with
//! std::fs::rename. The standard library replaces an existing destination and
//! keeps the temporary file on the same mount/volume as the target.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn sibling_temp_path(path: &Path, kind: &str) -> PathBuf {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("document");
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".{name}.notedit-{kind}-{}-{n}", std::process::id()))
}

fn create_sibling_temp(path: &Path) -> io::Result<(PathBuf, File)> {
    for _ in 0..32 {
        let temp = sibling_temp_path(path, "tmp");
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "unable to allocate a unique temporary file"))
}

/// Write a complete document without truncating the destination first.
///
/// Existing permissions are copied to the staged file when possible. The
/// temporary file is fsync'd before it replaces the destination.
pub fn durable_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let old_metadata = fs::metadata(path).ok();
    if old_metadata.as_ref().is_some_and(|m| m.permissions().readonly()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "destination is read-only"));
    }
    let old_permissions = old_metadata.map(|m| m.permissions());
    let (temp, mut file) = create_sibling_temp(path)?;

    let staged = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        if let Some(permissions) = old_permissions {
            file.set_permissions(permissions)?;
        }
        drop(file);
        replace_destination(&temp, path)
    })();

    if staged.is_err() {
        let _ = fs::remove_file(&temp);
    }
    staged
}

fn replace_destination(temp: &Path, path: &Path) -> io::Result<()> {
    fs::rename(temp, path)?;

    // Best-effort directory sync so the rename itself survives a sudden loss
    // of power on Unix filesystems that support syncing directories.
    #[cfg(unix)]
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn dir() -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("notedit-file-io-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn writes_new_file() {
        let d = dir();
        let p = d.join("new.txt");
        durable_write(&p, b"hello").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"hello");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn replaces_existing_file() {
        let d = dir();
        let p = d.join("existing.txt");
        fs::write(&p, b"old").unwrap();
        durable_write(&p, b"new contents").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new contents");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn refuses_read_only_destination() {
        let d = dir();
        let p = d.join("readonly.txt");
        fs::write(&p, b"keep me").unwrap();
        let mut permissions = fs::metadata(&p).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&p, permissions).unwrap();

        let err = durable_write(&p, b"must not replace").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(&p).unwrap(), b"keep me");

        let mut permissions = fs::metadata(&p).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&p, permissions).unwrap();
        fs::remove_dir_all(d).unwrap();
    }
}
