//! Per-user encrypted OpenRouter credentials on Windows.
use std::io;
use std::path::PathBuf;

fn path() -> PathBuf { crate::settings::config_dir().join("openrouter-key.dpapi") }

#[cfg(windows)]
fn crypt(data: &[u8], protect: bool) -> io::Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len().try_into().map_err(|_| io::Error::other("金鑰太長"))?, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if protect { CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
        else { CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
    };
    if ok == 0 { return Err(io::Error::last_os_error()); }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData.cast()); }
    Ok(bytes)
}

#[cfg(windows)]
pub fn save(key: &str) -> io::Result<()> {
    let encrypted = crypt(key.as_bytes(), true)?;
    std::fs::create_dir_all(crate::settings::config_dir())?;
    crate::file_io::durable_write(&path(), &encrypted)
}

#[cfg(windows)]
pub fn load() -> io::Result<Option<String>> {
    let encrypted = match std::fs::read(path()) { Ok(data) => data, Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None), Err(e) => return Err(e) };
    String::from_utf8(crypt(&encrypted, false)?).map(Some).map_err(io::Error::other)
}

#[cfg(not(windows))]
pub fn save(_: &str) -> io::Result<()> { Err(io::Error::other("此平台尚不支援安全儲存金鑰")) }
#[cfg(not(windows))]
pub fn load() -> io::Result<Option<String>> { Ok(None) }

pub fn clear() -> io::Result<()> {
    match std::fs::remove_file(path()) { Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()), result => result }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn encryption_roundtrip() {
        let key = b"test-key-not-a-secret";
        let encrypted = super::crypt(key, true).unwrap();
        assert!(!encrypted.windows(key.len()).any(|x| x == key));
        assert_eq!(super::crypt(&encrypted, false).unwrap(), key);
    }
}
