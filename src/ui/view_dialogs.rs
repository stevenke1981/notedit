//! Dialogs (port of app.slint): find/replace, find in files, prompt, goto,
//! about and rename. Non-modal like before; positions and sizes as in Slint.
use super::hit::{CheckId, DlgId, FieldId, R};
use super::paint::Painter;
use super::shell::Ctx;
use super::types::*;
use super::widgets::{self, button, checkbox, field, label, BTN_H, DLG_FG, FIELD_H, ROW_H};

const LABEL_W: f32 = 70.0;

/// "label： [field]" row; returns the field rect.
fn field_row(p: &mut Painter, c: &Ctx, x: f32, y: f32, w: f32, text: &str, d: DlgId, id: FieldId) -> R {
    label(p, c, R::new(x, y, LABEL_W, ROW_H), text, DLG_FG);
    let r = R::new(x + LABEL_W + 6.0, y + (ROW_H - FIELD_H) / 2.0, w - LABEL_W - 6.0, FIELD_H);
    field(p, c, r, d, id);
    r
}

fn find(p: &mut Painter, c: &Ctx, size: (f32, f32)) {
    let ui = c.ui;
    let rep = ui.get_replace_mode();
    let r = R::new(size.0 - 580.0, 90.0, 560.0, if rep { 236.0 } else { 196.0 });
    let a = widgets::dialog(p, c, r, if rep { "取代" } else { "尋找" }, DlgId::Find);
    let mut y = a.y;
    field_row(p, c, a.x, y, a.w, "尋找目標：", DlgId::Find, FieldId::Find);
    y += ROW_H + 6.0;
    if rep { field_row(p, c, a.x, y, a.w, "取代為：", DlgId::Find, FieldId::Replace); y += ROW_H + 6.0; }
    let mut x = a.x;
    x += checkbox(p, c, x, y, "大小寫須相符", ui.get_match_case(), CheckId::MatchCase, DLG_FG) + 12.0;
    x += checkbox(p, c, x, y, "全字拼寫須相符", ui.get_whole_word(), CheckId::WholeWord, DLG_FG) + 12.0;
    checkbox(p, c, x, y, "規則運算式", ui.get_use_regex(), CheckId::UseRegex, DLG_FG);
    y += 24.0 + 6.0;
    checkbox(p, c, a.x, y, "取代模式", rep, CheckId::ReplaceMode, DLG_FG);
    y += 24.0 + 6.0;
    let mut x = a.x;
    for (s, id) in [("找下一個", "find_next"), ("找上一個", "find_prev"), ("計數", "count"), ("在所有開啟文件中尋找", "find_all")] {
        x += button(p, c, x, y, s, id, true) + 4.0;
    }
    if rep {
        x += button(p, c, x, y, "取代", "replace_one", true) + 4.0;
        button(p, c, x, y, "全部取代", "replace_all", true);
    }
    y += BTN_H + 6.0;
    label(p, c, R::new(a.x, y, a.w, 18.0), &ui.get_find_status(), hex(0xaa3333));
}

