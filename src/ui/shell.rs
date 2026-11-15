//! The AcmeUI application: owns fonts/atlas/text cache, lays out and paints the
//! window every frame, routes platform input to `App`, and drives timers,
//! cross-thread jobs and host window requests.
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use acme_core::Scene;
use acme_platform::{run as acme_run, Application, CursorIcon, FrameContext, PlatformEvent, PlatformKey, WindowConfig, WindowId, WindowRequest};
use acme_text::{FontSystem, GlyphAtlas};

use super::hit::{CheckId, DlgId, FieldId, Hits, Target, R};
use super::paint::Painter;
use super::state::AppWindow;
use super::text::{Font, TextCache};
use super::timer::{Timer, TimerMode};
use super::view_chrome::{MENU_H, STATUS_H, TABS_H, TOOL_H};
use super::view_editor::{self, EdLayout};
use super::{dispatch, keys, timer, view_ai, view_chrome, view_dialogs, view_popup, view_preview, view_side, win};
use crate::App;

/// Ratio of a font's natural line box (ascent + descent) to its size; Slint's
/// metric text height, which AcmeUI does not expose.
const NATURAL: f32 = 1.2;
pub const SIDE_W: f32 = 240.0;
pub const AI_W: f32 = 360.0;
pub const HITS_H: f32 = 160.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus { Editor(usize), Field(DlgId, FieldId) }

/// Scroll offsets of the hand-made list / text panes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Scrolls { pub side: f32, pub hits: f32, pub preview: f32, pub ai: f32 }

#[derive(Clone, Copy, Debug, Default)]
pub struct Layout {
    pub size: (f32, f32),
    pub status: R,
    pub hits: Option<R>,
    pub side: Option<R>,
    pub ed: [Option<EdLayout>; 2],
    pub split_bar: Option<R>,
    pub ai: Option<R>,
    pub preview: Option<R>,
}

/// Read-only paint context shared by the view functions.
pub struct Ctx<'a> {
    pub ui: &'a AppWindow,
    pub dark: bool,
    pub ui_family: &'a str,
    pub mono: String,
    pub font_size: f32,
    pub nat_h: f32,
    pub line_h: f32,
    pub text_dy: f32,
    pub cell_w: f32,
    pub split: bool,
    pub active_view: usize,
    pub editor_focus: Option<usize>,
    pub field_focus: Option<(DlgId, FieldId)>,
    pub overwrite: bool,
    pub caret_idle: bool,
    pub blink_on: bool,
    pub preedit: &'a str,
    pub preedit_cursor: Option<(usize, usize)>,
    pub hover: Option<&'a Target>,
    pub pressed: Option<&'a Target>,
    pub tab_drag: Option<(usize, i32)>,
    pub tip: Option<(&'static str, f32)>,
    pub scroll: Scrolls,
    pub combo_open: Option<u8>,
    pub layout: &'a Layout,
    /// the editor font family has real bold / italic faces (cosmic-text would
    /// otherwise substitute a different, proportional family)
    pub mono_bold: bool,
    pub mono_italic: bool,
    pub ui_bold: bool,
    pub ui_italic: bool,
}

impl Ctx<'_> {
    pub fn ui_font(&self, size: f32) -> Font { Font::new(self.ui_family, size, (size * 1.35).round()) }
    pub fn mono_font(&self) -> Font { Font::new(&self.mono, self.font_size, self.nat_h) }
}

struct Capture { target: Target, rect: R, x0: f32 }

pub struct Shell {
    app: Rc<App>,
    ui: AppWindow,
    fonts: FontSystem,
    atlas: GlyphAtlas,
    cache: TextCache,
    hits: Hits,
    layout: Layout,
    menu_titles: Vec<R>,
    focus: Focus,
    mods: (bool, bool, bool),
    pointer: (f32, f32),
    capture: Option<Capture>,
    hover: Option<Target>,
    pending: Vec<WindowRequest>,
    blink: Timer,
    blink_on: Rc<Cell<bool>>,
    preedit: String,
    preedit_cursor: Option<(usize, usize)>,
    scale: f32,
    quitting: bool,
    title_sent: String,
    first_frame: bool,
    cursor: CursorIcon,
    clipboard: Option<acme_platform::Clipboard>,
    ui_family: String,
    tab_drag: Option<(usize, i32)>,
    side_click: (Option<Instant>, usize),
    scroll: Scrolls,
    combo_open: Option<(u8, R)>,
    dlg_seen: [bool; 7],
    ime_rect: Option<[f32; 4]>,
    metrics_key: (String, u32, u32, u32),
    metrics: (f32, f32, f32, f32, f32),
    fontdb: fontdb::Database,
    styles: std::collections::HashMap<String, (bool, bool)>,
    script: std::collections::VecDeque<super::script::Step>,
    script_next: Option<Instant>,
    forced_mods: Option<(bool, bool, bool)>,
    /// content heights of the scrollable text panes (preview, AI) from the last frame
    preview_h: f32,
    ai_h: f32,
    /// (history length, page) last painted: new replies scroll the history to the end
    ai_seen: (usize, i32),
}

/// Run the window until it closes.
pub fn run(app: Rc<App>) -> Result<(), String> {
    let ui = app.ui();
    let fonts = FontSystem::new();
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let ui_family = pick_ui_family(&db);
    let shell = Shell {
        app, ui, fonts, atlas: GlyphAtlas::default(), cache: TextCache::default(), hits: Hits::default(), layout: Layout::default(),
        menu_titles: Vec::new(), focus: Focus::Editor(0), mods: (false, false, false), pointer: (0.0, 0.0), capture: None, hover: None,
        pending: Vec::new(), blink: Timer::default(), blink_on: Rc::new(Cell::new(true)), preedit: String::new(), preedit_cursor: None,
        scale: 1.0, quitting: false, title_sent: String::new(), first_frame: true, cursor: CursorIcon::Default, clipboard: None, ui_family,
        tab_drag: None, side_click: (None, usize::MAX), scroll: Scrolls::default(), combo_open: None, dlg_seen: [false; 7], ime_rect: None,
        metrics_key: (String::new(), 0, 0, 0), metrics: (8.0, 18.0, 0.0, 16.0, 17.0), fontdb: db, styles: Default::default(),
        script: std::env::var_os("NOTEDIT_SCRIPT").and_then(|p| std::fs::read_to_string(p).ok()).map(|s| super::script::parse(&s).into()).unwrap_or_default(),
        script_next: None, forced_mods: None, preview_h: 0.0, ai_h: 0.0, ai_seen: (0, -1),
    };
    acme_run(shell).map_err(|e| e.to_string())
}

