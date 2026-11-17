//! Application behavior separated from the startup and state definitions.
use super::*;

impl App {
    pub(super) fn ui(&self) -> AppWindow { self.ui.clone() }

    pub(super) fn refresh_all(&self) {
        { let mut st = self.st.borrow_mut(); st.load_view0(); }
        self.refresh_meta();
        self.side_refresh();
        self.refresh_preview();
        self.render_all();
        self.ui().invoke_focus_editor();
    }

    pub(super) fn remember_cursor(&self) { self.st.borrow_mut().store_view0(); }

    pub(super) fn persist(&self) {
        self.remember_cursor();
        let mut st = self.st.borrow_mut();
        st.cfg.font = st.font; st.cfg.font_family = st.font_family.clone(); st.cfg.wrap = st.wrap; st.cfg.line_numbers = st.line_numbers; st.cfg.dark = st.dark; st.cfg.preview = st.preview;
        st.cfg.macro_keys = st.macro_buf.clone();
        st.cfg.docmap = st.docmap; st.cfg.autocomplete = st.autocomplete;
        st.cfg.sidebar = st.side_mode; st.cfg.workspace = st.side_root.clone(); st.cfg.workspace_expanded = st.side_expanded.iter().cloned().collect();
        st.cfg.session = st.docs.iter().filter_map(|d| d.path.clone().map(|p| settings::SessionFile { path: p, cursor: d.cursor })).collect();
        st.cfg.session_cur = st.docs[..st.cur].iter().filter(|d| d.path.is_some()).count();
        if let Err(e) = st.cfg.save() {
            eprintln!("NotEdit: failed to save settings: {e}");
        }
    }

    pub(super) fn recovery_session(&self) -> recovery::RecoverySession {
        let st = self.st.borrow();
        let docs = st.docs.iter().filter(|d| d.modified && d.hex.is_none()).map(|d| {
            let disk_len = d.disk_stamp.as_ref().map(|s| s.len);
            let disk_modified_ns = d.disk_stamp.as_ref()
                .and_then(|s| s.modified.as_ref())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .and_then(|d| u64::try_from(d.as_nanos()).ok());
            recovery::RecoveryDoc {
                path: d.path.clone(),
                title: d.title.clone(),
                text: d.text(),
                eol: match d.eol { Eol::Crlf => "crlf", Eol::Lf => "lf", Eol::Cr => "cr" }.into(),
                encoding: d.encoding.name().to_string(),
                bom: d.bom,
                lang: d.lang.clone(),
                cursor: d.cursor,
                disk_len,
                disk_modified_ns,
            }
        }).collect();
        recovery::RecoverySession::new(docs)
    }

    pub(super) fn persist_recovery(&self) {
        let session = self.recovery_session();
        if let Err(e) = recovery::save(&settings::config_dir(), &session) {
            eprintln!("NotEdit: failed to save crash-recovery journal: {e}");
        }
    }

