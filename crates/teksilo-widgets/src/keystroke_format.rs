// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Platform- and locale-aware formatting for [`KeyStroke`]s.
//!
//! - On macOS, the returned string uses the traditional symbol
//!   modifiers (⌘⇧⌥⌃) and Unicode key glyphs (↑↩⇥ …). These symbols
//!   are universal, so no locale lookup is needed.
//! - On Windows / Linux, the modifier labels and named-key labels
//!   (Enter, Esc, Space, arrows, Home/End, PageUp/Down, …) flow
//!   through `tr_widget!` (teksilo-i18n's compile-time-checked
//!   translation macro) so apps that register teksilo-widgets'
//!   framework locales see "Strg+Eingabe" in German, "Ctrl+Entrée"
//!   in French, "Ctrl+Enter" in English, etc. Letters, digits,
//!   character keys and F1..F24 fall through to `Key::Display`:
//!   those names are universal.
//!
//! [`format_keystroke`] gives the whole chord as one label;
//! [`keystroke_parts`] gives one label per key, for a surface that draws
//! each key on its own key cap. Both come from the same per-key labels, so
//! they cannot disagree.

use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::KeyStroke;
#[cfg(not(target_os = "macos"))]
use teksilo_i18n::tr_widget;

/// Render `keystroke` as the conventional label for the current
/// platform and active locale.
pub fn format_keystroke(keystroke: KeyStroke) -> String {
    let parts = keystroke_parts(keystroke);
    // macOS writes a chord as one run of glyphs (`⇧⌘Z`); everywhere else
    // the keys are joined by the locale's separator (`Ctrl+Shift+Z`).
    #[cfg(target_os = "macos")]
    {
        parts.concat()
    }
    #[cfg(not(target_os = "macos"))]
    {
        parts.join(&tr_widget!(keystroke_separator()).resolve_now())
    }
}

/// The labels of the keys in `keystroke`, modifiers first in the
/// platform's order, the main key last: `["Ctrl", "Shift", "Z"]`, or
/// `["⇧", "⌘", "Z"]` on macOS.
///
/// For a surface that draws one key cap per key. Joining the parts the
/// platform's way gives exactly [`format_keystroke`].
pub fn keystroke_parts(keystroke: KeyStroke) -> Vec<String> {
    let mut parts = modifier_labels(keystroke.modifiers);
    parts.push(key_label(keystroke.key));
    parts
}

