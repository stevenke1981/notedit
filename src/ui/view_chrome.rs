//! Window chrome (port of app.slint): menu bar, toolbar, tab strip, status bar, tooltip.
use super::hit::{Target, R};
use super::paint::Painter;
use super::shell::Ctx;
use super::types::*;

pub const MENU_H: f32 = 22.0;
pub const TOOL_H: f32 = 30.0;
pub const TABS_H: f32 = 26.0;
pub const STATUS_H: f32 = 22.0;

/// Toolbar: (label, action, tooltip). Text/Unicode labels replace the SVG icons (gap G5);
/// an empty action is a separator.
pub const TOOLS: &[(&str, &str, &str)] = &[
    ("新", "new", "開新檔案 (Ctrl+N)"), ("開", "open", "開啟舊檔 (Ctrl+O)"), ("存", "save", "儲存檔案 (Ctrl+S)"),
    ("全", "saveall", "全部儲存"), ("✕", "close", "關閉 (Ctrl+W)"), ("⊗", "closeall", "全部關閉"),
    ("印", "print", "列印 (以瀏覽器開啟可列印的 HTML)"), ("", "", ""),
    ("✂", "cut", "剪下 (Ctrl+X)"), ("⧉", "copy", "複製 (Ctrl+C)"), ("貼", "paste", "貼上 (Ctrl+V)"), ("", "", ""),
    ("↶", "undo", "復原 (Ctrl+Z)"), ("↷", "redo", "取消復原 (Ctrl+Y)"), ("", "", ""),
    ("尋", "find", "尋找 (Ctrl+F)"), ("換", "replace", "取代 (Ctrl+H)"), ("", "", ""),
    ("＋", "zoomin", "放大 (Ctrl++)"), ("－", "zoomout", "縮小 (Ctrl+-)"), ("", "", ""),
    ("↵", "wrap", "自動換行"), ("#", "linenumbers", "顯示行號"), ("▣", "selectall", "全選 (Ctrl+A)"), ("◫", "preview", "HTML / Markdown 預覽"),
];

pub fn tool_tip(action: &str) -> Option<&'static str> { TOOLS.iter().find(|t| t.1 == action && !action.is_empty()).map(|t| t.2) }

/// Menu bar; returns the rect of every menu title (for the dropdown position).
pub fn menubar(p: &mut Painter, c: &Ctx, w: f32) -> Vec<R> {
    let th = p.theme;
    p.rect(R::new(0.0, 0.0, w, MENU_H), th.menu);
    let f = c.ui_font(13.0);
    let open = c.ui.get_open_menu();
    let menus = c.ui.menus_ref();
    let mut x = 2.0;
    let mut out = Vec::with_capacity(menus.len());
    for (i, m) in menus.iter().enumerate() {
        let r = R::new(x, 0.0, (p.measure(&m.title, &f) + 14.0).round(), MENU_H);
        if open == i as i32 || c.hover == Some(&Target::MenuTitle(i)) { p.rect(r, th.hover); }
        p.text_centered(r, &m.title, &f, th.fg);
        p.hit(r, Target::MenuTitle(i));
        out.push(r);
        x = r.right();
    }
    out
}

pub fn toolbar(p: &mut Painter, c: &Ctx, w: f32) {
    let th = p.theme;
    let y0 = MENU_H;
    p.rect(R::new(0.0, y0, w, TOOL_H), th.chrome);
    let f = c.ui_font(13.0);
    let mut x = 3.0;
    for &(label, action, _) in TOOLS {
        if action.is_empty() {
            p.rect(R::new(x + 4.0, y0 + 6.0, 1.0, 18.0), hex(0xc8c8c8));
            x += 9.0 + 1.0;
            continue;
        }
        let r = R::new(x, y0 + 3.0, 24.0, 24.0);
        let t = Target::Tool(action);
        let pressed = c.pressed == Some(&t) && c.hover == Some(&t);
        if pressed { p.border_r(r, hex(0x99c9ef), 1.0, hex(0x99c9ef), 3.0); }
        else if c.hover == Some(&t) { p.border_r(r, hex(0xcce4f7), 1.0, hex(0x99c9ef), 3.0); }
        p.text_centered(r, label, &f, th.fg);
        p.hit(r, t);
        x += 25.0;
    }
    let r = R::new(x, y0 + 3.0, 64.0, 24.0);
    if c.ui.get_ai_visible() || c.hover == Some(&Target::AiToggle) { p.rect(r, th.hover); }
    p.text_centered(r, "AI 助手", &f, th.fg);
    p.hit(r, Target::AiToggle);
}