    pub(super) fn restore_recovery(&self) {
        let dir = settings::config_dir();
        let session = match recovery::load(&dir) {
            Ok(Some(session)) if !session.docs.is_empty() => session,
            Ok(_) => return,
            Err(e) => {
                eprintln!("NotEdit: failed to read crash-recovery journal: {e}");
                return;
            }
        };

        let answer = crate::dlg::msg()
            .set_title("NotEdit")
            .set_description(format!("偵測到上次未正常關閉留下的 {} 個未儲存文件。\n\n是否復原？復原內容只會先放回編輯器，不會自動覆寫原檔。", session.docs.len()))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if answer != rfd::MessageDialogResult::Yes {
            if let Err(e) = recovery::clear(&dir) {
                eprintln!("NotEdit: failed to discard crash-recovery journal: {e}");
            }
            return;
        }

        let mut st = self.st.borrow_mut();
        for rd in session.docs {
            let normalized_path = rd.path.map(|p| crate::file_io::canon(&p).unwrap_or(p));
            let mut d = Doc::new(0);
            d.path = normalized_path.clone();
            // Keep the stamp from the moment the recovery snapshot was made.
            // If the file changed while NotEdit was not running, the watcher
            // will compare this old stamp with disk and surface a conflict.
            d.disk_stamp = rd.disk_len.map(|len| file_watch::FileStamp {
                len,
                modified: rd.disk_modified_ns.map(|ns| UNIX_EPOCH + Duration::from_nanos(ns)),
            });
            d.title = rd.title;
            d.eol = match rd.eol.as_str() { "crlf" => Eol::Crlf, "cr" => Eol::Cr, _ => Eol::Lf };
            d.encoding = match rd.encoding.as_str() {
                "UTF-16LE" => encoding_rs::UTF_16LE,
                "UTF-16BE" => encoding_rs::UTF_16BE,
                _ => encoding_rs::Encoding::for_label(rd.encoding.as_bytes()).unwrap_or(encoding_rs::UTF_8),
            };
            d.bom = rd.bom;
            d.lang = rd.lang;
            d.cursor = rd.cursor;
            d.set_loaded(&rd.text);
            d.modified = true;
            d.saved = None;

            if let Some(i) = normalized_path.as_ref().and_then(|p| st.docs.iter().position(|x| x.path.as_ref() == Some(p))) {
                st.docs[i] = d;
                st.cur = i;
            } else {
                let replace_blank = st.docs.len() == 1
                    && st.docs[0].path.is_none()
                    && !st.docs[0].modified
                    && st.docs[0].buf.len() == 0;
                if replace_blank {
                    st.docs[0] = d;
                    st.cur = 0;
                } else {
                    st.docs.push(d);
                    st.cur = st.docs.len() - 1;
                }
            }
        }
    }

    pub(super) fn refresh_meta(&self) {
        let ui = self.ui();
        let st = self.st.borrow();
        let shown = st.views[st.view_idx()].doc.min(st.docs.len() - 1);
        let d = &st.docs[shown];
        let tabs: Vec<TabInfo> = st.docs.iter().enumerate().map(|(i, d)| TabInfo { title: d.title.clone(), modified: d.modified, w: tab_width(&d.title, i == shown) }).collect();
        ui.set_tabs(tabs);
        ui.set_current_tab(shown as i32);
        ui.set_menus(build_menus(&st));
        let name = d.path.as_ref().map(|p| p.display().to_string()).unwrap_or(d.title.clone());
        ui.set_window_title(format!("{}{} - NotEdit", if d.modified { "*" } else { "" }, name));
        ui.set_split(st.split);
        ui.set_docmap(st.docmap);
        ui.set_active_view(st.view_idx() as i32);
        ui.set_editor_font_size(st.font);
        ui.set_line_spacing(st.cfg.line_spacing.clamp(1.0, 2.5));
        ui.set_show_preview(st.preview && matches!(d.lang.as_str(), "markdown" | "html" | "xml"));
        ui.set_overwrite(st.overwrite);
        ui.set_dark(st.dark);
        ui.set_mono(st.font_family.clone());
        ui.set_macro_recording(st.macro_rec);
        ui.set_st_type(lang_desc(&d.lang).into());
        ui.set_st_eol(d.eol.label().into());
        ui.set_st_encoding(d.encoding_label().into());
        let ad = &st.docs[st.views[st.view_idx()].doc];
        let lines = ad.buf.rope.len_lines();
        let display_len = ad.buf.len() + if ad.eol == Eol::Crlf { lines - 1 } else { 0 };
        match &ad.hex {
            Some(h) => ui.set_st_length(format!("bytes : {}    [HEX]", h.buf.len()).into()),
            None => ui.set_st_length(format!("length : {}    lines : {}{}", display_len, lines, if st.column_mode { "    [欄模式]" } else { "" }).into()),
        }
        ui.set_hex_mode(ad.hex.is_some());
        ui.set_total_lines(lines as i32);
    }

