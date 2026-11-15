//! Tiny immediate-mode painting kit: quads, text and hit targets in one ordered scene.
use acme_core::{DrawCommand, QuadPrimitive, Rect, Scene};
use acme_text::{FontSystem, GlyphAtlas};

use super::hit::{Hits, Target, R};
use super::text::{Font, TextCache};
use super::types::{Color, ThemeColors};

pub struct Painter<'a> {
    pub scene: &'a mut Scene,
    pub fonts: &'a mut FontSystem,
    pub atlas: &'a mut GlyphAtlas,
    pub cache: &'a mut TextCache,
    pub theme: ThemeColors,
    pub scale: f32,
    pub hits: &'a mut Hits,
    clips: Vec<R>,
}

impl<'a> Painter<'a> {
    pub fn new(scene: &'a mut Scene, fonts: &'a mut FontSystem, atlas: &'a mut GlyphAtlas, cache: &'a mut TextCache,
               theme: ThemeColors, scale: f32, hits: &'a mut Hits) -> Self {
        Painter { scene, fonts, atlas, cache, theme, scale, hits, clips: Vec::new() }
    }

    fn quad(&mut self, r: R, color: Color, radius: f32, bw: f32, bc: Color) {
        if r.w <= 0.0 || r.h <= 0.0 || (color.a == 0 && (bw <= 0.0 || bc.a == 0)) { return; }
        self.scene.push(DrawCommand::Quad(QuadPrimitive {
            rect: Rect::new(r.x, r.y, r.w, r.h), color: color.to_acme(), radius, border_width: bw, border_color: bc.to_acme(),
        }));
    }

    pub fn rect(&mut self, r: R, color: Color) { self.quad(r, color, 0.0, 0.0, Color::TRANSPARENT); }
    pub fn round(&mut self, r: R, color: Color, radius: f32) { self.quad(r, color, radius, 0.0, Color::TRANSPARENT); }
    /// filled rect with a border (fill may be transparent)
    pub fn border(&mut self, r: R, fill: Color, width: f32, color: Color) { self.quad(r, fill, 0.0, width, color); }
    pub fn border_r(&mut self, r: R, fill: Color, width: f32, color: Color, radius: f32) { self.quad(r, fill, radius, width, color); }
    /// G8: no drop shadows upstream — a soft offset quad under popups instead
    pub fn shadow(&mut self, r: R) { self.rect(R::new(r.x + 2.0, r.y + 2.0, r.w, r.h), Color::from_argb_u8(0x30, 0, 0, 0)); }

    pub fn hit(&mut self, r: R, t: Target) {
        let r = match self.clips.last() { Some(c) => r.intersect(c), None => r };
        self.hits.push(r, t);
    }

    /// Run `f` with drawing (and hit targets) clipped to `r`.
    pub fn clip(&mut self, r: R, f: impl FnOnce(&mut Self)) {
        let r = match self.clips.last() { Some(c) => r.intersect(c), None => r };
        self.scene.push(DrawCommand::PushClip(Rect::new(r.x, r.y, r.w, r.h)));
        self.clips.push(r);
        f(self);
        self.clips.pop();
        self.scene.push(DrawCommand::PopClip);
    }

    pub fn measure(&mut self, s: &str, font: &Font) -> f32 { self.cache.measure(self.fonts, s, font, self.scale) }

    /// Draw `s` with the top of its line box (height `font.line_h`) at `y`. Returns the width.
    pub fn text(&mut self, x: f32, y: f32, s: &str, font: &Font, color: Color) -> f32 {
        self.cache.draw(self.scene, self.fonts, self.atlas, s, font, x, y, color, self.scale, None).0
    }

    /// word-wrapped text; returns (width, height)
    pub fn text_wrapped(&mut self, x: f32, y: f32, w: f32, s: &str, font: &Font, color: Color) -> (f32, f32) {
        self.cache.draw(self.scene, self.fonts, self.atlas, s, font, x, y, color, self.scale, Some(w))
    }

    pub fn wrapped_height(&mut self, w: f32, s: &str, font: &Font) -> f32 {
        if s.is_empty() { return font.line_h; }
        self.cache.layout(self.fonts, s, font, Some(w), self.scale).height
    }

    pub fn text_centered(&mut self, r: R, s: &str, font: &Font, color: Color) {
        let w = self.measure(s, font);
        let y = r.y + ((r.h - font.line_h) / 2.0).round();
        self.text(r.x + ((r.w - w) / 2.0).max(0.0), y, s, font, color);
    }

    /// Single line, vertically centred in `r`, elided with "…" when wider than `r.w`.
    pub fn text_elided(&mut self, r: R, s: &str, font: &Font, color: Color) {
        let y = r.y + ((r.h - font.line_h) / 2.0).round();
        let w = self.measure(s, font);
        if w <= r.w { self.text(r.x, y, s, font, color); return; }
        let chars: Vec<char> = s.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            let t: String = chars[..mid].iter().collect::<String>() + "…";
            if self.measure(&t, font) <= r.w { lo = mid; } else { hi = mid - 1; }
        }
        let t: String = chars[..lo].iter().collect::<String>() + "…";
        self.text(r.x, y, &t, font, color);
    }
}
