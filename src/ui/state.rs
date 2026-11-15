//! `AppWindow`: the plain-Rust replacement for the Slint-generated window
//! component. It keeps every former `in`/`in-out` property behind the same
//! `set_x` / `get_x` names so the application code reads unchanged; any
//! change flags a redraw for the shell.
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use acme_textinput::TextInputState;

use super::hit::FieldId;
use super::types::*;

macro_rules! props {
    ($($name:ident, $get:ident, $set:ident : $t:ty = $def:expr;)*) => {
        struct Props { $($name: RefCell<$t>,)* }
        impl Default for Props { fn default() -> Self { Props { $($name: RefCell::new($def),)* } } }
        impl AppWindow {
            $(
                #[allow(dead_code)]
                pub fn $get(&self) -> $t { self.0.p.$name.borrow().clone() }
                #[allow(dead_code)]
                pub fn $set(&self, v: $t) {
                    let changed = *self.0.p.$name.borrow() != v;
                    if changed { *self.0.p.$name.borrow_mut() = v; self.0.dirty.set(true); }
                }
            )*
        }
    };
}

props! {
    window_title, get_window_title, set_window_title: String = "NotEdit".into();
    version, get_version, set_version: String = String::new();
    menus, get_menus, set_menus: Vec<Menu> = Vec::new();
    tabs, get_tabs, set_tabs: Vec<TabInfo> = Vec::new();
    current_tab, get_current_tab, set_current_tab: i32 = 0;
    editor_font_size, get_editor_font_size, set_editor_font_size: f32 = 14.0;
    show_preview, get_show_preview, set_show_preview: bool = false;
    preview_blocks, get_preview_blocks, set_preview_blocks: Vec<Block> = Vec::new();
    preview_title, get_preview_title, set_preview_title: String = String::new();
    st_type, get_st_type, set_st_type: String = "Normal text file".into();
    st_length, get_st_length, set_st_length: String = String::new();
    st_pos, get_st_pos, set_st_pos: String = String::new();
    st_eol, get_st_eol, set_st_eol: String = "Windows (CR LF)".into();
    st_encoding, get_st_encoding, set_st_encoding: String = "UTF-8".into();
    overwrite, get_overwrite, set_overwrite: bool = false;
    find_visible, get_find_visible, set_find_visible: bool = false;
    replace_mode, get_replace_mode, set_replace_mode: bool = false;
    goto_visible, get_goto_visible, set_goto_visible: bool = false;
    fif_visible, get_fif_visible, set_fif_visible: bool = false;
    fif_recursive, get_fif_recursive, set_fif_recursive: bool = true;
    fif_running, get_fif_running, set_fif_running: bool = false;
    fif_replace, get_fif_replace, set_fif_replace: bool = false;
    prompt_visible, get_prompt_visible, set_prompt_visible: bool = false;
    prompt_title, get_prompt_title, set_prompt_title: String = String::new();
    prompt_label, get_prompt_label, set_prompt_label: String = String::new();
    hex_mode, get_hex_mode, set_hex_mode: bool = false;
    about_visible, get_about_visible, set_about_visible: bool = false;
    find_status, get_find_status, set_find_status: String = String::new();
    match_case, get_match_case, set_match_case: bool = false;
    total_lines, get_total_lines, set_total_lines: i32 = 1;
    dark, get_dark, set_dark: bool = false;
    hits, get_hits, set_hits: Vec<SearchHit> = Vec::new();
    hits_title, get_hits_title, set_hits_title: String = String::new();
    hits_visible, get_hits_visible, set_hits_visible: bool = false;
    whole_word, get_whole_word, set_whole_word: bool = false;
    use_regex, get_use_regex, set_use_regex: bool = false;
    macro_recording, get_macro_recording, set_macro_recording: bool = false;
    ctx_items, get_ctx_items, set_ctx_items: Vec<MenuItem> = Vec::new();
    ctx_visible, get_ctx_visible, set_ctx_visible: bool = false;
    ctx_x, get_ctx_x, set_ctx_x: f32 = 0.0;
    ctx_y, get_ctx_y, set_ctx_y: f32 = 0.0;
    rename_visible, get_rename_visible, set_rename_visible: bool = false;
    mono, get_mono, set_mono: String = "monospace".into();
    side_mode, get_side_mode, set_side_mode: i32 = 0;
    side_items, get_side_items, set_side_items: Vec<SideItem> = Vec::new();
    side_title, get_side_title, set_side_title: String = String::new();
    side_sel, get_side_sel, set_side_sel: i32 = -1;
    v0, get_v0, set_v0: EdState = EdState::default();
    v1, get_v1, set_v1: EdState = EdState::default();
    split, get_split, set_split: bool = false;
    docmap, get_docmap, set_docmap: bool = false;
    active_view, get_active_view, set_active_view: i32 = 0;
    line_spacing, get_line_spacing, set_line_spacing: f32 = 1.4;
    caret_idle, get_caret_idle, set_caret_idle: bool = false;
    open_menu, get_open_menu, set_open_menu: i32 = -1;
    ai_visible, get_ai_visible, set_ai_visible: bool = false;
    ai_page, get_ai_page, set_ai_page: i32 = 0;
    ai_role, get_ai_role, set_ai_role: i32 = 0;
    ai_language, get_ai_language, set_ai_language: i32 = 0;
    ai_mode, get_ai_mode, set_ai_mode: i32 = 0;
    ai_key_status, get_ai_key_status, set_ai_key_status: String = String::new();
    ai_result, get_ai_result, set_ai_result: String = String::new();
    ai_status, get_ai_status, set_ai_status: String = String::new();
    ai_include_document, get_ai_include_document, set_ai_include_document: bool = false;
    ai_running, get_ai_running, set_ai_running: bool = false;
    ai_source, get_ai_source, set_ai_source: String = String::new();
    ai_source_title, get_ai_source_title, set_ai_source_title: String = String::new();
    ai_has_source, get_ai_has_source, set_ai_has_source: bool = false;
    ai_history, get_ai_history, set_ai_history: String = String::new();
    // ---- written by the shell (former `out` properties)
    cell_w, get_cell_w, set_cell_w: f32 = 8.0;
    line_h, get_line_h, set_line_h: f32 = 18.0;
    text_dy, get_text_dy, set_text_dy: f32 = 0.0;
    cjk_w, get_cjk_w, set_cjk_w: f32 = 16.0;
    v0_w, get_v0_w, set_v0_w: f32 = 0.0;
    v0_h, get_v0_h, set_v0_h: f32 = 0.0;
    v1_w, get_v1_w, set_v1_w: f32 = 0.0;
    v1_h, get_v1_h, set_v1_h: f32 = 0.0;
    win_w, get_win_w, set_win_w: f32 = 0.0;
    win_h, get_win_h, set_win_h: f32 = 0.0;
    scale, get_scale, set_scale: f32 = 1.0;
}

