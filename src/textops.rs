//! Text transformations inspired by MadEdit's Edit / Tools menus:
//! Chinese conversion, full/half width, tab/space, line utilities, hex strings
//! and word count. All functions are pure and unit-tested.

use std::sync::OnceLock;

use ferrous_opencc::{config::BuiltinConfig, OpenCC};

fn opencc(cfg: BuiltinConfig, cell: &'static OnceLock<Option<OpenCC>>) -> Option<&'static OpenCC> {
    cell.get_or_init(|| OpenCC::from_config(cfg).ok()).as_ref()
}

/// Simplified → Traditional (Taiwan standard, with phrase conversion).
pub fn s2t(s: &str) -> String {
    static C: OnceLock<Option<OpenCC>> = OnceLock::new();
    opencc(BuiltinConfig::S2twp, &C).map(|c| c.convert(s)).unwrap_or_else(|| s.to_string())
}

/// Traditional → Simplified.
pub fn t2s(s: &str) -> String {
    static C: OnceLock<Option<OpenCC>> = OnceLock::new();
    opencc(BuiltinConfig::Tw2sp, &C).map(|c| c.convert(s)).unwrap_or_else(|| s.to_string())
}

/// ASCII (and space) → full-width forms.
pub fn to_fullwidth(s: &str) -> String {
    s.chars().map(|c| match c {
        ' ' => '\u{3000}',
        '!'..='~' => char::from_u32(c as u32 - 0x21 + 0xFF01).unwrap_or(c),
        _ => c,
    }).collect()
}

/// Full-width forms → ASCII.
pub fn to_halfwidth(s: &str) -> String {
    s.chars().map(|c| match c {
        '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFF01 + 0x21).unwrap_or(c),
        _ => c,
    }).collect()
}

/// Expand tabs to spaces using `tab`-wide tab stops (per line).
pub fn tabs_to_spaces(s: &str, tab: usize) -> String {
    let tab = tab.max(1);
    let mut out = String::with_capacity(s.len());
    let mut col = 0;
    for c in s.chars() {
        match c {
            '\t' => { let n = tab - col % tab; out.extend(std::iter::repeat_n(' ', n)); col += n; }
            '\n' => { out.push(c); col = 0; }
            _ => { out.push(c); col += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0); }
        }
    }
    out
}

/// Replace runs of spaces that end on a tab stop with tabs (leading and inner whitespace).
pub fn spaces_to_tabs(s: &str, tab: usize) -> String {
    let tab = tab.max(1);
    s.split('\n').map(|line| {
        let mut out = String::new();
        let mut col = 0;
        let mut pending = 0; // spaces not yet written
        for c in line.chars() {
            if c == ' ' {
                pending += 1; col += 1;
                if col % tab == 0 { if pending > 1 { out.push('\t'); } else { out.push(' '); } pending = 0; }
                continue;
            }
            out.extend(std::iter::repeat_n(' ', pending)); pending = 0;
            if c == '\t' { out.push('\t'); col += tab - col % tab; }
            else { out.push(c); col += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0); }
        }
        out.extend(std::iter::repeat_n(' ', pending));
        out
    }).collect::<Vec<_>>().join("\n")
}

pub fn trim_leading(s: &str) -> String { s.split('\n').map(|l| l.trim_start()).collect::<Vec<_>>().join("\n") }
pub fn join_lines(s: &str) -> String {
    let mut out = String::new();
    for (i, l) in s.split('\n').enumerate() {
        let l = if i == 0 { l.trim_end() } else { l.trim() };
        if i > 0 && !out.is_empty() && !l.is_empty() { out.push(' '); }
        out.push_str(l);
    }
    out
}
pub fn delete_empty_lines(s: &str, with_spaces: bool) -> String {
    s.split('\n').filter(|l| if with_spaces { !l.trim().is_empty() } else { !l.is_empty() }).collect::<Vec<_>>().join("\n")
}
pub fn capitalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() { if start { out.extend(c.to_uppercase()); } else { out.extend(c.to_lowercase()); } start = false; }
        else { out.push(c); start = true; }
    }
    out
}
pub fn invert_case(s: &str) -> String {
    s.chars().flat_map(|c| -> Vec<char> { if c.is_uppercase() { c.to_lowercase().collect() } else if c.is_lowercase() { c.to_uppercase().collect() } else { vec![c] } }).collect()
}

