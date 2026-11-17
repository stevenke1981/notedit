# Notepad++ parity 稽核：issue #2 × NOTEPADPP_PARITY.md × 程式碼實況

稽核日期：2026-11-17　基準 commit：`cc6c407`（PR #9 AcmeUI-Native 合併後，v0.7.1）

方法：每個功能都從核心邏輯 → callback／command → UI 觸發點（選單 `src/main.rs` build_menus、按鍵 `src/app_callbacks.rs`／`src/editor_actions.rs`、滑鼠 `src/ui/shell.rs`／`src/ui/hit.rs`）→ 繪製（`src/ui/view_*.rs`）逐段追蹤。對照組是換 UI 前的 `ui/app.slint`（`7317f64^`，最後一個仍使用 Slint UI 的 commit）。顧問（Fable）逐一比對了 Slint 的 45 個 `app.slint` callback、11 個 `editor.slint` callback、130 個選單 action 和全域按鍵表，現在都有對應的實作。

判定：**Done** ＝ 有邏輯、接上 UI，而且達到文件驗收標準；**Partial** ＝ 有接上 UI，但驗收項目缺一部分；**Missing** ＝ 沒有實作；**換 UI 後遺失** ＝ Slint 版有、AcmeUI 版沒接上。

## 結論摘要

- **沒有發現「換 UI 後遺失」的功能。** P2 編輯引擎（自動完成、分割檢視、多游標、欄模式、摺疊、文件地圖）全部在 AcmeUI 版本中繪製並可操作，沒有只剩 headless 邏輯的情況。
- **issue #2 嚴重過時。** 23 個未勾選項目中，7 個在程式碼中已經是 Done（建議勾選），9 個是 Partial，7 個確實 Missing。
- **NOTEPADPP_PARITY.md 有自相矛盾和 Slint 殘留**，另外幾項把 Partial／Missing 寫成 Implemented／Partial（見第 3 節）。
- **`cargo test --locked`：通過**（notedit 68 passed / 0 failed / 2 ignored，ignored 的是需要 OpenRouter 金鑰的線上測試；notedit_spell 1 passed）。

## 1. 對照表

圖例：issue 欄 ☑＝已勾、☐＝未勾、—＝issue 沒列這項。

### P0 — Reliability

| 功能 | issue #2 | 文件 | 程式碼實況 | 證據 |
|---|---|---|---|---|
| Durable save | ☑ | Implemented | **Done** | staged temp + fsync + rename：`src/file_io.rs:44-67`；存檔呼叫：`src/app_actions.rs:363`；測試：`src/file_io.rs:97,106,116`。Windows 上 rename 覆蓋不是原子操作，也沒有測試失敗路徑 |
| Crash recovery（具名＋未命名） | ☑ | Implemented | **Done** | `src/recovery.rs`；每 10 s 寫一次：`src/main.rs:681-682`；還原對話框：`src/app_actions.rs:63-129`。hex 文件不納入；`restore_recovery` 沒有測試 |
| 外部變更／刪除偵測 | ☑ | Implemented | **Done** | `src/app_actions.rs:166-221`，2 s 輪詢：`src/main.rs:686`。提示框是 rfd 原生對話框；一次只處理第一個變更的檔案 |
| 大檔行為／不整份複製 | ☐ | Implemented (v0.3.0) | **Done** → 建議勾選 | ropey：`src/editor.rs:4`；只建可見列：`src/editor_render.rs:37-60`；增量 highlight（20 ms 預算）：`src/editor_render.rs:63-67`、`src/highlight.rs:142,170-176`。文件寫的「140 MB ~1 s」沒有實測 |
| Encoding/EOL round-trip 測試矩陣 | ☐ | 「regression tests cover legacy and Unicode」 | **Partial** | 只有 Big5 拒存（`src/main.rs:527`）、UTF-8/16 只驗 `is_ok()`（`src/main.rs:539`）、CRLF（`src/main.rs:549`）、FiF 保留 CRLF/BOM（`src/fif.rs:156`）。沒有 decode→encode 位元組 round-trip，也沒有 Shift-JIS/GBK/CP1252/裸 CR/`detect_eol` 的測試 |

### P1 — Navigation / project editing

