//! MadEdit-style hex mode: offset gutter, 16 hex bytes per row and an ASCII column,
//! rendered through the regular EditorView.

use super::*;
use crate::hex::{HexBuf, ROW};

pub struct HexState {
    pub buf: HexBuf,
    pub cur: u64,
    pub nib: u8,
    pub ascii: bool,
    pub top: u64,
    pub anchor: Option<u64>,
    pub insert: bool,
}

impl HexState {
    pub fn new(buf: HexBuf) -> Self { HexState { buf, cur: 0, nib: 0, ascii: false, top: 0, anchor: None, insert: false } }
    fn rows(&self) -> u64 { self.buf.len() / ROW as u64 + 1 }
    fn sel(&self) -> Option<(u64, u64)> {
        let a = self.anchor?;
        (a != self.cur).then(|| (a.min(self.cur), a.max(self.cur)))
    }
}

/// cell column of byte `j` in the hex area
fn hex_col(j: usize) -> usize { j * 3 + usize::from(j >= 8) }
const ASCII_COL: usize = 16 * 3 + 2;

impl App {
    pub(crate) fn is_hex(&self, vi: usize) -> bool {
        let st = self.st.borrow();
        st.views.get(vi).map(|v| st.docs[v.doc].hex.is_some()).unwrap_or(false)
    }

    fn hex_vr(&self, vi: usize) -> u64 { let (_, lh) = self.metrics(); let (_, h) = self.view_size(vi); ((h / lh).floor() as u64).max(1) }

    fn hex_follow(&self, vi: usize) {
        let vr = self.hex_vr(vi);
        let mut st = self.st.borrow_mut();
        let di = st.views[vi].doc;
        let Some(h) = st.docs[di].hex.as_mut() else { return };
        h.cur = h.cur.min(h.buf.len());
        let r = h.cur / ROW as u64;
        if r < h.top { h.top = r; }
        if r >= h.top + vr { h.top = r + 1 - vr; }
    }

    pub(crate) fn hex_render(&self, vi: usize) {
        let ui = self.ui();
        let (cw, lh) = self.metrics();
        let (w, hgt) = self.view_size(vi);
        let vr = ((hgt / lh).ceil() as u64).max(1);
        let st = self.st.borrow();
        let dark = st.dark;
        let active = st.view_idx() == vi;
        let d = &st.docs[st.views[vi].doc];
        let h = d.hex.as_ref().unwrap();
        let len = h.buf.len();
        let wide = len > 0xFFFF_FFFF;
        let fg = if dark { slint::Color::from_rgb_u8(0xdc, 0xdc, 0xdc) } else { slint::Color::from_rgb_u8(0x1e, 0x1e, 0x1e) };
        let dim = slint::Color::from_rgb_u8(0x9a, 0x9a, 0x9a);
        let hi = if dark { slint::Color::from_rgb_u8(0x6c, 0xb6, 0xff) } else { slint::Color::from_rgb_u8(0x00, 0x40, 0xc0) };
        let num_w = (if wide { 10.0 } else { 8.0 }) * cw + 4.0;
        let gutter_w = 16.0 + num_w + 16.0;
        let sel = h.sel();
        let cur_row = h.cur / ROW as u64;
        let mut rows = vec![];
        let mut carets = vec![];
        for i in 0..vr {
            let r = h.top + i;
            let off = r * ROW as u64;
            if off > len || (off == len && len > 0 && len % ROW as u64 == 0 && h.cur != len) { break; }
            let bytes = h.buf.read(off, ROW);
            let mut toks = vec![];
            let mut rects = vec![];
            let mut asc = String::new();
            for (j, &b) in bytes.iter().enumerate() {
                let color = if b == 0 { dim } else if (0x20..0x7f).contains(&b) { fg } else { hi };
                toks.push(EdTok { x: hex_col(j) as f32 * cw, text: format!("{b:02X}").into(), color, bold: false, italic: false });
                asc.push(crate::hex::ascii(b));
                let p = off + j as u64;
                if let Some((s, e)) = sel { if p >= s && p < e {
                    rects.push(EdRect { x: hex_col(j) as f32 * cw, w: if j == 15 || j == 7 { 2.0 * cw } else { 3.0 * cw }, kind: 0 });
                    rects.push(EdRect { x: (ASCII_COL + j) as f32 * cw, w: cw, kind: 0 });
                } }
            }
            if !asc.is_empty() { toks.push(EdTok { x: ASCII_COL as f32 * cw, text: asc.into(), color: fg, bold: false, italic: false }); }
            if r == cur_row {
                let j = (h.cur - off) as usize;
                // caret in the active column, box in the other one
                let (cx, bx, bw) = if h.ascii { ((ASCII_COL + j) as f32 * cw, hex_col(j) as f32 * cw, 2.0 * cw) }
                                   else { ((hex_col(j) + h.nib as usize) as f32 * cw, (ASCII_COL + j) as f32 * cw, cw) };
                rects.push(EdRect { x: bx, w: bw, kind: 5 });
                carets.push(EdCaret { x: cx, y: i as f32 * lh });
            }
            rows.push(EdRow { num: crate::hex::offset_label(off, wide).into(), line: r as i32, fold: 0, cur: r == cur_row, mark: false, diff: 0,
                              toks: ModelRc::new(VecModel::from(toks)), rects: ModelRc::new(VecModel::from(rects)) });
        }
        if !active { carets.clear(); }
        let nrows = h.rows();
        let thumb_size = (vr as f32 / (nrows + vr) as f32).clamp(0.02, 1.0);
        let thumb_pos = (h.top as f32 / nrows.saturating_sub(1).max(1) as f32).clamp(0.0, 1.0);
        let title = if st.split { format!("{}{} [HEX]", if d.modified { "● " } else { "" }, d.title) } else { String::new() };
        let _ = w;
        let state = EdState {
            rows: ModelRc::new(VecModel::from(rows)), carets: ModelRc::new(VecModel::from(carets)),
            gutter_w, num_w, thumb_pos, thumb_size, hthumb_pos: 0.0, hthumb_size: 1.0, hbar: false,
            ac_items: ModelRc::new(VecModel::from(Vec::<SharedString>::new())), ac_sel: 0, ac_visible: false,
            title: title.into(), map_lines: ModelRc::new(VecModel::from(Vec::<MapLine>::new())), map_view_y: 0.0, map_view_h: 0.0,
        };
        drop(st);
        if vi == 0 { ui.set_v0(state); } else { ui.set_v1(state); }
        self.rkey.borrow_mut()[vi] = (cw, lh, w, hgt);
    }

