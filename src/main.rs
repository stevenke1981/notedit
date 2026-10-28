#![cfg_attr(windows, windows_subsystem = "windows")]
slint::include_modules!();
mod preview;
mod dlg;
mod workspace;
mod panels;
mod shell;
mod highlight;
mod search;
mod fmt;
mod settings;
mod editor;
mod textops;
mod hex;
mod fif;
mod spell;
mod hexview;
mod madops;
mod file_io;
mod recovery;
mod file_watch;
mod bookmarks;
mod render_model;
mod agent;

use i_slint_backend_winit::{EventResult, WinitWindowAccessor};

use slint::{Model, ModelRc, SharedString, VecModel};
use editor::{EditKind, Layout, Sel};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq)]
enum Eol { Crlf, Lf, Cr }
impl Eol {
    fn label(self) -> &'static str { match self { Eol::Crlf => "Windows (CR LF)", Eol::Lf => "Unix (LF)", Eol::Cr => "Macintosh (CR)" } }
    fn seq(self) -> &'static str { match self { Eol::Crlf => "\r\n", Eol::Lf => "\n", Eol::Cr => "\r" } }
}

struct Doc {
    path: Option<PathBuf>,
    title: String,
    buf: editor::Buffer, // text normalized to '\n'
    eol: Eol,
    encoding: &'static encoding_rs::Encoding,
    bom: bool,
    modified: bool,
    /// text at the last load/save; undo/redo back to it clears `modified`
    saved: Option<ropey::Rope>,
    lang: String,
    hl: highlight::Hl,
    cursor: usize, // byte offset, used for session/recovery
    disk_stamp: Option<file_watch::FileStamp>,
    folds: BTreeSet<usize>,
    folds_ver: u64,
    bookmarks: BTreeSet<usize>,
    /// main-view state remembered while the document is not shown
    sels: Vec<Sel>,
    top: usize,
    words: (u64, BTreeSet<String>),
    fold_cache: RefCell<(u64, HashMap<usize, Option<usize>>)>,
    hex: Option<Box<hexview::HexState>>,
    /// compare result: (buffer version it was computed for, line → 1 added / 2 removed / 3 changed, hunk start lines)
    diff: Option<(u64, HashMap<usize, u8>, Vec<usize>)>,
}

impl Doc {
    fn new(n: usize) -> Self {
        Doc { path: None, title: format!("new {}", n), buf: Default::default(), eol: if cfg!(windows) { Eol::Crlf } else { Eol::Lf },
              encoding: encoding_rs::UTF_8, bom: false, modified: false, saved: Some(Default::default()), lang: "text".into(), hl: Default::default(), cursor: 0, disk_stamp: None,
              folds: BTreeSet::new(), folds_ver: 0, bookmarks: BTreeSet::new(), sels: vec![Sel::caret(0)], top: 0, words: (0, BTreeSet::new()),
              fold_cache: Default::default(), hex: None, diff: None }
    }
    fn text(&self) -> String { self.buf.text() }
    fn set_loaded(&mut self, text: &str) {
        self.buf.reset(text);
        self.saved = Some(self.buf.rope.clone());
        self.folds.clear(); self.folds_ver += 1;
        self.bookmarks.clear();
        let c = self.cursor.min(text.len());
        let c = (0..=c).rev().find(|b| text.is_char_boundary(*b)).unwrap_or(0);
        let ch = self.buf.rope.byte_to_char(c);
        self.sels = vec![Sel::caret(ch)];
    }
    fn fold_end(&self, line: usize) -> Option<usize> {
        let mut c = self.fold_cache.borrow_mut();
        if c.0 != self.buf.version { c.0 = self.buf.version; c.1.clear(); }
        *c.1.entry(line).or_insert_with(|| editor::fold_end(&self.buf.rope, line))
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
    ("ini", "MS ini file", &["ini", "cfg"]),
    ("toml", "TOML file", &["toml"]),
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
    hits: Vec<(usize, usize, usize)>,
    cfg: settings::Settings,
    font_family: String,
    font_choices: Vec<String>,
    ctx_tab: usize,
    views: [View; 2],
    split: bool,
    active: usize,
    docmap: bool,
    autocomplete: bool,
    column_mode: bool,
    prompt_kind: String,
    file_hits: Vec<(PathBuf, usize, usize, usize)>,
    fif_rx: Option<std::sync::mpsc::Receiver<madops::FifMsg>>,
    fif_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    fif_root: PathBuf,
    /// (file, .bak) pairs written by the last Replace in Files
    rif_backups: Vec<(PathBuf, PathBuf)>,
    rif: Option<madops::RifJob>,
    side_mode: u8,
    side_root: Option<PathBuf>,
    side_expanded: std::collections::HashSet<PathBuf>,
    side_rows: Vec<workspace::TreeRow>,
    side_syms: Vec<workspace::Symbol>,
    side_sym_key: (usize, u64),
}

#[derive(Clone, Copy, PartialEq)]
enum Drag { None, Normal, Column { row: usize, cells: f32 }, Words(usize, usize), Lines(usize) }
impl Default for Drag { fn default() -> Self { Drag::None } }

struct Ac { items: Vec<String>, sel: usize, start: usize }

/// One editor surface (main view = 0, split view = 1).
#[derive(Default)]
struct View {
    doc: usize,
    sels: Vec<Sel>,
    primary: usize,
    top: usize,
    sx: f32,
    max_w: f32,
    follow: bool,
    layout: Layout,
    lkey: (usize, u64, usize, u64, bool),
    drag: Drag,
    ac: Option<Ac>,
    click: (Option<Instant>, usize, u32),
    map_key: (usize, u64, i32, bool),
    map: Vec<MapLine>,
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

fn encode(doc: &Doc) -> Result<Vec<u8>, String> {
    let text = doc.text().replace('\n', doc.eol.seq());
    let name = doc.encoding.name();
    if name == "UTF-16LE" || name == "UTF-16BE" {
        let le = name == "UTF-16LE";
        let mut v = if le { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
        for u in text.encode_utf16() { v.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() }); }
        return Ok(v);
    }
    let mut v = Vec::new();
    if doc.bom && name == "UTF-8" { v.extend_from_slice(&[0xEF, 0xBB, 0xBF]); }
    let (b, _, had_errors) = doc.encoding.encode(&text);
    if had_errors {
        return Err(format!(
            "目前的 {} 編碼無法表示文件中的部分字元。\n\n為避免資料遺失，本次沒有寫入檔案。請先將編碼改為 UTF-8 或 UTF-8-BOM，再重新儲存。",
            doc.encoding_label()
        ));
    }
    v.extend_from_slice(&b);
    Ok(v)
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
    let d = &st.docs[st.views[st.view_idx()].doc];
    let menu = |t: &str, items: Vec<MenuItem>| Menu { title: t.into(), items: ModelRc::new(VecModel::from(items)) };
    let mut lang_items: Vec<MenuItem> = LANGS.iter().map(|l| chk(l.0, "", &format!("lang:{}", l.0), d.lang == l.0)).collect();
    lang_items.insert(1, sep());
    let mut win_items = vec![mi("下一個分頁", "Ctrl+Tab", "nexttab"), mi("上一個分頁", "", "prevtab"), sep()];
    for (i, doc) in st.docs.iter().enumerate() {
        win_items.push(chk(&format!("{}: {}", i + 1, doc.title), "", &format!("tab:{}", i), i == st.cur));
    }
    let mut menus = vec![
        menu("檔案", vec![mi("開新檔案", "Ctrl+N", "new"), mi("開啟舊檔...", "Ctrl+O", "open"), mi("開啟資料夾作為工作區...", "", "openworkspace"), mi("重新載入", "", "reload"), sep(),
            mi("儲存檔案", "Ctrl+S", "save"), mi("另存新檔...", "Ctrl+Shift+S", "saveas"), mi("全部儲存", "", "saveall"), sep(),
            mi("關閉", "Ctrl+W", "close"), mi("全部關閉", "", "closeall"), mi("關閉其他分頁", "", "closeothers"), sep(),
            mi("匯出為 HTML...", "", "exporthtml"), mi("列印 (以瀏覽器)...", "Ctrl+P", "print"), sep()]
            .into_iter().chain(recent_items(st)).chain([sep(), mi("結束", "Alt+F4", "exit")]).collect()),
        menu("編輯", vec![mi("復原", "Ctrl+Z", "undo"), mi("取消復原", "Ctrl+Y", "redo"), sep(),
            mi("剪下", "Ctrl+X", "cut"), mi("複製", "Ctrl+C", "copy"), mi("貼上", "Ctrl+V", "paste"), mi("全選", "Ctrl+A", "selectall"), sep(),
            mi("複製目前行", "Ctrl+D", "dupline"), mi("刪除目前行", "", "delline"), mi("轉成大寫", "", "upper"), mi("轉成小寫", "", "lower"),
            mi("移除行尾空白", "", "trimtrail"), mi("排序行 (遞增)", "", "sortlines"), sep(),
            mi("換行格式 → Windows (CR LF)", "", "eol:crlf"), mi("換行格式 → Unix (LF)", "", "eol:lf"), mi("換行格式 → Macintosh (CR)", "", "eol:cr")]),
        menu("轉換", vec![mi("繁體 → 簡體", "", "xf:t2s"), mi("簡體 → 繁體 (台灣用語)", "", "xf:s2t"), sep(),
            mi("轉成全形", "", "xf:full"), mi("轉成半形", "", "xf:half"), sep(),
            mi("Tab → 空白", "", "xf:tab2sp"), mi("空白 → Tab (行首)", "", "xf:sp2tab"), sep(),
            mi("字首大寫", "", "xf:cap"), mi("大小寫互換", "", "xf:invert"), sep(),
            mi("移除行首空白", "", "xf:trimlead"), mi("移除行首及行尾空白", "", "xf:trimboth"), mi("合併行", "", "xf:join"),
            mi("刪除空行", "", "xf:delempty"), mi("刪除空行 (含只有空白的行)", "", "xf:delblank"),
            mi("排序行 (遞減)", "", "xf:sortdesc"), mi("自然排序行 (數字順序)", "", "xf:sortnatural"),
            mi("反轉行順序", "", "xf:reverse"), mi("移除重複行", "", "xf:dedup"), sep(),
            mi("插入遞增數字...", "", "xf:number"), mi("欄位對齊...", "", "xf:align"), sep(),
            mi("複製為十六進位字串", "", "xf:tohex"), mi("還原十六進位字串", "", "xf:fromhex"), sep(),
            mi("JSON 格式化", "", "xf:jsonfmt"), mi("JSON 壓縮", "", "xf:jsonmin"), mi("XML 格式化", "", "xf:xmlfmt"), mi("XML 壓縮", "", "xf:xmlmin")]),
        menu("搜尋", vec![mi("尋找...", "Ctrl+F", "find"), mi("找下一個", "F3", "findnext"), mi("找上一個", "Shift+F3", "findprev"),
            mi("取代...", "Ctrl+H", "replace"), mi("在所有開啟文件中尋找", "", "findall"), mi("在檔案中尋找...", "Ctrl+Shift+F", "fif"), mi("在檔案中取代...", "", "fifreplace"), mi("復原檔案取代", "", "fifundo"), sep(), mi("跳至...", "Ctrl+G", "goto"), mi("跳至對應括號", "Ctrl+B", "gotobrace"), sep(),
            mi("切換書籤", "Ctrl+F2", "bookmark"), mi("下一個書籤", "F2", "nextbm"), mi("上一個書籤", "Shift+F2", "prevbm"), mi("清除所有書籤", "", "clearbm"),
            sep(), mi("反轉書籤", "", "bmlines:invert"), mi("複製書籤行", "", "bmlines:copy"),
            mi("剪下書籤行", "", "bmlines:cut"), mi("刪除書籤行", "", "bmlines:delete")]),
        menu("檢視", vec![chk("文字模式", "", "textmode", d.hex.is_none() && !st.column_mode), chk("欄模式", "Alt+C", "columnmode", d.hex.is_none() && st.column_mode),
            chk("十六進位模式", "Alt+H", "hexmode", d.hex.is_some()), chk("拼字檢查 (英文，純文字/Markdown)", "", "spell", st.cfg.spell), sep(),chk("自動換行", "", "wrap", st.wrap), chk("顯示行號", "", "linenumbers", st.line_numbers),
            chk("HTML / Markdown 預覽", "", "preview", st.preview), chk("文件地圖", "", "docmap", st.docmap), chk("資料夾工作區", "", "side:1", st.side_mode == 1), chk("函式清單", "", "side:2", st.side_mode == 2), sep(),
            chk("分割檢視 (點分頁可在右側開啟其他文件)", "", "split", st.split), en("切換到另一個檢視", "F8", "otherview", st.split), sep(),
            mi("摺疊 / 展開目前區塊", "Ctrl+Alt+F", "foldcur"), mi("全部摺疊", "Alt+0", "foldall"), mi("全部展開", "Alt+Shift+0", "unfoldall"), sep(),
            chk("深色主題", "", "dark", st.dark), sep(),
            mi("放大", "Ctrl++", "zoomin"), mi("縮小", "Ctrl+-", "zoomout"), mi("恢復預設縮放", "Ctrl+0", "zoomreset")]),
        menu("編碼", vec![chk("UTF-8", "", "enc:utf8", d.encoding == encoding_rs::UTF_8 && !d.bom), chk("UTF-8-BOM", "", "enc:utf8bom", d.encoding == encoding_rs::UTF_8 && d.bom),
            chk("UTF-16 LE BOM", "", "enc:utf16le", d.encoding == encoding_rs::UTF_16LE), chk("UTF-16 BE BOM", "", "enc:utf16be", d.encoding == encoding_rs::UTF_16BE), sep(),
            chk("Big5 (繁體中文)", "", "enc:big5", d.encoding == encoding_rs::BIG5), chk("GBK (簡體中文)", "", "enc:gbk", d.encoding == encoding_rs::GBK),
            chk("Shift-JIS (日文)", "", "enc:sjis", d.encoding == encoding_rs::SHIFT_JIS), chk("Windows-1252 (西歐)", "", "enc:1252", d.encoding == encoding_rs::WINDOWS_1252)]),
        menu("語言", lang_items),
        menu("設定", vec![chk("自動換行", "", "wrap", st.wrap), chk("顯示行號", "", "linenumbers", st.line_numbers), chk("深色主題", "", "dark", st.dark),
            chk("覆寫模式", "Insert", "toggleovr", st.overwrite), chk("自動完成 (Ctrl+Space)", "", "autocomplete", st.autocomplete), chk("打字機模式 (游標行置中)", "", "typewriter", st.cfg.typewriter), chk("啟動時還原上次工作階段", "", "restoresession", st.cfg.restore_session), sep(),
            mi("編輯佈景主題色彩 / 快捷鍵 (settings.json)...", "", "editconfig"), mi("開啟設定檔所在資料夾", "", "openconfig"), sep(),
            mi(if cfg!(windows) { "安裝檔案總管右鍵選單「用 NotEdit 開啟」" } else { "安裝檔案管理員「用 NotEdit 開啟」(.desktop)" }, "", "installshell"),
            mi(if cfg!(windows) { "移除檔案總管右鍵選單" } else { "移除檔案管理員整合 (.desktop)" }, "", "uninstallshell"), sep()]
            .into_iter().chain(st.font_choices.iter().map(|f| chk(&format!("字型：{}{}", f, if BUNDLED_FONTS.contains(&f.as_str()) { "（內建）" } else { "" }), "", &format!("font:{}", f), *f == st.font_family))).collect()),
        menu("工具", vec![mi("AI 文件助手...", "", "ai"), sep(), mi("字數統計...", "", "wordcount"), mi("插入日期時間", "", "datetime"), sep(),
            mi("比較兩個檔案 (左：目前 / 右：分割檢視或選擇檔案)", "", "compare"), en("下一個差異", "F7", "diffnext", st.docs[st.views[0].doc].diff.is_some()), en("上一個差異", "Shift+F7", "diffprev", st.docs[st.views[0].doc].diff.is_some()),
            en("重新比較", "", "recompare", st.docs[st.views[0].doc].diff.is_some()), en("結束比較", "", "compareclear", st.docs.iter().any(|d| d.diff.is_some()))]),
        menu("巨集", vec![chk(if st.macro_rec { "停止錄製" } else { "開始錄製" }, "Ctrl+Shift+R", "macrorec", st.macro_rec),
            mi(&format!("播放 ({} 個動作)", st.macro_buf.len()), "Ctrl+Shift+P", "macroplay"), mi("重複播放 10 次", "", "macroplay10"), mi("清除巨集", "", "macroclear")]),
        menu("執行", vec![mi("在預設瀏覽器中開啟", "", "runbrowser"), mi("開啟所在資料夾", "", "openfolder")]),
        menu("外掛", vec![mi("(尚無外掛)", "", "none")]),
        menu("視窗", win_items),
        menu("?", vec![mi("關於 NotEdit...", "F1", "about")]),
    ];
    // show user-defined shortcuts in the menus
    if !st.cfg.keys.is_empty() {
        for m in &mut menus {
            let items: Vec<MenuItem> = m.items.iter().map(|mut it| { if let Some(k) = st.cfg.keys.get(it.action.as_str()) { it.shortcut = panels::norm_combo(k).into(); } it }).collect();
            m.items = ModelRc::new(VecModel::from(items));
        }
    }
    menus
}

/// Width of a tab button (kept in Rust so drop targets can be computed exactly).
fn tab_width(title: &str, bold: bool) -> f32 {
    let w: f32 = title.chars().map(|c| if unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) == 2 { 13.0 } else { 7.4 }).sum();
    let w = if bold { w * 1.08 } else { w };
    (w + 50.0).clamp(70.0, 260.0)
}

