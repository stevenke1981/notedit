//! HTML / Markdown preview pane — M5.
use super::hit::R;
use super::paint::Painter;
use super::shell::Ctx;
use super::types::hex;

pub fn paint(p: &mut Painter, _c: &Ctx, r: R) { p.rect(r, hex(0xfbfbfb)); }
