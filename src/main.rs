#![cfg_attr(windows, windows_subsystem = "windows")]
slint::include_modules!();
mod preview;

use slint::{Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
enum Eol { Crlf, Lf, Cr }
impl Eol {
    fn label(self) -> &'static str { match self { Eol::Crlf => "Windows (CR LF)", Eol::Lf => "Unix (LF)", Eol::Cr => "Macintosh (CR)" } }
    fn seq(self) -> &'static str { match self { Eol::Crlf => "\r\n", Eol::Lf => "\n", Eol::Cr => "\r" } }
}

struct Doc {
    path: Option<PathBuf>,
    title: String,
    text: String, // normalized to '\n'
    eol: Eol,
    encoding: &'static encoding_rs::Encoding,
    bom: bool,
    modified: bool,
    lang: String,
    undo: Vec<String>,
    redo: Vec<String>,
    last_snap: Option<Instant>,
}

impl Doc {
    fn new(n: usize) -> Self {
        Doc { path: None, title: format!("new {}", n), text: String::new(), eol: if cfg!(windows) { Eol::Crlf } else { Eol::Lf },
              encoding: encoding_rs::UTF_8, bom: false, modified: false, lang: "text".into(), undo: vec![], redo: vec![], last_snap: None }
    }
    fn encoding_label(&self) -> String {
        let n = self.encoding.name();
        match (n, self.bom) {
            ("UTF-8", false) => "UTF-8".into(),
            ("UTF-8", true) => "UTF-8-BOM".into(),
            ("UTF-16LE", _) => "UTF-16 LE BOM".into(),
            ("UTF-16BE", _) => "UTF-16 BE BOM".into(),
            ("Big5", _) => "Big5 (繁中)".into(),
            ("GBK", _) | ("gb18030", _) => "GBK (簡中)".into(),
            ("Shift_JIS", _) => "Shift-JIS".into(),
            (o, _) => o.to_string(),
        }
    }
}

const LANGS: &[(&str, &str, &[&str])] = &[
    ("text", "Normal text file", &["txt", "log"]),
    ("rust", "Rust source file", &["rs"]),
    ("c", "C source file", &["c", "h"]),
    ("cpp", "C++ source file", &["cpp", "cc", "hpp", "cxx"]),
    ("cs", "C# source file", &["cs"]),
    ("java", "Java source file", &["java"]),
    ("python", "Python file", &["py", "pyw"]),
    ("javascript", "JavaScript file", &["js", "mjs", "ts"]),
    ("html", "Hyper Text Markup Language file", &["html", "htm", "xhtml"]),
    ("css", "Cascade Style Sheets File", &["css"]),
    ("xml", "eXtensible Markup Language file", &["xml", "svg", "xaml"]),
    ("json", "JSON file", &["json"]),
    ("markdown", "Markdown file", &["md", "markdown"]),
    ("shell", "Unix script file", &["sh", "bash"]),
    ("batch", "Batch file", &["bat", "cmd"]),
    ("ini", "MS ini file", &["ini", "cfg", "toml"]),
    ("sql", "SQL file", &["sql"]),
    ("yaml", "YAML file", &["yml", "yaml"]),
    ("slint", "Slint file", &["slint"]),
];

fn lang_from_path(p: &std::path::Path) -> String {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    LANGS.iter().find(|l| l.2.contains(&ext.as_str())).map(|l| l.0).unwrap_or("text").to_string()
}
fn lang_desc(l: &str) -> &'static str { LANGS.iter().find(|x| x.0 == l).map(|x| x.1).unwrap_or("Normal text file") }

struct State {
    docs: Vec<Doc>,
    cur: usize,
    counter: usize,
    font: f32,
    wrap: bool,
    line_numbers: bool,
    preview: bool,
    overwrite: bool,
    macro_rec: bool,
}

fn decode(bytes: &[u8]) -> (String, &'static encoding_rs::Encoding, bool) {
    if let Some((enc, bom_len)) = encoding_rs::Encoding::for_bom(bytes) {
        let (s, _) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return (s.into_owned(), enc, true);
    }
    if let Ok(s) = std::str::from_utf8(bytes) { return (s.to_string(), encoding_rs::UTF_8, false); }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    det.feed(bytes, true);
    let enc = det.guess(Some(b"tw"), chardetng::Utf8Detection::Allow);
    let (s, _, _) = enc.decode(bytes);
    (s.into_owned(), enc, false)
}

fn encode(doc: &Doc) -> Vec<u8> {
    let text = doc.text.replace('\n', doc.eol.seq());
    let name = doc.encoding.name();
    if name == "UTF-16LE" || name == "UTF-16BE" {
        let le = name == "UTF-16LE";
        let mut v = if le { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
        for u in text.encode_utf16() { v.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() }); }
        return v;
    }
    let mut v = Vec::new();
    if doc.bom && name == "UTF-8" { v.extend_from_slice(&[0xEF, 0xBB, 0xBF]); }
    let (b, _, _) = doc.encoding.encode(&text);
    v.extend_from_slice(&b);
    v
}

