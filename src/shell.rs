//! OS file-manager integration ("使用 NotEdit 開啟") and single-instance forwarding.
//!
//! * Windows: per-user registry keys under `HKCU\Software\Classes` (no admin).
//! * Linux: a `.desktop` file in `~/.local/share/applications` (MimeType text/*).
//! * macOS: not supported (needs an app bundle / Finder extension).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_TEXT: &str = "使用 NotEdit 開啟";
#[cfg_attr(not(windows), allow(dead_code))]
pub const FOLDER_TEXT: &str = "使用 NotEdit 開啟資料夾";

/// One registry value to write: (key below HKCU, value name ("" = default), data).
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegValue { pub key: String, pub name: String, pub data: String }

/// The three context-menu verbs: files (`*`), folders, and folder background.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn registry_roots() -> [&'static str; 3] {
    [r"Software\Classes\*\shell\NotEdit", r"Software\Classes\Directory\shell\NotEdit", r"Software\Classes\Directory\Background\shell\NotEdit"]
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn registry_entries(exe: &Path) -> Vec<RegValue> {
    let exe = exe.display().to_string();
    let q = |a: &str| format!("\"{exe}\" {a}");
    let v = |key: String, name: &str, data: String| RegValue { key, name: name.into(), data };
    let [file, dir, bg] = registry_roots();
    let mut out = Vec::new();
    for (root, text, arg) in [(file, MENU_TEXT, "\"%1\""), (dir, FOLDER_TEXT, "\"%1\""), (bg, FOLDER_TEXT, "\"%V\"")] {
        out.push(v(root.into(), "", text.into()));
        out.push(v(root.into(), "Icon", format!("\"{exe}\",0")));
        // Explorer starts one process per selected file; MultiSelectModel=Player
        // keeps the verb visible for large selections, single-instance merges them.
        if root == file { out.push(v(root.into(), "MultiSelectModel", "Player".into())); }
        out.push(v(format!(r"{root}\command"), "", q(arg)));
    }
    out
}

#[cfg(windows)]
pub fn install_shell(exe: &Path) -> Result<String, String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for e in registry_entries(exe) {
        let (k, _) = hkcu.create_subkey(&e.key).map_err(|x| format!("{}: {x}", e.key))?;
        k.set_value(&e.name, &e.data).map_err(|x| format!("{}: {x}", e.key))?;
    }
    Ok("已安裝檔案總管右鍵選單「使用 NotEdit 開啟」（目前使用者）。".into())
}

#[cfg(windows)]
pub fn uninstall_shell() -> Result<String, String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for r in registry_roots() {
        match hkcu.delete_subkey_all(r) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{r}: {e}")),
        }
    }
    Ok("已移除檔案總管右鍵選單。".into())
}

