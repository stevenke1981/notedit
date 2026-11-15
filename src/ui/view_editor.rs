//! Editor surface (port of ui/editor.slint): gutter, rows, selections, carets,
//! IME preedit, autocomplete popup, scrollbars and document map.
use super::hit::{Target, R};
use super::paint::Painter;
use super::shell::Ctx;
use super::text::Font;
use super::types::*;

pub const BAR: f32 = 12.0;
pub const MAP_W: f32 = 90.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct EdLayout {
    pub outer: R,
    pub title: Option<R>,
    pub body: R,
    pub gutter: R,
    pub text: R,
    pub vbar: R,
    pub hbar: Option<R>,
    pub map: Option<R>,
}

pub fn layout(outer: R, st: &EdState, map_visible: bool) -> EdLayout {
    let title_h = if st.title.is_empty() { 0.0 } else { 20.0 };
    let map_w = if map_visible { MAP_W } else { 0.0 };
    let body = R::new(outer.x, outer.y + title_h, (outer.w - map_w).max(0.0), (outer.h - title_h).max(0.0));
    let gw = st.gutter_w.min(body.w);
    let text_h = (body.h - if st.hbar { BAR } else { 0.0 }).max(0.0);
    let text = R::new(body.x + gw, body.y, (body.w - gw - BAR).max(0.0), text_h);
    EdLayout {
        outer,
        title: (title_h > 0.0).then(|| R::new(outer.x, outer.y, outer.w, title_h)),
        body,
        gutter: R::new(body.x, body.y, gw, body.h),
        text,
        vbar: R::new(body.right() - BAR, body.y, BAR, text_h),
        hbar: st.hbar.then(|| R::new(text.x, body.bottom() - BAR, text.w, BAR)),
        map: map_visible.then(|| R::new(body.right(), body.y, MAP_W, body.h)),
    }
}

/// Thumb rect of the vertical scrollbar (shared with the drag handler).
pub fn vthumb(bar: R, st: &EdState) -> (f32, f32) {
    let th = (bar.h * st.thumb_size).max(24.0).min(bar.h);
    (bar.y + (bar.h - th) * st.thumb_pos, th)
}

pub fn hthumb(bar: R, st: &EdState) -> (f32, f32) {
    let tw = (bar.w * st.hthumb_size).max(24.0).min(bar.w);
    (bar.x + (bar.w - tw) * st.hthumb_pos, tw)
}