/// Text fields whose contents were `in-out` string properties.
macro_rules! field_props {
    ($($get:ident, $set:ident => $id:ident;)*) => {
        impl AppWindow {
            $(
                #[allow(dead_code)]
                pub fn $get(&self) -> String { self.field(FieldId::$id).text.clone() }
                #[allow(dead_code)]
                pub fn $set(&self, v: String) { self.set_field_text(FieldId::$id, v); }
            )*
        }
    };
}

field_props! {
    get_find_text, set_find_text => Find;
    get_replace_text, set_replace_text => Replace;
    get_fif_dir, set_fif_dir => FifDir;
    get_fif_filter, set_fif_filter => FifFilter;
    get_prompt_text, set_prompt_text => Prompt;
    get_rename_text, set_rename_text => Rename;
    get_goto_text, set_goto_text => Goto;
    get_ai_key, set_ai_key => AiKey;
    get_ai_model, set_ai_model => AiModel;
    get_ai_prompt, set_ai_prompt => AiPrompt;
    get_ai_language_custom, set_ai_language_custom => AiLangCustom;
}

pub struct UiInner {
    p: Props,
    dirty: Cell<bool>,
    focus_editor: Cell<bool>,
    quit: Cell<bool>,
    focus_window: Cell<bool>,
    theme: Cell<ThemeColors>,
    fields: RefCell<HashMap<FieldId, TextInputState>>,
    screenshot: RefCell<Option<PathBuf>>,
}

#[derive(Clone)]
pub struct AppWindow(Rc<UiInner>);

impl Default for AppWindow {
    fn default() -> Self { Self::new() }
}

