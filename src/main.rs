#![cfg_attr(windows, windows_subsystem = "windows")]
mod app_actions;
mod app_callbacks;
mod app_commands;
mod editor_actions;
mod editor_render;
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
mod agent;
mod credentials;
mod ai_files;
mod ui;

use ui::timer::{Timer, TimerMode};
use ui::{AppWindow, EdCaret, EdRect, EdRow, EdState, EdTok, MapLine, Menu, MenuItem, SearchHit, SideItem, TabInfo};
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

/// AcmeUI loads system fonts only (gap G1): the TTFs in ui/fonts are no longer bundled.
const BUNDLED_FONTS: &[&str] = &[];
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
    let menu = |t: &str, items: Vec<MenuItem>| Menu { title: t.into(), items };
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
            mi("OpenRouter API 金鑰...", "", "ai-settings"),
            chk("覆寫模式", "Insert", "toggleovr", st.overwrite), chk("自動完成 (Ctrl+Space)", "", "autocomplete", st.autocomplete), chk("打字機模式 (游標行置中)", "", "typewriter", st.cfg.typewriter), chk("啟動時還原上次工作階段", "", "restoresession", st.cfg.restore_session), sep(),
            mi("編輯佈景主題色彩 / 快捷鍵 (settings.json)...", "", "editconfig"), mi("開啟設定檔所在資料夾", "", "openconfig"), sep(),
            mi(if cfg!(windows) { "安裝檔案總管右鍵選單「使用 NotEdit 開啟」" } else { "安裝檔案管理員「使用 NotEdit 開啟」(.desktop)" }, "", "installshell"),
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
            for it in m.items.iter_mut() { if let Some(k) = st.cfg.keys.get(it.action.as_str()) { it.shortcut = panels::norm_combo(k); } }
        }
    }
    menus
}

/// Width of a tab button (kept in Rust so drop targets can be computed exactly).
/// Characters fonts like JetBrains Mono / Fira Code combine into ligatures.
fn is_lig_char(c: char) -> bool { matches!(c, '-' | '=' | '<' | '>' | '!' | '|' | '&' | ':' | '+' | '*' | '/' | '~' | '.' | '#' | '?' | '%' | '^' | '_' | '$' | '\\' | '[' | ']' | '{' | '}' | ';') }

fn tab_width(title: &str, bold: bool) -> f32 {
    let w: f32 = title.chars().map(|c| if unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) == 2 { 13.0 } else { 7.4 }).sum();
    let w = if bold { w * 1.08 } else { w };
    (w + 50.0).clamp(70.0, 260.0)
}

