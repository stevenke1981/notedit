//! Headless editor core: rope buffer, multi-selection editing, undo grouping,
//! visual-row layout (word wrap + folding) and fold-point detection.
//! Everything here is independent of Slint so it can be unit-tested.
use ropey::Rope;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

pub const TAB: usize = 4;

/// Continuation for a Markdown list item at the end of a line.
pub fn markdown_list_next(line: &str) -> Option<(String, bool)> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    let bytes = rest.as_bytes();
    let (marker, content) = if bytes.len() >= 2 && matches!(bytes[0], b'-' | b'+' | b'*') && bytes[1] == b' ' {
        (rest[..2].to_string(), &rest[2..])
    } else {
        let n = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        if n == 0 || n > 9 || bytes.get(n).is_none_or(|b| !matches!(b, b'.' | b')')) || bytes.get(n + 1) != Some(&b' ') { return None; }
        let number = rest[..n].parse::<u64>().ok()?.checked_add(1)?;
        (format!("{number}{} ", bytes[n] as char), &rest[n + 2..])
    };
    let (task, content) = if content.starts_with("[ ] ") || content.starts_with("[x] ") || content.starts_with("[X] ") {
        ("[ ] ", &content[4..])
    } else { ("", content) };
    Some((format!("{indent}{marker}{task}"), content.trim().is_empty()))
}

/// A selection in char offsets. `anchor == head` is a plain caret.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sel { pub anchor: usize, pub head: usize, pub goal: Option<usize> }

impl Sel {
    pub fn caret(p: usize) -> Self { Sel { anchor: p, head: p, goal: None } }
    pub fn new(anchor: usize, head: usize) -> Self { Sel { anchor, head, goal: None } }
    pub fn start(&self) -> usize { self.anchor.min(self.head) }
    pub fn end(&self) -> usize { self.anchor.max(self.head) }
    pub fn is_empty(&self) -> bool { self.anchor == self.head }
}

/// Sort selections and merge overlapping ones. Returns the new index of `primary`.
pub fn normalize(sels: &mut Vec<Sel>, primary: usize) -> usize {
    if sels.is_empty() { sels.push(Sel::caret(0)); return 0; }
    let prim = sels[primary.min(sels.len() - 1)];
    sels.sort_by_key(|s| (s.start(), s.end()));
    let mut out: Vec<Sel> = Vec::with_capacity(sels.len());
    for s in sels.drain(..) {
        if let Some(last) = out.last_mut() {
            let same = s.start() == last.start() && s.end() == last.end();
            if same || s.start() < last.end() || (s.is_empty() && last.is_empty() && s.start() == last.start()) {
                let (st, en) = (last.start().min(s.start()), last.end().max(s.end()));
                let fwd = last.head >= last.anchor;
                *last = if fwd { Sel::new(st, en) } else { Sel::new(en, st) };
                continue;
            }
        }
        out.push(s);
    }
    *sels = out;
    sels.iter().position(|s| s.start() <= prim.head && prim.head <= s.end()).unwrap_or(sels.len() - 1)
}

/// One applied replacement, in coordinates valid after all earlier changes of the same batch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change { pub start: usize, pub removed: usize, pub inserted: usize }

pub fn map_pos(mut pos: usize, changes: &[Change]) -> usize {
    for c in changes {
        if pos >= c.start + c.removed { pos = pos - c.removed + c.inserted; }
        else if pos > c.start { pos = c.start + c.inserted; }
    }
    pos
}

