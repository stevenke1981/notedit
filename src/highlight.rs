//! Incremental syntect highlighting with per-line state cache.
use ropey::Rope;
use slint::Color;
use std::sync::OnceLock;
use syntect::highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxDefinition, SyntaxReference, SyntaxSet};

const TOML_SYNTAX: &str = include_str!("../ui/syntaxes/TOML.sublime-syntax");

/// Default syntect syntaxes plus the bundled extras (TOML).
pub fn syntax_set() -> &'static SyntaxSet {
    static S: OnceLock<SyntaxSet> = OnceLock::new();
    S.get_or_init(|| {
        let mut b = SyntaxSet::load_defaults_newlines().into_builder();
        if let Ok(def) = SyntaxDefinition::load_from_str(TOML_SYNTAX, true, None) { b.add(def); }
        b.build()
    })
}
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

/// Find the bracket matching the one at/just before char offset `pos`. Returns (bracket, match).
pub fn match_bracket(rope: &Rope, pos: usize) -> Option<(usize, usize)> {
    const LIMIT: usize = 1_000_000;
    let len = rope.len_chars();
    let is_br = |c: char| "()[]{}".contains(c);
    let i = if pos < len && is_br(rope.char(pos)) { pos } else if pos > 0 && pos <= len && is_br(rope.char(pos - 1)) { pos - 1 } else { return None };
    let c = rope.char(i);
    let (open, close, fwd) = match c { '(' => ('(', ')', true), '[' => ('[', ']', true), '{' => ('{', '}', true),
        ')' => ('(', ')', false), ']' => ('[', ']', false), _ => ('{', '}', false) };
    let mut depth = 0i32;
    if fwd {
        for (k, x) in rope.chars_at(i).enumerate().take(LIMIT) {
            if x == open { depth += 1 } else if x == close { depth -= 1; if depth == 0 { return Some((i, i + k)); } }
        }
    } else {
        let mut it = rope.chars_at(i + 1);
        let mut j = i + 1;
        for _ in 0..LIMIT {
            let Some(x) = it.prev() else { break };
            j -= 1;
            if x == close { depth += 1 } else if x == open { depth -= 1; if depth == 0 { return Some((i, j)); } }
        }
    }
    None
}

#[derive(Clone)]
pub struct Tok { pub text: String, pub color: Color, pub bold: bool, pub italic: bool }

pub struct Hl {
    syntax: Option<String>, // syntax name
    dark: bool,
    lines: Vec<Vec<Tok>>,   // highlighted lines
    states: Vec<(ParseState, HighlightState)>, // state at start of line i (len == lines.len()+1)
}

impl Default for Hl { fn default() -> Self { Hl { syntax: None, dark: false, lines: vec![], states: vec![] } } }

fn find_syntax(lang: &str, ext: Option<&str>) -> Option<&'static SyntaxReference> {
    let ss = syntax_set();
    if lang == "text" { return None; }
    let by = |e: &str| ss.find_syntax_by_extension(e);
    let mapped = match lang {
        "rust" => "rs", "c" => "c", "cpp" => "cpp", "cs" => "cs", "java" => "java", "python" => "py", "javascript" => "js",
        "html" => "html", "css" => "css", "xml" => "xml", "json" => "json", "markdown" => "md", "shell" => "sh",
        "batch" => "bat", "ini" => "ini", "sql" => "sql", "yaml" => "yaml", "toml" => "toml", "slint" => "rs", _ => "",
    };
    by(mapped).or_else(|| ext.and_then(by)).filter(|s| s.name != "Plain Text")
}

impl Hl {
    /// Select syntax/theme; resets the cache when either changes.
    pub fn configure(&mut self, lang: &str, ext: Option<&str>, dark: bool) {
        let syn = find_syntax(lang, ext);
        let name = syn.map(|s| s.name.clone());
        if name != self.syntax || dark != self.dark {
            self.syntax = name; self.dark = dark; self.lines.clear(); self.states.clear();
            if let Some(s) = syn { self.states.push((ParseState::new(s), HighlightState::new(&Highlighter::new(theme(dark)), ScopeStack::new()))); }
        }
    }

    /// Drop cached lines from `line` on (text changed there).
    pub fn invalidate(&mut self, line: usize) {
        if self.lines.len() > line { self.lines.truncate(line); self.states.truncate(line + 1); }
    }


    /// Highlight lines until `upto` (exclusive) is cached, doing at most `budget` lines. Returns true if done.
    pub fn work(&mut self, rope: &Rope, upto: usize, budget: usize) -> bool {
        if self.syntax.is_none() { return true; }
        let total = rope.len_lines();
        let upto = upto.min(total);
        let start = self.lines.len();
        if start >= upto { return true; }
        let ss = syntax_set();
        let hl = Highlighter::new(theme(self.dark));
        let mut line = String::new();
        for i in start..upto.min(start + budget) {
            line.clear();
            for chunk in rope.line(i).chunks() { line.push_str(chunk); }
            while line.ends_with('\n') || line.ends_with('\r') { line.pop(); }
            line.push('\n');
            let (mut ps, mut hs) = self.states.last().cloned().unwrap();
            let ops = ps.parse_line(&line, ss).unwrap_or_default();
            let toks = HighlightIterator::new(&mut hs, &ops, &line, &hl)
                .filter_map(|(st, t)| {
                    let t = t.trim_end_matches(['\n', '\r']);
                    if t.is_empty() { return None; }
                    let c = st.foreground;
                    Some(Tok { text: t.to_string(), color: Color::from_argb_u8(255, c.r, c.g, c.b),
                        bold: st.font_style.contains(FontStyle::BOLD), italic: st.font_style.contains(FontStyle::ITALIC) })
                }).collect();
            self.lines.push(toks);
            self.states.push((ps, hs));
        }
        self.lines.len() >= upto
    }

    pub fn line(&self, i: usize) -> Option<&[Tok]> { self.lines.get(i).map(|v| v.as_slice()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn brackets() {
        let t = &Rope::from_str("fn a() { [1, (2)] }");
        assert_eq!(match_bracket(t, 7), Some((7, 18)));
        assert_eq!(match_bracket(t, 19), Some((18, 7)));
        assert_eq!(match_bracket(t, 5), Some((5, 4)));
    }
}

#[cfg(test)]
mod toml_tests {
    use super::*;
    #[test]
    fn toml_is_highlighted() {
        let mut h = Hl::default();
        h.configure("toml", Some("toml"), false);
        assert!(h.syntax.is_some());
        let r = Rope::from_str("[package]\nname = \"x\" # c\n");
        assert!(h.work(&r, 3, 10));
        assert!(h.line(1).unwrap().len() >= 3);
    }
}
