//! v0.6 panels: folder workspace + function list sidebar, compare, theme colours,
//! user keyboard shortcuts.
use super::*;

/// Built-in palettes (light, dark); user overrides from settings.json win.
const THEME_KEYS: &[(&str, &str, &str)] = &[
    ("editor-bg", "#ffffff", "#1e1e1e"), ("fg", "#24292f", "#d4d4d4"),
    ("gutter", "#f6f8fa", "#1e1e1e"), ("gutter-fg", "#6e7781", "#858585"),
    ("curline", "#eef4fc", "#2a2d2e"), ("sel", "#b6d7ff", "#264f78"),
    ("mark", "#b4f0b4", "#3a5a3a"), ("bracket", "#ffd97a", "#5a4a20"),
    ("chrome", "#f0f0f0", "#2b2b2b"), ("menu", "#f5f6f7", "#252526"),
    ("tabbar", "#e8e8e8", "#333337"), ("tab", "#dcdcdc", "#3c3c3c"), ("tab-active", "#ffffff", "#1e1e1e"),
    ("border", "#bbbbbb", "#444444"), ("hover", "#cce4f7", "#3e4a5a"),
    ("diff-add", "#d4f8d4", "#1f4a2a"), ("diff-del", "#ffd7d7", "#5a2a2a"), ("diff-chg", "#fff3c4", "#4a4520"),
];

pub fn parse_color(s: &str) -> Option<slint::Color> {
    let h = s.trim().strip_prefix('#')?;
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some(slint::Color::from_rgb_u8(v(0)?, v(2)?, v(4)?)),
        8 => Some(slint::Color::from_argb_u8(v(6)?, v(0)?, v(2)?, v(4)?)),
        3 => { let c = |i: usize| u8::from_str_radix(&h[i..i + 1].repeat(2), 16).ok(); Some(slint::Color::from_rgb_u8(c(0)?, c(1)?, c(2)?)) }
        _ => None,
    }
}

/// Normalised key combo, e.g. "Ctrl+Shift+K", "Alt+F5", "F9".
pub fn combo(text: &str, ctrl: bool, shift: bool, alt: bool) -> Option<String> {
    let c = text.chars().next()?;
    let key = match c {
        '\u{F704}'..='\u{F70F}' => format!("F{}", c as u32 - 0xF704 + 1),
        '\t' => "Tab".into(), '\n' | '\r' => "Enter".into(), ' ' => "Space".into(), '\u{1b}' => return None,
        '\u{F728}' => "Delete".into(), '\u{F727}' => "Insert".into(), '\u{F729}' => "Home".into(), '\u{F72B}' => "End".into(),
        '\u{F72C}' => "PageUp".into(), '\u{F72D}' => "PageDown".into(),
        '\u{F700}' => "Up".into(), '\u{F701}' => "Down".into(), '\u{F702}' => "Left".into(), '\u{F703}' => "Right".into(),
        c if c.is_control() => return None,
        c => c.to_uppercase().collect(),
    };
    let fkey = key.starts_with('F') && key.len() > 1;
    if !ctrl && !alt && !fkey { return None; } // never steal plain typing
    let mut s = String::new();
    if ctrl { s += "Ctrl+"; }
    if alt { s += "Alt+"; }
    if shift { s += "Shift+"; }
    Some(s + &key)
}

pub fn norm_combo(s: &str) -> String {
    let parts: Vec<String> = s.split('+').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    let has = |m: &str| parts.iter().any(|p| p.eq_ignore_ascii_case(m) || (m == "Ctrl" && p.eq_ignore_ascii_case("control")));
    let key = parts.iter().rev().find(|p| !["ctrl", "control", "alt", "shift"].contains(&p.to_lowercase().as_str())).cloned().unwrap_or_default();
    let key = if key.chars().count() == 1 { key.to_uppercase() } else { let mut c = key.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() };
    format!("{}{}{}{}", if has("Ctrl") { "Ctrl+" } else { "" }, if has("Alt") { "Alt+" } else { "" }, if has("Shift") { "Shift+" } else { "" }, key)
}

