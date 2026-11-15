//! Search, commands, and application actions.
use super::*;

impl App {
    pub(super) fn regex(&self) -> Option<regex::Regex> {
        let ui = self.ui();
        match search::build(&ui.get_find_text(), ui.get_match_case(), ui.get_whole_word(), ui.get_use_regex()) {
            Ok(r) => Some(r),
            Err(e) => { ui.set_find_status(e.into()); None }
        }
    }

    pub(super) fn find(&self, forward: bool) -> bool {
        let ui = self.ui();
        let Some(re) = self.regex() else { return false };
        let (a, c) = self.sel_bytes();
        let found = search::find(&re, &self.text(), a.min(c), a.max(c), forward);
        match found {
            Some((s, e)) => { self.select_bytes(s, e); ui.set_find_status("".into()); true }
            None => { ui.set_find_status(format!("找不到「{}」", ui.get_find_text()).into()); false }
        }
    }

    pub(super) fn find_all(&self) {
        let ui = self.ui();
        let Some(re) = self.regex() else { return };
        let mut rows = Vec::new();
        let mut hits = Vec::new();
        {
            let st = self.st.borrow();
            for (di, d) in st.docs.iter().enumerate() {
                let text = d.text();
                let d_text = &text;
                for m in re.find_iter(d_text).take(5000) {
                    if m.start() == m.end() { continue; }
                    let ls = d_text[..m.start()].rfind('\n').map(|x| x + 1).unwrap_or(0);
                    let le = d_text[m.start()..].find('\n').map(|x| x + m.start()).unwrap_or(d_text.len());
                    let line = d.buf.rope.byte_to_line(m.start()) + 1;
                    let snippet: String = d_text[ls..le].trim().chars().take(200).collect();
                    rows.push(SearchHit { doc: di as i32, title: d.title.clone().into(), line: line as i32, start: m.start() as i32, end: m.end() as i32, text: snippet.into() });
                    hits.push((di, m.start(), m.end()));
                }
            }
            ui.set_hits_title(format!("搜尋「{}」：在 {} 個文件中找到 {} 筆", ui.get_find_text(), st.docs.len(), rows.len()).into());
        }
        { let mut st = self.st.borrow_mut(); st.hits = hits; st.file_hits.clear(); }
        ui.set_hits(ModelRc::new(VecModel::from(rows)));
        ui.set_hits_visible(true);
    }

    pub(super) fn hit_clicked(&self, i: usize) {
        if !self.st.borrow().file_hits.is_empty() { self.file_hit_clicked(i); return; }
        let Some(&(d, s, e)) = self.st.borrow().hits.get(i) else { return };
        if d != self.cd() { if self.av() == 1 { self.st.borrow_mut().active = 0; } self.select_tab(d); }
        let ok = { let t = self.text(); e <= t.len() && t.is_char_boundary(s) && t.is_char_boundary(e) };
        if ok { self.select_bytes(s, e); }
    }

    pub(super) fn replace(&self, all: bool) {
        let ui = self.ui();
        let Some(re) = self.regex() else { return };
        let r = ui.get_replace_text().to_string();
        let rep = if ui.get_use_regex() { r.clone() } else { r.replace('$', "$$") };
        let t = self.text();
        if all {
            let c = re.find_iter(&t).count();
            let nt = re.replace_all(&t, rep.as_str()).into_owned();
            self.set_text(nt);
            ui.set_find_status(format!("已取代 {} 處", c).into());
            return;
        }
        let (s, e) = self.selection();
        if let Some(caps) = re.captures_at(&t, s).filter(|c| { let m = c.get(0).unwrap(); m.start() == s && m.end() == e && e > s }) {
            let mut out = String::new();
            caps.expand(&rep, &mut out);
            self.set_text(format!("{}{}{}", &t[..s], out, &t[e..]));
            let p = s + out.len();
            self.select_bytes(p, p);
        }
        self.find(true);
    }