fn detect_eol(s: &str) -> Eol {
    if s.contains("\r\n") { Eol::Crlf } else if s.contains('\n') { Eol::Lf } else if s.contains('\r') { Eol::Cr } else if cfg!(windows) { Eol::Crlf } else { Eol::Lf }
}

fn mi(label: &str, shortcut: &str, action: &str) -> MenuItem {
    MenuItem { label: label.into(), shortcut: shortcut.into(), action: action.into(), separator: false, checked: false }
}
fn sep() -> MenuItem { MenuItem { label: "".into(), shortcut: "".into(), action: "".into(), separator: true, checked: false } }
fn chk(label: &str, shortcut: &str, action: &str, c: bool) -> MenuItem { let mut m = mi(label, shortcut, action); m.checked = c; m }

fn build_menus(st: &State) -> Vec<Menu> {
    let d = &st.docs[st.cur];
    let menu = |t: &str, items: Vec<MenuItem>| Menu { title: t.into(), items: ModelRc::new(VecModel::from(items)) };
    let mut lang_items: Vec<MenuItem> = LANGS.iter().map(|l| chk(l.0, "", &format!("lang:{}", l.0), d.lang == l.0)).collect();
    lang_items.insert(1, sep());
    let mut win_items = vec![mi("下一個分頁", "Ctrl+Tab", "nexttab"), mi("上一個分頁", "", "prevtab"), sep()];
    for (i, doc) in st.docs.iter().enumerate() {
        win_items.push(chk(&format!("{}: {}", i + 1, doc.title), "", &format!("tab:{}", i), i == st.cur));
    }
    vec![
        menu("檔案", vec![mi("開新檔案", "Ctrl+N", "new"), mi("開啟舊檔...", "Ctrl+O", "open"), mi("重新載入", "", "reload"), sep(),
            mi("儲存檔案", "Ctrl+S", "save"), mi("另存新檔...", "Ctrl+Shift+S", "saveas"), mi("全部儲存", "", "saveall"), sep(),
            mi("關閉", "Ctrl+W", "close"), mi("全部關閉", "", "closeall"), mi("關閉其他分頁", "", "closeothers"), sep(), mi("結束", "Alt+F4", "exit")]),
        menu("編輯", vec![mi("復原", "Ctrl+Z", "undo"), mi("取消復原", "Ctrl+Y", "redo"), sep(),
            mi("剪下", "Ctrl+X", "cut"), mi("複製", "Ctrl+C", "copy"), mi("貼上", "Ctrl+V", "paste"), mi("全選", "Ctrl+A", "selectall"), sep(),
            mi("複製目前行", "Ctrl+D", "dupline"), mi("刪除目前行", "", "delline"), mi("轉成大寫", "", "upper"), mi("轉成小寫", "", "lower"),
            mi("移除行尾空白", "", "trimtrail"), mi("排序行 (遞增)", "", "sortlines"), sep(),
            mi("換行格式 → Windows (CR LF)", "", "eol:crlf"), mi("換行格式 → Unix (LF)", "", "eol:lf"), mi("換行格式 → Macintosh (CR)", "", "eol:cr")]),
        menu("搜尋", vec![mi("尋找...", "Ctrl+F", "find"), mi("找下一個", "F3", "findnext"), mi("找上一個", "Shift+F3", "findprev"),
            mi("取代...", "Ctrl+H", "replace"), sep(), mi("跳至...", "Ctrl+G", "goto")]),
        menu("檢視", vec![chk("自動換行", "", "wrap", st.wrap), chk("顯示行號", "", "linenumbers", st.line_numbers),
            chk("HTML / Markdown 預覽", "", "preview", st.preview), sep(),
            mi("放大", "Ctrl++", "zoomin"), mi("縮小", "Ctrl+-", "zoomout"), mi("恢復預設縮放", "Ctrl+0", "zoomreset")]),
        menu("編碼", vec![chk("UTF-8", "", "enc:utf8", d.encoding == encoding_rs::UTF_8 && !d.bom), chk("UTF-8-BOM", "", "enc:utf8bom", d.encoding == encoding_rs::UTF_8 && d.bom),
            chk("UTF-16 LE BOM", "", "enc:utf16le", d.encoding == encoding_rs::UTF_16LE), chk("UTF-16 BE BOM", "", "enc:utf16be", d.encoding == encoding_rs::UTF_16BE), sep(),
            chk("Big5 (繁體中文)", "", "enc:big5", d.encoding == encoding_rs::BIG5), chk("GBK (簡體中文)", "", "enc:gbk", d.encoding == encoding_rs::GBK),
            chk("Shift-JIS (日文)", "", "enc:sjis", d.encoding == encoding_rs::SHIFT_JIS), chk("Windows-1252 (西歐)", "", "enc:1252", d.encoding == encoding_rs::WINDOWS_1252)]),
        menu("語言", lang_items),
        menu("設定", vec![chk("自動換行", "", "wrap", st.wrap), chk("顯示行號", "", "linenumbers", st.line_numbers), chk("覆寫模式 (OVR 指示)", "Insert", "toggleovr", st.overwrite)]),
        menu("工具", vec![mi("統計 (字數/行數)", "", "stats"), mi("插入日期時間", "", "datetime")]),
        menu("巨集", vec![chk("開始/停止錄製 (未實作)", "", "macro", st.macro_rec)]),
        menu("執行", vec![mi("在預設瀏覽器中開啟", "", "runbrowser"), mi("開啟所在資料夾", "", "openfolder")]),
        menu("外掛", vec![mi("(尚無外掛)", "", "none")]),
        menu("視窗", win_items),
        menu("?", vec![mi("關於 Slint Notepad...", "F1", "about")]),
    ]
}

