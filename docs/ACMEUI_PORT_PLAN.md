# ACMEUI_PORT_PLAN.md — notedit: Slint → AcmeUI-Native

Author: adviser (Fable 5.2), from reading notedit @ 5572eaf and AcmeUI-Native @ a3e7588. Implementer follows this literally.

## 0. Findings that shape the plan
- AcmeUI is **not** a turnkey widget runtime: `WidgetNode` gives layout only; painting, hit-testing and click routing are application-defined (`F:\AcmeUI-Native\examples\sdk-consumer\src\main.rs` own `Painter`; `docs\guides\SDK_GUIDE.md` §8). The gallery carries ~2,000 lines of its own render/event code.
- Use the low-level path: `acme_platform::Application` → ordered `acme_core::Scene` (`DrawCommand::{Quad,Text,PushClip,PopClip}`) → `acme_text::FontSystem` + `GlyphAtlas`. Hand-computed rects for notedit's fixed-height chrome; `acme-textinput` only for single-line fields.
- Do NOT use `Frame`/`scene_from_frame`: it emits all quads then all text (`crates\acme-render-wgpu\src\batch.rs:257`), so popups/dialogs would sit under editor text.
- Keep the Rust call sites: ~150 `ui.set_*/get_*` calls stay, backed by a plain-Rust `AppWindow` shim with the same method names. `App` stays `Rc<App>` with `&self` methods.

## 1. Inventory (Slint surface → Rust touch points)
| Surface | Slint | Rust touch points |
|---|---|---|
| Window/title/icon/close | `AppWindow` `ui/app.slint:52` | `main.rs:607` `AppWindow::new`, `:891` `on_close_requested`, `:960` `ui.run()`; `app_actions.rs` `set_window_title`; `app_commands.rs:273` `quit_event_loop` |
| Menu bar + dropdown | `app.slint:259`, `:561-582`, `open-menu` | `main.rs:247-349` `build_menus` (`Menu`,`MenuItem`,`ModelRc`); `app_actions.rs:142` `set_menus`; `main.rs:638` `on_menu_action` |
| Toolbar (20 SVG icons, tooltip) | `app.slint:274-307,:555` | → `menu-action` only |
| Tabs (select/close/drag-reorder/right-click) | `app.slint:309-347` | `app_actions.rs:138-140` `get_tabs`/`set_tabs`/`set_current_tab`; `main.rs:355` `tab_width`; `on_tab_selected/close/move/target`, `on_context_request` |
| Editor canvas ×2 | `ui/editor.slint` | `editor_render.rs:11-260` (`EdState/EdRow/EdTok/EdRect/EdCaret`, `set_v0/v1`); `hexview.rs:61-110`; `render_model.rs` |
| Editor input | `ed-key/ed-input/ed-mouse/ed-gutter/ed-wheel/ed-focused` | `main.rs:770-812`; `editor_actions.rs` `ed_key:517`, `ed_wheel:392`, `ed_mouse`, `ed_gutter` |
| Gutter | `editor.slint:79-101` | `EdRow.{num,mark,fold,cur}` |
| Scrollbars | `editor.slint:191-209` | `EdState.{thumb_*,hthumb_*,hbar}`; `ed_vscroll/ed_hscroll` |
| Doc map | `editor.slint:212-224` | `MapLine` (`editor_render.rs:216-245`); `ed_map`; `set_docmap` |
| Autocomplete | `editor.slint:174-188` | `EdState.ac_*`; `on_ed_ac_pick` |
| Metrics | hidden metric texts; `cell-w,line-h,text-dy,cjk-w,v0-w/h` | `editor_actions.rs:141-149` `metrics/view_size`; `editor_render.rs:19` `get_cjk_w`; `on_layout_changed`→`kick` |
| IME | hidden TextInput at caret, preedit box | `ed_input` via `edited`; keys rejected during preedit |
| Caret blink/idle | native cursor, `caret-idle` | `editor_render.rs:15-17` idle timer |
| Sidebar | `app.slint:352-386` ListView | `panels.rs:149-151` side_items/sel/title/mode; `on_side_click/activate/switch/open_folder` |
| Find/replace, FIF, goto, prompt, rename, about | `Dialog` `app.slint:585-717` | `app_commands.rs` (find_text, match_case, whole_word, use_regex, find_status, *_visible, rename_text); `madops.rs` (fif_*, prompt_*); `main.rs:793-822` |
| Search hits | `app.slint:516-536` | `app_commands.rs:47`, `madops.rs:240,:424-426`; `on_hit_clicked` |
| Status bar | `app.slint:537-551` | `set_st_type/length/pos/eol/encoding`, `set_overwrite`, `set_macro_recording` |
| Context menu | `app.slint:687-707` | `app_commands.rs:154` `set_ctx_items/visible` |
| Preview | `app.slint:488-513` (`Block`) | `app_actions.rs:241` preview_blocks/title/show_preview; `preview.rs`; debounce `editor_actions.rs:63` |
| AI sidebar | `app.slint:422-486` | `main.rs:646-769` `on_ai_*`, ~40 `get/set_ai_*`, `upgrade_in_event_loop:708` |
| Theme | `global Theme` | `panels.rs:59-76`; `panels.rs:17` `parse_color` |
| Color | `slint::Color` | `highlight.rs:3`, `editor_render.rs`, `hexview.rs`, `panels.rs` |
| Timers | — | fields `ticker, preview_timer, idle` (`main.rs:369-372`); recovery 10s, file-watch 2s, smoke timers (`main.rs:883-895`); `Timer::single_shot` in app_commands:328, editor_actions:547/:967, hexview:141, madops:375, panels:83/:238, main:643/:868 |
| Clipboard | — | `arboard` direct — unchanged |
| Drag & drop | winit `DroppedFile` | `main.rs:860-875` |
| File dialogs | — | `dlg.rs` rfd parented via `slint::WindowHandle` |
| Single instance | — | `main.rs:845-853` `shell::listen`→`invoke_from_event_loop`→minimize/focus |
| Smoke test | — | `main.rs:892-959` |
| Fonts | bundled ttf | `main.rs:254-266` `BUNDLED_FONTS`, `detect_fonts` |

