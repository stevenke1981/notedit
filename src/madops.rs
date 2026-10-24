//! MadEdit-derived features: text conversions, word count, numbering, hex strings,
//! Find in Files, spell check and column-mode toggling.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub enum FifMsg { Hits(Vec<fif::Hit>), Done(usize, usize) }

impl App {
    /// Apply `f` to the selection, or to the whole document when nothing is selected.
    fn transform(&self, f: impl Fn(&str) -> String) {
        if self.is_hex(self.av()) { msg("請先切換回文字模式。"); return; }
        let t = self.text();
        let (s, e) = self.selection();
        let (s, e, whole) = if e > s { (s, e, false) } else { (0, t.len(), true) };
        let r = f(&t[s..e]);
        if r == t[s..e] { return; }
        self.set_text(format!("{}{}{}", &t[..s], r, &t[e..]));
        if !whole { self.select_bytes(s, s + r.len()); }
    }

    fn tab_width(&self) -> usize { 4 }

    /// returns false when `a` is not one of ours
    pub(crate) fn mad_action(&self, a: &str) -> bool {
        let ui = self.ui();
        match a {
            "xf:s2t" => self.transform(textops::s2t),
            "xf:t2s" => self.transform(textops::t2s),
            "xf:full" => self.transform(textops::to_fullwidth),
            "xf:half" => self.transform(textops::to_halfwidth),
            "xf:tab2sp" => { let n = self.tab_width(); self.transform(|s| textops::tabs_to_spaces(s, n)) }
            "xf:sp2tab" => { let n = self.tab_width(); self.transform(|s| textops::spaces_to_tabs(s, n)) }
            "xf:trimlead" => self.transform(textops::trim_leading),
            "xf:trimboth" => self.transform(|s| s.split('\n').map(str::trim).collect::<Vec<_>>().join("\n")),
            "xf:join" => self.transform(textops::join_lines),
            "xf:delempty" => self.transform(|s| textops::delete_empty_lines(s, false)),
            "xf:delblank" => self.transform(|s| textops::delete_empty_lines(s, true)),
            "xf:cap" => self.transform(textops::capitalize),
            "xf:invert" => self.transform(textops::invert_case),
            "xf:sortdesc" => self.transform(|s| { let mut v: Vec<&str> = s.split('\n').collect(); v.sort(); v.reverse(); v.join("\n") }),
            "xf:dedup" => self.transform(|s| { let mut seen = std::collections::HashSet::new(); s.split('\n').filter(|l| seen.insert(*l)).collect::<Vec<_>>().join("\n") }),
            "xf:tohex" => {
                let (s, e) = self.selection();
                if e == s { msg("請先選取文字。"); return true; }
                let t = self.text();
                let bytes = { let st = self.st.borrow(); st.docs[self.cd()].encoding.encode(&t[s..e]).0.into_owned() };
                self.clipboard_set(textops::to_hex_string(&bytes, true));
            }
            "xf:fromhex" => {
                let (s, e) = self.selection();
                let t = self.text();
                let src = if e > s { t[s..e].to_string() } else { self.clipboard_get().unwrap_or_default() };
                match textops::from_hex_string(&src) {
                    Some(b) => {
                        let enc = self.st.borrow().docs[self.cd()].encoding;
                        let txt = enc.decode(&b).0.into_owned();
                        if e > s { self.transform(|_| txt.clone()); } else { self.insert_text(self.av(), &txt); }
                    }
                    None => msg("不是有效的十六進位字串（例如 4D6F64、4D 6F 64、%4D%6F%64、4D,6F,64）。"),
                }
            }
            "xf:number" => self.prompt("insnum", "插入遞增數字", "起始值, 遞增量, 最小位數（例如 1, 1, 3）：", "1, 1, 0"),
            "xf:align" => self.prompt("align", "欄位對齊", "對齊分隔字元（例如 = 或 ,）：", "="),
            "wordcount" => {
                let t = self.text();
                let (s, e) = self.selection();
                let c = textops::count(if e > s { &t[s..e] } else { &t });
                let fmt = |n: usize| n.to_string();
                msg(&format!("{}字數 (中日韓每字計一)：{}\n字元數 (不含空白)：{}\n空白字元：{}\n中日韓文字：{}\n半形字元：{}\n全形字元：{}\n行數：{}\n位元組 (UTF-8)：{}",
                    if e > s { "【選取範圍】\n" } else { "" }, fmt(c.words), fmt(c.chars), fmt(c.spaces), fmt(c.cjk), fmt(c.halfwidth), fmt(c.fullwidth), fmt(c.lines), fmt(c.bytes_utf8)));
            }
            "hexmode" => self.set_hex_mode(true),
            "textmode" => { self.st.borrow_mut().column_mode = false; self.set_hex_mode(false) }
            "columnmode" => {
                let was_hex = self.is_hex(self.av());
                self.set_hex_mode(false);
                { let mut st = self.st.borrow_mut(); st.column_mode = was_hex || !st.column_mode; }
                self.refresh_meta();
            }
            "spell" => { { let mut st = self.st.borrow_mut(); st.cfg.spell = !st.cfg.spell; } self.persist(); self.refresh_meta(); self.render_all(); }
            "fif" => {
                if ui.get_fif_dir().is_empty() {
                    let dir = { let st = self.st.borrow(); st.docs[self.cd()].path.as_ref().and_then(|p| p.parent()).map(|p| p.display().to_string()) }
                        .or_else(|| std::env::current_dir().ok().map(|p| p.display().to_string())).unwrap_or_default();
                    ui.set_fif_dir(dir.into());
                }
                let (s, e) = self.selection();
                if e > s && e - s < 200 { let t = self.text(); if !t[s..e].contains('\n') { ui.set_find_text(t[s..e].into()); } }
                ui.set_find_status("".into());
                ui.set_fif_visible(true);
            }
            x if x.starts_with("sp:") => self.spell_replace(&x[3..]),
            x if x.starts_with("spignore:") => {
                spell::ignore(&x[9..]);
                { let mut st = self.st.borrow_mut(); let w = x[9..].to_string(); if !st.cfg.spell_ignore.contains(&w) { st.cfg.spell_ignore.push(w); } }
                self.persist(); self.render_all();
            }
            _ => return false,
        }
        true
    }

