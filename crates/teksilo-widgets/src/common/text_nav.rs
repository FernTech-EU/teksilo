// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Which caret motion a modified arrow key means, per platform convention.
//!
//! Desktop platforms do not merely disagree about *which modifier* carries a
//! text-navigation chord — they lay the motions out differently. Windows and
//! Linux put word-jump on `Ctrl+←/→` and line edges on bare `Home`/`End`.
//! macOS uses three distinct modifiers on the arrows themselves: `⌥←/→` for
//! word, `⌘←/→` for the line edge, `⌘↑/↓` for the document, `⌥↑/↓` for the
//! paragraph. So a single "is the accelerator held?" boolean cannot express
//! it: on macOS ⌘← is a *line* motion, not a word one, and word-jump lives on
//! a modifier that means nothing at all on the other two platforms.
//!
//! Hence this module, rather than a per-editor `if ctrl` at each call site.
//! [`caret_step`] and [`line_step`] answer for the current platform;
//! the `_for` variants take the convention explicitly so both branches stay
//! reachable from one host's test run — the same split as
//! [`Modifiers::with_command_convention`](teksilo_core::event::Modifiers::with_command_convention).
//!
//! What this module does **not** decide is `Home`/`End`: those already read
//! correctly from the primary accelerator alone (bare = line edge, accelerator
//! = document) on every platform, so their call sites keep testing
//! [`Modifiers::command`](teksilo_core::event::Modifiers::command) directly.
//!
//! Claiming `⌥`-modified **arrows** is safe even though `⌥`+letter produces
//! diacritics on macOS (Option+E → ´, which the editors' character-insert
//! path deliberately lets through): an arrow key carries no `text` payload,
//! so there is nothing to shadow.

use teksilo_core::event::Modifiers;

/// The desktop text-editing convention a caret chord is read against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TextNavConvention {
    /// macOS: `⌥` for word / paragraph, `⌘` for line / document.
    Mac,
    /// Windows and Linux: `Ctrl` for word, `Home`/`End` for the line edge.
    PcStyle,
}

impl TextNavConvention {
    /// The convention this build targets.
    pub(crate) const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else {
        Self::PcStyle
    };
}

/// How far a `←` / `→` press moves the caret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CaretStep {
    /// One character (or grapheme) — the unmodified arrow.
    Character,
    /// One word.
    Word,
    /// The near or far edge of the line, in the direction pressed.
    ///
    /// Never produced under [`TextNavConvention::PcStyle`], where the line
    /// edges belong to `Home` / `End`.
    LineEdge,
}

/// How far a `↑` / `↓` press moves the caret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LineStep {
    /// One visual line — the unmodified arrow.
    Line,
    /// The start or end of the paragraph (block).
    ///
    /// Never produced under [`TextNavConvention::PcStyle`].
    Paragraph,
    /// The start or end of the document.
    ///
    /// Never produced under [`TextNavConvention::PcStyle`], where the document
    /// edges belong to `Ctrl+Home` / `Ctrl+End`.
    Document,
}

/// What `←` / `→` means with `modifiers` held, on this platform.
pub(crate) fn caret_step(modifiers: Modifiers) -> CaretStep {
    caret_step_for(TextNavConvention::CURRENT, modifiers)
}

/// [`caret_step`] against an explicit convention.
pub(crate) fn caret_step_for(convention: TextNavConvention, modifiers: Modifiers) -> CaretStep {
    match convention {
        // ⌘ wins over ⌥ so ⌥⌘← resolves to the coarser motion rather than
        // falling in a hole — AppKit binds only the single-modifier forms, and
        // a two-modifier arrow should still go somewhere sensible.
        TextNavConvention::Mac => {
            if modifiers.super_key() {
                CaretStep::LineEdge
            } else if modifiers.alt() {
                CaretStep::Word
            } else {
                CaretStep::Character
            }
        }
        TextNavConvention::PcStyle => {
            if modifiers.ctrl() {
                CaretStep::Word
            } else {
                CaretStep::Character
            }
        }
    }
}

/// What `↑` / `↓` means with `modifiers` held, on this platform.
pub(crate) fn line_step(modifiers: Modifiers) -> LineStep {
    line_step_for(TextNavConvention::CURRENT, modifiers)
}

/// [`line_step`] against an explicit convention.
pub(crate) fn line_step_for(convention: TextNavConvention, modifiers: Modifiers) -> LineStep {
    match convention {
        TextNavConvention::Mac => {
            if modifiers.super_key() {
                LineStep::Document
            } else if modifiers.alt() {
                LineStep::Paragraph
            } else {
                LineStep::Line
            }
        }
        // Windows scrolls the view on Ctrl+↑/↓ without moving the caret, which
        // is a viewport concern rather than a motion; nothing to report here.
        TextNavConvention::PcStyle => LineStep::Line,
    }
}

/// Whether `Backspace` / `Delete` with `modifiers` held removes a whole word.
///
/// `⌥⌫` on macOS, `Ctrl+⌫` elsewhere. macOS additionally binds `⌘⌫` to
/// delete-to-line-start, which the editors do not implement — there it falls
/// through to a plain single-character delete rather than doing something
/// larger than the user asked for.
pub(crate) fn deletes_word(modifiers: Modifiers) -> bool {
    deletes_word_for(TextNavConvention::CURRENT, modifiers)
}