/// UI font: a CJK-capable family for the zh-TW interface when installed.
fn pick_ui_family(db: &fontdb::Database) -> String {
    let have: std::collections::HashSet<String> = db.faces().flat_map(|f| f.families.iter().map(|(n, _)| n.to_lowercase())).collect();
    ["Microsoft JhengHei UI", "Microsoft JhengHei", "Segoe UI", "PingFang TC", "Noto Sans CJK TC", "Noto Sans TC", "Noto Sans CJK SC", "Noto Sans"]
        .iter().find(|f| have.contains(&f.to_lowercase())).map(|s| s.to_string()).unwrap_or_else(|| "sans-serif".into())
}

/// Combo boxes of the AI panel (gap G4): id → options.
pub const COMBO_ROLE: u8 = 0;
pub const COMBO_LANG: u8 = 1;
pub const COMBO_MODE: u8 = 2;
pub const ROLES: &[&str] = &["通用助手", "校稿", "翻譯", "文字潤飾", "摘要", "格式整理"];
pub const LANGS: &[&str] = &["繁體中文（台灣）", "簡體中文", "英文", "日文", "韓文", "越南文", "泰文", "印尼文", "法文", "德文", "西班牙文", "自訂語言"];
pub const MODES: &[&str] = &["僅預覽", "自動建立新檔", "自動備份並修改目前檔案"];

pub fn combo_options(id: u8) -> &'static [&'static str] { match id { COMBO_ROLE => ROLES, COMBO_LANG => LANGS, _ => MODES } }

fn dlg_index(d: DlgId) -> usize { match d { DlgId::Find => 0, DlgId::Fif => 1, DlgId::Prompt => 2, DlgId::Goto => 3, DlgId::About => 4, DlgId::Rename => 5, DlgId::Ai => 6 } }

/// Fields of a dialog in Tab order.
pub fn dialog_fields(ui: &AppWindow, d: DlgId) -> Vec<FieldId> {
    match d {
        DlgId::Find => if ui.get_replace_mode() { vec![FieldId::Find, FieldId::Replace] } else { vec![FieldId::Find] },
        DlgId::Fif => if ui.get_fif_replace() { vec![FieldId::Find, FieldId::Replace, FieldId::FifDir, FieldId::FifFilter] } else { vec![FieldId::Find, FieldId::FifDir, FieldId::FifFilter] },
        DlgId::Prompt => vec![FieldId::Prompt],
        DlgId::Goto => vec![FieldId::Goto],
        DlgId::Rename => vec![FieldId::Rename],
        DlgId::About => vec![],
        DlgId::Ai => match ui.get_ai_page() {
            0 => { let mut v = vec![]; if ui.get_ai_role() == 2 && ui.get_ai_language() == 11 { v.push(FieldId::AiLangCustom); } v.push(FieldId::AiPrompt); v }
            2 => vec![FieldId::AiKey, FieldId::AiModel],
            _ => vec![],
        },
    }
}

pub fn dialog_visible(ui: &AppWindow, d: DlgId) -> bool {
    match d {
        DlgId::Find => ui.get_find_visible(), DlgId::Fif => ui.get_fif_visible(), DlgId::Prompt => ui.get_prompt_visible(),
        DlgId::Goto => ui.get_goto_visible(), DlgId::About => ui.get_about_visible(), DlgId::Rename => ui.get_rename_visible(),
        DlgId::Ai => ui.get_ai_visible(),
    }
}

pub fn close_dialog(ui: &AppWindow, d: DlgId) {
    match d {
        DlgId::Find => ui.set_find_visible(false), DlgId::Fif => ui.set_fif_visible(false), DlgId::Prompt => ui.set_prompt_visible(false),
        DlgId::Goto => ui.set_goto_visible(false), DlgId::About => ui.set_about_visible(false), DlgId::Rename => ui.set_rename_visible(false),
        DlgId::Ai => ui.set_ai_visible(false),
    }
    ui.invoke_focus_editor();
}

/// (has bold, has italic) faces for `family`.
fn family_styles(db: &fontdb::Database, family: &str) -> (bool, bool) {
    let mut out = (false, false);
    for f in db.faces().filter(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(family))) {
        if f.weight.0 >= 600 { out.0 = true; }
        if f.style != fontdb::Style::Normal { out.1 = true; }
    }
    out
}

impl Shell {
    fn mono_styles(&mut self, family: &str) -> (bool, bool) {
        if let Some(s) = self.styles.get(family) { return *s; }
        let s = family_styles(&self.fontdb, family);
        self.styles.insert(family.to_string(), s);
        s
    }

    fn editor_vi(&self) -> usize { if self.ui.get_split() && self.ui.get_active_view() == 1 { 1 } else { 0 } }

    fn set_focus(&mut self, f: Focus) {
        if self.focus == f { return; }
        if let Focus::Field(_, id) = self.focus { let mut s = self.ui.field(id); s.focused = false; s.cancel_preedit(); }
        self.focus = f;
        match f {
            Focus::Field(_, id) => { self.ui.field(id).focused = true; }
            Focus::Editor(vi) => { self.app.ed_focused(vi); }
        }
        self.preedit.clear();
        self.restart_blink();
        self.ui.request_redraw();
    }

    fn restart_blink(&mut self) {
        self.blink_on.set(true);
        if self.blink.running() { self.start_blink(); }
    }

