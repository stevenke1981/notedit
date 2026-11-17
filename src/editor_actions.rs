//! Application behavior separated from the startup and state definitions.
use super::*;

impl App {
    /// index of the active view / its document
    pub(super) fn av(&self) -> usize { self.st.borrow().view_idx() }
    pub(super) fn cd(&self) -> usize { let st = self.st.borrow(); st.views[st.view_idx()].doc }

    pub(super) fn text(&self) -> String { let st = self.st.borrow(); st.docs[st.views[st.view_idx()].doc].text() }

    /// (anchor, head) of the primary selection of the active view in byte offsets
    pub(super) fn sel_bytes(&self) -> (usize, usize) {
        let st = self.st.borrow();
        let v = &st.views[st.view_idx()];
        let r = &st.docs[v.doc].buf.rope;
        let s = v.sels.get(v.primary).copied().unwrap_or(Sel::caret(0));
        (r.char_to_byte(s.anchor.min(r.len_chars())), r.char_to_byte(s.head.min(r.len_chars())))
    }

    pub(super) fn selection(&self) -> (usize, usize) { let (a, c) = self.sel_bytes(); (a.min(c), a.max(c)) }

    pub(super) fn select_bytes(&self, s: usize, e: usize) {
        {
            let mut st = self.st.borrow_mut();
            let vi = st.view_idx();
            let State { docs, views, .. } = &mut *st;
            let v = &mut views[vi];
            let r = &docs[v.doc].buf.rope;
            let b = |x: usize| r.byte_to_char(x.min(r.len_bytes()));
            v.sels = vec![Sel::new(b(s), b(e))];
            v.primary = 0; v.follow = true; v.ac = None;
        }
        self.unfold_at_caret(self.av());
        self.render_all();
    }

    /// Replace the whole text of the active document as one undo step.
    pub(super) fn set_text(&self, text: String) {
        {
            let mut st = self.st.borrow_mut();
            let vi = st.view_idx();
            let State { docs, views, .. } = &mut *st;
            let di = views[vi].doc;
            let d = &mut docs[di];
            let old_lines = d.buf.rope.len_lines();
            d.buf.set_text(&views[vi].sels, &text);
            d.modified = true;
            let len = d.buf.len();
            let new_lines = d.buf.rope.len_lines();
            if old_lines != new_lines { d.folds.clear(); d.folds_ver += 1; d.bookmarks.retain(|&l| l < new_lines); }
            for v in views.iter_mut().filter(|v| v.doc == di) {
                for s in v.sels.iter_mut() { s.anchor = s.anchor.min(len); s.head = s.head.min(len); }
                v.follow = true;
            }
        }
        self.after_edit();
    }

    pub(super) fn after_edit(&self) {
        self.refresh_meta();
        if self.st.borrow().preview {
            let me = self.me.borrow().clone();
            self.preview_timer.start(TimerMode::SingleShot, Duration::from_millis(180), move || {
                if let Some(app) = me.upgrade() { app.refresh_preview(); }
            });
        }
        self.render_all();
    }

    /// Apply `edits` (sorted by start, non-overlapping) to the document of view `vi`.
    /// The selections of `vi` become carets at the end of each inserted text when `carets` is true.
    pub(super) fn apply_edits(&self, vi: usize, edits: Vec<(usize, usize, String)>, kind: EditKind, carets: bool) {
        {
            let mut st = self.st.borrow_mut();
            let State { docs, views, .. } = &mut *st;
            let di = views[vi].doc;
            let d = &mut docs[di];
            let first = edits.first().map(|e| e.0).unwrap_or(0).min(d.buf.len());
            let line = d.buf.rope.char_to_line(first);
            let old_lines = d.buf.rope.len_lines();
            let before = views[vi].sels.clone();
            let (changes, ends, order) = d.buf.edit(&before, edits, kind);
            if changes.is_empty() { return; }
            d.modified = true;
            let new_lines = d.buf.rope.len_lines();
            if new_lines != old_lines {
                let (o, n) = if new_lines > old_lines { (1, 1 + new_lines - old_lines) } else { (1 + old_lines - new_lines, 1) };
                editor::shift_lines(&mut d.bookmarks, line, o, n);
                editor::shift_lines(&mut d.folds, line, o, n);
                d.folds_ver += 1;
            }
            for (i, v) in views.iter_mut().enumerate() {
                if v.doc != di { continue; }
                if i == vi && carets {
                    let mut new = vec![Sel::caret(0); ends.len()];
                    for (k, &orig) in order.iter().enumerate() { new[orig] = Sel::caret(ends[k]); }
                    v.sels = new;
                } else {
                    for s in v.sels.iter_mut() { s.anchor = editor::map_pos(s.anchor, &changes); s.head = editor::map_pos(s.head, &changes); }
                }
                v.primary = editor::normalize(&mut v.sels, v.primary);
                if i == vi { v.follow = true; }
            }
        }
        self.after_edit();
    }