    fn prompt(&self, kind: &str, title: &str, label: &str, def: &str) {
        let ui = self.ui();
        self.st.borrow_mut().prompt_kind = kind.into();
        ui.set_prompt_title(title.into()); ui.set_prompt_label(label.into()); ui.set_prompt_text(def.into());
        ui.set_prompt_visible(true);
    }

    pub(crate) fn prompt_ok(&self, text: &str) {
        let kind = std::mem::take(&mut self.st.borrow_mut().prompt_kind);
        match kind.as_str() {
            "insnum" => {
                let v: Vec<i64> = text.split([',', ' ']).filter(|s| !s.is_empty()).filter_map(|s| s.trim().parse().ok()).collect();
                let (a, b, w) = (v.first().copied().unwrap_or(1), v.get(1).copied().unwrap_or(1), v.get(2).copied().unwrap_or(0).max(0) as usize);
                self.transform(|s| textops::number_lines(s, a, b, w));
            }
            "align" => {
                let d = text.trim().to_string();
                if d.is_empty() { return; }
                self.transform(|s| align_on(s, &d));
            }
            _ => {}
        }
        self.ui().invoke_focus_editor();
    }

    // ---------------- spell check
    /// (char start, char end, word) of the word under the primary caret
    fn word_at_caret(&self) -> Option<(usize, usize, String)> {
        let st = self.st.borrow();
        let v = &st.views[st.view_idx()];
        let r = &st.docs[v.doc].buf.rope;
        let head = v.sels.get(v.primary)?.head.min(r.len_chars());
        let line = r.char_to_line(head);
        let ls = r.line_to_char(line);
        let chars = line_chars(r, line);
        let col = head - ls;
        spell::misspelled(&chars).into_iter().find(|&(a, b)| a <= col && col <= b)
            .map(|(a, b)| (ls + a, ls + b, chars[a..b].iter().collect()))
    }

    pub(crate) fn spell_items(&self) -> Vec<MenuItem> {
        if !self.st.borrow().cfg.spell || self.is_hex(self.av()) { return vec![]; }
        if !{ let st = self.st.borrow(); matches!(st.docs[self.cd()].lang.as_str(), "text" | "markdown") } { return vec![]; }
        let Some((_, _, w)) = self.word_at_caret() else { return vec![] };
        let mut v: Vec<MenuItem> = spell::suggest(&w).into_iter().map(|s| mi(&format!("➜ {s}"), "", &format!("sp:{s}"))).collect();
        if v.is_empty() { v.push(en("(無拼字建議)", "", "none", false)); }
        v.push(mi(&format!("忽略「{w}」"), "", &format!("spignore:{w}")));
        v.push(sep());
        v
    }

    fn spell_replace(&self, s: &str) {
        let Some((a, b, _)) = self.word_at_caret() else { return };
        let (ba, bb) = { let st = self.st.borrow(); let r = &st.docs[self.cd()].buf.rope; (r.char_to_byte(a), r.char_to_byte(b)) };
        self.select_bytes(ba, bb);
        self.transform(|_| s.to_string());
    }

    // ---------------- find in files
    pub(crate) fn fif_browse(&self) {
        let ui = self.ui();
        let mut d = rfd::FileDialog::new().set_title("選擇資料夾");
        let cur = ui.get_fif_dir().to_string();
        if !cur.is_empty() { d = d.set_directory(cur); }
        if let Some(p) = d.pick_folder() { ui.set_fif_dir(p.display().to_string().into()); }
    }