/// The labels of the two modifiers a shortcut usually starts with on this
/// platform: `("Ctrl", "Alt")` on Windows and Linux, `("⌘", "⌥")` on macOS.
///
/// For a message telling the user which key to add to a chord.
pub(crate) fn command_modifier_labels() -> (String, String) {
    #[cfg(target_os = "macos")]
    {
        ("\u{2318}".to_string(), "\u{2325}".to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        (
            tr_widget!(keystroke_modifier_ctrl()).resolve_now(),
            tr_widget!(keystroke_modifier_alt()).resolve_now(),
        )
    }
}

#[cfg(target_os = "macos")]
fn modifier_labels(modifiers: Modifiers) -> Vec<String> {
    // macOS HIG order: Ctrl(⌃) Option(⌥) Shift(⇧) Command(⌘).
    // No localization: these symbols are universal.
    let mut out = Vec::new();
    if modifiers.ctrl() {
        out.push("\u{2303}".to_string());
    }
    if modifiers.alt() {
        out.push("\u{2325}".to_string());
    }
    if modifiers.shift() {
        out.push("\u{21E7}".to_string());
    }
    if modifiers.super_key() {
        out.push("\u{2318}".to_string());
    }
    out
}

#[cfg(not(target_os = "macos"))]
fn modifier_labels(modifiers: Modifiers) -> Vec<String> {
    // Modifier labels come from framework locale bundles
    // (`crates/teksilo-widgets/locales/*.ftl`) so apps get "Strg+S" in
    // German, "Ctrl+S" in English, etc. The macro validates these keys at
    // compile time.
    let mut out = Vec::new();
    if modifiers.ctrl() {
        out.push(tr_widget!(keystroke_modifier_ctrl()).resolve_now());
    }
    if modifiers.shift() {
        out.push(tr_widget!(keystroke_modifier_shift()).resolve_now());
    }
    if modifiers.alt() {
        out.push(tr_widget!(keystroke_modifier_alt()).resolve_now());
    }
    if modifiers.super_key() {
        out.push(tr_widget!(keystroke_modifier_super()).resolve_now());
    }
    out
}

/// The label of one key, as [`keystroke_parts`] writes it.
#[cfg(target_os = "macos")]
pub(crate) fn key_label(key: Key) -> String {
    match key {
        Key::ArrowUp => "\u{2191}".to_string(),
        Key::ArrowDown => "\u{2193}".to_string(),
        Key::ArrowLeft => "\u{2190}".to_string(),
        Key::ArrowRight => "\u{2192}".to_string(),
        Key::Enter => "\u{21A9}".to_string(),
        Key::Backspace => "\u{232B}".to_string(),
        Key::Delete => "\u{2326}".to_string(),
        Key::Escape => "\u{238B}".to_string(),
        Key::Space => "Space".to_string(),
        Key::Tab => "\u{21E5}".to_string(),
        other => other.to_string(),
    }
}

/// The label of one key, as [`keystroke_parts`] writes it.
#[cfg(not(target_os = "macos"))]
pub(crate) fn key_label(key: Key) -> String {
    // Named-key labels come from framework locale bundles
    // (`crates/teksilo-widgets/locales/*.ftl`) so apps see "Entrée"
    // in French, "Enter" in English, etc. Letters, digits, the
    // `Character(_)` catch-all and F1..F24 use `Key::Display`:
    // those names are universal across Latin-script locales.
    let label = match key {
        Key::Space => Some(tr_widget!(keystroke_key_space()).resolve_now()),
        Key::Enter => Some(tr_widget!(keystroke_key_enter()).resolve_now()),
        Key::Escape => Some(tr_widget!(keystroke_key_escape()).resolve_now()),
        Key::Tab => Some(tr_widget!(keystroke_key_tab()).resolve_now()),
        Key::Backspace => Some(tr_widget!(keystroke_key_backspace()).resolve_now()),
        Key::Delete => Some(tr_widget!(keystroke_key_delete()).resolve_now()),
        Key::ArrowUp => Some(tr_widget!(keystroke_key_arrow_up()).resolve_now()),
        Key::ArrowDown => Some(tr_widget!(keystroke_key_arrow_down()).resolve_now()),
        Key::ArrowLeft => Some(tr_widget!(keystroke_key_arrow_left()).resolve_now()),
        Key::ArrowRight => Some(tr_widget!(keystroke_key_arrow_right()).resolve_now()),
        Key::Home => Some(tr_widget!(keystroke_key_home()).resolve_now()),
        Key::End => Some(tr_widget!(keystroke_key_end()).resolve_now()),
        Key::PageUp => Some(tr_widget!(keystroke_key_page_up()).resolve_now()),
        Key::PageDown => Some(tr_widget!(keystroke_key_page_down()).resolve_now()),
        _ => None,
    };
    label.unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn non_macos_uses_translated_modifiers() {
        // Default locale is the source language (en-US) unless an
        // app installs teksilo-widgets' framework locales with a
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
    #[cfg(not(target_os = "macos"))]
    fn non_macos_named_keys_route_through_bundle() {
        // Named keys flow through `tr_widget!`. Asserting against the
        // English source confirms the bundle keys exist and the match
        // arms cover the named-key variants.
        assert_eq!(format_keystroke(KeyStroke::ctrl(Key::Enter)), "Ctrl+Enter");
        assert_eq!(
            format_keystroke(KeyStroke::new(Key::Escape, Modifiers::NONE)),
            "Esc"
        );
        assert_eq!(
            format_keystroke(KeyStroke::ctrl(Key::ArrowLeft)),
            "Ctrl+Left"
        );
        assert_eq!(
            format_keystroke(KeyStroke::new(Key::Tab, Modifiers::SHIFT)),
            "Shift+Tab"
        );
    }

    #[test]
    fn parts_are_the_keys_of_the_formatted_chord() {
        let ks = KeyStroke::new(Key::Z, Modifiers::CTRL | Modifiers::SHIFT);
        let parts = keystroke_parts(ks);
        assert_eq!(parts.len(), 3, "two modifiers, one key: {parts:?}");
        assert_eq!(parts.last().map(String::as_str), Some("Z"));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(parts.join("+"), format_keystroke(ks));
        #[cfg(target_os = "macos")]
        assert_eq!(parts.concat(), format_keystroke(ks));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_uses_symbols() {
        assert_eq!(
            format_keystroke(KeyStroke::new(Key::S, Modifiers::SUPER)),
            "\u{2318}S"
        );
        assert_eq!(
            format_keystroke(KeyStroke::new(Key::Z, Modifiers::SHIFT | Modifiers::SUPER)),
            "\u{21E7}\u{2318}Z"
        );
    }
}