struct App {
    ui: slint::Weak<AppWindow>,
    st: RefCell<State>,
    me: RefCell<std::rc::Weak<App>>,
    clip: RefCell<Option<arboard::Clipboard>>,
    dirty: std::cell::Cell<bool>,
    rkey: RefCell<[(f32, f32, f32, f32); 2]>,
    ticker: slint::Timer,
    /// fires after 10 s without rendering; switches to a static caret
    idle: slint::Timer,
}

impl App {
    fn ui(&self) -> AppWindow { self.ui.upgrade().unwrap() }

    fn refresh_all(&self) {
        { let mut st = self.st.borrow_mut(); st.load_view0(); }
        self.refresh_meta();
        self.side_refresh();
        self.refresh_preview();
        self.render_all();
        self.ui().invoke_focus_editor();
    }

    fn remember_cursor(&self) { self.st.borrow_mut().store_view0(); }

    fn persist(&self) {
        self.remember_cursor();
        let mut st = self.st.borrow_mut();
        st.cfg.font = st.font; st.cfg.font_family = st.font_family.clone(); st.cfg.wrap = st.wrap; st.cfg.line_numbers = st.line_numbers; st.cfg.dark = st.dark; st.cfg.preview = st.preview;
        st.cfg.macro_keys = st.macro_buf.clone();
        st.cfg.docmap = st.docmap; st.cfg.autocomplete = st.autocomplete;
        st.cfg.sidebar = st.side_mode; st.cfg.workspace = st.side_root.clone(); st.cfg.workspace_expanded = st.side_expanded.iter().cloned().collect();
        st.cfg.session = st.docs.iter().filter_map(|d| d.path.clone().map(|p| settings::SessionFile { path: p, cursor: d.cursor })).collect();
        st.cfg.session_cur = st.docs[..st.cur].iter().filter(|d| d.path.is_some()).count();
        if let Err(e) = st.cfg.save() {
            eprintln!("NotEdit: failed to save settings: {e}");
        }
    }