#[derive(Clone, Copy)]
pub enum LineOrder { Ascending, Descending, Natural, Reverse }

/// Operate on LF-normalized editor text without sorting the final terminator
/// as an extra empty line. Interior blank lines remain real data.
fn map_lines(s: &str, f: impl FnOnce(&mut Vec<&str>)) -> String {
    if s.is_empty() { return String::new(); }
    let terminated = s.ends_with('\n');
    let body = s.strip_suffix('\n').unwrap_or(s);
    let mut lines: Vec<&str> = body.split('\n').collect();
    f(&mut lines);
    let mut result = lines.join("\n");
    if terminated { result.push('\n'); }
    result
}

pub fn reorder_lines(s: &str, order: LineOrder) -> String {
    map_lines(s, |lines| match order {
        LineOrder::Ascending => lines.sort(),
        LineOrder::Descending => lines.sort_by(|a, b| b.cmp(a)),
        LineOrder::Natural => lines.sort_by(|a, b| natural_cmp(a, b)),
        LineOrder::Reverse => lines.reverse(),
    })
}

pub fn unique_lines(s: &str) -> String {
    map_lines(s, |lines| {
        let mut seen = std::collections::HashSet::new();
        lines.retain(|line| seen.insert(*line));
    })
}

/// Compare ASCII digit runs by magnitude without parsing (no integer overflow).
/// Equal numbers retain their input order, including differently padded forms.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    while !a.is_empty() && !b.is_empty() {
        let ordering = if a[0].is_ascii_digit() && b[0].is_ascii_digit() {
            let an = a.iter().take_while(|c| c.is_ascii_digit()).count();
            let bn = b.iter().take_while(|c| c.is_ascii_digit()).count();
            let ax = &a[..an];
            let bx = &b[..bn];
            let ax = &ax[ax.iter().take_while(|&&c| c == b'0').count()..];
            let bx = &bx[bx.iter().take_while(|&&c| c == b'0').count()..];
            let ordering = ax.len().cmp(&bx.len()).then_with(|| ax.cmp(bx));
            a = &a[an..]; b = &b[bn..];
            ordering
        } else {
            let ordering = a[0].cmp(&b[0]);
            a = &a[1..]; b = &b[1..];
            ordering
        };
        if ordering != std::cmp::Ordering::Equal { return ordering; }
    }
    a.len().cmp(&b.len())
}

/// `Mod` → `4D6F64` (or `4D 6F 64` with spaces).
pub fn to_hex_string(bytes: &[u8], spaced: bool) -> String {
    let v: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    v.join(if spaced { " " } else { "" })
}

/// `4D6F64`, `4D 6F 64`, `%4D%6F%64`, `4D,6F,64`, `0x4D 0x6F` → bytes.
pub fn from_hex_string(s: &str) -> Option<Vec<u8>> {
    let s = s.replace("0x", "").replace("0X", "");
    let digits: Vec<u8> = s.chars().filter(|c| !matches!(c, ' ' | ',' | '%' | '\n' | '\r' | '\t' | '-' | ':')).map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
    if digits.is_empty() || digits.len() % 2 != 0 { return None; }
    Some(digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect())
}

/// Insert incremental numbers at the start of every line: `prefix{n}suffix`.
pub fn number_lines(s: &str, start: i64, step: i64, width: usize) -> String {
    s.split('\n').enumerate().map(|(i, l)| format!("{:0w$}{}", start + step * i as i64, l, w = width)).collect::<Vec<_>>().join("\n")
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F | 0x3040..=0x30FF | 0xAC00..=0xD7AF)
}

#[derive(Debug, Default, PartialEq)]
pub struct Counts { pub words: usize, pub chars: usize, pub spaces: usize, pub cjk: usize, pub halfwidth: usize, pub fullwidth: usize, pub lines: usize, pub bytes_utf8: usize }