    fn start_blink(&self) {
        let (b, ui) = (self.blink_on.clone(), self.ui.clone());
        self.blink.start(TimerMode::Repeated, Duration::from_millis(500), move || { b.set(!b.get()); ui.request_redraw(); });
    }

    /// Apply app-side requests (quit, focus, title) after any call into App.
    fn after_app(&mut self) -> bool {
        if self.ui.take_focus_editor() { let vi = self.editor_vi(); self.set_focus(Focus::Editor(vi)); }
        if self.ui.quit_requested() && !self.quitting && !self.ui.screenshot_pending() {
            self.quitting = true;
            self.pending.push(WindowRequest::Close);
        }
        if self.ui.take_focus_window() { self.pending.push(WindowRequest::Focus); }
        let title = self.ui.get_window_title();
        if title != self.title_sent { self.title_sent = title.clone(); self.pending.push(WindowRequest::SetTitle(title)); }
        self.ui.take_dirty()
    }

    fn close_popups(&mut self) -> bool {
        let open = self.ui.get_open_menu() >= 0 || self.ui.get_ctx_visible() || self.combo_open.is_some();
        self.ui.set_open_menu(-1);
        self.ui.set_ctx_visible(false);
        self.combo_open = None;
        open
    }

    /// Run an App action after the current event (never open modal dialogs from inside the handler).
    fn later(&self, f: impl FnOnce(&Rc<App>) + 'static) {
        let a = self.app.clone();
        Timer::single_shot(Duration::ZERO, move || f(&a));
    }

    // ------------------------------------------------------------------ metrics & layout

    fn update_metrics(&mut self) -> (f32, f32, f32, f32, f32) {
        let ui = &self.ui;
        let (mono, size, spacing) = (ui.get_mono(), ui.get_editor_font_size(), ui.get_line_spacing());
        let key = (mono.clone(), size.to_bits(), spacing.to_bits(), self.scale.to_bits());
        if key != self.metrics_key {
            let nat = (size * NATURAL).round();
            let f = Font::new(&mono, size, nat);
            let cell = self.cache.measure(&mut self.fonts, &"M".repeat(20), &f, self.scale) / 20.0;
            let cjk = self.cache.measure(&mut self.fonts, &"中".repeat(10), &f, self.scale) / 10.0;
            let line_h = nat.ceil().max((size * spacing).round());
            let text_dy = ((line_h - nat) / 2.0).floor();
            self.metrics = (cell.max(1.0), line_h, text_dy, cjk.max(1.0), nat);
            self.metrics_key = key;
        }
        self.metrics
    }

    fn compute_layout(&self, w: f32, h: f32) -> Layout {
        let ui = &self.ui;
        let top = MENU_H + TOOL_H + TABS_H;
        let status = R::new(0.0, h - STATUS_H, w, STATUS_H);
        let hits_h = if ui.get_hits_visible() { HITS_H } else { 0.0 };
        let hits = (hits_h > 0.0).then(|| R::new(0.0, status.y - hits_h, w, hits_h));
        let mid = R::new(0.0, top, w, (h - STATUS_H - hits_h - top).max(0.0));
        let (split, preview, ai) = (ui.get_split(), ui.get_show_preview(), ui.get_ai_visible());
        let mut x = 0.0;
        let side = (ui.get_side_mode() > 0).then(|| { let r = R::new(0.0, mid.y, SIDE_W, mid.h); x = SIDE_W + 1.0; r });
        let stretch = 1.0 + split as u8 as f32 + preview as u8 as f32;
        let fixed = x + if split { 4.0 } else { 0.0 } + if ai { AI_W + 1.0 } else { 0.0 } + if preview { 1.0 } else { 0.0 };
        let each = ((mid.w - fixed) / stretch).max(0.0).floor();
        let mut ed = [None, None];
        let s0 = ui.view_state(0);
        ed[0] = Some(view_editor::layout(R::new(x, mid.y, each, mid.h), &s0, ui.get_docmap() && !split));
        drop(s0);
        x += each;
        let mut split_bar = None;
        if split {
            split_bar = Some(R::new(x, mid.y, 4.0, mid.h));
            x += 4.0;
            let s1 = ui.view_state(1);
            ed[1] = Some(view_editor::layout(R::new(x, mid.y, each, mid.h), &s1, false));
            x += each;
        }
        let ai_r = ai.then(|| { let r = R::new(x + 1.0, mid.y, AI_W, mid.h); x += AI_W + 1.0; r });
        let preview_r = preview.then(|| R::new(x + 1.0, mid.y, (w - x - 1.0).max(0.0), mid.h));
        Layout { size: (w, h), status, hits, side, ed, split_bar, ai: ai_r, preview: preview_r }
    }

    // ------------------------------------------------------------------ painting

