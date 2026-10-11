//! Incremental syntect highlighting with per-line state cache.
use crate::{HlLine, HlToken};
use slint::{Color, ModelRc, VecModel};
use std::sync::OnceLock;
use syntect::highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

pub fn syntax_set() -> &'static SyntaxSet { static S: OnceLock<SyntaxSet> = OnceLock::new(); S.get_or_init(SyntaxSet::load_defaults_newlines) }
pub fn theme() -> &'static Theme {
    static T: OnceLock<Theme> = OnceLock::new();
    T.get_or_init(|| { let mut ts = ThemeSet::load_defaults(); ts.themes.remove("InspiredGitHub").unwrap() })
}

#[derive(Clone)]
pub struct Tok { pub text: String, pub color: Color, pub bold: bool, pub italic: bool }

pub struct Hl {
    syntax: Option<String>, // syntax name
    src: String,            // text the cache corresponds to
    lines: Vec<Vec<Tok>>,   // highlighted lines
    states: Vec<(ParseState, HighlightState)>, // state at start of line i (len == lines.len()+1)
}

impl Default for Hl { fn default() -> Self { Hl { syntax: None, src: String::new(), lines: vec![], states: vec![] } } }

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
    pub fn sync(&mut self, text: &str, lang: &str, ext: Option<&str>) {
        let syn = find_syntax(lang, ext);
        let name = syn.map(|s| s.name.clone());
        if name != self.syntax {
            self.syntax = name; self.lines.clear(); self.states.clear();
            if let Some(s) = syn { self.states.push((ParseState::new(s), HighlightState::new(&Highlighter::new(theme()), ScopeStack::new()))); }
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
        let hl = Highlighter::new(theme());
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
    pub fn visible(&self, first: usize, count: usize) -> Vec<HlLine> {
        let mut out = Vec::new();
        for (i, raw) in self.src.split('\n').enumerate().skip(first).take(count) {
            let toks: Vec<HlToken> = match self.lines.get(i) {
                Some(t) => t.iter().map(|t| HlToken { text: t.text.as_str().into(), color: t.color, bold: t.bold, italic: t.italic }).collect(),
                None if raw.is_empty() => vec![],
                None => vec![HlToken { text: raw.into(), color: Color::from_rgb_u8(0x33, 0x33, 0x33), bold: false, italic: false }],
            };
            out.push(HlLine { line: i as i32, tokens: ModelRc::new(VecModel::from(toks)) });
        }
        out
    }
}