    pub(super) fn check_external_changes(&self) {
        let docs: Vec<_> = {
            let st = self.st.borrow();
            st.docs.iter().enumerate().filter_map(|(i, d)| {
                d.path.clone().filter(|_| d.hex.is_none()).map(|p| (i, p, d.disk_stamp.clone(), d.modified, d.title.clone()))
            }).collect()
        };

        for (idx, path, expected, modified, title) in docs {
            let current = match file_watch::stamp(&path) {
                Ok(stamp) => Some(stamp),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => continue,
            };
            if current == expected { continue; }

            if current.is_none() {
                msg(&format!("「{}」已被其他程式刪除。\n\nNotEdit 會保留目前記憶體中的內容並標記為未儲存。", title));
                {
                    let mut st = self.st.borrow_mut();
                    if let Some(d) = st.docs.get_mut(idx).filter(|d| d.path.as_ref() == Some(&path)) {
                        d.disk_stamp = None;
                        d.modified = true;
                    }
                }
                self.refresh_meta();
                self.persist_recovery();
                return;
            }

            let description = if modified {
                format!("「{}」已被其他程式修改。\n\nNotEdit 內也有未儲存變更。是否重新載入磁碟版本？\n選「否」會保留目前內容並標記為未儲存。", title)
            } else {
                format!("「{}」已被其他程式修改。\n\n是否重新載入磁碟上的新版本？", title)
            };
            let answer = crate::dlg::msg()
                .set_title("NotEdit")
                .set_description(description)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if answer == rfd::MessageDialogResult::Yes {
                self.reload_doc(idx);
            } else {
                {
                    let mut st = self.st.borrow_mut();
                    if let Some(d) = st.docs.get_mut(idx).filter(|d| d.path.as_ref() == Some(&path)) {
                        d.disk_stamp = current;
                        d.modified = true;
                    }
                }
                self.refresh_meta();
                self.persist_recovery();
            }
            return;
        }
    }

    pub(super) fn refresh_preview(&self) {
        self.preview_timer.stop();
        let ui = self.ui();
        let st = self.st.borrow();
        if !st.preview { return; }
        let d = &st.docs[st.cur];
        let blocks = match d.lang.as_str() {
            "markdown" => preview::markdown(&d.text()),
            "html" | "xml" => preview::html(&d.text()),
            _ => {
                let mut b = preview::markdown("*此檔案類型不支援預覽，請將語言設為 Markdown 或 HTML。*");
                b.truncate(1); b
            }
        };
        ui.set_preview_title(d.title.clone());
        ui.set_preview_blocks(blocks);
    }

    /// Command-line / forwarded argument: folders become the workspace.
    pub(super) fn open_arg(&self, p: PathBuf) {
        if p.is_dir() { self.set_workspace(p); self.persist(); } else { self.open_path(p); }
    }

    pub(super) fn open_path(&self, path: PathBuf) {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let already = { let cp = crate::file_io::canon(&path).unwrap_or(path.clone()); self.st.borrow().docs.iter().position(|d| d.path.as_ref() == Some(&cp)) };
        if let Some(i) = already { self.remember_cursor(); self.st.borrow_mut().cur = i; if !self.show_in_split_view() { self.refresh_all(); } return; }
        // binary or huge files open in hex mode (memory-mapped when large)
        let sample = { use std::io::Read; let mut b = vec![0u8; 8192]; std::fs::File::open(&path).and_then(|mut f| f.read(&mut b)).map(|n| { b.truncate(n); b }).unwrap_or_default() };
        if size > HUGE_TEXT || hex::looks_binary(&sample) {
            let buf = if size > 64 << 20 { hex::HexBuf::map(&path) } else { std::fs::read(&path).map(hex::HexBuf::from_vec) };
            match buf {
                Ok(b) => { self.open_hex(path, b); return; }
                Err(e) => { msg(&format!("無法開啟檔案：{}", e)); return; }
            }
        }
        let bytes = match std::fs::read(&path) { Ok(b) => b, Err(e) => { msg(&format!("無法開啟檔案：{}", e)); return; } };
        let (s, enc, bom) = decode(&bytes);
        let eol = detect_eol(&s);
        let text = s.replace("\r\n", "\n").replace('\r', "\n");
        self.remember_cursor();
        let path = crate::file_io::canon(&path).unwrap_or(path);
        {
            let mut st = self.st.borrow_mut();
            st.cfg.add_recent(path.clone());
            if let Some(i) = st.docs.iter().position(|d| d.path.as_ref() == Some(&path)) { st.cur = i; }
            else {
                let mut d = Doc::new(0);
                d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                d.lang = lang_from_path(&path);
                d.disk_stamp = file_watch::stamp(&path).ok();
                d.path = Some(path); d.set_loaded(&text); d.eol = eol; d.encoding = enc; d.bom = bom;
                let c = st.cur;
                if st.docs[c].path.is_none() && !st.docs[c].modified && st.docs[c].buf.len() == 0 { st.docs[c] = d; }
                else { st.docs.push(d); st.cur = st.docs.len() - 1; }
                if matches!(st.docs[st.cur].lang.as_str(), "markdown" | "html") { st.preview = true; }
            }
        }
        if self.show_in_split_view() { return; }
        self.refresh_all();
    }

