//! Dialogs (find/replace, find in files, prompt, goto, about, rename) — M4.
use super::paint::Painter;
use super::shell::Ctx;

/// Dialogs below the context menu (all but rename).
pub fn paint(_p: &mut Painter, _c: &Ctx, _size: (f32, f32)) {}

/// The rename dialog sits above the context menu (app.slint order).
pub fn paint_rename(_p: &mut Painter, _c: &Ctx, _size: (f32, f32)) {}