struct App {
    ui: AppWindow,
    st: RefCell<State>,
    me: RefCell<std::rc::Weak<App>>,
    clip: RefCell<Option<arboard::Clipboard>>,
    ai_key: RefCell<String>,
    dirty: std::cell::Cell<bool>,
    rkey: RefCell<[(f32, f32, f32, f32); 2]>,
    ticker: Timer,
    preview_timer: Timer,
    /// fires after 10 s without rendering; switches to a static caret
    idle: Timer,
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

/// Editor font: the configured family when installed, else the first installed
/// programming font, else the generic monospace family.
fn pick_font(cfg: &str, choices: &[String]) -> String {
    let want = cfg.trim();
    if !want.is_empty() && choices.iter().any(|c| c.eq_ignore_ascii_case(want)) { return want.to_string(); }
    choices.first().cloned().unwrap_or_else(|| "monospace".into())
}

fn main() {
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
    let mut start_guard: Option<shell::StartGuard> = None;
    if single {
        let cwd = std::env::current_dir().unwrap_or_default();
        let abs: Vec<PathBuf> = args.iter().map(|a| { let p = PathBuf::from(a); if p.is_absolute() { p } else { cwd.join(p) } }).collect();
        match shell::claim(&abs) { shell::Claim::Forwarded => return, shell::Claim::Primary(g) => start_guard = Some(g) }
    }

    let ui = AppWindow::new();
    let mut cfg = settings::Settings::load();
    if cfg.spell {
        if let Err(e) = spell::available() {
            cfg.spell = false;
            msg(&format!("無法啟用拼字檢查：{e}\n請確認拼字模組與程式位於同一資料夾。"));
        }
    }
    let font_choices = detect_fonts();
    let font_family = pick_font(&cfg.font_family, &font_choices);
    let st = State { docs: vec![Doc::new(1)], cur: 0, counter: 1, font: cfg.font.clamp(6.0, 48.0), wrap: cfg.wrap, line_numbers: cfg.line_numbers,
        preview: cfg.preview, overwrite: false, macro_rec: false, macro_play: false, macro_buf: cfg.macro_keys.clone(), dark: cfg.dark,
        hits: vec![], font_family, font_choices, ctx_tab: 0, views: Default::default(), split: false, active: 0, docmap: cfg.docmap, autocomplete: cfg.autocomplete,
        column_mode: false, prompt_kind: String::new(), file_hits: vec![], fif_rx: None, fif_cancel: Default::default(), fif_root: PathBuf::new(), rif_backups: vec![], rif: None,
        side_mode: cfg.sidebar.min(2), side_root: cfg.workspace.clone().filter(|p| p.is_dir()), side_expanded: cfg.workspace_expanded.iter().cloned().collect(),
        side_rows: vec![], side_syms: vec![], side_sym_key: (usize::MAX, 0), cfg };
    spell::set_ignored(&st.cfg.spell_ignore);
    editor::set_tab(st.cfg.tab_width);
    let app = Rc::new(App { ui: ui.clone(), st: RefCell::new(st), me: RefCell::new(std::rc::Weak::new()), clip: RefCell::new(None), ai_key: RefCell::new(String::new()), ticker: Default::default(), preview_timer: Default::default(), idle: Default::default(),
        dirty: std::cell::Cell::new(true), rkey: RefCell::new([(0.0, 0.0, 0.0, 0.0); 2]) });
    *app.me.borrow_mut() = Rc::downgrade(&app);
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    match credentials::load() {
        Ok(Some(key)) => { *app.ai_key.borrow_mut() = key.clone(); ui.set_ai_key(key); ui.set_ai_key_status("已載入 NotEdit 儲存的金鑰".into()); }
        Ok(None) => if let Ok(key) = std::env::var("OPENROUTER_API_KEY") { *app.ai_key.borrow_mut() = key.clone(); ui.set_ai_key(key); ui.set_ai_key_status("使用環境變數金鑰；可另行儲存到 NotEdit".into()); },
        Err(e) => ui.set_ai_key_status(format!("無法讀取已儲存的金鑰：{e}")),
    }

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
            ui::dispatch::invoke_from_event_loop(move || APP.with(|a| if let Some(app) = a.borrow().clone() {
                for p in paths { app.open_arg(p); }
                app.refresh_all();
                app.ui().request_focus_window();
            }));
        });
    }
    drop(start_guard.take()); // listener is published: queued launches now forward to us
    if smoke.is_none() { app.restore_recovery(); } // a modal recovery prompt would block the smoke test
    app.apply_theme();
    app.refresh_all();
    app.kick();

    // Periodically journal only modified buffers. Recovery never writes source
    // files; it is removed after a clean exit or once every buffer is saved.
    let ar = app.clone();
    let recovery_timer = Timer::default();
    recovery_timer.start(TimerMode::Repeated, std::time::Duration::from_secs(10), move || ar.persist_recovery());

    let aw = app.clone();
    let file_watch_timer = Timer::default();
    file_watch_timer.start(TimerMode::Repeated, std::time::Duration::from_secs(2), move || aw.check_external_changes());

    let _smoke_timer = Timer::default();
    if let Some(out) = smoke {
        let a = app.clone();
        _smoke_timer.start(TimerMode::SingleShot, Duration::from_millis(1500), move || smoke_test(&a, out.clone()));
    }
    let r = ui::shell::run(app.clone());
    drop((recovery_timer, file_watch_timer, _smoke_timer));
    if single { shell::release(); }
    APP.with(|a| a.borrow_mut().take());
    // pending single-shot callbacks hold Rc<App>: release them before thread-local teardown
    ui::timer::clear_all();
    if let Err(e) = r {
        eprintln!("NotEdit: {e}");
        msg(&format!("無法建立視窗：{e}"));
        std::process::exit(1);
    }
}

