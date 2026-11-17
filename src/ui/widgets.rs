//! Hand-painted controls (gap G4): dialog frame, push button, check box,
//! combo button and single-line text field over `acme_textinput`.
use super::hit::{CheckId, DlgId, FieldId, Target, R};
use super::paint::Painter;
use super::shell::Ctx;
use super::types::*;

pub const ROW_H: f32 = 30.0;
pub const FIELD_H: f32 = 28.0;
pub const BTN_H: f32 = 28.0;
pub const DLG_FG: Color = hex(0x1e1e1e);
pub const DLG_DIM: Color = hex(0x8a8a8a);
const ACCENT: Color = hex(0x0067c0);

/// Dialog frame (title bar with ✕); returns the content rect (10 px padding).
pub fn dialog(p: &mut Painter, c: &Ctx, r: R, title: &str, d: DlgId) -> R {
    p.shadow(R::new(r.x + 2.0, r.y + 2.0, r.w, r.h));
    p.border(r, hex(0xf0f0f0), 1.0, hex(0x888888));
    p.hit(r, Target::Absorb);
    let t = R::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, 25.0);
    p.rect(t, hex(0xdde6f2));
    let f = c.ui_font(13.0);
    p.text_elided(R::new(t.x + 8.0, t.y, t.w - 40.0, t.h), title, &f.clone().bold(true), DLG_FG);
    let x = R::new(t.right() - 24.0, t.y, 22.0, t.h);
    if c.hover == Some(&Target::DialogClose(d)) { p.rect(x, hex(0xe81123)); }
    p.text_centered(x, "✕", &f, if c.hover == Some(&Target::DialogClose(d)) { hex(0xffffff) } else { DLG_FG });
    p.hit(x, Target::DialogClose(d));
    R::new(r.x + 10.0, r.y + 26.0 + 10.0, r.w - 20.0, r.h - 26.0 - 20.0)
}

pub fn label(p: &mut Painter, c: &Ctx, r: R, s: &str, color: Color) {
    let f = c.ui_font(13.0);
    p.text_elided(r, s, &f, color);
}

pub fn button_width(p: &mut Painter, c: &Ctx, s: &str) -> f32 { (p.measure(s, &c.ui_font(13.0)) + 24.0).max(64.0).round() }

/// Push button at (x, y) sized to its label; returns its width.
pub fn button(p: &mut Painter, c: &Ctx, x: f32, y: f32, s: &str, id: &'static str, enabled: bool) -> f32 {
    let w = button_width(p, c, s);
    let r = R::new(x, y, w, BTN_H);
    let t = Target::Button(id);
    let hover = enabled && c.hover == Some(&t);
    let pressed = hover && c.pressed == Some(&t);
    let bg = if !enabled { hex(0xf5f5f5) } else if pressed { hex(0xcce4f7) } else if hover { hex(0xe5f1fb) } else { hex(0xfdfdfd) };
    p.border_r(r, bg, 1.0, if hover { ACCENT } else { hex(0xbcbcbc) }, 4.0);
    p.text_centered(r, s, &c.ui_font(13.0), if enabled { DLG_FG } else { DLG_DIM });
    if enabled { p.hit(r, t); }
    w
}

/// Check box with label; returns the total width.
pub fn checkbox(p: &mut Painter, c: &Ctx, x: f32, y: f32, s: &str, checked: bool, id: CheckId, fg: Color) -> f32 {
    let f = c.ui_font(13.0);
    let w = 16.0 + 6.0 + p.measure(s, &f);
    let row = R::new(x, y, w, 24.0);
    let b = R::new(x, y + 4.0, 16.0, 16.0);
    let hover = c.hover == Some(&Target::Check(id));
    if checked {
        p.border_r(b, ACCENT, 1.0, ACCENT, 3.0);
        p.text_centered(b, "✓", &c.ui_font(12.0), hex(0xffffff));
    } else {
        p.border_r(b, hex(0xffffff), 1.0, if hover { ACCENT } else { hex(0x8a8a8a) }, 3.0);
    }
    p.text(x + 22.0, y + ((24.0 - f.line_h) / 2.0).round(), s, &f, fg);
    p.hit(row, Target::Check(id));
    w
}

