//! Former Slint callbacks with logic of their own (menu dispatch, AI assistant,
//! goto/count) and the window-level shortcut table that lived in app.slint.
use super::*;

impl App {
    /// Close open menus first and run the action after the next frame, so a modal
    /// dialog opened by the action never shows a stale menu behind it. Without an
    /// open menu the action still runs from a 0 ms timer, outside the platform
    /// event handler (rfd modals must not run inside it).
    pub(super) fn menu_action(self: &Rc<Self>, s: &str) {
        let ui = self.ui();
        let popup = ui.get_open_menu() >= 0 || ui.get_ctx_visible();
        ui.set_open_menu(-1);
        ui.set_ctx_visible(false);
        let w = Rc::downgrade(self);
        let s = s.to_string();
        Timer::single_shot(Duration::from_millis(if popup { 30 } else { 0 }), move || { if let Some(a) = w.upgrade() { a.action(&s) } });
    }

    /// Push buttons of the dialogs and the AI panel.
    pub(super) fn ui_button(self: &Rc<Self>, id: &str) {
        let ui = self.ui();
        match id {
            "find_next" => { self.find(true); }
            "find_prev" => { self.find(false); }
            "count" => self.count_matches(),
            "find_all" => self.find_all(),
            "replace_one" => self.replace(false),
            "replace_all" => self.replace(true),
            "fif_browse" => self.fif_browse(),
            "fif_run" => self.fif_run(),
            "fif_replace_all" => self.fif_replace(),
            "fif_undo" => self.fif_replace_undo(),
            "fif_stop" => self.fif_stop(),
            "prompt_ok" => { ui.set_prompt_visible(false); let t = ui.get_prompt_text(); self.prompt_ok(&t); }
            "goto_ok" => { let t = ui.get_goto_text(); self.goto_line(&t); }
            "about_ok" => { ui.set_about_visible(false); ui.invoke_focus_editor(); }
            "rename_ok" => { let t = ui.get_rename_text(); self.rename(t); }
            "ai_send" => self.ai_send(),
            "ai_clear" => self.ai_clear_history(),
            "ai_apply" => self.ai_apply(false),
            "ai_apply_new" => self.ai_apply(true),
            "ai_save_key" => self.ai_save_key(),
            "ai_use_key" => self.ai_use_key(),
            "ai_clear_key" => self.ai_clear_key(),
            _ => {}
        }
    }

    /// Esc outside the editor's own handling: close every panel/dialog/popup.
    pub(super) fn escape_all(&self) {
        let ui = self.ui();
        ui.set_fif_visible(false); ui.set_prompt_visible(false); ui.set_find_visible(false); ui.set_goto_visible(false);
        ui.set_hits_visible(false); ui.set_about_visible(false); ui.set_open_menu(-1); ui.set_ctx_visible(false); ui.set_rename_visible(false);
        ui.invoke_focus_editor();
    }

    /// Window-level shortcuts (the `keys` FocusScope of app.slint); runs after the
    /// editor rejected the key. Returns true when handled.
    pub(super) fn global_key(self: &Rc<Self>, t: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        if self.custom_key(t, ctrl, shift, alt) { return true; }
        let ui = self.ui();
        let act = |a: &str| { self.menu_action(a); true };
        if ctrl && !alt {
            if (t == "r" || t == "R") && shift { return act("macrorec"); }
            if (t == "p" || t == "P") && shift { return act("macroplay"); }
            if (t == "s" || t == "S") && shift { return act("saveas"); }
            if (t == "f" || t == "F") && shift { return act("fif"); }
            match t {
                "s" => return act("save"), "o" => return act("open"), "n" => return act("new"), "w" => return act("close"),
                "f" => return act("find"), "h" => return act("replace"), "g" => return act("goto"), "z" => return act("undo"),
                "y" => return act("redo"), "d" => return act("dupline"), "q" => return act("comment"),
                "+" | "=" => return act("zoomin"), "-" => return act("zoomout"), "0" => return act("zoomreset"),
                "\t" => return act("nexttab"),
                _ => {}
            }
        }
        if ctrl && alt && (t == "f" || t == "F") { return act("foldcur"); }
        if alt && !ctrl && (t == "0" || t == ")") { return act(if shift { "unfoldall" } else { "foldall" }); }
        if alt && !ctrl && (t == "h" || t == "H") { return act(if ui.get_hex_mode() { "textmode" } else { "hexmode" }); }
        if alt && !ctrl && (t == "c" || t == "C") { return act("columnmode"); }
        match t {
            "\u{F70B}" => return act("otherview"),
            "\u{F70A}" => return act(if shift { "diffprev" } else { "diffnext" }),
            "\u{F706}" => { self.find(!shift); return true; }
            "\u{F727}" => return act("toggleovr"),
            "\u{1b}" => { self.escape_all(); return true; }
            _ => {}
        }
        false
    }

