//! Sidebar (folder workspace / function list) and the search-hits panel
//! (port of app.slint). Lists use fixed 20 px rows, wheel scrolling and paint
//! only the visible rows (gap G4: no ListView).
use super::hit::{Target, R};
use super::paint::Painter;
use super::shell::Ctx;
use super::text::Font;
use super::types::*;

pub const ROW: f32 = 20.0;
pub const SIDE_HEAD: f32 = 24.0 + 20.0;
pub const HITS_HEAD: f32 = 22.0;

/// Scroll range of a list with `n` rows in a pane `h` high.
pub fn max_scroll(n: usize, h: f32) -> f32 { (n as f32 * ROW - h).max(0.0) }

pub fn sidebar(p: &mut Painter, c: &Ctx, r: R) {
    let th = p.theme;
    let ui = c.ui;
    let mode = ui.get_side_mode();
    p.rect(r, th.editor_bg);
    p.hit(r, Target::Absorb);
    // ---- header: mode tabs, open-folder, close
    let head = R::new(r.x, r.y, r.w, 24.0);
    p.rect(head, th.chrome);
    let f = c.ui_font(13.0);
    let mut x = head.x + 2.0;
    for (i, t) in ["資料夾", "函式清單"].iter().enumerate() {
        let m = i as i32 + 1;
        let tr = R::new(x, head.y, (p.measure(t, &f) + 14.0).round(), head.h);
        let tg = Target::SideTab(m);
        if mode == m { p.rect(tr, th.tab_active); } else if c.hover == Some(&tg) { p.rect(tr, th.hover); }
        p.text_centered(tr, t, &f.clone().bold(mode == m), th.fg);
        p.hit(tr, tg);
        x = tr.right() + 2.0;
    }
    let close = R::new(head.right() - 22.0, head.y, 20.0, head.h);
    if c.hover == Some(&Target::SideTab(0)) { p.rect(close, th.hover); }
    p.text_centered(close, "✕", &f, th.fg);
    p.hit(close, Target::SideTab(0));
    if mode == 1 {
        let open = R::new(close.x - 24.0, head.y, 22.0, head.h);
        if c.hover == Some(&Target::SideFolder) { p.rect(open, th.hover); }
        p.text_centered(open, "📂", &f, th.fg);
        p.hit(open, Target::SideFolder);
    }
    let title = R::new(r.x, r.y + 24.0, r.w, 20.0);
    p.rect(title, th.chrome);
    p.text_elided(R::new(title.x + 6.0, title.y, title.w - 8.0, title.h), &ui.get_side_title(), &c.ui_font(12.0), th.gutter_fg);
    // ---- rows
    let list = R::new(r.x, r.y + SIDE_HEAD, r.w, (r.h - SIDE_HEAD).max(0.0));
    let items = ui.side_items_ref();
    let sel = ui.get_side_sel();
    let scroll = c.scroll.side.min(max_scroll(items.len(), list.h));
    let mono = Font::new(&c.mono, 13.0, 18.0);
    let small = c.ui_font(11.0);
    p.clip(list, |p| {
        p.hit(list, Target::SideList);
        let first = (scroll / ROW) as usize;
        for (i, it) in items.iter().enumerate().skip(first) {
            let y = list.y + i as f32 * ROW - scroll;
            if y > list.bottom() { break; }
            let row = R::new(list.x, y, list.w, ROW);
            let t = Target::SideRow(i);
            if i as i32 == sel { p.rect(row, th.curline); } else if c.hover == Some(&t) { p.rect(row, th.hover); }
            let mut x = row.x + 4.0 + it.indent as f32 * 14.0;
            let arrow = match it.kind { 1 => "▸", 2 => "▾", _ => "" };
            if !arrow.is_empty() { p.text(x, y + ((ROW - f.line_h) / 2.0).round(), arrow, &f, th.gutter_fg); }
            x += 12.0 + 4.0;
            let (icon, col) = match it.kind { 0 => ("📄", th.gutter_fg), 3 => ("ƒ", hex(0xa35cc6)), 4 => ("#", th.gutter_fg), _ => ("📁", th.gutter_fg) };
            p.text(x, y + ((ROW - small.line_h) / 2.0).round(), icon, &small, col);
            x += 14.0 + 4.0;
            let tf = if it.kind >= 3 { &mono } else { &f };
            p.text_elided(R::new(x, y, (row.right() - x - 2.0).max(0.0), ROW), &it.text, tf, th.fg);
            p.hit(row, t);
        }
    });
}

pub fn hits(p: &mut Painter, c: &Ctx, r: R) {
    let th = p.theme;
    let ui = c.ui;
    p.border(r, th.editor_bg, 1.0, th.border);
    p.hit(r, Target::Absorb);
    let head = R::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, HITS_HEAD);
    p.rect(head, th.chrome);
    let f = c.ui_font(13.0);
    p.text_elided(R::new(head.x + 6.0, head.y, head.w - 34.0, head.h), &ui.get_hits_title(), &f.clone().bold(true), th.fg);
    let close = R::new(head.right() - 24.0, head.y, 20.0, head.h);
    if c.hover == Some(&Target::HitsClose) { p.rect(close, th.hover); }
    p.text_centered(close, "✕", &f, th.fg);
    p.hit(close, Target::HitsClose);
    let list = R::new(r.x + 1.0, head.bottom(), r.w - 2.0, (r.bottom() - 1.0 - head.bottom()).max(0.0));
    let hits = ui.hits_ref();
    let scroll = c.scroll.hits.min(max_scroll(hits.len(), list.h));
    let mono = Font::new(&c.mono, 13.0, 18.0);
    p.clip(list, |p| {
        p.hit(list, Target::HitsList);
        let first = (scroll / ROW) as usize;
        for (i, h) in hits.iter().enumerate().skip(first) {
            let y = list.y + i as f32 * ROW - scroll;
            if y > list.bottom() { break; }
            let row = R::new(list.x, y, list.w, ROW);
            let t = Target::Hit(i);
            if c.hover == Some(&t) { p.rect(row, th.hover); }
            let ty = y + ((ROW - f.line_h) / 2.0).round();
            let mut x = row.x + 8.0;
            x += p.text(x, ty, &h.title, &f, hex(0x2a7a2a)) + 8.0;
            p.text(x, ty, &format!("第 {} 行", h.line), &f, hex(0xa06000));
            x += 70.0 + 8.0;
            p.text_elided(R::new(x, y, (row.right() - x - 4.0).max(0.0), ROW), &h.text, &mono, th.fg);
            p.hit(row, t);
        }
    });
}
