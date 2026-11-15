//! Spell checking (MadEdit "Spell Check") using the pure-Rust Hunspell-compatible
//! `spellbook` crate with a bundled en_US dictionary (SCOWL / wordlist.sourceforge.net).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

#[cfg(any(not(windows), test))]
fn dict() -> Option<&'static spellbook::Dictionary> {
    static D: OnceLock<Option<spellbook::Dictionary>> = OnceLock::new();
    D.get_or_init(|| spellbook::Dictionary::new(include_str!("../assets/dict/en_US.aff"), include_str!("../assets/dict/en_US.dic")).ok()).as_ref()
}

#[cfg(all(windows, not(test)))]
struct Plugin {
    _library: libloading::Library,
    check: unsafe extern "C" fn(*const u8, usize) -> bool,
    suggest: unsafe extern "C" fn(*const u8, usize, *mut u8, usize) -> usize,
}

#[cfg(all(windows, not(test)))]
fn plugin() -> Result<&'static Plugin, String> {
    static PLUGIN: OnceLock<Plugin> = OnceLock::new();
    if let Some(plugin) = PLUGIN.get() { return Ok(plugin); }
    let loaded = (|| -> Result<Plugin, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let path = exe.with_file_name("notedit_spell.dll");
        // The module is loaded only from the directory containing this executable.
        let library = unsafe { libloading::Library::new(&path) }.map_err(|e| format!("{}：{e}", path.display()))?;
        unsafe {
            let version: unsafe extern "C" fn() -> u32 = *library.get(b"notedit_spell_version\0").map_err(|e| e.to_string())?;
            let ready: unsafe extern "C" fn() -> bool = *library.get(b"notedit_spell_ready\0").map_err(|e| e.to_string())?;
            if version() != 1 || !ready() { return Err("拼字模組版本不符或字典載入失敗".into()); }
            let check = *library.get(b"notedit_spell_check\0").map_err(|e| e.to_string())?;
            let suggest = *library.get(b"notedit_spell_suggest\0").map_err(|e| e.to_string())?;
            Ok(Plugin { _library: library, check, suggest })
        }
    })()?;
    let _ = PLUGIN.set(loaded);
    Ok(PLUGIN.get().expect("plugin was initialized"))
}

pub fn available() -> Result<(), String> {
    #[cfg(all(windows, not(test)))]
    { plugin().map(|_| ()) }
    #[cfg(any(not(windows), test))]
    { dict().map(|_| ()).ok_or_else(|| "拼字字典載入失敗".to_string()) }
}

fn ignored() -> &'static Mutex<HashSet<String>> {
    static I: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    I.get_or_init(Default::default)
}

pub fn ignore(word: &str) { if let Ok(mut s) = ignored().lock() { s.insert(word.to_string()); } }
pub fn set_ignored(words: &[String]) { if let Ok(mut s) = ignored().lock() { s.extend(words.iter().cloned()); } }

pub fn check(word: &str) -> bool {
    if word.len() < 2 || word.chars().any(|c| c.is_ascii_digit() || !c.is_ascii()) { return true; }
    // camelCase / ALLCAPS identifiers are not prose
    if word.chars().skip(1).any(|c| c.is_ascii_uppercase()) { return true; }
    if ignored().lock().map(|s| s.contains(word)).unwrap_or(false) { return true; }
    #[cfg(all(windows, not(test)))]
    { plugin().map(|p| unsafe { (p.check)(word.as_ptr(), word.len()) }).unwrap_or(true) }
    #[cfg(any(not(windows), test))]
    { dict().map(|d| d.check(word)).unwrap_or(true) }
}

pub fn suggest(word: &str) -> Vec<String> {
    #[cfg(all(windows, not(test)))]
    {
        let Ok(p) = plugin() else { return vec![] };
        let mut bytes = vec![0; 8192];
        let len = unsafe { (p.suggest)(word.as_ptr(), word.len(), bytes.as_mut_ptr(), bytes.len()) };
        if len > bytes.len() { return vec![]; }
        return std::str::from_utf8(&bytes[..len]).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default();
    }
    #[cfg(any(not(windows), test))]
    {
    let mut out = Vec::new();
    if let Some(d) = dict() { d.suggest(word, &mut out); }
    out.truncate(6);
    out
    }
}

/// Misspelled words in `chars` as char ranges.
pub fn misspelled(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let s = i;
            while i < chars.len() && (chars[i].is_ascii_alphabetic() || (chars[i] == '\'' && i + 1 < chars.len() && chars[i + 1].is_ascii_alphabetic())) { i += 1; }
            // skip words glued to digits/underscores (identifiers)
            let glued = (s > 0 && (chars[s - 1].is_alphanumeric() || chars[s - 1] == '_')) || (i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_'));
            let w: String = chars[s..i].iter().collect();
            if !glued && !check(&w) { out.push((s, i)); }
        } else { i += 1; }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spelling() {
        assert!(check("hello"));
        assert!(check("Hello"));
        assert!(!check("helo"));
        assert!(suggest("helo").iter().any(|s| s == "hello"));
        let c: Vec<char> = "this is a tset of speling, fooBar x_y".chars().collect();
        let m = misspelled(&c);
        let words: Vec<String> = m.iter().map(|&(s, e)| c[s..e].iter().collect()).collect();
        assert_eq!(words, vec!["tset", "speling"]);
        ignore("tset");
        assert!(check("tset"));
    }
}
