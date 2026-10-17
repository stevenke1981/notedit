//! Lightweight external-file change detection.
//!
//! NotEdit deliberately starts with metadata polling instead of a platform
//! watcher crate. This keeps behavior deterministic across Windows, Linux and
//! macOS and is enough to detect the common save/delete/replace cases.

use std::io;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

pub fn stamp(path: &Path) -> io::Result<FileStamp> {
    let metadata = std::fs::metadata(path)?;
    Ok(FileStamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file() -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("notedit-file-watch-{}-{stamp}.txt", std::process::id()))
    }

    #[test]
    fn stamp_changes_when_file_is_replaced() {
        let p = temp_file();
        std::fs::write(&p, b"a").unwrap();
        let before = stamp(&p).unwrap();
        std::fs::write(&p, b"longer").unwrap();
        let after = stamp(&p).unwrap();
        assert_ne!(before, after);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn missing_file_is_not_found() {
        let p = temp_file();
        let err = stamp(&p).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
