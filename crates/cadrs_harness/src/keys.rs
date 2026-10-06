//! Maps key names and characters to Bevy key codes and logical keys.

use bevy::input::keyboard::{Key, KeyCode, NativeKeyCode};

/// A key as the harness sends it.
#[derive(Debug, Clone, PartialEq)]
pub struct KeySpec {
    pub code: KeyCode,
    pub logical: Key,
    pub text: Option<String>,
}

impl KeySpec {
    fn named(code: KeyCode, logical: Key) -> Self {
        Self {
            code,
            logical,
            text: None,
        }
    }
}

/// A key press with modifiers, parsed from strings like `"Ctrl+Shift+Z"`.
#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
    pub modifiers: Vec<KeySpec>,
    pub key: KeySpec,
}

/// A single key or modifier, for `KeyDown`/`KeyUp`: `"Ctrl"`, `"Shift"`, `"Alt"`, `"A"`.
pub fn parse_single(s: &str) -> Result<KeySpec, String> {
    match s.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Ok(KeySpec::named(KeyCode::ControlLeft, Key::Control)),
        "shift" => Ok(KeySpec::named(KeyCode::ShiftLeft, Key::Shift)),
        "alt" => Ok(KeySpec::named(KeyCode::AltLeft, Key::Alt)),
        "super" | "meta" | "cmd" => Ok(KeySpec::named(KeyCode::SuperLeft, Key::Super)),
        _ => parse_key(s, false).ok_or_else(|| format!("unknown key {s:?}")),
    }
}