/// [`deletes_word`] against an explicit convention.
pub(crate) fn deletes_word_for(convention: TextNavConvention, modifiers: Modifiers) -> bool {
    match convention {
        TextNavConvention::Mac => modifiers.alt() && !modifiers.super_key(),
        TextNavConvention::PcStyle => modifiers.ctrl(),
    }
}

#[cfg(test)]
mod tests {
    use super::TextNavConvention::{Mac, PcStyle};
    use super::*;

    const ALT: Modifiers = Modifiers::ALT;
    const SUPER: Modifiers = Modifiers::SUPER;
    const CTRL: Modifiers = Modifiers::CTRL;
    const SHIFT: Modifiers = Modifiers::SHIFT;

    #[test]
    fn mac_puts_word_on_option_and_the_line_edge_on_command() {
        assert_eq!(caret_step_for(Mac, Modifiers::NONE), CaretStep::Character);
        assert_eq!(caret_step_for(Mac, ALT), CaretStep::Word);
        assert_eq!(caret_step_for(Mac, SUPER), CaretStep::LineEdge);
    }

    #[test]
    fn pc_style_puts_word_on_ctrl_and_owns_no_arrow_line_edge() {
        assert_eq!(
            caret_step_for(PcStyle, Modifiers::NONE),
            CaretStep::Character
        );
        assert_eq!(caret_step_for(PcStyle, CTRL), CaretStep::Word);
        // The line edge is Home / End here, never an arrow.
        assert_eq!(caret_step_for(PcStyle, SUPER), CaretStep::Character);
    }

    #[test]
    fn physical_control_is_not_a_mac_text_chord() {
        // ⌃← belongs to Mission Control, and ⌃A/⌃E to the text system — none
        // of them are word motions, so the editor must not claim them.
        assert_eq!(caret_step_for(Mac, CTRL), CaretStep::Character);
        assert_eq!(line_step_for(Mac, CTRL), LineStep::Line);
        assert!(!deletes_word_for(Mac, CTRL));
    }

    #[test]
    fn shift_only_extends_and_never_changes_the_step() {
        for (convention, mods) in [
            (Mac, ALT),
            (Mac, SUPER),
            (Mac, Modifiers::NONE),
            (PcStyle, CTRL),
            (PcStyle, Modifiers::NONE),
        ] {
            assert_eq!(
                caret_step_for(convention, mods),
                caret_step_for(convention, mods | SHIFT),
                "Shift selects; it must not reinterpret the motion"
            );
            assert_eq!(
                line_step_for(convention, mods),
                line_step_for(convention, mods | SHIFT),
            );
        }
    }

    #[test]
    fn mac_vertical_arrows_reach_the_paragraph_and_the_document() {
        assert_eq!(line_step_for(Mac, Modifiers::NONE), LineStep::Line);
        assert_eq!(line_step_for(Mac, ALT), LineStep::Paragraph);
        assert_eq!(line_step_for(Mac, SUPER), LineStep::Document);
    }

    #[test]
    fn pc_style_vertical_arrows_are_always_one_line() {
        for m in [Modifiers::NONE, CTRL, ALT, SUPER] {
            assert_eq!(line_step_for(PcStyle, m), LineStep::Line);
        }
    }

    #[test]
    fn a_two_modifier_arrow_takes_the_coarser_motion() {
        // ⌥⌘← is bound by nothing in AppKit; resolving it to the line edge
        // beats dropping the keystroke on the floor.
        assert_eq!(caret_step_for(Mac, ALT | SUPER), CaretStep::LineEdge);
        assert_eq!(line_step_for(Mac, ALT | SUPER), LineStep::Document);
    }

    #[test]
    fn delete_word_follows_the_same_modifier_as_word_motion() {
        assert!(deletes_word_for(Mac, ALT));
        assert!(!deletes_word_for(Mac, Modifiers::NONE));
        assert!(deletes_word_for(PcStyle, CTRL));
        assert!(!deletes_word_for(PcStyle, Modifiers::NONE));
        assert!(!deletes_word_for(PcStyle, ALT));
    }

    #[test]
    fn command_backspace_on_mac_is_not_a_word_delete() {
        // ⌘⌫ means delete-to-line-start, which is not implemented. Deleting a
        // word instead would remove text the user did not ask to lose, so the
        // chord falls through to a plain single-character delete.
        assert!(!deletes_word_for(Mac, SUPER));
        assert!(!deletes_word_for(Mac, ALT | SUPER));
    }

    #[test]
    fn the_current_convention_matches_the_build_target() {
        if cfg!(target_os = "macos") {
            assert_eq!(TextNavConvention::CURRENT, Mac);
            assert_eq!(caret_step(ALT), CaretStep::Word);
        } else {
            assert_eq!(TextNavConvention::CURRENT, PcStyle);
            assert_eq!(caret_step(CTRL), CaretStep::Word);
        }
    }
}