    pub(super) fn action_plain(&self, a: &str) {
        let cur = self.cd();
        match a {
            "undo" | "redo" => self.undo(self.av(), a == "undo"),
            "gotobrace" => {
                let vi = self.av();
                let m = { let st = self.st.borrow(); let v = &st.views[vi]; highlight::match_bracket(&st.docs[v.doc].buf.rope, v.sels[v.primary.min(v.sels.len() - 1)].head) };
                if let Some((_, j)) = m { { let mut st = self.st.borrow_mut(); let v = &mut st.views[vi]; v.sels = vec![Sel::caret(j)]; v.primary = 0; v.follow = true; } self.unfold_at_caret(vi); self.render(vi); self.refresh_status(); }
            }
            "bookmark" => self.toggle_bookmark(),
            "nextbm" => self.jump_bookmark(true),
            "prevbm" => self.jump_bookmark(false),
            "clearbm" => { { let mut st = self.st.borrow_mut(); st.docs[cur].bookmarks.clear(); } self.render_all(); }
            "zoomin" => { { let mut st = self.st.borrow_mut(); st.font = (st.font + 1.0).min(48.0); } self.refresh_meta(); }
            "zoomout" => { { let mut st = self.st.borrow_mut(); st.font = (st.font - 1.0).max(6.0); } self.refresh_meta(); }
            "print" | "exporthtml" => {
                let (html, title, path) = {
                    let st = self.st.borrow(); let d = &st.docs[cur];
                    let ext = d.path.as_ref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase());
                    (highlight::export_html(&d.text(), &d.lang, ext.as_deref(), &d.title, false), d.title.clone(), d.path.clone())
                };
                let target = if a == "print" {
                    let p = std::env::temp_dir().join(format!("notedit-print-{}.html", std::process::id()));
                    Some(p)
                } else {
                    let mut dlg = crate::dlg::file().set_title("匯出為 HTML").set_file_name(format!("{}.html", title)).add_filter("HTML", &["html"]);
                    if let Some(dir) = path.as_ref().and_then(|p| p.parent()) { dlg = dlg.set_directory(dir); }
                    dlg.save_file()
                };
                if let Some(p) = target {
                    let html = if a == "print" { html.replace("</body>", "<script>window.onload=()=>window.print()</script></body>") } else { html };
                    match std::fs::write(&p, html) { Ok(_) => { if a == "print" { open_external(&p); } } Err(e) => msg(&format!("寫入失敗：{}", e)) }
                }
            }
            _ => {}
        }
    }

    pub(super) fn context_menu(&self, kind: &str, index: i32) {
        let ui = self.ui();
        let items = if kind == "tab" {
            let i = (index.max(0) as usize).min(self.st.borrow().docs.len() - 1);
            self.st.borrow_mut().ctx_tab = i;
            let st = self.st.borrow();
            let n = st.docs.len();
            let has_path = st.docs[i].path.is_some();
            vec![en("關閉", "Ctrl+W", "ctx:close", true), en("關閉其他分頁", "", "ctx:closeothers", n > 1),
                 en("關閉右側所有分頁", "", "ctx:closeright", i + 1 < n), en("關閉左側所有分頁", "", "ctx:closeleft", i > 0), sep(),
                 en("儲存", "Ctrl+S", "ctx:save", true), en("另存新檔...", "", "ctx:saveas", true), en("重新命名...", "", "ctx:rename", true), sep(),
                 en("複製完整路徑", "", "ctx:copypath", has_path), en("複製檔名", "", "ctx:copyname", true), en("開啟所在資料夾", "", "ctx:openfolder", has_path)]
        } else {
            if index == 1 && self.st.borrow().split { self.ed_focused(1); } else if index == 0 { self.ed_focused(0); }
            // close an open autocomplete popup so it cannot swallow the menu
            let vi = self.av();
            if self.st.borrow().views[vi].ac.is_some() { self.st.borrow_mut().views[vi].ac = None; self.render(vi); }
            let (s, e) = self.selection();
            let sel = e > s;
            let st = self.st.borrow();
            let d = &st.docs[st.views[st.view_idx()].doc];
            let pv = matches!(d.lang.as_str(), "markdown" | "html" | "xml");
            let spell_items = { drop(st); self.spell_items() };
            let st = self.st.borrow();
            let d = &st.docs[st.views[st.view_idx()].doc];
            spell_items.into_iter().chain([en("復原", "Ctrl+Z", "undo", d.buf.can_undo()), en("取消復原", "Ctrl+Y", "redo", d.buf.can_redo()), sep(),
                 en("剪下", "Ctrl+X", "cut", sel), en("複製", "Ctrl+C", "copy", sel), en("貼上", "Ctrl+V", "paste", true), en("刪除", "Del", "delete", sel), en("全選", "Ctrl+A", "selectall", d.buf.len() > 0), sep(),
                 en("轉成大寫", "", "upper", sel), en("轉成小寫", "", "lower", sel), en("複製目前行", "Ctrl+D", "dupline", true), en("刪除目前行", "", "delline", true), sep(),
                 en("註解 / 取消註解", "Ctrl+Q", "comment", comment_syntax(&d.lang).is_some()), en("搜尋選取的文字", "", "searchsel", sel), sep(),
                 mi("切換書籤", "Ctrl+F2", "bookmark"), mi("全部摺疊", "", "foldall"), mi("全部展開", "", "unfoldall"),
                 chk(if st.split { "關閉分割檢視" } else { "複製到另一個檢視 (分割)" }, "", "split", st.split), sep(),
                 chk(if st.preview { "關閉預覽" } else { "開啟預覽" }, "", "preview", false)]).map(|mut m| { if m.action == "preview" { m.enabled = pv; } m }).collect()
        };
        ui.set_ctx_items(ModelRc::new(VecModel::from(items)));
        ui.set_ctx_visible(true);
    }

    pub(super) fn ctx_action(self: &Rc<Self>, a: &str) {
        let ui = self.ui();
        let i = self.st.borrow().ctx_tab.min(self.st.borrow().docs.len() - 1);
        let n = self.st.borrow().docs.len();
        match a {
            "close" => { self.close(i); }
            "closeothers" => { for j in (0..n).rev() { if j != i && !self.close(j) { break; } } }
            "closeright" => { for j in (i + 1..n).rev() { if !self.close(j) { break; } } }
            "closeleft" => { for j in (0..i).rev() { if !self.close(j) { break; } } }
            "save" | "saveas" => { self.save(i, a == "saveas"); }
            "rename" => { let t = self.st.borrow().docs[i].title.clone(); ui.set_rename_text(t.into()); ui.set_rename_visible(true); }
            "copypath" | "copyname" => {
                let st = self.st.borrow(); let d = &st.docs[i];
                let t = if a == "copypath" { d.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default() } else { d.title.clone() };
                drop(st);
                self.clipboard_set(t);
            }
            "openfolder" => { if let Some(p) = self.st.borrow().docs[i].path.as_ref().and_then(|p| p.parent()) { open_external(p); } }
            _ => {}
        }
    }

    pub(super) fn rename(&self, name: String) {
        let name = name.trim().to_string();
        let ui = self.ui();
        if name.is_empty() || name.contains(['/', '\\']) { return; }
        let i = self.st.borrow().ctx_tab.min(self.st.borrow().docs.len() - 1);
        let path = self.st.borrow().docs[i].path.clone();
        if let Some(old) = path {
            let new = old.with_file_name(&name);
            if new.exists() { msg(&format!("檔案已存在：{}", new.display())); return; }
            if let Err(e) = std::fs::rename(&old, &new) { msg(&format!("重新命名失敗：{}", e)); return; }
            let mut st = self.st.borrow_mut();
            st.docs[i].lang = lang_from_path(&new);
            st.docs[i].path = Some(new.clone());
            st.cfg.recent.retain(|x| x != &old);
            st.cfg.add_recent(new);
        }
        self.st.borrow_mut().docs[i].title = name;
        ui.set_rename_visible(false);
        self.refresh_meta();
    }

    pub(super) fn toggle_comment(&self) {
        let (pre, post) = { let st = self.st.borrow(); match comment_syntax(&st.docs[self.cd()].lang) { Some(c) => c, None => return } };
        let t = self.text();
        let (s, e) = self.selection();
        let ls = t[..s.min(t.len())].rfind('\n').map(|x| x + 1).unwrap_or(0);
        let e2 = if e > s && t[..e].ends_with('\n') { e - 1 } else { e };
        let le = t[e2.min(t.len())..].find('\n').map(|x| x + e2).unwrap_or(t.len());
        let block = &t[ls..le];
        let new = toggle_comment_lines(block, pre, post);
        let nt = format!("{}{}{}", &t[..ls], new, &t[le..]);
        self.set_text(nt);
        self.select_bytes(ls, ls + new.len());
    }

    /// Index where a tab dragged by `dx` pixels from its original place would land.
    pub(super) fn tab_target(&self, from: usize, dx: f32) -> i32 {
        let st = self.st.borrow();
        let ws: Vec<f32> = st.docs.iter().enumerate().map(|(i, d)| tab_width(&d.title, i == st.cur) + 1.0).collect();
        if from >= ws.len() { return -1; }
        let left: f32 = ws[..from].iter().sum();
        let center = left + ws[from] / 2.0 + dx;
        let mut x = 0.0;
        for (i, w) in ws.iter().enumerate() {
            if center < x + w { return i as i32; }
            x += w;
        }
        (ws.len() - 1) as i32
    }

    pub(super) fn move_tab(&self, from: usize, to: usize) {
        {
            let mut st = self.st.borrow_mut();
            if from >= st.docs.len() || to >= st.docs.len() || from == to { return; }
            st.store_view0();
            let d = st.docs.remove(from);
            st.docs.insert(to, d);
            let v1 = st.views[1].doc;
            st.views[1].doc = if v1 == from { to } else if from < v1 && v1 <= to { v1 - 1 } else if to <= v1 && v1 < from { v1 + 1 } else { v1 };
            st.cur = to;
            st.load_view0();
        }
        self.refresh_meta();
        self.render_all();
    }

    pub(super) fn action(self: &Rc<Self>, a: &str) {
        let ui = self.ui();
        if !a.starts_with("macro") && !matches!(a, "none" | "open" | "save" | "saveas" | "exit" | "about" | "find" | "replace" | "goto") { self.record(format!("a:{a}")); }
        let cur = self.cd();
        if self.is_hex(self.av()) {
            let k = match a { "undo" => "z", "redo" => "y", "copy" => "c", "cut" => "c", "paste" => "v", "selectall" => "a", _ => "" };
            if !k.is_empty() { self.hex_key(self.av(), k, true, false, false); self.refresh_status(); return; }
            if matches!(a, "upper" | "lower" | "trimtrail" | "sortlines" | "dupline" | "delline" | "comment" | "find" | "replace" | "findnext" | "findprev" | "goto" | "datetime") || a.starts_with("xf:") || a.starts_with("bmlines:") {
                msg("十六進位模式下不支援此功能，請先切換回文字模式 (Alt+H)。"); return;
            }
        }
        match a {
            "ai" => { ui.set_ai_visible(!ui.get_ai_visible()); }
            "ai-settings" => { ui.set_ai_visible(true); ui.set_ai_page(2); }
            "new" => { self.remember_cursor(); { let mut st = self.st.borrow_mut(); st.counter += 1; let n = st.counter; st.docs.push(Doc::new(n)); st.cur = st.docs.len() - 1; } self.refresh_all(); }
            "open" => {
                if let Some(files) = crate::dlg::file().set_title("開啟舊檔")
                    .add_filter("所有檔案", &["*"]).add_filter("文字/程式碼", &["txt", "md", "html", "htm", "rs", "py", "js", "json", "xml", "css", "c", "cpp", "h", "toml", "ini", "log"])
                    .pick_files() { for f in files { self.open_path(f); } }
            }
            "reload" => { self.reload_doc(cur); }
            "save" => { self.save(cur, false); }
            "saveas" => { self.save(cur, true); }
            "saveall" => { let n = self.st.borrow().docs.len(); for i in 0..n { if self.st.borrow().docs[i].modified { self.save(i, false); } } }
            "close" => { self.close(cur); }
            "closeall" => { while self.st.borrow().docs.len() > 1 || self.st.borrow().docs[0].modified || self.st.borrow().docs[0].path.is_some() { let i = self.st.borrow().docs.len() - 1; if !self.close(i) { break; } } }
            "closeothers" => { let mut i = self.st.borrow().docs.len(); while i > 0 { i -= 1; if i != self.st.borrow().cur { if !self.close(i) { break; } } } }
            "exit" => { if self.confirm_exit() { let _ = slint::quit_event_loop(); } }
            "undo" | "redo" | "gotobrace" | "print" | "exporthtml" | "bookmark" | "nextbm" | "prevbm" | "clearbm" | "zoomin" | "zoomout" => self.action_plain(a),
            "split" => self.toggle_split(),
            "openworkspace" => self.side_open_folder(),
            "side:1" => { let m = if self.st.borrow().side_mode == 1 { 0 } else { 1 }; self.side_switch(m); }
            "side:2" => { let m = if self.st.borrow().side_mode == 2 { 0 } else { 2 }; self.side_switch(m); }
            "compare" => self.compare(),
            "recompare" => { self.recompare(); self.render_all(); }
            "compareclear" => self.compare_clear(),
            "diffnext" => self.diff_nav(true, false),
            "diffprev" => self.diff_nav(false, false),
            "editconfig" => self.edit_user_config(),
            "installshell" | "uninstallshell" => {
                let r = if a == "installshell" {
                    let exe = std::env::current_exe().map(|e| file_io::canon(&e).unwrap_or(e)).unwrap_or_default();
                    shell::install_shell(&exe)
                } else { shell::uninstall_shell() };
                match r { Ok(m) => msg(&m), Err(e) => msg(&format!("失敗：{e}")) }
            }
            "otherview" => { let split = self.st.borrow().split; if split { let o = 1 - self.av(); self.st.borrow_mut().active = o; self.refresh_meta(); self.render_all(); ui.invoke_focus_editor(); } }
            "docmap" => { { let mut st = self.st.borrow_mut(); st.docmap = !st.docmap; } self.refresh_meta(); self.persist(); }
            "typewriter" => { { let mut st = self.st.borrow_mut(); st.cfg.typewriter = !st.cfg.typewriter; for v in st.views.iter_mut() { v.follow = true; } } self.persist(); self.refresh_meta(); self.render_all(); }
            "autocomplete" => { { let mut st = self.st.borrow_mut(); st.autocomplete = !st.autocomplete; } self.refresh_meta(); self.persist(); }
            "foldall" => self.fold_all(true),
            "unfoldall" => self.fold_all(false),
            "foldcur" => {
                let vi = self.av();
                let line = { let st = self.st.borrow(); let v = &st.views[vi]; let r = &st.docs[v.doc].buf.rope; r.char_to_line(v.sels[v.primary.min(v.sels.len() - 1)].head.min(r.len_chars())) };
                self.toggle_fold(vi, line);
            }
            "findall" => { ui.set_find_visible(true); if !ui.get_find_text().is_empty() { self.find_all(); } }
            "dark" => { { let mut st = self.st.borrow_mut(); st.dark = !st.dark; } self.apply_theme(); self.refresh_meta(); self.persist(); }
            "restoresession" => { { let mut st = self.st.borrow_mut(); st.cfg.restore_session = !st.cfg.restore_session; } self.refresh_meta(); self.persist(); }
            x if x.starts_with("ctx:") => self.ctx_action(&x[4..]),
            "delete" => { let vi = self.av(); let edits: Vec<_> = self.st.borrow().views[vi].sels.iter().map(|s| (s.start(), s.end(), String::new())).collect(); self.apply_edits(vi, edits, EditKind::Other, true); }
            "comment" => self.toggle_comment(),
            "searchsel" => {
                let (s, e) = self.selection();
                let t = self.text().get(s..e).unwrap_or("").to_string();
                if !t.is_empty() { ui.set_find_text(t.into()); ui.set_use_regex(false); ui.set_find_visible(true); self.find(true); }
            }
            x if x.starts_with("font:") => {
                { let mut st = self.st.borrow_mut(); st.font_family = x[5..].to_string(); }
                self.mark_dirty();
                self.refresh_meta(); self.persist();
            }
            "openconfig" => { let d = settings::config_dir(); let _ = std::fs::create_dir_all(&d); open_external(&d); }
            "macrorec" => {
                { let mut st = self.st.borrow_mut(); st.macro_rec = !st.macro_rec; if st.macro_rec { st.macro_buf.clear(); } }
                self.refresh_meta();
            }
            "macroplay" | "macroplay10" => {
                if !self.st.borrow().macro_rec {
                    let n = if a == "macroplay" { 1 } else { 10 };
                    let me = self.clone();
                    slint::Timer::single_shot(std::time::Duration::from_millis(0), move || me.play_macro(n));
                    return;
                }
            }
            "macroclear" => { self.st.borrow_mut().macro_buf.clear(); self.refresh_meta(); }
            x if x.starts_with("recent:") => {
                let p = x[7..].parse::<usize>().ok().and_then(|i| self.st.borrow().cfg.recent.get(i).cloned());
                if let Some(p) = p { if p.exists() { self.open_path(p); } else { msg(&format!("找不到檔案：{}", p.display())); self.st.borrow_mut().cfg.recent.retain(|x| x != &p); self.refresh_meta(); } }
            }
            "clearrecent" => { self.st.borrow_mut().cfg.recent.clear(); self.refresh_meta(); }
            "cut" => self.copy(self.av(), true),
            x if x.starts_with("bmlines:") => self.bookmark_lines(&x[8..]),
            "copy" => self.copy(self.av(), false),
            "paste" => self.paste(self.av()),
            "selectall" => self.select_all(self.av()),
            "dupline" | "delline" => {
                let t = self.text();
                let b = self.sel_bytes().1.min(t.len());
                let ls = t[..b].rfind('\n').map(|x| x + 1).unwrap_or(0);
                let le = t[b..].find('\n').map(|x| x + b).unwrap_or(t.len());
                let line = &t[ls..le];
                let nt = if a == "dupline" { format!("{}\n{}{}", &t[..le], line, &t[le..]) }
                    else { let end = if le < t.len() { le + 1 } else { le }; let start = if end == le && ls > 0 { ls - 1 } else { ls }; format!("{}{}", &t[..start], &t[end..]) };
                self.set_text(nt);
            }
            "upper" | "lower" | "trimtrail" => {
                let t = self.text();
                let (s, e) = self.selection();
                let (s, e) = if e > s { (s, e) } else { (0, t.len()) };
                let seg = &t[s..e];
                let r = match a {
                    "upper" => seg.to_uppercase(),
                    "lower" => seg.to_lowercase(),
                    _ => seg.split('\n').map(|l| l.trim_end()).collect::<Vec<_>>().join("\n"),
                };
                self.set_text(format!("{}{}{}", &t[..s], r, &t[e..]));
                if e > s && e < t.len() { self.select_bytes(s, s + r.len()); }
            }
            x if x.starts_with("eol:") => {
                { let mut st = self.st.borrow_mut(); let d = &mut st.docs[cur]; d.eol = match &x[4..] { "crlf" => Eol::Crlf, "lf" => Eol::Lf, _ => Eol::Cr }; d.modified = true; }
                self.refresh_meta();
            }
            x if x.starts_with("enc:") => {
                { let mut st = self.st.borrow_mut(); let d = &mut st.docs[cur];
                  let (e, b) = match &x[4..] { "utf8" => (encoding_rs::UTF_8, false), "utf8bom" => (encoding_rs::UTF_8, true), "utf16le" => (encoding_rs::UTF_16LE, true),
                      "utf16be" => (encoding_rs::UTF_16BE, true), "big5" => (encoding_rs::BIG5, false), "gbk" => (encoding_rs::GBK, false),
                      "sjis" => (encoding_rs::SHIFT_JIS, false), _ => (encoding_rs::WINDOWS_1252, false) };
                  d.encoding = e; d.bom = b; d.modified = true; }
                self.refresh_meta();
            }
            x if x.starts_with("lang:") => { { let mut st = self.st.borrow_mut(); st.docs[cur].lang = x[5..].to_string(); } self.refresh_meta(); self.refresh_preview(); }
            x if x.starts_with("tab:") => { if let Ok(i) = x[4..].parse() { self.select_tab(i); } }
            "nexttab" | "prevtab" => { let n = self.st.borrow().docs.len(); let i = if a == "nexttab" { (cur + 1) % n } else { (cur + n - 1) % n }; self.select_tab(i); }
            "find" | "replace" => { ui.set_replace_mode(a == "replace"); ui.set_find_visible(true); ui.set_find_status("".into()); }
            "findnext" => { self.find(true); }
            "findprev" => { self.find(false); }
            "goto" => ui.set_goto_visible(true),
            "wrap" => { { let mut st = self.st.borrow_mut(); st.wrap = !st.wrap; } self.refresh_meta(); }
            "linenumbers" => { { let mut st = self.st.borrow_mut(); st.line_numbers = !st.line_numbers; } self.refresh_meta(); }
            "preview" => { { let mut st = self.st.borrow_mut(); st.preview = !st.preview; } self.refresh_meta(); self.refresh_preview(); }
            "zoomreset" => { { let mut st = self.st.borrow_mut(); st.font = 14.0; } self.refresh_meta(); }
            "toggleovr" => { { let mut st = self.st.borrow_mut(); st.overwrite = !st.overwrite; } self.refresh_meta(); }
            "stats" => { self.mad_action("wordcount"); }
            "datetime" => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let stamp = format_unix(now);
                self.insert_text(self.av(), &stamp);
            }
            "runbrowser" | "openfolder" => {
                let p = self.st.borrow().docs[cur].path.clone();
                if let Some(p) = p {
                    let target = if a == "openfolder" { p.parent().map(|x| x.to_path_buf()).unwrap_or(p) } else { p };
                    open_external(&target);
                } else { msg("請先儲存檔案。"); }
            }
            "about" => ui.set_about_visible(true),
            x => { self.mad_action(x); }
        }
    }

    pub(super) fn confirm_exit(&self) -> bool {
        self.persist();
        loop {
            let idx = self.st.borrow().docs.iter().position(|d| d.modified);
            match idx {
                Some(i) => { self.st.borrow_mut().cur = i; self.refresh_all(); if !self.close(i) { return false; } }
                None => {
                    if let Err(e) = recovery::clear(&settings::config_dir()) {
                        eprintln!("NotEdit: failed to clear crash-recovery journal: {e}");
                    }
                    return true;
                }
            }
        }
    }
}