/// Apply sorted, non-overlapping replacements. Returns changes and the end offset of each inserted text.
pub fn apply(rope: &mut Rope, edits: &[(usize, usize, String)]) -> (Vec<Change>, Vec<usize>) {
    let mut delta: isize = 0;
    let mut changes = Vec::new();
    let mut ends = Vec::new();
    for (s, e, t) in edits {
        let s2 = (*s as isize + delta) as usize;
        let e2 = (*e as isize + delta) as usize;
        let len = rope.len_chars();
        let (s2, e2) = (s2.min(len), e2.min(len).max(s2.min(len)));
        if e2 > s2 { rope.remove(s2..e2); }
        let n = t.chars().count();
        if n > 0 { rope.insert(s2, t); }
        changes.push(Change { start: s2, removed: e2 - s2, inserted: n });
        ends.push(s2 + n);
        delta += n as isize - (e2 - s2) as isize;
    }
    (changes, ends)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EditKind { Insert, Delete, Other }

#[derive(Clone)]
struct Snap { rope: Rope, sels: Vec<Sel> }

pub struct Buffer {
    pub rope: Rope,
    pub version: u64,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last: Option<(Instant, EditKind)>,
    /// lowest line touched since the highlighter last looked
    pub dirty_line: Option<usize>,
}

const UNDO_MAX: usize = 1000;

impl Default for Buffer { fn default() -> Self { Buffer::from_str("") } }

impl Buffer {
    pub fn from_str(s: &str) -> Self { Buffer { rope: Rope::from_str(s), version: 1, undo: vec![], redo: vec![], last: None, dirty_line: Some(0) } }
    pub fn text(&self) -> String { self.rope.to_string() }
    pub fn len(&self) -> usize { self.rope.len_chars() }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
    pub fn break_group(&mut self) { self.last = None; }

    /// Replace content without history (load / reload).
    pub fn reset(&mut self, s: &str) {
        self.rope = Rope::from_str(s);
        self.undo.clear(); self.redo.clear(); self.last = None;
        self.touch(0);
    }

    fn touch(&mut self, line: usize) {
        self.version += 1;
        self.dirty_line = Some(self.dirty_line.map_or(line, |d| d.min(line)));
    }

    fn snapshot(&mut self, sels: &[Sel], kind: EditKind) {
        let now = Instant::now();
        let group = matches!(self.last, Some((t, k)) if k == kind && kind != EditKind::Other && now.duration_since(t) < Duration::from_millis(1000));
        if !group {
            self.undo.push(Snap { rope: self.rope.clone(), sels: sels.to_vec() });
            if self.undo.len() > UNDO_MAX { self.undo.remove(0); }
        }
        self.redo.clear();
        self.last = Some((now, kind));
    }

    /// Apply replacements (any order, must not overlap) recording undo. Returns the changes.
    pub fn edit(&mut self, sels_before: &[Sel], mut edits: Vec<(usize, usize, String)>, kind: EditKind) -> (Vec<Change>, Vec<usize>, Vec<usize>) {
        // keep track of original order for the returned end offsets
        let mut order: Vec<usize> = (0..edits.len()).collect();
        order.sort_by_key(|&i| edits[i].0);
        let sorted: Vec<(usize, usize, String)> = order.iter().map(|&i| std::mem::take(&mut edits[i])).collect();
        if sorted.iter().all(|(s, e, t)| s == e && t.is_empty()) { return (vec![], vec![], order); }
        self.snapshot(sels_before, kind);
        let first = sorted.first().map(|e| e.0).unwrap_or(0).min(self.rope.len_chars());
        let line = self.rope.char_to_line(first);
        let (changes, ends) = apply(&mut self.rope, &sorted);
        self.touch(line);
        (changes, ends, order)
    }

    /// Replace the whole text as one undoable step.
    pub fn set_text(&mut self, sels: &[Sel], s: &str) {
        if self.rope == s { return; }
        self.snapshot(sels, EditKind::Other);
        let line = first_diff_line(&self.rope, &Rope::from_str(s));
        self.rope = Rope::from_str(s);
        self.touch(line);
        self.last = None;
    }

    pub fn undo(&mut self, sels: &mut Vec<Sel>) -> bool { self.step(sels, true) }
    pub fn redo(&mut self, sels: &mut Vec<Sel>) -> bool { self.step(sels, false) }

    fn step(&mut self, sels: &mut Vec<Sel>, undo: bool) -> bool {
        let Some(snap) = (if undo { self.undo.pop() } else { self.redo.pop() }) else { return false };
        let cur = Snap { rope: std::mem::replace(&mut self.rope, snap.rope), sels: sels.clone() };
        let line = first_diff_line(&cur.rope, &self.rope);
        if undo { self.redo.push(cur) } else { self.undo.push(cur) }
        *sels = snap.sels;
        let len = self.rope.len_chars();
        for s in sels.iter_mut() { s.anchor = s.anchor.min(len); s.head = s.head.min(len); }
        self.last = None;
        self.touch(line);
        true
    }
}

fn first_diff_line(a: &Rope, b: &Rope) -> usize {
    let mut chars = 0usize;
    let (mut ia, mut ib) = (a.chars(), b.chars());
    loop {
        match (ia.next(), ib.next()) {
            (Some(x), Some(y)) if x == y => chars += 1,
            (None, None) => return a.len_lines().saturating_sub(1),
            _ => break,
        }
    }
    a.char_to_line(chars.min(a.len_chars()))
}

// ---------------------------------------------------------------- text helpers

pub fn is_word(c: char) -> bool { c.is_alphanumeric() || c == '_' }

/// Number of chars in `line` excluding the line break.
pub fn line_len(rope: &Rope, line: usize) -> usize {
    if line >= rope.len_lines() { return 0; }
    let l = rope.line(line);
    let n = l.len_chars();
    if n > 0 && l.char(n - 1) == '\n' { if n > 1 && l.char(n - 2) == '\r' { n - 2 } else { n - 1 } } else { n }
}

pub fn line_string(rope: &Rope, line: usize) -> String {
    if line >= rope.len_lines() { return String::new(); }
    let mut s = rope.line(line).to_string();
    while s.ends_with('\n') || s.ends_with('\r') { s.pop(); }
    s
}

pub fn char_cells(c: char, col: usize) -> usize {
    if c == '\t' { TAB - col % TAB } else { UnicodeWidthChar::width(c).unwrap_or(1) }
}

/// Cells taken by chars[from..to] when the row starts at `from0`.
pub fn cells_between(chars: &[char], from0: usize, to: usize) -> usize {
    let mut col = 0;
    for &c in &chars[from0.min(chars.len())..to.min(chars.len())] { col += char_cells(c, col); }
    col
}

/// Char offset (within row starting at `start`, ending at `end`) closest to `cells`.
pub fn offset_at_cells(chars: &[char], start: usize, end: usize, cells: f32) -> usize {
    let mut col = 0usize;
    for (i, &c) in chars.iter().enumerate().take(end.min(chars.len())).skip(start) {
        let w = char_cells(c, col);
        if cells < col as f32 + w as f32 / 2.0 { return i; }
        col += w;
    }
    end.min(chars.len())
}

// ---------------------------------------------------------------- layout

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row { pub line: usize, pub start: usize, pub end: usize }

/// Visual rows of a document: identity (one row per line) when there is no wrap and no fold.
#[derive(Default)]
pub struct Layout { rows: Option<Vec<Row>>, nlines: usize }

/// Split one line into wrapped segments (char ranges).
pub fn wrap_line(chars: &[char], cols: usize) -> Vec<(usize, usize)> {
    let cols = cols.max(4);
    let mut out = Vec::new();
    let mut start = 0;
    let mut col = 0;
    let mut brk: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let w = char_cells(c, col);
        if col + w > cols && i > start {
            let cut = match brk { Some(b) if b > start => b, _ => i };
            out.push((start, cut));
            start = cut; col = 0; brk = None;
            i = cut;
            continue;
        }
        col += w;
        if c == ' ' || c == '\t' || w == 2 || matches!(c, ',' | ';' | '-' | '/' | '、' | '，' | '。') { brk = Some(i + 1); }
        i += 1;
    }
    out.push((start, chars.len()));
    out
}

