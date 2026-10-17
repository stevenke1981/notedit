#![cfg_attr(windows, windows_subsystem = "windows")]
slint::include_modules!();
mod preview;
mod highlight;
mod search;
mod settings;
mod file_io;
mod recovery;
mod file_watch;

use i_slint_backend_winit::{WinitWindowAccessor, WinitWindowEventResult};

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
    hl: highlight::Hl,
    cursor: usize,
    disk_stamp: Option<file_watch::FileStamp>,
}

impl Doc {
    fn new(n: usize) -> Self {
        Doc { path: None, title: format!("new {}", n), text: String::new(), eol: if cfg!(windows) { Eol::Crlf } else { Eol::Lf },
              encoding: encoding_rs::UTF_8, bom: false, modified: false, lang: "text".into(), undo: vec![], redo: vec![], last_snap: None, hl: Default::default(), cursor: 0, disk_stamp: None }
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
    macro_play: bool,
    macro_buf: Vec<String>,
    dark: bool,
    marks: Vec<usize>,
    hits: Vec<(usize, usize, usize)>,
    cfg: settings::Settings,
    font_family: String,
    font_choices: Vec<String>,
    ctx_tab: usize,
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
    MenuItem { label: label.into(), shortcut: shortcut.into(), action: action.into(), separator: false, checked: false, enabled: true }
}
fn sep() -> MenuItem { MenuItem { label: "".into(), shortcut: "".into(), action: "".into(), separator: true, checked: false, enabled: false } }
fn en(label: &str, shortcut: &str, action: &str, e: bool) -> MenuItem { let mut m = mi(label, shortcut, action); m.enabled = e; m }
fn chk(label: &str, shortcut: &str, action: &str, c: bool) -> MenuItem { let mut m = mi(label, shortcut, action); m.checked = c; m }

const BUNDLED_FONTS: &[&str] = &["JetBrains Mono", "DejaVu Sans Mono"];
const SYSTEM_MONO_FONTS: &[&str] = &["Cascadia Code", "Cascadia Mono", "Consolas", "Fira Code", "Fira Mono", "Source Code Pro", "Hack",
    "Ubuntu Mono", "Noto Sans Mono", "Menlo", "SF Mono", "Monaco", "Courier New", "Sarasa Mono TC", "Roboto Mono", "Iosevka"];

/// Bundled fonts plus well-known programming fonts that are installed on this system.
fn detect_fonts() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let installed: std::collections::HashSet<String> = db.faces().flat_map(|f| f.families.iter().map(|(n, _)| n.to_lowercase())).collect();
    BUNDLED_FONTS.iter().map(|s| s.to_string())
        .chain(SYSTEM_MONO_FONTS.iter().filter(|f| installed.contains(&f.to_lowercase())).map(|s| s.to_string()))
        .collect()
}

fn recent_items(st: &State) -> Vec<MenuItem> {
    if st.cfg.recent.is_empty() { return vec![mi("(無最近開啟的檔案)", "", "none")]; }
    let mut v: Vec<MenuItem> = st.cfg.recent.iter().enumerate()
        .map(|(i, p)| mi(&format!("{}: {}", i + 1, p.display()), "", &format!("recent:{}", i))).collect();
    v.push(mi("清除最近開啟清單", "", "clearrecent"));
    v
}

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
            mi("關閉", "Ctrl+W", "close"), mi("全部關閉", "", "closeall"), mi("關閉其他分頁", "", "closeothers"), sep(),
            mi("匯出為 HTML...", "", "exporthtml"), mi("列印 (以瀏覽器)...", "Ctrl+P", "print"), sep()]
            .into_iter().chain(recent_items(st)).chain([sep(), mi("結束", "Alt+F4", "exit")]).collect()),
        menu("編輯", vec![mi("復原", "Ctrl+Z", "undo"), mi("取消復原", "Ctrl+Y", "redo"), sep(),
            mi("剪下", "Ctrl+X", "cut"), mi("複製", "Ctrl+C", "copy"), mi("貼上", "Ctrl+V", "paste"), mi("全選", "Ctrl+A", "selectall"), sep(),
            mi("複製目前行", "Ctrl+D", "dupline"), mi("刪除目前行", "", "delline"), mi("轉成大寫", "", "upper"), mi("轉成小寫", "", "lower"),
            mi("移除行尾空白", "", "trimtrail"), mi("排序行 (遞增)", "", "sortlines"), sep(),
            mi("換行格式 → Windows (CR LF)", "", "eol:crlf"), mi("換行格式 → Unix (LF)", "", "eol:lf"), mi("換行格式 → Macintosh (CR)", "", "eol:cr")]),
        menu("搜尋", vec![mi("尋找...", "Ctrl+F", "find"), mi("找下一個", "F3", "findnext"), mi("找上一個", "Shift+F3", "findprev"),
            mi("取代...", "Ctrl+H", "replace"), mi("在所有開啟文件中尋找", "", "findall"), sep(), mi("跳至...", "Ctrl+G", "goto"), mi("跳至對應括號", "Ctrl+B", "gotobrace")]),
        menu("檢視", vec![chk("自動換行", "", "wrap", st.wrap), chk("顯示行號", "", "linenumbers", st.line_numbers),
            chk("HTML / Markdown 預覽", "", "preview", st.preview), sep(),
            chk("深色主題", "", "dark", st.dark), sep(),
            mi("放大", "Ctrl++", "zoomin"), mi("縮小", "Ctrl+-", "zoomout"), mi("恢復預設縮放", "Ctrl+0", "zoomreset")]),
        menu("編碼", vec![chk("UTF-8", "", "enc:utf8", d.encoding == encoding_rs::UTF_8 && !d.bom), chk("UTF-8-BOM", "", "enc:utf8bom", d.encoding == encoding_rs::UTF_8 && d.bom),
            chk("UTF-16 LE BOM", "", "enc:utf16le", d.encoding == encoding_rs::UTF_16LE), chk("UTF-16 BE BOM", "", "enc:utf16be", d.encoding == encoding_rs::UTF_16BE), sep(),
            chk("Big5 (繁體中文)", "", "enc:big5", d.encoding == encoding_rs::BIG5), chk("GBK (簡體中文)", "", "enc:gbk", d.encoding == encoding_rs::GBK),
            chk("Shift-JIS (日文)", "", "enc:sjis", d.encoding == encoding_rs::SHIFT_JIS), chk("Windows-1252 (西歐)", "", "enc:1252", d.encoding == encoding_rs::WINDOWS_1252)]),
        menu("語言", lang_items),
        menu("設定", vec![chk("自動換行", "", "wrap", st.wrap), chk("顯示行號", "", "linenumbers", st.line_numbers), chk("深色主題", "", "dark", st.dark),
            chk("覆寫模式", "Insert", "toggleovr", st.overwrite), chk("啟動時還原上次工作階段", "", "restoresession", st.cfg.restore_session), sep(),
            mi("開啟設定檔所在資料夾", "", "openconfig"), sep()]
            .into_iter().chain(st.font_choices.iter().map(|f| chk(&format!("字型：{}{}", f, if BUNDLED_FONTS.contains(&f.as_str()) { "（內建）" } else { "" }), "", &format!("font:{}", f), *f == st.font_family))).collect()),
        menu("工具", vec![mi("統計 (字數/行數)", "", "stats"), mi("插入日期時間", "", "datetime")]),
        menu("巨集", vec![chk(if st.macro_rec { "停止錄製" } else { "開始錄製" }, "Ctrl+Shift+R", "macrorec", st.macro_rec),
            mi(&format!("播放 ({} 個按鍵)", st.macro_buf.len()), "Ctrl+Shift+P", "macroplay"), mi("重複播放 10 次", "", "macroplay10"), mi("清除巨集", "", "macroclear")]),
        menu("執行", vec![mi("在預設瀏覽器中開啟", "", "runbrowser"), mi("開啟所在資料夾", "", "openfolder")]),
        menu("外掛", vec![mi("(尚無外掛)", "", "none")]),
        menu("視窗", win_items),
        menu("?", vec![mi("關於 NotEdit...", "F1", "about")]),
    ]
}

