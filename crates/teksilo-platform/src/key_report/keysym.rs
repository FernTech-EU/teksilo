// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! winit keys in X11 terms: keysym, keycode, modifier mask, text.
//!
//! winit hands over what a key *means* (`Key`), not the X keysym it was
//! decoded from, so the keysym is recovered: named keys through the table
//! below (the inverse of winit's own `keysym_to_key`), characters as their
//! Latin-1 keysym or `0x1000000 +` their code point, dead keys through the
//! character winit gives a dead key. A keysym winit has no name for arrives
//! raw (`NativeKey::Xkb`) and passes through. What cannot be recovered is
//! `VoidSymbol`, which every reader names (a keysym of 0 has no name, and
//! Orca 46's `KeyboardEvent` cannot handle a key without one).
//!
//! Some characters also have a legacy keysym (`Cyrillic_a` beside
//! `U0430`); a layout may use either, and both decode to the same character.
//! A reader takes the character from the event's text in any case.

use winit::keyboard::{Key, KeyLocation, NamedKey, NativeKey, PhysicalKey};

use super::KeyboardState;

/// `XK_VoidSymbol`: a key with no keysym.
pub(super) const VOID_SYMBOL: u32 = 0x00ff_ffff;

/// X modifier masks (`X.h`).
const SHIFT_MASK: u32 = 1 << 0;
const LOCK_MASK: u32 = 1 << 1;
const CONTROL_MASK: u32 = 1 << 2;
const MOD1_MASK: u32 = 1 << 3;
const MOD2_MASK: u32 = 1 << 4;
const MOD4_MASK: u32 = 1 << 6;

/// The X keysym of `key`, pressed at `location`, with Shift held or not.
pub(super) fn keysym(key: &Key, location: KeyLocation, shift: bool) -> u32 {
    match key {
        Key::Named(named) => named_keysym(*named, location, shift),
        Key::Character(text) => text
            .chars()
            .next()
            .map_or(VOID_SYMBOL, |ch| character_keysym(ch, location)),
        Key::Dead(ch) => ch.and_then(dead_keysym).unwrap_or(VOID_SYMBOL),
        Key::Unidentified(NativeKey::Xkb(raw)) => *raw,
        Key::Unidentified(_) => VOID_SYMBOL,
    }
}

fn named_keysym(key: NamedKey, location: KeyLocation, shift: bool) -> u32 {
    let keypad = location == KeyLocation::Numpad;
    let right = location == KeyLocation::Right;
    let pick = |main: u32, kp: u32| if keypad { kp } else { main };
    let side = |left: u32, right_side: u32| if right { right_side } else { left };
    // Function keys: F1 to F35 are consecutive, and the keypad has its own
    // F1 to F4.
    if let Some(n) = function_key_number(key) {
        return if keypad && n <= 4 {
            0xff91 + n - 1
        } else {
            0xffbe + n - 1
        };
    }
    match key {
        // TTY function keys.
        NamedKey::Backspace => 0xff08,
        // Shift+Tab is `ISO_Left_Tab` on every xkb layout, and winit folds
        // both into `Tab`.
        NamedKey::Tab if keypad => 0xff89,
        NamedKey::Tab if shift => 0xfe20,
        NamedKey::Tab => 0xff09,
        NamedKey::Clear => 0xff0b,
        NamedKey::Enter => pick(0xff0d, 0xff8d),
        NamedKey::Pause => 0xff13,
        NamedKey::ScrollLock => 0xff14,
        NamedKey::Escape => 0xff1b,
        NamedKey::Delete => pick(0xffff, 0xff9f),
        // Input method keys.
        NamedKey::Compose => 0xff20,
        NamedKey::KanjiMode => 0xff21,
        NamedKey::NonConvert => 0xff22,
        NamedKey::Convert => 0xff23,
        NamedKey::Romaji => 0xff24,
        NamedKey::Hiragana => 0xff25,
        NamedKey::HiraganaKatakana => 0xff27,
        NamedKey::Zenkaku => 0xff28,
        NamedKey::Hankaku => 0xff29,
        NamedKey::ZenkakuHankaku => 0xff2a,
        NamedKey::KanaMode => 0xff2d,
        NamedKey::Alphanumeric => 0xff30,
        NamedKey::CodeInput => 0xff37,
        NamedKey::SingleCandidate => 0xff3c,
        NamedKey::AllCandidates => 0xff3d,
        NamedKey::PreviousCandidate => 0xff3e,
        // Cursor control and motion.
        NamedKey::Home => pick(0xff50, 0xff95),
        NamedKey::ArrowLeft => pick(0xff51, 0xff96),
        NamedKey::ArrowUp => pick(0xff52, 0xff97),
        NamedKey::ArrowRight => pick(0xff53, 0xff98),
        NamedKey::ArrowDown => pick(0xff54, 0xff99),
        NamedKey::PageUp => pick(0xff55, 0xff9a),
        NamedKey::PageDown => pick(0xff56, 0xff9b),
        NamedKey::End => pick(0xff57, 0xff9c),
        // Miscellaneous functions.
        NamedKey::Select => 0xff60,
        NamedKey::PrintScreen => 0xff61,
        NamedKey::Execute => 0xff62,
        NamedKey::Insert => pick(0xff63, 0xff9e),
        NamedKey::Undo => 0xff65,
        NamedKey::Redo => 0xff66,
        NamedKey::ContextMenu => 0xff67,
        NamedKey::Find => 0xff68,
        NamedKey::Cancel => 0xff69,
        NamedKey::Help => 0xff6a,
        NamedKey::ModeChange => 0xff7e,
        NamedKey::NumLock => 0xff7f,
        // Modifiers.
        NamedKey::Shift => side(0xffe1, 0xffe2),
        NamedKey::Control => side(0xffe3, 0xffe4),
        NamedKey::CapsLock => 0xffe5,
        NamedKey::Meta => side(0xffe7, 0xffe8),
        NamedKey::Alt => side(0xffe9, 0xffea),
        NamedKey::Super => side(0xffeb, 0xffec),
        NamedKey::Hyper => side(0xffed, 0xffee),
        NamedKey::AltGraph => 0xfe03,
        NamedKey::GroupNext => 0xfe08,
        NamedKey::GroupPrevious => 0xfe0a,
        NamedKey::GroupFirst => 0xfe0c,
        NamedKey::GroupLast => 0xfe0e,
        NamedKey::Space => 0x20,
        _ => VOID_SYMBOL,
    }
}