pub fn parse_chord(s: &str) -> Result<Chord, String> {
    let parts: Vec<&str> = s.split('+').collect();
    // "Ctrl++" or a lone "+" means the plus key.
    let (mods, key) = if s.ends_with("++") {
        (&parts[..parts.len() - 2], "+")
    } else if s == "+" {
        (&parts[..0], "+")
    } else {
        (&parts[..parts.len() - 1], parts[parts.len() - 1])
    };
    let modifiers = mods
        .iter()
        .map(|m| match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Ok(KeySpec::named(KeyCode::ControlLeft, Key::Control)),
            "shift" => Ok(KeySpec::named(KeyCode::ShiftLeft, Key::Shift)),
            "alt" => Ok(KeySpec::named(KeyCode::AltLeft, Key::Alt)),
            "super" | "meta" | "cmd" => Ok(KeySpec::named(KeyCode::SuperLeft, Key::Super)),
            other => Err(format!("unknown modifier {other:?} in {s:?}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let shifted = modifiers.iter().any(|m| m.code == KeyCode::ShiftLeft);
    let key = parse_key(key, shifted).ok_or_else(|| format!("unknown key {key:?} in {s:?}"))?;
    Ok(Chord { modifiers, key })
}

fn parse_key(name: &str, shifted: bool) -> Option<KeySpec> {
    use KeyCode as C;
    let named = |code, logical| Some(KeySpec::named(code, logical));
    match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => return named(C::Enter, Key::Enter),
        "escape" | "esc" => return named(C::Escape, Key::Escape),
        "backspace" => return named(C::Backspace, Key::Backspace),
        "delete" | "del" => return named(C::Delete, Key::Delete),
        "insert" | "ins" => return named(C::Insert, Key::Insert),
        "tab" => return named(C::Tab, Key::Tab),
        "space" => {
            return Some(KeySpec {
                code: C::Space,
                logical: Key::Space,
                text: Some(" ".into()),
            });
        }
        "left" | "arrowleft" => return named(C::ArrowLeft, Key::ArrowLeft),
        "right" | "arrowright" => return named(C::ArrowRight, Key::ArrowRight),
        "up" | "arrowup" => return named(C::ArrowUp, Key::ArrowUp),
        "down" | "arrowdown" => return named(C::ArrowDown, Key::ArrowDown),
        "home" => return named(C::Home, Key::Home),
        "end" => return named(C::End, Key::End),
        "pageup" => return named(C::PageUp, Key::PageUp),
        "pagedown" => return named(C::PageDown, Key::PageDown),
        "f1" => return named(C::F1, Key::F1),
        "f2" => return named(C::F2, Key::F2),
        "f3" => return named(C::F3, Key::F3),
        "f4" => return named(C::F4, Key::F4),
        "f5" => return named(C::F5, Key::F5),
        "f6" => return named(C::F6, Key::F6),
        "f7" => return named(C::F7, Key::F7),
        "f8" => return named(C::F8, Key::F8),
        "f9" => return named(C::F9, Key::F9),
        "f10" => return named(C::F10, Key::F10),
        "f11" => return named(C::F11, Key::F11),
        "f12" => return named(C::F12, Key::F12),
        _ => {}
    }
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let c = if shifted {
        shift_char(c)
    } else {
        c.to_ascii_lowercase()
    };
    Some(char_key(c))
}

fn shift_char(c: char) -> char {
    match c {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        c => c.to_ascii_uppercase(),
    }
}

/// The key that types `c` (with the character as its text).
pub fn char_key(c: char) -> KeySpec {
    use KeyCode as C;
    let code = match c.to_ascii_lowercase() {
        'a' => C::KeyA,
        'b' => C::KeyB,
        'c' => C::KeyC,
        'd' => C::KeyD,
        'e' => C::KeyE,
        'f' => C::KeyF,
        'g' => C::KeyG,
        'h' => C::KeyH,
        'i' => C::KeyI,
        'j' => C::KeyJ,
        'k' => C::KeyK,
        'l' => C::KeyL,
        'm' => C::KeyM,
        'n' => C::KeyN,
        'o' => C::KeyO,
        'p' => C::KeyP,
        'q' => C::KeyQ,
        'r' => C::KeyR,
        's' => C::KeyS,
        't' => C::KeyT,
        'u' => C::KeyU,
        'v' => C::KeyV,
        'w' => C::KeyW,
        'x' => C::KeyX,
        'y' => C::KeyY,
        'z' => C::KeyZ,
        '1' | '!' => C::Digit1,
        '2' | '@' => C::Digit2,
        '3' | '#' => C::Digit3,
        '4' | '$' => C::Digit4,
        '5' | '%' => C::Digit5,
        '6' | '^' => C::Digit6,
        '7' | '&' => C::Digit7,
        '8' | '*' => C::Digit8,
        '9' | '(' => C::Digit9,
        '0' | ')' => C::Digit0,
        ' ' => C::Space,
        '-' | '_' => C::Minus,
        '=' | '+' => C::Equal,
        '.' | '>' => C::Period,
        ',' | '<' => C::Comma,
        '/' | '?' => C::Slash,
        ';' | ':' => C::Semicolon,
        '\'' | '"' => C::Quote,
        // P3E.3b: `[` (the Measure tool's shortcut) reached the app as an unknown key.
        '[' | '{' => C::BracketLeft,
        ']' | '}' => C::BracketRight,
        '\\' | '|' => C::Backslash,
        '`' | '~' => C::Backquote,
        _ => C::Unidentified(NativeKeyCode::Unidentified),
    };
    let logical = if c == ' ' {
        Key::Space
    } else {
        Key::Character(c.to_string().into())
    };
    KeySpec {
        code,
        logical,
        text: Some(c.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        let c = parse_chord("Ctrl+A").unwrap();
        assert_eq!(c.modifiers.len(), 1);
        assert_eq!(c.key.code, KeyCode::KeyA);
        assert_eq!(c.key.logical, Key::Character("a".into()));
        let c = parse_chord("Shift+7").unwrap();
        assert_eq!(c.key.code, KeyCode::Digit7);
        assert_eq!(c.key.text.as_deref(), Some("&"));
        assert_eq!(parse_chord("Enter").unwrap().key.code, KeyCode::Enter);
        assert_eq!(parse_chord("L").unwrap().key.code, KeyCode::KeyL);
        assert!(parse_chord("Hyper+A").is_err());
        assert_eq!(parse_single("Ctrl").unwrap().code, KeyCode::ControlLeft);
        assert_eq!(parse_single("n").unwrap().code, KeyCode::KeyN);
    }
}