struct App { ui: slint::Weak<AppWindow>, st: RefCell<State>, ver: std::cell::Cell<u64>, last: std::cell::Cell<(u64, usize, usize, bool)> }

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
        let c = { let st = self.st.borrow(); let d = &st.docs[st.cur]; let c = d.cursor.min(d.text.len()); if d.text.is_char_boundary(c) { c } else { 0 } };
        ui.invoke_select_range(c as i32, c as i32);
        self.refresh_status(c);
        ui.invoke_focus_editor();
        // re-apply after the TextInput has processed the new text (setting text resets the cursor)
        let w = self.ui.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(30), move || {
            if let Some(ui) = w.upgrade() { if ui.get_cursor_byte() as usize != c { ui.invoke_select_range(c as i32, c as i32); } }
        });
    }

    fn remember_cursor(&self) {
        let c = self.ui().get_cursor_byte().max(0) as usize;
        let mut st = self.st.borrow_mut();
        let cur = st.cur;
        if let Some(d) = st.docs.get_mut(cur) { d.cursor = c; }
    }

    fn persist(&self) {
        self.remember_cursor();
        let mut st = self.st.borrow_mut();
        st.cfg.font = st.font; st.cfg.font_family = st.font_family.clone(); st.cfg.wrap = st.wrap; st.cfg.line_numbers = st.line_numbers; st.cfg.dark = st.dark; st.cfg.preview = st.preview;
        st.cfg.macro_keys = st.macro_buf.clone();
        st.cfg.session = st.docs.iter().filter_map(|d| d.path.clone().map(|p| settings::SessionFile { path: p, cursor: d.cursor })).collect();
        st.cfg.session_cur = st.docs[..st.cur].iter().filter(|d| d.path.is_some()).count();
        if let Err(e) = st.cfg.save() {
            eprintln!("NotEdit: failed to save settings: {e}");
        }
    }

    fn recovery_session(&self) -> recovery::RecoverySession {
        let st = self.st.borrow();
        let docs = st.docs.iter().filter(|d| d.modified).map(|d| recovery::RecoveryDoc {
            path: d.path.clone(),
            title: d.title.clone(),
            text: d.text.clone(),
            eol: match d.eol { Eol::Crlf => "crlf", Eol::Lf => "lf", Eol::Cr => "cr" }.into(),
            encoding: d.encoding.name().to_string(),
            bom: d.bom,
            lang: d.lang.clone(),
            cursor: d.cursor,
        }).collect();
        recovery::RecoverySession::new(docs)
    }

    fn persist_recovery(&self) {
        let session = self.recovery_session();
        if let Err(e) = recovery::save(&settings::config_dir(), &session) {
            eprintln!("NotEdit: failed to save crash-recovery journal: {e}");
        }
    }

    fn restore_recovery(&self) {
        let dir = settings::config_dir();
        let session = match recovery::load(&dir) {
            Ok(Some(session)) if !session.docs.is_empty() => session,
            Ok(_) => return,
            Err(e) => {
                eprintln!("NotEdit: failed to read crash-recovery journal: {e}");
                return;
            }
        };

        let answer = rfd::MessageDialog::new()
            .set_title("NotEdit")
            .set_description(format!("偵測到上次未正常關閉留下的 {} 個未儲存文件。\n\n是否復原？復原內容只會先放回編輯器，不會自動覆寫原檔。", session.docs.len()))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if answer != rfd::MessageDialogResult::Yes {
            if let Err(e) = recovery::clear(&dir) {
                eprintln!("NotEdit: failed to discard crash-recovery journal: {e}");
            }
            return;
        }

        let mut st = self.st.borrow_mut();
        for rd in session.docs {
            let normalized_path = rd.path.map(|p| std::fs::canonicalize(&p).unwrap_or(p));
            let mut d = Doc::new(0);
            d.path = normalized_path.clone();
            d.disk_stamp = normalized_path.as_deref().and_then(|p| file_watch::stamp(p).ok());
            d.title = rd.title;
            d.text = rd.text;
            d.eol = match rd.eol.as_str() { "crlf" => Eol::Crlf, "cr" => Eol::Cr, _ => Eol::Lf };
            d.encoding = match rd.encoding.as_str() {
                "UTF-16LE" => encoding_rs::UTF_16LE,
                "UTF-16BE" => encoding_rs::UTF_16BE,
                _ => encoding_rs::Encoding::for_label(rd.encoding.as_bytes()).unwrap_or(encoding_rs::UTF_8),
            };
            d.bom = rd.bom;
            d.lang = rd.lang;
            d.modified = true;
            let mut cursor = rd.cursor.min(d.text.len());
            while cursor > 0 && !d.text.is_char_boundary(cursor) { cursor -= 1; }
            d.cursor = cursor;

            if let Some(i) = normalized_path.as_ref().and_then(|p| st.docs.iter().position(|x| x.path.as_ref() == Some(p))) {
                st.docs[i] = d;
                st.cur = i;
            } else {
                let replace_blank = st.docs.len() == 1
                    && st.docs[0].path.is_none()
                    && !st.docs[0].modified
                    && st.docs[0].text.is_empty();
                if replace_blank {
                    st.docs[0] = d;
                    st.cur = 0;
                } else {
                    st.docs.push(d);
                    st.cur = st.docs.len() - 1;
                }
            }
        }
    }

    fn refresh_meta(&self) {
        self.ver.set(self.ver.get() + 1);
        let ui = self.ui();
        let st = self.st.borrow();
        let d = &st.docs[st.cur];
        let tabs: Vec<TabInfo> = st.docs.iter().map(|d| TabInfo { title: d.title.clone().into(), modified: d.modified }).collect();
        // only replace the model when it really changed (keeps tab TouchAreas alive during drags)
        let model = ui.get_tabs();
        let same = model.row_count() == tabs.len() && tabs.iter().enumerate().all(|(i, t)| model.row_data(i).as_ref() == Some(t));
        if !same { ui.set_tabs(ModelRc::new(VecModel::from(tabs))); }
        ui.set_current_tab(st.cur as i32);
        ui.set_menus(ModelRc::new(VecModel::from(build_menus(&st))));
        let name = d.path.as_ref().map(|p| p.display().to_string()).unwrap_or(d.title.clone());
        ui.set_window_title(format!("{}{} - NotEdit", if d.modified { "*" } else { "" }, name).into());
        ui.set_word_wrap(st.wrap);
        ui.set_show_line_numbers(st.line_numbers && !st.wrap);
        ui.set_editor_font_size(st.font);
        ui.set_show_preview(st.preview && matches!(d.lang.as_str(), "markdown" | "html" | "xml"));
        ui.set_overwrite(st.overwrite);
        ui.set_dark(st.dark);
        ui.set_mono(st.font_family.as_str().into());
        ui.set_macro_recording(st.macro_rec);
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
        let marks: Vec<usize> = highlight::match_bracket(t, byte).map(|(a, b)| vec![a, b]).unwrap_or_default();
        drop(st);
        let mut st = self.st.borrow_mut();
        if st.marks != marks { st.marks = marks; self.ver.set(self.ver.get() + 1); }
    }

    fn check_external_changes(&self) {
        let docs: Vec<_> = {
            let st = self.st.borrow();
            st.docs.iter().enumerate().filter_map(|(i, d)| {
                d.path.clone().map(|p| (i, p, d.disk_stamp.clone(), d.modified, d.title.clone()))
            }).collect()
        };

        for (idx, path, expected, modified, title) in docs {
            let current = match file_watch::stamp(&path) {
                Ok(stamp) => Some(stamp),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => continue,
            };
            if current == expected { continue; }

            if current.is_none() {
                msg(&format!("「{}」已被其他程式刪除。\n\nNotEdit 會保留目前記憶體中的內容並標記為未儲存。", title));
                {
                    let mut st = self.st.borrow_mut();
                    if let Some(d) = st.docs.get_mut(idx).filter(|d| d.path.as_ref() == Some(&path)) {
                        d.disk_stamp = None;
                        d.modified = true;
                    }
                }
                self.refresh_meta();
                self.persist_recovery();
                return;
            }

            let description = if modified {
                format!("「{}」已被其他程式修改。\n\nNotEdit 內也有未儲存變更。是否重新載入磁碟版本？\n選「否」會保留目前內容並標記為未儲存。", title)
            } else {
                format!("「{}」已被其他程式修改。\n\n是否重新載入磁碟上的新版本？", title)
            };
            let answer = rfd::MessageDialog::new()
                .set_title("NotEdit")
                .set_description(description)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if answer == rfd::MessageDialogResult::Yes {
                self.reload_doc(idx);
            } else {
                {
                    let mut st = self.st.borrow_mut();
                    if let Some(d) = st.docs.get_mut(idx).filter(|d| d.path.as_ref() == Some(&path)) {
                        d.disk_stamp = current;
                        d.modified = true;
                    }
                }
                self.refresh_meta();
                self.persist_recovery();
            }
            return;
        }
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
        self.remember_cursor();
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        {
            let mut st = self.st.borrow_mut();
            st.cfg.add_recent(path.clone());
            if let Some(i) = st.docs.iter().position(|d| d.path.as_ref() == Some(&path)) { st.cur = i; }
            else {
                let mut d = Doc::new(0);
                d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                d.lang = lang_from_path(&path);
                d.disk_stamp = file_watch::stamp(&path).ok();
                d.path = Some(path); d.text = text; d.eol = eol; d.encoding = enc; d.bom = bom;
                let c = st.cur;
                if st.docs[c].path.is_none() && !st.docs[c].modified && st.docs[c].text.is_empty() { st.docs[c] = d; }
                else { st.docs.push(d); st.cur = st.docs.len() - 1; }
                if matches!(st.docs[st.cur].lang.as_str(), "markdown" | "html") { st.preview = true; }
            }
        }
        self.refresh_all();
    }

    fn reload_doc(&self, idx: usize) -> bool {
        let path = {
            let st = self.st.borrow();
            match st.docs.get(idx).and_then(|d| d.path.clone()) {
                Some(path) => path,
                None => return false,
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                msg(&format!("無法重新載入「{}」：{}", path.display(), e));
                return false;
            }
        };
        let (s, enc, bom) = decode(&bytes);
        let eol = detect_eol(&s);
        let text = s.replace("\r\n", "\n").replace('\r', "\n");
        let disk_stamp = file_watch::stamp(&path).ok();
        let is_current = {
            let mut st = self.st.borrow_mut();
            if idx >= st.docs.len() { return false; }
            let is_current = st.cur == idx;
            let d = &mut st.docs[idx];
            d.text = text;
            d.eol = eol;
            d.encoding = enc;
            d.bom = bom;
            d.modified = false;
            d.undo.clear();
            d.redo.clear();
            d.last_snap = None;
            d.hl = Default::default();
            d.disk_stamp = disk_stamp;
            d.cursor = d.cursor.min(d.text.len());
            while d.cursor > 0 && !d.text.is_char_boundary(d.cursor) { d.cursor -= 1; }
            is_current
        };
        if is_current { self.refresh_all(); } else { self.refresh_meta(); }
        self.persist_recovery();
        true
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
        if let Err(e) = file_io::durable_write(&path, &bytes) { msg(&format!("儲存失敗：{}", e)); return false; }
        {
            let mut st = self.st.borrow_mut();
            let d = &mut st.docs[idx];
            d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if d.path.as_ref() != Some(&path) { d.lang = lang_from_path(&path); }
            d.path = Some(path.clone());
            d.disk_stamp = file_watch::stamp(&path).ok();
            d.modified = false;
            st.cfg.add_recent(path);
        }
        self.refresh_meta();
        self.refresh_preview();
        self.persist_recovery();
        true
    }

    /// returns false when cancelled
    fn close(&self, idx: usize) -> bool {
        let (modified, title) = { let st = self.st.borrow(); (st.docs[idx].modified, st.docs[idx].title.clone()) };
        if modified {
            let r = rfd::MessageDialog::new().set_title("NotEdit").set_description(format!("是否儲存「{}」的變更？", title))
                .set_buttons(rfd::MessageButtons::YesNoCancel).show();
            match r {
                rfd::MessageDialogResult::Yes => { if !self.save(idx, false) { return false; } }
                rfd::MessageDialogResult::No => {}
                _ => return false,
            }
        }
        self.remember_cursor();
        {
            let mut st = self.st.borrow_mut();
            st.docs.remove(idx);
            if st.docs.is_empty() { st.counter += 1; let n = st.counter; st.docs.push(Doc::new(n)); }
            if st.cur >= st.docs.len() || st.cur > idx { st.cur = st.cur.saturating_sub(1).min(st.docs.len() - 1); }
        }
        self.refresh_all();
        self.persist_recovery();
        true
    }

    fn select_tab(&self, i: usize) {
        self.remember_cursor();
        { let mut st = self.st.borrow_mut(); if i >= st.docs.len() { return; } st.cur = i; }
        self.refresh_all();
    }

    fn regex(&self) -> Option<regex::Regex> {
        let ui = self.ui();
        match search::build(&ui.get_find_text(), ui.get_match_case(), ui.get_whole_word(), ui.get_use_regex()) {
            Ok(r) => Some(r),
            Err(e) => { ui.set_find_status(e.into()); None }
        }
    }

    fn find(&self, forward: bool) -> bool {
        let ui = self.ui();
        let Some(re) = self.regex() else { return false };
        let (a, c) = (ui.get_anchor_byte() as usize, ui.get_cursor_byte() as usize);
        let found = { let st = self.st.borrow(); search::find(&re, &st.docs[st.cur].text, a.min(c), a.max(c), forward) };
        match found {
            Some((s, e)) => { ui.invoke_select_range(s as i32, e as i32); ui.set_find_status("".into()); true }
            None => { ui.set_find_status(format!("找不到「{}」", ui.get_find_text()).into()); false }
        }
    }

    fn find_all(&self) {
        let ui = self.ui();
        let Some(re) = self.regex() else { return };
        let mut rows = Vec::new();
        let mut hits = Vec::new();
        {
            let st = self.st.borrow();
            for (di, d) in st.docs.iter().enumerate() {
                for m in re.find_iter(&d.text).take(5000) {
                    if m.start() == m.end() { continue; }
                    let ls = d.text[..m.start()].rfind('\n').map(|x| x + 1).unwrap_or(0);
                    let le = d.text[m.start()..].find('\n').map(|x| x + m.start()).unwrap_or(d.text.len());
                    let line = d.text[..m.start()].matches('\n').count() + 1;
                    let snippet: String = d.text[ls..le].trim().chars().take(200).collect();
                    rows.push(SearchHit { doc: di as i32, title: d.title.clone().into(), line: line as i32, start: m.start() as i32, end: m.end() as i32, text: snippet.into() });
                    hits.push((di, m.start(), m.end()));
                }
            }
            ui.set_hits_title(format!("搜尋「{}」：在 {} 個文件中找到 {} 筆", ui.get_find_text(), st.docs.len(), rows.len()).into());
        }
        self.st.borrow_mut().hits = hits;
        ui.set_hits(ModelRc::new(VecModel::from(rows)));
        ui.set_hits_visible(true);
    }

    fn hit_clicked(&self, i: usize) {
        let Some(&(d, s, e)) = self.st.borrow().hits.get(i) else { return };
        if d != self.st.borrow().cur { self.select_tab(d); }
        let ok = { let st = self.st.borrow(); let t = &st.docs[st.cur].text; e <= t.len() && t.is_char_boundary(s) && t.is_char_boundary(e) };
        if ok { self.ui().invoke_select_range(s as i32, e as i32); }
    }

    fn replace(&self, all: bool) {
        let ui = self.ui();
        let Some(re) = self.regex() else { return };
        let r = ui.get_replace_text().to_string();
        let rep = if ui.get_use_regex() { r.clone() } else { r.replace('$', "$$") };
        let t = { let st = self.st.borrow(); st.docs[st.cur].text.clone() };
        if all {
            let c = re.find_iter(&t).count();
            let nt = re.replace_all(&t, rep.as_str()).into_owned();
            self.set_text(nt);
            ui.set_find_status(format!("已取代 {} 處", c).into());
            return;
        }
        let (an, cu) = (ui.get_anchor_byte() as usize, ui.get_cursor_byte() as usize);
        let (s, e) = (an.min(cu), an.max(cu));
        if let Some(caps) = re.captures_at(&t, s).filter(|c| { let m = c.get(0).unwrap(); m.start() == s && m.end() == e && e > s }) {
            let mut out = String::new();
            caps.expand(&rep, &mut out);
            self.set_text(format!("{}{}{}", &t[..s], out, &t[e..]));
            let p = (s + out.len()) as i32;
            ui.invoke_select_range(p, p);
        }
        self.find(true);
    }

    /// Hook for every key that reaches the editor. Returns true when handled.
    fn editor_key(&self, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        let ui = self.ui();
        if ctrl && !alt {
            match text {
                "z" | "Z" if !shift => { self.action_plain("undo"); return true; }
                "y" | "Y" => { self.action_plain("redo"); return true; }
                "z" | "Z" => { self.action_plain("redo"); return true; }
                "b" | "B" => { self.action_plain("gotobrace"); return true; }
                "p" | "P" if !shift => { self.action_plain("print"); return true; }
                _ => return false,
            }
        }
        {
            let mut st = self.st.borrow_mut();
            if st.macro_rec && !st.macro_play && !alt { st.macro_buf.push(text.to_string()); }
        }
        // overwrite mode: replace the next character instead of inserting
        let ow = self.st.borrow().overwrite;
        if ow && !alt {
            let mut ch = text.chars();
            if let (Some(c), None) = (ch.next(), ch.next()) {
                if !c.is_control() && !('\u{f700}'..='\u{f8ff}').contains(&c) {
                    let (a, cur) = (ui.get_anchor_byte() as usize, ui.get_cursor_byte() as usize);
                    if a == cur {
                        let st = self.st.borrow();
                        let t = &st.docs[st.cur].text;
                        if let Some(next) = t.get(cur..).and_then(|r| r.chars().next()) {
                            if next != '\n' { drop(st); ui.invoke_select_range(cur as i32, (cur + next.len_utf8()) as i32); }
                        }
                    }
                }
            }
        }
        false
    }

    fn action_plain(&self, a: &str) {
        let ui = self.ui();
        let cur = self.st.borrow().cur;
        match a {
            "undo" | "redo" => {
                let ok = {
                    let mut st = self.st.borrow_mut();
                    let d = &mut st.docs[cur];
                    let (from, to) = if a == "undo" { (&mut d.undo, &mut d.redo) } else { (&mut d.redo, &mut d.undo) };
                    if let Some(t) = from.pop() { to.push(std::mem::replace(&mut d.text, t)); d.modified = true; d.last_snap = None; true } else { false }
                };
                if ok {
                    let c = ui.get_cursor_byte() as usize;
                    let t = self.st.borrow().docs[cur].text.clone();
                    ui.set_editor_text(t.clone().into());
                    let c = (0..=c.min(t.len())).rev().find(|x| t.is_char_boundary(*x)).unwrap_or(0) as i32;
                    ui.invoke_select_range(c, c);
                    self.refresh_meta(); self.refresh_preview();
                }
            }
            "gotobrace" => {
                let pos = ui.get_cursor_byte() as usize;
                let m = { let st = self.st.borrow(); highlight::match_bracket(&st.docs[cur].text, pos) };
                if let Some((_, j)) = m { ui.invoke_select_range(j as i32, j as i32); }
            }
            "print" | "exporthtml" => {
                let (html, title, path) = {
                    let st = self.st.borrow(); let d = &st.docs[cur];
                    let ext = d.path.as_ref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase());
                    (highlight::export_html(&d.text, &d.lang, ext.as_deref(), &d.title, false), d.title.clone(), d.path.clone())
                };
                let target = if a == "print" {
                    let p = std::env::temp_dir().join(format!("notedit-print-{}.html", std::process::id()));
                    Some(p)
                } else {
                    let mut dlg = rfd::FileDialog::new().set_title("匯出為 HTML").set_file_name(format!("{}.html", title)).add_filter("HTML", &["html"]);
                    if let Some(dir) = path.as_ref().and_then(|p| p.parent()) { dlg = dlg.set_directory(dir); }
                    dlg.save_file()
                };
                if let Some(p) = target {
                    let html = if a == "print" { html.replace("</body>", "<script>window.onload=()=>window.print()</script></body>") } else { html };
                    match std::fs::write(&p, html) { Ok(_) => { if a == "print" { open_external(&p); } } Err(e) => msg(&format!("寫入失敗：{}", e)) }
                }
            }
            _ => {}
        }
    }

    fn play_macro(&self, times: usize) {
        let keys = self.st.borrow().macro_buf.clone();
        if keys.is_empty() { return; }
        let ui = self.ui();
        ui.invoke_focus_editor();
        self.st.borrow_mut().macro_play = true;
        // reset modifier state (the shortcut that triggered playback may still be held)
        for k in [slint::platform::Key::Control, slint::platform::Key::Shift, slint::platform::Key::Alt, slint::platform::Key::Meta] {
            ui.window().dispatch_event(slint::platform::WindowEvent::KeyReleased { text: k.into() });
        }
        for _ in 0..times {
            for k in &keys {
                let text: SharedString = k.as_str().into();
                ui.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: text.clone() });
                ui.window().dispatch_event(slint::platform::WindowEvent::KeyReleased { text });
            }
        }
        self.st.borrow_mut().macro_play = false;
    }

    fn selection(&self) -> (usize, usize) {
        let ui = self.ui();
        let (a, c) = (ui.get_anchor_byte().max(0) as usize, ui.get_cursor_byte().max(0) as usize);
        (a.min(c), a.max(c))
    }

    fn context_menu(&self, kind: &str, index: i32) {
        let ui = self.ui();
        let items = if kind == "tab" {
            let i = (index.max(0) as usize).min(self.st.borrow().docs.len() - 1);
            self.st.borrow_mut().ctx_tab = i;
            let st = self.st.borrow();
            let n = st.docs.len();
            let has_path = st.docs[i].path.is_some();
            vec![en("關閉", "Ctrl+W", "ctx:close", true), en("關閉其他分頁", "", "ctx:closeothers", n > 1),
                 en("關閉右側所有分頁", "", "ctx:closeright", i + 1 < n), en("關閉左側所有分頁", "", "ctx:closeleft", i > 0), sep(),
                 en("儲存", "Ctrl+S", "ctx:save", true), en("另存新檔...", "", "ctx:saveas", true), en("重新命名...", "", "ctx:rename", true), sep(),
                 en("複製完整路徑", "", "ctx:copypath", has_path), en("複製檔名", "", "ctx:copyname", true), en("開啟所在資料夾", "", "ctx:openfolder", has_path)]
        } else {
            let (s, e) = self.selection();
            let sel = e > s;
            let st = self.st.borrow();
            let d = &st.docs[st.cur];
            let pv = matches!(d.lang.as_str(), "markdown" | "html" | "xml");
            vec![en("復原", "Ctrl+Z", "undo", !d.undo.is_empty()), en("取消復原", "Ctrl+Y", "redo", !d.redo.is_empty()), sep(),
                 en("剪下", "Ctrl+X", "cut", sel), en("複製", "Ctrl+C", "copy", sel), en("貼上", "Ctrl+V", "paste", true), en("刪除", "Del", "delete", sel), en("全選", "Ctrl+A", "selectall", !d.text.is_empty()), sep(),
                 en("轉成大寫", "", "upper", sel), en("轉成小寫", "", "lower", sel), en("複製目前行", "Ctrl+D", "dupline", true), en("刪除目前行", "", "delline", true), sep(),
                 en("註解 / 取消註解", "Ctrl+Q", "comment", comment_syntax(&d.lang).is_some()), en("搜尋選取的文字", "", "searchsel", sel), sep(),
                 chk(if st.preview { "關閉預覽" } else { "開啟預覽" }, "", "preview", false)].into_iter().map(|mut m| { if m.action == "preview" { m.enabled = pv; } m }).collect()
        };
        ui.set_ctx_items(ModelRc::new(VecModel::from(items)));
        ui.set_ctx_visible(true);
    }

    fn ctx_action(self: &Rc<Self>, a: &str) {
        let ui = self.ui();
        let i = self.st.borrow().ctx_tab.min(self.st.borrow().docs.len() - 1);
        let n = self.st.borrow().docs.len();
        match a {
            "close" => { self.close(i); }
            "closeothers" => { for j in (0..n).rev() { if j != i && !self.close(j) { break; } } }
            "closeright" => { for j in (i + 1..n).rev() { if !self.close(j) { break; } } }
            "closeleft" => { for j in (0..i).rev() { if !self.close(j) { break; } } }
            "save" | "saveas" => { self.save(i, a == "saveas"); }
            "rename" => { let t = self.st.borrow().docs[i].title.clone(); ui.set_rename_text(t.into()); ui.set_rename_visible(true); }
            "copypath" | "copyname" => {
                let st = self.st.borrow(); let d = &st.docs[i];
                let t = if a == "copypath" { d.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default() } else { d.title.clone() };
                if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(t); }
            }
            "openfolder" => { if let Some(p) = self.st.borrow().docs[i].path.as_ref().and_then(|p| p.parent()) { open_external(p); } }
            _ => {}
        }
    }

    fn rename(&self, name: String) {
        let name = name.trim().to_string();
        let ui = self.ui();
        if name.is_empty() || name.contains(['/', '\\']) { return; }
        let i = self.st.borrow().ctx_tab.min(self.st.borrow().docs.len() - 1);
        let path = self.st.borrow().docs[i].path.clone();
        if let Some(old) = path {
            let new = old.with_file_name(&name);
            if new.exists() { msg(&format!("檔案已存在：{}", new.display())); return; }
            if let Err(e) = std::fs::rename(&old, &new) { msg(&format!("重新命名失敗：{}", e)); return; }
            let mut st = self.st.borrow_mut();
            st.docs[i].lang = lang_from_path(&new);
            st.docs[i].path = Some(new.clone());
            st.cfg.recent.retain(|x| x != &old);
            st.cfg.add_recent(new);
        }
        self.st.borrow_mut().docs[i].title = name;
        ui.set_rename_visible(false);
        self.refresh_meta();
    }

    fn toggle_comment(&self) {
        let (pre, post) = { let st = self.st.borrow(); match comment_syntax(&st.docs[st.cur].lang) { Some(c) => c, None => return } };
        let t = { let st = self.st.borrow(); st.docs[st.cur].text.clone() };
        let (s, e) = self.selection();
        let ls = t[..s.min(t.len())].rfind('\n').map(|x| x + 1).unwrap_or(0);
        let e2 = if e > s && t[..e].ends_with('\n') { e - 1 } else { e };
        let le = t[e2.min(t.len())..].find('\n').map(|x| x + e2).unwrap_or(t.len());
        let block = &t[ls..le];
        let new = toggle_comment_lines(block, pre, post);
        let nt = format!("{}{}{}", &t[..ls], new, &t[le..]);
        self.set_text(nt);
        self.ui().invoke_select_range(ls as i32, (ls + new.len()) as i32);
    }

    fn move_tab(&self, from: usize, to: usize) {
        {
            let mut st = self.st.borrow_mut();
            if from >= st.docs.len() || to >= st.docs.len() || from == to { return; }
            let d = st.docs.remove(from);
            st.docs.insert(to, d);
            st.cur = to;
        }
        self.refresh_meta();
    }

    fn action(self: &Rc<Self>, a: &str) {
        let ui = self.ui();
        let cur = self.st.borrow().cur;
        match a {
            "new" => { self.remember_cursor(); { let mut st = self.st.borrow_mut(); st.counter += 1; let n = st.counter; st.docs.push(Doc::new(n)); st.cur = st.docs.len() - 1; } self.refresh_all(); }
            "open" => {
                if let Some(files) = rfd::FileDialog::new().set_title("開啟舊檔")
                    .add_filter("所有檔案", &["*"]).add_filter("文字/程式碼", &["txt", "md", "html", "htm", "rs", "py", "js", "json", "xml", "css", "c", "cpp", "h", "toml", "ini", "log"])
                    .pick_files() { for f in files { self.open_path(f); } }
            }
            "reload" => { self.reload_doc(cur); }
            "save" => { self.save(cur, false); }
            "saveas" => { self.save(cur, true); }
            "saveall" => { let n = self.st.borrow().docs.len(); for i in 0..n { if self.st.borrow().docs[i].modified { self.save(i, false); } } }
            "close" => { self.close(cur); }
            "closeall" => { while self.st.borrow().docs.len() > 1 || self.st.borrow().docs[0].modified || self.st.borrow().docs[0].path.is_some() { let i = self.st.borrow().docs.len() - 1; if !self.close(i) { break; } } }
            "closeothers" => { let mut i = self.st.borrow().docs.len(); while i > 0 { i -= 1; if i != self.st.borrow().cur { if !self.close(i) { break; } } } }
            "exit" => { if self.confirm_exit() { let _ = slint::quit_event_loop(); } }
            "undo" | "redo" | "gotobrace" | "print" | "exporthtml" => self.action_plain(a),
            "findall" => { ui.set_find_visible(true); if !ui.get_find_text().is_empty() { self.find_all(); } }
            "dark" => { { let mut st = self.st.borrow_mut(); st.dark = !st.dark; } self.refresh_meta(); self.persist(); }
            "restoresession" => { { let mut st = self.st.borrow_mut(); st.cfg.restore_session = !st.cfg.restore_session; } self.refresh_meta(); self.persist(); }
            x if x.starts_with("ctx:") => self.ctx_action(&x[4..]),
            "delete" => { let (s, e) = self.selection(); if e > s { let t = self.st.borrow().docs[cur].text.clone(); self.set_text(format!("{}{}", &t[..s], &t[e..])); ui.invoke_select_range(s as i32, s as i32); } }
            "comment" => self.toggle_comment(),
            "searchsel" => {
                let (s, e) = self.selection();
                let t = self.st.borrow().docs[cur].text.get(s..e).unwrap_or("").to_string();
                if !t.is_empty() { ui.set_find_text(t.into()); ui.set_use_regex(false); ui.set_find_visible(true); self.find(true); }
            }
            x if x.starts_with("font:") => {
                { let mut st = self.st.borrow_mut(); st.font_family = x[5..].to_string(); }
                self.ver.set(self.ver.get() + 1);
                self.refresh_meta(); self.persist();
            }
            "openconfig" => { let d = settings::config_dir(); let _ = std::fs::create_dir_all(&d); open_external(&d); }
            "macrorec" => {
                { let mut st = self.st.borrow_mut(); st.macro_rec = !st.macro_rec; if st.macro_rec { st.macro_buf.clear(); } }
                self.refresh_meta();
            }
            "macroplay" | "macroplay10" => {
                if !self.st.borrow().macro_rec {
                    let n = if a == "macroplay" { 1 } else { 10 };
                    let me = self.clone();
                    slint::Timer::single_shot(std::time::Duration::from_millis(0), move || me.play_macro(n));
                }
            }
            "macroclear" => { self.st.borrow_mut().macro_buf.clear(); self.refresh_meta(); }
            x if x.starts_with("recent:") => {
                let p = x[7..].parse::<usize>().ok().and_then(|i| self.st.borrow().cfg.recent.get(i).cloned());
                if let Some(p) = p { if p.exists() { self.open_path(p); } else { msg(&format!("找不到檔案：{}", p.display())); self.st.borrow_mut().cfg.recent.retain(|x| x != &p); self.refresh_meta(); } }
            }
            "clearrecent" => { self.st.borrow_mut().cfg.recent.clear(); self.refresh_meta(); }
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

    /// called periodically: keep syntax highlight overlay in sync with visible range
    fn tick_highlight(&self) {
        let ui = self.ui();
        let lh = ui.get_line_h().max(1.0);
        let first = ((ui.get_scroll_y() - 2.0).max(0.0) / lh) as usize;
        let count = (ui.get_view_h() / lh) as usize + 2;
        let ver = self.ver.get();
        let (lv, lf, lc, done) = self.last.get();
        if lv == ver && lf == first && lc == count && done { return; }
        let t0 = Instant::now();
        let mut st = self.st.borrow_mut();
        let ext = st.docs[st.cur].path.as_ref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase());
        let dark = st.dark;
        let marks = st.marks.clone();
        let cur = st.cur;
        let Doc { hl, text, lang, .. } = &mut st.docs[cur];
        if lv != ver { hl.sync(text, lang, ext.as_deref(), dark); }
        let mut finished = hl.work(first + count, 300);
        while !finished && t0.elapsed().as_millis() < 25 { finished = hl.work(first + count, 300); }
        ui.set_hl_active(hl.active());
        let fg = if dark { slint::Color::from_rgb_u8(0xdc, 0xdc, 0xdc) } else { slint::Color::from_rgb_u8(0x1e, 0x1e, 0x1e) };
        ui.set_hl_lines(ModelRc::new(VecModel::from(hl.visible(first, count, fg, &marks))));
        self.last.set((ver, first, count, finished));
    }

    fn confirm_exit(&self) -> bool {
        self.persist();
        loop {
            let idx = self.st.borrow().docs.iter().position(|d| d.modified);
            match idx {
                Some(i) => { self.st.borrow_mut().cur = i; self.refresh_all(); if !self.close(i) { return false; } }
                None => {
                    if let Err(e) = recovery::clear(&settings::config_dir()) {
                        eprintln!("NotEdit: failed to clear crash-recovery journal: {e}");
                    }
                    return true;
                }
            }
        }
    }
}

