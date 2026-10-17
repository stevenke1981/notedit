//! Crash-recovery journal for modified editor buffers.
//!
//! Recovery is deliberately separate from the normal session file. Session
//! restore re-opens files that already exist on disk; this journal stores the
//! *unsaved in-memory text* and must never overwrite a source file by itself.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

const RECOVERY_FILE: &str = "recovery.json";
const RECOVERY_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryDoc {
    pub path: Option<PathBuf>,
    pub title: String,
    pub text: String,
    pub eol: String,
    pub encoding: String,
    pub bom: bool,
    pub lang: String,
    pub cursor: usize,
    /// On-disk state when this recovery snapshot was captured. Optional so
    /// journals produced by NotEdit v0.2.x before file watching remain readable.
    pub disk_len: Option<u64>,
    pub disk_modified_ns: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoverySession {
    pub version: u32,
    pub docs: Vec<RecoveryDoc>,
}

impl RecoverySession {
    pub fn new(docs: Vec<RecoveryDoc>) -> Self {
        Self { version: RECOVERY_VERSION, docs }
    }
}

fn file(config_dir: &Path) -> PathBuf {
    config_dir.join(RECOVERY_FILE)
}

pub fn load(config_dir: &Path) -> io::Result<Option<RecoverySession>> {
    let path = file(config_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let session: RecoverySession = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if session.version != RECOVERY_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported recovery version {}", session.version),
        ));
    }
    Ok(Some(session))
}

pub fn save(config_dir: &Path, session: &RecoverySession) -> io::Result<()> {
    if session.docs.is_empty() {
        return clear(config_dir);
    }
    std::fs::create_dir_all(config_dir)?;
    let json = serde_json::to_vec_pretty(session).map_err(io::Error::other)?;
    crate::file_io::durable_write(&file(config_dir), &json)
}

pub fn clear(config_dir: &Path) -> io::Result<()> {
    match std::fs::remove_file(file(config_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("notedit-recovery-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> RecoverySession {
        RecoverySession::new(vec![RecoveryDoc {
            path: Some(PathBuf::from("/tmp/example.txt")),
            title: "example.txt".into(),
            text: "unsaved\ntext".into(),
            eol: "lf".into(),
            encoding: "UTF-8".into(),
            bom: false,
            lang: "text".into(),
            cursor: 7,
            disk_len: Some(12),
            disk_modified_ns: Some(123_456_789),
        }])
    }

    #[test]
    fn roundtrip() {
        let dir = temp_dir();
        let expected = sample();
        save(&dir, &expected).unwrap();
        assert_eq!(load(&dir).unwrap(), Some(expected));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_session_removes_journal() {
        let dir = temp_dir();
        save(&dir, &sample()).unwrap();
        save(&dir, &RecoverySession::new(vec![])).unwrap();
        assert_eq!(load(&dir).unwrap(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loads_older_journal_without_disk_stamp_fields() {
        let dir = temp_dir();
        let json = r#"{
          "version": 1,
          "docs": [{
            "path": "/tmp/example.txt",
            "title": "example.txt",
            "text": "old journal",
            "eol": "lf",
            "encoding": "UTF-8",
            "bom": false,
            "lang": "text",
            "cursor": 3
          }]
        }"#;
        std::fs::write(dir.join(RECOVERY_FILE), json).unwrap();
        let loaded = load(&dir).unwrap().unwrap();
        assert_eq!(loaded.docs.len(), 1);
        assert_eq!(loaded.docs[0].disk_len, None);
        assert_eq!(loaded.docs[0].disk_modified_ns, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

}