    fn paint(&mut self, w: f32, h: f32) -> Scene {
        self.atlas.begin_frame();
        self.cache.begin_frame();
        let tip = match &self.hover {
            Some(t @ Target::Tool(a)) => view_chrome::tool_tip(a).map(|s| (s, self.hits.rect_of(t).map(|r| r.x).unwrap_or(0.0))),
            _ => None,
        };
        let ui = self.ui.clone();
        let theme = ui.theme();
        let mut scene = Scene::with_clear(theme.editor_bg.to_acme());
        self.hits.clear();
        let (cell_w, line_h, text_dy, _, nat_h) = self.metrics;
        let editor_focus = match self.focus { Focus::Editor(v) => Some(if ui.get_split() { v } else { 0 }), _ => None };
        let field_focus = match self.focus { Focus::Field(d, f) => Some((d, f)), _ => None };
        let layout = self.layout;
        let (mono_bold, mono_italic) = self.mono_styles(&ui.get_mono());
        let ui_family = self.ui_family.clone();
        let (ui_bold, ui_italic) = self.mono_styles(&ui_family);
        let ctx = Ctx {
            ui: &ui, dark: ui.get_dark(), ui_family: &self.ui_family, mono: ui.get_mono(), font_size: ui.get_editor_font_size(), nat_h,
            line_h, text_dy, cell_w, split: ui.get_split(), active_view: ui.get_active_view().max(0) as usize, editor_focus, field_focus,
            overwrite: ui.get_overwrite(), caret_idle: ui.get_caret_idle(), blink_on: self.blink_on.get(), preedit: &self.preedit,
            preedit_cursor: self.preedit_cursor, hover: self.hover.as_ref(), pressed: self.capture.as_ref().map(|c| &c.target),
            tab_drag: self.tab_drag, tip, scroll: self.scroll, combo_open: self.combo_open.map(|c| c.0), layout: &layout,
            mono_bold, mono_italic, ui_bold, ui_italic,
        };
        let mut p = Painter::new(&mut scene, &mut self.fonts, &mut self.atlas, &mut self.cache, theme, self.scale, &mut self.hits);
        let mut preview_h = 0.0;
        let titles = view_chrome::menubar(&mut p, &ctx, w);
        view_chrome::toolbar(&mut p, &ctx, w);
        view_chrome::tabs(&mut p, &ctx, w);
        if let Some(r) = layout.side {
            view_side::sidebar(&mut p, &ctx, r);
            p.rect(R::new(r.right(), r.y, 1.0, r.h), theme.border);
        }
        for vi in 0..2 {
            if let Some(el) = &layout.ed[vi] {
                let st = ui.view_state(vi);
                view_editor::paint(&mut p, &ctx, vi, el, &st);
            }
        }
        if let Some(b) = layout.split_bar { p.rect(b, theme.border); }
        let mut ai_h = 0.0;
        if let Some(r) = layout.ai {
            p.rect(R::new(r.x - 1.0, r.y, 1.0, r.h), theme.border);
            ai_h = view_ai::paint(&mut p, &ctx, r);
        }
        if let Some(r) = layout.preview {
            p.rect(R::new(r.x - 1.0, r.y, 1.0, r.h), super::types::hex(0xaaaaaa));
            preview_h = view_preview::paint(&mut p, &ctx, r);
        }
        if let Some(r) = layout.hits { view_side::hits(&mut p, &ctx, r); }
        view_chrome::status(&mut p, &ctx, layout.status);
        view_chrome::tooltip(&mut p, &ctx, w);
        view_popup::dropdown(&mut p, &ctx, (w, h), &titles);
        view_dialogs::paint(&mut p, &ctx, (w, h));
        view_popup::context(&mut p, &ctx, (w, h));
        view_dialogs::paint_rename(&mut p, &ctx, (w, h));
        let combo = self.combo_open.map(|(id, r)| (id, r, combo_options(id), match id { COMBO_ROLE => ui.get_ai_role(), COMBO_LANG => ui.get_ai_language(), _ => ui.get_ai_mode() }));
        view_popup::combo(&mut p, &ctx, (w, h), combo);
        drop(p);
        self.menu_titles = titles;
        self.preview_h = preview_h;
        self.ai_h = ai_h;
        scene
    }

    fn ime_area(&self) -> Option<[f32; 4]> {
        match self.focus {
            Focus::Editor(v) => {
                let vi = if self.ui.get_split() { v } else { 0 };
                let el = self.layout.ed[vi]?;
                let st = self.ui.view_state(vi);
                let c = st.carets.first()?;
                Some([el.text.x + c.x, el.text.y + c.y, 2.0, self.metrics.1])
            }
            Focus::Field(..) => self.hits.list.iter().rev().find(|(_, t)| matches!(t, Target::Field(..)) && *t == self.field_target()).map(|(r, _)| [r.x + 4.0, r.y, 2.0, r.h]),
        }
    }

    fn field_target(&self) -> Target { match self.focus { Focus::Field(d, f) => Target::Field(d, f), _ => Target::Absorb } }

    // ------------------------------------------------------------------ keyboard