## 2. Mapping to AcmeUI
| Need | AcmeUI |
|---|---|
| Window/loop | `acme_platform::{run, Application, WindowConfig, FrameContext}` (`crates\acme-platform\src\lib.rs:47,:272,:392`); title `WindowRequest::SetTitle` (`waker.rs:115`) |
| Close confirm | `Application::close_requested` → `app.confirm_exit()` (`lib.rs:353`); exit `WindowRequest::Close` |
| Bring to front | `WindowRequest::Focus` |
| Cross-thread | `Application::on_waker`/`wake` + `Waker::wake()` (`waker.rs:8`) |
| Timers | `background_poll_interval`/`background_tick` (`lib.rs:361-370`, `scheduling.rs:33`) |
| Keys | `PlatformEvent::Key{key,text,character,ctrl,shift,alt,repeat}` (`keys.rs:4`) |
| Mouse/wheel | `PointerMoved/PointerButton/Scroll/PointerLeft`; line scroll pre-multiplied by 32px (`lib.rs:1623`); captured drags report outside coords |
| IME | `ImePreedit/ImeCommit/ImeEnabled/ImeDisabled`, `ime_cursor_area`, `WindowRequest::SetImeAllowed` |
| Drop | `PlatformEvent::FileDropped{paths}` |
| Cursor | `WindowRequest::SetCursor(CursorIcon::…)` |
| Draw | `Scene::with_clear`, `Scene::push(DrawCommand::Quad(QuadPrimitive{rect,color,radius,border_width,border_color}))`, `PushClip/PopClip` (`crates\acme-core\src\scene.rs:20,:190,:388`) |
| Text | `FontSystem::shape(text,&TextStyle{..},TextConstraints,scale)`→`prepare(&layout,&mut atlas)`→`DrawCommand::Text(TextPrimitive{origin=baseline,color,glyphs,uploads})`; copy recipe from `batch.rs:343-385` |
| Atlas | `atlas.begin_frame()` each frame; `atlas.clear()` in `on_gpu_recovered` (`apps\gallery\src\main.rs:490,:500`) |
| Single-line fields | `acme_textinput::{TextInputState, handle_key, handle_keyboard_shortcut, handle_text, scroll_caret_into_view}`; `password: bool` |
| Clipboard (fields) | `acme_platform::Clipboard` |
| SW fallback | automatic (`cpu-renderer` default); `ACME_RENDER_BACKEND=cpu` |

