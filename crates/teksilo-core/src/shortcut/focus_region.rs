// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The two navigation shortcuts every widget tree binds: F6 and Shift+F6 move
//! focus to the next and the previous focus region of the window.
//!
//! [`WidgetTree::new`](crate::WidgetTree::new) registers them beside the
//! standard edits, global and with no owner, so they apply in a modal too, and
//! a user can rebind them in a settings UI. The tree runs them itself when no
//! `Action` on the route takes the intent (see
//! [`WidgetBuilder::focus_region`](crate::widget_builder::WidgetBuilder::focus_region)).

use super::{KeyStroke, Shortcut};
use crate::event::{Key, Modifiers};

/// The id, and intent name, of the command that moves focus to the next
/// focus region (F6).
#[doc(hidden)]
pub const NEXT_REGION: &str = "focus.next_region";

/// The id, and intent name, of the command that moves focus to the previous
/// focus region (Shift+F6).
#[doc(hidden)]
pub const PREVIOUS_REGION: &str = "focus.previous_region";

/// The settings-UI category the region shortcuts belong to.
const NAVIGATION_CATEGORY: &str = "navigation";

/// Which way a region command moves (`true` forward), or `None` for any
/// other id.
pub(crate) fn region_direction(id: &str) -> Option<bool> {
    match id {
        NEXT_REGION => Some(true),
        PREVIOUS_REGION => Some(false),
        _ => None,
    }
}

/// The two region shortcuts as the tree registers them: F6 and Shift+F6,
/// English names, the `navigation` category. `teksilo-widgets` gives them
/// localized names in every window `teksilo-app` opens.
#[doc(hidden)]
pub fn focus_region_shortcuts() -> [Shortcut; 2] {
    [
        region_shortcut(NEXT_REGION, "Next Region", Modifiers::NONE),
        region_shortcut(PREVIOUS_REGION, "Previous Region", Modifiers::SHIFT),
    ]
}

fn region_shortcut(id: &'static str, name: &str, modifiers: Modifiers) -> Shortcut {
    Shortcut::new(id)
        .name(name.to_owned())
        .category(NAVIGATION_CATEGORY)
        .category_label("Navigation")
        .primary(KeyStroke::new(Key::F6, modifiers))
        .build()
}

/// The tree's own registration of region shortcut `id`, or `None`.
pub(crate) fn region_default(id: &str) -> Option<Shortcut> {
    region_direction(id)?;
    focus_region_shortcuts()
        .into_iter()
        .find(|shortcut| shortcut.id == id)
}

/// Register both region shortcuts. User overrides already in `registry` are
/// kept.
pub(crate) fn register_focus_region_shortcuts(registry: &mut super::ShortcutRegistry) {
    for shortcut in focus_region_shortcuts() {
        registry.register(shortcut);
    }
}