impl App {
    // ------------------------------------------------------------------ theme
    pub(crate) fn apply_theme(&self) {
        let ui = self.ui();
        let st = self.st.borrow();
        let dark = st.dark;
        let over = if dark { &st.cfg.theme_dark } else { &st.cfg.theme_light };
        let t = ui.global::<Theme>();
        for (k, l, d) in THEME_KEYS {
            let c = over.get(*k).and_then(|s| parse_color(s)).or_else(|| parse_color(if dark { d } else { l })).unwrap();
            match *k {
                "editor-bg" => t.set_editor_bg(c), "fg" => t.set_fg(c), "gutter" => t.set_gutter(c), "gutter-fg" => t.set_gutter_fg(c),
                "curline" => t.set_curline(c), "sel" => t.set_sel(c), "mark" => t.set_mark(c), "bracket" => t.set_bracket(c),
                "chrome" => t.set_chrome(c), "menu" => t.set_menu(c), "tabbar" => t.set_tabbar(c), "tab" => t.set_tab(c),
                "tab-active" => t.set_tab_active(c), "border" => t.set_border(c), "hover" => t.set_hover(c),
                "diff-add" => t.set_diff_add(c), "diff-del" => t.set_diff_del(c), "diff-chg" => t.set_diff_chg(c),
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------ shortcuts
    pub(crate) fn custom_key(&self, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        let Some(c) = combo(text, ctrl, shift, alt) else { return false };
        let act = { let st = self.st.borrow(); st.cfg.keys.iter().find(|(_, v)| norm_combo(v) == c).map(|(k, _)| k.clone()) };
        match act {
            Some(a) => { let me = self.me.borrow().clone(); slint::Timer::single_shot(Duration::from_millis(0), move || { if let Some(me) = me.upgrade() { me.action(&a) } }); true }
            None => false,
        }
    }

    /// settings.json was saved from the editor: pick up theme / shortcut changes
    pub(crate) fn reload_user_config(&self, path: &std::path::Path) {
        if path != settings::file().as_path() && crate::file_io::canon(&settings::file()).ok().as_deref() != Some(path) { return; }
        let text = match std::fs::read_to_string(path) { Ok(t) => t, Err(_) => return };
        match serde_json::from_str::<settings::Settings>(&text) {
            Ok(c) => { { let mut st = self.st.borrow_mut(); st.cfg.theme_light = c.theme_light; st.cfg.theme_dark = c.theme_dark; st.cfg.keys = c.keys; } self.apply_theme(); self.refresh_meta(); self.render_all(); }
            Err(e) => msg(&format!("設定檔格式錯誤，佈景主題 / 快捷鍵未更新：\n{e}")),
        }
    }

    pub(crate) fn edit_user_config(&self) {
        let f = settings::file();
        { let mut st = self.st.borrow_mut(); if st.cfg.keys.is_empty() { st.cfg.keys.insert("wordcount".into(), "Ctrl+Shift+W".into()); } }
        self.persist();
        self.open_path(f);
    }

    // ------------------------------------------------------------------ sidebar
    pub(crate) fn side_refresh(&self) {
        let ui = self.ui();
        let mut st = self.st.borrow_mut();
        let mode = st.side_mode;
        ui.set_side_mode(mode as i32);
        let mut items: Vec<SideItem> = vec![];
        match mode {
            1 => {
                match st.side_root.clone() {
                    Some(root) => {
                        st.side_rows = workspace::flatten(&root, &st.side_expanded, 20000);
                        ui.set_side_title(root.display().to_string().into());
                        let cur_path = st.docs[st.views[st.view_idx()].doc].path.clone();
                        let mut sel = -1;
                        for (i, r) in st.side_rows.iter().enumerate() {
                            if Some(&r.path) == cur_path.as_ref() { sel = i as i32; }
                            items.push(SideItem { text: r.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into(),
                                indent: r.depth as i32, kind: if r.dir { if r.open { 2 } else { 1 } } else { 0 }, line: -1 });
                        }
                        ui.set_side_sel(sel);
                    }
                    None => { st.side_rows.clear(); ui.set_side_title("(尚未開啟資料夾，按 📂 選擇)".into()); ui.set_side_sel(-1); }
                }
            }
            2 => {
                let di = st.views[st.view_idx()].doc;
                let d = &st.docs[di];
                let key = (di, d.buf.version);
                if st.side_sym_key != key {
                    st.side_syms = if d.hex.is_some() { vec![] } else { workspace::symbols(&d.lang, &d.text()) };
                    st.side_sym_key = key;
                }
                let d = &st.docs[di];
                ui.set_side_title(format!("{} — {} 個符號", d.title, st.side_syms.len()).into());
                let head_line = { let v = &st.views[st.view_idx()]; let r = &d.buf.rope; v.sels.get(v.primary).map(|s| r.char_to_line(s.head.min(r.len_chars()))).unwrap_or(0) };
                let sel = st.side_syms.iter().rposition(|s| s.line <= head_line).map(|i| i as i32).unwrap_or(-1);
                for s in &st.side_syms {
                    items.push(SideItem { text: s.name.as_str().into(), indent: s.depth as i32, kind: if d.lang == "markdown" { 4 } else { 3 }, line: s.line as i32 + 1 });
                }
                ui.set_side_sel(sel);
            }
            _ => {}
        }
        let model = ui.get_side_items();
        let same = model.row_count() == items.len() && items.iter().enumerate().all(|(i, t)| model.row_data(i).as_ref() == Some(t));
        if !same { ui.set_side_items(ModelRc::new(VecModel::from(items))); }
    }

    pub(crate) fn side_switch(&self, m: i32) {
        { let mut st = self.st.borrow_mut(); st.side_mode = m.clamp(0, 2) as u8; st.side_sym_key = (usize::MAX, 0); }
        if m == 1 && self.st.borrow().side_root.is_none() { self.side_open_folder(); }
        self.side_refresh();
        self.refresh_meta();
    }

    pub(crate) fn side_open_folder(&self) {
        let start = { let st = self.st.borrow(); st.side_root.clone().or_else(|| st.docs[st.cur].path.as_ref().and_then(|p| p.parent().map(|p| p.to_path_buf()))) };
        let mut d = crate::dlg::file().set_title("開啟資料夾作為工作區");
        if let Some(s) = start { d = d.set_directory(s); }
        if let Some(p) = d.pick_folder() { self.set_workspace(p); }
    }

    pub(crate) fn set_workspace(&self, p: PathBuf) {
        let p = crate::file_io::canon(&p).unwrap_or(p);
        { let mut st = self.st.borrow_mut(); st.side_root = Some(p); st.side_expanded.clear(); st.side_mode = 1; }
        self.side_refresh();
        self.refresh_meta();
    }

    pub(crate) fn side_click(&self, i: usize) {
        let mode = self.st.borrow().side_mode;
        if mode == 1 {
            let row = self.st.borrow().side_rows.get(i).cloned();
            if let Some(r) = row {
                if r.dir {
                    { let mut st = self.st.borrow_mut(); if !st.side_expanded.remove(&r.path) { st.side_expanded.insert(r.path.clone()); } }
                    self.side_refresh();
                } else { self.ui().set_side_sel(i as i32); }
            }
        } else if mode == 2 {
            let line = self.st.borrow().side_syms.get(i).map(|s| s.line);
            if let Some(l) = line { self.goto_line_n(l); }
        }
    }

    pub(crate) fn side_activate(&self, i: usize) {
        let row = { let st = self.st.borrow(); if st.side_mode == 1 { st.side_rows.get(i).cloned() } else { None } };
        match row {
            Some(r) if !r.dir => { self.open_path(r.path); self.side_refresh(); }
            Some(_) => {}
            None => self.side_click(i),
        }
    }

    pub(crate) fn goto_line_n(&self, line: usize) {
        let off = { let st = self.st.borrow(); let r = &st.docs[self.cd()].buf.rope; r.line_to_byte(line.min(r.len_lines().saturating_sub(1))) };
        self.select_bytes(off, off);
        // show the target near the top instead of the bottom edge
        let vi = self.av();
        { let mut st = self.st.borrow_mut(); let r = st.docs[st.views[vi].doc].buf.rope.clone(); let v = &mut st.views[vi]; v.top = v.layout.row_of(&r, v.sels[0].head).saturating_sub(5); v.follow = false; }
        self.render(vi);
        self.ui().invoke_focus_editor();
    }

    // ------------------------------------------------------------------ compare
    pub(crate) fn compare(self: &Rc<Self>) {
        // left = main view document; right = split-view document, or a file picked now
        let (split, a, b) = { let st = self.st.borrow(); (st.split, st.views[0].doc, st.views[1].doc) };
        let a = if split { a } else { self.st.borrow().cur };
        let b = if split && a != b { b } else {
            let Some(f) = crate::dlg::file().set_title("選擇要比較的檔案 (右側)").pick_file() else { return };
            self.open_path(f.clone());
            let st = self.st.borrow();
            match st.docs.iter().position(|d| d.path.as_deref() == Some(crate::file_io::canon(&f).unwrap_or(f.clone()).as_path())) { Some(i) => i, None => return }
        };
        if a == b { msg("請選擇另一個檔案來比較。"); return; }
        {
            let mut st = self.st.borrow_mut();
            st.store_view0();
            st.cur = a;
            st.load_view0();
            st.split = true;
            let d = &st.docs[b];
            let (sels, top) = (d.sels.clone(), d.top);
            let v = &mut st.views[1];
            v.doc = b; v.sels = if sels.is_empty() { vec![Sel::caret(0)] } else { sels }; v.primary = 0; v.top = top; v.sx = 0.0; v.follow = true; v.ac = None;
            v.lkey = (usize::MAX, 0, 0, 0, false);
            st.active = 0;
        }
        self.recompare();
        self.refresh_meta();
        let me = self.me.borrow().clone();
        slint::Timer::single_shot(Duration::from_millis(30), move || { if let Some(a) = me.upgrade() { a.render_all(); a.diff_nav(true, true); } });
    }

    pub(crate) fn recompare(&self) {
        let mut st = self.st.borrow_mut();
        let (a, b) = (st.views[0].doc, st.views[1].doc);
        if !st.split || a == b { return; }
        let m = workspace::diff(&st.docs[a].text(), &st.docs[b].text());
        let (va, vb) = (st.docs[a].buf.version, st.docs[b].buf.version);
        let stats = m.stats;
        st.docs[a].diff = Some((va, m.left, m.hunks_left));
        st.docs[b].diff = Some((vb, m.right, m.hunks_right));
        drop(st);
        let t = if stats == (0, 0, 0) { "比較結果：兩個檔案內容相同".to_string() }
            else { format!("比較：新增 {} 行、刪除 {} 行、修改 {} 行 (F7 / Shift+F7 下一個 / 上一個差異)", stats.0, stats.1, stats.2) };
        self.ui().set_find_status(t.clone().into());
        self.ui().set_st_type(t.into());
    }

    pub(crate) fn compare_clear(&self) {
        { let mut st = self.st.borrow_mut(); for d in st.docs.iter_mut() { d.diff = None; } }
        self.refresh_meta();
        self.render_all();
    }

    /// Jump both views to the next / previous difference.
    pub(crate) fn diff_nav(&self, forward: bool, first: bool) {
        let stale = { let st = self.st.borrow(); [0usize, 1].iter().any(|&vi| { let d = &st.docs[st.views[vi].doc]; d.diff.as_ref().is_some_and(|x| x.0 != d.buf.version) }) };
        if stale { self.recompare(); }
        let (target, hunks_b) = {
            let st = self.st.borrow();
            if !st.split { return; }
            let vi = st.view_idx();
            let d = &st.docs[st.views[vi].doc];
            let Some((_, _, hunks)) = &d.diff else { msg("尚未比較。請使用「工具 → 比較兩個檔案」。"); return };
            if hunks.is_empty() { return; }
            let r = &d.buf.rope;
            let v = &st.views[vi];
            let line = r.char_to_line(v.sels[v.primary.min(v.sels.len() - 1)].head.min(r.len_chars()));
            let k = if first { Some(0) } else if forward { hunks.iter().position(|&h| h > line) } else { hunks.iter().rposition(|&h| h < line) };
            let Some(k) = k else { self.ui().set_st_type(if forward { "已是最後一個差異".into() } else { "已是第一個差異".into() }); return };
            let other = &st.docs[st.views[1 - vi].doc];
            (Some((vi, hunks[k])), other.diff.as_ref().and_then(|o| o.2.get(k).copied()))
        };
        if let Some((vi, l)) = target {
            self.move_view_to_line(vi, l);
            if let Some(ol) = hunks_b { self.move_view_to_line(1 - vi, ol); }
            self.render_all();
        }
    }

    fn move_view_to_line(&self, vi: usize, line: usize) {
        let mut st = self.st.borrow_mut();
        let di = st.views[vi].doc;
        let r = st.docs[di].buf.rope.clone();
        let c = r.line_to_char(line.min(r.len_lines().saturating_sub(1)));
        let v = &mut st.views[vi];
        v.sels = vec![Sel::caret(c)]; v.primary = 0; v.follow = true;
        v.top = line.saturating_sub(3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colors_and_combos() {
        assert_eq!(parse_color("#ff0000"), Some(slint::Color::from_rgb_u8(255, 0, 0)));
        assert_eq!(parse_color("#0f0"), Some(slint::Color::from_rgb_u8(0, 255, 0)));
        assert_eq!(parse_color("#11223380"), Some(slint::Color::from_argb_u8(0x80, 0x11, 0x22, 0x33)));
        assert_eq!(parse_color("red"), None);
        assert_eq!(combo("k", true, true, false).as_deref(), Some("Ctrl+Shift+K"));
        assert_eq!(combo("k", false, false, false), None);
        assert_eq!(combo("\u{F708}", false, false, false).as_deref(), Some("F5"));
        assert_eq!(norm_combo("shift + ctrl + k"), "Ctrl+Shift+K");
        assert_eq!(norm_combo("alt+f5"), "Alt+F5");
        for (k, l, d) in THEME_KEYS { assert!(parse_color(l).is_some() && parse_color(d).is_some(), "{k}"); }
    }
}