    pub(super) fn open_hex(&self, path: PathBuf, buf: hex::HexBuf) {
        self.remember_cursor();
        let path = crate::file_io::canon(&path).unwrap_or(path);
        {
            let mut st = self.st.borrow_mut();
            st.cfg.add_recent(path.clone());
            let mut d = Doc::new(0);
            d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            d.disk_stamp = file_watch::stamp(&path).ok();
            d.path = Some(path);
            d.hex = Some(Box::new(hexview::HexState::new(buf)));
            let c = st.cur;
            if st.docs[c].path.is_none() && !st.docs[c].modified && st.docs[c].buf.len() == 0 && st.docs[c].hex.is_none() { st.docs[c] = d; }
            else { st.docs.push(d); st.cur = st.docs.len() - 1; }
        }
        self.refresh_all();
    }

    pub(super) fn reload_doc(&self, idx: usize) -> bool {
        let path = {
            let st = self.st.borrow();
            match st.docs.get(idx).and_then(|d| d.path.clone()) {
                Some(path) => path,
                None => return false,
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                msg(&format!("無法重新載入「{}」：{}", path.display(), e));
                return false;
            }
        };
        let (s, enc, bom) = decode(&bytes);
        let eol = detect_eol(&s);
        let text = s.replace("\r\n", "\n").replace('\r', "\n");
        let disk_stamp = file_watch::stamp(&path).ok();
        let is_current = {
            let mut st = self.st.borrow_mut();
            if idx >= st.docs.len() { return false; }
            let is_current = st.cur == idx;
            if is_current { st.store_view0(); }
            let d = &mut st.docs[idx];
            d.hex = None;
            d.set_loaded(&text);
            d.eol = eol;
            d.encoding = enc;
            d.bom = bom;
            d.modified = false;
            d.saved = Some(d.buf.rope.clone());
            d.hl = Default::default();
            d.disk_stamp = disk_stamp;
            for v in st.views.iter_mut().filter(|v| v.doc == idx) { v.sels = vec![Sel::caret(0)]; v.lkey.0 = usize::MAX; }
            is_current
        };
        if is_current { self.refresh_all(); } else { self.refresh_meta(); }
        self.persist_recovery();
        true
    }

    pub(super) fn save(&self, idx: usize, save_as: bool) -> bool {
        let path = {
            let st = self.st.borrow();
            let d = &st.docs[idx];
            match (&d.path, save_as) {
                (Some(p), false) => Some(p.clone()),
                _ => crate::dlg::file().set_title("另存新檔").set_file_name(&d.title).save_file(),
            }
        };
        let Some(path) = path else { return false };
        if self.st.borrow().docs[idx].hex.is_some() {
            let r = self.st.borrow_mut().docs[idx].hex.as_mut().unwrap().buf.save(&path);
            if let Err(e) = r { msg(&format!("儲存失敗：{}", e)); return false; }
        } else {
        let bytes = match encode(&self.st.borrow().docs[idx]) {
            Ok(bytes) => bytes,
            Err(e) => { msg(&e); return false; }
        };
        if let Err(e) = file_io::durable_write(&path, &bytes) { msg(&format!("儲存失敗：{}", e)); return false; }
        }
        let saved_path: Option<PathBuf>;
        {
            let mut st = self.st.borrow_mut();
            let d = &mut st.docs[idx];
            d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if d.path.as_ref() != Some(&path) { d.lang = lang_from_path(&path); }
            d.path = Some(path.clone());
            d.disk_stamp = file_watch::stamp(&path).ok();
            d.modified = false;
            d.saved = if d.hex.is_none() { Some(d.buf.rope.clone()) } else { None };
            st.cfg.add_recent(path.clone());
            saved_path = Some(path);
        }
        self.refresh_meta();
        self.refresh_preview();
        self.persist_recovery();
        if let Some(p) = saved_path { self.reload_user_config(&p); }
        true
    }

