//! Scripted input for headless checks: `NOTEDIT_SCRIPT=<file>` feeds synthetic
//! platform events through the real event routing, one step per line:
//!
//! ```text
//! wait 200            # pause before the next step (ms)
//! key ctrl+shift+f    # modifiers + key name (enter, esc, tab, f3, left, a, ...)
//! type hello 中文      # typed text, one key event per character
//! click 120 80 [right|shift|ctrl|alt]
//! drag 100 200 300 200
//! move 120 80
//! wheel 400 300 -3 [ctrl|shift]
//! ime 注音 / commit 中文
//! shot C:\out.png     # CPU-rendered screenshot of the next frame
//! quit
//! ```
use acme_platform::{PlatformEvent, PlatformKey, WindowId};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Step {
    Wait(Duration),
    Events(Vec<PlatformEvent>),
    Wheel { x: f32, y: f32, dy: f32, ctrl: bool, shift: bool },
    Shot(std::path::PathBuf),
    Quit,
}

const W: WindowId = WindowId(0);

fn key_ev(key: PlatformKey, text: Option<String>, character: Option<String>, m: (bool, bool, bool), pressed: bool) -> PlatformEvent {
    PlatformEvent::Key { window: W, key, pressed, shift: m.1, ctrl: m.0, alt: m.2, meta: false, repeat: false, text, character }
}

fn named(k: &str) -> Option<PlatformKey> {
    Some(match k {
        "enter" | "return" => PlatformKey::Enter, "esc" | "escape" => PlatformKey::Escape, "tab" => PlatformKey::Tab,
        "space" => PlatformKey::Space, "backspace" => PlatformKey::Backspace, "delete" | "del" => PlatformKey::Delete,
        "left" => PlatformKey::ArrowLeft, "right" => PlatformKey::ArrowRight, "up" => PlatformKey::ArrowUp, "down" => PlatformKey::ArrowDown,
        "home" => PlatformKey::Home, "end" => PlatformKey::End, "pageup" => PlatformKey::PageUp, "pagedown" => PlatformKey::PageDown,
        "insert" => PlatformKey::Insert,
        f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => PlatformKey::Function(f[1..].parse().ok()?),
        _ => return None,
    })
}

fn mods_of(words: &[&str]) -> (bool, bool, bool) {
    (words.contains(&"ctrl"), words.contains(&"shift"), words.contains(&"alt"))
}

fn button(x: f32, y: f32, b: u16, pressed: bool, m: (bool, bool, bool)) -> PlatformEvent {
    PlatformEvent::PointerButton { window: W, pressed, x, y, button: b, pointer: 0, shift: m.1, ctrl: m.0, alt: m.2 }
}

pub fn parse(src: &str) -> Vec<Step> {
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let nums: Vec<f32> = rest.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        let words: Vec<&str> = rest.split_whitespace().collect();
        match cmd {
            "wait" => out.push(Step::Wait(Duration::from_millis(nums.first().copied().unwrap_or(100.0) as u64))),
            "key" => {
                let parts: Vec<&str> = rest.trim().split('+').collect();
                let m = mods_of(&parts);
                let k = parts.last().copied().unwrap_or("").to_string();
                let lk = k.to_lowercase();
                let ev = match named(&lk) {
                    Some(pk) => { let t = (pk == PlatformKey::Space).then(|| " ".to_string()); vec![key_ev(pk, t, None, m, true), key_ev(pk, None, None, m, false)] }
                    None => {
                        let ch = if m.1 { k.to_uppercase() } else { k.clone() };
                        let text = if m.0 && !m.2 { None } else { Some(ch.clone()) };
                        vec![key_ev(PlatformKey::Other, text, Some(ch), m, true), key_ev(PlatformKey::Other, None, None, m, false)]
                    }
                };
                out.push(Step::Events(ev));
            }
            "type" => {
                let ev = rest.chars().map(|c| {
                    let s = c.to_string();
                    if c == ' ' { key_ev(PlatformKey::Space, Some(s.clone()), None, (false, false, false), true) }
                    else { key_ev(PlatformKey::Other, Some(s.clone()), Some(s), (false, false, false), true) }
                }).collect();
                out.push(Step::Events(ev));
            }
            "click" if nums.len() >= 2 => {
                let (x, y) = (nums[0], nums[1]);
                let b = if words.contains(&"right") { 1 } else { 0 };
                let m = mods_of(&words);
                out.push(Step::Events(vec![PlatformEvent::PointerMoved { window: W, x, y }, button(x, y, b, true, m), button(x, y, b, false, m)]));
            }
            "press" if nums.len() >= 2 => out.push(Step::Events(vec![PlatformEvent::PointerMoved { window: W, x: nums[0], y: nums[1] }, button(nums[0], nums[1], 0, true, mods_of(&words))])),
            "release" if nums.len() >= 2 => out.push(Step::Events(vec![PlatformEvent::PointerMoved { window: W, x: nums[0], y: nums[1] }, button(nums[0], nums[1], 0, false, mods_of(&words))])),
            "move" if nums.len() >= 2 => out.push(Step::Events(vec![PlatformEvent::PointerMoved { window: W, x: nums[0], y: nums[1] }])),
            "drag" if nums.len() >= 4 => {
                let (x1, y1, x2, y2) = (nums[0], nums[1], nums[2], nums[3]);
                let mut ev = vec![PlatformEvent::PointerMoved { window: W, x: x1, y: y1 }, button(x1, y1, 0, true, (false, false, false))];
                for i in 1..=8 { let f = i as f32 / 8.0; ev.push(PlatformEvent::PointerMoved { window: W, x: x1 + (x2 - x1) * f, y: y1 + (y2 - y1) * f }); }
                ev.push(button(x2, y2, 0, false, (false, false, false)));
                out.push(Step::Events(ev));
            }
            "wheel" if nums.len() >= 3 => out.push(Step::Wheel { x: nums[0], y: nums[1], dy: nums[2], ctrl: words.contains(&"ctrl"), shift: words.contains(&"shift") }),
            "ime" => out.push(Step::Events(vec![PlatformEvent::ImePreedit { window: W, text: rest.to_string(), cursor: None }])),
            "commit" => out.push(Step::Events(vec![PlatformEvent::ImeCommit { window: W, text: rest.to_string() }])),
            "shot" => out.push(Step::Shot(std::path::PathBuf::from(rest.trim()))),
            "quit" => out.push(Step::Quit),
            _ => eprintln!("NotEdit script: ignored line {line:?}"),
        }
    }
    out
}