impl Layout {
    pub fn build(rope: &Rope, wrap_cols: Option<usize>, folds: &[(usize, usize)]) -> Self {
        let nlines = rope.len_lines();
        if wrap_cols.is_none() && folds.is_empty() { return Layout { rows: None, nlines }; }
        let mut rows = Vec::with_capacity(nlines);
        let mut fi = 0;
        let mut line = 0;
        let mut buf: Vec<char> = Vec::new();
        while line < nlines {
            match wrap_cols {
                Some(cols) => {
                    buf.clear();
                    let l = rope.line(line);
                    buf.extend(l.chars().filter(|&c| c != '\n' && c != '\r'));
                    for (s, e) in wrap_line(&buf, cols) { rows.push(Row { line, start: s, end: e }); }
                }
                None => rows.push(Row { line, start: 0, end: line_len(rope, line) }),
            }
            while fi < folds.len() && folds[fi].0 < line { fi += 1; }
            if fi < folds.len() && folds[fi].0 == line { line = folds[fi].1.max(line) + 1; } else { line += 1; }
        }
        Layout { rows: Some(rows), nlines }
    }
    pub fn len(&self) -> usize { self.rows.as_ref().map(|r| r.len()).unwrap_or(self.nlines) }
    pub fn row(&self, rope: &Rope, i: usize) -> Row {
        match &self.rows {
            Some(r) => r.get(i).copied().unwrap_or(Row { line: self.nlines.saturating_sub(1), start: 0, end: 0 }),
            None => Row { line: i, start: 0, end: line_len(rope, i) },
        }
    }
    /// Visual row containing char offset `pos` (rows hidden by folds map to the fold header).
    pub fn row_of(&self, rope: &Rope, pos: usize) -> usize {
        let pos = pos.min(rope.len_chars());
        let line = rope.char_to_line(pos);
        let col = pos - rope.line_to_char(line);
        match &self.rows {
            None => line,
            Some(rows) => {
                let i = rows.partition_point(|r| (r.line, r.start) <= (line, col));
                let i = i.saturating_sub(1);
                // a caret at the exact end of a wrapped segment belongs to the next segment
                i
            }
        }
    }
}