    fn hex_changed(&self, vi: usize) {
        { let mut st = self.st.borrow_mut(); let di = st.views[vi].doc; st.docs[di].modified = true; }
        self.hex_follow(vi);
        self.refresh_meta();
        self.render_all();
    }

    pub(crate) fn hex_status(&self) -> Option<String> {
        let st = self.st.borrow();
        let h = st.docs[st.views[st.view_idx()].doc].hex.as_ref()?;
        let mut s = format!("Off : 0x{:X}  [{}]", h.cur, h.buf.get(h.cur).map(|b| format!("{b:02X}")).unwrap_or("EOF".into()));
        if let Some((a, b)) = h.sel() { s += &format!("  Sel : {}", b - a); }
        s += if h.insert { "  插入" } else { "  覆寫" };
        if h.buf.is_mapped() { s += " mmap"; }
        Some(s)
    }

    pub(crate) fn hex_key(&self, vi: usize, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        let mut ch = text.chars();
        let (Some(k), None) = (ch.next(), ch.next()) else { return false };
        let vr = self.hex_vr(vi);
        let lk = k.to_ascii_lowercase();
        if alt && !ctrl && (lk == 'h' || lk == 'c') {
            let name = if lk == 'h' { "textmode" } else { "columnmode" };
            if let Some(me) = self.me.borrow().upgrade() { slint::Timer::single_shot(std::time::Duration::from_millis(0), move || me.action(name)); }
            return true;
        }
        if ctrl {
            match lk {
                'z' | 'y' => {
                    let ok = {
                        let mut st = self.st.borrow_mut(); let di = st.views[vi].doc;
                        let h = st.docs[di].hex.as_mut().unwrap();
                        let p = if lk == 'z' && !shift { h.buf.undo() } else { h.buf.redo() };
                        if let Some(p) = p { h.cur = p; h.anchor = None; h.nib = 0; }
                        p.is_some()
                    };
                    if ok { self.hex_changed(vi); }
                    return true;
                }
                'a' => { { let mut st = self.st.borrow_mut(); let di = st.views[vi].doc; let h = st.docs[di].hex.as_mut().unwrap(); h.anchor = Some(0); h.cur = h.buf.len(); } self.hex_follow(vi); self.render(vi); return true; }
                'c' | 'x' => { self.hex_copy(vi); return true; }
                'v' => { if let Some(t) = self.clipboard_get() { self.hex_paste(vi, &t); } return true; }
                _ => {}
            }
        }
        let moved = {
            let mut st = self.st.borrow_mut(); let di = st.views[vi].doc;
            let h = st.docs[di].hex.as_mut().unwrap();
            let len = h.buf.len();
            let row = ROW as u64;
            let new = match k {
                K_LEFT => Some(if !h.ascii && h.nib == 1 { h.cur } else { h.cur.saturating_sub(1) }),
                K_RIGHT => Some((h.cur + 1).min(len)),
                K_UP => Some(h.cur.saturating_sub(row)),
                K_DOWN => Some(if h.cur + row <= len { h.cur + row } else { h.cur }),
                K_PGUP => Some(h.cur.saturating_sub(row * vr)),
                K_PGDN => Some((h.cur + row * vr).min(len)),
                K_HOME => Some(if ctrl { 0 } else { h.cur - h.cur % row }),
                K_END => Some(if ctrl { len } else { (h.cur - h.cur % row + row - 1).min(len) }),
                _ => None,
            };
            match new {
                Some(p) => {
                    if shift { if h.anchor.is_none() { h.anchor = Some(h.cur); } } else { h.anchor = None; }
                    h.cur = p; h.nib = 0; true
                }
                None => false,
            }
        };
        if moved { self.hex_follow(vi); self.render(vi); self.refresh_status(); return true; }
        match k {
            '\t' => { { let mut st = self.st.borrow_mut(); let di = st.views[vi].doc; let h = st.docs[di].hex.as_mut().unwrap(); h.ascii = !h.ascii; h.nib = 0; } self.render(vi); true }
            K_INSERT => { { let mut st = self.st.borrow_mut(); let di = st.views[vi].doc; let h = st.docs[di].hex.as_mut().unwrap(); h.insert = !h.insert; } self.refresh_status(); true }
            '\u{8}' | '\u{7f}' => {
                let r = {
                    let mut st = self.st.borrow_mut(); let di = st.views[vi].doc;
                    let h = st.docs[di].hex.as_mut().unwrap();
                    if h.buf.is_mapped() { Err(()) } else {
                        let (s, e) = h.sel().unwrap_or(if k == '\u{8}' { (h.cur.saturating_sub(1), h.cur) } else { (h.cur, (h.cur + 1).min(h.buf.len())) });
                        for _ in s..e { h.buf.delete(s); }
                        h.cur = s; h.anchor = None; h.nib = 0;
                        Ok(e > s)
                    }
                };
                match r { Ok(true) => self.hex_changed(vi), Ok(false) => {}, Err(()) => msg("記憶體映射的大型檔案無法插入或刪除位元組（僅可覆寫）。") }
                true
            }
            _ => false,
        }
    }