/// MadEdit-style word count: every CJK character counts as one word, other words
/// are runs of alphanumerics.
pub fn count(s: &str) -> Counts {
    let mut c = Counts { lines: s.matches('\n').count() + 1, bytes_utf8: s.len(), ..Default::default() };
    let mut in_word = false;
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' { in_word = false; continue; }
        c.chars += 1;
        if ch.is_whitespace() { c.spaces += 1; in_word = false; continue; }
        if unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) >= 2 { c.fullwidth += 1 } else { c.halfwidth += 1 }
        if is_cjk(ch) { c.cjk += 1; c.words += 1; in_word = false; }
        else if ch.is_alphanumeric() || ch == '_' || ch == '\'' { if !in_word { c.words += 1; } in_word = true; }
        else { in_word = false; }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reorder_preserves_final_newline_and_blank_lines() {
        assert_eq!(reorder_lines("b\na\n", LineOrder::Ascending), "a\nb\n");
        assert_eq!(reorder_lines("b\n\na\n", LineOrder::Descending), "b\na\n\n");
        assert_eq!(reorder_lines("b\na", LineOrder::Reverse), "a\nb");
        assert_eq!(reorder_lines("中\n文\n", LineOrder::Reverse), "文\n中\n");
        assert_eq!(reorder_lines("", LineOrder::Natural), "");
        assert_eq!(reorder_lines("\n", LineOrder::Reverse), "\n");
    }

    #[test]
    fn natural_order_handles_large_numbers_and_leading_zeroes() {
        assert_eq!(reorder_lines("file10\nfile02\nfile2\nfile1\n", LineOrder::Natural),
            "file1\nfile02\nfile2\nfile10\n");
        assert_eq!(reorder_lines("x100000000000000000000\nx9\nx20", LineOrder::Natural),
            "x9\nx20\nx100000000000000000000");
        assert_eq!(reorder_lines("中10節2\n中2節10\n中2節3", LineOrder::Natural),
            "中2節3\n中2節10\n中10節2");
    }

    #[test]
    fn dedup_keeps_order_and_final_newline() {
        assert_eq!(unique_lines("中\na\n中\na\n"), "中\na\n");
        assert_eq!(unique_lines("a\n\n\na"), "a\n");
        assert_eq!(unique_lines("\n\n"), "\n");
    }
    #[test]
    fn chinese() {
        assert_eq!(s2t("简体中文软件"), "簡體中文軟體");
        assert_eq!(t2s("繁體中文"), "繁体中文");
    }
    #[test]
    fn widths() {
        assert_eq!(to_fullwidth("Ab 1!"), "Ａｂ　１！");
        assert_eq!(to_halfwidth("Ａｂ　１！中"), "Ab 1!中");
    }
    #[test]
    fn tabs() {
        assert_eq!(tabs_to_spaces("\ta\tb\nab\tc", 4), "    a   b\nab  c");
        assert_eq!(spaces_to_tabs("        x  y", 4), "\t\tx  y");
        assert_eq!(spaces_to_tabs(tabs_to_spaces("\tif\n\t\tx", 4).as_str(), 4), "\tif\n\t\tx");
    }
    #[test]
    fn lines() {
        assert_eq!(join_lines("a  \n  b\n\nc"), "a b c");
        assert_eq!(delete_empty_lines("a\n\n  \nb", false), "a\n  \nb");
        assert_eq!(delete_empty_lines("a\n\n  \nb", true), "a\nb");
        assert_eq!(trim_leading("  a\n\tb"), "a\nb");
        assert_eq!(capitalize("hello wORLD"), "Hello World");
        assert_eq!(invert_case("aBc"), "AbC");
        assert_eq!(number_lines("a\nb", 1, 1, 2), "01a\n02b");
    }
    #[test]
    fn hex_strings() {
        assert_eq!(to_hex_string(b"Mod", false), "4D6F64");
        assert_eq!(to_hex_string(b"Mod", true), "4D 6F 64");
        for s in ["4D6F64", "4D 6F 64", "%4D%6F%64", "4D,6F,64", "0x4d 0x6f 0x64"] { assert_eq!(from_hex_string(s).unwrap(), b"Mod"); }
        assert!(from_hex_string("4G").is_none());
    }
    #[test]
    fn counting() {
        let c = count("Hello, world! 你好\nit's");
        assert_eq!((c.words, c.cjk, c.lines, c.fullwidth), (5, 2, 2, 2));
    }
}