| 功能 | issue #2 | 文件 | 程式碼實況 | 證據 |
|---|---|---|---|---|
| Find in Files | ☐ | Implemented | **Done** → 建議勾選 | 對話框：`src/ui/view_dialogs.rs:47-78`；Ctrl+Shift+F：`src/app_callbacks.rs:68`；背景執行緒：`src/madops.rs:245-249`；結果面板：`src/ui/view_side.rs:80-115`；點擊開檔：`src/ui/shell.rs:670` → `src/madops.rs:430` |
| Replace in Files（備份還原、停止、進度） | — | v0.5 表 Done ／ MadEdit 表 Missing（矛盾） | **Done** | 按鈕：`src/ui/view_dialogs.rs:70-76`；執行緒：`src/madops.rs:254-371`；.bak 還原：`src/madops.rs:379` |
| 在所有開啟文件中尋找 | — | Implemented | **Done** | `src/app_commands.rs:24-57`、`src/ui/view_dialogs.rs:36` |
| Folder as Workspace | ☐ | Partial（no file ops） | **Partial** | 樹、開資料夾、雙擊開檔：`src/ui/view_side.rs:41-77`、`src/panels.rs:153-183`、`src/ui/shell.rs:662-666`。**缺**：refresh、新增／重新命名／刪除、側欄右鍵選單（`src/ui/shell.rs:528-545` 只處理分頁和編輯區） |
| Function List | ☐ | Implemented（no filter box） | **Partial** | 符號擷取：`src/workspace.rs:44-100`（16 種語言 key，文件寫 14）；點擊跳轉：`src/panels.rs:177-180`。**缺** filter box（`src/ui/hit.rs:6` 沒有對應的 FieldId），而這是驗收標準之一，所以不算 Implemented |
| Bookmarks | ☐（與 mark-all 合為一項） | Implemented / Partial | **Done**（書籤本身） | Ctrl+F2／F2／Shift+F2：`src/editor_actions.rs:567,585`；gutter 圓點：`src/ui/view_editor.rs:72`；反轉／複製／剪下／刪除書籤行：`src/main.rs:309-311` → `src/bookmarks.rs`。點 gutter 不會切換書籤（Slint 版也一樣，不算遺失） |
| Mark-all 搜尋結果 | （同上） | Partial | **Partial** | 只會自動標示選取字詞：`src/editor_render.rs:75,156`。沒有 Notepad++ 式的 Mark 搜尋結果功能 |
| MRU 文件切換器 | ☐ | 併在 Tab management Partial 裡 | **Missing** | Ctrl+Tab 只執行 `nexttab`（依分頁順序）：`src/app_callbacks.rs:74` → `src/app_commands.rs:380`；搜不到 `mru`。另外：自動完成彈出時 Ctrl+Tab 會被吃掉（`src/editor_actions.rs:540`）；Ctrl+Shift+Tab 也是 `nexttab`，`prevtab` 沒有綁定按鍵（Slint 版也一樣） |
| 分頁管理 | ☐ | Partial | **Partial** | 有：關閉其他／左側／右側（`src/app_commands.rs:129-132,164-166`）、拖曳排序（`src/ui/shell.rs:572-574,613-617,641-645`）。**缺**：pin、sort、close unchanged |
| Compare | — | Implemented | **Done** | `src/panels.rs:203-289`，F7/Shift+F7：`src/app_callbacks.rs:84`；不做行對齊或同步捲動（與文件一致） |
| HTML/Markdown preview | — | Implemented | **Done** | `src/ui/view_preview.rs`、`src/ui/shell.rs:376-379`、`src/app_actions.rs:223` |

### P2 — Editor engine（重點：換 UI 後是否仍接上 UI）