    fn key(&mut self, key: PlatformKey, text: Option<String>, character: Option<String>, ctrl: bool, shift: bool, alt: bool) -> bool {
        if key == PlatformKey::Escape && (self.ui.get_open_menu() >= 0 || self.ui.get_ctx_visible() || self.combo_open.is_some()) {
            self.close_popups();
            return true;
        }
        match self.focus {
            Focus::Field(d, id) if dialog_visible(&self.ui, d) => { self.field_key(d, id, key, text, character, ctrl, shift, alt); true }
            Focus::Field(..) => { let vi = self.editor_vi(); self.set_focus(Focus::Editor(vi)); self.editor_key(vi, key, text, character, ctrl, shift, alt) }
            Focus::Editor(v) => { let vi = if self.ui.get_split() { v } else { 0 }; self.editor_key(vi, key, text, character, ctrl, shift, alt) }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn editor_key(&mut self, vi: usize, key: PlatformKey, text: Option<String>, character: Option<String>, ctrl: bool, shift: bool, alt: bool) -> bool {
        // while an IME composition (注音/倉頡…) is active, keys belong to the IME
        if !self.preedit.is_empty() { return false; }
        self.blink_on.set(true);
        let t = keys::slint_text(key, character.as_deref(), text.as_deref());
        if let Some(t) = &t {
            if self.app.ed_key(vi, t, ctrl, shift, alt) { return true; }
            if self.app.global_key(t, ctrl, shift, alt) { return true; }
        }
        if let Some(s) = keys::typed_text(key, text.as_deref(), ctrl, alt) { self.app.ed_input(vi, &s); return true; }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn field_key(&mut self, d: DlgId, id: FieldId, key: PlatformKey, text: Option<String>, character: Option<String>, ctrl: bool, shift: bool, alt: bool) {
        self.blink_on.set(true);
        match key {
            PlatformKey::Enter => { self.field_accept(d, id); return; }
            PlatformKey::Escape => {
                let had = !self.ui.field(id).preedit.is_empty();
                if had { self.ui.field(id).cancel_preedit(); } else if d == DlgId::Ai { let vi = self.editor_vi(); self.set_focus(Focus::Editor(vi)); } else { close_dialog(&self.ui, d); }
                self.ui.request_redraw();
                return;
            }
            PlatformKey::Tab => {
                let fields = dialog_fields(&self.ui, d);
                if let Some(i) = fields.iter().position(|f| *f == id) {
                    let n = fields.len();
                    let j = if shift { (i + n - 1) % n } else { (i + 1) % n };
                    self.set_focus(Focus::Field(d, fields[j]));
                    let mut f = self.ui.field(fields[j]);
                    f.select_all();
                }
                return;
            }
            _ => {}
        }
        // shortcuts the field does not use (Ctrl+H, Ctrl+G, F3, ...) reach the window table
        let field_shortcut = ctrl && !alt && matches!(character.as_deref().map(str::to_lowercase).as_deref(), Some("a" | "c" | "v" | "x" | "z" | "y"));
        if (ctrl || alt || matches!(key, PlatformKey::Function(_) | PlatformKey::Insert)) && !field_shortcut && !(ctrl && alt) {
            if let Some(t) = keys::slint_text(key, character.as_deref(), text.as_deref()) {
                if self.app.global_key(&t, ctrl, shift, alt) { return; }
            }
        }
        if self.clipboard.is_none() { self.clipboard = acme_platform::Clipboard::new().ok(); }
        let mut f = self.ui.field(id);
        let changed = if ctrl && !alt {
            let c = character.as_deref().unwrap_or("").to_lowercase();
            let c = if c == "z" && shift { "y".to_string() } else { c };
            match c.as_str() {
                "a" | "c" | "v" | "x" | "z" | "y" => acme_textinput::handle_keyboard_shortcut(&mut f, &c, self.clipboard.as_ref()),
                _ => acme_textinput::handle_key(&mut f, &key, true, None, ctrl, shift),
            }
        } else if let Some(s) = keys::typed_text(key, text.as_deref(), ctrl, alt) {
            acme_textinput::handle_text(&mut f, &s)
        } else {
            acme_textinput::handle_key(&mut f, &key, true, None, ctrl, shift)
        };
        drop(f);
        if changed { self.ui.request_redraw(); }
    }

    fn field_accept(&mut self, d: DlgId, id: FieldId) {
        let text = self.ui.field(id).text.clone();
        let ui = self.ui.clone();
        match (d, id) {
            (DlgId::Find, FieldId::Find) => self.later(|a| { a.find(true); }),
            (DlgId::Fif, FieldId::Find) => self.later(|a| a.fif_run()),
            (DlgId::Prompt, _) => { ui.set_prompt_visible(false); self.later(move |a| a.prompt_ok(&text)); }
            (DlgId::Goto, _) => self.later(move |a| a.goto_line(&text)),
            (DlgId::Rename, _) => self.later(move |a| a.rename(text)),
            (DlgId::Ai, FieldId::AiPrompt) => self.later(|a| a.ai_send()),
            _ => {}
        }
    }

    /// byte offset in field `id` closest to x (relative to the text start)
    fn field_offset(&mut self, id: FieldId, x: f32) -> usize {
        let f = Font::new(&self.ui_family, 13.0, 18.0);
        let st = self.ui.field(id);
        let (text, password, scroll) = (st.text.clone(), st.password, st.scroll_offset);
        drop(st);
        let x = x + scroll;
        let mut best = (0usize, f32::MAX);
        let mut chars = 0usize;
        for (b, _) in text.char_indices().map(|(i, c)| (i, c)).chain(std::iter::once((text.len(), ' '))) {
            let shown: String = if password { "•".repeat(chars) } else { text[..b].to_string() };
            let w = self.cache.measure(&mut self.fonts, &shown, &f, self.scale);
            let d = (w - x).abs();
            if d < best.1 { best = (b, d); } else if w > x { break; }
            chars += 1;
        }
        best.0
    }

    // ------------------------------------------------------------------ mouse

    fn press(&mut self, x: f32, y: f32, button: u16, ctrl: bool, shift: bool, alt: bool) -> bool {
        let Some((r, t)) = self.hits.at(x, y) else { return self.close_popups() };
        let ui = self.ui.clone();
        if button == 1 {
            // right button: context menus
            match t {
                Target::Backdrop => { self.close_popups(); }
                Target::Tab(i) | Target::TabClose(i) => {
                    self.close_popups();
                    ui.set_ctx_x(x); ui.set_ctx_y(y);
                    self.app.context_menu("tab", i as i32);
                }
                Target::Editor(vi) => {
                    self.close_popups();
                    self.set_focus(Focus::Editor(vi));
                    let el = self.layout.ed[vi].unwrap_or_default();
                    self.app.ed_mouse(vi, 3, x - el.text.x, y - el.text.y, false, false, false);
                    ui.set_ctx_x(x); ui.set_ctx_y(y);
                    self.app.context_menu("editor", vi as i32);
                }
                _ => {}
            }
            return true;
        }
        if button != 0 { return false; }
        match &t {
            Target::Backdrop => { self.close_popups(); return true; }
            Target::MenuTitle(i) => {
                let open = ui.get_open_menu();
                self.combo_open = None;
                ui.set_ctx_visible(false);
                ui.set_open_menu(if open == *i as i32 { -1 } else { *i as i32 });
                return true;
            }
            Target::Editor(vi) => {
                let vi = *vi;
                self.set_focus(Focus::Editor(vi));
                let el = self.layout.ed[vi].unwrap_or_default();
                self.app.ed_mouse(vi, 0, x - el.text.x, y - el.text.y, ctrl, shift, alt);
            }
            Target::Gutter(vi) => {
                let vi = *vi;
                self.set_focus(Focus::Editor(vi));
                let gw = ui.view_state(vi).gutter_w;
                let row = ((y - r.y) / self.metrics.1).floor() as i32;
                self.app.ed_gutter(vi, row, x - r.x >= gw - 15.0, shift);
                return true;
            }
            Target::VBar(_) | Target::HBar(_) | Target::Map(_) => { self.capture = Some(Capture { target: t.clone(), rect: r, x0: x }); self.drag(x, y); return true; }
            Target::Tab(i) => {
                self.app.select_tab(*i);
                self.tab_drag = Some((*i, *i as i32));
            }
            Target::Field(d, id) => {
                let (d, id) = (*d, *id);
                self.set_focus(Focus::Field(d, id));
                let off = self.field_offset(id, x - r.x - 5.0);
                let mut f = ui.field(id);
                if shift { let a = f.selection().map(|s| s.0).unwrap_or(f.cursor()); f.set_selection_range(a.min(off), a.max(off)); } else { f.move_cursor_to_offset(off); }
            }
            _ => {}
        }
        self.capture = Some(Capture { target: t, rect: r, x0: x });
        true
    }

    /// pointer moved while a button is held on `capture`
    fn drag(&mut self, x: f32, y: f32) -> bool {
        let Some(cap) = &self.capture else { return false };
        let (t, r, x0) = (cap.target.clone(), cap.rect, cap.x0);
        match t {
            Target::Editor(vi) => {
                let el = self.layout.ed[vi].unwrap_or_default();
                self.app.ed_mouse(vi, 1, x - el.text.x, y - el.text.y, false, false, false);
            }
            Target::VBar(vi) => {
                let st = self.ui.view_state(vi);
                let th = (r.h * st.thumb_size).max(24.0).min(r.h);
                drop(st);
                let f = ((y - r.y - th / 2.0) / (r.h - th).max(1.0)).clamp(0.0, 1.0);
                self.app.ed_vscroll(vi, f);
            }
            Target::HBar(vi) => {
                let st = self.ui.view_state(vi);
                let tw = (r.w * st.hthumb_size).max(24.0).min(r.w);
                drop(st);
                let f = ((x - r.x - tw / 2.0) / (r.w - tw).max(1.0)).clamp(0.0, 1.0);
                self.app.ed_hscroll(vi, f);
            }
            Target::Map(vi) => { self.app.ed_map(vi, (y - r.y) / r.h.max(1.0)); }
            Target::Tab(_) => {
                if let Some((from, _)) = self.tab_drag {
                    let target = self.app.tab_target(from, x - x0);
                    if self.tab_drag != Some((from, target)) { self.tab_drag = Some((from, target)); self.ui.request_redraw(); }
                }
            }
            Target::Field(_, id) => {
                let off = self.field_offset(id, x - r.x - 5.0);
                let mut f = self.ui.field(id);
                let anchor = match f.selection() { Some((s, e)) => if f.cursor() == e { s } else { e }, None => f.cursor() };
                f.set_selection_range(anchor.min(off), anchor.max(off));
                if off < anchor { f.move_cursor_to_offset(off); f.set_selection_range(off, anchor); }
                drop(f);
                self.ui.request_redraw();
            }
            _ => { self.ui.request_redraw(); }
        }
        true
    }

    fn release(&mut self, x: f32, y: f32, button: u16) -> bool {
        if button != 0 { return false; }
        let Some(cap) = self.capture.take() else { return false };
        match &cap.target {
            Target::Editor(vi) => {
                let el = self.layout.ed[*vi].unwrap_or_default();
                self.app.ed_mouse(*vi, 2, x - el.text.x, y - el.text.y, false, false, false);
            }
            Target::Tab(_) => {
                if let Some((from, to)) = self.tab_drag.take() {
                    if to >= 0 && to as usize != from { self.app.move_tab(from, to as usize); }
                }
            }
            t => {
                if self.hits.at(x, y).map(|(_, h)| h).as_ref() == Some(t) { self.click(t.clone()); }
            }
        }
        self.tab_drag = None;
        true
    }

    fn click(&mut self, t: Target) {
        let ui = self.ui.clone();
        match t {
            Target::MenuItem(a) | Target::CtxItem(a) => self.app.menu_action(&a),
            Target::Tool(a) => self.app.menu_action(a),
            Target::AiToggle => ui.set_ai_visible(!ui.get_ai_visible()),
            Target::TabClose(i) => self.later(move |a| { a.close(i); }),
            Target::AcItem(vi, i) => self.app.accept_autocomplete(vi, Some(i)),
            Target::SideRow(i) => {
                let now = Instant::now();
                let double = matches!(self.side_click, (Some(t), j) if j == i && now.duration_since(t) < Duration::from_millis(400));
                self.side_click = (if double { None } else { Some(now) }, i);
                if double { self.later(move |a| a.side_activate(i)); } else { self.later(move |a| a.side_click(i)); }
            }
            Target::SideTab(m) => self.later(move |a| a.side_switch(m)),
            Target::SideFolder => self.later(|a| a.side_open_folder()),
            Target::Hit(i) => self.later(move |a| a.hit_clicked(i)),
            Target::HitsClose => ui.set_hits_visible(false),
            Target::Check(c) => {
                match c {
                    CheckId::MatchCase => ui.set_match_case(!ui.get_match_case()),
                    CheckId::WholeWord => ui.set_whole_word(!ui.get_whole_word()),
                    CheckId::UseRegex => ui.set_use_regex(!ui.get_use_regex()),
                    CheckId::ReplaceMode => ui.set_replace_mode(!ui.get_replace_mode()),
                    CheckId::FifRecursive => ui.set_fif_recursive(!ui.get_fif_recursive()),
                    CheckId::FifReplace => ui.set_fif_replace(!ui.get_fif_replace()),
                    CheckId::AiIncludeDoc => ui.set_ai_include_document(!ui.get_ai_include_document()),
                }
            }
            Target::Button(id) => self.later(move |a| a.ui_button(id)),
            Target::Combo(id) => {
                let r = self.hits.rect_of(&Target::Combo(id)).unwrap_or_default();
                self.combo_open = if self.combo_open.map(|c| c.0) == Some(id) { None } else { Some((id, r)) };
            }
            Target::ComboItem(id, i) => {
                match id { COMBO_ROLE => ui.set_ai_role(i as i32), COMBO_LANG => ui.set_ai_language(i as i32), _ => ui.set_ai_mode(i as i32) }
                self.combo_open = None;
            }
            Target::AiPage(n) => ui.set_ai_page(n),
            Target::AiClose => { ui.set_ai_visible(false); ui.invoke_focus_editor(); }
            Target::DialogClose(d) => close_dialog(&ui, d),
            _ => {}
        }
        ui.request_redraw();
    }

    fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let Some((_, t)) = self.hits.at(x, y) else { return false };
        // G9: Scroll carries no modifiers; ask Windows, else use the last known state
        let (ctrl, shift, _) = self.forced_mods.take().or_else(win::mods).unwrap_or(self.mods);
        let step = |v: &mut f32, d: f32, max: f32| { *v = (*v - d).clamp(0.0, max.max(0.0)); };
        let l = self.layout;
        let side_max = l.side.map_or(0.0, |r| view_side::max_scroll(self.ui.side_items_ref().len(), r.h - view_side::SIDE_HEAD));
        let hits_max = l.hits.map_or(0.0, |r| view_side::max_scroll(self.ui.hits_ref().len(), r.h - view_side::HITS_HEAD - 2.0));
        let preview_max = l.preview.map_or(0.0, |r| self.preview_h - (r.h - 22.0));
        let ai_max = self.ai_h;
        match t {
            Target::Editor(vi) | Target::Gutter(vi) | Target::VBar(vi) | Target::HBar(vi) | Target::Map(vi) | Target::AcItem(vi, _) => {
                self.app.ed_wheel(vi, dx, dy, shift, ctrl);
            }
            Target::SideRow(_) | Target::SideList => step(&mut self.scroll.side, dy, side_max),
            Target::Hit(_) | Target::HitsList => step(&mut self.scroll.hits, dy, hits_max),
            Target::Preview => step(&mut self.scroll.preview, dy, preview_max),
            Target::AiScroll => step(&mut self.scroll.ai, dy, ai_max),
            _ => return false,
        }
        self.ui.request_redraw();
        true
    }

    fn update_hover(&mut self, x: f32, y: f32) -> bool {
        let h = self.hits.at(x, y).map(|(_, t)| t);
        let mut changed = false;
        // moving over another menu title while a menu is open switches menus
        if let Some(Target::MenuTitle(i)) = &h {
            let open = self.ui.get_open_menu();
            if open >= 0 && open != *i as i32 { self.ui.set_open_menu(*i as i32); changed = true; }
        }
        let cursor = match &h { Some(Target::Editor(_)) | Some(Target::Field(..)) => CursorIcon::Text, _ => CursorIcon::Default };
        if cursor != self.cursor { self.cursor = cursor; self.pending.push(WindowRequest::SetCursor(cursor)); }
        if h != self.hover { self.hover = h; changed = true; }
        changed
    }

    fn handle(&mut self, ev: PlatformEvent) -> bool {
        match ev {
            PlatformEvent::Key { key, pressed, shift, ctrl, alt, text, character, .. } => {
                self.mods = (ctrl, shift, alt);
                if !pressed { return false; }
                self.key(key, text, character, ctrl, shift, alt)
            }
            PlatformEvent::PointerMoved { x, y, .. } => {
                self.pointer = (x, y);
                if self.capture.is_some() { return self.drag(x, y); }
                self.update_hover(x, y)
            }
            PlatformEvent::PointerLeft { .. } => { if self.capture.is_none() && self.hover.take().is_some() { return true; } false }
            PlatformEvent::PointerButton { pressed, x, y, button, shift, ctrl, alt, .. } => {
                self.mods = (ctrl, shift, alt);
                self.pointer = (x, y);
                if pressed { self.press(x, y, button, ctrl, shift, alt) } else { self.release(x, y, button) }
            }
            PlatformEvent::Scroll { delta_x, delta_y, lines, x, y, .. } => {
                // AcmeUI reports 32 px per wheel line; Slint used 60 (three editor rows per notch)
                let k = if lines { 60.0 / 32.0 } else { 1.0 };
                self.wheel(x, y, delta_x * k, delta_y * k)
            }
            PlatformEvent::ImePreedit { text, cursor, .. } => {
                match self.focus {
                    Focus::Field(_, id) => { self.ui.field(id).set_preedit(&text, cursor); }
                    Focus::Editor(_) => { self.preedit = text; self.preedit_cursor = cursor; }
                }
                true
            }
            PlatformEvent::ImeCommit { text, .. } => {
                match self.focus {
                    Focus::Field(_, id) => { acme_textinput::handle_text(&mut self.ui.field(id), &text); }
                    Focus::Editor(v) => {
                        self.preedit.clear();
                        self.preedit_cursor = None;
                        let vi = if self.ui.get_split() { v } else { 0 };
                        self.app.ed_input(vi, &text);
                    }
                }
                true
            }
            PlatformEvent::ImeDisabled(_) => { self.preedit.clear(); true }
            PlatformEvent::FileDropped { paths, .. } => { self.app.open_dropped(paths.into_iter().map(std::path::PathBuf::from).collect()); false }
            PlatformEvent::FocusChanged { gained, .. } => { if gained { self.blink_on.set(true); } true }
            PlatformEvent::Resized { .. } | PlatformEvent::MaximizedChanged { .. } => true,
            _ => false,
        }
    }
}

impl Application for Shell {
    fn window_config(&self) -> WindowConfig {
        WindowConfig { title: self.ui.get_window_title(), width: 1100.0, height: 720.0, min_width: Some(400.0), min_height: Some(240.0), ..Default::default() }
    }

    fn event(&mut self, ev: PlatformEvent) -> bool {
        let local = self.handle(ev);
        self.after_app() || local
    }

    fn on_waker(&mut self, w: acme_platform::Waker) { dispatch::set_waker(w); }

    fn wake(&mut self) -> bool {
        dispatch::drain();
        self.after_app() || true
    }

    fn take_window_requests(&mut self, _w: WindowId) -> Vec<WindowRequest> {
        self.after_app_requests_only();
        std::mem::take(&mut self.pending)
    }

    fn ime_cursor_area(&self, _w: WindowId) -> Option<[f32; 4]> { self.ime_rect }

    fn on_gpu_recovered(&mut self, _w: WindowId) { self.atlas.clear(); }

    fn close_requested(&mut self, _w: WindowId) -> bool {
        if self.quitting { return true; }
        let ok = self.app.confirm_exit();
        if ok { self.quitting = true; }
        ok
    }

    fn background_poll_interval(&self) -> Option<Duration> {
        let t = timer::next_interval();
        if self.script.is_empty() { t } else { Some(t.unwrap_or(Duration::from_millis(10)).min(Duration::from_millis(10))) }
    }

    fn background_tick(&mut self) -> bool {
        timer::fire_due();
        dispatch::drain();
        let scripted = self.run_script();
        self.after_app() || scripted
    }

    fn frame(&mut self, ctx: FrameContext) -> Scene {
        let (w, h) = (ctx.logical_width, ctx.logical_height);
        self.scale = ctx.scale_factor;
        let ui = self.ui.clone();
        ui.set_win_w(w); ui.set_win_h(h); ui.set_scale(self.scale);
        if self.first_frame {
            self.first_frame = false;
            if let Some(hwnd) = win::main_hwnd() { win::set_icon(hwnd); }
            dispatch::drain();
        }
        // metrics + view sizes (former `out` properties); a change re-renders synchronously
        let (cell_w, line_h, text_dy, cjk_w, _) = self.update_metrics();
        let before = (ui.get_cell_w(), ui.get_line_h(), ui.get_v0_w(), ui.get_v0_h(), ui.get_v1_w(), ui.get_v1_h(), ui.get_cjk_w());
        ui.set_cell_w(cell_w); ui.set_line_h(line_h); ui.set_text_dy(text_dy); ui.set_cjk_w(cjk_w);
        let mut layout = self.compute_layout(w, h);
        let size_of = |l: &Option<EdLayout>| l.map(|e| (e.text.w, e.text.h)).unwrap_or((0.0, 0.0));
        let ((w0, h0), (w1, h1)) = (size_of(&layout.ed[0]), size_of(&layout.ed[1]));
        ui.set_v0_w(w0); ui.set_v0_h(h0); ui.set_v1_w(w1); ui.set_v1_h(h1);
        if before != (cell_w, line_h, w0, h0, w1, h1, cjk_w) {
            self.app.kick();
            self.app.tick();
            // the render may change the gutter width / scrollbars: lay out again
            layout = self.compute_layout(w, h);
        }
        self.layout = layout;
        self.after_app();
        // dialogs that just opened take keyboard focus (LineEdit `init => focus()`)
        for d in [DlgId::Find, DlgId::Fif, DlgId::Prompt, DlgId::Goto, DlgId::Rename] {
            let vis = dialog_visible(&ui, d);
            let i = dlg_index(d);
            if vis && !self.dlg_seen[i] {
                if d == DlgId::Goto { ui.set_goto_text(String::new()); }
                if let Some(f) = dialog_fields(&ui, d).first() { self.set_focus(Focus::Field(d, *f)); self.ui.field(*f).select_all(); }
            }
            if !vis && self.dlg_seen[i] { if let Focus::Field(fd, _) = self.focus { if fd == d { let vi = self.editor_vi(); self.set_focus(Focus::Editor(vi)); } } }
            self.dlg_seen[i] = vis;
        }
        if let Focus::Field(DlgId::Ai, _) = self.focus { if !ui.get_ai_visible() { let vi = self.editor_vi(); self.set_focus(Focus::Editor(vi)); } }
        // caret blink only while an editor or field has focus and the editor is not idle
        let want_blink = !ui.get_caret_idle() || matches!(self.focus, Focus::Field(..));
        if want_blink && !self.blink.running() { self.start_blink(); }
        if !want_blink && self.blink.running() { self.blink.stop(); self.blink_on.set(true); }

        let seen = (ui.get_ai_history().len(), ui.get_ai_page());
        if seen != self.ai_seen {
            self.scroll.ai = if seen.1 == 0 { f32::MAX } else if seen.1 != self.ai_seen.1 { 0.0 } else { self.scroll.ai };
            self.ai_seen = seen;
        }
        if let Some(path) = ui.take_screenshot() {
            // a fresh CPU renderer needs every glyph upload: paint once from an empty atlas
            self.atlas.clear();
            let scene = self.paint(w, h);
            if let Err(e) = super::snapshot::write_png(&scene, w, h, self.scale, &path) { eprintln!("NotEdit: screenshot failed: {e}"); }
            self.atlas.clear();
        }
        let scene = self.paint(w, h);
        self.ime_rect = self.ime_area();
        self.after_app();
        ui.take_dirty();
        scene
    }
}

impl Shell {
    /// Feed due `NOTEDIT_SCRIPT` steps through the normal event routing.
    fn run_script(&mut self) -> bool {
        use super::script::Step;
        if self.first_frame || self.script.is_empty() { return false; }
        let now = Instant::now();
        let next = *self.script_next.get_or_insert(now + Duration::from_millis(300));
        if now < next { return false; }
        let Some(step) = self.script.pop_front() else { return false };
        let mut gap = Duration::from_millis(80);
        match step {
            Step::Wait(d) => gap = d,
            Step::Events(evs) => { for ev in evs { self.handle(ev); self.after_app(); } }
            Step::Wheel { x, y, dy, ctrl, shift } => { self.forced_mods = Some((ctrl, shift, false)); self.wheel(x, y, 0.0, dy * 60.0); }
            Step::Shot(p) => { self.ui.request_screenshot(p); gap = Duration::from_millis(150); }
            Step::Quit => self.ui.request_quit(),
        }
        self.script_next = Some(Instant::now() + gap);
        self.ui.request_redraw();
        true
    }

    /// requests that do not need a redraw (title, quit, focus)
    fn after_app_requests_only(&mut self) {
        let dirty = self.after_app();
        if dirty { self.ui.request_redraw(); }
    }
}