// ---------------------------------------------------------------- folding

fn indent_of(s: &str) -> Option<usize> {
    if s.trim().is_empty() { return None; }
    let mut n = 0;
    for c in s.chars() { match c { ' ' => n += 1, '\t' => n += TAB, _ => break } }
    Some(n)
}

/// Last line of the foldable block starting at `line`, if any.
/// Brackets win: an unmatched `{`/`[`/`(` on the line folds to its matching close.
/// Otherwise an indentation increase on the next non-blank line folds the indented block.
pub fn fold_end(rope: &Rope, line: usize) -> Option<usize> {
    let nlines = rope.len_lines();
    if line + 1 >= nlines { return None; }
    let text = line_string(rope, line);
    // bracket based
    let mut stack: Vec<char> = Vec::new();
    for c in text.chars() {
        match c { '{' | '[' | '(' => stack.push(c), '}' | ']' | ')' => { stack.pop(); } _ => {} }
    }
    if let Some(&open) = stack.first() {
        let close = match open { '{' => '}', '[' => ']', _ => ')' };
        let mut depth = stack.iter().filter(|&&c| c == open).count() as i64;
        let start = rope.line_to_char(line + 1);
        let limit = (start + 2_000_000).min(rope.len_chars());
        let mut pos = start;
        for c in rope.slice(start..limit).chars() {
            if c == open { depth += 1 } else if c == close { depth -= 1; if depth == 0 { let end = rope.char_to_line(pos); return (end > line).then_some(end); } }
            pos += 1;
        }
        return None;
    }
    // indentation based
    let base = indent_of(&text)?;
    let mut last = None;
    for l in (line + 1)..nlines.min(line + 20_000) {
        let s = line_string(rope, l);
        match indent_of(&s) {
            None => continue,
            Some(i) if i > base => last = Some(l),
            _ => break,
        }
    }
    last
}

/// Resolve fold start lines into sorted (start, end) ranges, dropping invalid / nested-in-folded ones.
pub fn fold_ranges(rope: &Rope, folded: &BTreeSet<usize>) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &l in folded {
        if out.last().map(|r| l <= r.1).unwrap_or(false) { continue; }
        if let Some(e) = fold_end(rope, l) { out.push((l, e)); }
    }
    out
}