struct App { ui: slint::Weak<AppWindow>, st: RefCell<State> }

impl App {
    fn ui(&self) -> AppWindow { self.ui.upgrade().unwrap() }

    fn refresh_all(&self) {
        let ui = self.ui();
        {
            let st = self.st.borrow();
            let d = &st.docs[st.cur];
            ui.set_editor_text(d.text.clone().into());
            ui.invoke_scroll_top();
        }
        self.refresh_meta();
        self.refresh_preview();
        self.refresh_status(0);
        ui.invoke_focus_editor();
    }

    fn refresh_meta(&self) {
        let ui = self.ui();
        let st = self.st.borrow();
        let d = &st.docs[st.cur];
        let tabs: Vec<TabInfo> = st.docs.iter().map(|d| TabInfo { title: d.title.clone().into(), modified: d.modified }).collect();
        ui.set_tabs(ModelRc::new(VecModel::from(tabs)));
        ui.set_current_tab(st.cur as i32);
        ui.set_menus(ModelRc::new(VecModel::from(build_menus(&st))));
        let name = d.path.as_ref().map(|p| p.display().to_string()).unwrap_or(d.title.clone());
        ui.set_window_title(format!("{}{} - Slint Notepad", if d.modified { "*" } else { "" }, name).into());
        ui.set_word_wrap(st.wrap);
        ui.set_show_line_numbers(st.line_numbers && !st.wrap);
        ui.set_editor_font_size(st.font);
        ui.set_show_preview(st.preview);
        ui.set_overwrite(st.overwrite);
        ui.set_st_type(lang_desc(&d.lang).into());
        ui.set_st_eol(d.eol.label().into());
        ui.set_st_encoding(d.encoding_label().into());
        let lines = d.text.matches('\n').count() + 1;
        ui.set_total_lines(lines as i32);
        let ln: String = (1..=lines).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        if ui.get_line_numbers().as_str() != ln { ui.set_line_numbers(ln.into()); }
    }