    /// returns false when cancelled
    pub(super) fn close(&self, idx: usize) -> bool {
        let (modified, title) = { let st = self.st.borrow(); (st.docs[idx].modified, st.docs[idx].title.clone()) };
        if modified {
            let r = crate::dlg::msg().set_title("NotEdit").set_description(format!("是否儲存「{}」的變更？", title))
                .set_buttons(rfd::MessageButtons::YesNoCancel).show();
            match r {
                rfd::MessageDialogResult::Yes => { if !self.save(idx, false) { return false; } }
                rfd::MessageDialogResult::No => {}
                _ => return false,
            }
        }
        self.remember_cursor();
        {
            let mut st = self.st.borrow_mut();
            st.docs.remove(idx);
            if st.split {
                if st.views[1].doc == idx { st.split = false; st.active = 0; }
                else if st.views[1].doc > idx { st.views[1].doc -= 1; }
            }
            if st.docs.is_empty() { st.counter += 1; let n = st.counter; st.docs.push(Doc::new(n)); }
            if st.cur >= st.docs.len() || st.cur > idx { st.cur = st.cur.saturating_sub(1).min(st.docs.len() - 1); }
        }
        self.refresh_all();
        self.persist_recovery();
        true
    }

    /// After open_path picked `st.cur`: when the split view is active, show that
    /// document there and give the main view back its previous document.
    pub(super) fn show_in_split_view(&self) -> bool {
        let (split, active, cur, v0) = { let st = self.st.borrow(); (st.split, st.active, st.cur, st.views[0].doc) };
        if !(split && active == 1) { return false; }
        { let mut st = self.st.borrow_mut(); st.cur = v0.min(st.docs.len() - 1); }
        self.select_tab(cur);
        true
    }

    pub(super) fn select_tab(&self, i: usize) {
        let (split, active) = { let st = self.st.borrow(); (st.split, st.active) };
        if split && active == 1 {
            // the split (right) view can show any document, independent of the main view
            {
                let mut st = self.st.borrow_mut();
                if i >= st.docs.len() { return; }
                let old = st.views[1].doc;
                if old != st.views[0].doc { let (sels, top) = (st.views[1].sels.clone(), st.views[1].top); if let Some(d) = st.docs.get_mut(old) { d.sels = sels; d.top = top; } }
                let d = &st.docs[i];
                let len = d.buf.len();
                let sels: Vec<Sel> = d.sels.iter().map(|s| Sel::new(s.anchor.min(len), s.head.min(len))).collect();
                let top = d.top;
                let v = &mut st.views[1];
                v.doc = i; v.sels = if sels.is_empty() { vec![Sel::caret(0)] } else { sels }; v.primary = 0; v.top = top;
                v.sx = 0.0; v.max_w = 0.0; v.ac = None; v.follow = true; v.drag = Drag::None; v.lkey = (usize::MAX, 0, 0, 0, false);
            }
            self.refresh_meta(); self.refresh_preview(); self.render_all(); self.side_refresh(); self.ui().invoke_focus_editor();
            return;
        }
        self.remember_cursor();
        { let mut st = self.st.borrow_mut(); if i >= st.docs.len() { return; } st.cur = i; }
        self.refresh_all();
    }

}
