//! Editor rendering, status, and refresh scheduling.
use super::*;

impl App {
    pub(super) fn render_all(&self) {
        self.render(0);
        if self.st.borrow().split { self.render(1); }
        self.refresh_status();
    }

    pub(super) fn render(&self, vi: usize) {
        if vi == 1 && !self.st.borrow().split { return; }
        if self.is_hex(vi) { self.hex_render(vi); return; }
        let ui = self.ui();
        if ui.get_caret_idle() { ui.set_caret_idle(false); }
        let u = ui.clone();
        self.idle.start(TimerMode::SingleShot, Duration::from_secs(10), move || u.set_caret_idle(true));
        let (cw, lh) = self.metrics();
        let cjk_w = self.ui().get_cjk_w().max(1.0);
        let ligatures = self.st.borrow().cfg.ligatures;
        let (w, h) = self.view_size(vi);
        if w <= 1.0 || h <= 1.0 { self.mark_dirty(); }
        self.ensure_layout(vi);
        let t0 = Instant::now();
        let mut st = self.st.borrow_mut();
        let dark = st.dark;
        let line_numbers = st.line_numbers;
        let spell_on = st.cfg.spell;
        let split = st.split;
        let docmap = st.docmap && !split && vi == 0;
        let active = st.view_idx() == vi;
        let typewriter = st.cfg.typewriter;
        let State { docs, views, .. } = &mut *st;
        let v = &mut views[vi];
        let d = &mut docs[v.doc];
        let fg = if dark { crate::ui::Color::from_rgb_u8(0xdc, 0xdc, 0xdc) } else { crate::ui::Color::from_rgb_u8(0x1e, 0x1e, 0x1e) };
        let nrows = v.layout.len();
        let vr = ((h / lh).ceil() as usize).max(1);
        if v.sels.is_empty() { v.sels.push(Sel::caret(0)); }
        v.primary = v.primary.min(v.sels.len() - 1);
        let prim = v.sels[v.primary];
        let r = d.buf.rope.clone();

        // ---- keep the primary caret visible
        if v.follow {
            let ri = v.layout.row_of(&r, prim.head);
            if ri < v.top { v.top = ri; }
            if ri + 1 >= v.top + vr { v.top = (ri + 2).saturating_sub(vr).min(ri); }
            if typewriter { v.top = ri.saturating_sub(vr / 2); }
            let row = v.layout.row(&r, ri);
            let chars = line_chars(&r, row.line);
            let col = prim.head.min(r.len_chars()) - r.line_to_char(row.line);
            let x = editor::cells_between(&chars, row.start, col) as f32 * cw;
            if x - v.sx < 0.0 { v.sx = (x - 40.0).max(0.0); }
            if w > 80.0 && x - v.sx > w - 20.0 { v.sx = x - w + 60.0; }
            if w > 80.0 { v.follow = false; }
        }
        v.top = v.top.min(nrows.saturating_sub(1));
        if v.lkey.4 { v.sx = 0.0; }

        // ---- highlighting (time bounded)
        let ext = d.path.as_ref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase());
        d.hl.configure(&d.lang, ext.as_deref(), dark);
        if let Some(l) = d.buf.dirty_line.take() { d.hl.invalidate(l); }
        let last_line = v.layout.row(&r, (v.top + vr).min(nrows.saturating_sub(1))).line + 1;
        let mut done = d.hl.work(&r, last_line, 200);
        while !done && t0.elapsed() < Duration::from_millis(20) { done = d.hl.work(&r, last_line, 200); }
        if !done { self.mark_dirty(); }

        // ---- marks: matching brackets and occurrences of the selected word
        let mut brackets: Vec<usize> = vec![];
        if prim.is_empty() && v.sels.len() == 1 {
            if let Some((a, b)) = highlight::match_bracket(&r, prim.head) { brackets = vec![a, b]; }
        }
        let needle: Option<String> = if !prim.is_empty() && prim.end() - prim.start() <= 200 {
            let t = r.slice(prim.start()..prim.end()).to_string();
            (!t.contains('\n') && !t.trim().is_empty() && t.chars().all(editor::is_word)).then_some(t)
        } else { None };

        let head_line = r.char_to_line(prim.head.min(r.len_chars()));
        let nlines = r.len_lines();
        let num_w = if line_numbers { (digits(nlines).max(3) as f32) * cw + 4.0 } else { 0.0 };
        let gutter_w = 16.0 + num_w + 16.0;

