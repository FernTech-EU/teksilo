// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Keyboard → PTY byte encoding: the "heart" of the terminal view. Translates
//! a decoded [`Key`] + [`Modifiers`] (+ the printable `text` winit already
//! resolved) into the exact byte sequence a VT-100/xterm-class child expects,
//! honouring the DEC private modes the engine reports ([`TermMode`]).
//!
//! This is pure, engine-independent logic — the largest body of unit tests in
//! the crate lives here (every key × relevant mode → exact bytes).

use crate::engine::TermMode;
use bastyde_core::event::{Key, Modifiers};

/// Knobs that change how keystrokes are encoded.
#[derive(Debug, Clone, Copy)]
pub struct InputConfig {
    /// When `true`, `Alt+<key>` is sent as an ESC prefix (the "meta sends
    /// escape" convention; macOS calls this "use Option as Meta"). When
    /// `false`, Alt is left for the OS to compose accented characters and the
    /// resolved `text` is sent verbatim.
    pub alt_sends_escape: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            alt_sends_escape: true,
        }
    }
}

/// The xterm modifier parameter: `1 + shift + 2·alt + 4·ctrl + 8·super`, or
/// `None` when no modifier is held (so the base, unparameterised form is used).
fn modifier_param(mods: Modifiers) -> Option<u8> {
    let mut n = 1u8;
    if mods.shift() {
        n += 1;
    }
    if mods.alt() {
        n += 2;
    }
    if mods.ctrl() {
        n += 4;
    }
    if mods.super_key() {
        n += 8;
    }
    (n != 1).then_some(n)
}

/// `ESC [ <params> <final>` — a CSI sequence.
fn csi(params: &str, final_byte: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + params.len());
    out.push(0x1b);
    out.push(b'[');
    out.extend_from_slice(params.as_bytes());
    out.push(final_byte);
    out
}

/// `ESC [ <code> [; <mod>] ~` — the numbered "tilde" function keys.
fn csi_tilde(code: u16, modp: Option<u8>) -> Vec<u8> {
    match modp {
        Some(m) => csi(&format!("{code};{m}"), b'~'),
        None => csi(&code.to_string(), b'~'),
    }
}

/// Cursor / Home / End keys: `ESC [ 1 ; <mod> <final>` when modified, else
/// `ESC O <final>` in application-cursor mode or `ESC [ <final>` otherwise.
fn cursor_key(final_byte: u8, app_cursor: bool, modp: Option<u8>) -> Vec<u8> {
    match modp {
        Some(m) => csi(&format!("1;{m}"), final_byte),
        None if app_cursor => vec![0x1b, b'O', final_byte],
        None => vec![0x1b, b'[', final_byte],
    }
}

/// PF keys F1–F4: `ESC [ 1 ; <mod> <final>` when modified, else `ESC O <final>`.
fn ss3_key(final_byte: u8, modp: Option<u8>) -> Vec<u8> {
    match modp {
        Some(m) => csi(&format!("1;{m}"), final_byte),
        None => vec![0x1b, b'O', final_byte],
    }
}

/// The `CSI ~` code for F5–F24 (F1–F4 use [`ss3_key`]). `None` for keys outside
/// that range.
fn function_key_code(key: Key) -> Option<u16> {
    Some(match key {
        Key::F5 => 15,
        Key::F6 => 17,
        Key::F7 => 18,
        Key::F8 => 19,
        Key::F9 => 20,
        Key::F10 => 21,
        Key::F11 => 23,
        Key::F12 => 24,
        Key::F13 => 25,
        Key::F14 => 26,
        Key::F15 => 28,
        Key::F16 => 29,
        Key::F17 => 31,
        Key::F18 => 32,
        Key::F19 => 33,
        Key::F20 => 34,
        Key::F21 => 35,
        Key::F22 => 36,
        Key::F23 => 37,
        Key::F24 => 38,
        _ => return None,
    })
}

/// The control byte for `Ctrl+<letter>` (`Ctrl+A`=0x01 … `Ctrl+Z`=0x1a).
fn ctrl_letter(key: Key) -> Option<u8> {
    let base = match key {
        Key::A => 1,
        Key::B => 2,
        Key::C => 3,
        Key::D => 4,
        Key::E => 5,
        Key::F => 6,
        Key::G => 7,
        Key::H => 8,
        Key::I => 9,
        Key::J => 10,
        Key::K => 11,
        Key::L => 12,
        Key::M => 13,
        Key::N => 14,
        Key::O => 15,
        Key::P => 16,
        Key::Q => 17,
        Key::R => 18,
        Key::S => 19,
        Key::T => 20,
        Key::U => 21,
        Key::V => 22,
        Key::W => 23,
        Key::X => 24,
        Key::Y => 25,
        Key::Z => 26,
        _ => return None,
    };
    Some(base)
}

