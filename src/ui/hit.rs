//! Hit targets recorded in paint order each frame and tested in reverse
//! (topmost first), replacing the former toolkit's touch areas.

/// Dialog text fields (single-line, backed by `acme_textinput`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldId { Find, Replace, FifDir, FifFilter, Prompt, Rename, Goto, AiKey, AiModel, AiPrompt, AiLangCustom }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckId { MatchCase, WholeWord, UseRegex, ReplaceMode, FifRecursive, FifReplace, AiIncludeDoc }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DlgId { Find, Fif, Prompt, Goto, About, Rename, Ai }

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    MenuTitle(usize),
    /// dropdown / context-menu item: the action string
    MenuItem(String),
    CtxItem(String),
    /// option of an open combo popup: (combo id, index)
    ComboItem(u8, usize),
    Tool(&'static str),
    AiToggle,
    Tab(usize),
    TabClose(usize),
    Editor(usize),
    Gutter(usize),
    VBar(usize),
    HBar(usize),
    Map(usize),
    AcItem(usize, usize),
    SideRow(usize),
    SideTab(i32),
    SideFolder,
    SideList,
    Hit(usize),
    HitsClose,
    HitsList,
    Preview,
    Field(DlgId, FieldId),
    Check(CheckId),
    /// a push button: its action id
    Button(&'static str),
    Combo(u8),
    AiPage(i32),
    AiClose,
    AiScroll,
    /// absorbs clicks inside a dialog/panel so they don't fall through
    Absorb,
    /// click-away layer under popups
    Backdrop,
    DialogClose(DlgId),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

impl R {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self { R { x, y, w, h } }
    pub fn contains(&self, px: f32, py: f32) -> bool { px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h }
    pub fn right(&self) -> f32 { self.x + self.w }
    pub fn bottom(&self) -> f32 { self.y + self.h }
    pub fn intersect(&self, o: &R) -> R {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        R::new(x, y, (self.right().min(o.right()) - x).max(0.0), (self.bottom().min(o.bottom()) - y).max(0.0))
    }
}

#[derive(Default)]
pub struct Hits { pub list: Vec<(R, Target)> }

impl Hits {
    pub fn clear(&mut self) { self.list.clear(); }
    pub fn push(&mut self, r: R, t: Target) { if r.w > 0.0 && r.h > 0.0 { self.list.push((r, t)); } }
    /// topmost target under (x, y) with its rect
    pub fn at(&self, x: f32, y: f32) -> Option<(R, Target)> {
        self.list.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(r, t)| (*r, t.clone()))
    }
    pub fn rect_of(&self, t: &Target) -> Option<R> { self.list.iter().rev().find(|(_, x)| x == t).map(|(r, _)| *r) }
}
