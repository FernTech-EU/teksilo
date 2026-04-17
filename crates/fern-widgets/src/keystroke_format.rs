//! Platform- and locale-aware formatting for [`KeyStroke`]s.
//!
//! - On macOS, the returned string uses the traditional symbol
//!   modifiers (⌘⇧⌥⌃) and Unicode key glyphs (↑↩⇥ …). These symbols
//!   are universal — no locale lookup needed.
//! - On Windows / Linux, the modifier labels flow through
//!   `tr_widget!` (fern-i18n's compile-time-checked translation
//!   macro) so apps that register fern-widgets' framework locales
//!   see "Strg+S" in German, "Ctrl+S" in English, etc. Key names
//!   currently use the plain [`Key::Display`] impl; per-key
//!   localization can be layered on later the same way.
//!
//! Apps that want full control can bypass this function entirely —
//! the settings widget simply calls [`format_keystroke`] at render
//! time, so overriding the visible label means substituting a
//! different formatter at the caller.

use fern_core::event::{Key, Modifiers};
use fern_core::shortcut::KeyStroke;
use fern_i18n::tr_widget;

/// Render `keystroke` as the conventional label for the current
/// platform and active locale.
pub fn format_keystroke(keystroke: KeyStroke) -> String {
    let mut out = String::new();
    write_modifiers(&mut out, keystroke.modifiers);
    write_key(&mut out, keystroke.key);
    out
}

#[cfg(target_os = "macos")]
fn write_modifiers(out: &mut String, modifiers: Modifiers) {
    // macOS HIG order: Ctrl(⌃) Option(⌥) Shift(⇧) Command(⌘).
    // No localization — these symbols are universal.
    if modifiers.ctrl() {
        out.push('\u{2303}');
    }
    if modifiers.alt() {
        out.push('\u{2325}');
    }
    if modifiers.shift() {
        out.push('\u{21E7}');
    }
    if modifiers.super_key() {
        out.push('\u{2318}');
    }
}

#[cfg(not(target_os = "macos"))]
fn write_modifiers(out: &mut String, modifiers: Modifiers) {
    // Modifier labels + separator come from framework locale bundles
    // (`crates/fern-widgets/locales/*.ftl`) so apps get "Strg+S" in
    // German, "Ctrl+S" in English, etc. Resolved once per call via
    // `tr_widget!` — the macro validates these keys at compile time.
    let sep = tr_widget!(keystroke_separator()).resolve_now();
    if modifiers.ctrl() {
        out.push_str(&tr_widget!(keystroke_modifier_ctrl()).resolve_now());
        out.push_str(&sep);
    }
    if modifiers.shift() {
        out.push_str(&tr_widget!(keystroke_modifier_shift()).resolve_now());
        out.push_str(&sep);
    }
    if modifiers.alt() {
        out.push_str(&tr_widget!(keystroke_modifier_alt()).resolve_now());
        out.push_str(&sep);
    }
    if modifiers.super_key() {
        out.push_str(&tr_widget!(keystroke_modifier_super()).resolve_now());
        out.push_str(&sep);
    }
}

#[cfg(target_os = "macos")]
fn write_key(out: &mut String, key: Key) {
    match key {
        Key::ArrowUp => out.push('\u{2191}'),
        Key::ArrowDown => out.push('\u{2193}'),
        Key::ArrowLeft => out.push('\u{2190}'),
        Key::ArrowRight => out.push('\u{2192}'),
        Key::Enter => out.push('\u{21A9}'),
        Key::Backspace => out.push('\u{232B}'),
        Key::Delete => out.push('\u{2326}'),
        Key::Escape => out.push('\u{238B}'),
        Key::Space => out.push_str("Space"),
        Key::Tab => out.push('\u{21E5}'),
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(not(target_os = "macos"))]
fn write_key(out: &mut String, key: Key) {
    out.push_str(&key.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn non_macos_uses_translated_modifiers() {
        // Default locale is the source language (en-US) unless an
        // app installs fern-widgets' framework locales with a
        // different active locale. We can only safely check the
        // English source here.
        assert_eq!(format_keystroke(KeyStroke::ctrl(Key::S)), "Ctrl+S");
        assert_eq!(
            format_keystroke(KeyStroke::ctrl_shift(Key::Z)),
            "Ctrl+Shift+Z"
        );
        assert_eq!(format_keystroke(KeyStroke::alt(Key::F4)), "Alt+F4");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_uses_symbols() {
        assert_eq!(
            format_keystroke(KeyStroke::new(Key::S, Modifiers::SUPER)),
            "\u{2318}S"
        );
        assert_eq!(
            format_keystroke(KeyStroke::new(
                Key::Z,
                Modifiers::SHIFT | Modifiers::SUPER
            )),
            "\u{21E7}\u{2318}Z"
        );
    }
}