    /// Insert text at every selection of the active view (honours overwrite mode).
    pub(super) fn insert_text(&self, vi: usize, text: &str) {
        let ow = self.st.borrow().overwrite;
        let edits: Vec<(usize, usize, String)> = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let r = &st.docs[v.doc].buf.rope;
            v.sels.iter().map(|s| {
                let (a, mut b) = (s.start(), s.end());
                if ow && s.is_empty() && !text.contains('\n') && b < r.len_chars() && r.char(b) != '\n' { b += 1; }
                (a, b, text.to_string())
            }).collect()
        };
        let kind = if text.chars().all(editor::is_word) || text == " " { EditKind::Insert } else { EditKind::Other };
        self.apply_edits(vi, edits, kind, true);
    }

    pub(super) fn unfold_at_caret(&self, vi: usize) {
        let mut st = self.st.borrow_mut();
        let State { docs, views, .. } = &mut *st;
        let v = &views[vi];
        let d = &mut docs[v.doc];
        if d.folds.is_empty() { return; }
        let r = &d.buf.rope;
        let lines: Vec<usize> = v.sels.iter().map(|s| r.char_to_line(s.head.min(r.len_chars()))).collect();
        let ranges = editor::fold_ranges(r, &d.folds);
        let mut changed = false;
        for (a, b) in ranges {
            if lines.iter().any(|&l| l > a && l <= b) { d.folds.remove(&a); changed = true; }
        }
        if changed { d.folds_ver += 1; }
    }

    pub(super) fn metrics(&self) -> (f32, f32) {
        let ui = self.ui();
        (ui.get_cell_w().max(1.0), ui.get_line_h().max(1.0))
    }

    pub(super) fn view_size(&self, vi: usize) -> (f32, f32) {
        let ui = self.ui();
        if vi == 0 { (ui.get_v0_w(), ui.get_v0_h()) } else { (ui.get_v1_w(), ui.get_v1_h()) }
    }

    /// Make sure the layout of view `vi` matches the document state.
    pub(super) fn ensure_layout(&self, vi: usize) {
        let (cw, _) = self.metrics();
        let (w, _) = self.view_size(vi);
        let mut st = self.st.borrow_mut();
        let wrap = st.wrap;
        let State { docs, views, .. } = &mut *st;
        let v = &mut views[vi];
        let d = &docs[v.doc];
        let cols = if wrap { ((w - 4.0) / cw).floor().max(8.0) as usize } else { 0 };
        let key = (v.doc, d.buf.version, cols, d.folds_ver, wrap);
        if v.lkey == key { return; }
        let ranges = editor::fold_ranges(&d.buf.rope, &d.folds);
        v.layout = Layout::build(&d.buf.rope, wrap.then_some(cols), &ranges);
        v.lkey = key;
    }

    /// char offset at text-area point (x, y) of view `vi`
    pub(super) fn pos_at(&self, vi: usize, x: f32, y: f32) -> (usize, usize, f32) {
        self.ensure_layout(vi);
        let (cw, lh) = self.metrics();
        let st = self.st.borrow();
        let v = &st.views[vi];
        let r = &st.docs[v.doc].buf.rope;
        let n = v.layout.len();
        let row_i = ((v.top as f32) + (y / lh).floor()).clamp(0.0, (n.saturating_sub(1)) as f32) as usize;
        let row = v.layout.row(r, row_i);
        let chars = line_chars(r, row.line);
        let cells = ((x + v.sx) / cw).max(0.0);
        let off = editor::offset_at_cells(&chars, row.start, row.end, cells);
        (r.line_to_char(row.line) + off, row_i, cells)
    }

    pub(super) fn pos_in_row(&self, vi: usize, row_i: usize, cells: f32) -> usize {
        let st = self.st.borrow();
        let v = &st.views[vi];
        let r = &st.docs[v.doc].buf.rope;
        let row = v.layout.row(r, row_i.min(v.layout.len().saturating_sub(1)));
        let chars = line_chars(r, row.line);
        r.line_to_char(row.line) + editor::offset_at_cells(&chars, row.start, row.end, cells)
    }

    /// cells of `pos` within its visual row
    pub(super) fn cells_of(&self, vi: usize, pos: usize) -> (usize, f32) {
        let st = self.st.borrow();
        let v = &st.views[vi];
        let r = &st.docs[v.doc].buf.rope;
        let ri = v.layout.row_of(r, pos);
        let row = v.layout.row(r, ri);
        let chars = line_chars(r, row.line);
        let col = pos.min(r.len_chars()) - r.line_to_char(row.line);
        (ri, editor::cells_between(&chars, row.start, col) as f32)
    }

    pub(super) fn ed_focused(&self, vi: usize) {
        let changed = {
            let mut st = self.st.borrow_mut();
            let c = st.active != vi;
            st.active = vi;
            c
        };
        if changed { self.refresh_meta(); self.render_all(); }
    }

    pub(super) fn ed_mouse(&self, vi: usize, kind: i32, x: f32, y: f32, ctrl: bool, shift: bool, alt: bool) {
        if self.is_hex(vi) { if kind != 3 { self.hex_mouse(vi, kind, x, y, shift); } return; }
        if kind == 3 {
            // right click: move the caret there unless the click is inside a selection
            self.ed_focused(vi);
            let (pos, _, _) = self.pos_at(vi, x, y);
            let mut st = self.st.borrow_mut();
            let v = &mut st.views[vi];
            if !v.sels.iter().any(|s| !s.is_empty() && s.start() <= pos && pos <= s.end()) {
                v.sels = vec![Sel::caret(pos)]; v.primary = 0; v.ac = None;
            }
            drop(st);
            self.render(vi); self.refresh_status();
            return;
        }
        let alt = alt ^ self.st.borrow().column_mode;
        if kind == 0 { self.ed_focused(vi); }
        let (cw, lh) = self.metrics();
        let (_, h) = self.view_size(vi);
        // auto-scroll while dragging outside the view
        if kind == 1 {
            let mut st = self.st.borrow_mut();
            let v = &mut st.views[vi];
            if y < 0.0 { v.top = v.top.saturating_sub(1); } else if y > h { v.top = (v.top + 1).min(v.layout.len().saturating_sub(1)); }
            if x < 0.0 { v.sx = (v.sx - cw * 2.0).max(0.0); }
        }
        let (pos, row_i, cells) = self.pos_at(vi, x, y);
        let _ = lh;
        {
            let mut st = self.st.borrow_mut();
            let State { docs, views, .. } = &mut *st;
            let v = &mut views[vi];
            let r = &docs[v.doc].buf.rope;
            match kind {
                0 => {
                    v.ac = None;
                    let now = Instant::now();
                    let count = match v.click { (Some(t), p, c) if now.duration_since(t) < Duration::from_millis(450) && p == pos => c + 1, _ => 1 };
                    v.click = (Some(now), pos, count);
                    if alt {
                        v.sels = vec![Sel::caret(pos)]; v.primary = 0;
                        v.drag = Drag::Column { row: row_i, cells };
                    } else if count == 2 || count == 3 {
                        let (a, b) = if count == 2 { word_at(r, pos) } else { line_span(r, pos) };
                        v.sels = vec![Sel::new(a, b)]; v.primary = 0;
                        v.drag = if count == 2 { Drag::Words(a, b) } else { Drag::Lines(r.char_to_line(pos)) };
                    } else if ctrl {
                        v.sels.push(Sel::caret(pos));
                        v.primary = v.sels.len() - 1;
                        v.primary = editor::normalize(&mut v.sels, v.primary);
                        v.drag = Drag::Normal;
                    } else if shift {
                        let p = v.primary.min(v.sels.len() - 1);
                        let a = v.sels[p].anchor;
                        v.sels = vec![Sel::new(a, pos)]; v.primary = 0;
                        v.drag = Drag::Normal;
                    } else {
                        v.sels = vec![Sel::caret(pos)]; v.primary = 0;
                        v.drag = Drag::Normal;
                    }
                }
                1 => match v.drag {
                    Drag::Normal => { let p = v.primary.min(v.sels.len() - 1); v.sels[p].head = pos; v.sels[p].goal = None; }
                    Drag::Words(a, b) => {
                        let (wa, wb) = word_at(r, pos);
                        v.sels = vec![if pos < a { Sel::new(b, wa) } else { Sel::new(a, wb.max(b)) }]; v.primary = 0;
                    }
                    Drag::Lines(l0) => {
                        let l1 = r.char_to_line(pos);
                        let (a, b) = (line_span(r, r.line_to_char(l0.min(l1))).0, line_span(r, r.line_to_char(l0.max(l1))).1);
                        v.sels = vec![if l1 < l0 { Sel::new(b, a) } else { Sel::new(a, b) }]; v.primary = 0;
                    }
                    Drag::Column { row, cells: c0 } => {
                        let (r0, r1) = (row.min(row_i), row.max(row_i));
                        let mut sels = Vec::new();
                        for ri in r0..=r1 {
                            let rw = v.layout.row(r, ri);
                            let chars = line_chars(r, rw.line);
                            let ls = r.line_to_char(rw.line);
                            let a = ls + editor::offset_at_cells(&chars, rw.start, rw.end, c0);
                            let b = ls + editor::offset_at_cells(&chars, rw.start, rw.end, cells);
                            sels.push(Sel::new(a, b));
                        }
                        v.primary = if row_i >= row { sels.len() - 1 } else { 0 };
                        v.sels = sels;
                    }
                    Drag::None => {}
                },
                _ => { v.drag = Drag::None; }
            }
            if kind != 2 { v.follow = kind == 1; }
        }
        if kind == 2 { self.unfold_at_caret(vi); }
        self.render(vi);
        self.refresh_status();
    }

    pub(super) fn ed_gutter(&self, vi: usize, row: i32, fold_area: bool, shift: bool) {
        self.ed_focused(vi);
        self.ensure_layout(vi);
        let line = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let r = &st.docs[v.doc].buf.rope;
            let ri = v.top + row.max(0) as usize;
            if ri >= v.layout.len() { return; }
            v.layout.row(r, ri).line
        };
        if fold_area { self.toggle_fold(vi, line); return; }
        {
            let mut st = self.st.borrow_mut();
            let State { docs, views, .. } = &mut *st;
            let v = &mut views[vi];
            let r = &docs[v.doc].buf.rope;
            let (a, b) = line_span(r, r.line_to_char(line));
            if shift { let p = v.primary.min(v.sels.len() - 1); let an = v.sels[p].anchor; v.sels = vec![Sel::new(an.min(a), if a < an { a } else { b })]; }
            else { v.sels = vec![Sel::new(a, b)]; }
            v.primary = 0;
            v.drag = Drag::Lines(line);
        }
        self.render(vi);
        self.refresh_status();
    }

    pub(super) fn toggle_fold(&self, vi: usize, line: usize) {
        {
            let mut st = self.st.borrow_mut();
            let di = st.views[vi].doc;
            let d = &mut st.docs[di];
            if d.folds.contains(&line) { d.folds.remove(&line); }
            else if d.fold_end(line).is_some() { d.folds.insert(line); }
            else { return; }
            d.folds_ver += 1;
            // carets inside the folded block move to the header line
            let r = d.buf.rope.clone();
            let end = d.fold_end(line).unwrap_or(line);
            let ls = r.line_to_char(line);
            for v in st.views.iter_mut().filter(|v| v.doc == di) {
                for s in v.sels.iter_mut() {
                    let l = r.char_to_line(s.head.min(r.len_chars()));
                    if l > line && l <= end { *s = Sel::caret(ls + editor::line_len(&r, line)); }
                }
                v.primary = editor::normalize(&mut v.sels, v.primary);
            }
        }
        self.render_all();
    }

    pub(super) fn fold_all(&self, fold: bool) {
        {
            let mut st = self.st.borrow_mut();
            let vi = st.view_idx();
            let di = st.views[vi].doc;
            let d = &mut st.docs[di];
            d.folds.clear();
            if fold {
                let n = d.buf.rope.len_lines().min(50_000);
                let mut l = 0;
                while l < n {
                    if let Some(e) = d.fold_end(l) { d.folds.insert(l); l = e + 1; } else { l += 1; }
                }
            }
            d.folds_ver += 1;
            let r = d.buf.rope.clone();
            let ranges = editor::fold_ranges(&r, &d.folds);
            for v in st.views.iter_mut().filter(|v| v.doc == di) {
                for s in v.sels.iter_mut() {
                    let l = r.char_to_line(s.head.min(r.len_chars()));
                    if let Some(&(a, _)) = ranges.iter().find(|(a, b)| l > *a && l <= *b) { *s = Sel::caret(r.line_to_char(a)); }
                }
                v.primary = editor::normalize(&mut v.sels, v.primary);
                v.follow = true;
            }
        }
        self.render_all();
    }

    pub(super) fn ed_wheel(&self, vi: usize, dx: f32, dy: f32, shift: bool, ctrl: bool) {
        if ctrl { self.action_plain(if dy > 0.0 { "zoomin" } else { "zoomout" }); return; }
        let (cw, lh) = self.metrics();
        if self.is_hex(vi) { let r = (-dy / lh).round() as i64; self.hex_scroll(vi, Some(if r == 0 { -dy.signum() as i64 } else { r }), None); return; }
        {
            let mut st = self.st.borrow_mut();
            let v = &mut st.views[vi];
            let (dx, dy) = if shift && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
            if dy != 0.0 {
                let rows = (-dy / lh).round() as i64;
                let rows = if rows == 0 { -dy.signum() as i64 } else { rows };
                let n = v.layout.len().saturating_sub(1) as i64;
                v.top = (v.top as i64 + rows).clamp(0, n) as usize;
            }
            if dx != 0.0 { v.sx = (v.sx - dx).max(0.0); let _ = cw; }
            v.follow = false;
        }
        self.render(vi);
    }

    pub(super) fn ed_vscroll(&self, vi: usize, f: f32) {
        if self.is_hex(vi) { self.hex_scroll(vi, None, Some(f)); return; }
        {
            let mut st = self.st.borrow_mut();
            let v = &mut st.views[vi];
            let n = v.layout.len().saturating_sub(1);
            v.top = ((n as f32) * f.clamp(0.0, 1.0)).round() as usize;
            v.follow = false;
        }
        self.render(vi);
    }

    pub(super) fn ed_hscroll(&self, vi: usize, f: f32) {
        let (w, _) = self.view_size(vi);
        {
            let mut st = self.st.borrow_mut();
            let v = &mut st.views[vi];
            let content = (v.max_w + 80.0).max(w);
            v.sx = ((content - w) * f.clamp(0.0, 1.0)).max(0.0);
            v.follow = false;
        }
        self.render(vi);
    }

    pub(super) fn ed_map(&self, vi: usize, f: f32) {
        let (_, h) = self.view_size(vi);
        let (_, lh) = self.metrics();
        {
            let mut st = self.st.borrow_mut();
            let State { docs, views, .. } = &mut *st;
            let v = &mut views[vi];
            let r = &docs[v.doc].buf.rope;
            let n = r.len_lines().max(1);
            let scale = (h / n as f32).min(2.0);
            let vr = (h / lh) as usize;
            let line = ((f * h / scale) as usize).saturating_sub(vr / 2).min(n - 1);
            v.top = v.layout.row_of(r, r.line_to_char(line));
            v.follow = false;
        }
        self.render(vi);
    }

    // ------------------------------------------------------------ keyboard

    pub(super) fn record(&self, entry: String) {
        let mut st = self.st.borrow_mut();
        if st.macro_rec && !st.macro_play { st.macro_buf.push(entry); }
    }

    pub(super) fn ed_input(&self, vi: usize, text: &str) {
        // key events forwarded while idle may carry private-use navigation key codes
        let owned: String;
        let text = if text.chars().any(|c| ('\u{f700}'..='\u{f8ff}').contains(&c) || (c.is_control() && c != '\n' && c != '\t')) {
            owned = text.chars().filter(|c| !('\u{f700}'..='\u{f8ff}').contains(c) && (!c.is_control() || *c == '\n' || *c == '\t')).collect();
            owned.as_str()
        } else { text };
        if text.is_empty() { return; }
        if self.is_hex(vi) { self.hex_input(vi, text); self.refresh_status(); return; }
        self.record(format!("i:{text}"));
        self.ed_focused(vi);
        // typing replaces the autocomplete popup state
        self.st.borrow_mut().views[vi].ac = None;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_text(vi, &text);
        if text.chars().count() == 1 && text.chars().all(editor::is_word) { self.update_autocomplete(vi, false); }
        self.refresh_status();
    }

    pub(super) fn update_autocomplete(&self, vi: usize, force: bool) {
        let enabled = self.st.borrow().autocomplete;
        if !enabled && !force { return; }
        let mut st = self.st.borrow_mut();
        let State { docs, views, .. } = &mut *st;
        let v = &mut views[vi];
        v.ac = None;
        if v.sels.len() != 1 || !v.sels[0].is_empty() { drop(st); self.render(vi); return; }
        let d = &mut docs[v.doc];
        let r = &d.buf.rope;
        let pos = v.sels[0].head;
        let mut start = pos;
        while start > 0 && editor::is_word(r.char(start - 1)) { start -= 1; }
        let prefix: String = r.slice(start..pos).to_string();
        if prefix.chars().count() < 2 { drop(st); self.render(vi); return; }
        if d.words.0 != d.buf.version && (d.words.0 == 0 || d.buf.len() < 2_000_000) {
            d.words = (d.buf.version, editor::collect_words(r, 4_000_000));
        }
        let items = editor::complete(&d.words.1, &d.lang, &prefix, 50);
        if !items.is_empty() { v.ac = Some(Ac { items, sel: 0, start }); }
        drop(st);
        self.render(vi);
    }

    pub(super) fn accept_autocomplete(&self, vi: usize, idx: Option<usize>) {
        let pick = {
            let mut st = self.st.borrow_mut();
            let v = &mut st.views[vi];
            match v.ac.take() { Some(ac) => { let i = idx.unwrap_or(ac.sel).min(ac.items.len() - 1); Some((ac.start, ac.items[i].clone())) } None => None }
        };
        if let Some((start, word)) = pick {
            let head = { let st = self.st.borrow(); st.views[vi].sels[0].head };
            self.apply_edits(vi, vec![(start, head, word)], EditKind::Other, true);
        }
    }

    /// Returns true when the key was consumed by the editor.
    pub(super) fn ed_key(&self, vi: usize, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        if self.custom_key(text, ctrl, shift, alt) { return true; }
        if self.is_hex(vi) { let h = self.hex_key(vi, text, ctrl, shift, alt); if h { self.refresh_status(); } return h; }
        // column mode: Shift+arrows make a rectangular selection (like Alt+Shift)
        let alt = alt || (shift && !ctrl && self.st.borrow().column_mode && text.chars().next().is_some_and(|c| [K_LEFT, K_RIGHT, K_UP, K_DOWN, K_HOME, K_END].contains(&c)));
        let handled = self.ed_key_inner(vi, text, ctrl, shift, alt);
        let lk = text.to_ascii_lowercase();
        let via_action = (alt && !ctrl && matches!(lk.as_str(), "0" | ")" | "h" | "c")) || (alt && ctrl && lk == "f") || ((text == "\u{f70b}" || text == "\u{f70a}") && !ctrl && !alt);
        if handled && !via_action { self.record(format!("k:{}{}{}:{}", ctrl as u8, shift as u8, alt as u8, text)); self.refresh_status(); }
        handled
    }

    pub(super) fn ed_key_inner(&self, vi: usize, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        let mut chars = text.chars();
        let (Some(k), None) = (chars.next(), chars.next()) else { return false };
        // autocomplete popup navigation
        if self.st.borrow().views[vi].ac.is_some() {
            match k {
                K_UP | K_DOWN => {
                    { let mut st = self.st.borrow_mut(); let ac = st.views[vi].ac.as_mut().unwrap();
                      let n = ac.items.len().min(10); ac.sel = if k == K_UP { (ac.sel + n - 1) % n } else { (ac.sel + 1) % n }; }
                    self.render(vi); return true;
                }
                '\n' | '\t' => { self.accept_autocomplete(vi, None); return true; }
                '\u{1b}' => { self.st.borrow_mut().views[vi].ac = None; self.render(vi); return true; }
                _ => { self.st.borrow_mut().views[vi].ac = None; }
            }
        }
        let lk = k.to_ascii_lowercase();
        let run = |name: &'static str| {
            if let Some(me) = self.me.borrow().upgrade() { Timer::single_shot(std::time::Duration::from_millis(0), move || me.action(name)); }
        };
        if alt && !ctrl && matches!(k, '0' | ')') { run(if shift || k == ')' { "unfoldall" } else { "foldall" }); return true; }
        if alt && ctrl && lk == 'f' { run("foldcur"); return true; }
        if k == '\u{f70a}' && !ctrl && !alt { run(if shift { "diffprev" } else { "diffnext" }); return true; }
        if k == '\u{f70b}' && !ctrl && !alt { run("otherview"); return true; }
        if alt && !ctrl && (lk == 'h' || lk == 'c') { run(if lk == 'c' { "columnmode" } else { "hexmode" }); return true; }
        if alt && !ctrl && !k.is_control() && (k as u32) < 0xf700 { return true; }
        if ctrl && !alt {
            match lk {
                'a' => { self.select_all(vi); return true; }
                'c' => { self.copy(vi, false); return true; }
                'x' => { self.copy(vi, true); return true; }
                'v' => { self.paste(vi); return true; }
                'z' if !shift => { self.undo(vi, true); return true; }
                'z' | 'y' => { self.undo(vi, false); return true; }
                'b' => { self.action_plain("gotobrace"); return true; }
                ' ' => { self.update_autocomplete(vi, true); return true; }
                '\u{8}' | '\u{7f}' => { self.delete(vi, k == '\u{7f}', true); return true; }
                K_INSERT => { self.copy(vi, false); return true; }
                K_F2 => { self.action_plain("bookmark"); return true; }
                _ => {}
            }
        }
        if shift && !ctrl && k == K_INSERT { self.paste(vi); return true; }
        if shift && !ctrl && k == '\u{7f}' { self.copy(vi, true); return true; }
        match k {
            K_LEFT | K_RIGHT | K_UP | K_DOWN | K_HOME | K_END | K_PGUP | K_PGDN => {
                if ctrl && alt && (k == K_UP || k == K_DOWN) { self.add_caret_vertical(vi, k == K_UP); return true; }
                if ctrl && !alt && (k == K_UP || k == K_DOWN) && !shift { self.ed_wheel(vi, 0.0, if k == K_UP { 1.0 } else { -1.0 } * self.metrics().1, false, false); return true; }
                self.move_carets(vi, k, ctrl, shift, alt);
                true
            }
            '\u{8}' => { self.delete(vi, false, false); true }
            '\u{7f}' => { self.delete(vi, true, false); true }
            '\n' if !ctrl && !alt => { self.newline(vi); true }
            '\t' if !ctrl && !alt => { self.tab(vi, shift); true }
            '\u{19}' => { self.tab(vi, true); true }
            K_F2 if !ctrl => { self.action_plain(if shift { "prevbm" } else { "nextbm" }); true }
            '\u{1b}' => {
                let multi = { let st = self.st.borrow(); st.views[vi].sels.len() > 1 };
                if multi {
                    { let mut st = self.st.borrow_mut(); let v = &mut st.views[vi]; let p = v.sels[v.primary.min(v.sels.len() - 1)]; v.sels = vec![Sel::caret(p.head)]; v.primary = 0; }
                    self.render(vi); true
                } else { false }
            }
            _ => false,
        }
    }

    pub(super) fn select_all(&self, vi: usize) {
        { let mut st = self.st.borrow_mut(); let State { docs, views, .. } = &mut *st; let v = &mut views[vi]; let n = docs[v.doc].buf.len(); v.sels = vec![Sel::new(0, n)]; v.primary = 0; }
        self.render(vi);
    }

    pub(super) fn undo(&self, vi: usize, undo: bool) {
        let ok = {
            let mut st = self.st.borrow_mut();
            let State { docs, views, .. } = &mut *st;
            let di = views[vi].doc;
            let d = &mut docs[di];
            let mut sels = views[vi].sels.clone();
            let ok = if undo { d.buf.undo(&mut sels) } else { d.buf.redo(&mut sels) };
            if ok {
                d.modified = d.saved.as_ref().is_none_or(|r| *r != d.buf.rope);
                d.folds.clear(); d.folds_ver += 1;
                let len = d.buf.len();
                for (i, v) in views.iter_mut().enumerate().filter(|(_, v)| v.doc == di) {
                    if i == vi { v.sels = sels.clone(); v.follow = true; }
                    for s in v.sels.iter_mut() { s.anchor = s.anchor.min(len); s.head = s.head.min(len); }
                    v.primary = editor::normalize(&mut v.sels, 0);
                }
            }
            ok
        };
        if ok { self.after_edit(); }
    }

    pub(super) fn clipboard_set(&self, t: String) -> bool {
        let mut cb = self.clip.borrow_mut();
        if cb.is_none() { *cb = arboard::Clipboard::new().ok(); }
        cb.as_mut().is_some_and(|c| c.set_text(t).is_ok())
    }

    pub(super) fn bookmark_lines(&self, action: &str) {
        let vi = self.av();
        if action == "invert" {
            {
                let mut st = self.st.borrow_mut();
                let di = st.views[vi].doc;
                let d = &mut st.docs[di];
                d.bookmarks = bookmarks::invert(&d.buf.rope, &d.bookmarks);
            }
            self.render_all();
            return;
        }
        let (text, edits) = {
            let st = self.st.borrow();
            let d = &st.docs[st.views[vi].doc];
            if d.bookmarks.is_empty() { return; }
            (if action == "delete" { String::new() } else { bookmarks::collect(&d.buf.rope, &d.bookmarks) },
             if matches!(action, "cut" | "delete") { bookmarks::deletions(&d.buf.rope, &d.bookmarks) } else { vec![] })
        };
        if action != "delete" && !self.clipboard_set(text) {
            msg("無法寫入剪貼簿，書籤行尚未變更。請稍後再試。");
            return;
        }
        if matches!(action, "cut" | "delete") && !edits.is_empty() {
            // All marked lines are removed; clear before the shared edit path
            // adjusts remaining line metadata and renders both split views.
            { let mut st = self.st.borrow_mut(); let di = st.views[vi].doc; st.docs[di].bookmarks.clear(); }
            self.apply_edits(vi, edits, EditKind::Other, false);
        }
    }

    pub(super) fn clipboard_get(&self) -> Option<String> {
        let mut cb = self.clip.borrow_mut();
        if cb.is_none() { *cb = arboard::Clipboard::new().ok(); }
        cb.as_mut().and_then(|c| c.get_text().ok())
    }

    pub(super) fn copy(&self, vi: usize, cut: bool) {
        let (text, edits) = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let r = &st.docs[v.doc].buf.rope;
            let any = v.sels.iter().any(|s| !s.is_empty());
            if any {
                let parts: Vec<String> = v.sels.iter().filter(|s| !s.is_empty()).map(|s| r.slice(s.start()..s.end()).to_string()).collect();
                (parts.join("\n"), v.sels.iter().map(|s| (s.start(), s.end(), String::new())).collect::<Vec<_>>())
            } else {
                // no selection: copy/cut the whole current line (Notepad++ behaviour)
                let (a, b) = line_span(r, v.sels[v.primary.min(v.sels.len() - 1)].head);
                let mut t = r.slice(a..b).to_string();
                if !t.ends_with('\n') { t.push('\n'); }
                (t, vec![(a, b, String::new())])
            }
        };
        self.clipboard_set(text);
        if cut { self.apply_edits(vi, edits, EditKind::Other, true); }
    }

    pub(super) fn paste(&self, vi: usize) {
        let Some(t) = self.clipboard_get() else { return };
        let t = t.replace("\r\n", "\n").replace('\r', "\n");
        let edits: Vec<(usize, usize, String)> = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let lines: Vec<&str> = t.trim_end_matches('\n').split('\n').collect();
            if v.sels.len() > 1 && lines.len() == v.sels.len() {
                v.sels.iter().zip(lines).map(|(s, l)| (s.start(), s.end(), l.to_string())).collect()
            } else {
                v.sels.iter().map(|s| (s.start(), s.end(), t.clone())).collect()
            }
        };
        self.apply_edits(vi, edits, EditKind::Other, true);
        { let mut st = self.st.borrow_mut(); let d = st.views[vi].doc; st.docs[d].buf.break_group(); }
    }

    pub(super) fn delete(&self, vi: usize, forward: bool, word: bool) {
        let edits: Vec<(usize, usize, String)> = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let r = &st.docs[v.doc].buf.rope;
            v.sels.iter().filter_map(|s| {
                if !s.is_empty() { return Some((s.start(), s.end(), String::new())); }
                let h = s.head;
                if forward {
                    if h >= r.len_chars() { return None; }
                    let e = if word { word_end(r, h) } else { h + 1 };
                    Some((h, e, String::new()))
                } else {
                    if h == 0 { return None; }
                    let b = if word { word_start(r, h) } else { h - 1 };
                    Some((b, h, String::new()))
                }
            }).collect()
        };
        if edits.is_empty() { return; }
        // keep a caret for selections that were skipped (at document edges)
        self.apply_edits(vi, edits, EditKind::Delete, true);
    }

    pub(super) fn newline(&self, vi: usize) {
        let edits: Vec<(usize, usize, String)> = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let doc = &st.docs[v.doc];
            let r = &doc.buf.rope;
            let has_list_caret = doc.lang == "markdown" && v.sels.iter().any(|s| {
                let line = r.char_to_line(s.start());
                let ls = r.line_to_char(line);
                s.is_empty() && s.start() == ls + editor::line_len(r, line)
                    && editor::markdown_list_next(&r.slice(ls..s.start()).to_string()).is_some()
            });
            let code_ranges: Vec<_> = if has_list_caret {
                let text = r.to_string();
                pulldown_cmark::Parser::new(&text).into_offset_iter().filter_map(|(event, range)| {
                    if matches!(event, pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_))) { Some(range) } else { None }
                }).collect()
            } else { Vec::new() };
            v.sels.iter().map(|s| {
                let line = r.char_to_line(s.start());
                let ls = r.line_to_char(line);
                let indent: String = r.slice(ls..s.start()).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                let at_end = s.start() == ls + editor::line_len(r, line);
                let byte = r.char_to_byte(s.start());
                if doc.lang == "markdown" && s.is_empty() && at_end && !code_ranges.iter().any(|range| range.start <= byte && byte <= range.end) {
                    let current = r.slice(ls..s.start()).to_string();
                    if let Some((next, empty)) = editor::markdown_list_next(&current) {
                        if empty { return (ls, s.end(), indent); }
                        return (s.start(), s.end(), format!("\n{next}"));
                    }
                }
                (s.start(), s.end(), format!("\n{indent}"))
            }).collect()
        };
        self.apply_edits(vi, edits, EditKind::Other, true);
    }

    pub(super) fn tab(&self, vi: usize, outdent: bool) {
        let block = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let r = &st.docs[v.doc].buf.rope;
            v.sels.iter().any(|s| r.char_to_line(s.start()) != r.char_to_line(s.end())) || outdent
        };
        let json = { let st = self.st.borrow(); st.docs[st.views[vi].doc].lang == "json" };
        if !block { self.insert_text(vi, if json { "    " } else { "\t" }); return; }
        // indent / outdent every touched line, keeping the selections over the same text
        let (edits, sels_lines) = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let r = &st.docs[v.doc].buf.rope;
            let mut lines = BTreeSet::new();
            for s in &v.sels {
                let (a, mut b) = (r.char_to_line(s.start()), r.char_to_line(s.end()));
                if b > a && s.end() == r.line_to_char(b) { b -= 1; }
                for l in a..=b { lines.insert(l); }
            }
            let mut edits = Vec::new();
            for &l in &lines {
                let ls = r.line_to_char(l);
                if outdent {
                    let chars = line_chars(r, l);
                    let n = if chars.first() == Some(&'\t') { 1 } else { chars.iter().take(editor::tab()).take_while(|c| **c == ' ').count() };
                    if n > 0 { edits.push((ls, ls + n, String::new())); }
                } else if editor::line_len(r, l) > 0 || lines.len() == 1 {
                    edits.push((ls, ls, if json { "    " } else { "\t" }.to_string()));
                }
            }
            (edits, v.sels.clone())
        };
        if edits.is_empty() { return; }
        {
            let mut st = self.st.borrow_mut();
            let State { docs, views, .. } = &mut *st;
            let d = &mut docs[views[vi].doc];
            let (changes, _, _) = d.buf.edit(&sels_lines, edits, EditKind::Other);
            d.modified = true;
            let v = &mut views[vi];
            for s in v.sels.iter_mut() {
                // anchors at a line start stay at the line start
                s.anchor = editor::map_pos(s.anchor, &changes);
                s.head = editor::map_pos(s.head, &changes);
            }
            v.primary = editor::normalize(&mut v.sels, v.primary);
        }
        self.after_edit();
    }

    pub(super) fn add_caret_vertical(&self, vi: usize, up: bool) {
        self.ensure_layout(vi);
        let (ri, cells) = { let st = self.st.borrow(); let v = &st.views[vi]; let s = if up { v.sels[0] } else { *v.sels.last().unwrap() }; drop(st); self.cells_of(vi, s.head) };
        let target = if up { ri.checked_sub(1) } else { Some(ri + 1) };
        let Some(t) = target else { return };
        if t >= self.st.borrow().views[vi].layout.len() { return; }
        let pos = self.pos_in_row(vi, t, cells);
        { let mut st = self.st.borrow_mut(); let v = &mut st.views[vi]; v.sels.push(Sel::caret(pos)); v.primary = v.sels.len() - 1; v.primary = editor::normalize(&mut v.sels, v.primary); v.follow = true; }
        self.render(vi);
    }

    pub(super) fn move_carets(&self, vi: usize, k: char, ctrl: bool, shift: bool, alt: bool) {
        self.ensure_layout(vi);
        let (_, lh) = self.metrics();
        let (_, h) = self.view_size(vi);
        let page = ((h / lh) as usize).saturating_sub(1).max(1);
        // Alt+Shift+arrows: grow a column selection
        if alt && shift && matches!(k, K_UP | K_DOWN | K_LEFT | K_RIGHT) {
            let (n, prim) = { let st = self.st.borrow(); let v = &st.views[vi]; (v.sels.len(), v.sels[v.primary.min(v.sels.len() - 1)]) };
            let _ = n;
            let (ri, hc) = self.cells_of(vi, prim.head);
            let (ra, ac) = self.cells_of(vi, if let Drag::Column { .. } = self.st.borrow().views[vi].drag { prim.anchor } else { prim.anchor });
            let (anchor_row, anchor_cells) = match self.st.borrow().views[vi].drag { Drag::Column { row, cells } => (row, cells), _ => (ra, ac) };
            let (nr, nc) = match k {
                K_UP => (ri.saturating_sub(1), hc), K_DOWN => (ri + 1, hc),
                K_LEFT => (ri, (hc - 1.0).max(0.0)), _ => (ri, hc + 1.0),
            };
            let nrows = self.st.borrow().views[vi].layout.len();
            let nr = nr.min(nrows.saturating_sub(1));
            let mut sels = Vec::new();
            for r in anchor_row.min(nr)..=anchor_row.max(nr) {
                sels.push(Sel::new(self.pos_in_row(vi, r, anchor_cells), self.pos_in_row(vi, r, nc)));
            }
            let mut st = self.st.borrow_mut();
            let v = &mut st.views[vi];
            v.primary = if nr >= anchor_row { sels.len() - 1 } else { 0 };
            v.sels = sels;
            v.drag = Drag::Column { row: anchor_row, cells: anchor_cells };
            v.follow = true;
            drop(st);
            self.render(vi);
            return;
        }
        let sels = self.st.borrow().views[vi].sels.clone();
        let mut out = Vec::with_capacity(sels.len());
        for s in sels {
            let (new_head, goal) = {
                let st = self.st.borrow();
                let v = &st.views[vi];
                let r = &st.docs[v.doc].buf.rope;
                let len = r.len_chars();
                let h = s.head;
                match k {
                    K_LEFT if !shift && !s.is_empty() && !ctrl => (s.start(), None),
                    K_RIGHT if !shift && !s.is_empty() && !ctrl => (s.end(), None),
                    K_LEFT => (if ctrl { word_start(r, h) } else { h.saturating_sub(1) }, None),
                    K_RIGHT => (if ctrl { word_end(r, h) } else { (h + 1).min(len) }, None),
                    K_HOME if ctrl => (0, None),
                    K_END if ctrl => (len, None),
                    K_HOME => {
                        let line = r.char_to_line(h);
                        let ls = r.line_to_char(line);
                        let ind = line_chars(r, line).iter().take_while(|c| c.is_whitespace()).count();
                        (if h == ls + ind { ls } else { ls + ind }, None)
                    }
                    K_END => { let line = r.char_to_line(h); (r.line_to_char(line) + editor::line_len(r, line), None) }
                    _ => {
                        drop(st);
                        let (ri, cells) = self.cells_of(vi, h);
                        let goal = s.goal.map(|g| g as f32).unwrap_or(cells);
                        let n = self.st.borrow().views[vi].layout.len();
                        let step = if matches!(k, K_PGUP | K_PGDN) { page } else { 1 };
                        let up = matches!(k, K_UP | K_PGUP);
                        if up && ri == 0 { (0, None) }
                        else if !up && ri + 1 >= n { (len, None) }
                        else {
                            let t = if up { ri.saturating_sub(step) } else { (ri + step).min(n - 1) };
                            (self.pos_in_row(vi, t, goal), Some(goal as usize))
                        }
                    }
                }
            };
            out.push(Sel { anchor: if shift { s.anchor } else { new_head }, head: new_head, goal });
        }
        {
            let mut st = self.st.borrow_mut();
            let v = &mut st.views[vi];
            if matches!(k, K_PGUP | K_PGDN) {
                v.top = if k == K_PGUP { v.top.saturating_sub(page) } else { (v.top + page).min(v.layout.len().saturating_sub(1)) };
            }
            v.sels = out;
            v.primary = editor::normalize(&mut v.sels, v.primary);
            v.follow = true;
            v.drag = Drag::None;
        }
        self.unfold_at_caret(vi);
        self.render(vi);
    }

    pub(super) fn toggle_bookmark(&self) {
        let vi = self.av();
        {
            let mut st = self.st.borrow_mut();
            let State { docs, views, .. } = &mut *st;
            let v = &views[vi];
            let d = &mut docs[v.doc];
            let line = d.buf.rope.char_to_line(v.sels[v.primary.min(v.sels.len() - 1)].head.min(d.buf.len()));
            if !d.bookmarks.remove(&line) { d.bookmarks.insert(line); }
        }
        self.render_all();
    }

    pub(super) fn jump_bookmark(&self, next: bool) {
        let vi = self.av();
        let target = {
            let st = self.st.borrow();
            let v = &st.views[vi];
            let d = &st.docs[v.doc];
            let line = d.buf.rope.char_to_line(v.sels[v.primary.min(v.sels.len() - 1)].head.min(d.buf.len()));
            if next { d.bookmarks.range(line + 1..).next().or(d.bookmarks.iter().next()).copied() }
            else { d.bookmarks.range(..line).next_back().or(d.bookmarks.iter().next_back()).copied() }
        };
        if let Some(l) = target {
            { let mut st = self.st.borrow_mut(); let State { docs, views, .. } = &mut *st; let v = &mut views[vi]; let r = &docs[v.doc].buf.rope; let p = r.line_to_char(l.min(r.len_lines() - 1)); v.sels = vec![Sel::caret(p)]; v.primary = 0; v.follow = true; }
            self.unfold_at_caret(vi);
            self.render(vi);
            self.refresh_status();
        }
    }

    pub(super) fn toggle_split(&self) {
        {
            let mut st = self.st.borrow_mut();
            st.split = !st.split;
            if st.split {
                st.store_view0();
                let cur = st.cur;
                let (sels, top) = (st.views[0].sels.clone(), st.views[0].top);
                let v = &mut st.views[1];
                v.doc = cur; v.sels = sels; v.primary = 0; v.top = top; v.sx = 0.0; v.follow = true; v.ac = None;
                v.lkey = (usize::MAX, 0, 0, 0, false);
                st.active = 1;
            } else { st.active = 0; }
        }
        self.refresh_meta();
        let ui = self.ui();
        // wait for the layout to give the second view its size
        let w = self.ui();
        let me = self.me.borrow().clone();
        Timer::single_shot(Duration::from_millis(20), move || { if let Some(a) = me.upgrade() { a.render_all(); } w.invoke_focus_editor(); });
        ui.invoke_focus_editor();
    }

    // ------------------------------------------------------------ rendering

}
