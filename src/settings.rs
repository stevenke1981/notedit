//! Persistent settings + session (JSON in the user config directory).
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct SessionFile { pub path: PathBuf, pub cursor: usize }

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub font: f32,
    pub font_family: String,
    pub wrap: bool,
    pub line_numbers: bool,
    pub dark: bool,
    pub preview: bool,
    pub restore_session: bool,
    pub recent: Vec<PathBuf>,
    pub session: Vec<SessionFile>,
    pub session_cur: usize,
    pub macro_keys: Vec<String>,
    pub docmap: bool,
    pub autocomplete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { font: 14.0, font_family: "JetBrains Mono".into(), wrap: false, line_numbers: true, dark: false, preview: false, restore_session: true,
                   recent: vec![], session: vec![], session_cur: 0, macro_keys: vec![], docmap: false, autocomplete: true }
    }
}

pub fn config_dir() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) { env("APPDATA") }
        else if cfg!(target_os = "macos") { env("HOME").map(|h| h.join("Library/Application Support")) }
        else { env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config"))) };
    base.unwrap_or_else(std::env::temp_dir).join("notedit")
}

fn file() -> PathBuf { std::env::var_os("NOTEDIT_CONFIG").map(PathBuf::from).unwrap_or_else(|| config_dir().join("settings.json")) }

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        let f = file();
        if let Some(d) = f.parent() { std::fs::create_dir_all(d)?; }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::file_io::durable_write(&f, &json)
    }
    pub fn add_recent(&mut self, p: PathBuf) {
        self.recent.retain(|x| x != &p);
        self.recent.insert(0, p);
        self.recent.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_recent() {
        let mut s = Settings::default();
        for i in 0..12 { s.add_recent(PathBuf::from(format!("/f{}", i))); }
        s.add_recent(PathBuf::from("/f5"));
        assert_eq!(s.recent.len(), 10);
        assert_eq!(s.recent[0], PathBuf::from("/f5"));
        let j = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(back.recent, s.recent);
        let partial: Settings = serde_json::from_str("{\"dark\":true}").unwrap();
        assert!(partial.dark && partial.font == 14.0 && partial.font_family == "JetBrains Mono");
    }
}