    fn refresh_status(&self, byte: usize) {
        let ui = self.ui();
        let st = self.st.borrow();
        let d = &st.docs[st.cur];
        let t = &d.text;
        let byte = byte.min(t.len());
        let byte = (0..=byte).rev().find(|b| t.is_char_boundary(*b)).unwrap_or(0);
        let before = &t[..byte];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map(|s| s.chars().count()).unwrap_or(0) + 1;
        let pos = before.chars().count() + 1;
        let lines = t.matches('\n').count() + 1;
        let len = t.chars().count() + if d.eol == Eol::Crlf { lines - 1 } else { 0 };
        ui.set_st_length(format!("length : {}    lines : {}", len, lines).into());
        let (a, c) = (ui.get_anchor_byte() as usize, ui.get_cursor_byte() as usize);
        let sel = if a != c && a <= t.len() && c <= t.len() {
            let (s, e) = (a.min(c), a.max(c));
            if t.is_char_boundary(s) && t.is_char_boundary(e) { format!("    Sel : {}", t[s..e].chars().count()) } else { String::new() }
        } else { String::new() };
        ui.set_st_pos(format!("Ln : {}    Col : {}    Pos : {}{}", line, col, pos, sel).into());
    }

    fn refresh_preview(&self) {
        let ui = self.ui();
        let st = self.st.borrow();
        if !st.preview { return; }
        let d = &st.docs[st.cur];
        let blocks = match d.lang.as_str() {
            "markdown" => preview::markdown(&d.text),
            "html" | "xml" => preview::html(&d.text),
            _ => {
                let mut b = preview::markdown("*此檔案類型不支援預覽，請將語言設為 Markdown 或 HTML。*");
                b.truncate(1); b
            }
        };
        ui.set_preview_title(d.title.clone().into());
        ui.set_preview_blocks(ModelRc::new(VecModel::from(blocks)));
    }

    fn on_edit(&self, text: String) {
        {
            let mut st = self.st.borrow_mut();
            let cur = st.cur;
            let d = &mut st.docs[cur];
            if d.text == text { return; }
            let now = Instant::now();
            if d.last_snap.map(|t| now.duration_since(t).as_millis() > 700).unwrap_or(true) {
                let old = std::mem::take(&mut d.text);
                d.undo.push(old);
                if d.undo.len() > 300 { d.undo.remove(0); }
                d.redo.clear();
            }
            d.last_snap = Some(now);
            d.text = text;
            d.modified = true;
        }
        self.refresh_meta();
        self.refresh_preview();
    }

    /// replace the whole text programmatically (records undo)
    fn set_text(&self, text: String) {
        {
            let mut st = self.st.borrow_mut();
            let cur = st.cur;
            let d = &mut st.docs[cur];
            if d.text == text { return; }
            let old = std::mem::replace(&mut d.text, text);
            d.undo.push(old);
            d.redo.clear();
            d.last_snap = None;
            d.modified = true;
        }
        let byte = self.ui().get_cursor_byte();
        let t = self.st.borrow().docs[self.st.borrow().cur].text.clone();
        self.ui().set_editor_text(t.clone().into());
        self.refresh_meta();
        self.refresh_preview();
        let b = (byte as usize).min(t.len());
        let b = (0..=b).rev().find(|x| t.is_char_boundary(*x)).unwrap_or(0) as i32;
        self.ui().invoke_select_range(b, b);
    }

    fn open_path(&self, path: PathBuf) {
        let bytes = match std::fs::read(&path) { Ok(b) => b, Err(e) => { msg(&format!("無法開啟檔案：{}", e)); return; } };
        let (s, enc, bom) = decode(&bytes);
        let eol = detect_eol(&s);
        let text = s.replace("\r\n", "\n").replace('\r', "\n");
        {
            let mut st = self.st.borrow_mut();
            if let Some(i) = st.docs.iter().position(|d| d.path.as_ref() == Some(&path)) { st.cur = i; }
            else {
                let mut d = Doc::new(0);
                d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                d.lang = lang_from_path(&path);
                d.path = Some(path); d.text = text; d.eol = eol; d.encoding = enc; d.bom = bom;
                let c = st.cur;
                if st.docs[c].path.is_none() && !st.docs[c].modified && st.docs[c].text.is_empty() { st.docs[c] = d; }
                else { st.docs.push(d); st.cur = st.docs.len() - 1; }
                if matches!(st.docs[st.cur].lang.as_str(), "markdown" | "html") { st.preview = true; }
            }
        }
        self.refresh_all();
    }