impl AppWindow {
    pub fn new() -> Self {
        let mut fields = HashMap::new();
        let mk = |text: &str, placeholder: &str| { let mut s = TextInputState::new(); s.text = text.into(); s.placeholder = placeholder.into(); s };
        fields.insert(FieldId::Find, mk("", ""));
        fields.insert(FieldId::Replace, mk("", ""));
        fields.insert(FieldId::FifDir, mk("", ""));
        fields.insert(FieldId::FifFilter, mk("*.*", "*.rs;*.txt"));
        fields.insert(FieldId::Prompt, mk("", ""));
        fields.insert(FieldId::Rename, mk("", ""));
        fields.insert(FieldId::Goto, mk("", ""));
        let mut key = mk("", "OpenRouter API key");
        key.password = true;
        fields.insert(FieldId::AiKey, key);
        fields.insert(FieldId::AiModel, mk("openrouter/free", "openrouter/free 或模型名稱:free"));
        fields.insert(FieldId::AiPrompt, mk("", "例如：整理目前文章或建立 JSON"));
        fields.insert(FieldId::AiLangCustom, mk("", "輸入目標語言，例如葡萄牙文"));
        AppWindow(Rc::new(UiInner {
            p: Props::default(), dirty: Cell::new(true), focus_editor: Cell::new(false), quit: Cell::new(false), focus_window: Cell::new(false),
            theme: Cell::new(ThemeColors::default()), fields: RefCell::new(fields), screenshot: RefCell::new(None),
        }))
    }

    /// true (once) when something changed since the last call
    pub fn take_dirty(&self) -> bool { self.0.dirty.replace(false) }
    pub fn request_redraw(&self) { self.0.dirty.set(true); }

    /// Former `public function focus-editor()`.
    pub fn invoke_focus_editor(&self) { self.0.focus_editor.set(true); self.0.dirty.set(true); }
    pub fn take_focus_editor(&self) -> bool { self.0.focus_editor.replace(false) }

    /// Close the window without asking again (replaces `slint::quit_event_loop`).
    pub fn request_quit(&self) { self.0.quit.set(true); self.0.dirty.set(true); }
    pub fn quit_requested(&self) -> bool { self.0.quit.get() }
    /// Un-minimise and focus the window (second instance forwarded files).
    pub fn request_focus_window(&self) { self.0.focus_window.set(true); self.0.dirty.set(true); }
    pub fn take_focus_window(&self) -> bool { self.0.focus_window.replace(false) }

    pub fn theme(&self) -> ThemeColors { self.0.theme.get() }
    pub fn set_theme(&self, t: ThemeColors) { if self.0.theme.get() != t { self.0.theme.set(t); self.0.dirty.set(true); } }

    pub fn field(&self, id: FieldId) -> RefMut<'_, TextInputState> {
        RefMut::map(self.0.fields.borrow_mut(), |m| m.get_mut(&id).expect("field"))
    }
    pub fn set_field_text(&self, id: FieldId, v: String) {
        let mut f = self.field(id);
        if f.text != v {
            f.text = v;
            let n = f.text.len();
            f.move_cursor_to_offset(n);
            f.scroll_offset = 0.0;
            self.0.dirty.set(true);
        }
    }

    /// borrow an editor state without cloning
    pub fn view_state(&self, vi: usize) -> Ref<'_, EdState> { if vi == 0 { self.0.p.v0.borrow() } else { self.0.p.v1.borrow() } }
    /// append Find-in-Files results without cloning the list
    pub fn extend_hits(&self, rows: Vec<SearchHit>) { if !rows.is_empty() { self.0.p.hits.borrow_mut().extend(rows); self.0.dirty.set(true); } }
    pub fn hits_ref(&self) -> Ref<'_, Vec<SearchHit>> { self.0.p.hits.borrow() }
    pub fn side_items_ref(&self) -> Ref<'_, Vec<SideItem>> { self.0.p.side_items.borrow() }
    pub fn preview_blocks_ref(&self) -> Ref<'_, Vec<Block>> { self.0.p.preview_blocks.borrow() }
    pub fn ctx_items_ref(&self) -> Ref<'_, Vec<MenuItem>> { self.0.p.ctx_items.borrow() }
    pub fn tabs_ref(&self) -> Ref<'_, Vec<TabInfo>> { self.0.p.tabs.borrow() }
    pub fn menus_ref(&self) -> Ref<'_, Vec<Menu>> { self.0.p.menus.borrow() }

    pub fn request_screenshot(&self, p: PathBuf) { *self.0.screenshot.borrow_mut() = Some(p); self.0.dirty.set(true); }
    pub fn take_screenshot(&self) -> Option<PathBuf> { self.0.screenshot.borrow_mut().take() }
    pub fn screenshot_pending(&self) -> bool { self.0.screenshot.borrow().is_some() }
}