pub fn paint(p: &mut Painter, c: &Ctx, vi: usize, l: &EdLayout, st: &EdState) {
    let th = p.theme;
    let dark = c.dark;
    let lh = c.line_h;
    let active = c.active_view == vi || !c.split;
    let focused = c.editor_focus == Some(vi);
    p.rect(l.outer, th.editor_bg);
    if let Some(t) = l.title {
        p.rect(t, if active { if dark { hex(0x3a3d41) } else { hex(0xdde6f2) } } else if dark { hex(0x2b2b2b) } else { hex(0xececec) });
        let f = c.ui_font(13.0).bold(active);
        p.text_elided(R::new(t.x + 6.0, t.y, t.w - 8.0, t.h), &st.title, &f, th.fg);
    }
    let mono = c.mono_font();
    p.clip(l.body, |p| {
        // ---- gutter
        p.rect(l.gutter, th.gutter);
        for (i, r) in st.rows.iter().enumerate() {
            let y = l.gutter.y + i as f32 * lh;
            if r.mark {
                p.border_r(R::new(l.gutter.x + 3.0, y + ((lh - 10.0) / 2.0).round(), 10.0, 10.0), hex(0x2a6fc9), 1.0, hex(0x1a4f99), 5.0);
            }
            if st.num_w > 0.0 && !r.num.is_empty() {
                let w = p.measure(&r.num, &mono);
                p.text(l.gutter.x + 16.0 + st.num_w - w, y + c.text_dy, &r.num, &mono, if r.cur { th.fg } else { th.gutter_fg });
            }
            if r.fold > 0 {
                let b = R::new(l.gutter.x + st.gutter_w - 13.0, y + ((lh - 9.0) / 2.0).round(), 9.0, 9.0);
                p.border(b, if dark { hex(0x2d2d30) } else { hex(0xffffff) }, 1.0, th.gutter_fg);
                p.rect(R::new(b.x + 2.0, b.y + 4.0, 5.0, 1.0), th.fg);
                if r.fold == 2 { p.rect(R::new(b.x + 4.0, b.y + 2.0, 1.0, 5.0), th.fg); }
            }
        }
        p.hit(l.gutter, Target::Gutter(vi));

        // ---- text area
        let ta = l.text;
        p.clip(ta, |p| {
            for (i, r) in st.rows.iter().enumerate() {
                let y = ta.y + i as f32 * lh;
                let bg = match r.diff { 1 => th.diff_add, 2 => th.diff_del, 3 => th.diff_chg, _ if r.cur => th.curline, _ => Color::TRANSPARENT };
                p.rect(R::new(ta.x, y, ta.w, lh), bg);
                for s in &r.rects {
                    let col = match s.kind { 0 => th.sel, 1 => th.mark, 2 => th.bracket, 4 => hex(0xe02020), 5 => hexa(0x4080ff38), _ => hexa(0xffa50060) };
                    let (yy, hh) = if s.kind == 4 { (y + lh - 2.0, 1.5) } else { (y, lh) };
                    p.rect(R::new(ta.x + s.x, yy, s.w, hh), col);
                }
                for t in &r.toks {
                    let (b, it) = (t.bold && c.mono_bold, t.italic && c.mono_italic);
                    let f = if b || it { mono.clone().bold(b).italic(it) } else { mono.clone() };
                    p.text(ta.x + t.x, y + c.text_dy, &t.text, &f, t.color);
                }
                if r.fold == 2 { p.rect(R::new(ta.x, y + lh - 1.0, ta.w, 1.0), th.gutter_fg); }
            }
            p.hit(ta, Target::Editor(vi));

            // ---- carets (primary blinks; secondary and overwrite carets are static)
            if focused {
                for (ci, k) in st.carets.iter().enumerate() {
                    let primary_blink = ci == 0 && !c.overwrite && !c.caret_idle;
                    if primary_blink && (!c.blink_on || !c.preedit.is_empty()) { continue; }
                    if c.overwrite {
                        p.rect(R::new(ta.x + k.x, ta.y + k.y, c.cell_w, lh), if dark { hexa(0xffffff70) } else { hexa(0x00000060) });
                    } else {
                        p.rect(R::new(ta.x + k.x, ta.y + k.y, 2.0, lh), th.fg);
                    }
                }
                // ---- IME composition, drawn inline at the primary caret
                if let (Some(k), false) = (st.carets.first(), c.preedit.is_empty()) {
                    let w = p.measure(c.preedit, &mono);
                    let r = R::new(ta.x + k.x, ta.y + k.y, w + 2.0, lh);
                    p.border(r, if dark { hex(0x1e1e1e) } else { hex(0xffffff) }, 1.0, th.gutter_fg);
                    p.text(r.x + 1.0, r.y + c.text_dy, c.preedit, &mono, th.fg);
                    p.rect(R::new(r.x + 1.0, r.bottom() - 3.0, w, 1.0), th.fg);
                    let cut = c.preedit_cursor.map_or(c.preedit.len(), |(_, e)| e.min(c.preedit.len()));
                    let cut = (0..=cut).rev().find(|&b| c.preedit.is_char_boundary(b)).unwrap_or(0);
                    let cx = p.measure(&c.preedit[..cut], &mono);
                    if c.blink_on || c.caret_idle { p.rect(R::new(r.x + 1.0 + cx, r.y, 1.0, lh), th.fg); }
                }
            }

            // ---- autocomplete popup
            if st.ac_visible {
                if let Some(k) = st.carets.first() {
                    let n = st.ac_items.len().min(10);
                    let h = n as f32 * 20.0 + 4.0;
                    let x = k.x.min(ta.w - 240.0);
                    let y = if k.y + lh + h < ta.h { k.y + lh } else { k.y - h };
                    let r = R::new(ta.x + x, ta.y + y, 240.0, h);
                    p.shadow(r);
                    p.border(r, if dark { hex(0x252526) } else { hex(0xffffff) }, 1.0, th.gutter_fg);
                    p.hit(r, Target::Absorb);
                    let f = Font::new(&c.mono, 13.0, 18.0);
                    for (i, it) in st.ac_items.iter().take(10).enumerate() {
                        let row = R::new(r.x + 2.0, r.y + 2.0 + i as f32 * 20.0, r.w - 4.0, 20.0);
                        if i as i32 == st.ac_sel { p.rect(row, if dark { hex(0x094771) } else { hex(0xcce4f7) }); }
                        p.text_elided(R::new(row.x + 6.0, row.y, row.w - 8.0, row.h), it, &f, th.fg);
                        p.hit(row, Target::AcItem(vi, i));
                    }
                }
            }
        });

        // ---- scrollbars
        let track = if dark { hex(0x252526) } else { hex(0xf0f0f0) };
        let thumb = if dark { hex(0x5a5a5a) } else { hex(0xc2c2c2) };
        let pressed = if dark { hex(0x8a8a8a) } else { hex(0x8f8f8f) };
        p.rect(l.vbar, track);
        let (ty, tlen) = vthumb(l.vbar, st);
        p.round(R::new(l.vbar.x + 2.0, ty, 8.0, tlen), if c.pressed == Some(&Target::VBar(vi)) { pressed } else { thumb }, 4.0);
        p.hit(l.vbar, Target::VBar(vi));
        if let Some(hb) = l.hbar {
            p.rect(hb, track);
            let (tx, tw) = hthumb(hb, st);
            p.round(R::new(tx, hb.y + 2.0, tw, 8.0), if c.pressed == Some(&Target::HBar(vi)) { pressed } else { thumb }, 4.0);
            p.hit(hb, Target::HBar(vi));
        }
    });

    // ---- document map
    if let Some(m) = l.map {
        p.clip(m, |p| {
            p.rect(m, if dark { hex(0x252526) } else { hex(0xf7f7f7) });
            p.rect(R::new(m.x, m.y, 1.0, m.h), th.gutter_fg);
            for ml in &st.map_lines { p.rect(R::new(m.x + 4.0 + ml.x, m.y + ml.y, ml.w, 1.0), ml.color); }
            p.border(R::new(m.x, m.y + st.map_view_y, m.w, st.map_view_h.max(4.0)),
                     if dark { hexa(0xffffff18) } else { hexa(0x0000ff14) }, 1.0, if dark { hexa(0xffffff40) } else { hexa(0x2a6fc980) });
            p.hit(m, Target::Map(vi));
        });
    }
}
