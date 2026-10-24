//! Find in Files: recursive folder search with wildcard filters and regex, run on a
//! worker thread; results are delivered in batches.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
/// `start`/`end` are char columns within the (1-based) `line`.
pub struct Hit { pub path: PathBuf, pub line: usize, pub start: usize, pub end: usize, pub snippet: String }

/// `*.rs;*.txt` style wildcard list. Empty / `*` / `*.*` matches everything.
pub fn filter_matches(filters: &str, name: &str) -> bool {
    let pats: Vec<&str> = filters.split([';', ',', ' ']).map(str::trim).filter(|p| !p.is_empty()).collect();
    if pats.is_empty() { return true; }
    let name = name.to_lowercase();
    pats.iter().any(|p| wildcard(&p.to_lowercase(), &name))
}

fn wildcard(p: &str, s: &str) -> bool {
    if p == "*" || p == "*.*" { return true; }
    let (p, s): (Vec<char>, Vec<char>) = (p.chars().collect(), s.chars().collect());
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) { pi += 1; si += 1; }
        else if pi < p.len() && p[pi] == '*' { star = Some(pi); mark = si; pi += 1; }
        else if let Some(st) = star { pi = st + 1; mark += 1; si = mark; }
        else { return false; }
    }
    while pi < p.len() && p[pi] == '*' { pi += 1; }
    pi == p.len()
}

const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".svn", ".hg"];
const MAX_FILE: u64 = 64 << 20;

/// Search `root`. `emit` receives batches of hits; returns (files searched, hits).
pub fn search(root: &Path, filters: &str, recursive: bool, re: &regex::Regex, cancel: &Arc<AtomicBool>, limit: usize, mut emit: impl FnMut(Vec<Hit>)) -> (usize, usize) {
    let mut stack = vec![root.to_path_buf()];
    let (mut files, mut total) = (0, 0);
    let mut batch = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if cancel.load(Ordering::Relaxed) || total >= limit { emit(batch); return (files, total); }
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            if ft.is_dir() {
                if recursive && !SKIP_DIRS.contains(&name.as_str()) { stack.push(e.path()); }
                continue;
            }
            if !ft.is_file() || !filter_matches(filters, &name) { continue; }
            if e.metadata().map(|m| m.len() > MAX_FILE).unwrap_or(true) { continue; }
            let Ok(bytes) = std::fs::read(e.path()) else { continue };
            if crate::hex::looks_binary(&bytes[..bytes.len().min(8192)]) { continue; }
            files += 1;
            let (text, _, _) = crate::decode(&bytes);
            let mut line = 0;
            let mut last = 0;
            for m in re.find_iter(&text) {
                line += text[last..m.start()].matches('\n').count();
                last = m.start();
                let line_start = text[..m.start()].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let le = text[m.start()..].find('\n').map(|i| i + m.start()).unwrap_or(text.len());
                let snippet: String = text[line_start..le].trim().chars().take(200).collect();
                let nocr = |t: &str| t.chars().filter(|&c| c != '\r').count();
                let col = nocr(&text[line_start..m.start()]);
                batch.push(Hit { path: e.path(), line: line + 1, start: col, end: col + nocr(m.as_str()), snippet });
                total += 1;
                if total >= limit { break; }
            }
            if batch.len() >= 200 { emit(std::mem::take(&mut batch)); }
        }
    }
    emit(batch);
    (files, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcards() {
        assert!(filter_matches("*.rs;*.TXT", "main.rs"));
        assert!(filter_matches("*.rs;*.txt", "A.txt"));
        assert!(!filter_matches("*.rs", "main.rso"));
        assert!(filter_matches("", "x"));
        assert!(filter_matches("ma?n.*", "main.c"));
    }
    #[test]
    fn folder_search() {
        let d = std::env::temp_dir().join(format!("notedit-fif-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("a.txt"), "hello\nfoo bar foo\n").unwrap();
        std::fs::write(d.join("sub/b.txt"), "中文 foo").unwrap();
        std::fs::write(d.join(".git/c.txt"), "foo").unwrap();
        std::fs::write(d.join("bin.txt"), b"foo\0\0").unwrap();
        std::fs::write(d.join("x.md"), "foo").unwrap();
        let re = regex::Regex::new("fo+").unwrap();
        let mut hits = vec![];
        let (files, n) = search(&d, "*.txt", true, &re, &Arc::new(AtomicBool::new(false)), 100, |b| hits.extend(b));
        assert_eq!((files, n), (2, 3));
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].snippet, "foo bar foo");
        assert!(hits.iter().any(|h| h.path.ends_with("sub/b.txt") && h.start == 3 && h.end == 6));
        let (_, n) = search(&d, "*.txt", false, &re, &Arc::new(AtomicBool::new(false)), 100, |_| {});
        assert_eq!(n, 2);
        std::fs::remove_dir_all(&d).unwrap();
    }
}