        let mut rows = Vec::with_capacity(vr);
        let mut carets: Vec<EdCaret> = Vec::new();
        let mut line_cache: (usize, Vec<char>, usize) = (usize::MAX, vec![], 0);
        let sel_ranges: Vec<(usize, usize)> = v.sels.iter().filter(|s| !s.is_empty()).map(|s| (s.start(), s.end())).collect();
        let mut max_w = v.max_w;
        for (i, ri) in (v.top..(v.top + vr).min(nrows)).enumerate() {
            let row = v.layout.row(&r, ri);
            if line_cache.0 != row.line { line_cache = (row.line, line_chars(&r, row.line), r.line_to_char(row.line)); }
            let (_, chars, ls) = (&line_cache.0, &line_cache.1, line_cache.2);
            let last_seg = row.end >= chars.len();
            // x (in cells) of each char boundary in the row
            let mut xs = Vec::with_capacity(row.end - row.start + 1);
            let mut col = 0usize;
            xs.push(0usize);
            for &c in &chars[row.start.min(chars.len())..row.end.min(chars.len())] { col += editor::char_cells(c, col); xs.push(col); }
            let px = |off: usize| -> f32 { xs[(off.clamp(row.start, row.end)) - row.start] as f32 * cw - v.sx };
            max_w = max_w.max(col as f32 * cw);

            // tokens
            let mut toks: Vec<EdTok> = Vec::new();
            let spans: Vec<(usize, usize, crate::ui::Color, bool, bool)> = match d.hl.line(row.line) {
                Some(ts) => { let mut o = 0; ts.iter().map(|t| { let n = t.text.chars().count(); let s = (o, o + n, t.color, t.bold, t.italic); o += n; s }).collect() }
                None => vec![(0, chars.len(), fg, false, false)],
            };
            let mut si = 0;
            let mut run = String::new();
            let mut run_start = row.start;
            let mut run_style = (fg, false, false);
            let flush = |run: &mut String, start: usize, style: (crate::ui::Color, bool, bool), toks: &mut Vec<EdTok>| {
                if run.is_empty() { return; }
                let mut x = px(start);
                // a wide (CJK) glyph is narrower than its 2 cells: centre it instead of leaving
                // all the slack on the right, so 中文 looks evenly spaced and stays on the grid
                let mut it = run.chars();
                if let (Some(c), None) = (it.next(), it.next()) {
                    if unicode_width::UnicodeWidthChar::width(c) == Some(2) { x += ((2.0 * cw - cjk_w) / 2.0).max(0.0).round(); }
                }
                let wpx = run.chars().count() as f32 * cw;
                if x + wpx >= -cw && x <= w + cw && !run.trim().is_empty() {
                    toks.push(EdTok { x, text: run.as_str().into(), color: style.0, bold: style.1, italic: style.2 });
                }
                run.clear();
            };
            for off in row.start..row.end.min(chars.len()) {
                while si + 1 < spans.len() && spans[si].1 <= off { si += 1; }
                let style = spans.get(si).filter(|s| s.0 <= off && off < s.1).map(|s| (s.2, s.3, s.4)).unwrap_or((fg, false, false));
                let c = chars[off];
                let cwid = xs[off + 1 - row.start] - xs[off - row.start];
                // without ligatures, break between adjacent operator characters so the
                // font cannot merge e.g. `->` / `!=` / `>=` into one glyph
                let lig_break = !ligatures && is_lig_char(c) && run.chars().last().is_some_and(is_lig_char);
                if c == '\t' || cwid != 1 || style != run_style || c.is_control() || lig_break {
                    flush(&mut run, run_start, run_style, &mut toks);
                    run_style = style;
                    run_start = off;
                    if c == '\t' || c.is_control() { run_start = off + 1; continue; }
                    if cwid != 1 {
                        run.push(c);
                        flush(&mut run, off, style, &mut toks);
                        run_start = off + 1;
                        continue;
                    }
                }
                if run.is_empty() { run_start = off; }
                run.push(c);
            }
            flush(&mut run, run_start, run_style, &mut toks);

            // rects: word marks, brackets, selections
            let (rs, re) = (ls + row.start, ls + row.end);
            let mut rects: Vec<EdRect> = Vec::new();
            if let Some(nd) = &needle {
                let line_str: String = chars[row.start.min(chars.len())..row.end.min(chars.len())].iter().collect();
                let nlen = nd.chars().count();
                for (bi, _) in line_str.match_indices(nd.as_str()) {
                    let co = line_str[..bi].chars().count() + row.start;
                    let before_ok = co == 0 || !editor::is_word(chars[co - 1]);
                    let after_ok = co + nlen >= chars.len() || !editor::is_word(chars[co + nlen]);
                    if before_ok && after_ok { rects.push(EdRect { x: px(co), w: px(co + nlen) - px(co), kind: 1 }); }
                }
            }
            if spell_on && matches!(d.lang.as_str(), "text" | "markdown") {
                let seg = &chars[row.start.min(chars.len())..row.end.min(chars.len())];
                for (a, b) in spell::misspelled(seg) { rects.push(EdRect { x: px(row.start + a), w: px(row.start + b) - px(row.start + a), kind: 4 }); }
            }
            for &b in &brackets { if b >= rs && b < re { let o = b - ls; rects.push(EdRect { x: px(o), w: px(o + 1) - px(o), kind: 2 }); } }
            for &(s, e) in &sel_ranges {
                if e < rs || s > re || (s == re && !(last_seg && e > re)) { continue; }
                let a = s.max(rs) - ls;
                let b = e.min(re) - ls;
                let extra = if e > re && last_seg { cw * 0.6 } else { 0.0 };
                let wpx = px(b) - px(a) + extra;
                if wpx > 0.0 { rects.push(EdRect { x: px(a), w: wpx, kind: 0 }); }
            }
            // carets
            for (k, s) in v.sels.iter().enumerate() {
                let hh = s.head;
                if hh < rs || hh > re { continue; }
                if hh == re && !last_seg { continue; } // belongs to the next wrapped segment
                let c = EdCaret { x: px(hh - ls), y: i as f32 * lh };
                if k == v.primary { carets.insert(0, c); } else { carets.push(c); }
            }
            let fold = if row.start == 0 && d.fold_end(row.line).is_some() { if d.folds.contains(&row.line) { 2 } else { 1 } } else { 0 };
            rows.push(EdRow {
                num: if row.start == 0 { (row.line + 1).to_string() } else { String::new() },
                line: row.line as i32, fold, cur: row.line == head_line,
                mark: row.start == 0 && d.bookmarks.contains(&row.line),
                diff: d.diff.as_ref().filter(|x| x.0 == d.buf.version).and_then(|x| x.1.get(&row.line)).map(|&k| k as i32).unwrap_or(0),
                toks, rects,
            });
        }
        v.max_w = max_w;
        if !active { carets.clear(); }