/// Contents of the Linux `.desktop` launcher.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
pub fn desktop_entry(exe: &Path) -> String {
    let exe = exe.display().to_string();
    let exe = if exe.contains(' ') { format!("\"{exe}\"") } else { exe };
    format!("[Desktop Entry]\nType=Application\nName=NotEdit\nGenericName=Text Editor\nGenericName[zh_TW]=文字編輯器\n\
Comment=Notepad++-like text editor\nExec={exe} %F\nIcon=accessories-text-editor\nTerminal=false\nCategories=Utility;TextEditor;\n\
MimeType=text/plain;text/markdown;text/html;text/x-csrc;text/x-chdr;text/x-c++src;text/x-python;text/x-rust;text/x-shellscript;text/xml;text/css;text/csv;application/json;application/xml;application/x-yaml;application/toml;inode/directory;\n\
StartupNotify=true\nActions=new-window;\n\n[Desktop Action new-window]\nName=開啟 NotEdit\nExec={exe}\n")
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn desktop_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("applications").join("notedit.desktop")
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn install_shell(exe: &Path) -> Result<String, String> {
    let p = desktop_path();
    if let Some(d) = p.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
    std::fs::write(&p, desktop_entry(exe)).map_err(|e| format!("{}: {e}", p.display()))?;
    let _ = std::process::Command::new("update-desktop-database").arg(p.parent().unwrap()).status();
    Ok(format!("已安裝 {}，檔案管理員的「用其他應用程式開啟」會出現 NotEdit。", p.display()))
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn uninstall_shell() -> Result<String, String> {
    let p = desktop_path();
    match std::fs::remove_file(&p) {
        Ok(()) => { let _ = std::process::Command::new("update-desktop-database").arg(p.parent().unwrap()).status(); Ok(format!("已移除 {}。", p.display())) }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("尚未安裝。".into()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(target_os = "macos")]
pub fn install_shell(_exe: &Path) -> Result<String, String> { Err("macOS 尚不支援 Finder 右鍵選單；請在 Finder 使用「打開檔案的應用程式」選擇 notedit。".into()) }
#[cfg(target_os = "macos")]
pub fn uninstall_shell() -> Result<String, String> { install_shell(Path::new("")) }

// ---------------------------------------------------------------- single instance

const MAGIC: &str = "NOTEDIT-OPEN-1";

fn lock_file() -> PathBuf { crate::settings::file().with_file_name("instance.lock") }

/// Encode the forwarding message (token, then one absolute path per line).
pub fn encode(token: &str, paths: &[PathBuf]) -> String {
    let mut s = format!("{MAGIC} {token}\n");
    for p in paths { s.push_str(&p.display().to_string()); s.push('\n'); }
    s
}

pub fn decode(msg: &str, token: &str) -> Option<Vec<PathBuf>> {
    let mut lines = msg.lines();
    if lines.next()? != format!("{MAGIC} {token}") { return None; }
    Some(lines.filter(|l| !l.is_empty()).map(PathBuf::from).collect())
}

/// Try handing `paths` to an already running NotEdit. Returns true when it accepted.
pub fn forward(paths: &[PathBuf]) -> bool { forward_at(&lock_file(), paths) }

fn forward_at(lock: &Path, paths: &[PathBuf]) -> bool {
    let Ok(s) = std::fs::read_to_string(lock) else { return false };
    let mut it = s.split_whitespace();
    let (Some(port), Some(token)) = (it.next().and_then(|p| p.parse::<u16>().ok()), it.next()) else { return false };
    let Ok(mut c) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), std::time::Duration::from_millis(500)) else { return false };
    let _ = c.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    if c.write_all(encode(token, paths).as_bytes()).is_err() { return false; }
    let _ = c.shutdown(std::net::Shutdown::Write);
    let mut ack = String::new();
    c.read_to_string(&mut ack).is_ok() && ack.starts_with("OK")
}

/// Startup mutual exclusion. Explorer (MultiSelectModel=Player) launches one
/// process per selected file at the same moment; without this each could find no
/// listener and become primary. The guard is an exclusively created marker file
/// held from "forward failed" until the listener is published.
pub struct StartGuard(Option<PathBuf>);
impl Drop for StartGuard { fn drop(&mut self) { if let Some(p) = self.0.take() { let _ = std::fs::remove_file(p); } } }

pub enum Claim { Forwarded, Primary(StartGuard) }

const GUARD_STALE: std::time::Duration = std::time::Duration::from_secs(15);
const CLAIM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

fn guard_path(lock: &Path) -> PathBuf { lock.with_file_name("instance.starting") }

enum Guard { Got, Busy, Unavailable }

fn try_guard(g: &Path) -> Guard {
    if let Some(d) = g.parent() { if std::fs::create_dir_all(d).is_err() { return Guard::Unavailable; } }
    let make = || std::fs::OpenOptions::new().write(true).create_new(true).open(g);
    match make() {
        Ok(_) => return Guard::Got,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Guard::Unavailable, // unwritable dir etc.: not contention, don't wait
    }
    // a crashed starter must not block everyone forever
    let stale = std::fs::metadata(g).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).map(|e| e > GUARD_STALE).unwrap_or(false);
    if stale {
        // rename is atomic: of several launchers seeing the same stale file only one wins,
        // so nobody can delete a fresh guard another launcher just created
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let tmp = g.with_extension(format!("stale{}-{nanos}", std::process::id()));
        if std::fs::rename(g, &tmp).is_ok() { let _ = std::fs::remove_file(&tmp); }
        return match make() {
            Ok(_) => Guard::Got,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Guard::Busy,
            Err(_) => Guard::Unavailable,
        };
    }
    Guard::Busy
}

/// Either hand `paths` to the running instance or win the right to become it.
/// Concurrent callers are serialized: exactly one gets `Primary` until it
/// publishes its listener (`listen`) and drops the guard; the rest keep
/// retrying `forward` and end up forwarding to it.
pub fn claim(paths: &[PathBuf]) -> Claim { claim_at(&lock_file(), paths, CLAIM_TIMEOUT) }

fn claim_at(lock: &Path, paths: &[PathBuf], timeout: std::time::Duration) -> Claim {
    let g = guard_path(lock);
    let start = std::time::Instant::now();
    loop {
        if forward_at(lock, paths) { return Claim::Forwarded; }
        match try_guard(&g) {
            Guard::Got => {
                // the previous primary may have published between our forward and the guard
                if forward_at(lock, paths) { drop(StartGuard(Some(g))); return Claim::Forwarded; }
                return Claim::Primary(StartGuard(Some(g)));
            }
            Guard::Unavailable => return Claim::Primary(StartGuard(None)),
            Guard::Busy => {}
        }
        if start.elapsed() > timeout {
            return if forward_at(lock, paths) { Claim::Forwarded } else { Claim::Primary(StartGuard(None)) };
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Become the primary instance: listen on loopback and call `on_open` (from a
/// background thread) for every batch of forwarded paths.
pub fn listen(on_open: impl Fn(Vec<PathBuf>) + Send + 'static) -> Option<()> { listen_at(&lock_file(), on_open) }

fn listen_at(lf: &Path, on_open: impl Fn(Vec<PathBuf>) + Send + 'static) -> Option<()> {
    let l = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = l.local_addr().ok()?.port();
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) ^ ((std::process::id() as u128) << 64);
    let token = format!("{:032x}", seed.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835));
    if let Some(d) = lf.parent() { let _ = std::fs::create_dir_all(d); }
    // write-then-rename so a concurrent forward never reads a half-written file
    let tmp = lf.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, format!("{port} {token}\n")).ok()?;
    if std::fs::rename(&tmp, lf).is_err() { std::fs::write(lf, format!("{port} {token}\n")).ok()?; let _ = std::fs::remove_file(&tmp); }
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let mut msg = String::new();
            let mut r = BufReader::new(&c);
            let mut line = String::new();
            while r.read_line(&mut line).map(|n| n > 0).unwrap_or(false) { msg.push_str(&line); line.clear(); if msg.len() > 1 << 20 { break; } }
            if let Some(p) = decode(&msg, &token) { let _ = (&c).write_all(b"OK\n"); on_open(p); }
        }
    });
    Some(())
}

