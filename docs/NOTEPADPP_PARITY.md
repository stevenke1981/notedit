# NotEdit → Notepad++ parity roadmap

Baseline date: 2026-10-17  
Reference target: Notepad++ 8.9.8.1 (2026-09-24) and the current Notepad++ User Manual.

NotEdit is intentionally cross-platform and Rust + Slint based. "Parity" therefore means equivalent user capability and reliability, not a pixel-for-pixel clone or compatibility with Notepad++'s Win32/Scintilla internals.

## Current baseline

Already implemented in NotEdit v0.4.0 (see the MadEdit section below for v0.4.0 additions):

- multi-tab editing, drag/drop and tab reorder
- syntax highlighting with syntect
- regex / whole-word / case-sensitive find and replace
- find in all open documents
- encoding and EOL detection/conversion
- session restore and recent files
- undo/redo, line operations, comment toggle
- macro record/playback
- HTML/Markdown preview
- dark theme, fonts, zoom, status bar
- editor and tab context menus
- release builds for Windows, Linux and macOS
- v0.3.0: custom ropey-based editor surface (`src/editor.rs` headless core + `ui/editor.slint`): wrap with highlighting, folding, split/clone view, multi-caret + column selection, autocomplete, bookmarks, document map, macro recording of shortcuts and menu actions

## MadEdit features (v0.4.0)