fn fif(p: &mut Painter, c: &Ctx, size: (f32, f32)) {
    let ui = c.ui;
    let rep = ui.get_fif_replace();
    let running = ui.get_fif_running();
    let r = R::new(((size.0 - 640.0) / 2.0).round(), 90.0, 640.0, if rep { 276.0 } else { 236.0 });
    let a = widgets::dialog(p, c, r, if rep { "在檔案中取代" } else { "在檔案中尋找" }, DlgId::Fif);
    let mut y = a.y;
    field_row(p, c, a.x, y, a.w, "尋找目標：", DlgId::Fif, FieldId::Find);
    y += ROW_H + 6.0;
    if rep { field_row(p, c, a.x, y, a.w, "取代為：", DlgId::Fif, FieldId::Replace); y += ROW_H + 6.0; }
    let bw = widgets::button_width(p, c, "瀏覽...");
    field_row(p, c, a.x, y, a.w - bw - 6.0, "資料夾：", DlgId::Fif, FieldId::FifDir);
    button(p, c, a.right() - bw, y + (ROW_H - BTN_H) / 2.0, "瀏覽...", "fif_browse", true);
    y += ROW_H + 6.0;
    field_row(p, c, a.x, y, a.w, "檔案類型：", DlgId::Fif, FieldId::FifFilter);
    y += ROW_H + 6.0;
    let mut x = a.x;
    x += checkbox(p, c, x, y, "包含子資料夾", ui.get_fif_recursive(), CheckId::FifRecursive, DLG_FG) + 12.0;
    x += checkbox(p, c, x, y, "大小寫須相符", ui.get_match_case(), CheckId::MatchCase, DLG_FG) + 12.0;
    x += checkbox(p, c, x, y, "全字拼寫須相符", ui.get_whole_word(), CheckId::WholeWord, DLG_FG) + 12.0;
    checkbox(p, c, x, y, "規則運算式", ui.get_use_regex(), CheckId::UseRegex, DLG_FG);
    y += 24.0 + 6.0;
    let mut x = a.x;
    x += button(p, c, x, y, if rep { "預覽" } else { "全部尋找" }, "fif_run", !running) + 4.0;
    if rep {
        x += button(p, c, x, y, "全部取代...", "fif_replace_all", !running) + 4.0;
        x += button(p, c, x, y, "復原檔案取代", "fif_undo", true) + 4.0;
    }
    x += checkbox(p, c, x, y + 2.0, "取代", rep, CheckId::FifReplace, DLG_FG) + 8.0;
    x += button(p, c, x, y, "停止", "fif_stop", running) + 6.0;
    label(p, c, R::new(x, y, (a.right() - x).max(0.0), BTN_H), &ui.get_find_status(), hex(0xaa3333));
}

/// small dialog: label, one field, one button
#[allow(clippy::too_many_arguments)]
fn one_field(p: &mut Painter, c: &Ctx, size: (f32, f32), w: f32, y: f32, title: &str, text: &str, d: DlgId, id: FieldId, btn: &str, action: &'static str) {
    let r = R::new(((size.0 - w) / 2.0).round(), y, w, 130.0);
    let a = widgets::dialog(p, c, r, title, d);
    label(p, c, R::new(a.x, a.y, a.w, 18.0), text, DLG_FG);
    field(p, c, R::new(a.x, a.y + 24.0, a.w, FIELD_H), d, id);
    button(p, c, a.x, a.y + 24.0 + FIELD_H + 6.0, btn, action, true);
}

fn about(p: &mut Painter, c: &Ctx, size: (f32, f32)) {
    let r = R::new(((size.0 - 360.0) / 2.0).round(), 150.0, 360.0, 160.0);
    let a = widgets::dialog(p, c, r, "關於 NotEdit", DlgId::About);
    let big = c.ui_font(18.0).bold(true);
    p.text(a.x + 4.0, a.y - 4.0, &format!("NotEdit {}", c.ui.get_version()), &big, DLG_FG);
    label(p, c, R::new(a.x + 4.0, a.y + 26.0, a.w, 18.0), "以 Rust + AcmeUI 製作的類 Notepad++ 文字編輯器", DLG_FG);
    label(p, c, R::new(a.x + 4.0, a.y + 48.0, a.w, 18.0), "支援 HTML / Markdown 預覽", DLG_FG);
    button(p, c, a.x + 4.0, a.y + 74.0, "確定", "about_ok", true);
}

/// Dialogs below the context menu (all but rename).
pub fn paint(p: &mut Painter, c: &Ctx, size: (f32, f32)) {
    let ui = c.ui;
    if ui.get_find_visible() { find(p, c, size); }
    if ui.get_fif_visible() { fif(p, c, size); }
    if ui.get_prompt_visible() {
        let (title, text) = (ui.get_prompt_title(), ui.get_prompt_label());
        one_field(p, c, size, 380.0, 120.0, &title, &text, DlgId::Prompt, FieldId::Prompt, "確定", "prompt_ok");
    }
    if ui.get_goto_visible() {
        let text = format!("行號 (1 - {})：", ui.get_total_lines());
        one_field(p, c, size, 320.0, 120.0, "跳至...", &text, DlgId::Goto, FieldId::Goto, "跳至", "goto_ok");
    }
    if ui.get_about_visible() { about(p, c, size); }
}

/// The rename dialog sits above the context menu (app.slint order).
pub fn paint_rename(p: &mut Painter, c: &Ctx, size: (f32, f32)) {
    if c.ui.get_rename_visible() {
        one_field(p, c, size, 380.0, 140.0, "重新命名", "新的檔名：", DlgId::Rename, FieldId::Rename, "確定", "rename_ok");
    }
}