    /// typed characters: hex digits in the hex column, raw bytes in the ASCII column
    pub(crate) fn hex_input(&self, vi: usize, text: &str) {
        let mut warn = false;
        let mut any = false;
        {
            let mut st = self.st.borrow_mut(); let di = st.views[vi].doc;
            let h = st.docs[di].hex.as_mut().unwrap();
            h.anchor = None;
            let put = |h: &mut HexState, b: u8, warn: &mut bool| -> bool {
                if h.insert && h.nib == 0 { if !h.buf.insert(h.cur, b) { *warn = true; return false; } true } else { h.buf.set(h.cur, b) }
            };
            if h.ascii {
                for b in text.bytes() {
                    if put(h, b, &mut warn) { h.cur += 1; any = true; }
                }
            } else {
                for c in text.chars() {
                    let Some(v) = c.to_digit(16) else { continue };
                    let v = v as u8;
                    if h.nib == 0 {
                        let old = if h.insert { 0 } else { h.buf.get(h.cur).unwrap_or(0) };
                        if put(h, (v << 4) | (old & 0x0f), &mut warn) { h.nib = 1; any = true; }
                    } else {
                        let old = h.buf.get(h.cur).unwrap_or(0);
                        if h.buf.set(h.cur, (old & 0xf0) | v) { h.nib = 0; h.cur += 1; any = true; }
                    }
                }
            }
        }
        if warn { msg("記憶體映射的大型檔案無法插入位元組（僅可覆寫）。"); }
        if any { self.hex_changed(vi); }
    }

    fn hex_copy(&self, vi: usize) {
        let t = {
            let st = self.st.borrow();
            let h = st.docs[st.views[vi].doc].hex.as_ref().unwrap();
            let (s, e) = h.sel().unwrap_or((h.cur, (h.cur + 1).min(h.buf.len())));
            let e = e.min(s + (16 << 20));
            let bytes = h.buf.read(s, (e - s) as usize);
            if h.ascii { String::from_utf8_lossy(&bytes).into_owned() } else { textops::to_hex_string(&bytes, true) }
        };
        self.clipboard_set(t);
    }