    fn save(&self, idx: usize, save_as: bool) -> bool {
        let path = {
            let st = self.st.borrow();
            let d = &st.docs[idx];
            match (&d.path, save_as) {
                (Some(p), false) => Some(p.clone()),
                _ => rfd::FileDialog::new().set_title("另存新檔").set_file_name(&d.title).save_file(),
            }
        };
        let Some(path) = path else { return false };
        let bytes = encode(&self.st.borrow().docs[idx]);
        if let Err(e) = std::fs::write(&path, bytes) { msg(&format!("儲存失敗：{}", e)); return false; }
        {
            let mut st = self.st.borrow_mut();
            let d = &mut st.docs[idx];
            d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if d.path.as_ref() != Some(&path) { d.lang = lang_from_path(&path); }
            d.path = Some(path);
            d.modified = false;
        }
        self.refresh_meta();
        self.refresh_preview();
        true
    }

    /// returns false when cancelled
    fn close(&self, idx: usize) -> bool {
        let (modified, title) = { let st = self.st.borrow(); (st.docs[idx].modified, st.docs[idx].title.clone()) };
        if modified {
            let r = rfd::MessageDialog::new().set_title("Slint Notepad").set_description(format!("是否儲存「{}」的變更？", title))
                .set_buttons(rfd::MessageButtons::YesNoCancel).show();
            match r {
                rfd::MessageDialogResult::Yes => { if !self.save(idx, false) { return false; } }
                rfd::MessageDialogResult::No => {}
                _ => return false,
            }
        }
        {
            let mut st = self.st.borrow_mut();
            st.docs.remove(idx);
            if st.docs.is_empty() { st.counter += 1; let n = st.counter; st.docs.push(Doc::new(n)); }
            if st.cur >= st.docs.len() || st.cur > idx { st.cur = st.cur.saturating_sub(1).min(st.docs.len() - 1); }
        }
        self.refresh_all();
        true
    }

    fn select_tab(&self, i: usize) {
        { let mut st = self.st.borrow_mut(); if i >= st.docs.len() { return; } st.cur = i; }
        self.refresh_all();
    }

    fn find(&self, forward: bool) -> bool {
        let ui = self.ui();
        let needle = ui.get_find_text().to_string();
        if needle.is_empty() { return false; }
        let mc = ui.get_match_case();
        let st = self.st.borrow();
        let t = &st.docs[st.cur].text;
        let (hay, nd) = if mc { (t.clone(), needle.clone()) } else { (t.to_lowercase(), needle.to_lowercase()) };
        if hay.len() != t.len() { /* lowercase changed lengths: fall back to case-sensitive */ }
        let (hay, nd) = if hay.len() == t.len() { (hay, nd) } else { (t.clone(), needle.clone()) };
        let (a, c) = (ui.get_anchor_byte() as usize, ui.get_cursor_byte() as usize);
        let (s, e) = (a.min(c), a.max(c));
        let found = if forward {
            hay.get(e..).and_then(|h| h.find(&nd)).map(|p| p + e).or_else(|| hay.find(&nd))
        } else {
            hay.get(..s).and_then(|h| h.rfind(&nd)).or_else(|| hay.rfind(&nd))
        };
        drop(st);
        match found {
            Some(p) => { ui.invoke_select_range(p as i32, (p + nd.len()) as i32); ui.set_find_status("".into()); true }
            None => { ui.set_find_status(format!("找不到「{}」", needle).into()); false }
        }
    }