/// Replace every match in a file's raw bytes, keeping its encoding, BOM and line endings.
/// Returns `None` for binary files or when nothing matched; `Err` when the result
/// cannot be represented in the file's encoding.
pub fn replace_bytes(bytes: &[u8], re: &regex::Regex, rep: &str, expand: bool) -> Result<Option<(Vec<u8>, usize)>, String> {
    if crate::hex::looks_binary(&bytes[..bytes.len().min(8192)]) { return Ok(None); }
    let (text, enc, bom) = crate::decode(bytes);
    let n = re.find_iter(&text).count();
    if n == 0 { return Ok(None); }
    let out = if expand { re.replace_all(&text, rep) } else { re.replace_all(&text, regex::NoExpand(rep)) };
    Ok(Some((encode_raw(&out, enc, bom)?, n)))
}

pub fn encode_raw(text: &str, enc: &'static encoding_rs::Encoding, bom: bool) -> Result<Vec<u8>, String> {
    let name = enc.name();
    if name == "UTF-16LE" || name == "UTF-16BE" {
        let le = name == "UTF-16LE";
        let mut v = if bom { if le { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] } } else { vec![] };
        for u in text.encode_utf16() { v.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() }); }
        return Ok(v);
    }
    let mut v = if bom && name == "UTF-8" { vec![0xEF, 0xBB, 0xBF] } else { vec![] };
    let (b, _, err) = enc.encode(text);
    if err { return Err(format!("{} 編碼無法表示取代後的字元", name)); }
    v.extend_from_slice(&b);
    Ok(v)
}

/// Files under `root` that contain at least one match, with their match count.
pub fn matching_files(root: &Path, filters: &str, recursive: bool, re: &regex::Regex) -> Vec<(PathBuf, usize)> {
    let mut out: Vec<(PathBuf, usize)> = vec![];
    let cancel = Arc::new(AtomicBool::new(false));
    search(root, filters, recursive, re, &cancel, usize::MAX, |b| for h in b {
        match out.last_mut() { Some((p, n)) if *p == h.path => *n += 1, _ => out.push((h.path, 1)) }
    });
    out
}

#[cfg(test)]
mod rtests {
    use super::*;
    #[test]
    fn replace_keeps_crlf_and_bom() {
        let re = regex::Regex::new("a(b)").unwrap();
        let src = b"\xEF\xBB\xBFab x\r\nab\r\n";
        let (out, n) = replace_bytes(src, &re, "[$1]", true).unwrap().unwrap();
        assert_eq!(n, 2);
        assert_eq!(out, b"\xEF\xBB\xBF[b] x\r\n[b]\r\n");
        let (out, _) = replace_bytes(b"ab", &re, "$1", false).unwrap().unwrap();
        assert_eq!(out, b"$1");
        assert!(replace_bytes(b"ab\0\0\0", &re, "x", false).unwrap().is_none());
        assert!(replace_bytes(b"zz", &re, "x", false).unwrap().is_none());
    }
}
