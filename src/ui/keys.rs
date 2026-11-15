//! Map AcmeUI key events to the key-text vocabulary the editor (and recorded
//! macros / user shortcuts) have always used — Slint's private-use key codes.
use acme_platform::PlatformKey;

pub const UP: char = '\u{F700}';
pub const DOWN: char = '\u{F701}';
pub const LEFT: char = '\u{F702}';
pub const RIGHT: char = '\u{F703}';

/// Key text for `ed_key`: control characters for editing keys, U+F7xx for
/// navigation / function keys, otherwise the logical character (unaffected
/// by Ctrl, so Ctrl+S yields "s").
pub fn slint_text(key: PlatformKey, character: Option<&str>, text: Option<&str>) -> Option<String> {
    let c = match key {
        PlatformKey::Tab => '\t',
        PlatformKey::Enter => '\n',
        PlatformKey::Escape => '\u{1b}',
        PlatformKey::Backspace => '\u{8}',
        PlatformKey::Delete => '\u{7f}',
        PlatformKey::Space => ' ',
        PlatformKey::ArrowUp => UP,
        PlatformKey::ArrowDown => DOWN,
        PlatformKey::ArrowLeft => LEFT,
        PlatformKey::ArrowRight => RIGHT,
        PlatformKey::Function(n) if (1..=35).contains(&n) => char::from_u32(0xF704 + n as u32 - 1)?,
        PlatformKey::Function(_) => return None,
        PlatformKey::Insert => '\u{F727}',
        PlatformKey::Home => '\u{F729}',
        PlatformKey::End => '\u{F72B}',
        PlatformKey::PageUp => '\u{F72C}',
        PlatformKey::PageDown => '\u{F72D}',
        PlatformKey::Other => {
            let s = character.or(text).filter(|s| !s.is_empty())?;
            if s.chars().all(|c| c.is_control()) { return None; }
            return Some(s.to_string());
        }
    };
    Some(c.to_string())
}

/// Text a key press types into the editor (None for shortcuts / non-printing keys).
pub fn typed_text(key: PlatformKey, text: Option<&str>, ctrl: bool, alt: bool) -> Option<String> {
    // Ctrl+Alt is AltGr on Windows layouts: it types characters
    if (ctrl || alt) && !(ctrl && alt) { return None; }
    match key {
        PlatformKey::Space => Some(" ".into()),
        PlatformKey::Other => text.filter(|t| !t.is_empty() && !t.chars().any(|c| c.is_control())).map(str::to_string),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vocabulary_matches_slint() {
        assert_eq!(slint_text(PlatformKey::Function(3), None, None).as_deref(), Some("\u{F706}"));
        assert_eq!(slint_text(PlatformKey::Function(8), None, None).as_deref(), Some("\u{F70B}"));
        assert_eq!(slint_text(PlatformKey::Delete, None, None).as_deref(), Some("\u{7f}"));
        assert_eq!(slint_text(PlatformKey::Other, Some("s"), Some("\u{13}")).as_deref(), Some("s"));
        assert_eq!(slint_text(PlatformKey::PageDown, None, None).as_deref(), Some("\u{F72D}"));
        assert_eq!(typed_text(PlatformKey::Other, Some("a"), true, false), None);
        assert_eq!(typed_text(PlatformKey::Other, Some("@"), true, true).as_deref(), Some("@"));
        assert_eq!(typed_text(PlatformKey::Other, Some("中"), false, false).as_deref(), Some("中"));
    }
}