    pub(super) fn count_matches(&self) {
        if let Some(re) = self.regex() { let c = re.find_iter(&self.text()).count(); self.ui().set_find_status(format!("計數：{} 個符合項目", c)); }
    }

    pub(super) fn goto_line(&self, s: &str) {
        let ui = self.ui();
        if let Ok(n) = s.trim().parse::<usize>() {
            let off = { let st = self.st.borrow(); let r = &st.docs[self.cd()].buf.rope; r.line_to_byte(n.saturating_sub(1).min(r.len_lines() - 1)) };
            ui.set_goto_visible(false);
            self.select_bytes(off, off);
            ui.invoke_focus_editor();
        }
    }

    /// Files dropped from the OS (deferred: never re-enter from the event handler).
    pub(super) fn open_dropped(self: &Rc<Self>, paths: Vec<PathBuf>) {
        let a = self.clone();
        Timer::single_shot(Duration::from_millis(0), move || {
            for f in paths { if f.is_file() { a.open_path(f); } }
        });
    }

    // ------------------------------------------------------------------ AI assistant

    pub(super) fn ai_save_key(&self) {
        let ui = self.ui();
        let key = ui.get_ai_key();
        if key.trim().is_empty() { ui.set_ai_key_status("請先輸入金鑰".into()); return; }
        match credentials::save(key.trim()) {
            Ok(()) => { *self.ai_key.borrow_mut() = key.trim().to_string(); ui.set_ai_key_status("已加密儲存金鑰".into()); }
            Err(e) => ui.set_ai_key_status(format!("儲存金鑰失敗：{e}")),
        }
    }

    pub(super) fn ai_use_key(&self) {
        let ui = self.ui();
        let key = ui.get_ai_key();
        if key.trim().is_empty() { ui.set_ai_key_status("請先輸入金鑰".into()); return; }
        *self.ai_key.borrow_mut() = key.trim().to_string();
        ui.set_ai_key_status("金鑰僅用於本次執行，未變更已儲存金鑰".into());
    }

    pub(super) fn ai_clear_key(&self) {
        let ui = self.ui();
        match credentials::clear() {
            Ok(()) => { self.ai_key.borrow_mut().clear(); ui.set_ai_key(String::new()); ui.set_ai_key_status("已刪除儲存的金鑰".into()); }
            Err(e) => ui.set_ai_key_status(format!("刪除金鑰失敗：{e}")),
        }
    }

    pub(super) fn ai_clear_history(&self) {
        let ui = self.ui();
        ui.set_ai_history(String::new()); ui.set_ai_result(String::new()); ui.set_ai_status("已清除對話".into()); ui.set_ai_has_source(false);
    }

