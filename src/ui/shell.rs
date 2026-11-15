//! M1 spike: an AcmeUI window drawing a monospace grid (ASCII + CJK) and logging input.
//! `notedit --acme-spike [report.txt]`; set `NOTEDIT_SPIKE_AUTOCLOSE=1` to write the
//! report after the first frames and close (used for headless checks).
use acme_core::{Color as AColor, Scene};
use acme_platform::{run, Application, FrameContext, PlatformEvent, WindowConfig, WindowRequest, WindowId};
use acme_text::{FontSystem, GlyphAtlas};
use std::time::Duration;

use super::hit::{Hits, R};
use super::paint::Painter;
use super::text::{Font, TextCache};
use super::types::{hex, ThemeColors};

struct Spike {
    fonts: FontSystem,
    atlas: GlyphAtlas,
    cache: TextCache,
    hits: Hits,
    family: String,
    log: Vec<String>,
    frames: u32,
    report: Option<std::path::PathBuf>,
    autoclose: bool,
    pending: Vec<WindowRequest>,
    mods: (bool, bool, bool),
    icon: String,
}

pub fn run_spike(family: String, report: Option<std::path::PathBuf>) {
    let app = Spike {
        fonts: FontSystem::new(), atlas: GlyphAtlas::default(), cache: TextCache::default(), hits: Hits::default(), family,
        log: vec![], frames: 0, report, autoclose: std::env::var_os("NOTEDIT_SPIKE_AUTOCLOSE").is_some(), pending: vec![], mods: (false, false, false),
        icon: String::new(),
    };
    if let Err(e) = run(app) { eprintln!("acme spike: {e}"); std::process::exit(1); }
}

impl Spike {
    fn note(&mut self, s: String) { eprintln!("{s}"); if self.log.len() < 200 { self.log.push(s); } }
    fn write_report(&mut self) {
        let Some(p) = &self.report else { return };
        let mono = Font::new(&self.family, 15.0, 21.0);
        let cell = self.cache.measure(&mut self.fonts, &"M".repeat(20), &mono, 1.0) / 20.0;
        let cjk = self.cache.measure(&mut self.fonts, &"中".repeat(10), &mono, 1.0) / 10.0;
        let d = self.fonts.diagnostics().clone();
        let s = format!("ACME_SPIKE_OK family={} cell_w={cell:.3} cjk_w={cjk:.3} frames={} system_faces={} fonts_used={} missing={} atlas_full={} icon={}\n{}\n",
            self.family, self.frames, d.system_faces, d.distinct_fonts_used, d.missing_glyphs, d.atlas_full_glyphs, self.icon, self.log.join("\n"));
        let _ = std::fs::write(p, s);
    }
}

impl Application for Spike {
    fn window_config(&self) -> WindowConfig { WindowConfig { title: "NotEdit — AcmeUI spike".into(), width: 900.0, height: 600.0, ..Default::default() } }

    fn event(&mut self, ev: PlatformEvent) -> bool {
        match ev {
            PlatformEvent::Key { key, pressed, shift, ctrl, alt, text, character, repeat, .. } => {
                self.mods = (ctrl, shift, alt);
                if pressed { self.note(format!("key {key:?} text={text:?} char={character:?} c={ctrl} s={shift} a={alt} r={repeat}")); }
                true
            }
            PlatformEvent::PointerButton { pressed, x, y, button, shift, ctrl, alt, .. } => {
                self.mods = (ctrl, shift, alt);
                self.note(format!("button {button} pressed={pressed} at {x:.0},{y:.0} c={ctrl} s={shift} a={alt}"));
                true
            }
            PlatformEvent::Scroll { delta_x, delta_y, lines, .. } => {
                let w = super::win::mods();
                self.note(format!("scroll {delta_x:.1},{delta_y:.1} lines={lines} tracked_mods={:?} win32_mods={w:?}", self.mods));
                false
            }
            PlatformEvent::ImePreedit { text, cursor, .. } => { self.note(format!("preedit {text:?} {cursor:?}")); true }
            PlatformEvent::ImeCommit { text, .. } => { self.note(format!("commit {text:?}")); true }
            PlatformEvent::FileDropped { paths, .. } => { self.note(format!("drop {paths:?}")); false }
            PlatformEvent::WindowCloseRequested(_) => { self.write_report(); false }
            _ => false,
        }
    }