        // ---- scrollbars
        let max_top = nrows.saturating_sub(1).max(1);
        let thumb_size = (vr as f32 / (nrows + vr) as f32).clamp(0.02, 1.0);
        let thumb_pos = (v.top as f32 / max_top as f32).clamp(0.0, 1.0);
        let content_w = (v.max_w + 80.0).max(w);
        let wrap = v.lkey.4;
        let hbar = !wrap && content_w > w + 1.0;
        let hthumb_size = (w / content_w).clamp(0.05, 1.0);
        let hthumb_pos = if content_w > w { (v.sx / (content_w - w)).clamp(0.0, 1.0) } else { 0.0 };

        // ---- autocomplete
        let (ac_items, ac_sel, ac_visible) = match &v.ac {
            Some(ac) => (ac.items.iter().take(10).cloned().collect::<Vec<_>>(), ac.sel as i32, true),
            None => (vec![], 0, false),
        };

        // ---- document map
        let (mut map_y, mut map_h) = (0.0, 0.0);
        if docmap {
            let n = nlines.max(1);
            let scale = (h / n as f32).min(2.0);
            let key = (v.doc, d.buf.version, h as i32, dark);
            if v.map_key != key {
                v.map_key = key;
                let col = if dark { crate::ui::Color::from_argb_u8(150, 0xc8, 0xc8, 0xc8) } else { crate::ui::Color::from_argb_u8(150, 0x50, 0x50, 0x50) };
                let step = if scale >= 1.0 { 1.0 } else { 1.0 / scale };
                let mut out = Vec::new();
                let mut y = 0.0f32;
                let mut lf = 0.0f32;
                while (lf as usize) < n && y < h {
                    let l = lf as usize;
                    let lc = line_chars(&r, l);
                    let ind = lc.iter().take_while(|c| c.is_whitespace()).map(|&c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
                    let len = editor::cells_between(&lc, 0, lc.len());
                    if len > ind {
                        let x = (ind as f32 * 0.7).min(80.0);
                        out.push(MapLine { y, x, w: (((len - ind) as f32) * 0.7).min(82.0 - x).max(1.0), color: col });
                    }
                    y += scale.max(1.0);
                    lf += step;
                }
                v.map = out;
            }
            let top_line = v.layout.row(&r, v.top).line;
            map_y = top_line as f32 * scale;
            map_h = vr as f32 * scale;
        }

        let title = if split { format!("{}{}", if d.modified { "● " } else { "" }, d.title) } else { String::new() };
        let map_lines = if docmap { v.map.clone() } else { vec![] };
        drop(st);
        let state = EdState {
            rows, carets,
            gutter_w, num_w, thumb_pos, thumb_size, hthumb_pos, hthumb_size, hbar,
            ac_items, ac_sel, ac_visible,
            title,
            map_lines, map_view_y: map_y, map_view_h: map_h,
        };
        if vi == 0 { ui.set_v0(state); } else { ui.set_v1(state); }
        self.rkey.borrow_mut()[vi] = (cw, lh, w, h);
    }