/// Remove the lock file if it still belongs to this process's listener.
pub fn release() { let _ = std::fs::remove_file(lock_file()); }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_keys() {
        let e = registry_entries(Path::new(r"C:\Program Files\NotEdit\notedit.exe"));
        let get = |k: &str, n: &str| e.iter().find(|v| v.key == k && v.name == n).map(|v| v.data.clone());
        assert_eq!(get(r"Software\Classes\*\shell\NotEdit", "").as_deref(), Some(MENU_TEXT));
        assert_eq!(get(r"Software\Classes\*\shell\NotEdit\command", "").unwrap(), r#""C:\Program Files\NotEdit\notedit.exe" "%1""#);
        assert_eq!(get(r"Software\Classes\Directory\Background\shell\NotEdit\command", "").unwrap(), r#""C:\Program Files\NotEdit\notedit.exe" "%V""#);
        assert_eq!(get(r"Software\Classes\Directory\shell\NotEdit", "Icon").unwrap(), r#""C:\Program Files\NotEdit\notedit.exe",0"#);
        assert!(e.iter().all(|v| v.key.starts_with(r"Software\Classes\")));
        assert_eq!(e.len(), 10);
    }
    #[test]
    fn desktop_file() {
        let d = desktop_entry(Path::new("/opt/note edit/notedit"));
        assert!(d.contains("Exec=\"/opt/note edit/notedit\" %F\n"));
        assert!(d.contains("MimeType=text/plain;"));
        assert!(d.starts_with("[Desktop Entry]\n"));
    }
    #[test]
    fn message_roundtrip() {
        let p = vec![PathBuf::from("/a b/c.txt"), PathBuf::from("/d")];
        assert_eq!(decode(&encode("t1", &p), "t1"), Some(p.clone()));
        assert_eq!(decode(&encode("t1", &p), "t2"), None);
        assert_eq!(decode("garbage", "t1"), None);
    }
    #[test]
    fn labels() {
        assert_eq!(MENU_TEXT, "使用 NotEdit 開啟");
        assert_eq!(FOLDER_TEXT, "使用 NotEdit 開啟資料夾");
    }
    #[test]
    fn concurrent_launches_elect_one_primary() {
        use std::sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex};
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("notedit-claim-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("instance.lock");
        let primaries = Arc::new(AtomicUsize::new(0));
        let got: Arc<Mutex<Vec<PathBuf>>> = Default::default();
        const N: usize = 12;
        let hs: Vec<_> = (0..N).map(|i| {
            let (lock, primaries, got) = (lock.clone(), primaries.clone(), got.clone());
            std::thread::spawn(move || {
                let p = vec![PathBuf::from(format!("/f{i}.txt"))];
                match claim_at(&lock, &p, std::time::Duration::from_secs(10)) {
                    Claim::Forwarded => {}
                    Claim::Primary(g) => {
                        primaries.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(100)); // slow startup
                        let got2 = got.clone();
                        listen_at(&lock, move |v| got2.lock().unwrap().extend(v)).unwrap();
                        got.lock().unwrap().extend(p);
                        drop(g);
                    }
                }
            })
        }).collect();
        for h in hs { h.join().unwrap(); }
        assert_eq!(primaries.load(Ordering::SeqCst), 1);
        for _ in 0..100 { if got.lock().unwrap().len() == N { break; } std::thread::sleep(std::time::Duration::from_millis(30)); }
        let mut v = got.lock().unwrap().clone(); v.sort(); v.dedup();
        assert_eq!(v.len(), N, "every launch's file must reach the single primary");
        assert!(!guard_path(&lock).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn unwritable_dir_does_not_stall() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let f = std::env::temp_dir().join(format!("notedit-notdir-{}-{nanos}", std::process::id()));
        std::fs::write(&f, "x").unwrap(); // a file where the config dir should be
        let t = std::time::Instant::now();
        let c = claim_at(&f.join("instance.lock"), &[], std::time::Duration::from_secs(10));
        assert!(matches!(c, Claim::Primary(_)));
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
        let _ = std::fs::remove_file(&f);
    }
    #[test]
    fn stale_guard_is_broken() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("notedit-stale-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let g = dir.join("instance.starting");
        std::fs::write(&g, "").unwrap();
        assert!(matches!(try_guard(&g), Guard::Busy)); // fresh guard held by someone else
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&g).unwrap().set_modified(old).unwrap();
        assert!(matches!(try_guard(&g), Guard::Got));
        assert!(matches!(try_guard(&g), Guard::Busy)); // the fresh replacement is not stolen
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "stale temp must be cleaned");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