pub fn tabs(p: &mut Painter, c: &Ctx, w: f32) {
    let th = p.theme;
    let y0 = MENU_H + TOOL_H;
    p.rect(R::new(0.0, y0, w, TABS_H), th.tabbar);
    let cur = c.ui.get_current_tab();
    let f = c.ui_font(13.0);
    let tabs = c.ui.tabs_ref();
    let mut x = 2.0;
    p.clip(R::new(0.0, y0, w, TABS_H), |p| {
        for (i, t) in tabs.iter().enumerate() {
            let r = R::new(x, y0, t.w, TABS_H);
            x += t.w + 1.0;
            if r.x > w { continue; }
            let current = i as i32 == cur;
            let hover = matches!(c.hover, Some(Target::Tab(j)) | Some(Target::TabClose(j)) if *j == i);
            p.border(r, if current { th.tab_active } else if hover { th.hover } else { th.tab }, 1.0, th.border);
            if current { p.rect(R::new(r.x, r.y, r.w, 3.0), hex(0xf7a535)); }
            if let Some((from, target)) = c.tab_drag {
                if from != i && target == i as i32 {
                    p.rect(R::new(if from > i { r.x } else { r.right() - 3.0 }, r.y, 3.0, r.h), hex(0x2a6fc9));
                }
            }
            p.hit(r, Target::Tab(i));
            let dot = if t.modified { "●" } else { "○" };
            let dw = p.measure(dot, &f);
            p.text(r.x + 6.0, r.y + ((r.h - f.line_h) / 2.0).round(), dot, &f, if t.modified { hex(0xdd3322) } else { hex(0x33aa33) });
            let tf = f.clone().bold(current);
            let tx = r.x + 6.0 + dw + 4.0;
            p.text_elided(R::new(tx, r.y, (r.right() - 4.0 - 16.0 - 4.0 - tx).max(0.0), r.h), &t.title, &tf, th.fg);
            let xr = R::new(r.right() - 4.0 - 16.0, r.y, 16.0, r.h);
            p.text_centered(xr, "✕", &f, if c.hover == Some(&Target::TabClose(i)) { hex(0xdd0000) } else { hex(0x777777) });
            p.hit(xr, Target::TabClose(i));
        }
    });
}

pub fn status(p: &mut Painter, c: &Ctx, r: R) {
    let th = p.theme;
    p.border(r, th.chrome, 1.0, th.border);
    let f = c.ui_font(13.0);
    let ui = c.ui;
    let rec = ui.get_macro_recording();
    let cols: [(String, f32); 5] = [
        (ui.get_st_length(), 230.0), (ui.get_st_pos(), 260.0), (ui.get_st_eol(), 120.0), (ui.get_st_encoding(), 110.0),
        ((if ui.get_overwrite() { "OVR" } else { "INS" }).to_string(), 40.0),
    ];
    let fixed: f32 = cols.iter().map(|c| c.1 + 1.0).sum();
    let inner = R::new(r.x + 6.0, r.y, r.w - 12.0, r.h);
    let type_w = (inner.w - fixed).max(0.0);
    let ty = format!("{}{}", if rec { "● 巨集錄製中   " } else { "" }, ui.get_st_type());
    p.text_elided(R::new(inner.x, r.y, type_w, r.h), &ty, &f, if rec { hex(0xee3333) } else { th.fg });
    let mut x = inner.x + type_w;
    for (s, w) in cols.iter() {
        p.rect(R::new(x, r.y + 1.0, 1.0, r.h - 2.0), th.border);
        x += 1.0;
        let cell = R::new(x, r.y, *w, r.h);
        let tw = p.measure(s, &f);
        if tw <= *w { p.text_centered(cell, s, &f, th.fg); } else { p.text_elided(cell, s, &f, th.fg); }
        x += w;
    }
}

pub fn tooltip(p: &mut Painter, c: &Ctx, w: f32) {
    let Some((text, x)) = c.tip else { return };
    if c.ui.get_open_menu() >= 0 { return; }
    let f = c.ui_font(12.0);
    let tw = p.measure(text, &f);
    let r = R::new(x.min(w - tw - 12.0).max(0.0), 54.0, tw + 10.0, 20.0);
    p.border(r, hex(0xffffe1), 1.0, hex(0x767676));
    p.text_centered(r, text, &f, hex(0x1e1e1e));
}
