//! Sidebar (folder workspace / function list) and the search-hits panel — M5.
use super::hit::R;
use super::paint::Painter;
use super::shell::Ctx;

pub fn sidebar(p: &mut Painter, _c: &Ctx, r: R) { let th = p.theme; p.rect(r, th.editor_bg); }

pub fn hits(p: &mut Painter, _c: &Ctx, r: R) { let th = p.theme; p.border(r, th.editor_bg, 1.0, th.border); }
