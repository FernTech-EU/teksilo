// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The standard edit commands every widget tree binds to a rebindable chord.
//!
//! Cut, copy, paste, undo, redo and select-all are reached through the
//! registry like any other shortcut, so a user can rebind them, or give them a
//! second chord, in a settings UI. [`WidgetTree::new`](crate::WidgetTree::new)
//! registers one [`Global`](super::ShortcutScope::Global) [`Shortcut`] per
//! [`StandardEdit`], under the id [`StandardEdit::id`]. When the chord fires,
//! the intent of that name walks from the focused widget to the root:
//!
//! - an editor registers an [`Action`](crate::action::Action) for each id it
//!   implements, on its own node, so the focused editor runs the command;
//! - an application handles a command where no editor has focus by
//!   registering an `Action` for the same id higher up (on its root, or with
//!   `register_action_global`);
//! - when no `Action` handles the intent, the key reaches the focused widget
//!   as a plain `KeyDown`, exactly as it would with no shortcut bound.
//!
//! An application that used to bind Ctrl+C to a shortcut of its own should
//! handle `edit.copy` instead: two shortcuts on one chord conflict.

use super::{KeyStroke, Shortcut};
use crate::event::Key;

/// The settings-UI category every standard edit shortcut belongs to.
pub const STANDARD_EDIT_CATEGORY: &str = "edit";

/// One of the standard edit commands. Its [`id`](Self::id) is both the id of
/// its shortcut in the registry and the name of the intent it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StandardEdit {
    /// `edit.cut`, Ctrl+X (⌘X on macOS).
    Cut,
    /// `edit.copy`, Ctrl+C (⌘C on macOS).
    Copy,
    /// `edit.paste`, Ctrl+V (⌘V on macOS).
    Paste,
    /// `edit.paste_plain`, Ctrl+Shift+V (⇧⌘V on macOS): paste without the
    /// clipboard's formatting. An editor with no formatting pastes as usual.
    PastePlain,
    /// `edit.undo`, Ctrl+Z (⌘Z on macOS).
    Undo,
    /// `edit.redo`, Ctrl+Shift+Z and Ctrl+Y (⇧⌘Z and ⌘Y on macOS). Ctrl+Y is
    /// the primary chord on Windows, Ctrl+Shift+Z everywhere else.
    Redo,
    /// `edit.select_all`, Ctrl+A (⌘A on macOS).
    SelectAll,
}

impl StandardEdit {
    /// Every standard edit command, in the order a settings UI lists them.
    pub const ALL: [StandardEdit; 7] = [
        StandardEdit::Undo,
        StandardEdit::Redo,
        StandardEdit::Cut,
        StandardEdit::Copy,
        StandardEdit::Paste,
        StandardEdit::PastePlain,
        StandardEdit::SelectAll,
    ];