| 功能 | issue #2 | 文件 | 程式碼實況 | 證據（核心 → 觸發 → 繪製） |
|---|---|---|---|---|
| Auto-completion | ☐ | Implemented | **Done** → 建議勾選 | `src/editor.rs:413`（文件字詞＋語言關鍵字）→ 打字時：`src/editor_actions.rs:476-502`、Ctrl+Space：`:564`、鍵盤導覽：`:533-543`、滑鼠：`src/ui/shell.rs:661` → 繪製：`src/ui/view_editor.rs:134-153`；開關：`src/app_commands.rs:295`、`src/settings.rs:24`。多游標或有選取時不彈出 |
| Dual/split view + clone | ☐ | Implemented | **Done** → 建議勾選 | `toggle_split`：`src/editor_actions.rs:948-968`；選單：`src/main.rs:315`、F8 切換焦點：`src/editor_actions.rs:552`；第二檢視的版面：`src/ui/shell.rs:315-320`，鍵盤／滑鼠輸入：`src/ui/shell.rs:421,557-561`。驗收標準是「Move **or** clone」，clone 已有；沒有「移到另一檢視」(move) 指令 |
| Multi-caret | ☐ | Implemented | **Done** → 建議勾選 | Ctrl+click：`src/ui/shell.rs:561` → `src/editor_actions.rs:261-265`；Ctrl+Alt+↑↓：`src/editor_actions.rs:818-826`；同步編輯：`:72-96`；undo 合併：`src/editor.rs:140-158`；繪製：`src/ui/view_editor.rs:111-119`。沒有 Ctrl+D「加入下一個相符」 |
| Column mode | ☐ | Implemented | **Partial** | Alt+C：`src/editor_actions.rs:553`；Alt+drag：`:230,254-256,287-300`；Alt+Shift+方向鍵：`:834-855`；狀態列「[欄模式]」：`src/app_actions.rs:160`。**缺**：遞增編號不認矩形欄位（`src/madops.rs:34-42,165-168`，從行首插入）；狀態列不顯示矩形大小 |
| Code folding | ☐ | Implemented | **Done** → 建議勾選 | `src/editor_actions.rs:339,363`；gutter ±：`src/ui/view_editor.rs:79-84`；點擊：`src/ui/shell.rs:563-568` → `src/editor_actions.rs:312-323`；Alt+0／Alt+Shift+0：`:549-550`。依括號／縮排判斷 |
| Document map | ☐ | Implemented | **Done** → 建議勾選 | 繪製＋視窗框：`src/ui/view_editor.rs:172-182`；點擊／拖曳：`src/ui/shell.rs:571,612` → `src/editor_actions.rs:436-451`。分割檢視時隱藏（`src/ui/shell.rs:311`），Slint 版也是這樣，不算遺失 |
| Typewriter mode | — | v0.5 表 Done ／ MadEdit 表 Missing（矛盾） | **Done** | `src/editor_render.rs:49`、`src/app_commands.rs:294` |

### P3 — Extensibility

| 功能 | issue #2 | 文件 | 程式碼實況 | 證據 |
|---|---|---|---|---|
| User-defined languages | ☐ | Missing | **Missing** | 只內建 TOML：`src/highlight.rs:8-15` |
| 可設定的外部命令 | ☐ | **Partial** | **Missing** | Run 選單只有兩個寫死的項目：`src/main.rs:336` → `src/app_commands.rs:396-402`。不能設定，沒有變數，也不擷取輸出 |
| Versioned plugin API | ☐ | Missing | **Missing** | 「外掛」選單是佔位：`src/main.rs:337`（「尚無外掛」）。spell DLL（`src/spell.rs:14-40`）不是通用 API |
| Plugin isolation / capability manifest | ☐ | Missing | **Missing** | — |
| 自訂主題／快捷鍵 | — | Implemented | **Done**（符合文件寫的範圍） | `src/panels.rs:73-97`；只在 NotEdit 內存檔時 reload（`src/app_actions.rs:381`），外部改檔不會套用；沒有 GUI |
| Shell 整合／單一實例 | — | Implemented | **Done** | `src/shell.rs:16-160`、`src/main.rs:608-612,663-671`；不支援 macOS |

### P4 — Distribution / quality

| 功能 | issue #2 | 文件 | 程式碼實況 | 證據 |
|---|---|---|---|---|
| Installer/update | ☐ | Partial | **Partial** | IronSetup（Inno .iss 子集）：`installer/notedit.iss`、`scripts/package-windows.ps1:14-28`、CI：`.github/workflows/build.yml:97`；沒有更新機制 |
| Checksums/signing | ☐ | （併在 Installer Partial） | **Missing** | workflow 沒有 hash 或簽章步驟；`Get-FileHash` 只印到 console：`scripts/package-windows.ps1:29` |
| Accessibility + i18n | ☐ | Partial | **Partial**（a11y）／**Missing**（i18n） | 鍵盤操作和對話框 Tab 順序：`src/ui/shell.rs:173`；沒有 accessibility tree 或螢幕閱讀器支援；UI 字串寫死中文（`src/main.rs:286-337`） |
| Security/path normalization | ☐ | Partial | **Partial**（未深入稽核） | `file_io::canon`：`src/file_io.rs:135-139`，recovery／開檔／session 都會用到（`src/app_actions.rs:88,248,264`）。沒有外部命令邊界或相依套件審查的紀錄 |
| Performance benchmarks / release gates | ☐ | 未列 | **Missing** | repo 中沒有 bench 檔案 |

### issue 沒列、文件有提到的其他功能（全部 Done）

