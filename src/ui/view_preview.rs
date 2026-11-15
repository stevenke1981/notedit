//! HTML / Markdown preview pane (port of app.slint): word-wrapped blocks in a
//! wheel-scrolled viewport.
use super::hit::{Target, R};
use super::paint::Painter;
use super::shell::Ctx;
use super::text::Font;
use super::types::*;

const HEAD: f32 = 22.0;
const PAD: f32 = 12.0;
const GAP: f32 = 6.0;

fn block_font(c: &Ctx, b: &Block) -> Font {
    let size = match b.kind { 1 => 26.0, 2 => 21.0, 3 => 18.0, 4..=6 => 15.0, _ => 14.0 };
    let bold = ((1..=6).contains(&b.kind) || b.bold) && c.ui_bold;
    let italic = b.italic && c.ui_italic;
    let family = if b.kind == 10 || b.code { c.mono.as_str() } else { c.ui_family };
    Font::new(family, size, (size * 1.4).round()).bold(bold).italic(italic)
}

/// Paint the pane; returns the scrollable content height.
pub fn paint(p: &mut Painter, c: &Ctx, r: R) -> f32 {
    p.rect(r, hex(0xfbfbfb));
    p.hit(r, Target::Preview);
    let head = R::new(r.x, r.y, r.w, HEAD);
    p.rect(head, hex(0xe9eef5));
    let f = c.ui_font(13.0).bold(true);
    p.text_elided(R::new(head.x + 6.0, head.y, head.w - 8.0, head.h), &format!("預覽：{}", c.ui.get_preview_title()), &f, hex(0x222222));
    let view = R::new(r.x, head.bottom(), r.w, (r.h - HEAD).max(0.0));
    let blocks = c.ui.preview_blocks_ref();
    let inner_w = (view.w - 2.0 * PAD).max(40.0);
    // measure every block, then paint the visible ones
    let mut layout = Vec::with_capacity(blocks.len());
    let mut y = PAD;
    for b in blocks.iter() {
        let font = block_font(c, b);
        let indent = b.indent as f32 * 18.0 + if b.kind == 10 { 8.0 } else { 0.0 } + if b.kind == 12 { 14.0 } else { 0.0 };
        let tw = (inner_w - indent - if b.kind == 12 { 9.0 } else { 0.0 }).max(20.0);
        let (pad_v, h) = match b.kind {
            11 => (0.0, 1.0),
            10 => (6.0, p.cache.layout(p.fonts, &b.text, &font, None, p.scale).height.max(font.line_h)),
            _ => (0.0, p.wrapped_height(tw, &b.text, &font)),
        };
        layout.push((y, h + 2.0 * pad_v, indent, tw, pad_v, font));
        y += h + 2.0 * pad_v + GAP;
    }
    let content = y - GAP + PAD;
    let scroll = c.scroll.preview.min((content - view.h).max(0.0));
    p.clip(view, |p| {
        for (b, (by, bh, indent, tw, pad_v, font)) in blocks.iter().zip(layout) {
            let top = view.y + by - scroll;
            if top + bh < view.y { continue; }
            if top > view.bottom() { break; }
            let x0 = view.x + PAD;
            match b.kind {
                10 => p.border(R::new(x0, top, inner_w, bh), hex(0xf0f0f0), 1.0, hex(0xdddddd)),
                12 => p.rect(R::new(x0, top, inner_w, bh), hex(0xf5f5f5)),
                _ => {}
            }
            let tx = x0 + indent;
            match b.kind {
                11 => p.rect(R::new(tx, top, inner_w - indent, 1.0), hex(0xcccccc)),
                12 => {
                    p.rect(R::new(tx, top, 3.0, bh), hex(0xbbbbbb));
                    p.text_wrapped(tx + 9.0, top, tw, &b.text, &font, hex(0x555555));
                }
                10 => { p.cache.draw(p.scene, p.fonts, p.atlas, &b.text, &font, tx, top + pad_v, hex(0x222222), p.scale, None); }
                _ => { p.text_wrapped(tx, top, tw, &b.text, &font, if b.link { hex(0x0645ad) } else { hex(0x222222) }); }
            }
        }
    });
    content
}
