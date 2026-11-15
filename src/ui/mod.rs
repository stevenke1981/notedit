//! AcmeUI-Native front end. This is the only module that imports `acme_*` crates.
pub mod dispatch;
pub mod hit;
pub mod keys;
pub mod paint;
pub mod script;
pub mod shell;
pub mod snapshot;
pub mod state;
pub mod text;
pub mod timer;
pub mod types;
pub mod view_ai;
pub mod view_chrome;
pub mod view_dialogs;
pub mod view_editor;
pub mod view_popup;
pub mod view_preview;
pub mod view_side;
pub mod win;

pub use state::AppWindow;
pub use types::*;
