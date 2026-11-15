//! Shaped-text cache on top of `acme_text`: layouts are reused across frames
//! (keyed by text + style + DPI) and evicted after 120 unused frames.
use acme_core::{AtlasUpload, DrawCommand, GlyphDraw, GlyphFormat, Point, Scene, TextPrimitive};
use acme_text::{AtlasFormat, FontSystem, GlyphAtlas, TextConstraints, TextLayout, TextStyle, TextWrap};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::types::Color;

/// Font selection for one text run. `line_h` is the height of the line box the
/// text is vertically centred in (cosmic-text centres the glyphs in it).
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub family: String,
    pub size: f32,
    pub line_h: f32,
    pub bold: bool,
    pub italic: bool,
}

impl Font {
    pub fn new(family: &str, size: f32, line_h: f32) -> Self { Font { family: family.into(), size, line_h, bold: false, italic: false } }
    pub fn bold(mut self, b: bool) -> Self { self.bold = b; self }
    pub fn italic(mut self, i: bool) -> Self { self.italic = i; self }
    fn style(&self) -> TextStyle {
        TextStyle { font_size: self.size, line_height: self.line_h, family: self.family.clone(), bold: self.bold, italic: self.italic }
    }
}

const EVICT_AFTER: u64 = 120;

struct Entry { text: String, font: Font, wrap: Option<u32>, scale: u32, layout: TextLayout, used: u64 }

#[derive(Default)]
pub struct TextCache {
    map: HashMap<u64, Entry>,
    frame: u64,
}

fn key(text: &str, f: &Font, wrap: Option<u32>, scale: u32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    f.family.hash(&mut h);
    f.size.to_bits().hash(&mut h);
    f.line_h.to_bits().hash(&mut h);
    f.bold.hash(&mut h);
    f.italic.hash(&mut h);
    wrap.hash(&mut h);
    scale.hash(&mut h);
    h.finish()
}

impl TextCache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        let f = self.frame;
        if f % 30 == 0 { self.map.retain(|_, e| f - e.used <= EVICT_AFTER); }
    }

    pub fn clear(&mut self) { self.map.clear(); }

    pub fn len(&self) -> usize { self.map.len() }

    /// Shaped layout for `text` (single line unless `wrap_w` is given).
    pub fn layout(&mut self, fonts: &mut FontSystem, text: &str, font: &Font, wrap_w: Option<f32>, scale: f32) -> &TextLayout {
        let wrap = wrap_w.map(|w| w.max(1.0).to_bits());
        let sb = scale.to_bits();
        let k = key(text, font, wrap, sb);
        let frame = self.frame;
        let hit = self.map.get(&k).is_some_and(|e| e.text == text && e.font == *font && e.wrap == wrap && e.scale == sb);
        if !hit {
            let constraints = match wrap_w {
                Some(w) => TextConstraints { max_width: Some(w.max(1.0)), wrap: TextWrap::WordOrGlyph },
                None => TextConstraints { max_width: None, wrap: TextWrap::None },
            };
            let layout = fonts.shape(text, &font.style(), constraints, scale);
            self.map.insert(k, Entry { text: text.to_string(), font: font.clone(), wrap, scale: sb, layout, used: frame });
        }
        let e = self.map.get_mut(&k).unwrap();
        e.used = frame;
        &e.layout
    }

    /// Logical width of a single-line `text`.
    pub fn measure(&mut self, fonts: &mut FontSystem, text: &str, font: &Font, scale: f32) -> f32 {
        if text.is_empty() { return 0.0; }
        self.layout(fonts, text, font, None, scale).width
    }

    /// Draw `text` with the top of its line box at `y_top`. Returns the drawn width.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, scene: &mut Scene, fonts: &mut FontSystem, atlas: &mut GlyphAtlas, text: &str, font: &Font,
                x: f32, y_top: f32, color: Color, scale: f32, wrap_w: Option<f32>) -> (f32, f32) {
        if text.is_empty() || color.a == 0 { return (0.0, 0.0); }
        let layout = self.layout(fonts, text, font, wrap_w, scale);
        let (w, h, baseline) = (layout.width, layout.height, layout.baseline);
        let prepared = fonts.prepare(layout, atlas);
        if prepared.glyphs.is_empty() && prepared.uploads.is_empty() { return (w, h); }
        let fmt = |f: AtlasFormat| match f { AtlasFormat::Alpha8 => GlyphFormat::Alpha8, AtlasFormat::Rgba8 => GlyphFormat::Rgba8 };
        let uploads = prepared.uploads.into_iter().map(|u| AtlasUpload {
            page: u.page, origin: [u.x, u.y], size: [u.width, u.height], format: fmt(u.format), pixels: u.pixels,
        }).collect();
        let glyphs = prepared.glyphs.iter().map(|g| GlyphDraw {
            x: g.x as f32, y: g.y as f32, width: g.width as f32, height: g.height as f32,
            atlas_x: g.atlas_x, atlas_y: g.atlas_y, atlas_page: g.atlas_page, format: fmt(g.format),
        }).collect();
        // snap the origin to whole physical pixels: glyph bitmaps are rasterised on the pixel grid
        let snap = |v: f32| (v * scale).round() / scale;
        scene.push(DrawCommand::Text(TextPrimitive {
            origin: Point::new(snap(x), snap(y_top + baseline)),
            color: color.to_acme(),
            glyphs,
            uploads,
        }));
        (w, h)
    }
}