    /// The shortcut id, which is also the intent name.
    pub const fn id(self) -> &'static str {
        match self {
            StandardEdit::Cut => "edit.cut",
            StandardEdit::Copy => "edit.copy",
            StandardEdit::Paste => "edit.paste",
            StandardEdit::PastePlain => "edit.paste_plain",
            StandardEdit::Undo => "edit.undo",
            StandardEdit::Redo => "edit.redo",
            StandardEdit::SelectAll => "edit.select_all",
        }
    }

    /// The command whose [`id`](Self::id) is `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    /// The shortcut a widget tree registers for this command: its default
    /// chords, an English name, and the [`STANDARD_EDIT_CATEGORY`] category.
    ///
    /// The name and category label are English because this crate carries
    /// no translations; `teksilo-app` replaces them with localized ones in
    /// every window it opens.
    pub fn shortcut(self) -> Shortcut {
        let (primary, secondary) = self.default_chords();
        let builder = Shortcut::new(self.id())
            .name(self.english_name())
            .category(STANDARD_EDIT_CATEGORY)
            .category_label("Edit")
            .primary(primary);
        match secondary {
            Some(secondary) => builder.secondary(secondary).build(),
            None => builder.build(),
        }
    }

    fn english_name(self) -> &'static str {
        match self {
            StandardEdit::Cut => "Cut",
            StandardEdit::Copy => "Copy",
            StandardEdit::Paste => "Paste",
            StandardEdit::PastePlain => "Paste Unformatted",
            StandardEdit::Undo => "Undo",
            StandardEdit::Redo => "Redo",
            StandardEdit::SelectAll => "Select All",
        }
    }

    /// Declared as `Ctrl` chords, which the registry reads as the platform
    /// accelerator (⌘ on macOS).
    fn default_chords(self) -> (KeyStroke, Option<KeyStroke>) {
        match self {
            StandardEdit::Cut => (KeyStroke::ctrl(Key::X), None),
            StandardEdit::Copy => (KeyStroke::ctrl(Key::C), None),
            StandardEdit::Paste => (KeyStroke::ctrl(Key::V), None),
            StandardEdit::PastePlain => (KeyStroke::ctrl_shift(Key::V), None),
            StandardEdit::Undo => (KeyStroke::ctrl(Key::Z), None),
            StandardEdit::Redo if cfg!(target_os = "windows") => {
                (KeyStroke::ctrl(Key::Y), Some(KeyStroke::ctrl_shift(Key::Z)))
            }
            StandardEdit::Redo => (KeyStroke::ctrl_shift(Key::Z), Some(KeyStroke::ctrl(Key::Y))),
            StandardEdit::SelectAll => (KeyStroke::ctrl(Key::A), None),
        }
    }
}

/// Register the default shortcut of every [`StandardEdit`] command. User
/// overrides already in `registry` are kept.
pub(crate) fn register_standard_shortcuts(registry: &mut super::ShortcutRegistry) {
    for command in StandardEdit::ALL {
        registry.register(command.shortcut());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Modifiers;
    use crate::shortcut::ShortcutRegistry;

    #[test]
    fn ids_round_trip_and_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for command in StandardEdit::ALL {
            assert!(seen.insert(command.id()), "{} listed twice", command.id());
            assert_eq!(StandardEdit::from_id(command.id()), Some(command));
            assert_eq!(command.shortcut().id, command.id());
        }
        assert_eq!(StandardEdit::from_id("edit.nothing"), None);
    }

    #[test]
    fn defaults_cover_the_chords_the_editors_used_to_hardcode() {
        let mut registry = ShortcutRegistry::new();
        register_standard_shortcuts(&mut registry);
        let fires = |key: Key, modifiers: Modifiers| {
            registry
                .find_by_keystroke(KeyStroke::new(key, modifiers))
                .map(|s| s.shortcut.id)
        };
        let accel = Modifiers::COMMAND;
        let accel_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(fires(Key::X, accel), Some("edit.cut"));
        assert_eq!(fires(Key::C, accel), Some("edit.copy"));
        assert_eq!(fires(Key::V, accel), Some("edit.paste"));
        assert_eq!(fires(Key::V, accel_shift), Some("edit.paste_plain"));
        assert_eq!(fires(Key::Z, accel), Some("edit.undo"));
        assert_eq!(fires(Key::Z, accel_shift), Some("edit.redo"));
        assert_eq!(fires(Key::Y, accel), Some("edit.redo"));
        assert_eq!(fires(Key::A, accel), Some("edit.select_all"));
    }

    #[test]
    fn a_secondary_binding_fires_beside_the_default() {
        let mut registry = ShortcutRegistry::new();
        register_standard_shortcuts(&mut registry);
        // A Bépo user keeping Ctrl+Z on the Z key and adding the chord at
        // QWERTY's Z position, which types `à` there.
        let positional = KeyStroke::new(Key::Character('à'), Modifiers::CTRL);
        registry.rebind_secondary("edit.undo", Some(positional));
        let effective = registry.effective("edit.undo").unwrap();
        assert!(effective.matches(KeyStroke::new(Key::Z, Modifiers::COMMAND)));
        assert!(effective.matches(positional));
    }
}
