//! C ABI boundary for the optional Windows spelling module.
use std::sync::OnceLock;

fn dictionary() -> Option<&'static spellbook::Dictionary> {
    static DICT: OnceLock<Option<spellbook::Dictionary>> = OnceLock::new();
    DICT.get_or_init(|| spellbook::Dictionary::new(include_str!("../../../assets/dict/en_US.aff"), include_str!("../../../assets/dict/en_US.dic")).ok()).as_ref()
}

#[no_mangle]
pub extern "C" fn notedit_spell_version() -> u32 { 1 }

#[no_mangle]
pub extern "C" fn notedit_spell_ready() -> bool { dictionary().is_some() }

/// The caller provides a valid UTF-8 byte slice for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn notedit_spell_check(word: *const u8, len: usize) -> bool {
    let bytes = std::slice::from_raw_parts(word, len);
    let Ok(text) = std::str::from_utf8(bytes) else { return true };
    dictionary().is_none_or(|d| d.check(text))
}

/// Writes newline-separated UTF-8 suggestions to a caller-owned buffer.
#[no_mangle]
pub unsafe extern "C" fn notedit_spell_suggest(word: *const u8, len: usize, out: *mut u8, capacity: usize) -> usize {
    let bytes = std::slice::from_raw_parts(word, len);
    let Ok(text) = std::str::from_utf8(bytes) else { return 0 };
    let Some(dict) = dictionary() else { return 0 };
    let mut suggestions = Vec::new();
    dict.suggest(text, &mut suggestions);
    let result = suggestions.into_iter().take(6).collect::<Vec<_>>().join("\n");
    if result.len() > capacity { return 0; }
    std::ptr::copy_nonoverlapping(result.as_ptr(), out, result.len());
    result.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exported_api_checks_and_suggests_without_cross_module_allocation() {
        assert_eq!(notedit_spell_version(), 1);
        assert!(notedit_spell_ready());
        let word = b"helo";
        assert!(!unsafe { notedit_spell_check(word.as_ptr(), word.len()) });
        let mut output = [0; 8192];
        let n = unsafe { notedit_spell_suggest(word.as_ptr(), word.len(), output.as_mut_ptr(), output.len()) };
        assert!(std::str::from_utf8(&output[..n]).unwrap().lines().any(|s| s == "hello"));
        assert_eq!(unsafe { notedit_spell_suggest(word.as_ptr(), word.len(), output.as_mut_ptr(), 1) }, 0);
    }
}