    fn take_window_requests(&mut self, _w: WindowId) -> Vec<WindowRequest> { std::mem::take(&mut self.pending) }
    fn background_poll_interval(&self) -> Option<Duration> { self.autoclose.then_some(Duration::from_millis(250)) }
    fn background_tick(&mut self) -> bool {
        if self.autoclose && self.frames >= 1 { self.write_report(); self.pending.push(WindowRequest::Close); self.autoclose = false; }
        true
    }
    fn on_gpu_recovered(&mut self, _w: WindowId) { self.atlas.clear(); }

    fn frame(&mut self, ctx: FrameContext) -> Scene {
        if self.frames == 0 {
            if let Some(shot) = std::env::var_os("NOTEDIT_SCREENSHOT") {
                // a fresh CPU renderer needs every glyph upload: start from an empty atlas
                self.atlas.clear();
                self.frames = 1;
                let scene = self.paint(&ctx);
                if let Err(e) = super::snapshot::write_png(&scene, ctx.logical_width, ctx.logical_height, ctx.scale_factor, std::path::Path::new(&shot)) { self.note(format!("screenshot failed: {e}")); }
                self.atlas.clear();
                self.frames = 0;
            }
        }
        self.frames += 1;
        if self.frames == 1 {
            self.icon = match super::win::main_hwnd() {
                Some(h) => { let before = super::win::icon_state(h); let set = super::win::set_icon(h); format!("before={before:?},set={set},after={:?}", super::win::icon_state(h)) }
                None => "no-hwnd".into(),
            };
        }
        self.paint(&ctx)
    }
}

impl Spike {
    fn paint(&mut self, ctx: &FrameContext) -> Scene {
        self.atlas.begin_frame();
        self.cache.begin_frame();
        let theme = ThemeColors::default();
        let mut scene = Scene::with_clear(AColor::rgba(1.0, 1.0, 1.0, 1.0));
        let mut p = Painter::new(&mut scene, &mut self.fonts, &mut self.atlas, &mut self.cache, theme, ctx.scale_factor, &mut self.hits);
        let mono = Font::new(&self.family, 15.0, 21.0);
        let cw = p.measure(&"M".repeat(20), &mono) / 20.0;
        p.rect(R::new(0.0, 0.0, ctx.logical_width, 22.0), hex(0xf5f6f7));
        let ui = Font::new("Microsoft JhengHei UI", 13.0, 22.0);
        p.text(6.0, 0.0, "檔案  編輯  搜尋  檢視  — AcmeUI spike", &ui, hex(0x24292f));
        let lines = ["fn main() { let x = 42; }", "中文字對齊測試：一二三四五", "ABCDEFGHIJ0123456789-><=!=", "\t混合 mixed 文字 text"];
        for (i, l) in lines.iter().enumerate() {
            let y = 40.0 + i as f32 * 21.0;
            if i == 1 { p.rect(R::new(0.0, y, ctx.logical_width, 21.0), hex(0xeef4fc)); }
            // draw per char on the cell grid (2 cells for wide chars)
            let mut col = 0usize;
            for c in l.chars() {
                let wide = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) == 2;
                if c != '\t' { p.text(60.0 + col as f32 * cw, y, &c.to_string(), &mono, hex(0x24292f)); }
                col += if c == '\t' { 4 } else if wide { 2 } else { 1 };
            }
            p.text(8.0, y, &format!("{}", i + 1), &mono, hex(0x6e7781));
        }
        for c in 0..=40 { p.vline(60.0 + c as f32 * cw, 36.0, 4.0 * 21.0 + 8.0, hex(0xeeeeee)); }
        p.text(8.0, 140.0, &format!("cell_w={cw:.2}  scale={}  size={}x{}", ctx.scale_factor, ctx.logical_width, ctx.logical_height), &mono, hex(0x0040c0));
        scene
    }
}
