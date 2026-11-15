//! AI assistant side panel (port of app.slint). Gap G3: AcmeUI has no
//! multi-line editor, so the conversation and the reply are read-only wrapped
//! text in wheel-scrolled boxes and the prompt is a single-line field (Enter
//! sends). Gap G4: combo boxes open the shell's option popup.
use super::hit::{CheckId, DlgId, FieldId, Target, R};
use super::paint::Painter;
use super::shell::{Ctx, COMBO_LANG, COMBO_MODE, COMBO_ROLE, LANGS, MODES, ROLES};
use super::text::Font;
use super::types::*;
use super::widgets::{button, checkbox, combo, field, FIELD_H};

const PAD: f32 = 8.0;
const GAP: f32 = 6.0;

fn label(p: &mut Painter, c: &Ctx, x: f32, y: f32, w: f32, s: &str, color: Color) -> f32 {
    let f = c.ui_font(13.0);
    let (_, h) = p.text_wrapped(x, y, w, s, &f, color);
    h.max(f.line_h)
}

fn label_h(p: &mut Painter, c: &Ctx, w: f32, s: &str) -> f32 { p.wrapped_height(w, s, &c.ui_font(13.0)) }

/// Read-only wrapped text in a bordered, wheel-scrolled box; returns the scroll range.
fn text_box(p: &mut Painter, c: &Ctx, r: R, text: &str, placeholder: &str, font: &Font) -> f32 {
    let th = p.theme;
    p.border_r(r, if c.dark { hex(0x1e1e1e) } else { hex(0xffffff) }, 1.0, th.border, 3.0);
    p.hit(r, Target::AiScroll);
    let inner = R::new(r.x + 6.0, r.y + 4.0, r.w - 12.0, r.h - 8.0);
    if text.is_empty() {
        p.text_wrapped(inner.x, inner.y, inner.w, placeholder, &c.ui_font(13.0), th.gutter_fg);
        return 0.0;
    }
    let h = p.wrapped_height(inner.w, text, font);
    let max = (h - inner.h).max(0.0);
    let scroll = c.scroll.ai.min(max);
    p.clip(inner, |p| { p.text_wrapped(inner.x, inner.y - scroll, inner.w, text, font, th.fg); });
    max
}

