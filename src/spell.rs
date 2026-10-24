//! Spell checking (MadEdit "Spell Check") using the pure-Rust Hunspell-compatible
//! `spellbook` crate with a bundled en_US dictionary (SCOWL / wordlist.sourceforge.net).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

fn dict() -> Option<&'static spellbook::Dictionary> {
    static D: OnceLock<Option<spellbook::Dictionary>> = OnceLock::new();
    D.get_or_init(|| spellbook::Dictionary::new(include_str!("../assets/dict/en_US.aff"), include_str!("../assets/dict/en_US.dic")).ok()).as_ref()
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
    dict().map(|d| d.check(word)).unwrap_or(true)
}

pub fn suggest(word: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(d) = dict() { d.suggest(word, &mut out); }
    out.truncate(6);
    out
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
