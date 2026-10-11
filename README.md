# NotEdit

以 **Rust + [Slint](https://slint.dev)** 製作的類 Notepad++ 文字編輯器，內建簡易 **HTML / Markdown 預覽**。介面為繁體中文。

![截圖](docs/screenshot.png)

## 功能

- **選單列**：檔案、編輯、搜尋、檢視、編碼、語言、設定、工具、巨集、執行、外掛、視窗、?
- **工具列**：SVG 圖示（ui/icons）與滑鼠提示，包含 新增、開啟、儲存、全部儲存、關閉、剪下/複製/貼上、復原/取消復原、尋找、取代、放大/縮小、自動換行、行號、預覽
- **多分頁**：分頁顯示修改狀態（● 未儲存 / ○ 已儲存），可點 ✕ 關閉，Ctrl+Tab 切換
- **檔案**：開新檔案、開啟（可多選、可從命令列帶入檔案）、重新載入、儲存、另存新檔、全部儲存、關閉 / 全部關閉 / 關閉其他；未儲存時詢問
- **編輯**：復原 / 取消復原（Ctrl+Z / Ctrl+Y）、剪下 / 複製 / 貼上 / 全選、複製目前行（Ctrl+D）、刪除目前行、轉大寫 / 小寫、移除行尾空白、排序行
- **搜尋**：尋找（Ctrl+F）、找下一個 / 上一個（F3 / Shift+F3）、取代 / 全部取代（Ctrl+H）、計數、大小寫須相符、跳至行（Ctrl+G）
- **檢視**：行號、自動換行、縮放（Ctrl + + / − / 0）、HTML/Markdown 預覽窗格
- **編碼**：自動偵測（BOM / UTF-8 / chardetng），支援 UTF-8、UTF-8-BOM、UTF-16 LE/BE、Big5、GBK、Shift-JIS、Windows-1252，儲存時依選擇的編碼轉換
- **換行格式**：自動偵測 CRLF / LF / CR，可轉換
- **語法上色**：使用 syntect（純 Rust regex-fancy，InspiredGitHub 配色），僅渲染可見行並快取每行解析狀態，編輯時從變更行起增量重算，數千行檔案仍流暢
- **程式圖示**：視窗圖示與 Windows exe 圖示（ui/icon.ico）
- **等寬字型**：內嵌 DejaVu Sans Mono（授權見 `ui/fonts/LICENSE-DejaVu.txt`）
- **語言**：依副檔名辨識（Rust、C/C++、C#、Java、Python、JS、HTML、CSS、XML、JSON、Markdown、Shell、Batch、INI、SQL、YAML、Slint…），可手動切換
- **狀態列**：檔案類型、length / lines、Ln / Col / Pos / Sel、換行格式、編碼、INS/OVR
- **工具**：字數統計、插入日期時間
- **執行**：以預設瀏覽器開啟、開啟所在資料夾

## 建置

```bash
cargo build --release
./target/release/notedit [檔案...]
```

Linux 需安裝：`libfontconfig1-dev libxkbcommon-dev libwayland-dev libgtk-3-dev`。

Windows / Linux / macOS 執行檔由 GitHub Actions 自動建置，可於 Actions 頁面的 Artifacts 下載。

## 已知限制（相較 Notepad++）

- 語法上色僅在「不自動換行」模式下啟用（自動換行時改為單色顯示）；選取範圍內文字以黑色顯示
- 語法定義使用 syntect 內建集合（無 TOML、Slint 專屬語法；Slint 以 Rust 規則近似）
- 預覽為簡化版：區塊層級樣式（標題、清單、引用、程式碼、連結），行內粗體/斜體以整段套用；不支援 CSS、圖片、表格版面
- 覆寫模式僅為狀態指示；巨集、外掛、程式碼摺疊、多重選取、分割視窗、正規表示式搜尋、自動完成、工作階段保存尚未實作
- 開啟自動換行時隱藏行號