/// Paint the panel; returns the scroll range of its text box.
pub fn paint(p: &mut Painter, c: &Ctx, r: R) -> f32 {
    let th = p.theme;
    let ui = c.ui;
    p.rect(r, th.editor_bg);
    p.hit(r, Target::Absorb);
    let page = ui.get_ai_page();
    let running = ui.get_ai_running();
    // ---- header
    let head = R::new(r.x, r.y, r.w, 28.0);
    p.rect(head, th.chrome);
    let f = c.ui_font(13.0);
    for (i, s) in ["對話", "結果", "設定"].iter().enumerate() {
        let tr = R::new(head.x + 5.0 + i as f32 * 66.0, head.y, 64.0, head.h);
        let t = Target::AiPage(i as i32);
        if page == i as i32 { p.rect(tr, th.tab_active); } else if c.hover == Some(&t) { p.rect(tr, th.hover); }
        p.text_centered(tr, s, &f.clone().bold(page == i as i32 && c.ui_bold), th.fg);
        p.hit(tr, t);
    }
    let close = R::new(head.right() - 28.0, head.y, 24.0, head.h);
    if c.hover == Some(&Target::AiClose) { p.rect(close, th.hover); }
    p.text_centered(close, "✕", &f, th.fg);
    p.hit(close, Target::AiClose);

    let x = r.x + PAD;
    let w = r.w - 2.0 * PAD;
    let mut y = head.bottom() + PAD;
    let bottom = r.bottom() - PAD;
    let mut range = 0.0;
    match page {
        0 => {
            let role = ui.get_ai_role();
            label(p, c, x, y + 5.0, 60.0, "角色", th.fg);
            combo(p, c, R::new(x + 40.0, y, w - 40.0, FIELD_H), ROLES[role.clamp(0, ROLES.len() as i32 - 1) as usize], COMBO_ROLE, true);
            y += FIELD_H + GAP;
            if role == 2 {
                let lang = ui.get_ai_language();
                label(p, c, x, y + 5.0, 70.0, "翻譯語言", th.fg);
                combo(p, c, R::new(x + 66.0, y, w - 66.0, FIELD_H), LANGS[lang.clamp(0, LANGS.len() as i32 - 1) as usize], COMBO_LANG, true);
                y += FIELD_H + GAP;
                if lang == 11 { field(p, c, R::new(x, y, w, FIELD_H), DlgId::Ai, FieldId::AiLangCustom); y += FIELD_H + GAP; }
            }
            y += label(p, c, x, y, w, "對話紀錄", th.fg) + 2.0;
            // fixed part below the history box
            let note = "先前對話會傳送至 OpenRouter；自動修改會傳送目前文件。";
            let status = ui.get_ai_status();
            let below = 18.0 + 2.0 + FIELD_H + GAP + 24.0 + GAP + FIELD_H + GAP + label_h(p, c, w, note) + GAP + 28.0 + GAP
                + if status.is_empty() { 0.0 } else { label_h(p, c, w, &status) };
            let box_h = (bottom - y - GAP - below).max(40.0);
            let mono = Font::new(c.ui_family, 13.0, 18.0);
            range = text_box(p, c, R::new(x, y, w, box_h), &ui.get_ai_history(), "在下方輸入需求，開始與 AI 對話。", &mono);
            y += box_h + GAP;
            y += label(p, c, x, y, w, "指令（Enter 送出）", th.fg) + 2.0;
            field(p, c, R::new(x, y, w, FIELD_H), DlgId::Ai, FieldId::AiPrompt);
            y += FIELD_H + GAP;
            checkbox(p, c, x, y, "傳送目前文件內容", ui.get_ai_include_document(), CheckId::AiIncludeDoc, th.fg);
            y += 24.0 + GAP;
            let mode = ui.get_ai_mode();
            combo(p, c, R::new(x, y, w, FIELD_H), MODES[mode.clamp(0, MODES.len() as i32 - 1) as usize], COMBO_MODE, !running);
            y += FIELD_H + GAP;
            y += label(p, c, x, y, w, note, th.gutter_fg) + GAP;
            let bx = x + button(p, c, x, y, "送出", "ai_send", !running) + 6.0;
            button(p, c, bx, y, "清除對話", "ai_clear", !running);
            y += 28.0 + GAP;
            if !status.is_empty() { label(p, c, x, y, w, &status, th.fg); }
        }
        1 => {
            y += label(p, c, x, y, w, "回覆預覽（唯讀；套用後可在編輯器中修改）", th.fg) + 2.0;
            let status = ui.get_ai_status();
            let below = 28.0 + GAP + if status.is_empty() { 0.0 } else { label_h(p, c, w, &status) };
            let box_h = (bottom - y - GAP - below).max(40.0);
            let mono = Font::new(&c.mono, 13.0, 18.0);
            let result = ui.get_ai_result();
            range = text_box(p, c, R::new(x, y, w, box_h), &result, "", &mono);
            y += box_h + GAP;
            let ok = !running && !result.is_empty();
            let bx = x + button(p, c, x, y, "套用至目前文件", "ai_apply", ok) + 6.0;
            button(p, c, bx, y, "建立新文件", "ai_apply_new", ok);
            y += 28.0 + GAP;
            if !status.is_empty() { label(p, c, x, y, w, &status, th.fg); }
        }
        _ => {
            y += label(p, c, x, y, w, "OpenRouter API 金鑰", th.fg) + GAP;
            field(p, c, R::new(x, y, w, FIELD_H), DlgId::Ai, FieldId::AiKey);
            y += FIELD_H + GAP;
            let mut bx = x;
            for (s, id) in [("儲存金鑰", "ai_save_key"), ("本次使用", "ai_use_key"), ("刪除已存金鑰", "ai_clear_key")] {
                bx += button(p, c, bx, y, s, id, true) + 6.0;
            }
            y += 28.0 + GAP;
            let ks = ui.get_ai_key_status();
            if !ks.is_empty() { y += label(p, c, x, y, w, &ks, th.fg) + GAP; }
            y += label(p, c, x, y, w, "Windows 使用目前帳戶加密保存；儲存後才會套用。", th.gutter_fg) + GAP + 2.0;
            y += label(p, c, x, y, w, "免費模型", th.fg) + GAP;
            field(p, c, R::new(x, y, w, FIELD_H), DlgId::Ai, FieldId::AiModel);
            y += FIELD_H + GAP;
            label(p, c, x, y, w, "僅支援 openrouter/free 或以 :free 結尾的模型。", th.gutter_fg);
        }
    }
    range
}
