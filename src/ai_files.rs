//! Backups and exclusive creation for automatic AI file actions.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn create(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = result {
        if let Err(cleanup) = fs::remove_file(path) {
            return Err(io::Error::new(e.kind(), format!("{e}；無法清除不完整檔案 {}：{cleanup}", path.display())));
        }
        return Err(e);
    }
    Ok(())
}

pub fn backup(path: &Path, disk: &[u8], buffer: &[u8]) -> io::Result<(PathBuf, PathBuf)> {
    let name = path.file_name().ok_or_else(|| io::Error::other("無效檔名"))?.to_string_lossy();
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
    let prefix = format!(".{name}.notedit-ai-{stamp}-{}", std::process::id());
    let disk_path = path.with_file_name(format!("{prefix}.disk.bak"));
    let buffer_path = path.with_file_name(format!("{prefix}.buffer.bak"));
    create(&disk_path, disk)?;
    create(&buffer_path, buffer)?;
    if fs::read(&disk_path)? != disk || fs::read(&buffer_path)? != buffer { return Err(io::Error::other("備份讀回驗證失敗")); }
    Ok((disk_path, buffer_path))
}

#[cfg(test)]
mod tests {
    #[test]
    fn backup_preserves_disk_and_unsaved_buffer_and_creation_never_overwrites() {
        let root = std::env::temp_dir().join(format!("notedit-ai-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("example.md");
        std::fs::write(&target, b"on disk").unwrap();
        let (disk, buffer) = super::backup(&target, b"on disk", b"unsaved changes").unwrap();
        assert_eq!(std::fs::read(&disk).unwrap(), b"on disk");
        assert_eq!(std::fs::read(&buffer).unwrap(), b"unsaved changes");
        assert!(super::create(&target, b"new").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"on disk");
        std::fs::remove_dir_all(root).unwrap();
    }
}