    pub(super) fn ai_send(&self) {
        let ui = self.ui();
        if ui.get_ai_running() { return; }
        let key = self.ai_key.borrow().clone();
        let model = ui.get_ai_model().trim().to_string();
        let prompt = ui.get_ai_prompt();
        let (role, role_name) = match ui.get_ai_role() {
            1 => (agent::Role::Proofread, "校稿"),
            2 => {
                let language = match ui.get_ai_language() {
                    0 => "繁體中文（台灣）".to_string(), 1 => "簡體中文".into(), 2 => "英文".into(), 3 => "日文".into(),
                    4 => "韓文".into(), 5 => "越南文".into(), 6 => "泰文".into(), 7 => "印尼文".into(),
                    8 => "法文".into(), 9 => "德文".into(), 10 => "西班牙文".into(),
                    _ => ui.get_ai_language_custom().trim().to_string(),
                };
                if language.is_empty() { ui.set_ai_status("請輸入自訂翻譯語言".into()); return; }
                (agent::Role::Translate(language), "翻譯")
            }
            3 => (agent::Role::Polish, "文字潤飾"), 4 => (agent::Role::Summarize, "摘要"),
            5 => (agent::Role::Format, "格式整理"), _ => (agent::Role::General, "通用助手"),
        };
        let history = ui.get_ai_history();
        let mode = ui.get_ai_mode();
        let create_path = if mode == 1 {
            let Some(path) = crate::dlg::file().set_title("AI 自動建立新檔（不覆蓋既有檔案）").add_filter("文件", &["md", "json", "txt"]).save_file() else { return; };
            if path.exists() { ui.set_ai_status("檔案已存在，請選擇新檔名".into()); return; }
            Some(path)
        } else { None };
        let document = (ui.get_ai_include_document() || mode == 2).then(|| self.text());
        let target = if mode == 2 {
            let st = self.st.borrow();
            let d = &st.docs[st.views[st.view_idx()].doc];
            let Some(path) = d.path.clone() else { ui.set_ai_status("自動修改前請先儲存目前文件".into()); return; };
            if d.hex.is_some() { ui.set_ai_status("自動修改僅支援文字文件".into()); return; }
            if file_watch::stamp(&path).ok() != d.disk_stamp { ui.set_ai_status("磁碟檔案已變更，請先重新載入後再執行".into()); return; }
            let disk = match std::fs::read(&path) { Ok(bytes) => bytes, Err(e) => { ui.set_ai_status(format!("無法讀取原檔：{e}")); return; } };
            let buffer = match encode(d) { Ok(bytes) => bytes, Err(e) => { ui.set_ai_status(e); return; } };
            Some((path, disk, buffer))
        } else { None };
        let title = { let st = self.st.borrow(); st.docs[st.views[st.view_idx()].doc].title.clone() };
        ui.set_ai_running(true);
        ui.set_ai_status("正在請求 OpenRouter...".into());
        std::thread::spawn(move || {
            let result = agent::request(&key, &model, &prompt, &history, document.as_deref(), role);
            crate::ui::dispatch::invoke_from_event_loop(move || APP.with(|slot| {
                let Some(app) = slot.borrow().clone() else { return };
                let ui = app.ui();
                ui.set_ai_running(false);
                match result {
                    Ok(text) => {
                        ui.set_ai_history(format!("{history}\n使用者（{role_name}）：{prompt}\n助手：{text}\n"));
                        ui.set_ai_result(text);
                        ui.set_ai_page(1);
                        ui.set_ai_has_source(document.is_some());
                        ui.set_ai_source(document.clone().unwrap_or_default());
                        ui.set_ai_source_title(title);
                        ui.set_ai_status("完成：請檢查內容後套用".into());
                        let result_text = ui.get_ai_result();
                        if let Some(path) = create_path {
                            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) && serde_json::from_str::<serde_json::Value>(&result_text).is_err() {
                                ui.set_ai_status("模型輸出的 JSON 無效，未建立檔案；請檢查預覽".into()); return;
                            }
                            match ai_files::create(&path, result_text.as_bytes()) {
                                Ok(()) => { app.open_path(path.clone()); ui.set_ai_status(format!("已建立：{}", path.display())); }
                                Err(e) => ui.set_ai_status(format!("建立檔案失敗：{e}")),
                            }
                        } else if let Some((path, disk, buffer)) = target {
                            let current = { let st = app.st.borrow(); let d = &st.docs[st.views[st.view_idx()].doc]; d.path.as_ref() == Some(&path) && document.as_deref() == Some(d.text().as_str()) };
                            if !current || std::fs::read(&path).ok().as_deref() != Some(disk.as_slice()) {
                                ui.set_ai_status("文件已變更或切換，未自動修改；請重新送出".into()); return;
                            }
                            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) && serde_json::from_str::<serde_json::Value>(&result_text).is_err() {
                                ui.set_ai_status("模型輸出的 JSON 無效，未修改檔案；請檢查預覽".into()); return;
                            }
                            match ai_files::backup(&path, &disk, &buffer) {
                                Ok((disk_backup, buffer_backup)) => {
                                    if std::fs::read(&path).ok().as_deref() != Some(disk.as_slice()) {
                                        ui.set_ai_status(format!("備份期間原檔已變更，未修改；備份：{}", disk_backup.display())); return;
                                    }
                                    app.set_text(result_text);
                                    let idx = app.cd();
                                    if app.save(idx, false) { ui.set_ai_status(format!("已修改並儲存；備份：{}、{}", disk_backup.display(), buffer_backup.display())); }
                                    else { ui.set_ai_status(format!("儲存失敗；原檔及備份保留於 {}", disk_backup.display())); }
                                }
                                Err(e) => ui.set_ai_status(format!("備份失敗，未修改文件：{e}")),
                            }
                        }
                    }
                    Err(e) => ui.set_ai_status(e),
                }
            }));
        });
    }

    pub(super) fn ai_apply(self: &Rc<Self>, new_doc: bool) {
        let ui = self.ui();
        if !new_doc && ui.get_ai_has_source() {
            let title = { let st = self.st.borrow(); st.docs[st.views[st.view_idx()].doc].title.clone() };
            if title != ui.get_ai_source_title() || self.text() != ui.get_ai_source() {
                ui.set_ai_status("文件已變更或切換，請重新送出以避免覆蓋變更".into()); return;
            }
        }
        if !new_doc && self.is_hex(self.av()) { ui.set_ai_status("請先切換回文字模式".into()); return; }
        if new_doc { self.action("new"); self.st.borrow_mut().active = 0; }
        self.set_text(ui.get_ai_result());
        ui.set_ai_status("已套用，可使用 Ctrl+Z 復原；請儲存文件".into());
    }
}