    fn action(self: &Rc<Self>, a: &str) {
        let ui = self.ui();
        let cur = self.st.borrow().cur;
        match a {
            "new" => { { let mut st = self.st.borrow_mut(); st.counter += 1; let n = st.counter; st.docs.push(Doc::new(n)); st.cur = st.docs.len() - 1; } self.refresh_all(); }
            "open" => {
                if let Some(files) = rfd::FileDialog::new().set_title("開啟舊檔")
                    .add_filter("所有檔案", &["*"]).add_filter("文字/程式碼", &["txt", "md", "html", "htm", "rs", "py", "js", "json", "xml", "css", "c", "cpp", "h", "toml", "ini", "log"])
                    .pick_files() { for f in files { self.open_path(f); } }
            }
            "reload" => { if let Some(p) = self.st.borrow().docs[cur].path.clone() { self.st.borrow_mut().docs[cur].modified = false; let mut st = self.st.borrow_mut(); let d = st.docs.remove(cur); st.docs.insert(cur, Doc { path: None, ..d }); drop(st); self.st.borrow_mut().docs[cur].path = Some(p.clone()); if let Ok(b) = std::fs::read(&p) { let (s, enc, bom) = decode(&b); let mut st = self.st.borrow_mut(); let d = &mut st.docs[cur]; d.eol = detect_eol(&s); d.text = s.replace("\r\n", "\n").replace('\r', "\n"); d.encoding = enc; d.bom = bom; } self.refresh_all(); } }
            "save" => { self.save(cur, false); }
            "saveas" => { self.save(cur, true); }
            "saveall" => { let n = self.st.borrow().docs.len(); for i in 0..n { if self.st.borrow().docs[i].modified { self.save(i, false); } } }
            "close" => { self.close(cur); }
            "closeall" => { while self.st.borrow().docs.len() > 1 || self.st.borrow().docs[0].modified || self.st.borrow().docs[0].path.is_some() { let i = self.st.borrow().docs.len() - 1; if !self.close(i) { break; } } }
            "closeothers" => { let mut i = self.st.borrow().docs.len(); while i > 0 { i -= 1; if i != self.st.borrow().cur { if !self.close(i) { break; } } } }
            "exit" => { if self.confirm_exit() { let _ = slint::quit_event_loop(); } }
            "undo" | "redo" => {
                let ok = {
                    let mut st = self.st.borrow_mut();
                    let d = &mut st.docs[cur];
                    let (from, to) = if a == "undo" { (&mut d.undo, &mut d.redo) } else { (&mut d.redo, &mut d.undo) };
                    if let Some(t) = from.pop() { to.push(std::mem::replace(&mut d.text, t)); d.modified = true; d.last_snap = None; true } else { false }
                };
                if ok { let t = self.st.borrow().docs[cur].text.clone(); ui.set_editor_text(t.into()); self.refresh_meta(); self.refresh_preview(); }
            }
            "cut" => ui.invoke_do_cut(),
            "copy" => ui.invoke_do_copy(),
            "paste" => ui.invoke_do_paste(),
            "selectall" => ui.invoke_do_select_all(),
            "dupline" | "delline" => {
                let t = self.st.borrow().docs[cur].text.clone();
                let b = (ui.get_cursor_byte() as usize).min(t.len());
                let ls = t[..b].rfind('\n').map(|x| x + 1).unwrap_or(0);
                let le = t[b..].find('\n').map(|x| x + b).unwrap_or(t.len());
                let line = &t[ls..le];
                let nt = if a == "dupline" { format!("{}\n{}{}", &t[..le], line, &t[le..]) }
                    else { let end = if le < t.len() { le + 1 } else { le }; let start = if end == le && ls > 0 { ls - 1 } else { ls }; format!("{}{}", &t[..start], &t[end..]) };
                self.set_text(nt);
            }
            "upper" | "lower" | "trimtrail" | "sortlines" => {
                let t = self.st.borrow().docs[cur].text.clone();
                let (an, cu) = (ui.get_anchor_byte() as usize, ui.get_cursor_byte() as usize);
                let (s, e) = if an != cu { (an.min(cu), an.max(cu)) } else { (0, t.len()) };
                let seg = &t[s..e];
                let r = match a {
                    "upper" => seg.to_uppercase(),
                    "lower" => seg.to_lowercase(),
                    "trimtrail" => seg.split('\n').map(|l| l.trim_end()).collect::<Vec<_>>().join("\n"),
                    _ => { let mut v: Vec<&str> = seg.split('\n').collect(); v.sort(); v.join("\n") }
                };
                self.set_text(format!("{}{}{}", &t[..s], r, &t[e..]));
            }
            x if x.starts_with("eol:") => {
                { let mut st = self.st.borrow_mut(); let d = &mut st.docs[cur]; d.eol = match &x[4..] { "crlf" => Eol::Crlf, "lf" => Eol::Lf, _ => Eol::Cr }; d.modified = true; }
                self.refresh_meta();
            }
            x if x.starts_with("enc:") => {
                { let mut st = self.st.borrow_mut(); let d = &mut st.docs[cur];
                  let (e, b) = match &x[4..] { "utf8" => (encoding_rs::UTF_8, false), "utf8bom" => (encoding_rs::UTF_8, true), "utf16le" => (encoding_rs::UTF_16LE, true),
                      "utf16be" => (encoding_rs::UTF_16BE, true), "big5" => (encoding_rs::BIG5, false), "gbk" => (encoding_rs::GBK, false),
                      "sjis" => (encoding_rs::SHIFT_JIS, false), _ => (encoding_rs::WINDOWS_1252, false) };
                  d.encoding = e; d.bom = b; d.modified = true; }
                self.refresh_meta();
            }
            x if x.starts_with("lang:") => { { let mut st = self.st.borrow_mut(); st.docs[cur].lang = x[5..].to_string(); } self.refresh_meta(); self.refresh_preview(); }
            x if x.starts_with("tab:") => { if let Ok(i) = x[4..].parse() { self.select_tab(i); } }
            "nexttab" | "prevtab" => { let n = self.st.borrow().docs.len(); let i = if a == "nexttab" { (cur + 1) % n } else { (cur + n - 1) % n }; self.select_tab(i); }
            "find" | "replace" => { ui.set_replace_mode(a == "replace"); ui.set_find_visible(true); ui.set_find_status("".into()); }
            "findnext" => { self.find(true); }
            "findprev" => { self.find(false); }
            "goto" => ui.set_goto_visible(true),
            "wrap" => { { let mut st = self.st.borrow_mut(); st.wrap = !st.wrap; } self.refresh_meta(); }
            "linenumbers" => { { let mut st = self.st.borrow_mut(); st.line_numbers = !st.line_numbers; } self.refresh_meta(); }
            "preview" => { { let mut st = self.st.borrow_mut(); st.preview = !st.preview; } self.refresh_meta(); self.refresh_preview(); }
            "zoomin" => { { let mut st = self.st.borrow_mut(); st.font = (st.font + 1.0).min(48.0); } self.refresh_meta(); }
            "zoomout" => { { let mut st = self.st.borrow_mut(); st.font = (st.font - 1.0).max(6.0); } self.refresh_meta(); }
            "zoomreset" => { { let mut st = self.st.borrow_mut(); st.font = 14.0; } self.refresh_meta(); }
            "toggleovr" => { { let mut st = self.st.borrow_mut(); st.overwrite = !st.overwrite; } self.refresh_meta(); }
            "stats" => {
                let st = self.st.borrow(); let t = &st.docs[cur].text;
                msg(&format!("字元數：{}\n字數：{}\n行數：{}\n位元組 (UTF-8)：{}", t.chars().count(), t.split_whitespace().count(), t.matches('\n').count() + 1, t.len()));
            }
            "datetime" => {
                let t = self.st.borrow().docs[cur].text.clone();
                let b = (ui.get_cursor_byte() as usize).min(t.len());
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let stamp = format_unix(now);
                self.set_text(format!("{}{}{}", &t[..b], stamp, &t[b..]));
            }
            "macro" => { { let mut st = self.st.borrow_mut(); st.macro_rec = !st.macro_rec; } self.refresh_meta(); }
            "runbrowser" | "openfolder" => {
                let p = self.st.borrow().docs[cur].path.clone();
                if let Some(p) = p {
                    let target = if a == "openfolder" { p.parent().map(|x| x.to_path_buf()).unwrap_or(p) } else { p };
                    open_external(&target);
                } else { msg("請先儲存檔案。"); }
            }
            "about" => ui.set_about_visible(true),
            _ => {}
        }
    }

