//! AI assistant side panel — M6.
use super::hit::R;
use super::paint::Painter;
use super::shell::Ctx;

pub fn paint(p: &mut Painter, _c: &Ctx, r: R) { let th = p.theme; p.rect(r, th.editor_bg); }