/// `n` for `NamedKey::Fn`, 1 to 35.
fn function_key_number(key: NamedKey) -> Option<u32> {
    const KEYS: [NamedKey; 35] = [
        NamedKey::F1,
        NamedKey::F2,
        NamedKey::F3,
        NamedKey::F4,
        NamedKey::F5,
        NamedKey::F6,
        NamedKey::F7,
        NamedKey::F8,
        NamedKey::F9,
        NamedKey::F10,
        NamedKey::F11,
        NamedKey::F12,
        NamedKey::F13,
        NamedKey::F14,
        NamedKey::F15,
        NamedKey::F16,
        NamedKey::F17,
        NamedKey::F18,
        NamedKey::F19,
        NamedKey::F20,
        NamedKey::F21,
        NamedKey::F22,
        NamedKey::F23,
        NamedKey::F24,
        NamedKey::F25,
        NamedKey::F26,
        NamedKey::F27,
        NamedKey::F28,
        NamedKey::F29,
        NamedKey::F30,
        NamedKey::F31,
        NamedKey::F32,
        NamedKey::F33,
        NamedKey::F34,
        NamedKey::F35,
    ];
    KEYS.iter()
        .position(|&k| k == key)
        .map(|index| index as u32 + 1)
}

fn character_keysym(ch: char, location: KeyLocation) -> u32 {
    if location == KeyLocation::Numpad
        && let Some(keypad) = keypad_keysym(ch)
    {
        return keypad;
    }
    let code = ch as u32;
    match code {
        // Latin-1 keysyms are the code point itself.
        0x20..=0x7e | 0xa0..=0xff => code,
        // The C0 controls that have a function keysym: BackSpace, Tab,
        // Linefeed, Clear, Return, Escape; and Delete.
        0x08..=0x0b | 0x0d | 0x1b => 0xff00 | code,
        0x7f => 0xffff,
        0x00..=0x1f | 0x80..=0x9f => VOID_SYMBOL,
        _ => 0x0100_0000 + code,
    }
}

/// The keypad's own keysym for a character it types.
fn keypad_keysym(ch: char) -> Option<u32> {
    Some(match ch {
        '0'..='9' => 0xffb0 + (ch as u32 - '0' as u32),
        ' ' => 0xff80,
        '*' => 0xffaa,
        '+' => 0xffab,
        ',' => 0xffac,
        '-' => 0xffad,
        '.' => 0xffae,
        '/' => 0xffaf,
        '=' => 0xffbd,
        _ => return None,
    })
}