/// Combo button (opens the shell's option popup on click).
pub fn combo(p: &mut Painter, c: &Ctx, r: R, value: &str, id: u8, enabled: bool) {
    let t = Target::Combo(id);
    let hover = enabled && c.hover == Some(&t);
    let th = p.theme;
    p.border_r(r, if c.dark { hex(0x2d2d30) } else { hex(0xfdfdfd) }, 1.0, if hover { ACCENT } else { th.border }, 4.0);
    let f = c.ui_font(13.0);
    p.text_elided(R::new(r.x + 8.0, r.y, r.w - 30.0, r.h), value, &f, if enabled { th.fg } else { th.gutter_fg });
    p.text_centered(R::new(r.right() - 22.0, r.y, 18.0, r.h), "▾", &f, th.gutter_fg);
    if enabled { p.hit(r, t); }
}

/// Single-line text field bound to `ui.field(id)`.
pub fn field(p: &mut Painter, c: &Ctx, r: R, d: DlgId, id: FieldId) {
    let focused = c.field_focus == Some((d, id));
    let dark = c.dark && d == DlgId::Ai;
    let (bg, fg) = if dark { (hex(0x1e1e1e), hex(0xd4d4d4)) } else { (hex(0xffffff), DLG_FG) };
    p.border_r(r, bg, if focused { 2.0 } else { 1.0 }, if focused { ACCENT } else { hex(0x8a8a8a) }, 3.0);
    p.hit(r, Target::Field(d, id));
    let f = c.ui_font(13.0);
    let inner = R::new(r.x + 5.0, r.y, r.w - 10.0, r.h);
    let ty = r.y + ((r.h - f.line_h) / 2.0).round();
    let mut st = c.ui.field(id);
    let (text, cursor, sel, preedit) = (st.text.clone(), st.cursor(), st.selection(), st.preedit.clone());
    let pw = st.password;
    let shown = move |s: &str| if pw { "•".repeat(s.chars().count()) } else { s.to_string() };
    let before = shown(&text[..cursor.min(text.len())]);
    let all = shown(&text);
    let caret_x = p.measure(&before, &f);
    let pre_w = if preedit.is_empty() { 0.0 } else { p.measure(&preedit, &f) };
    // keep the caret (and an IME composition) inside the field
    let vis = (inner.w - 2.0).max(0.0);
    let mut scroll = st.scroll_offset.max(0.0);
    let end = caret_x + pre_w;
    if end - scroll > vis { scroll = end - vis; }
    if caret_x < scroll { scroll = caret_x; }
    let total = p.measure(&all, &f) + pre_w;
    scroll = scroll.min((total - vis).max(0.0)).max(0.0);
    st.scroll_offset = scroll;
    let placeholder = st.placeholder.clone();
    drop(st);
    p.clip(inner, |p| {
        let x0 = inner.x - scroll;
        if let Some((a, b)) = sel {
            let xa = p.measure(&shown(&text[..a.min(text.len())]), &f);
            let xb = p.measure(&shown(&text[..b.min(text.len())]), &f);
            p.rect(R::new(x0 + xa, r.y + 4.0, xb - xa, r.h - 8.0), if focused { hex(0xb6d7ff) } else { hex(0xdddddd) });
        }
        if all.is_empty() && preedit.is_empty() {
            p.text(inner.x, ty, &placeholder, &f, DLG_DIM);
        } else if preedit.is_empty() {
            p.text(x0, ty, &all, &f, fg);
        } else {
            let after = shown(&text[cursor.min(text.len())..]);
            p.text(x0, ty, &before, &f, fg);
            p.text(x0 + caret_x, ty, &preedit, &f, fg);
            p.rect(R::new(x0 + caret_x, ty + f.line_h - 1.0, pre_w, 1.0), fg);
            p.text(x0 + caret_x + pre_w, ty, &after, &f, fg);
        }
        if focused && (c.blink_on || !preedit.is_empty()) {
            p.rect(R::new(x0 + caret_x + pre_w, r.y + 5.0, 1.5, r.h - 10.0), fg);
        }
    });
}