    fn recovery_session(&self) -> recovery::RecoverySession {
        let st = self.st.borrow();
        let docs = st.docs.iter().filter(|d| d.modified && d.hex.is_none()).map(|d| {
            let disk_len = d.disk_stamp.as_ref().map(|s| s.len);
            let disk_modified_ns = d.disk_stamp.as_ref()
                .and_then(|s| s.modified.as_ref())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .and_then(|d| u64::try_from(d.as_nanos()).ok());
            recovery::RecoveryDoc {
                path: d.path.clone(),
                title: d.title.clone(),
                text: d.text(),
                eol: match d.eol { Eol::Crlf => "crlf", Eol::Lf => "lf", Eol::Cr => "cr" }.into(),
                encoding: d.encoding.name().to_string(),
                bom: d.bom,
                lang: d.lang.clone(),
                cursor: d.cursor,
                disk_len,
                disk_modified_ns,
            }
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

        let answer = crate::dlg::msg()
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
            let normalized_path = rd.path.map(|p| crate::file_io::canon(&p).unwrap_or(p));
            let mut d = Doc::new(0);
            d.path = normalized_path.clone();
            // Keep the stamp from the moment the recovery snapshot was made.
            // If the file changed while NotEdit was not running, the watcher
            // will compare this old stamp with disk and surface a conflict.
            d.disk_stamp = rd.disk_len.map(|len| file_watch::FileStamp {
                len,
                modified: rd.disk_modified_ns.map(|ns| UNIX_EPOCH + Duration::from_nanos(ns)),
            });
            d.title = rd.title;
            d.eol = match rd.eol.as_str() { "crlf" => Eol::Crlf, "cr" => Eol::Cr, _ => Eol::Lf };
            d.encoding = match rd.encoding.as_str() {
                "UTF-16LE" => encoding_rs::UTF_16LE,
                "UTF-16BE" => encoding_rs::UTF_16BE,
                _ => encoding_rs::Encoding::for_label(rd.encoding.as_bytes()).unwrap_or(encoding_rs::UTF_8),
            };
            d.bom = rd.bom;
            d.lang = rd.lang;
            d.cursor = rd.cursor;
            d.set_loaded(&rd.text);
            d.modified = true;
            d.saved = None;

            if let Some(i) = normalized_path.as_ref().and_then(|p| st.docs.iter().position(|x| x.path.as_ref() == Some(p))) {
                st.docs[i] = d;
                st.cur = i;
            } else {
                let replace_blank = st.docs.len() == 1
                    && st.docs[0].path.is_none()
                    && !st.docs[0].modified
                    && st.docs[0].buf.len() == 0;
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
        let ui = self.ui();
        let st = self.st.borrow();
        let shown = st.views[st.view_idx()].doc.min(st.docs.len() - 1);
        let d = &st.docs[shown];
        let tabs: Vec<TabInfo> = st.docs.iter().enumerate().map(|(i, d)| TabInfo { title: d.title.clone().into(), modified: d.modified, w: tab_width(&d.title, i == shown) }).collect();
        // only replace the model when it really changed (keeps tab TouchAreas alive during drags)
        let model = ui.get_tabs();
        let same = model.row_count() == tabs.len() && tabs.iter().enumerate().all(|(i, t)| model.row_data(i).as_ref() == Some(t));
        if !same { ui.set_tabs(ModelRc::new(VecModel::from(tabs))); }
        ui.set_current_tab(shown as i32);
        ui.set_menus(ModelRc::new(VecModel::from(build_menus(&st))));
        let name = d.path.as_ref().map(|p| p.display().to_string()).unwrap_or(d.title.clone());
        ui.set_window_title(format!("{}{} - NotEdit", if d.modified { "*" } else { "" }, name).into());
        ui.set_split(st.split);
        ui.set_docmap(st.docmap);
        ui.set_active_view(st.view_idx() as i32);
        ui.set_editor_font_size(st.font);
        ui.set_show_preview(st.preview && matches!(d.lang.as_str(), "markdown" | "html" | "xml"));
        ui.set_overwrite(st.overwrite);
        ui.set_dark(st.dark);
        ui.set_mono(st.font_family.as_str().into());
        ui.set_macro_recording(st.macro_rec);
        ui.set_st_type(lang_desc(&d.lang).into());
        ui.set_st_eol(d.eol.label().into());
        ui.set_st_encoding(d.encoding_label().into());
        let ad = &st.docs[st.views[st.view_idx()].doc];
        let lines = ad.buf.rope.len_lines();
        let display_len = ad.buf.len() + if ad.eol == Eol::Crlf { lines - 1 } else { 0 };
        match &ad.hex {
            Some(h) => ui.set_st_length(format!("bytes : {}    [HEX]", h.buf.len()).into()),
            None => ui.set_st_length(format!("length : {}    lines : {}{}", display_len, lines, if st.column_mode { "    [欄模式]" } else { "" }).into()),
        }
        ui.set_hex_mode(ad.hex.is_some());
        ui.set_total_lines(lines as i32);
    }

    fn check_external_changes(&self) {
        let docs: Vec<_> = {
            let st = self.st.borrow();
            st.docs.iter().enumerate().filter_map(|(i, d)| {
                d.path.clone().filter(|_| d.hex.is_none()).map(|p| (i, p, d.disk_stamp.clone(), d.modified, d.title.clone()))
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
            let answer = crate::dlg::msg()
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
            "markdown" => preview::markdown(&d.text()),
            "html" | "xml" => preview::html(&d.text()),
            _ => {
                let mut b = preview::markdown("*此檔案類型不支援預覽，請將語言設為 Markdown 或 HTML。*");
                b.truncate(1); b
            }
        };
        ui.set_preview_title(d.title.clone().into());
        ui.set_preview_blocks(ModelRc::new(VecModel::from(blocks)));
    }

    /// Command-line / forwarded argument: folders become the workspace.
    fn open_arg(&self, p: PathBuf) {
        if p.is_dir() { self.set_workspace(p); self.persist(); } else { self.open_path(p); }
    }

    fn open_path(&self, path: PathBuf) {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let already = { let cp = crate::file_io::canon(&path).unwrap_or(path.clone()); self.st.borrow().docs.iter().position(|d| d.path.as_ref() == Some(&cp)) };
        if let Some(i) = already { self.remember_cursor(); self.st.borrow_mut().cur = i; if !self.show_in_split_view() { self.refresh_all(); } return; }
        // binary or huge files open in hex mode (memory-mapped when large)
        let sample = { use std::io::Read; let mut b = vec![0u8; 8192]; std::fs::File::open(&path).and_then(|mut f| f.read(&mut b)).map(|n| { b.truncate(n); b }).unwrap_or_default() };
        if size > HUGE_TEXT || hex::looks_binary(&sample) {
            let buf = if size > 64 << 20 { hex::HexBuf::map(&path) } else { std::fs::read(&path).map(hex::HexBuf::from_vec) };
            match buf {
                Ok(b) => { self.open_hex(path, b); return; }
                Err(e) => { msg(&format!("無法開啟檔案：{}", e)); return; }
            }
        }
        let bytes = match std::fs::read(&path) { Ok(b) => b, Err(e) => { msg(&format!("無法開啟檔案：{}", e)); return; } };
        let (s, enc, bom) = decode(&bytes);
        let eol = detect_eol(&s);
        let text = s.replace("\r\n", "\n").replace('\r', "\n");
        self.remember_cursor();
        let path = crate::file_io::canon(&path).unwrap_or(path);
        {
            let mut st = self.st.borrow_mut();
            st.cfg.add_recent(path.clone());
            if let Some(i) = st.docs.iter().position(|d| d.path.as_ref() == Some(&path)) { st.cur = i; }
            else {
                let mut d = Doc::new(0);
                d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                d.lang = lang_from_path(&path);
                d.disk_stamp = file_watch::stamp(&path).ok();
                d.path = Some(path); d.set_loaded(&text); d.eol = eol; d.encoding = enc; d.bom = bom;
                let c = st.cur;
                if st.docs[c].path.is_none() && !st.docs[c].modified && st.docs[c].buf.len() == 0 { st.docs[c] = d; }
                else { st.docs.push(d); st.cur = st.docs.len() - 1; }
                if matches!(st.docs[st.cur].lang.as_str(), "markdown" | "html") { st.preview = true; }
            }
        }
        if self.show_in_split_view() { return; }
        self.refresh_all();
    }

    fn open_hex(&self, path: PathBuf, buf: hex::HexBuf) {
        self.remember_cursor();
        let path = crate::file_io::canon(&path).unwrap_or(path);
        {
            let mut st = self.st.borrow_mut();
            st.cfg.add_recent(path.clone());
            let mut d = Doc::new(0);
            d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            d.disk_stamp = file_watch::stamp(&path).ok();
            d.path = Some(path);
            d.hex = Some(Box::new(hexview::HexState::new(buf)));
            let c = st.cur;
            if st.docs[c].path.is_none() && !st.docs[c].modified && st.docs[c].buf.len() == 0 && st.docs[c].hex.is_none() { st.docs[c] = d; }
            else { st.docs.push(d); st.cur = st.docs.len() - 1; }
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
            if is_current { st.store_view0(); }
            let d = &mut st.docs[idx];
            d.hex = None;
            d.set_loaded(&text);
            d.eol = eol;
            d.encoding = enc;
            d.bom = bom;
            d.modified = false;
            d.saved = Some(d.buf.rope.clone());
            d.hl = Default::default();
            d.disk_stamp = disk_stamp;
            for v in st.views.iter_mut().filter(|v| v.doc == idx) { v.sels = vec![Sel::caret(0)]; v.lkey.0 = usize::MAX; }
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
                _ => crate::dlg::file().set_title("另存新檔").set_file_name(&d.title).save_file(),
            }
        };
        let Some(path) = path else { return false };
        if self.st.borrow().docs[idx].hex.is_some() {
            let r = self.st.borrow_mut().docs[idx].hex.as_mut().unwrap().buf.save(&path);
            if let Err(e) = r { msg(&format!("儲存失敗：{}", e)); return false; }
        } else {
        let bytes = match encode(&self.st.borrow().docs[idx]) {
            Ok(bytes) => bytes,
            Err(e) => { msg(&e); return false; }
        };
        if let Err(e) = file_io::durable_write(&path, &bytes) { msg(&format!("儲存失敗：{}", e)); return false; }
        }
        let saved_path: Option<PathBuf>;
        {
            let mut st = self.st.borrow_mut();
            let d = &mut st.docs[idx];
            d.title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if d.path.as_ref() != Some(&path) { d.lang = lang_from_path(&path); }
            d.path = Some(path.clone());
            d.disk_stamp = file_watch::stamp(&path).ok();
            d.modified = false;
            d.saved = if d.hex.is_none() { Some(d.buf.rope.clone()) } else { None };
            st.cfg.add_recent(path.clone());
            saved_path = Some(path);
        }
        self.refresh_meta();
        self.refresh_preview();
        self.persist_recovery();
        if let Some(p) = saved_path { self.reload_user_config(&p); }
        true
    }

    /// returns false when cancelled
    fn close(&self, idx: usize) -> bool {
        let (modified, title) = { let st = self.st.borrow(); (st.docs[idx].modified, st.docs[idx].title.clone()) };
        if modified {
            let r = crate::dlg::msg().set_title("NotEdit").set_description(format!("是否儲存「{}」的變更？", title))
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
            if st.split {
                if st.views[1].doc == idx { st.split = false; st.active = 0; }
                else if st.views[1].doc > idx { st.views[1].doc -= 1; }
            }
            if st.docs.is_empty() { st.counter += 1; let n = st.counter; st.docs.push(Doc::new(n)); }
            if st.cur >= st.docs.len() || st.cur > idx { st.cur = st.cur.saturating_sub(1).min(st.docs.len() - 1); }
        }
        self.refresh_all();
        self.persist_recovery();
        true
    }

    /// After open_path picked `st.cur`: when the split view is active, show that
    /// document there and give the main view back its previous document.
    fn show_in_split_view(&self) -> bool {
        let (split, active, cur, v0) = { let st = self.st.borrow(); (st.split, st.active, st.cur, st.views[0].doc) };
        if !(split && active == 1) { return false; }
        { let mut st = self.st.borrow_mut(); st.cur = v0.min(st.docs.len() - 1); }
        self.select_tab(cur);
        true
    }

    fn select_tab(&self, i: usize) {
        let (split, active) = { let st = self.st.borrow(); (st.split, st.active) };
        if split && active == 1 {
            // the split (right) view can show any document, independent of the main view
            {
                let mut st = self.st.borrow_mut();
                if i >= st.docs.len() { return; }
                let old = st.views[1].doc;
                if old != st.views[0].doc { let (sels, top) = (st.views[1].sels.clone(), st.views[1].top); if let Some(d) = st.docs.get_mut(old) { d.sels = sels; d.top = top; } }
                let d = &st.docs[i];
                let len = d.buf.len();
                let sels: Vec<Sel> = d.sels.iter().map(|s| Sel::new(s.anchor.min(len), s.head.min(len))).collect();
                let top = d.top;
                let v = &mut st.views[1];
                v.doc = i; v.sels = if sels.is_empty() { vec![Sel::caret(0)] } else { sels }; v.primary = 0; v.top = top;
                v.sx = 0.0; v.max_w = 0.0; v.ac = None; v.follow = true; v.drag = Drag::None; v.lkey = (usize::MAX, 0, 0, 0, false);
            }
            self.refresh_meta(); self.refresh_preview(); self.render_all(); self.side_refresh(); self.ui().invoke_focus_editor();
            return;
        }
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
        let (a, c) = self.sel_bytes();
        let found = search::find(&re, &self.text(), a.min(c), a.max(c), forward);
        match found {
            Some((s, e)) => { self.select_bytes(s, e); ui.set_find_status("".into()); true }
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
                let text = d.text();
                let d_text = &text;
                for m in re.find_iter(d_text).take(5000) {
                    if m.start() == m.end() { continue; }
                    let ls = d_text[..m.start()].rfind('\n').map(|x| x + 1).unwrap_or(0);
                    let le = d_text[m.start()..].find('\n').map(|x| x + m.start()).unwrap_or(d_text.len());
                    let line = d.buf.rope.byte_to_line(m.start()) + 1;
                    let snippet: String = d_text[ls..le].trim().chars().take(200).collect();
                    rows.push(SearchHit { doc: di as i32, title: d.title.clone().into(), line: line as i32, start: m.start() as i32, end: m.end() as i32, text: snippet.into() });
                    hits.push((di, m.start(), m.end()));
                }
            }
            ui.set_hits_title(format!("搜尋「{}」：在 {} 個文件中找到 {} 筆", ui.get_find_text(), st.docs.len(), rows.len()).into());
        }
        { let mut st = self.st.borrow_mut(); st.hits = hits; st.file_hits.clear(); }
        ui.set_hits(ModelRc::new(VecModel::from(rows)));
        ui.set_hits_visible(true);
    }

    fn hit_clicked(&self, i: usize) {
        if !self.st.borrow().file_hits.is_empty() { self.file_hit_clicked(i); return; }
        let Some(&(d, s, e)) = self.st.borrow().hits.get(i) else { return };
        if d != self.cd() { if self.av() == 1 { self.st.borrow_mut().active = 0; } self.select_tab(d); }
        let ok = { let t = self.text(); e <= t.len() && t.is_char_boundary(s) && t.is_char_boundary(e) };
        if ok { self.select_bytes(s, e); }
    }

    fn replace(&self, all: bool) {
        let ui = self.ui();
        let Some(re) = self.regex() else { return };
        let r = ui.get_replace_text().to_string();
        let rep = if ui.get_use_regex() { r.clone() } else { r.replace('$', "$$") };
        let t = self.text();
        if all {
            let c = re.find_iter(&t).count();
            let nt = re.replace_all(&t, rep.as_str()).into_owned();
            self.set_text(nt);
            ui.set_find_status(format!("已取代 {} 處", c).into());
            return;
        }
        let (s, e) = self.selection();
        if let Some(caps) = re.captures_at(&t, s).filter(|c| { let m = c.get(0).unwrap(); m.start() == s && m.end() == e && e > s }) {
            let mut out = String::new();
            caps.expand(&rep, &mut out);
            self.set_text(format!("{}{}{}", &t[..s], out, &t[e..]));
            let p = s + out.len();
            self.select_bytes(p, p);
        }
        self.find(true);
    }

    fn action_plain(&self, a: &str) {
        let cur = self.cd();
        match a {
            "undo" | "redo" => self.undo(self.av(), a == "undo"),
            "gotobrace" => {
                let vi = self.av();
                let m = { let st = self.st.borrow(); let v = &st.views[vi]; highlight::match_bracket(&st.docs[v.doc].buf.rope, v.sels[v.primary.min(v.sels.len() - 1)].head) };
                if let Some((_, j)) = m { { let mut st = self.st.borrow_mut(); let v = &mut st.views[vi]; v.sels = vec![Sel::caret(j)]; v.primary = 0; v.follow = true; } self.unfold_at_caret(vi); self.render(vi); self.refresh_status(); }
            }
            "bookmark" => self.toggle_bookmark(),
            "nextbm" => self.jump_bookmark(true),
            "prevbm" => self.jump_bookmark(false),
            "clearbm" => { { let mut st = self.st.borrow_mut(); st.docs[cur].bookmarks.clear(); } self.render_all(); }
            "zoomin" => { { let mut st = self.st.borrow_mut(); st.font = (st.font + 1.0).min(48.0); } self.refresh_meta(); }
            "zoomout" => { { let mut st = self.st.borrow_mut(); st.font = (st.font - 1.0).max(6.0); } self.refresh_meta(); }
            "print" | "exporthtml" => {
                let (html, title, path) = {
                    let st = self.st.borrow(); let d = &st.docs[cur];
                    let ext = d.path.as_ref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase());
                    (highlight::export_html(&d.text(), &d.lang, ext.as_deref(), &d.title, false), d.title.clone(), d.path.clone())
                };
                let target = if a == "print" {
                    let p = std::env::temp_dir().join(format!("notedit-print-{}.html", std::process::id()));
                    Some(p)
                } else {
                    let mut dlg = crate::dlg::file().set_title("匯出為 HTML").set_file_name(format!("{}.html", title)).add_filter("HTML", &["html"]);
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
            if index == 1 && self.st.borrow().split { self.ed_focused(1); } else if index == 0 { self.ed_focused(0); }
            // close an open autocomplete popup so it cannot swallow the menu
            let vi = self.av();
            if self.st.borrow().views[vi].ac.is_some() { self.st.borrow_mut().views[vi].ac = None; self.render(vi); }
            let (s, e) = self.selection();
            let sel = e > s;
            let st = self.st.borrow();
            let d = &st.docs[st.views[st.view_idx()].doc];
            let pv = matches!(d.lang.as_str(), "markdown" | "html" | "xml");
            let spell_items = { drop(st); self.spell_items() };
            let st = self.st.borrow();
            let d = &st.docs[st.views[st.view_idx()].doc];
            spell_items.into_iter().chain([en("復原", "Ctrl+Z", "undo", d.buf.can_undo()), en("取消復原", "Ctrl+Y", "redo", d.buf.can_redo()), sep(),
                 en("剪下", "Ctrl+X", "cut", sel), en("複製", "Ctrl+C", "copy", sel), en("貼上", "Ctrl+V", "paste", true), en("刪除", "Del", "delete", sel), en("全選", "Ctrl+A", "selectall", d.buf.len() > 0), sep(),
                 en("轉成大寫", "", "upper", sel), en("轉成小寫", "", "lower", sel), en("複製目前行", "Ctrl+D", "dupline", true), en("刪除目前行", "", "delline", true), sep(),
                 en("註解 / 取消註解", "Ctrl+Q", "comment", comment_syntax(&d.lang).is_some()), en("搜尋選取的文字", "", "searchsel", sel), sep(),
                 mi("切換書籤", "Ctrl+F2", "bookmark"), mi("全部摺疊", "", "foldall"), mi("全部展開", "", "unfoldall"),
                 chk(if st.split { "關閉分割檢視" } else { "複製到另一個檢視 (分割)" }, "", "split", st.split), sep(),
                 chk(if st.preview { "關閉預覽" } else { "開啟預覽" }, "", "preview", false)]).map(|mut m| { if m.action == "preview" { m.enabled = pv; } m }).collect()
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
                drop(st);
                self.clipboard_set(t);
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
        let (pre, post) = { let st = self.st.borrow(); match comment_syntax(&st.docs[self.cd()].lang) { Some(c) => c, None => return } };
        let t = self.text();
        let (s, e) = self.selection();
        let ls = t[..s.min(t.len())].rfind('\n').map(|x| x + 1).unwrap_or(0);
        let e2 = if e > s && t[..e].ends_with('\n') { e - 1 } else { e };
        let le = t[e2.min(t.len())..].find('\n').map(|x| x + e2).unwrap_or(t.len());
        let block = &t[ls..le];
        let new = toggle_comment_lines(block, pre, post);
        let nt = format!("{}{}{}", &t[..ls], new, &t[le..]);
        self.set_text(nt);
        self.select_bytes(ls, ls + new.len());
    }

    /// Index where a tab dragged by `dx` pixels from its original place would land.
    fn tab_target(&self, from: usize, dx: f32) -> i32 {
        let st = self.st.borrow();
        let ws: Vec<f32> = st.docs.iter().enumerate().map(|(i, d)| tab_width(&d.title, i == st.cur) + 1.0).collect();
        if from >= ws.len() { return -1; }
        let left: f32 = ws[..from].iter().sum();
        let center = left + ws[from] / 2.0 + dx;
        let mut x = 0.0;
        for (i, w) in ws.iter().enumerate() {
            if center < x + w { return i as i32; }
            x += w;
        }
        (ws.len() - 1) as i32
    }

    fn move_tab(&self, from: usize, to: usize) {
        {
            let mut st = self.st.borrow_mut();
            if from >= st.docs.len() || to >= st.docs.len() || from == to { return; }
            st.store_view0();
            let d = st.docs.remove(from);
            st.docs.insert(to, d);
            let v1 = st.views[1].doc;
            st.views[1].doc = if v1 == from { to } else if from < v1 && v1 <= to { v1 - 1 } else if to <= v1 && v1 < from { v1 + 1 } else { v1 };
            st.cur = to;
            st.load_view0();
        }
        self.refresh_meta();
        self.render_all();
    }

    fn action(self: &Rc<Self>, a: &str) {
        let ui = self.ui();
        if !a.starts_with("macro") && !matches!(a, "none" | "open" | "save" | "saveas" | "exit" | "about" | "find" | "replace" | "goto") { self.record(format!("a:{a}")); }
        let cur = self.cd();
        if self.is_hex(self.av()) {
            let k = match a { "undo" => "z", "redo" => "y", "copy" => "c", "cut" => "c", "paste" => "v", "selectall" => "a", _ => "" };
            if !k.is_empty() { self.hex_key(self.av(), k, true, false, false); self.refresh_status(); return; }
            if matches!(a, "upper" | "lower" | "trimtrail" | "sortlines" | "dupline" | "delline" | "comment" | "find" | "replace" | "findnext" | "findprev" | "goto" | "datetime") || a.starts_with("xf:") || a.starts_with("bmlines:") {
                msg("十六進位模式下不支援此功能，請先切換回文字模式 (Alt+H)。"); return;
            }
        }
        match a {
            "ai" => { ui.set_ai_visible(!ui.get_ai_visible()); }
            "new" => { self.remember_cursor(); { let mut st = self.st.borrow_mut(); st.counter += 1; let n = st.counter; st.docs.push(Doc::new(n)); st.cur = st.docs.len() - 1; } self.refresh_all(); }
            "open" => {
                if let Some(files) = crate::dlg::file().set_title("開啟舊檔")
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
            "undo" | "redo" | "gotobrace" | "print" | "exporthtml" | "bookmark" | "nextbm" | "prevbm" | "clearbm" | "zoomin" | "zoomout" => self.action_plain(a),
            "split" => self.toggle_split(),
            "openworkspace" => self.side_open_folder(),
            "side:1" => { let m = if self.st.borrow().side_mode == 1 { 0 } else { 1 }; self.side_switch(m); }
            "side:2" => { let m = if self.st.borrow().side_mode == 2 { 0 } else { 2 }; self.side_switch(m); }
            "compare" => self.compare(),
            "recompare" => { self.recompare(); self.render_all(); }
            "compareclear" => self.compare_clear(),
            "diffnext" => self.diff_nav(true, false),
            "diffprev" => self.diff_nav(false, false),
            "editconfig" => self.edit_user_config(),
            "installshell" | "uninstallshell" => {
                let r = if a == "installshell" {
                    let exe = std::env::current_exe().map(|e| file_io::canon(&e).unwrap_or(e)).unwrap_or_default();
                    shell::install_shell(&exe)
                } else { shell::uninstall_shell() };
                match r { Ok(m) => msg(&m), Err(e) => msg(&format!("失敗：{e}")) }
            }
            "otherview" => { let split = self.st.borrow().split; if split { let o = 1 - self.av(); self.st.borrow_mut().active = o; self.refresh_meta(); self.render_all(); ui.invoke_focus_editor(); } }
            "docmap" => { { let mut st = self.st.borrow_mut(); st.docmap = !st.docmap; } self.refresh_meta(); self.persist(); }
            "typewriter" => { { let mut st = self.st.borrow_mut(); st.cfg.typewriter = !st.cfg.typewriter; for v in st.views.iter_mut() { v.follow = true; } } self.persist(); self.refresh_meta(); self.render_all(); }
            "autocomplete" => { { let mut st = self.st.borrow_mut(); st.autocomplete = !st.autocomplete; } self.refresh_meta(); self.persist(); }
            "foldall" => self.fold_all(true),
            "unfoldall" => self.fold_all(false),
            "foldcur" => {
                let vi = self.av();
                let line = { let st = self.st.borrow(); let v = &st.views[vi]; let r = &st.docs[v.doc].buf.rope; r.char_to_line(v.sels[v.primary.min(v.sels.len() - 1)].head.min(r.len_chars())) };
                self.toggle_fold(vi, line);
            }
            "findall" => { ui.set_find_visible(true); if !ui.get_find_text().is_empty() { self.find_all(); } }
            "dark" => { { let mut st = self.st.borrow_mut(); st.dark = !st.dark; } self.apply_theme(); self.refresh_meta(); self.persist(); }
            "restoresession" => { { let mut st = self.st.borrow_mut(); st.cfg.restore_session = !st.cfg.restore_session; } self.refresh_meta(); self.persist(); }
            x if x.starts_with("ctx:") => self.ctx_action(&x[4..]),
            "delete" => { let vi = self.av(); let edits: Vec<_> = self.st.borrow().views[vi].sels.iter().map(|s| (s.start(), s.end(), String::new())).collect(); self.apply_edits(vi, edits, EditKind::Other, true); }
            "comment" => self.toggle_comment(),
            "searchsel" => {
                let (s, e) = self.selection();
                let t = self.text().get(s..e).unwrap_or("").to_string();
                if !t.is_empty() { ui.set_find_text(t.into()); ui.set_use_regex(false); ui.set_find_visible(true); self.find(true); }
            }
            x if x.starts_with("font:") => {
                { let mut st = self.st.borrow_mut(); st.font_family = x[5..].to_string(); }
                self.mark_dirty();
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
                    return;
                }
            }
            "macroclear" => { self.st.borrow_mut().macro_buf.clear(); self.refresh_meta(); }
            x if x.starts_with("recent:") => {
                let p = x[7..].parse::<usize>().ok().and_then(|i| self.st.borrow().cfg.recent.get(i).cloned());
                if let Some(p) = p { if p.exists() { self.open_path(p); } else { msg(&format!("找不到檔案：{}", p.display())); self.st.borrow_mut().cfg.recent.retain(|x| x != &p); self.refresh_meta(); } }
            }
            "clearrecent" => { self.st.borrow_mut().cfg.recent.clear(); self.refresh_meta(); }
            "cut" => self.copy(self.av(), true),
            x if x.starts_with("bmlines:") => self.bookmark_lines(&x[8..]),
            "copy" => self.copy(self.av(), false),
            "paste" => self.paste(self.av()),
            "selectall" => self.select_all(self.av()),
            "dupline" | "delline" => {
                let t = self.text();
                let b = self.sel_bytes().1.min(t.len());
                let ls = t[..b].rfind('\n').map(|x| x + 1).unwrap_or(0);
                let le = t[b..].find('\n').map(|x| x + b).unwrap_or(t.len());
                let line = &t[ls..le];
                let nt = if a == "dupline" { format!("{}\n{}{}", &t[..le], line, &t[le..]) }
                    else { let end = if le < t.len() { le + 1 } else { le }; let start = if end == le && ls > 0 { ls - 1 } else { ls }; format!("{}{}", &t[..start], &t[end..]) };
                self.set_text(nt);
            }
            "upper" | "lower" | "trimtrail" => {
                let t = self.text();
                let (s, e) = self.selection();
                let (s, e) = if e > s { (s, e) } else { (0, t.len()) };
                let seg = &t[s..e];
                let r = match a {
                    "upper" => seg.to_uppercase(),
                    "lower" => seg.to_lowercase(),
                    _ => seg.split('\n').map(|l| l.trim_end()).collect::<Vec<_>>().join("\n"),
                };
                self.set_text(format!("{}{}{}", &t[..s], r, &t[e..]));
                if e > s && e < t.len() { self.select_bytes(s, s + r.len()); }
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
            "zoomreset" => { { let mut st = self.st.borrow_mut(); st.font = 14.0; } self.refresh_meta(); }
            "toggleovr" => { { let mut st = self.st.borrow_mut(); st.overwrite = !st.overwrite; } self.refresh_meta(); }
            "stats" => { self.mad_action("wordcount"); }
            "datetime" => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let stamp = format_unix(now);
                self.insert_text(self.av(), &stamp);
            }
            "runbrowser" | "openfolder" => {
                let p = self.st.borrow().docs[cur].path.clone();
                if let Some(p) = p {
                    let target = if a == "openfolder" { p.parent().map(|x| x.to_path_buf()).unwrap_or(p) } else { p };
                    open_external(&target);
                } else { msg("請先儲存檔案。"); }
            }
            "about" => ui.set_about_visible(true),
            x => { self.mad_action(x); }
        }
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

// ======================================================================
// Custom editor surface: input handling and rendering for both views.
// ======================================================================

/// text files above this size open in (memory-mapped) hex mode
const HUGE_TEXT: u64 = 512 << 20;
const K_LEFT: char = '\u{F702}';
const K_RIGHT: char = '\u{F703}';
const K_UP: char = '\u{F700}';
const K_DOWN: char = '\u{F701}';
const K_HOME: char = '\u{F729}';
const K_END: char = '\u{F72B}';
const K_PGUP: char = '\u{F72C}';
const K_PGDN: char = '\u{F72D}';
const K_F2: char = '\u{F705}';
const K_INSERT: char = '\u{F727}';

fn line_chars(rope: &ropey::Rope, line: usize) -> Vec<char> {
    if line >= rope.len_lines() { return vec![]; }
    rope.line(line).chars().filter(|&c| c != '\n' && c != '\r').collect()
}

fn digits(n: usize) -> usize { n.max(1).to_string().len() }

impl State {
    fn store_view0(&mut self) {
        let v = &self.views[0];
        if let Some(d) = self.docs.get_mut(v.doc) {
            if !v.sels.is_empty() { d.sels = v.sels.clone(); }
            d.top = v.top;
            let head = d.sels.first().map(|s| s.head).unwrap_or(0).min(d.buf.len());
            d.cursor = d.buf.rope.char_to_byte(head);
        }
    }
    fn load_view0(&mut self) {
        let cur = self.cur.min(self.docs.len() - 1);
        let d = &self.docs[cur];
        let len = d.buf.len();
        let v = &mut self.views[0];
        v.doc = cur;
        v.sels = d.sels.iter().map(|s| Sel::new(s.anchor.min(len), s.head.min(len))).collect();
        if v.sels.is_empty() { v.sels.push(Sel::caret(0)); }
        v.primary = 0;
        v.top = d.top;
        v.sx = 0.0; v.max_w = 0.0; v.ac = None; v.follow = true; v.drag = Drag::None;
        v.lkey = (usize::MAX, 0, 0, 0, false);
    }
    fn view_idx(&self) -> usize { if self.split { self.active } else { 0 } }
}

impl App {
    /// index of the active view / its document
    fn av(&self) -> usize { self.st.borrow().view_idx() }
    fn cd(&self) -> usize { let st = self.st.borrow(); st.views[st.view_idx()].doc }

    fn text(&self) -> String { let st = self.st.borrow(); st.docs[st.views[st.view_idx()].doc].text() }

    /// (anchor, head) of the primary selection of the active view in byte offsets
    fn sel_bytes(&self) -> (usize, usize) {
        let st = self.st.borrow();
        let v = &st.views[st.view_idx()];
        let r = &st.docs[v.doc].buf.rope;
        let s = v.sels.get(v.primary).copied().unwrap_or(Sel::caret(0));
        (r.char_to_byte(s.anchor.min(r.len_chars())), r.char_to_byte(s.head.min(r.len_chars())))
    }

    fn selection(&self) -> (usize, usize) { let (a, c) = self.sel_bytes(); (a.min(c), a.max(c)) }

    fn select_bytes(&self, s: usize, e: usize) {
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
    fn set_text(&self, text: String) {
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

    fn after_edit(&self) {
        self.refresh_meta();
        if self.st.borrow().preview { self.refresh_preview(); }
        self.render_all();
    }

    /// Apply `edits` (sorted by start, non-overlapping) to the document of view `vi`.
    /// The selections of `vi` become carets at the end of each inserted text when `carets` is true.
    fn apply_edits(&self, vi: usize, edits: Vec<(usize, usize, String)>, kind: EditKind, carets: bool) {
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
    fn insert_text(&self, vi: usize, text: &str) {
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

    fn unfold_at_caret(&self, vi: usize) {
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

    fn metrics(&self) -> (f32, f32) {
        let ui = self.ui();
        (ui.get_cell_w().max(1.0), ui.get_line_h().max(1.0))
    }

    fn view_size(&self, vi: usize) -> (f32, f32) {
        let ui = self.ui();
        if vi == 0 { (ui.get_v0_w(), ui.get_v0_h()) } else { (ui.get_v1_w(), ui.get_v1_h()) }
    }

    /// Make sure the layout of view `vi` matches the document state.
    fn ensure_layout(&self, vi: usize) {
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
    fn pos_at(&self, vi: usize, x: f32, y: f32) -> (usize, usize, f32) {
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

    fn pos_in_row(&self, vi: usize, row_i: usize, cells: f32) -> usize {
        let st = self.st.borrow();
        let v = &st.views[vi];
        let r = &st.docs[v.doc].buf.rope;
        let row = v.layout.row(r, row_i.min(v.layout.len().saturating_sub(1)));
        let chars = line_chars(r, row.line);
        r.line_to_char(row.line) + editor::offset_at_cells(&chars, row.start, row.end, cells)
    }

    /// cells of `pos` within its visual row
    fn cells_of(&self, vi: usize, pos: usize) -> (usize, f32) {
        let st = self.st.borrow();
        let v = &st.views[vi];
        let r = &st.docs[v.doc].buf.rope;
        let ri = v.layout.row_of(r, pos);
        let row = v.layout.row(r, ri);
        let chars = line_chars(r, row.line);
        let col = pos.min(r.len_chars()) - r.line_to_char(row.line);
        (ri, editor::cells_between(&chars, row.start, col) as f32)
    }

    fn ed_focused(&self, vi: usize) {
        let changed = {
            let mut st = self.st.borrow_mut();
            let c = st.active != vi;
            st.active = vi;
            c
        };
        if changed { self.refresh_meta(); self.render_all(); }
    }

    fn ed_mouse(&self, vi: usize, kind: i32, x: f32, y: f32, ctrl: bool, shift: bool, alt: bool) {
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

    fn ed_gutter(&self, vi: usize, row: i32, fold_area: bool, shift: bool) {
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

    fn toggle_fold(&self, vi: usize, line: usize) {
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

    fn fold_all(&self, fold: bool) {
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

    fn ed_wheel(&self, vi: usize, dx: f32, dy: f32, shift: bool, ctrl: bool) {
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

    fn ed_vscroll(&self, vi: usize, f: f32) {
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

    fn ed_hscroll(&self, vi: usize, f: f32) {
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

    fn ed_map(&self, vi: usize, f: f32) {
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

    fn record(&self, entry: String) {
        let mut st = self.st.borrow_mut();
        if st.macro_rec && !st.macro_play { st.macro_buf.push(entry); }
    }

    fn ed_input(&self, vi: usize, text: &str) {
        // key events forwarded while idle may carry Slint's private-use key codes
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

    fn update_autocomplete(&self, vi: usize, force: bool) {
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

    fn accept_autocomplete(&self, vi: usize, idx: Option<usize>) {
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
    fn ed_key(&self, vi: usize, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
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

    fn ed_key_inner(&self, vi: usize, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
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
            if let Some(me) = self.me.borrow().upgrade() { slint::Timer::single_shot(std::time::Duration::from_millis(0), move || me.action(name)); }
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

    fn select_all(&self, vi: usize) {
        { let mut st = self.st.borrow_mut(); let State { docs, views, .. } = &mut *st; let v = &mut views[vi]; let n = docs[v.doc].buf.len(); v.sels = vec![Sel::new(0, n)]; v.primary = 0; }
        self.render(vi);
    }

    fn undo(&self, vi: usize, undo: bool) {
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

    fn clipboard_set(&self, t: String) -> bool {
        let mut cb = self.clip.borrow_mut();
        if cb.is_none() { *cb = arboard::Clipboard::new().ok(); }
        cb.as_mut().is_some_and(|c| c.set_text(t).is_ok())
    }

    fn bookmark_lines(&self, action: &str) {
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

    fn clipboard_get(&self) -> Option<String> {
        let mut cb = self.clip.borrow_mut();
        if cb.is_none() { *cb = arboard::Clipboard::new().ok(); }
        cb.as_mut().and_then(|c| c.get_text().ok())
    }

    fn copy(&self, vi: usize, cut: bool) {
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

    fn paste(&self, vi: usize) {
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

    fn delete(&self, vi: usize, forward: bool, word: bool) {
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

    fn newline(&self, vi: usize) {
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

    fn tab(&self, vi: usize, outdent: bool) {
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
                    let n = if chars.first() == Some(&'\t') { 1 } else { chars.iter().take(editor::TAB).take_while(|c| **c == ' ').count() };
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

    fn add_caret_vertical(&self, vi: usize, up: bool) {
        self.ensure_layout(vi);
        let (ri, cells) = { let st = self.st.borrow(); let v = &st.views[vi]; let s = if up { v.sels[0] } else { *v.sels.last().unwrap() }; drop(st); self.cells_of(vi, s.head) };
        let target = if up { ri.checked_sub(1) } else { Some(ri + 1) };
        let Some(t) = target else { return };
        if t >= self.st.borrow().views[vi].layout.len() { return; }
        let pos = self.pos_in_row(vi, t, cells);
        { let mut st = self.st.borrow_mut(); let v = &mut st.views[vi]; v.sels.push(Sel::caret(pos)); v.primary = v.sels.len() - 1; v.primary = editor::normalize(&mut v.sels, v.primary); v.follow = true; }
        self.render(vi);
    }

    fn move_carets(&self, vi: usize, k: char, ctrl: bool, shift: bool, alt: bool) {
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

    fn toggle_bookmark(&self) {
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

    fn jump_bookmark(&self, next: bool) {
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

    fn toggle_split(&self) {
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
        let w = self.ui.clone();
        let me = self.me.borrow().clone();
        slint::Timer::single_shot(Duration::from_millis(20), move || { if let Some(a) = me.upgrade() { a.render_all(); } if let Some(ui) = w.upgrade() { ui.invoke_focus_editor(); } });
        ui.invoke_focus_editor();
    }

    // ------------------------------------------------------------ rendering

    fn render_all(&self) {
        self.render(0);
        if self.st.borrow().split { self.render(1); }
        self.refresh_status();
    }

    fn render(&self, vi: usize) {
        if vi == 1 && !self.st.borrow().split { return; }
        if self.is_hex(vi) { self.hex_render(vi); return; }
        let ui = self.ui();
        if ui.get_caret_idle() { ui.set_caret_idle(false); }
        let w = ui.as_weak();
        self.idle.start(slint::TimerMode::SingleShot, Duration::from_secs(10), move || { if let Some(u) = w.upgrade() { u.set_caret_idle(true); } });
        let (cw, lh) = self.metrics();
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
        let fg = if dark { slint::Color::from_rgb_u8(0xdc, 0xdc, 0xdc) } else { slint::Color::from_rgb_u8(0x1e, 0x1e, 0x1e) };
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
            let spans: Vec<(usize, usize, slint::Color, bool, bool)> = match d.hl.line(row.line) {
                Some(ts) => { let mut o = 0; ts.iter().map(|t| { let n = t.text.chars().count(); let s = (o, o + n, t.color, t.bold, t.italic); o += n; s }).collect() }
                None => vec![(0, chars.len(), fg, false, false)],
            };
            let mut si = 0;
            let mut run = String::new();
            let mut run_start = row.start;
            let mut run_style = (fg, false, false);
            let flush = |run: &mut String, start: usize, style: (slint::Color, bool, bool), toks: &mut Vec<EdTok>| {
                if run.is_empty() { return; }
                let x = px(start);
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
                if c == '\t' || cwid != 1 || style != run_style || c.is_control() {
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
                num: if row.start == 0 { (row.line + 1).to_string().into() } else { SharedString::new() },
                line: row.line as i32, fold, cur: row.line == head_line,
                mark: row.start == 0 && d.bookmarks.contains(&row.line),
                diff: d.diff.as_ref().filter(|x| x.0 == d.buf.version).and_then(|x| x.1.get(&row.line)).map(|&k| k as i32).unwrap_or(0),
                toks: ModelRc::new(VecModel::from(toks)), rects: ModelRc::new(VecModel::from(rects)),
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
            Some(ac) => (ac.items.iter().take(10).map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>(), ac.sel as i32, true),
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
                let col = if dark { slint::Color::from_argb_u8(150, 0xc8, 0xc8, 0xc8) } else { slint::Color::from_argb_u8(150, 0x50, 0x50, 0x50) };
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
        let previous = if vi == 0 { ui.get_v0() } else { ui.get_v1() };
        let state = EdState {
            rows: render_model::update_rows(previous.rows, rows), carets: render_model::update(previous.carets, carets),
            gutter_w, num_w, thumb_pos, thumb_size, hthumb_pos, hthumb_size, hbar,
            ac_items: render_model::update(previous.ac_items, ac_items), ac_sel, ac_visible,
            title: title.into(),
            map_lines: render_model::update(previous.map_lines, map_lines), map_view_y: map_y, map_view_h: map_h,
        };
        if vi == 0 { ui.set_v0(state); } else { ui.set_v1(state); }
        self.rkey.borrow_mut()[vi] = (cw, lh, w, h);
    }

    /// Status bar for the active view.
    fn refresh_status(&self) {
        if self.st.borrow().side_mode == 2 { self.side_refresh(); }
        let ui = self.ui();
        if let Some(s) = self.hex_status() { ui.set_st_pos(s.into()); return; }
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
        ui.set_st_pos(s.into());
    }

    /// Periodic: re-render when sizes/metrics changed or highlighting is still in progress.
    /// Start the 30 ms housekeeping timer; it stops itself once nothing is pending,
    /// so an idle editor does not wake up (only the caret blink timer runs).
    fn mark_dirty(&self) { self.dirty.set(true); self.kick(); }

    fn kick(&self) {
        if self.ticker.running() { return; }
        let me = self.me.borrow().clone();
        self.ticker.start(slint::TimerMode::Repeated, Duration::from_millis(30), move || { if let Some(a) = me.upgrade() { a.tick(); } });
    }

    fn tick(&self) {
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

    fn play_macro(&self, times: usize) {
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

fn word_at(r: &ropey::Rope, pos: usize) -> (usize, usize) {
    let len = r.len_chars();
    let is_w = |p: usize| p < len && editor::is_word(r.char(p));
    if !is_w(pos) && !(pos > 0 && is_w(pos - 1)) { return (pos, (pos + 1).min(len).max(pos)); }
    let mut a = pos;
    while a > 0 && is_w(a - 1) { a -= 1; }
    let mut b = pos;
    while is_w(b) { b += 1; }
    (a, b)
}

fn line_span(r: &ropey::Rope, pos: usize) -> (usize, usize) {
    let line = r.char_to_line(pos.min(r.len_chars()));
    let a = r.line_to_char(line);
    let b = if line + 1 < r.len_lines() { r.line_to_char(line + 1) } else { r.len_chars() };
    (a, b)
}

fn word_start(r: &ropey::Rope, pos: usize) -> usize {
    let mut p = pos;
    while p > 0 && r.char(p - 1).is_whitespace() && r.char(p - 1) != '\n' { p -= 1; }
    if p > 0 && r.char(p - 1) == '\n' { return if p == pos { p - 1 } else { p }; }
    let w = p > 0 && editor::is_word(r.char(p - 1));
    while p > 0 && !r.char(p - 1).is_whitespace() && editor::is_word(r.char(p - 1)) == w { p -= 1; }
    p
}

fn word_end(r: &ropey::Rope, pos: usize) -> usize {
    let len = r.len_chars();
    let mut p = pos;
    if p < len && r.char(p) == '\n' { return p + 1; }
    let w = p < len && editor::is_word(r.char(p));
    while p < len && !r.char(p).is_whitespace() && editor::is_word(r.char(p)) == w { p += 1; }
    while p < len && r.char(p).is_whitespace() && r.char(p) != '\n' { p += 1; }
    p
}

/// line comment prefix, or (open, close) for block-only languages
fn comment_syntax(lang: &str) -> Option<(&'static str, &'static str)> {
    Some(match lang {
        "rust" | "c" | "cpp" | "cs" | "java" | "javascript" | "slint" => ("// ", ""),
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
    #[test]
    fn line_transform_is_one_undoable_edit() {
        let original = "file10\nfile2\n中文\n";
        let mut buffer = crate::editor::Buffer::from_str(original);
        let mut selections = vec![crate::editor::Sel::new(0, 6)];
        let sorted = crate::textops::reorder_lines(original, crate::textops::LineOrder::Natural);
        buffer.set_text(&selections, &sorted);
        assert_eq!(buffer.text(), "file2\nfile10\n中文\n");
        assert!(buffer.undo(&mut selections));
        assert_eq!(buffer.text(), original);
        assert_eq!(selections, vec![crate::editor::Sel::new(0, 6)]);
        assert!(!buffer.can_undo());
        assert!(buffer.redo(&mut selections));
        assert_eq!(buffer.text(), sorted);
    }
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

    #[test]
    fn legacy_encoding_rejects_unrepresentable_text() {
        let mut d = Doc::new(0);
        d.encoding = encoding_rs::BIG5;
        d.buf.reset("繁體中文");
        assert!(encode(&d).is_ok());

        d.buf.reset("繁體中文 😀");
        let err = encode(&d).unwrap_err();
        assert!(err.contains("Big5"));
    }

    #[test]
    fn utf8_and_utf16_are_lossless() {
        for encoding in [encoding_rs::UTF_8, encoding_rs::UTF_16LE, encoding_rs::UTF_16BE] {
            let mut d = Doc::new(0);
            d.encoding = encoding;
            d.buf.reset("中文\n日本語\nemoji 😀");
            assert!(encode(&d).is_ok(), "{}", encoding.name());
        }
    }

    #[test]
    fn encoding_respects_selected_eol() {
        let mut d = Doc::new(0);
        d.encoding = encoding_rs::UTF_8;
        d.eol = Eol::Crlf;
        d.buf.reset("a\nb");
        assert_eq!(encode(&d).unwrap(), b"a\r\nb");
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
    crate::dlg::msg().set_title("NotEdit").set_description(s).set_buttons(rfd::MessageButtons::Ok).show();
}

thread_local! { static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) }; }

fn shell_result(r: Result<String, String>) -> i32 {
    let (ok, m) = match r { Ok(m) => (true, m), Err(e) => (false, format!("失敗：{e}")) };
    println!("{m}");
    // a GUI-subsystem exe has no console on Windows, so also show a dialog there
    if cfg!(windows) { let _ = rfd::MessageDialog::new().set_title("NotEdit").set_description(m).show(); }
    if ok { 0 } else { 1 }
}

fn main() -> Result<(), slint::PlatformError> {
    // ---- CLI-only actions and single-instance forwarding (before any window exists)
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--install-shell") {
        let exe = std::env::current_exe().map(|e| file_io::canon(&e).unwrap_or(e)).unwrap_or_default();
        std::process::exit(shell_result(shell::install_shell(&exe)));
    }
    if args.iter().any(|a| a == "--uninstall-shell") { std::process::exit(shell_result(shell::uninstall_shell())); }
    let new_inst = args.iter().position(|a| a == "--new-instance" || a == "-multiInst").map(|i| args.remove(i)).is_some();
    let smoking = args.iter().any(|a| a == "--smoke-test");
    let single = !new_inst && !smoking;
    if single {
        let cwd = std::env::current_dir().unwrap_or_default();
        let abs: Vec<PathBuf> = args.iter().map(|a| { let p = PathBuf::from(a); if p.is_absolute() { p } else { cwd.join(p) } }).collect();
        if shell::forward(&abs) { return Ok(()); }
    }

    let ui = AppWindow::new()?;
    dlg::set_main(ui.as_weak());
    let cfg = settings::Settings::load();
    let st = State { docs: vec![Doc::new(1)], cur: 0, counter: 1, font: cfg.font.clamp(6.0, 48.0), wrap: cfg.wrap, line_numbers: cfg.line_numbers,
        preview: cfg.preview, overwrite: false, macro_rec: false, macro_play: false, macro_buf: cfg.macro_keys.clone(), dark: cfg.dark,
        hits: vec![], font_family: if cfg.font_family.trim().is_empty() { "JetBrains Mono".into() } else { cfg.font_family.clone() },
        font_choices: detect_fonts(), ctx_tab: 0, views: Default::default(), split: false, active: 0, docmap: cfg.docmap, autocomplete: cfg.autocomplete,
        column_mode: false, prompt_kind: String::new(), file_hits: vec![], fif_rx: None, fif_cancel: Default::default(), fif_root: PathBuf::new(), rif_backups: vec![], rif: None,
        side_mode: cfg.sidebar.min(2), side_root: cfg.workspace.clone().filter(|p| p.is_dir()), side_expanded: cfg.workspace_expanded.iter().cloned().collect(),
        side_rows: vec![], side_syms: vec![], side_sym_key: (usize::MAX, 0), cfg };
    spell::set_ignored(&st.cfg.spell_ignore);
    let app = Rc::new(App { ui: ui.as_weak(), st: RefCell::new(st), me: RefCell::new(std::rc::Weak::new()), clip: RefCell::new(None), ticker: Default::default(), idle: Default::default(),
        dirty: std::cell::Cell::new(true), rkey: RefCell::new([(0.0, 0.0, 0.0, 0.0); 2]) });
    *app.me.borrow_mut() = Rc::downgrade(&app);
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") { ui.set_ai_key(key.into()); }

    macro_rules! cb { ($m:ident, |$a:ident $(, $p:ident : $t:ty)*| $body:block) => {{ let $a = app.clone(); ui.$m(move |$($p: $t),*| $body); }}; }
    // Close open menus first and run the action after the next frame, so a modal
    // dialog opened by the action never shows a stale menu behind it.
    cb!(on_menu_action, |a, s: SharedString| {
        let ui = a.ui();
        if ui.get_open_menu() >= 0 || ui.get_ctx_visible() {
            ui.set_open_menu(-1); ui.set_ctx_visible(false);
            let w = Rc::downgrade(&a);
            slint::Timer::single_shot(Duration::from_millis(30), move || { if let Some(a) = w.upgrade() { a.action(&s) } });
        } else { a.action(&s) }
    });
    cb!(on_ai_send, |a| {
        let ui = a.ui();
        if ui.get_ai_running() { return; }
        let key = ui.get_ai_key().to_string();
        let model = ui.get_ai_model().trim().to_string();
        let prompt = ui.get_ai_prompt().to_string();
        let (role, role_name) = match ui.get_ai_role() {
            1 => (agent::Role::Proofread, "校稿"), 2 => (agent::Role::Translate, "翻譯"),
            3 => (agent::Role::Polish, "文字潤飾"), 4 => (agent::Role::Summarize, "摘要"),
            5 => (agent::Role::Format, "格式整理"), _ => (agent::Role::General, "通用助手"),
        };
        let history = ui.get_ai_history().to_string();
        let document = ui.get_ai_include_document().then(|| a.text());
        let title = { let st = a.st.borrow(); st.docs[st.views[st.view_idx()].doc].title.clone() };
        ui.set_ai_running(true);
        ui.set_ai_status("正在請求 OpenRouter...".into());
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let result = agent::request(&key, &model, &prompt, &history, document.as_deref(), role);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_ai_running(false);
                match result {
                    Ok(text) => {
                        ui.set_ai_history(format!("{history}\n使用者（{role_name}）：{prompt}\n助手：{text}\n").into());
                        ui.set_ai_result(text.into());
                        ui.set_ai_page(1);
                        ui.set_ai_has_source(document.is_some());
                        ui.set_ai_source(document.unwrap_or_default().into());
                        ui.set_ai_source_title(title.into());
                        ui.set_ai_status("完成：請檢查內容後套用".into());
                    }
                    Err(e) => ui.set_ai_status(e.into()),
                }
            });
        });
    });
    cb!(on_ai_apply, |a, new_doc: bool| {
        let ui = a.ui();
        if !new_doc && ui.get_ai_has_source() {
            let title = { let st = a.st.borrow(); st.docs[st.views[st.view_idx()].doc].title.clone() };
            if title != ui.get_ai_source_title().as_str() || a.text() != ui.get_ai_source().as_str() {
                ui.set_ai_status("文件已變更或切換，請重新送出以避免覆蓋變更".into()); return;
            }
        }
        if !new_doc && a.is_hex(a.av()) { ui.set_ai_status("請先切換回文字模式".into()); return; }
        if new_doc { a.action("new"); a.st.borrow_mut().active = 0; }
        a.set_text(ui.get_ai_result().to_string());
        ui.set_ai_status("已套用，可使用 Ctrl+Z 復原；請儲存文件".into());
    });
    cb!(on_ed_input, |a, v: i32, t: SharedString| { a.ed_input(v as usize & 1, &t) });
    cb!(on_ed_mouse, |a, v: i32, k: i32, x: f32, y: f32, c: bool, s: bool, al: bool| { a.ed_mouse(v as usize & 1, k, x, y, c, s, al) });
    cb!(on_ed_gutter, |a, v: i32, r: i32, f: bool, s: bool| { a.ed_gutter(v as usize & 1, r, f, s) });
    cb!(on_ed_wheel, |a, v: i32, dx: f32, dy: f32, s: bool, c: bool| { a.ed_wheel(v as usize & 1, dx, dy, s, c) });
    cb!(on_ed_vscroll, |a, v: i32, f: f32| { a.ed_vscroll(v as usize & 1, f) });
    cb!(on_ed_hscroll, |a, v: i32, f: f32| { a.ed_hscroll(v as usize & 1, f) });
    cb!(on_ed_ac_pick, |a, v: i32, i: i32| { a.accept_autocomplete(v as usize & 1, Some(i.max(0) as usize)) });
    cb!(on_ed_map, |a, v: i32, f: f32| { a.ed_map(v as usize & 1, f) });
    cb!(on_ed_focused, |a, v: i32| { a.ed_focused(v as usize & 1) });
    {
        let a = app.clone();
        ui.on_tab_target(move |from: i32, dx: f32| a.tab_target(from.max(0) as usize, dx));
    }
    cb!(on_side_click, |a, i: i32| { a.side_click(i.max(0) as usize) });
    cb!(on_side_activate, |a, i: i32| { a.side_activate(i.max(0) as usize) });
    cb!(on_side_switch, |a, m: i32| { a.side_switch(m) });
    cb!(on_side_open_folder, |a| { a.side_open_folder() });
    {
        let a = app.clone();
        ui.on_custom_key(move |t: SharedString, c: bool, s: bool, al: bool| a.custom_key(&t, c, s, al));
    }
    cb!(on_tab_selected, |a, i: i32| { a.select_tab(i as usize) });
    cb!(on_tab_close, |a, i: i32| { a.close(i as usize); });
    cb!(on_find_next, |a, f: bool| { a.find(f); });
    cb!(on_count_matches, |a| {
        if let Some(re) = a.regex() { let c = re.find_iter(&a.text()).count(); a.ui().set_find_status(format!("計數：{} 個符合項目", c).into()); }
    });
    cb!(on_replace_one, |a| { a.replace(false) });
    cb!(on_replace_all, |a| { a.replace(true) });
    cb!(on_find_all, |a| { a.find_all() });
    cb!(on_fif_browse, |a| { a.fif_browse() });
    cb!(on_fif_run, |a| { a.fif_run() });
    cb!(on_fif_stop, |a| { a.fif_stop() });
    cb!(on_fif_replace_all, |a| { a.fif_replace() });
    cb!(on_fif_replace_undo, |a| { a.fif_replace_undo() });
    cb!(on_prompt_ok, |a, t: SharedString| { a.prompt_ok(&t) });
    cb!(on_hit_clicked, |a, i: i32| { a.hit_clicked(i.max(0) as usize) });
    cb!(on_context_request, |a, k: SharedString, i: i32| { a.context_menu(&k, i) });
    cb!(on_rename_accepted, |a, t: SharedString| { a.rename(t.to_string()) });
    cb!(on_tab_move, |a, f: i32, t: i32| { a.move_tab(f.max(0) as usize, t.max(0) as usize) });
    {
        let a = app.clone();
        ui.on_ed_key(move |v: i32, t: SharedString, c: bool, s: bool, al: bool| a.ed_key(v as usize & 1, &t, c, s, al));
    }
    cb!(on_goto_line, |a, s: SharedString| {
        let ui = a.ui();
        if let Ok(n) = s.trim().parse::<usize>() {
            let off = { let st = a.st.borrow(); let r = &st.docs[a.cd()].buf.rope; r.line_to_byte(n.saturating_sub(1).min(r.len_lines() - 1)) };
            ui.set_goto_visible(false);
            a.select_bytes(off, off);
            ui.invoke_focus_editor();
        }
    });

    // open files passed on command line, otherwise restore last session
    // `--smoke-test <out-file>`: start the full UI, open the given files, write a
    // short report and quit (used by CI on Windows/macOS/Linux; no console needed).
    let smoke: Option<PathBuf> = args.iter().position(|a| a == "--smoke-test").map(|i| { args.remove(i); if i < args.len() { PathBuf::from(args.remove(i)) } else { PathBuf::from("notedit-smoke.txt") } });
    if args.is_empty() && app.st.borrow().cfg.restore_session {
        let (files, sel) = { let st = app.st.borrow(); (st.cfg.session.clone(), st.cfg.session_cur) };
        for f in files.iter().filter(|f| f.path.exists()) { app.open_path(f.path.clone()); }
        for f in &files {
            let p = crate::file_io::canon(&f.path).unwrap_or(f.path.clone());
            let mut st = app.st.borrow_mut();
            if let Some(d) = st.docs.iter_mut().find(|d| d.path.as_ref() == Some(&p)) {
                let t = d.text();
                d.cursor = f.cursor;
                d.set_loaded(&t);
            }
        }
        let n = app.st.borrow().docs.len();
        app.st.borrow_mut().cur = sel.min(n - 1);
    }
    for arg in args { app.open_arg(PathBuf::from(arg)); }
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));
    if single {
        // files opened later from Explorer / the file manager arrive here
        shell::listen(|paths| {
            let _ = slint::invoke_from_event_loop(move || APP.with(|a| if let Some(app) = a.borrow().clone() {
                for p in paths { app.open_arg(p); }
                app.refresh_all();
                app.ui().window().with_winit_window(|w| { w.set_minimized(false); w.focus_window(); });
            }));
        });
    }
    if smoke.is_none() { app.restore_recovery(); } // a modal recovery prompt would block the smoke test
    app.apply_theme();
    app.refresh_all();

    // drag & drop files from the OS
    {
        let a = app.clone();
        let pending: Rc<RefCell<Vec<PathBuf>>> = Default::default();
        ui.window().on_winit_window_event(move |_w, ev| {
            if let winit::event::WindowEvent::DroppedFile(p) = ev {
                pending.borrow_mut().push(p.clone());
                let (a, pending) = (a.clone(), pending.clone());
                // defer: the event filter must not re-enter the UI synchronously
                slint::Timer::single_shot(std::time::Duration::from_millis(0), move || {
                    let files: Vec<PathBuf> = pending.borrow_mut().drain(..).collect();
                    for f in files { if f.is_file() { a.open_path(f); } }
                });
            }
            EventResult::Propagate
        });
    }

    app.kick();
    cb!(on_layout_changed, |a| { a.kick() });

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
    let _smoke_timer = slint::Timer::default();
    if let Some(out) = smoke {
        let a = app.clone();
        _smoke_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(1500), move || {
            let ui = a.ui();
            a.action("side:2");
            // Exercise the real input callbacks; timings cover handlers, not GPU frames.
            let mut interaction = String::new();
            if !a.is_hex(0) {
                let before = ui.get_v0().rows;
                let tokens: Vec<_> = before.iter().map(|r| r.toks).collect();
                let version = { let st = a.st.borrow(); st.docs[st.views[0].doc].buf.version };
                let (_, lh) = a.metrics();
                let mut timings = Vec::new();
                for i in 0..40 {
                    let y = (i % before.row_count().max(1).min(10)) as f32 * lh + lh / 2.0;
                    let start = Instant::now();
                    ui.invoke_ed_mouse(0, 0, 24.0, y, false, false, false);
                    ui.invoke_ed_mouse(0, 1, 64.0, y, false, false, false);
                    ui.invoke_ed_mouse(0, 2, 64.0, y, false, false, false);
                    timings.push(start.elapsed().as_micros());
                }
                timings.sort_unstable();
                let after = ui.get_v0().rows;
                let reused = tokens.iter().zip(after.iter()).filter(|(old, row)| **old == row.toks).count();
                assert_eq!(before, after, "clicks must retain the row model");
                assert_eq!(reused, tokens.len(), "clicks must retain text models");
                { let st = a.st.borrow(); assert_eq!(version, st.docs[st.views[0].doc].buf.version); }
                interaction = format!("INPUT_CALLBACKS_OK gestures=40 text_models_reused={reused} handler_median_us={} handler_p95_us={}\n", timings[20], timings[37]);
            }
            let report = {
                let st = a.st.borrow();
                let sz = ui.window().size();
                format!("NOTEDIT_SMOKE_OK version={} docs={} first={} lines={} symbols={} window={}x{} scale={}\n{interaction}",
                    env!("CARGO_PKG_VERSION"), st.docs.len(), st.docs[0].title, st.docs[0].buf.rope.len_lines(), st.side_syms.len(),
                    sz.width, sz.height, ui.window().scale_factor())
            };
            let _ = std::fs::write(&out, report);
            let _ = slint::quit_event_loop();
        });
    }
    let r = ui.run();
    if single { shell::release(); }
    APP.with(|a| a.borrow_mut().take());
    r
}