Sources: [MadEdit on SourceForge](https://sourceforge.net/projects/madedit/) ("Text/Column/Hex modes, ... WordCount"), the [MadEdit-Mod README](https://github.com/LiMinggang/MadEdit-Mod) feature list, and the menu strings in `src/MadEditFrame.cpp` of MadEdit-Mod.

| MadEdit feature | NotEdit v0.4.0 | Notes |
|---|---|---|
| Hex mode (edit, insert) | Implemented (`src/hex.rs`, `src/hexview.rs`) | offset / hex / ASCII columns, overwrite + insert, undo/redo, hex-string copy/paste |
| Large files | Implemented | binary → hex; > 512 MB → mmap hex (overwrite only); 140 MB text opens in ~1 s |
| Column mode | Implemented | Alt+C toggle |
| Text / Column / Hex mode switch | Implemented | View menu radio items |
| Chinese conversion (Trad ↔ Simp) | Implemented (ferrous-opencc s2twp / tw2sp) | file/selection; MadEdit's clipboard variants and Kanji conversion not added |
| To fullwidth / halfwidth | Implemented | |
| Space ↔ Tab | Implemented | tab width 4 |
| Word count (CJK aware) | Implemented | |
| Find / Replace in Files | Find implemented; Replace in Files missing | |
| Search in all open documents | Implemented (since v0.2) | |
| Spell check (suggestions, ignore, dictionary) | Implemented for en_US, text/Markdown only | no personal-dictionary editor |
| Copy as hex string / revert hex string | Implemented | |
| Insert incremental numbers, column align | Implemented | |
| Join lines, delete empty lines, trim leading/trailing | Implemented | |
| Capitalize, invert case | Implemented | |
| Bookmarks | Implemented; copy/cut/delete bookmarked lines missing | |
| Highlight word | Implemented (mark-all on selection) | |
| Markdown / HTML preview | Implemented (since v0.1) | |
| Typewriter mode, auto backup, Python macros, JSON/XML/Astyle formatters, Pan-Chinese search | Missing | |

## Priority matrix

| Priority | Capability | Current state | Target acceptance criteria |
|---|---|---|---|
| P0 | Durable saves | Implemented (#1) | Never truncate the destination before a complete staged write; preserve existing permissions; tests cover create/replace |
| P0 | Crash/session recovery | Implemented (#3) | Recover modified named and unnamed buffers after abnormal exit without overwriting source files |
| P0 | External file-change detection | Implemented (#4) | Detect changed/deleted files, offer reload/keep choices, and surface recovery-vs-disk conflicts |
| P0 | Lossless legacy-encoding saves | Implemented (#6) | Refuse writes that would replace unrepresentable characters; regression tests cover legacy and Unicode encodings |
| P0 | Large-file behavior | Implemented (v0.3.0) | ropey buffer + custom virtualized editor surface; incremental, time-budgeted highlighting |
| P1 | Find in Files | Implemented (v0.4.0) | Folder scope, filters, regex, recursive mode, result panel, click-to-open |
| P1 | Folder as Workspace | Missing | Tree panel, refresh, open/rename/create/delete with safe confirmation |
| P1 | Function List | Missing | Language-aware symbol extraction, filter, click-to-navigate |
| P1 | Auto-completion | Implemented (v0.3.0) | Word completion plus language keywords; keyboard navigation; configurable enable/disable |
| P1 | Bookmarks + search marks | Partial (v0.3.0: bookmarks, mark-all occurrences of selected word) | Toggle/navigate bookmarks and mark-all search hits |
| P2 | Dual/split view | Partial (v0.3.0: clone into second view; move-to-other-view missing) | Move or clone a document into a second synchronized editor view |
| P2 | Multi-selection / multi-caret | Implemented (v0.3.0) | Multiple carets and simultaneous edits with correct undo grouping |
| P2 | Column mode | Implemented (v0.4.0: column-mode toggle Alt+C, Alt+drag, Alt+Shift+arrows, incremental numbering) | Rectangular selection/edit with status reporting |
| P2 | Code folding | Implemented (v0.3.0: bracket/indent based) | Fold/unfold scopes with visible gutter controls and preserved navigation |
| P2 | Document Map | Implemented (v0.3.0) | Minimap/document overview with viewport indicator |
| P2 | Tab management parity | Partial | pin tabs, sort tabs, close unchanged/left/right, MRU switcher |
| P3 | User Defined Languages | Missing | User-editable syntax definitions independent of built-in syntect definitions |
| P3 | Plugin API | Missing | Versioned, permission-aware extension boundary with failure isolation |
| P3 | External commands | Partial | Configurable commands, variables for file/path/selection, captured output |
| P4 | Installer/update | Partial | signed/versioned artifacts; update strategy documented; checksums published |
| P4 | Accessibility/i18n | Partial | keyboard-complete UI, scalable controls, localization layer |
| P4 | Security hardening | Partial | path normalization, untrusted-session handling, external-command boundaries, dependency review |

## Delivery phases

### Phase A — Reliability foundation (v0.2.x)

1. Durable staged writes for documents and settings.
2. Crash-recovery journal for modified buffers.
3. External file modification detection.
4. Tests for encoding/EOL round trips, save failure behavior, recovery, and session paths.
5. Make failures visible instead of silently swallowing persistent-setting errors.

### Phase B — Navigation and project editing (v0.3)

1. Find in Files.
2. Folder as Workspace.
3. Function List.
4. Bookmarks and search marks.
5. MRU document switcher and expanded tab management.

These features deliver the highest everyday value without requiring a new text rendering engine.

### Phase C — Editing engine parity (v0.4)

1. Auto-completion.
2. Dual/split view and clone-to-other-view.
3. Multi-selection / multi-caret.
4. Column mode.
5. Code folding.
6. Document Map.

**Architecture gate:** true multi-caret, column mode, folding, and synchronized split views must not be simulated with brittle string rewrites. If Slint's TextInput API cannot expose the required primitives, introduce an `EditorCore` document model and a custom editor surface (or a proven editor backend) before implementing these features.

### Phase D — Extensibility (v0.5+)

1. User Defined Language format.
2. Stable plugin/extension API.
3. Configurable external commands.
4. Plugin process isolation or WASM-style sandboxing for untrusted extensions.
5. Extension capability manifest and API version negotiation.

## Engineering rules

- Preserve text bytes/encoding/EOL intentionally; no silent lossy conversion on save.
- Never overwrite a user's source file merely to recover a session.
- All path data loaded from settings/session files is untrusted input.
- New editor operations must be undoable as one logical transaction.
- Heavy search/highlight/workspace scans must be cancellable or incremental and must not block the UI thread.
- Avoid adding a feature only at the menu level; every shipped feature needs behavior tests.
- Keep platform-specific behavior behind small modules rather than spreading `cfg` branches throughout `main.rs`.
- Prefer small modules (`file_io`, `workspace`, `recovery`, `function_list`, `completion`) over further growth of the current monolithic `main.rs`.

## Test gates

For every PR:

- `cargo test` on Windows, Linux and macOS
- release build on all three platforms
- save/open regression tests for UTF-8/BOM, UTF-16 LE/BE, Big5, Shift-JIS and CRLF/LF/CR
- no data-loss regression when save fails midway
- UI smoke test checklist for tabs, find/replace, session restore and close-with-unsaved-changes

For editor-core features (Phase C), add deterministic unit tests independent of Slint rendering so selection, caret, folding and undo behavior can be tested headlessly.

## Reference sources

- Notepad++ User Manual: https://npp-user-manual.org/
- Notepad++ User Interface: https://npp-user-manual.org/docs/user-interface/
- Notepad++ repository/changelog: https://github.com/notepad-plus-plus/notepad-plus-plus/wiki/Changes
- Notepad++ releases: https://github.com/notepad-plus-plus/notepad-plus-plus/releases
