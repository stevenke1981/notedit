//! Incremental syntect highlighting with per-line state cache.
use crate::{HlLine, HlToken};
use slint::{Color, ModelRc, VecModel};
use std::sync::OnceLock;
use syntect::highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

pub fn syntax_set() -> &'static SyntaxSet { static S: OnceLock<SyntaxSet> = OnceLock::new(); S.get_or_init(SyntaxSet::load_defaults_newlines) }
pub fn theme(dark: bool) -> &'static Theme {
    static T: OnceLock<(Theme, Theme)> = OnceLock::new();
    let t = T.get_or_init(|| {
        let mut ts = ThemeSet::load_defaults();
        (ts.themes.remove("InspiredGitHub").unwrap(), ts.themes.remove("base16-eighties.dark").unwrap())
    });
    if dark { &t.1 } else { &t.0 }
}


/// Export the whole document as a standalone highlighted HTML page.
pub fn export_html(text: &str, lang: &str, ext: Option<&str>, title: &str, dark: bool) -> String {
    let ss = syntax_set();
    let syn = find_syntax(lang, ext).unwrap_or_else(|| ss.find_syntax_plain_text());
    let body = syntect::html::highlighted_html_for_string(text, ss, syn, theme(dark)).unwrap_or_default();
    let esc = title.replace('&', "&amp;").replace('<', "&lt;");
    format!("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{esc}</title>\n<style>body{{margin:0}} pre{{margin:0;padding:12px;font-family:'DejaVu Sans Mono',Consolas,monospace;font-size:13px}} @media print{{pre{{white-space:pre-wrap}}}}</style></head><body>\n{body}</body></html>\n")
}

/// Find the bracket matching the one at/just before `pos` (byte offset). Returns (bracket, match).
pub fn match_bracket(text: &str, pos: usize) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    let at = |i: usize| -> Option<u8> { b.get(i).copied().filter(|c| b"()[]{}".contains(c)) };
    let i = at(pos).map(|_| pos).or_else(|| pos.checked_sub(1).and_then(|p| at(p).map(|_| p)))?;
    let c = b[i];
    let (open, close, fwd) = match c { b'(' => (b'(', b')', true), b'[' => (b'[', b']', true), b'{' => (b'{', b'}', true),
        b')' => (b'(', b')', false), b']' => (b'[', b']', false), _ => (b'{', b'}', false) };
    let mut depth = 0i32;
    if fwd {
        for (j, &x) in b.iter().enumerate().skip(i) {
            if x == open { depth += 1 } else if x == close { depth -= 1; if depth == 0 { return Some((i, j)); } }
        }
    } else {
        for j in (0..=i).rev() {
            let x = b[j];
            if x == close { depth += 1 } else if x == open { depth -= 1; if depth == 0 { return Some((i, j)); } }
        }
    }
    None
}

#[derive(Clone)]
pub struct Tok { pub text: String, pub color: Color, pub bold: bool, pub italic: bool }

fn split_marks(toks: Vec<Tok>, marks: &[usize]) -> Vec<(Tok, bool)> {
    let mut out = Vec::new();
    let mut off = 0;
    for t in toks {
        let len = t.text.len();
        let mut cuts: Vec<usize> = marks.iter().filter(|&&m| m >= off && m < off + len).map(|m| m - off).collect();
        cuts.sort();
        let mut start = 0;
        for c in cuts {
            let end = t.text[c..].chars().next().map(|ch| c + ch.len_utf8()).unwrap_or(c);
            if c > start { out.push((Tok { text: t.text[start..c].to_string(), ..t.clone() }, false)); }
            out.push((Tok { text: t.text[c..end].to_string(), ..t.clone() }, true));
            start = end;
        }
        if start < len { out.push((Tok { text: t.text[start..].to_string(), ..t.clone() }, false)); }
        off += len;
    }
    out
}

pub struct Hl {
    syntax: Option<String>, // syntax name
    dark: bool,
    src: String,            // text the cache corresponds to
    lines: Vec<Vec<Tok>>,   // highlighted lines
    states: Vec<(ParseState, HighlightState)>, // state at start of line i (len == lines.len()+1)
}

impl Default for Hl { fn default() -> Self { Hl { syntax: None, dark: false, src: String::new(), lines: vec![], states: vec![] } } }

fn find_syntax(lang: &str, ext: Option<&str>) -> Option<&'static SyntaxReference> {
    let ss = syntax_set();
    if lang == "text" { return None; }
    let by = |e: &str| ss.find_syntax_by_extension(e);
    let mapped = match lang {
        "rust" => "rs", "c" => "c", "cpp" => "cpp", "cs" => "cs", "java" => "java", "python" => "py", "javascript" => "js",
        "html" => "html", "css" => "css", "xml" => "xml", "json" => "json", "markdown" => "md", "shell" => "sh",
        "batch" => "bat", "ini" => "ini", "sql" => "sql", "yaml" => "yaml", "slint" => "rs", _ => "",
    };
    by(mapped).or_else(|| ext.and_then(by)).filter(|s| s.name != "Plain Text")
}