    pub(crate) fn fif_run(&self) {
        let ui = self.ui();
        let Some(re) = self.regex() else { return };
        let root = PathBuf::from(ui.get_fif_dir().to_string());
        if !root.is_dir() { ui.set_find_status("資料夾不存在".into()); return; }
        let filters = ui.get_fif_filter().to_string();
        let recursive = ui.get_fif_recursive();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        {
            let mut st = self.st.borrow_mut();
            st.fif_cancel.store(true, Ordering::Relaxed);
            st.fif_cancel = cancel.clone();
            st.fif_rx = Some(rx);
            st.hits.clear();
            st.file_hits.clear();
            st.fif_root = root.clone();
        }
        ui.set_hits(ModelRc::new(VecModel::from(Vec::<SearchHit>::new())));
        ui.set_hits_title(format!("在檔案中尋找「{}」：搜尋中… ({})", ui.get_find_text(), root.display()).into());
        ui.set_hits_visible(true);
        ui.set_fif_running(true);
        ui.set_find_status("".into());
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let (files, n) = fif::search(&root, &filters, recursive, &re, &cancel, 20000, |b| { if !b.is_empty() { let _ = tx2.send(FifMsg::Hits(b)); } });
            let _ = tx.send(FifMsg::Done(files, n));
        });
    }

    pub(crate) fn fif_stop(&self) { self.st.borrow().fif_cancel.store(true, Ordering::Relaxed); }

    /// called from the periodic tick
    pub(crate) fn fif_poll(&self) {
        let ui = self.ui();
        let mut new = vec![];
        let mut done = None;
        {
            let st = self.st.borrow();
            let Some(rx) = &st.fif_rx else { return };
            loop {
                match rx.try_recv() {
                    Ok(FifMsg::Hits(h)) => new.extend(h),
                    Ok(FifMsg::Done(f, n)) => { done = Some((f, n)); break; }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => { done = Some((0, 0)); break; }
                }
            }
        }
        if !new.is_empty() {
            let mut st = self.st.borrow_mut();
            let root = st.fif_root.clone();
            let rows: Vec<SearchHit> = new.iter().map(|h| SearchHit {
                doc: -1, title: h.path.strip_prefix(&root).unwrap_or(&h.path).display().to_string().into(), line: h.line as i32,
                start: h.start as i32, end: h.end as i32, text: h.snippet.as_str().into() }).collect();
            st.file_hits.extend(new.into_iter().map(|h| (h.path, h.line, h.start, h.end)));
            drop(st);
            let model = ui.get_hits();
            if let Some(vm) = model.as_any().downcast_ref::<VecModel<SearchHit>>() { for r in rows { vm.push(r); } }
            else { ui.set_hits(ModelRc::new(VecModel::from(rows))); }
        }
        if let Some((files, _)) = done {
            let n = self.st.borrow().file_hits.len();
            let cancelled = self.st.borrow().fif_cancel.load(Ordering::Relaxed);
            self.st.borrow_mut().fif_rx = None;
            ui.set_fif_running(false);
            ui.set_hits_title(format!("在檔案中尋找「{}」：在 {} 個檔案中找到 {} 筆{}", ui.get_find_text(), files, n, if cancelled { "（已停止）" } else { "" }).into());
        }
    }

    /// open a Find-in-Files hit
    pub(crate) fn file_hit_clicked(&self, i: usize) -> bool {
        let Some((path, line, s, e)) = self.st.borrow().file_hits.get(i).cloned() else { return false };
        if self.av() == 1 { self.st.borrow_mut().active = 0; }
        self.open_path(path);
        let (bs, be) = {
            let st = self.st.borrow();
            let r = &st.docs[st.cur].buf.rope;
            if line == 0 || line > r.len_lines() { return true; }
            let ls = r.line_to_char(line - 1);
            let n = line_chars(r, line - 1).len();
            (r.char_to_byte(ls + s.min(n)), r.char_to_byte(ls + e.min(n)))
        };
        self.select_bytes(bs, be);
        true
    }
}

/// Align lines of `s` on the first occurrence of `delim` (MadEdit "Column Align").
pub fn align_on(s: &str, delim: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    let col = lines.iter().filter_map(|l| l.find(delim).map(|i| l[..i].trim_end().chars().count())).max().unwrap_or(0);
    lines.iter().map(|l| match l.find(delim) {
        Some(i) => {
            let left = l[..i].trim_end();
            let right = l[i + delim.len()..].trim_start();
            let pad = " ".repeat(col - left.chars().count());
            if right.is_empty() { format!("{left}{pad} {delim}") } else { format!("{left}{pad} {delim} {right}") }
        }
        None => l.to_string(),
    }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn align() {
        assert_eq!(super::align_on("a = 1\nlong = 2\nx", "="), "a    = 1\nlong = 2\nx");
    }
}