### Gaps → workarounds
- **G1 no custom font loading** (system fonts only): `BUNDLED_FONTS=&[]`, default family = first installed `SYSTEM_MONO_FONTS` (Cascadia Mono/Consolas) else `"monospace"`. Keep `ui/fonts/`. Follow-up upstream: `FontSystem::load_font_data`.
- **G2 no widget paint/event runtime**: small immediate-mode kit `src/ui/paint.rs` + `src/ui/hit.rs` (hit list rebuilt each frame).
- **G3 no multi-line edit**: AI history/result = read-only wrapped text in clipped wheel-scrolled box; prompt = single-line field. Editable AI result dropped; document in README.
- **G4 no ComboBox/CheckBox/Button/ListView**: hand-paint. Combo = button opening the context-menu popup; lists = fixed 20px rows, own scroll, clip, visible rows only.
- **G5 no SVG/image**: toolbar = short text/Unicode glyph labels, same tooltips.
- **G6 no window icon API**: exe icon via winresource still applies; verify title-bar icon at M1, else `WM_SETICON` via G7 HWND.
- **G7 no raw window handle**: Windows `GetActiveWindow()` on UI thread at first frame → `Win32WindowHandle` wrapper implementing `HasWindowHandle+HasDisplayHandle` → rfd `set_parent`. Other OS: unparented.
- **G8 no shadow**: 1px border (+ optional 2px translucent offset quad).
- **G9 Scroll has no modifiers**: track `(ctrl,shift,alt)` from Key/PointerButton; Windows fallback `GetKeyState(VK_CONTROL)` for Ctrl+wheel.
- **G10 no ligature toggle**: keep existing `lig_break` run splitting.
- **G11 no double-click event**: 400ms same-row check for sidebar activation (editor already times clicks).
- **G12 注音 IME not validated upstream**: on manual smoke list.