/// `--smoke-test`: exercise the real input handlers and write a report.
/// `NOTEDIT_SCREENSHOT=<file.png>` additionally saves a CPU-rendered frame.
fn smoke_test(a: &Rc<App>, out: PathBuf) {
    let ui = a.ui();
    // GPU start-up can take longer than the initial delay: wait (up to 10 s) for a laid-out frame
    static WAITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if (ui.get_win_w() <= 0.0 || ui.get_v0_h() <= 0.0) && WAITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 40 {
        let a = a.clone();
        Timer::single_shot(Duration::from_millis(250), move || smoke_test(&a, out));
        return;
    }
    a.action("side:2");
    // Exercise the real input handlers; timings cover handlers, not GPU frames.
    let mut interaction = String::new();
    if !a.is_hex(0) {
        let rows_before = ui.view_state(0).rows.len();
        let version = { let st = a.st.borrow(); st.docs[st.views[0].doc].buf.version };
        let (_, lh) = a.metrics();
        let mut timings = Vec::new();
        for i in 0..40 {
            let y = (i % rows_before.max(1).min(10)) as f32 * lh + lh / 2.0;
            let start = Instant::now();
            a.ed_mouse(0, 0, 24.0, y, false, false, false);
            a.ed_mouse(0, 1, 64.0, y, false, false, false);
            a.ed_mouse(0, 2, 64.0, y, false, false, false);
            timings.push(start.elapsed().as_micros());
        }
        timings.sort_unstable();
        let rows_after = ui.view_state(0).rows.len();
        assert_eq!(rows_before, rows_after, "clicks must not change the visible rows");
        assert!(rows_after > 0, "the editor must render rows");
        { let st = a.st.borrow(); assert_eq!(version, st.docs[st.views[0].doc].buf.version); }
        interaction = format!("INPUT_CALLBACKS_OK gestures=40 rows={rows_after} handler_median_us={} handler_p95_us={}\n", timings[20], timings[37]);
    }
    let report = {
        let st = a.st.borrow();
        let scale = ui.get_scale();
        format!("NOTEDIT_SMOKE_OK version={} docs={} first={} lines={} symbols={} window={}x{} scale={}\n{interaction}",
            env!("CARGO_PKG_VERSION"), st.docs.len(), st.docs[0].title, st.docs[0].buf.rope.len_lines(), st.side_syms.len(),
            (ui.get_win_w() * scale).round(), (ui.get_win_h() * scale).round(), scale)
    };
    let report = if std::env::var_os("NOTEDIT_SPELL_SMOKE").is_some() {
        let result = spell::available().is_ok() && spell::check("hello") && !spell::check("helo") && spell::suggest("helo").iter().any(|s| s == "hello");
        format!("{report}{}\n", if result { "SPELL_MODULE_OK" } else { "SPELL_MODULE_FAILED" })
    } else { report };
    if let Some(p) = std::env::var_os("NOTEDIT_SCREENSHOT") { ui.request_screenshot(PathBuf::from(p)); }
    if std::env::var_os("NOTEDIT_PREVIEW_SMOKE").is_some() && ui.get_show_preview() {
        a.select_bytes(0, 0);
        let first = |ui: &AppWindow| ui.preview_blocks_ref().first().map(|b| b.text.clone()).unwrap_or_default();
        let before = first(&ui);
        let baseline = std::env::var_os("NOTEDIT_PREVIEW_BASELINE").is_some();
        let start = Instant::now();
        for _ in 0..10 {
            a.ed_input(0, "x");
            if baseline { a.refresh_preview(); }
        }
        let input_us = start.elapsed().as_micros();
        let during = first(&ui);
        let a = a.clone();
        Timer::single_shot(Duration::from_millis(400), move || {
            let after = first(&a.ui());
            let result = if baseline && during != before && after == during && after.contains("xxxxxxxxxx") { "PREVIEW_BASELINE_OK" }
                else if !baseline && during == before && after != before && after.contains("xxxxxxxxxx") { "PREVIEW_DEBOUNCE_OK" }
                else { "PREVIEW_SMOKE_FAILED" };
            let _ = std::fs::write(&out, format!("{report}{result} edits=10 input_us={input_us}\n"));
            a.ui().request_quit();
        });
        return;
    }
    let _ = std::fs::write(&out, report);
    ui.request_quit();
}