    fn confirm_exit(&self) -> bool {
        loop {
            let idx = self.st.borrow().docs.iter().position(|d| d.modified);
            match idx { Some(i) => { self.st.borrow_mut().cur = i; self.refresh_all(); if !self.close(i) { return false; } } None => return true }
        }
    }
}

fn format_unix(secs: u64) -> String {
    // UTC date-time without extra crates (civil-from-days algorithm)
    let days = (secs / 86400) as i64; let rem = secs % 86400;
    let z = days + 719468; let era = z.div_euclid(146097); let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1; let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

fn open_external(p: &std::path::Path) {
    #[cfg(windows)] let _ = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(p).spawn();
    #[cfg(target_os = "macos")] let _ = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))] let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

fn msg(s: &str) {
    rfd::MessageDialog::new().set_title("Slint Notepad").set_description(s).set_buttons(rfd::MessageButtons::Ok).show();
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let st = State { docs: vec![Doc::new(1)], cur: 0, counter: 1, font: 14.0, wrap: false, line_numbers: true, preview: false, overwrite: false, macro_rec: false };
    let app = Rc::new(App { ui: ui.as_weak(), st: RefCell::new(st) });

    macro_rules! cb { ($m:ident, |$a:ident $(, $p:ident : $t:ty)*| $body:block) => {{ let $a = app.clone(); ui.$m(move |$($p: $t),*| $body); }}; }
    cb!(on_menu_action, |a, s: SharedString| { a.action(&s) });
    cb!(on_text_edited, |a, s: SharedString| { a.on_edit(s.to_string()) });
    cb!(on_cursor_moved, |a, b: i32| { a.refresh_status(b.max(0) as usize) });
    cb!(on_tab_selected, |a, i: i32| { a.select_tab(i as usize) });
    cb!(on_tab_close, |a, i: i32| { a.close(i as usize); });
    cb!(on_find_next, |a, f: bool| { a.find(f); });
    cb!(on_count_matches, |a| {
        let ui = a.ui(); let n = ui.get_find_text().to_string();
        if !n.is_empty() { let st = a.st.borrow(); let t = &st.docs[st.cur].text;
            let c = if ui.get_match_case() { t.matches(&n).count() } else { t.to_lowercase().matches(&n.to_lowercase()).count() };
            ui.set_find_status(format!("計數：{} 個符合項目", c).into()); }
    });
    cb!(on_replace_one, |a| {
        let ui = a.ui(); let n = ui.get_find_text().to_string(); let r = ui.get_replace_text().to_string();
        let t = a.st.borrow().docs[a.st.borrow().cur].text.clone();
        let (an, cu) = (ui.get_anchor_byte() as usize, ui.get_cursor_byte() as usize);
        let (s, e) = (an.min(cu), an.max(cu));
        let sel = t.get(s..e).unwrap_or("");
        let hit = if ui.get_match_case() { sel == n } else { sel.to_lowercase() == n.to_lowercase() };
        if hit && !n.is_empty() {
            a.set_text(format!("{}{}{}", &t[..s], r, &t[e..]));
            let p = (s + r.len()) as i32; ui.invoke_select_range(p, p);
        }
        a.find(true);
    });
    cb!(on_replace_all, |a| {
        let ui = a.ui(); let n = ui.get_find_text().to_string(); let r = ui.get_replace_text().to_string();
        if n.is_empty() { return; }
        let t = a.st.borrow().docs[a.st.borrow().cur].text.clone();
        let (nt, c) = if ui.get_match_case() { (t.replace(&n, &r), t.matches(&n).count()) } else {
            let low = t.to_lowercase(); let nl = n.to_lowercase();
            if low.len() != t.len() { (t.replace(&n, &r), t.matches(&n).count()) } else {
                let mut out = String::new(); let mut last = 0; let mut c = 0;
                for (i, _) in low.match_indices(&nl) { out.push_str(&t[last..i]); out.push_str(&r); last = i + n.len(); c += 1; }
                out.push_str(&t[last..]); (out, c) } };
        a.set_text(nt);
        ui.set_find_status(format!("已取代 {} 處", c).into());
    });
    cb!(on_goto_line, |a, s: SharedString| {
        let ui = a.ui();
        if let Ok(n) = s.trim().parse::<usize>() {
            let t = a.st.borrow().docs[a.st.borrow().cur].text.clone();
            let off = if n <= 1 { 0 } else { t.match_indices('\n').nth(n - 2).map(|(i, _)| i + 1).unwrap_or(t.len()) };
            ui.set_goto_visible(false);
            ui.invoke_select_range(off as i32, off as i32);
        }
    });

    // open files passed on command line
    for arg in std::env::args().skip(1) { app.open_path(PathBuf::from(arg)); }
    app.refresh_all();

    let a2 = app.clone();
    ui.window().on_close_requested(move || if a2.confirm_exit() { slint::CloseRequestResponse::HideWindow } else { slint::CloseRequestResponse::KeepWindowShown });
    let _ = Model::row_count(&VecModel::<i32>::default());
    ui.run()
}