## 3. Target architecture — new `src/ui/` (only place importing `acme_*`)
- `types.rs`: plain structs replacing Slint-generated ones, same field names: `EdTok, EdRect, EdRow, EdCaret, MapLine, EdState, SearchHit, MenuItem, Menu, TabInfo, SideItem, Block`. `SharedString`→`String`, models→`Vec<T>`, length→`f32`. `Color{r,g,b,a:u8}` Copy+PartialEq+Default with `from_rgb_u8`, `from_argb_u8`, `to_acme()`. `ThemeColors` with the 18 Theme keys.
- `state.rs`: `pub struct AppWindow(Rc<UiInner>)` Clone; `props!` macro generates `set_x/get_x` over Cell/RefCell for every property in `app.slint:58-207`, each setter flags redraw. Plus `invoke_focus_editor()`, `theme()`, and shell-written out-values `cell_w, line_h, text_dy, cjk_w, v0_w/h, v1_w/h`. Field-backed props (find_text, replace_text, fif_dir, fif_filter, prompt_text, rename_text, ai_key, ai_model, ai_prompt, ai_language_custom, goto) read/write a `TextInputState.text`. `App.ui` becomes `AppWindow`; `fn ui()` returns clone.
- `timer.rs`: `Timer`, `TimerMode::{SingleShot,Repeated}`, `start/stop/running/single_shot/Default`; thread-local registry; `next_interval()` → None if nothing armed else bucket (10/50/250ms) ≤ time to earliest deadline (buckets matter: `BackgroundPoll::advance` re-arms when interval value changes); `fire_due()->bool` collects due callbacks then runs them outside the borrow.
- `dispatch.rs`: `invoke_from_event_loop(f: impl FnOnce()+Send+'static)` via `static QUEUE: Mutex<Vec<Box<dyn FnOnce()+Send>>>` + `static WAKER: OnceLock<Waker>`. Replaces `slint::invoke_from_event_loop` and `upgrade_in_event_loop`; AI closure fetches app from an `APP` thread-local.
- `keys.rs`: `slint_text(key, character, text) -> Option<String>` keeping the existing key vocabulary (so `ed_key`, `hex_key`, `combo()`, recorded macros stay valid): Tab `\t`, Enter `\n`, Esc `\u{1b}`, Backspace `\u{8}`, Delete `\u{7f}`, Space `" "`, arrows U+F700–F703 (up,down,left,right), `Function(n)`→U+F704+n−1, Insert U+F727, Home U+F729, End U+F72B, PageUp U+F72C, PageDown U+F72D, Other→`character`.
- `text.rs`: `TextCache` `HashMap<(String,family,size_bits,bold,italic,scale_bits),(TextLayout,last_frame)>` evict >120 frames unused; `draw(scene,text,style,x,y_top,color,clip)`, `measure`. Mono metrics: `cell_w = measure("M"×20)/20`, `cjk_w = measure("中"×10)/10`, `line_h`/`text_dy` exactly as `app.slint:147-148`.
- `paint.rs`: `Painter{scene,fonts,atlas,cache,theme,scale,hits}` with `rect, border, text, text_centered, text_elided, clip(rect,|p|..), button, checkbox, field(&mut TextInputState, rect, focused)`.
- `hit.rs`: `enum Target` pushed in paint order, hit-tested in reverse: `MenuTitle(i), MenuItem(String), CtxItem(String), Tool(&'static str), Tab(i), TabClose(i), Editor(vi), Gutter(vi), VBar(vi), HBar(vi), Map(vi), AcItem(vi,i), SideRow(i), SideTab(m), Hit(i), Field(FieldId), Check(CheckId), Button(BtnId), Backdrop, DialogClose(DlgId)`, …
- `view_*.rs`: `view_chrome` (menu, toolbar, tabs, status, tooltip), `view_editor`, `view_side`, `view_dialogs`, `view_preview`, `view_ai`, `view_popup` (dropdown, context menu, combo).
- `shell.rs`: `struct Shell{app:Rc<App>,fonts,atlas,cache,hits,focus,mods,pointer,capture:Option<Target>,pending:Vec<WindowRequest>,blink_on,preedit,size,scale,quitting}` + `impl Application for Shell`.

### Frame / event flow
- `frame(ctx)`: `atlas.begin_frame()`; compute region rects; write metrics + `v*_w/h` into AppWindow, if changed `app.kick()` (replaces layout-changed); paint z-order chrome→sidebar→editors→AI→preview→hits→status→tooltip→dialogs→menu/ctx popup; record hits; return scene.
- Editor paint consumes `ui.get_v0()/get_v1()` unchanged: row bg (diff/current line) → `rects` by kind (colours `editor.slint:110-111`) → tokens at `(text_x+tok.x, row_y+text_dy)` clipped → fold underline → carets (primary blinks, others static, overwrite block) → preedit at primary caret underlined → AC popup. Gutter/scrollbars/map per `editor.slint:79-224`. `ime_cursor_area` = primary caret rect.
- `event(ev)->bool` returns `ui.take_dirty()`:
  - Key pressed: ignore if preedit non-empty & focus editor. Popup open: Esc closes. Field focused: `acme_textinput::handle_key/handle_keyboard_shortcut/handle_text`; Enter → field accept; Esc → close dialog. Else `t=slint_text(..)` → `app.ed_key(vi,&t,c,s,a)` (runs `custom_key`) → global table ported from `app.slint:222-254` into `fn global_key` → `app.ed_input(vi,text)` if no ctrl/alt and printable.
  - ImePreedit stores; ImeCommit → `ed_input` or field `handle_text`.
  - PointerButton/Moved: hit-test, call App methods directly: `ed_mouse(vi,kind,x-text_x,y-text_y,c,s,a)` kinds 0 down,1 move,2 up,3 right-down; `ed_gutter, ed_vscroll, ed_hscroll, ed_map, select_tab, close, tab_target, move_tab, context_menu, side_click, side_activate, hit_clicked`. `capture` keeps drags routed to pressed target. Buttons 0 left,1 right,2 middle.
  - Scroll → `ed_wheel(vi,dx,dy,shift,ctrl)` or list/preview offsets. FileDropped → `open_path` on 0ms timer. Resized → `app.kick()`.
  - Menu actions keep "close popup, run after 30ms" (`main.rs:638`) so rfd modals never run inside `event()`.