/// Shift line-indexed marks (bookmarks / folds) after an edit that turned `old_lines` into `new_lines` lines starting at `line`.
pub fn shift_lines(set: &mut BTreeSet<usize>, line: usize, old_lines: usize, new_lines: usize) {
    if old_lines == new_lines { return; }
    let moved: Vec<usize> = set.iter().copied().filter(|&l| l > line).collect();
    for l in &moved { set.remove(l); }
    for l in moved {
        if l <= line + old_lines.saturating_sub(1) && new_lines < old_lines && l > line + new_lines.saturating_sub(1) { set.insert(line); continue; }
        let n = l as isize + new_lines as isize - old_lines as isize;
        if n > line as isize { set.insert(n as usize); } else { set.insert(line); }
    }
}

// ---------------------------------------------------------------- completion

pub fn keywords(lang: &str) -> &'static [&'static str] {
    match lang {
        "rust" => &["as", "async", "await", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "for", "impl", "loop", "match", "move", "mut", "pub", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "Option", "Result", "String", "Vec", "Some", "None", "Ok", "Err", "println", "format"],
        "python" => &["and", "async", "await", "break", "class", "continue", "def", "elif", "else", "except", "False", "finally", "for", "from", "global", "import", "lambda", "None", "nonlocal", "pass", "raise", "return", "True", "try", "while", "with", "yield", "print", "range", "len", "self"],
        "javascript" => &["async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "else", "export", "extends", "false", "finally", "function", "import", "instanceof", "return", "switch", "this", "throw", "true", "typeof", "undefined", "while", "console", "document", "window"],
        "c" | "cpp" | "cs" | "java" => &["break", "case", "char", "class", "const", "continue", "default", "double", "else", "enum", "float", "for", "include", "define", "long", "namespace", "private", "protected", "public", "return", "short", "signed", "sizeof", "static", "struct", "switch", "template", "typedef", "unsigned", "virtual", "void", "while"],
        "html" | "xml" => &["html", "head", "body", "div", "span", "script", "style", "title", "class", "href", "table", "input", "button"],
        "css" => &["background", "border", "color", "display", "flex", "font-size", "font-family", "height", "margin", "padding", "position", "width"],
        "sql" => &["SELECT", "FROM", "WHERE", "INSERT", "UPDATE", "DELETE", "JOIN", "GROUP", "ORDER", "LIMIT", "VALUES", "CREATE", "TABLE"],
        _ => &[],
    }
}

/// Completion candidates for `prefix` from document words + language keywords.
pub fn complete(words: &BTreeSet<String>, lang: &str, prefix: &str, limit: usize) -> Vec<String> {
    if prefix.chars().count() < 2 { return vec![]; }
    let lp = prefix.to_lowercase();
    let mut v: Vec<String> = keywords(lang).iter().map(|s| s.to_string()).chain(words.iter().cloned())
        .filter(|w| w != prefix && w.to_lowercase().starts_with(&lp)).collect();
    v.sort_by_key(|w| (w.to_lowercase(), w.clone()));
    v.dedup();
    v.truncate(limit);
    v
}

pub fn collect_words(rope: &Rope, limit_chars: usize) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let mut cur = String::new();
    for c in rope.chars().take(limit_chars) {
        if is_word(c) { cur.push(c) } else if !cur.is_empty() {
            if cur.chars().count() >= 3 && !cur.chars().next().unwrap().is_ascii_digit() { set.insert(std::mem::take(&mut cur)); } else { cur.clear(); }
        }
    }
    if cur.chars().count() >= 3 && !cur.starts_with(|c: char| c.is_ascii_digit()) { set.insert(cur); }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_list_continuation() {
        assert_eq!(markdown_list_next("  - 中文"), Some(("  - ".into(), false)));
        assert_eq!(markdown_list_next("9. item"), Some(("10. ".into(), false)));
        assert_eq!(markdown_list_next("* [x] done"), Some(("* [ ] ".into(), false)));
        assert_eq!(markdown_list_next("  - [ ] "), Some(("  - [ ] ".into(), true)));
        assert_eq!(markdown_list_next("-not a list"), None);
        assert_eq!(markdown_list_next("1234567890. item"), None);
    }

    #[test]
    fn multi_insert_and_map() {
        let mut r = Rope::from_str("abc\ndef\nghi");
        let (ch, ends) = apply(&mut r, &[(1, 1, "X".into()), (5, 6, "YY".into())]);
        assert_eq!(r.to_string(), "aXbc\ndYYf\nghi");
        assert_eq!(ends, vec![2, 8]);
        assert_eq!(map_pos(10, &ch), 12);
        assert_eq!(map_pos(0, &ch), 0);
    }

    #[test]
    fn normalize_merges() {
        let mut v = vec![Sel::new(5, 8), Sel::caret(1), Sel::new(7, 10), Sel::caret(1)];
        let p = normalize(&mut v, 0);
        assert_eq!(v, vec![Sel::caret(1), Sel::new(5, 10)]);
        assert_eq!(p, 1);
    }

    #[test]
    fn undo_groups_typing() {
        let mut b = Buffer::from_str("");
        let mut sels = vec![Sel::caret(0)];
        for (i, c) in "abc".chars().enumerate() {
            b.edit(&sels, vec![(i, i, c.to_string())], EditKind::Insert);
            sels = vec![Sel::caret(i + 1)];
        }
        b.edit(&sels, vec![(0, 3, String::new())], EditKind::Other);
        assert_eq!(b.text(), "");
        assert!(b.undo(&mut sels));
        assert_eq!(b.text(), "abc");
        assert!(b.undo(&mut sels));
        assert_eq!(b.text(), "");
        assert_eq!(sels, vec![Sel::caret(0)]);
        assert!(b.redo(&mut sels));
        assert_eq!(b.text(), "abc");
    }

    #[test]
    fn wrap_prefers_spaces_and_cjk_width() {
        let c: Vec<char> = "hello world foo".chars().collect();
        assert_eq!(wrap_line(&c, 12), vec![(0, 12), (12, 15)]);
        let c: Vec<char> = "中文字元".chars().collect();
        assert_eq!(wrap_line(&c, 4), vec![(0, 2), (2, 4)]);
        assert_eq!(cells_between(&"a\tb中".chars().collect::<Vec<_>>(), 0, 4), 7);
    }

    #[test]
    fn layout_with_folds_and_wrap() {
        let r = Rope::from_str("fn a() {\n  x();\n  y();\n}\nlong line here\n");
        let l = Layout::build(&r, None, &[(0, 3)]);
        assert_eq!(l.len(), 3); // header, "long line here", trailing empty
        assert_eq!(l.row(&r, 1).line, 4);
        assert_eq!(l.row_of(&r, r.line_to_char(2)), 0);
        let w = Layout::build(&r, Some(5), &[]);
        assert!(w.len() > r.len_lines());
        let row = w.row_of(&r, r.line_to_char(4) + 10);
        assert_eq!(w.row(&r, row).line, 4);
    }

    #[test]
    fn fold_points() {
        let r = Rope::from_str("fn a() {\n  x();\n}\ndef f():\n    pass\n    pass\nend\n");
        assert_eq!(fold_end(&r, 0), Some(2));
        assert_eq!(fold_end(&r, 1), None);
        assert_eq!(fold_end(&r, 3), Some(5));
        let mut s = BTreeSet::from([0, 3]);
        assert_eq!(fold_ranges(&r, &s), vec![(0, 2), (3, 5)]);
        shift_lines(&mut s, 1, 1, 3);
        assert_eq!(s, BTreeSet::from([0, 5]));
    }

    #[test]
    fn completion() {
        let r = Rope::from_str("let counter = 1; count_all(counter); 42abc");
        let w = collect_words(&r, usize::MAX);
        assert!(w.contains("counter") && !w.contains("42abc"));
        assert_eq!(complete(&w, "rust", "cou", 10), vec!["count_all", "counter"]);
        assert!(complete(&w, "rust", "c", 10).is_empty());
        assert!(complete(&w, "rust", "mat", 10).contains(&"match".to_string()));
    }

    #[test]
    fn offsets_at_cells() {
        let c: Vec<char> = "ab中d".chars().collect();
        assert_eq!(offset_at_cells(&c, 0, 4, 0.4), 0);
        assert_eq!(offset_at_cells(&c, 0, 4, 2.6), 2);
        assert_eq!(offset_at_cells(&c, 0, 4, 3.2), 3);
        assert_eq!(offset_at_cells(&c, 0, 4, 99.0), 4);
    }
}
