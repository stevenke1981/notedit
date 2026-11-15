//! Popups (port of app.slint): menu dropdown, context menu and the option list
//! of a combo box (gap G4).
use super::hit::{Target, R};
use super::paint::Painter;
use super::shell::Ctx;
use super::types::*;

fn items_height(items: &[MenuItem]) -> f32 { items.iter().map(|i| if i.separator { 7.0 } else { 22.0 }).sum::<f32>() + 4.0 }

fn bg(dark: bool) -> Color { if dark { hex(0x2d2d30) } else { hex(0xf9f9f9) } }

/// Menu dropdown below title `titles[open]`.
pub fn dropdown(p: &mut Painter, c: &Ctx, size: (f32, f32), titles: &[R]) {
    let open = c.ui.get_open_menu();
    if open < 0 { return; }
    let menus = c.ui.menus_ref();
    let Some(m) = menus.get(open as usize) else { return };
    let th = p.theme;
    // click-away layer; menu titles stay live above it so another title switches menus
    p.hit(R::new(0.0, 0.0, size.0, size.1), Target::Backdrop);
    for (i, t) in titles.iter().enumerate() { p.hit(*t, Target::MenuTitle(i)); }
    let x = titles.get(open as usize).map(|t| t.x).unwrap_or(2.0).min(size.0 - 300.0).max(0.0);
    let r = R::new(x, 22.0, 300.0, items_height(&m.items));
    p.shadow(r);
    p.border(r, bg(c.dark), 1.0, th.border);
    p.hit(r, Target::Absorb);
    let f = c.ui_font(13.0);
    let mut y = r.y + 2.0;
    for it in &m.items {
        if it.separator {
            p.rect(R::new(r.x + 2.0, y + 3.0, r.w - 4.0, 1.0), hex(0xcccccc));
            y += 7.0;
            continue;
        }
        let row = R::new(r.x + 2.0, y, r.w - 4.0, 22.0);
        let t = Target::MenuItem(it.action.clone());
        if it.enabled {
            if c.hover == Some(&t) { p.rect(row, th.hover); }
            p.hit(row, t);
        }
        if it.checked { p.text(row.x + 4.0, y + ((22.0 - f.line_h) / 2.0).round(), "✓", &f, th.fg); }
        let sw = if it.shortcut.is_empty() { 0.0 } else { p.measure(&it.shortcut, &f) };
        p.text_elided(R::new(row.x + 24.0, y, row.w - 24.0 - 10.0 - sw - 8.0, 22.0), &it.label, &f, if it.enabled { th.fg } else { th.gutter_fg });
        if sw > 0.0 { p.text(row.right() - 10.0 - sw, y + ((22.0 - f.line_h) / 2.0).round(), &it.shortcut, &f, th.gutter_fg); }
        y += 22.0;
    }
}

pub fn context(p: &mut Painter, c: &Ctx, size: (f32, f32)) {
    if !c.ui.get_ctx_visible() { return; }
    let items = c.ui.ctx_items_ref();
    let th = p.theme;
    p.hit(R::new(0.0, 0.0, size.0, size.1), Target::Backdrop);
    let h = items_height(&items);
    let x = c.ui.get_ctx_x().min(size.0 - 250.0 - 2.0).max(0.0);
    let y = c.ui.get_ctx_y().min(size.1 - h - 2.0).max(0.0);
    let r = R::new(x, y, 250.0, h);
    p.shadow(r);
    p.border(r, bg(c.dark), 1.0, th.border);
    p.hit(r, Target::Absorb);
    let f = c.ui_font(13.0);
    let mut yy = r.y + 2.0;
    for it in items.iter() {
        if it.separator {
            p.rect(R::new(r.x + 2.0, yy + 3.0, r.w - 4.0, 1.0), th.border);
            yy += 7.0;
            continue;
        }
        let row = R::new(r.x + 2.0, yy, r.w - 4.0, 22.0);
        let t = Target::CtxItem(it.action.clone());
        if it.enabled {
            if c.hover == Some(&t) { p.rect(row, th.hover); }
            p.hit(row, t);
        }
        let sw = if it.shortcut.is_empty() { 0.0 } else { p.measure(&it.shortcut, &f) };
        p.text_elided(R::new(row.x + 24.0, yy, row.w - 24.0 - 10.0 - sw - 8.0, 22.0), &it.label, &f, if it.enabled { th.fg } else { th.gutter_fg });
        if sw > 0.0 { p.text(row.right() - 10.0 - sw, yy + ((22.0 - f.line_h) / 2.0).round(), &it.shortcut, &f, th.gutter_fg); }
        yy += 22.0;
    }
}

/// Open combo box option list: (combo id, anchor rect, options, current index).
pub fn combo(p: &mut Painter, c: &Ctx, size: (f32, f32), open: Option<(u8, R, &[&str], i32)>) {
    let Some((id, anchor, options, cur)) = open else { return };
    let th = p.theme;
    p.hit(R::new(0.0, 0.0, size.0, size.1), Target::Backdrop);
    let h = options.len() as f32 * 22.0 + 4.0;
    let y = if anchor.bottom() + h <= size.1 { anchor.bottom() } else { (anchor.y - h).max(0.0) };
    let r = R::new(anchor.x, y, anchor.w.max(160.0), h);
    p.shadow(r);
    p.border(r, bg(c.dark), 1.0, th.border);
    p.hit(r, Target::Absorb);
    let f = c.ui_font(13.0);
    for (i, o) in options.iter().enumerate() {
        let row = R::new(r.x + 2.0, r.y + 2.0 + i as f32 * 22.0, r.w - 4.0, 22.0);
        let t = Target::ComboItem(id, i);
        if c.hover == Some(&t) || i as i32 == cur { p.rect(row, th.hover); }
        p.text_elided(R::new(row.x + 8.0, row.y, row.w - 12.0, row.h), o, &f, th.fg);
        p.hit(row, t);
    }
}