    /// Status bar for the active view.
    pub(super) fn refresh_status(&self) {
        if self.st.borrow().side_mode == 2 { self.side_refresh(); }
        let ui = self.ui();
        if let Some(s) = self.hex_status() { ui.set_st_pos(s); return; }
        let st = self.st.borrow();
        let v = &st.views[st.view_idx()];
        let r = &st.docs[v.doc].buf.rope;
        if v.sels.is_empty() { return; }
        let p = v.sels[v.primary.min(v.sels.len() - 1)];
        let head = p.head.min(r.len_chars());
        let line = r.char_to_line(head);
        let col = head - r.line_to_char(line);
        let sel: usize = v.sels.iter().map(|s| s.end() - s.start()).sum();
        let mut s = format!("Ln : {}    Col : {}    Pos : {}", line + 1, col + 1, head + 1);
        if sel > 0 { s += &format!("    Sel : {}", sel); }
        if v.sels.len() > 1 { s += &format!("  ({} 個游標)", v.sels.len()); }
        ui.set_st_pos(s);
    }

    /// Periodic: re-render when sizes/metrics changed or highlighting is still in progress.
    /// Start the 30 ms housekeeping timer; it stops itself once nothing is pending,
    /// so an idle editor does not wake up (only the caret blink timer runs).
    pub(super) fn mark_dirty(&self) { self.dirty.set(true); self.kick(); }

    pub(super) fn kick(&self) {
        if self.ticker.running() { return; }
        let me = self.me.borrow().clone();
        self.ticker.start(TimerMode::Repeated, Duration::from_millis(30), move || { if let Some(a) = me.upgrade() { a.tick(); } });
    }

    pub(super) fn tick(&self) {
        let (cw, lh) = self.metrics();
        let split = self.st.borrow().split;
        for vi in 0..if split { 2 } else { 1 } {
            let (w, h) = self.view_size(vi);
            let k = self.rkey.borrow()[vi];
            if self.dirty.get() || k != (cw, lh, w, h) { self.dirty.set(false); self.render(vi); }
        }
        self.fif_poll();
        self.rif_poll();
        if !self.dirty.get() && self.st.borrow().fif_rx.is_none() && self.st.borrow().rif.is_none() { self.ticker.stop(); }
    }

    pub(super) fn play_macro(&self, times: usize) {
        let entries = self.st.borrow().macro_buf.clone();
        if entries.is_empty() { return; }
        self.st.borrow_mut().macro_play = true;
        let me = self.me.borrow().upgrade();
        for _ in 0..times {
            for e in &entries {
                let vi = self.av();
                if let Some(t) = e.strip_prefix("i:") { self.ed_input(vi, t); }
                else if let Some(rest) = e.strip_prefix("k:") {
                    let b = rest.as_bytes();
                    if b.len() >= 4 { self.ed_key(vi, &rest[4..], b[0] == b'1', b[1] == b'1', b[2] == b'1'); }
                }
                else if let Some(a) = e.strip_prefix("a:") { if let Some(m) = &me { m.action(a); } }
                else {
                    // legacy (v0.2) entries: raw key texts
                    let special = e.chars().count() == 1 && e.chars().all(|c| c.is_control() || ('\u{f700}'..='\u{f8ff}').contains(&c));
                    if special { self.ed_key(vi, e, false, false, false); } else { self.ed_input(vi, e); }
                }
            }
        }
        self.st.borrow_mut().macro_play = false;
        self.ui().invoke_focus_editor();
    }
}
