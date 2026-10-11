//! Search helpers (plain / whole word / regex, case options).
use regex::{Regex, RegexBuilder};

pub fn build(needle: &str, match_case: bool, whole_word: bool, use_regex: bool) -> Result<Regex, String> {
    if needle.is_empty() { return Err("請輸入搜尋字串".into()); }
    let mut pat = if use_regex { needle.to_string() } else { regex::escape(needle) };
    if whole_word { pat = format!(r"\b(?:{})\b", pat); }
    RegexBuilder::new(&pat).case_insensitive(!match_case).multi_line(true).build().map_err(|e| format!("規則運算式錯誤：{}", e))
}

/// Find next (forward) after `from`, or previous before `from`, with wrap-around. Skips empty matches at `from`.
pub fn find(re: &Regex, text: &str, from_start: usize, from_end: usize, forward: bool) -> Option<(usize, usize)> {
    if forward {
        let ok = |m: &regex::Match| !(m.start() == m.end() && m.start() == from_end);
        re.find_iter(text).find(|m| m.start() >= from_end && ok(m)).or_else(|| re.find_iter(text).next()).map(|m| (m.start(), m.end()))
    } else {
        let all: Vec<_> = re.find_iter(text).map(|m| (m.start(), m.end())).collect();
        all.iter().rev().find(|m| m.0 < from_start).or(all.last()).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes() {
        let t = "cat concat Cat cat.";
        let re = build("cat", false, true, false).unwrap();
        assert_eq!(re.find_iter(t).count(), 3);
        let re = build("cat", true, false, false).unwrap();
        assert_eq!(re.find_iter(t).count(), 3);
        let re = build(r"c(a)t", false, false, true).unwrap();
        assert_eq!(re.replace_all(t, "[$1]").matches('[').count(), 4);
        assert!(build("(", false, false, true).is_err());
        let re = build("cat", true, true, false).unwrap();
        assert_eq!(find(&re, t, 0, 0, true), Some((0, 3)));
        assert_eq!(find(&re, t, 0, 3, true), Some((15, 18)));
        assert_eq!(find(&re, t, 0, 0, false), Some((15, 18)));
    }
}