- `background_poll_interval`→`timer::next_interval()`; `background_tick`→`timer::fire_due() | ui.take_dirty()`; `wake`→drain dispatch queue, true; `take_window_requests`→drain pending (SetTitle, SetImeAllowed(true) once, SetCursor, Focus, Close); `close_requested`→`quitting || app.confirm_exit()`. Caret blink: 500ms repeating timer toggles `blink_on` while editor focused and !caret_idle.
- De-slint non-UI modules: `highlight.rs`, `panels.rs::parse_color`, `hexview.rs`, `editor_render.rs` use `crate::ui::Color`. Delete `render_model.rs` (update/update_rows → plain assignment). `dlg.rs` uses G7 handle.

## 4. Cargo / dependencies
Remove `slint`, `i-slint-backend-winit`, `winit`, build-dep `slint-build`. Add (git, pinned):
```toml
acme-core      = { git = "https://github.com/stevenke1981/AcmeUI-Native", rev = "a3e758852b77c7e94c344a267c55d7cb7441c965" }
acme-platform  = { git = "https://github.com/stevenke1981/AcmeUI-Native", rev = "a3e758852b77c7e94c344a267c55d7cb7441c965" }
acme-text      = { git = "https://github.com/stevenke1981/AcmeUI-Native", rev = "a3e758852b77c7e94c344a267c55d7cb7441c965" }
acme-textinput = { git = "https://github.com/stevenke1981/AcmeUI-Native", rev = "a3e758852b77c7e94c344a267c55d7cb7441c965" }
```
- Not needed directly: acme-ui, acme-widgets, acme-layout, acme-render-wgpu. Prefer a local field painter over `render_text_input` (avoids acme-theme).
- Editions: AcmeUI edition 2024 / MSRV 1.88; notedit stays 2021 / 1.92 — fine.
- Only AcmeUI's winit 0.30 remains. Bump `fontdb` 0.22→0.23 to dedupe with cosmic-text 0.14. Keep arboard, raw-window-handle 0.6, rfd.
- windows-sys: add `Win32_UI_Input_KeyboardAndMouse`, `Win32_UI_WindowsAndMessaging`.
- `build.rs`: delete `slint_build::compile`; keep winresource block; `ui/icon.ico` stays.
- Regenerate & commit `Cargo.lock` (CI uses `--locked`, `.github/workflows/build.yml:39-41`). Git-URL dep form unverified upstream — first CI run is the check.
- Final cleanup: delete `ui/app.slint`, `ui/editor.slint`, `ui/icons/*.svg`.