/// line comment prefix, or (open, close) for block-only languages
fn comment_syntax(lang: &str) -> Option<(&'static str, &'static str)> {
    Some(match lang {
        "rust" | "c" | "cpp" | "cs" | "java" | "javascript" | "json" | "slint" => ("// ", ""),
        "python" | "shell" | "yaml" => ("# ", ""),
        "ini" => ("; ", ""),
        "sql" => ("-- ", ""),
        "batch" => ("REM ", ""),
        "html" | "xml" | "markdown" => ("<!-- ", " -->"),
        "css" => ("/* ", " */"),
        _ => return None,
    })
}

/// Toggle comments on every non-empty line of `block` (remove if all are commented).
fn toggle_comment_lines(block: &str, pre: &str, post: &str) -> String {
    let lines: Vec<&str> = block.split('\n').collect();
    let (p, q) = (pre.trim_end(), post.trim_start());
    let is_c = |l: &str| { let t = l.trim_start(); t.starts_with(p) && (q.is_empty() || t.trim_end().ends_with(q)) };
    let nonempty: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let all = !nonempty.is_empty() && nonempty.iter().all(|l| is_c(l));
    let indent = nonempty.iter().map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    lines.iter().map(|l| {
        if l.trim().is_empty() { return l.to_string(); }
        if all {
            let ws = l.len() - l.trim_start().len();
            let mut body = &l[ws..];
            body = body.strip_prefix(pre).or_else(|| body.strip_prefix(p)).unwrap_or(body);
            if !q.is_empty() { let b = body.trim_end(); body = b.strip_suffix(post).or_else(|| b.strip_suffix(q)).unwrap_or(b); }
            format!("{}{}", &l[..ws], body)
        } else {
            format!("{}{}{}{}", &l[..indent], pre, &l[indent..], post)
        }
    }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments() {
        let b = "fn a() {\n    x();\n}";
        let c = toggle_comment_lines(b, "// ", "");
        assert_eq!(c, "// fn a() {\n//     x();\n// }");
        assert_eq!(toggle_comment_lines(&c, "// ", ""), b);
        let h = toggle_comment_lines("  <p>x</p>", "<!-- ", " -->");
        assert_eq!(h, "  <!-- <p>x</p> -->");
        assert_eq!(toggle_comment_lines(&h, "<!-- ", " -->"), "  <p>x</p>");
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
    rfd::MessageDialog::new().set_title("NotEdit").set_description(s).set_buttons(rfd::MessageButtons::Ok).show();
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let cfg = settings::Settings::load();
    let st = State { docs: vec![Doc::new(1)], cur: 0, counter: 1, font: cfg.font.clamp(6.0, 48.0), wrap: cfg.wrap, line_numbers: cfg.line_numbers,
        preview: cfg.preview, overwrite: false, macro_rec: false, macro_play: false, macro_buf: cfg.macro_keys.clone(), dark: cfg.dark,
        marks: vec![], hits: vec![], font_family: if cfg.font_family.trim().is_empty() { "JetBrains Mono".into() } else { cfg.font_family.clone() },
        font_choices: detect_fonts(), ctx_tab: 0, cfg };
    let app = Rc::new(App { ui: ui.as_weak(), st: RefCell::new(st), ver: Default::default(), last: std::cell::Cell::new((u64::MAX, 0, 0, false)) });

    macro_rules! cb { ($m:ident, |$a:ident $(, $p:ident : $t:ty)*| $body:block) => {{ let $a = app.clone(); ui.$m(move |$($p: $t),*| $body); }}; }
    cb!(on_menu_action, |a, s: SharedString| { a.action(&s) });
    cb!(on_text_edited, |a, s: SharedString| { a.on_edit(s.to_string()) });
    cb!(on_cursor_moved, |a, b: i32| { a.refresh_status(b.max(0) as usize) });
    cb!(on_tab_selected, |a, i: i32| { a.select_tab(i as usize) });
    cb!(on_tab_close, |a, i: i32| { a.close(i as usize); });
    cb!(on_find_next, |a, f: bool| { a.find(f); });
    cb!(on_count_matches, |a| {
        if let Some(re) = a.regex() { let st = a.st.borrow(); let c = re.find_iter(&st.docs[st.cur].text).count(); a.ui().set_find_status(format!("計數：{} 個符合項目", c).into()); }
    });
    cb!(on_replace_one, |a| { a.replace(false) });
    cb!(on_replace_all, |a| { a.replace(true) });
    cb!(on_find_all, |a| { a.find_all() });
    cb!(on_hit_clicked, |a, i: i32| { a.hit_clicked(i.max(0) as usize) });
    cb!(on_context_request, |a, k: SharedString, i: i32| { a.context_menu(&k, i) });
    cb!(on_rename_accepted, |a, t: SharedString| { a.rename(t.to_string()) });
    cb!(on_tab_move, |a, f: i32, t: i32| { a.move_tab(f.max(0) as usize, t.max(0) as usize) });
    {
        let a = app.clone();
        ui.on_editor_key(move |t: SharedString, c: bool, s: bool, al: bool| a.editor_key(&t, c, s, al));
    }
    cb!(on_goto_line, |a, s: SharedString| {
        let ui = a.ui();
        if let Ok(n) = s.trim().parse::<usize>() {
            let t = a.st.borrow().docs[a.st.borrow().cur].text.clone();
            let off = if n <= 1 { 0 } else { t.match_indices('\n').nth(n - 2).map(|(i, _)| i + 1).unwrap_or(t.len()) };
            ui.set_goto_visible(false);
            ui.invoke_select_range(off as i32, off as i32);
        }
    });

    // open files passed on command line, otherwise restore last session
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() && app.st.borrow().cfg.restore_session {
        let (files, sel) = { let st = app.st.borrow(); (st.cfg.session.clone(), st.cfg.session_cur) };
        for f in files.iter().filter(|f| f.path.exists()) { app.open_path(f.path.clone()); }
        for f in &files {
            let p = std::fs::canonicalize(&f.path).unwrap_or(f.path.clone());
            let mut st = app.st.borrow_mut();
            if let Some(d) = st.docs.iter_mut().find(|d| d.path.as_ref() == Some(&p)) { d.cursor = f.cursor; }
        }
        let n = app.st.borrow().docs.len();
        app.st.borrow_mut().cur = sel.min(n - 1);
    }
    for arg in args { app.open_path(PathBuf::from(arg)); }
    app.restore_recovery();
    app.refresh_all();

    // drag & drop files from the OS
    {
        let a = app.clone();
        let pending: Rc<RefCell<Vec<PathBuf>>> = Default::default();
        let cursor = std::cell::Cell::new((0.0f64, 0.0f64));
        ui.window().on_winit_window_event(move |w, ev| {
            if let winit::event::WindowEvent::CursorMoved { position, .. } = ev { cursor.set((position.x, position.y)); }
            if let winit::event::WindowEvent::MouseInput { state: winit::event::ElementState::Pressed, button: winit::event::MouseButton::Right, .. } = ev {
                let sf = w.scale_factor() as f64;
                let (x, y) = cursor.get();
                let (a, x, y) = (a.clone(), (x / sf) as f32, (y / sf) as f32);
                slint::Timer::single_shot(std::time::Duration::from_millis(0), move || a.ui().invoke_editor_right_click(x, y));
            }
            if let winit::event::WindowEvent::DroppedFile(p) = ev {
                pending.borrow_mut().push(p.clone());
                let (a, pending) = (a.clone(), pending.clone());
                // defer: the event filter must not re-enter the UI synchronously
                slint::Timer::single_shot(std::time::Duration::from_millis(0), move || {
                    let files: Vec<PathBuf> = pending.borrow_mut().drain(..).collect();
                    for f in files { if f.is_file() { a.open_path(f); } }
                });
            }
            WinitWindowEventResult::Propagate
        });
    }

    let a3 = app.clone();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(30), move || a3.tick_highlight());

    // Periodically journal only modified buffers. Recovery never writes source
    // files; it is removed after a clean exit or once every buffer is saved.
    let ar = app.clone();
    let recovery_timer = slint::Timer::default();
    recovery_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(10), move || ar.persist_recovery());

    let aw = app.clone();
    let file_watch_timer = slint::Timer::default();
    file_watch_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(2), move || aw.check_external_changes());

    let a2 = app.clone();
    ui.window().on_close_requested(move || if a2.confirm_exit() { slint::CloseRequestResponse::HideWindow } else { slint::CloseRequestResponse::KeepWindowShown });
    ui.run()
}
