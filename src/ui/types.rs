//! Plain-Rust replacements for the structs the Slint compiler used to generate.
//! Field names are kept so the editor/render code reads the same as before.

/// 8-bit sRGB colour with alpha (replaces `slint::Color`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self { Color { r, g, b, a: 255 } }
    pub const fn from_argb_u8(a: u8, r: u8, g: u8, b: u8) -> Self { Color { r, g, b, a } }
    pub const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };
    pub fn red(self) -> u8 { self.r }
    pub fn green(self) -> u8 { self.g }
    pub fn blue(self) -> u8 { self.b }
    pub fn alpha(self) -> u8 { self.a }
    pub fn with_alpha(self, a: u8) -> Self { Color { a, ..self } }
    pub fn to_acme(self) -> acme_core::Color {
        acme_core::Color::rgba(self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0, self.a as f32 / 255.0)
    }
}

/// `#rrggbb` / `#rrggbbaa` shorthand used by the hand-painted views.
pub const fn hex(v: u32) -> Color { Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8) }
pub const fn hexa(v: u32) -> Color { Color::from_argb_u8(v as u8, (v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8) }

/// The colours of the former `global Theme` (editor.slint).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeColors {
    pub editor_bg: Color,
    pub fg: Color,
    pub gutter: Color,
    pub gutter_fg: Color,
    pub curline: Color,
    pub sel: Color,
    pub mark: Color,
    pub bracket: Color,
    pub chrome: Color,
    pub menu: Color,
    pub tabbar: Color,
    pub tab: Color,
    pub tab_active: Color,
    pub border: Color,
    pub hover: Color,
    pub diff_add: Color,
    pub diff_del: Color,
    pub diff_chg: Color,
}

impl Default for ThemeColors {
    fn default() -> Self {
        ThemeColors {
            editor_bg: hex(0xffffff), fg: hex(0x1e1e1e), gutter: hex(0xe4e4e4), gutter_fg: hex(0x808080),
            curline: hex(0xe8f2fe), sel: hex(0xc0d8f0), mark: hex(0xb4f0b4), bracket: hex(0xffd97a),
            chrome: hex(0xf0f0f0), menu: hex(0xf5f6f7), tabbar: hex(0xe8e8e8), tab: hex(0xdcdcdc), tab_active: hex(0xffffff),
            border: hex(0xbbbbbb), hover: hex(0xcce4f7), diff_add: hex(0xd4f8d4), diff_del: hex(0xffd7d7), diff_chg: hex(0xfff3c4),
        }
    }
}

impl ThemeColors {
    /// Set one colour by its settings.json key; false for an unknown key.
    pub fn set(&mut self, key: &str, c: Color) -> bool {
        let slot = match key {
            "editor-bg" => &mut self.editor_bg, "fg" => &mut self.fg, "gutter" => &mut self.gutter, "gutter-fg" => &mut self.gutter_fg,
            "curline" => &mut self.curline, "sel" => &mut self.sel, "mark" => &mut self.mark, "bracket" => &mut self.bracket,
            "chrome" => &mut self.chrome, "menu" => &mut self.menu, "tabbar" => &mut self.tabbar, "tab" => &mut self.tab,
            "tab-active" => &mut self.tab_active, "border" => &mut self.border, "hover" => &mut self.hover,
            "diff-add" => &mut self.diff_add, "diff-del" => &mut self.diff_del, "diff-chg" => &mut self.diff_chg,
            _ => return false,
        };
        *slot = c;
        true
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdTok { pub x: f32, pub text: String, pub color: Color, pub bold: bool, pub italic: bool }

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdRect { pub x: f32, pub w: f32, pub kind: i32 }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdRow {
    pub num: String,
    pub line: i32,
    pub fold: i32,
    pub cur: bool,
    pub mark: bool,
    pub diff: i32,
    pub toks: Vec<EdTok>,
    pub rects: Vec<EdRect>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdCaret { pub x: f32, pub y: f32 }

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MapLine { pub y: f32, pub x: f32, pub w: f32, pub color: Color }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdState {
    pub rows: Vec<EdRow>,
    pub carets: Vec<EdCaret>,
    pub gutter_w: f32,
    pub num_w: f32,
    pub thumb_pos: f32,
    pub thumb_size: f32,
    pub hthumb_pos: f32,
    pub hthumb_size: f32,
    pub hbar: bool,
    pub ac_items: Vec<String>,
    pub ac_sel: i32,
    pub ac_visible: bool,
    pub title: String,
    pub map_lines: Vec<MapLine>,
    pub map_view_y: f32,
    pub map_view_h: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchHit { pub doc: i32, pub title: String, pub line: i32, pub start: i32, pub end: i32, pub text: String }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuItem { pub label: String, pub shortcut: String, pub action: String, pub separator: bool, pub checked: bool, pub enabled: bool }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu { pub title: String, pub items: Vec<MenuItem> }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabInfo { pub title: String, pub modified: bool, pub w: f32 }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SideItem { pub text: String, pub indent: i32, pub kind: i32, pub line: i32 }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block { pub text: String, pub kind: i32, pub indent: i32, pub bold: bool, pub italic: bool, pub code: bool, pub link: bool }