/// The control byte for a `Ctrl+<punctuation>` combination, following the
/// xterm/ASCII convention (`Ctrl+[`=ESC, `Ctrl+\\`=FS, …, `Ctrl+Space`=NUL).
fn ctrl_punctuation(ch: char) -> Option<u8> {
    Some(match ch {
        ' ' | '@' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// Encode one key press into the bytes to write to the child, or `None` when
/// nothing should be sent (e.g. a bare `Super` chord the app handles, or a
/// modifier-only event). `text` is winit's already-resolved printable string
/// for the key (shift / AltGr / dead-key composition already applied).
pub fn encode_key(
    key: Key,
    mods: Modifiers,
    text: Option<&str>,
    mode: TermMode,
    cfg: InputConfig,
) -> Option<Vec<u8>> {
    // Bare Super/Cmd chords are reserved for the host app (copy/paste, window
    // commands) — never forwarded to the child. (Copy/paste chords are
    // intercepted by the widget before this is called.)
    if mods.super_key() {
        return None;
    }

    let ctrl = mods.ctrl();
    let alt = mods.alt();
    let modp = modifier_param(mods);

    // Prefix a byte sequence with ESC when Alt acts as Meta.
    let meta = |bytes: Vec<u8>| -> Vec<u8> {
        if alt && cfg.alt_sends_escape {
            let mut out = Vec::with_capacity(bytes.len() + 1);
            out.push(0x1b);
            out.extend_from_slice(&bytes);
            out
        } else {
            bytes
        }
    };

    match key {
        // --- Named editing / navigation keys ---
        Key::Enter => Some(meta(vec![b'\r'])),
        Key::Tab if mods.shift() => Some(csi("", b'Z')),
        Key::Tab => Some(meta(vec![b'\t'])),
        Key::Backspace => {
            // Backspace sends DEL (0x7f) by xterm default; Ctrl+Backspace sends
            // BS (0x08).
            let b = if ctrl { 0x08 } else { 0x7f };
            Some(meta(vec![b]))
        }
        Key::Escape => Some(meta(vec![0x1b])),
        Key::Delete => Some(csi_tilde(3, modp)),
        Key::Insert => Some(csi_tilde(2, modp)),
        Key::PageUp => Some(csi_tilde(5, modp)),
        Key::PageDown => Some(csi_tilde(6, modp)),
        Key::Home => Some(cursor_key(b'H', mode.app_cursor, modp)),
        Key::End => Some(cursor_key(b'F', mode.app_cursor, modp)),
        Key::ArrowUp => Some(cursor_key(b'A', mode.app_cursor, modp)),
        Key::ArrowDown => Some(cursor_key(b'B', mode.app_cursor, modp)),
        Key::ArrowRight => Some(cursor_key(b'C', mode.app_cursor, modp)),
        Key::ArrowLeft => Some(cursor_key(b'D', mode.app_cursor, modp)),

        // --- Function keys ---
        Key::F1 => Some(ss3_key(b'P', modp)),
        Key::F2 => Some(ss3_key(b'Q', modp)),
        Key::F3 => Some(ss3_key(b'R', modp)),
        Key::F4 => Some(ss3_key(b'S', modp)),
        Key::F5 | Key::F6 | Key::F7 | Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12 => {
            function_key_code(key).map(|c| csi_tilde(c, modp))
        }
        Key::F13
        | Key::F14
        | Key::F15
        | Key::F16
        | Key::F17
        | Key::F18
        | Key::F19
        | Key::F20
        | Key::F21
        | Key::F22
        | Key::F23
        | Key::F24 => function_key_code(key).map(|c| csi_tilde(c, modp)),

        // --- Space (Ctrl+Space = NUL) ---
        Key::Space if ctrl => Some(meta(vec![0x00])),
        Key::Space => Some(meta(vec![b' '])),

        // --- Letters ---
        Key::A
        | Key::B
        | Key::C
        | Key::D
        | Key::E
        | Key::F
        | Key::G
        | Key::H
        | Key::I
        | Key::J
        | Key::K
        | Key::L
        | Key::M
        | Key::N
        | Key::O
        | Key::P
        | Key::Q
        | Key::R
        | Key::S
        | Key::T
        | Key::U
        | Key::V
        | Key::W
        | Key::X
        | Key::Y
        | Key::Z => {
            if ctrl {
                ctrl_letter(key).map(|b| meta(vec![b]))
            } else {
                // Prefer winit's resolved text (handles Shift casing), else the
                // key's own lowercase char.
                let s = text
                    .filter(|t| !t.is_empty())
                    .map(|t| t.to_string())
                    .or_else(|| key.to_char().map(|c| c.to_string()))?;
                Some(meta(s.into_bytes()))
            }
        }

        Key::CapsLock => None,

        // --- Any other printable character ---
        Key::Character(ch) => {
            if ctrl {
                // Control combinations with punctuation / digits.
                if let Some(b) = ctrl_punctuation(ch) {
                    Some(meta(vec![b]))
                } else {
                    // Ctrl with an ordinary character that has no control
                    // mapping: send the character itself (Alt still metafies).
                    text.filter(|t| !t.is_empty())
                        .map(|t| meta(t.as_bytes().to_vec()))
                }
            } else {
                let s = text
                    .filter(|t| !t.is_empty())
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| ch.to_string());
                Some(meta(s.into_bytes()))
            }
        }
    }
}

/// Wrap pasted text in bracketed-paste markers when the child enabled the mode
/// (`ESC [ 200 ~` … `ESC [ 201 ~`); otherwise return the bytes unchanged.
pub fn encode_paste(text: &str, mode: TermMode) -> Vec<u8> {
    // Normalise every line break to CR (`\r`) — the same byte the Enter key
    // sends — so pasted lines are submitted/inserted consistently with typing.
    // (Sending LF instead misbehaves in raw-mode apps that distinguish CR from
    // Ctrl+J.) The PTY's line discipline maps CR→NL for cooked-mode shells.
    let normalized: String = text.replace("\r\n", "\r").replace('\n', "\r");
    if mode.bracketed_paste {
        let mut out = Vec::with_capacity(normalized.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        // Strip the paste-end marker if it somehow appears in the payload, so a
        // malicious clipboard can't break out of bracketed paste.
        let safe = normalized.replace("\x1b[201~", "");
        out.extend_from_slice(safe.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        normalized.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> TermMode {
        TermMode::default()
    }
    fn app_cursor() -> TermMode {
        TermMode {
            app_cursor: true,
            ..TermMode::default()
        }
    }
    const NONE: Modifiers = Modifiers::NONE;
    const CTRL: Modifiers = Modifiers::CTRL;

    fn enc(key: Key, mods: Modifiers, text: Option<&str>, mode: TermMode) -> Vec<u8> {
        encode_key(key, mods, text, mode, InputConfig::default()).unwrap()
    }

    #[test]
    fn plain_letter_uses_text() {
        assert_eq!(enc(Key::A, NONE, Some("a"), m()), b"a");
        assert_eq!(enc(Key::A, Modifiers::SHIFT, Some("A"), m()), b"A");
    }

    #[test]
    fn ctrl_letters_are_control_codes() {
        assert_eq!(enc(Key::C, CTRL, None, m()), vec![0x03]); // SIGINT
        assert_eq!(enc(Key::A, CTRL, None, m()), vec![0x01]);
        assert_eq!(enc(Key::Z, CTRL, None, m()), vec![0x1a]);
        assert_eq!(enc(Key::D, CTRL, None, m()), vec![0x04]); // EOF
    }

    #[test]
    fn enter_backspace_tab_escape() {
        assert_eq!(enc(Key::Enter, NONE, None, m()), b"\r");
        assert_eq!(enc(Key::Backspace, NONE, None, m()), vec![0x7f]);
        assert_eq!(enc(Key::Backspace, CTRL, None, m()), vec![0x08]);
        assert_eq!(enc(Key::Tab, NONE, None, m()), b"\t");
        assert_eq!(enc(Key::Tab, Modifiers::SHIFT, None, m()), b"\x1b[Z");
        assert_eq!(enc(Key::Escape, NONE, None, m()), vec![0x1b]);
    }

    #[test]
    fn arrows_switch_on_app_cursor_mode() {
        assert_eq!(enc(Key::ArrowUp, NONE, None, m()), b"\x1b[A");
        assert_eq!(enc(Key::ArrowUp, NONE, None, app_cursor()), b"\x1bOA");
        assert_eq!(enc(Key::ArrowLeft, NONE, None, m()), b"\x1b[D");
        assert_eq!(enc(Key::ArrowLeft, NONE, None, app_cursor()), b"\x1bOD");
    }

    #[test]
    fn modified_arrows_use_csi_with_modifier_param() {
        // Ctrl+Right = ESC [ 1 ; 5 C  (5 = 1 + ctrl(4))
        assert_eq!(enc(Key::ArrowRight, CTRL, None, m()), b"\x1b[1;5C");
        // Even in app-cursor mode, a modified arrow uses the CSI form.
        assert_eq!(enc(Key::ArrowRight, CTRL, None, app_cursor()), b"\x1b[1;5C");
        // Shift+Up = ESC [ 1 ; 2 A
        assert_eq!(enc(Key::ArrowUp, Modifiers::SHIFT, None, m()), b"\x1b[1;2A");
    }

    #[test]
    fn nav_cluster_tilde_sequences() {
        assert_eq!(enc(Key::Delete, NONE, None, m()), b"\x1b[3~");
        assert_eq!(enc(Key::Insert, NONE, None, m()), b"\x1b[2~");
        assert_eq!(enc(Key::PageUp, NONE, None, m()), b"\x1b[5~");
        assert_eq!(enc(Key::PageDown, NONE, None, m()), b"\x1b[6~");
        // Home/End default CSI form.
        assert_eq!(enc(Key::Home, NONE, None, m()), b"\x1b[H");
        assert_eq!(enc(Key::End, NONE, None, m()), b"\x1b[F");
        assert_eq!(enc(Key::Home, NONE, None, app_cursor()), b"\x1bOH");
    }

    #[test]
    fn function_keys() {
        assert_eq!(enc(Key::F1, NONE, None, m()), b"\x1bOP");
        assert_eq!(enc(Key::F4, NONE, None, m()), b"\x1bOS");
        assert_eq!(enc(Key::F5, NONE, None, m()), b"\x1b[15~");
        assert_eq!(enc(Key::F10, NONE, None, m()), b"\x1b[21~");
        assert_eq!(enc(Key::F12, NONE, None, m()), b"\x1b[24~");
        // Modified F-key: Shift+F5 = ESC [ 15 ; 2 ~
        assert_eq!(enc(Key::F5, Modifiers::SHIFT, None, m()), b"\x1b[15;2~");
        // Modified PF key: Ctrl+F1 = ESC [ 1 ; 5 P
        assert_eq!(enc(Key::F1, CTRL, None, m()), b"\x1b[1;5P");
    }

    #[test]
    fn alt_metafies_as_escape_prefix() {
        assert_eq!(enc(Key::A, Modifiers::ALT, Some("a"), m()), b"\x1ba");
        // Alt disabled → plain text (OS composes).
        let no_meta = InputConfig {
            alt_sends_escape: false,
        };
        assert_eq!(
            encode_key(Key::A, Modifiers::ALT, Some("a"), m(), no_meta).unwrap(),
            b"a"
        );
    }

    #[test]
    fn super_chords_are_not_forwarded() {
        assert_eq!(
            encode_key(Key::C, Modifiers::SUPER, None, m(), InputConfig::default()),
            None
        );
    }

    #[test]
    fn ctrl_space_is_nul() {
        assert_eq!(enc(Key::Space, CTRL, None, m()), vec![0x00]);
    }

    #[test]
    fn bracketed_paste_wraps_and_sanitizes() {
        let mode = TermMode {
            bracketed_paste: true,
            ..TermMode::default()
        };
        let out = encode_paste("ls\r\n-la", mode);
        // Line breaks normalise to CR (what Enter sends), not LF.
        assert_eq!(out, b"\x1b[200~ls\r-la\x1b[201~");
        // Plain mode: CRLF and lone LF both become CR, no markers.
        assert_eq!(encode_paste("a\r\nb", TermMode::default()), b"a\rb");
        assert_eq!(encode_paste("a\nb", TermMode::default()), b"a\rb");
        // A payload trying to smuggle the end marker is stripped.
        let attack = encode_paste("x\x1b[201~rm -rf", mode);
        assert_eq!(attack, b"\x1b[200~xrm -rf\x1b[201~");
    }
}