Hex mode（經編輯區繪製：`src/hexview.rs:49-115`）、spell check（en_US）、繁簡轉換、JSON/XML 格式化、macro 錄製／播放、深色主題／字型／縮放／狀態列、右鍵選單，都已接上 UI。AI 面板（`src/ui/view_ai.rs`）是 Done，但 parity 文件完全沒提到。

## 2. 建議更新 issue #2

- **勾選（Done）**：large-file、Find in Files、auto-completion、dual/split view + clone、multi-caret、code folding、document map。
- **保留未勾，但補一句剩下什麼**：encoding/EOL 矩陣、Folder as Workspace（refresh、檔案操作）、Function List（filter）、bookmarks＋mark-all（書籤 Done，mark 搜尋結果缺）、分頁管理（pin/sort/close unchanged）、column mode（欄感知編號、矩形狀態）、installer、a11y/i18n、security。
- **確實 Missing**：MRU、UDL、外部命令、plugin API／隔離、checksums/signing、benchmarks。
- issue 內文的「Slint TextInput primitive 不足」規則已經過時：EditorCore（`src/editor.rs`）加上自繪編輯區（`src/ui/view_editor.rs`）就是那條規則要求的架構。
- 「下一批：P1 Find in Files → …」也過時了（FiF 已 Done）。

## 3. NOTEPADPP_PARITY.md 與實況不符／Slint 殘留

| 行 | 問題 |
|---|---|
| 38 | 指向 `ui/editor.slint`，這個檔案在 `4794a40` 已刪除。目前編輯區在 `src/ui/view_editor.rs`（繪製）＋ `src/editor.rs`（核心） |
| 50 | 「native TextInput caret replaces our blink timer」「Software GL ~12% → ~6%」是 Slint 時代的描述。現在編輯區游標自繪，有 500 ms blink timer（`src/ui/shell.rs:245-247`、`src/ui/view_editor.rs:112-130`）；`acme_textinput` 只用在對話框欄位；渲染走 `acme-render-cpu`，不是 GL |
| 66 | 「Replace in Files missing」和第 44 行、程式碼（Done）矛盾 |
| 76 | 把 Typewriter mode、JSON/XML formatters 列為 Missing，和第 45-49 行、程式碼矛盾（只有 Astyle、Python macros、Pan-Chinese search 真的 Missing；auto backup 可以說部分由 recovery journal 涵蓋） |
| 85 | 「regression tests cover legacy and Unicode encodings」誇大：只有 Big5 拒存和 UTF-8/16 `is_ok()` |
| 89 | Function List「Implemented … 14 languages」：缺 filter（驗收標準的一部分）應改為 Partial；語言數是 16 |
| 92 | Split view「Move or clone」：只有 clone，建議在 Notes 註明沒有 move 指令 |
| 94 | Column mode「incremental numbering」「status reporting」：編號不認欄位，狀態列只有旗標，應為 Partial |
| 96 | Document map 沒註明分割檢視時隱藏 |
| 97 | Tab management Partial 應列明：pin/sort/close unchanged/MRU 缺 |
| 99 | Custom theme / shortcuts「hot-reload on save」：建議寫明只有 NotEdit 內部存檔會觸發 |
| 103 | External commands「Partial」誇大，實際為 Missing |
| 104 | Installer Partial 沒說明 checksums／簽章完全沒有 |
| 156 | 建議的模組 `function_list`、`completion` 不存在（邏輯在 `src/workspace.rs`、`src/editor.rs`）。可以改寫成「實際模組對照」 |
| — | AI 面板（v0.7）沒列入任何表格 |

其他 Slint 字樣：`src/highlight.rs:154`、`src/main.rs:133,464`、`src/workspace.rs:61`、`README.md:59` 是 `.slint` **語言**支援，不是 UI toolkit 殘留，可以保留。`docs/ACMEUI_PORT_PLAN.md` 是遷移紀錄，也合理。`Cargo.toml`／`build.rs`／`.github/` 裡已經沒有 Slint。

## 4. 測試基線

`cargo test --locked`（Windows，`cc6c407`）：

```
notedit       test result: ok. 68 passed; 0 failed; 2 ignored
notedit_spell test result: ok. 1 passed; 0 failed; 0 ignored
```

2 個 ignored 是 `agent::tests::live_*`（需要 OpenRouter API key 和網路）。cargo test 沒有涵蓋 UI 繪製或事件處理（只有 `ui::keys`、`ui::dispatch`、`ui::timer` 的單元測試），所以本稿的 UI 接線結論來自讀原始碼追蹤，沒有實際操作驗證。