    fn hex_paste(&self, vi: usize, t: &str) {
        let bytes = {
            let st = self.st.borrow();
            let h = st.docs[st.views[vi].doc].hex.as_ref().unwrap();
            if h.ascii { t.as_bytes().to_vec() } else { textops::from_hex_string(t).unwrap_or_else(|| t.as_bytes().to_vec()) }
        };
        let mut warn = false;
        {
            let mut st = self.st.borrow_mut(); let di = st.views[vi].doc;
            let h = st.docs[di].hex.as_mut().unwrap();
            for b in bytes {
                let ok = if h.insert { h.buf.insert(h.cur, b) } else { h.buf.set(h.cur, b) };
                if !ok { warn = true; break; }
                h.cur += 1;
            }
            h.nib = 0; h.anchor = None;
        }
        if warn { msg("記憶體映射的大型檔案無法插入位元組（僅可覆寫）。"); }
        self.hex_changed(vi);
    }

    pub(crate) fn hex_mouse(&self, vi: usize, kind: i32, x: f32, y: f32, shift: bool) {
        if kind == 0 { self.ed_focused(vi); }
        if kind > 1 { return; }
        let (cw, lh) = self.metrics();
        {
            let mut st = self.st.borrow_mut(); let di = st.views[vi].doc;
            let h = st.docs[di].hex.as_mut().unwrap();
            let row = h.top as i64 + (y / lh).floor() as i64;
            let cell = (x / cw).max(0.0) as usize;
            let (j, ascii, nib) = if cell >= ASCII_COL - 1 { ((cell + 1 - ASCII_COL).min(15), true, 0) } else {
                let c = if cell >= hex_col(8) - 1 { cell - 1 } else { cell };
                ((c / 3).min(15), false, u8::from(c % 3 == 1))
            };
            let p = (row.max(0) as u64 * ROW as u64 + j as u64).min(h.buf.len());
            if kind == 0 {
                if shift { if h.anchor.is_none() { h.anchor = Some(h.cur); } } else { h.anchor = Some(p); }
                h.ascii = ascii; h.nib = if p == h.buf.len() { 0 } else { nib };
            } else if h.anchor.is_none() { h.anchor = Some(h.cur); }
            h.cur = p;
            if kind == 1 { h.nib = 0; }
        }
        self.hex_follow(vi);
        self.render(vi);
        self.refresh_status();
    }

    pub(crate) fn hex_scroll(&self, vi: usize, rows: Option<i64>, frac: Option<f32>) {
        {
            let mut st = self.st.borrow_mut(); let di = st.views[vi].doc;
            let h = st.docs[di].hex.as_mut().unwrap();
            let n = h.rows().saturating_sub(1);
            if let Some(r) = rows { h.top = (h.top as i64 + r).clamp(0, n as i64) as u64; }
            if let Some(f) = frac { h.top = ((n as f64) * f.clamp(0.0, 1.0) as f64).round() as u64; }
        }
        self.render(vi);
    }

    /// toggle text <-> hex for the active document
    pub(crate) fn set_hex_mode(&self, on: bool) {
        let vi = self.av();
        let r: Result<(), String> = (|| {
            let mut st = self.st.borrow_mut();
            let di = st.views[vi].doc;
            let head = st.views[vi].sels.first().map(|s| s.head).unwrap_or(0);
            let d = &mut st.docs[di];
            if on == d.hex.is_some() { return Ok(()); }
            if on {
                let bytes = encode(d)?;
                let cur = d.buf.rope.char_to_byte(head.min(d.buf.len())) as u64;
                let mut h = HexState::new(HexBuf::from_vec(bytes));
                h.cur = cur.min(h.buf.len());
                d.hex = Some(Box::new(h));
                d.buf.reset("");
            } else {
                let h = d.hex.as_ref().unwrap();
                let bytes = h.buf.to_vec(512 << 20).ok_or("檔案太大，無法切換為文字模式（上限 512 MB）。")?;
                let modified = d.modified;
                let (s, enc, bom) = decode(&bytes);
                d.eol = detect_eol(&s);
                let text = s.replace("\r\n", "\n").replace('\r', "\n");
                d.cursor = 0;
                d.set_loaded(&text);
                d.encoding = enc; d.bom = bom; d.modified = modified;
                d.hl = Default::default();
                d.hex = None;
                for v in st.views.iter_mut().filter(|v| v.doc == di) { v.sels = vec![Sel::caret(0)]; v.lkey.0 = usize::MAX; v.top = 0; }
            }
            Ok(())
        })();
        if let Err(e) = r { msg(&e); }
        self.hex_follow(vi);
        self.refresh_all();
    }
}