## 5. Sequence (each milestone compiles + tests pass)
Single primary implementer, serial (`main.rs`, `Cargo.toml`, `src/ui/state.rs` touched by everything).
- **M0** baseline `cargo test --workspace` recorded.
- **M1 spike**: add deps alongside Slint; `src/ui/{types,text,paint,shell}.rs` behind `--acme-spike` flag drawing a mono grid + logging keys. Proves git dep, release build, G6, G9, CJK shaping. Stop & report if release build fails.
- **M2 de-slint logic while still on Slint**: `ui::Color` in highlight/panels/hexview/editor_render (convert at boundary); add `ui::timer`, `ui::dispatch` (slint::Timer still used).
- **M3 the flip**: `ui/state.rs` + plain types; remove `slint::include_modules!`; switch App.ui, timers, dispatch; delete render_model.rs; rewrite `main()` (`cb!` blocks → `impl App` methods: ai_send, ai_apply, ai_save_key, goto_line, count_matches, …) → `acme_platform::run(Shell::new(app))`; implement view_editor, view_chrome, view_popup + key/mouse/wheel/IME routing; dialogs/sidebar/preview/hits/AI may be stubs but compile; remove Slint deps + build.rs line. Exit: open, edit, save, tabs, menus, shortcuts work.
- **M4 dialogs**: find/replace, FIF, goto, prompt, rename, about; fields with focus & Tab order; dlg.rs parent handle.
- **M5**: sidebar, hits panel, preview, doc map, AC popup, tooltip, tab drag-reorder, split title.
- **M6**: AI sidebar (G3, G4).
- **M7**: rewrite `--smoke-test` to call `app.ed_mouse/ed_input` directly and read `ui.get_v0().rows.len()`; drop model-identity asserts; keep `NOTEDIT_SMOKE_OK`, `INPUT_CALLBACKS_OK`, `SPELL_MODULE_OK`, `PREVIEW_DEBOUNCE_OK` lines; delete `.slint` files; README/About "Rust + AcmeUI".

Parallelism only after M3: M4 / M5 / M6 are each a new `src/ui/view_*.rs` filling a stub; `shell.rs, state.rs, hit.rs, main.rs, Cargo.toml` single-writer. If in doubt, stay serial.

## 6. Risks & acceptance
Risks: text perf (needs TextCache; join adjacent same-style ASCII tokens; measure 200×60); atlas pressure (watch `FontSystem::diagnostics().atlas_full_glyphs`); grid alignment with CJK fallback font; re-entrancy (rfd modals only via timer; never hold `st.borrow_mut()` across a timer fire); unverified G6/G9/G12; idle wakeups ~4/s (no redraw unless dirty); CI runners rely on CPU fallback, macOS untested upstream; fat LTO + wgpu = long build/larger binary. Accepted regressions: bundled fonts (G1), SVG icons (G5), editable AI result & multi-line prompt (G3), shadows (G8).

Acceptance:
1. `grep -ri slint Cargo.toml build.rs src/` → only `"slint"` language-table strings.
2. `cargo build --release --locked` (Windows).
3. `cargo test --workspace --release --locked`.
4. `target\release\notedit.exe --smoke-test %TEMP%\s.txt src\workspace.rs` → `NOTEDIT_SMOKE_OK`.
5. Same with `ACME_RENDER_BACKEND=cpu`.
6. GitHub CI green on all OSes.
7. Manual Windows smoke: ASCII + 注音 中文 (candidate at caret, inline preedit); Backspace/Enter/Tab/arrows/Home/End/PgUp/PgDn, Shift-select, Alt+Shift column select, multi-caret, Insert overwrite, undo/redo, cut/copy/paste; drag-select past edge, double/triple click, wheel, Ctrl+wheel zoom, scrollbars, doc map; fold/bookmark gutter; every menu + actions; toolbar + tooltip; tabs select/close/drag/right-click/rename; split F8; find/replace/count/find-all/FIF run-stop-replace-undo/goto; sidebar tree + function list double-click; markdown preview debounce; hex Alt+H; dark theme, font, zoom; drag-drop file; second instance forwards+focuses; close-with-unsaved prompt; recovery prompt; resize/DPI; AI send/apply/save key.

Critical files: `src/main.rs`, `src/editor_render.rs`, `ui/app.slint` + `ui/editor.slint` (spec for hand-painted views), `F:\AcmeUI-Native\crates\acme-platform\src\lib.rs`, `F:\AcmeUI-Native\crates\acme-render-wgpu\src\batch.rs`, `F:\AcmeUI-Native\crates\acme-core\src\scene.rs`.