impl Hl {
    /// Bring cache in sync with `text` / language. Invalidates from first changed line.
    pub fn sync(&mut self, text: &str, lang: &str, ext: Option<&str>, dark: bool) {
        let syn = find_syntax(lang, ext);
        let name = syn.map(|s| s.name.clone());
        if name != self.syntax || dark != self.dark {
            self.syntax = name; self.dark = dark; self.lines.clear(); self.states.clear();
            if let Some(s) = syn { self.states.push((ParseState::new(s), HighlightState::new(&Highlighter::new(theme(dark)), ScopeStack::new()))); }
        } else if text != self.src {
            let common = self.src.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
            let line = self.src[..common.min(self.src.len())].matches('\n').count();
            if self.lines.len() > line { self.lines.truncate(line); self.states.truncate(line + 1); }
        }
        if text != self.src { self.src = text.to_string(); }
    }

    pub fn active(&self) -> bool { self.syntax.is_some() }

    /// Highlight lines until `upto` (exclusive) is cached, doing at most `budget` lines. Returns true if done.
    pub fn work(&mut self, upto: usize, budget: usize) -> bool {
        if self.syntax.is_none() { return true; }
        let ss = syntax_set();
        let hl = Highlighter::new(theme(self.dark));
        let start = self.lines.len();
        if start >= upto { return true; }
        let mut done = 0;
        for line in self.src.split_inclusive('\n').skip(start) {
            if self.lines.len() >= upto || done >= budget { break; }
            let (mut ps, mut hs) = self.states.last().cloned().unwrap();
            let owned;
            let l = if line.ends_with('\n') { line } else { owned = format!("{}\n", line); &owned };
            let ops = ps.parse_line(l, ss).unwrap_or_default();
            let toks = HighlightIterator::new(&mut hs, &ops, l, &hl)
                .filter_map(|(st, t)| {
                    let t = t.trim_end_matches(['\n', '\r']);
                    if t.is_empty() { return None; }
                    let c = st.foreground;
                    Some(Tok { text: t.to_string(), color: Color::from_argb_u8(255, c.r, c.g, c.b),
                        bold: st.font_style.contains(FontStyle::BOLD), italic: st.font_style.contains(FontStyle::ITALIC) })
                }).collect();
            self.lines.push(toks);
            self.states.push((ps, hs));
            done += 1;
        }
        // trailing empty line after final '\n' has no tokens
        let total = self.src.split_inclusive('\n').count();
        self.lines.len() >= upto.min(total)
    }

    /// Build the model for visible lines [first, first+count).
    pub fn visible(&self, first: usize, count: usize, fg: Color, marks: &[usize]) -> Vec<HlLine> {
        let mut out = Vec::new();
        let mut line_start = 0usize;
        for (i, raw) in self.src.split('\n').enumerate() {
            let ls = line_start;
            line_start += raw.len() + 1;
            if i < first { continue; }
            if i >= first + count { break; }
            let base: Vec<Tok> = match self.lines.get(i) {
                Some(t) => t.clone(),
                None if raw.is_empty() => vec![],
                None => vec![Tok { text: raw.to_string(), color: fg, bold: false, italic: false }],
            };
            let lm: Vec<usize> = marks.iter().filter(|&&m| m >= ls && m < ls + raw.len()).map(|m| m - ls).collect();
            let toks: Vec<HlToken> = split_marks(base, &lm).into_iter()
                .map(|(t, bg)| HlToken { text: t.text.as_str().into(), color: t.color, bold: t.bold || bg, italic: t.italic, bg }).collect();
            out.push(HlLine { line: i as i32, tokens: ModelRc::new(VecModel::from(toks)) });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn brackets() {
        let t = "fn a() { [1, (2)] }";
        assert_eq!(match_bracket(t, 7), Some((7, 18)));
        assert_eq!(match_bracket(t, 19), Some((18, 7)));
        assert_eq!(match_bracket(t, 5), Some((5, 4)));
    }
    #[test]
    fn marks_split() {
        let toks = vec![Tok { text: "ab(cd)".into(), color: Color::from_rgb_u8(0, 0, 0), bold: false, italic: false }];
        let r = split_marks(toks, &[2, 5]);
        let s: Vec<_> = r.iter().map(|(t, b)| (t.text.as_str(), *b)).collect();
        assert_eq!(s, vec![("ab", false), ("(", true), ("cd", false), (")", true)]);
    }
}