/// The dead keysym for the character winit gives a dead key: the character
/// the dead key makes when pressed twice, by xkb's Compose table.
fn dead_keysym(ch: char) -> Option<u32> {
    Some(match ch {
        '`' => 0xfe50,
        '´' => 0xfe51,
        '^' => 0xfe52,
        '~' => 0xfe53,
        '¯' => 0xfe54,
        '˘' => 0xfe55,
        '˙' => 0xfe56,
        '¨' => 0xfe57,
        '°' => 0xfe58,
        '˝' => 0xfe59,
        'ˇ' => 0xfe5a,
        '¸' => 0xfe5b,
        '˛' => 0xfe5c,
        _ => return None,
    })
}

/// The X keycode of a physical key: on Linux winit's scancode is the evdev
/// code, and X and Wayland number keys from evdev + 8. Elsewhere nothing is
/// reported, and this is never asked.
pub(super) fn hardware_keycode(key: PhysicalKey) -> u32 {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::scancode::PhysicalKeyExtScancode;
        key.to_scancode().map_or(0, |scancode| scancode + 8)
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = key;
        0
    }
}

/// The X modifier mask held with a key, Num Lock (Mod2) included when it is
/// known to be on (see [`num_lock_shown`]).
pub(super) fn modifier_mask(keyboard: KeyboardState, num_lock: bool) -> u32 {
    let modifiers = keyboard.modifiers;
    let mut mask = 0;
    if modifiers.shift_key() {
        mask |= SHIFT_MASK;
    }
    if keyboard.caps_lock {
        mask |= LOCK_MASK;
    }
    if modifiers.control_key() {
        mask |= CONTROL_MASK;
    }
    if modifiers.alt_key() {
        mask |= MOD1_MASK;
    }
    if num_lock {
        mask |= MOD2_MASK;
    }
    if modifiers.super_key() {
        mask |= MOD4_MASK;
    }
    mask
}

/// What a key shows of Num Lock: winit knows nothing of the lock, but a
/// keypad key tells. It types a digit or the decimal separator only while
/// Num Lock is on, and moves the caret (KP_Home to KP_Delete, KP_Begin) only
/// while it is off. Every other key shows nothing.
///
/// Num Lock matters to a reader on the keypad: libatspi keeps it significant
/// for the keypad's keycodes (`_atspi_key_is_on_keypad`), so Orca's keypad
/// commands, grabbed with no modifier, take keypad Enter, +, -, * and / from
/// an event that does not say Num Lock is on. The registry turns Mod2 into
/// AT-SPI's Num Lock bit.
pub(super) fn num_lock_shown(key: &Key, location: KeyLocation) -> Option<bool> {
    if location != KeyLocation::Numpad {
        return None;
    }
    match key {
        Key::Character(text) => text
            .chars()
            .next()
            .filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ','))
            .map(|_| true),
        Key::Named(
            NamedKey::Home
            | NamedKey::End
            | NamedKey::ArrowLeft
            | NamedKey::ArrowRight
            | NamedKey::ArrowUp
            | NamedKey::ArrowDown
            | NamedKey::PageUp
            | NamedKey::PageDown
            | NamedKey::Insert
            | NamedKey::Delete
            | NamedKey::Clear,
        ) => Some(false),
        // KP_Begin, the keypad's 5 with Num Lock off; winit has no name for it.
        Key::Unidentified(NativeKey::Xkb(0xff9d)) => Some(false),
        _ => None,
    }
}

/// Whether `key` types something into a text field: a character, a dead key
/// (an accent to come), or Space. What a secure field withholds.
pub(super) fn types_text(key: &Key) -> bool {
    matches!(
        key,
        Key::Character(_) | Key::Dead(_) | Key::Named(NamedKey::Space)
    )
}

/// The text of a key that has printable text; empty for every other key,
/// which a reader then names from its keysym (Orca: `input_event.py`, "Some
/// implementors don't populate this field at all").
///
/// Read from the logical key rather than winit's `text`, which also carries
/// control characters (`\r` for Enter, `\t` for Tab, `\u{8}` for
/// BackSpace). A release is not described from its own key: the gate gives
/// it the keysym and text of its press (see `Hold::named`).
pub(super) fn event_string(key: &Key) -> String {
    match key {
        Key::Character(text) if !text.is_empty() && !text.chars().any(char::is_control) => {
            text.to_string()
        }
        Key::Named(NamedKey::Space) => " ".to_owned(),
        _ => String::new(),
    }
}
