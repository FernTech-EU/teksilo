// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ShortcutSettings: the user-facing panel for browsing and rebinding an
//! application's shortcuts.
//!
//! It lists every shortcut in the tree's
//! [`ShortcutRegistry`](teksilo_core::shortcut::ShortcutRegistry), grouped
//! by category, one row per command:
//!
//! ```text
//!  [ Search commands or keys         ] [⌨]  [ ] Modified only  [Reset all…]
//!
//!  File
//!     New project                 [Ctrl] [N]          +
//!     Open…                       [Ctrl] [O]          +
//!   ● Save                        [Ctrl] [Shift] [S]  [Ctrl] [S]   ↺
//!  Format
//!     Bold                        ( Press keys… )     +
//!        Press the new shortcut. Esc cancels, Del removes it.
//! ```
//!
//! - **The chord is the control.** Each chord is drawn as key caps; click
//!   it (or focus it and press Enter or Space) to record a new one. The
//!   first column is the shortcut, the second its alternative (`+` adds
//!   one). Delete or Backspace on a focused chord removes it.
//! - **Recording** shows "Press keys…" in a field the width of its column,
//!   so the list never reflows. Escape or Tab cancels; Delete or Backspace
//!   removes the chord; leaving the field cancels too. A key that cannot be
//!   a shortcut is refused with the reason, and recording goes on: a
//!   printable key needs Ctrl, Alt or ⌘ (Shift alone types a capital), and
//!   chords the system keeps (Alt+F4, Alt+Tab, ⌘Tab, ⌘Space, …) and
//!   Ctrl+Tab (the way out of a control that keeps Tab) are reserved.
//! - **Conflicts are confirmed.** A chord another command already uses
//!   is not taken from it until the user presses "Reassign" on the line
//!   that appears under the row. [`confirm_conflicts(false)`] reassigns at
//!   once instead; either way [`on_conflict`] is told.
//! - **Changed rows are marked** with a dot and get a reset button, which
//!   puts that command's declared defaults back. "Reset all…" in the
//!   toolbar does it for every command, after a confirmation.
//! - **Search** matches a command's name, its group, its description and
//!   the text of its chords ("ctrl+s" finds Save). The keyboard button
//!   next to the field finds by chord instead: press it, press the keys,
//!   and the list shows the commands that use exactly that chord.
//!   "Modified only" narrows the list to the commands the user changed.
//!
//! Commands are sorted by name within a group and groups by their title,
//! both in the interface language. Every visible string comes from the
//! framework's locale bundles.
//!
//! **Keyboard.** The list is one Tab stop. The arrow keys move between rows
//! and between a row's chord, alternative and reset button; Home and End go
//! to the first and last row. Every control carries its command in its
//! accessible name ("Save: Ctrl+S"), its description and whether the user
//! changed it in its description, and the group titles are headings.
//!
//! The widget owns the armed key capture. Dropping the widget cancels it,
//! so navigating away mid-recording cannot rebind the next key pressed
//! somewhere else.
//!
//! The list grows with its content and does not scroll; place the widget
//! in a `ScrollArea` when there are more shortcuts than fit.
//!
//! ```ignore
//! // Inside a settings Dialog build():
//! ctx.add(
//!     ShortcutSettings::new()
//!         .on_conflict(|c| println!("{} loses its shortcut", c.displaced_name)),
//! );
//! ```
//!
//! [`confirm_conflicts(false)`]: ShortcutSettings::confirm_conflicts
//! [`on_conflict`]: ShortcutSettings::on_conflict

mod controls;
mod state;
mod table;
#[cfg(test)]
mod tests;

use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::tr_widget;

use self::state::{FocusKey, Shared, start_finding};
use self::table::ShortcutTable;
use crate::button::Button;
use crate::checkbox::Checkbox;
use crate::icon_button::{BuiltInIcons, IconButton};
use crate::primitives::{FixedSize, HStack, Shrinkable, Spacer, VStack};
use crate::search_field::SearchField;

/// The search field's width in the toolbar.
const SEARCH_WIDTH: f32 = 320.0;
/// The narrowest the search field gets on a narrow panel.
const SEARCH_MIN_WIDTH: f32 = 160.0;

/// Describes a rebind that collides with an existing binding.
///
/// Passed to the [`ShortcutSettings::on_conflict`] callback so the app
/// can surface a toast ("Save lost its Ctrl+S binding").
#[derive(Debug, Clone)]
pub struct ShortcutConflict {
    /// Id of the shortcut that currently owns the chord and loses it if the
    /// rebind goes ahead.
    pub displaced_id: String,
    /// Display name of that shortcut (its registry `name`).
    pub displaced_name: String,
    /// The chord being assigned to the new target.
    pub keystroke: KeyStroke,
}

/// A settings panel for browsing and rebinding application shortcuts.
///
/// See the module documentation for what it shows and how it behaves.
pub struct ShortcutSettings {
    filter: Option<Signal<String>>,
    modified_only: Option<Signal<bool>>,
    toolbar: bool,
    confirm_conflicts: bool,
    on_conflict: Option<Rc<dyn Fn(&ShortcutConflict)>>,
    shared: Option<Rc<Shared>>,
    root_child_id: Option<WidgetId>,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortcutSettings {
    /// A panel listing every shortcut registered in the tree's
    /// `ShortcutRegistry`, with its toolbar, asking before it takes a chord
    /// from another command.
    pub fn new() -> Self {
        Self {
            filter: None,
            modified_only: None,
            toolbar: true,
            confirm_conflicts: true,
            on_conflict: None,
            shared: None,
            root_child_id: None,
        }
    }

    /// Bind the search text to `filter`. The toolbar's search field edits
    /// this signal, and anything else may write it too: a command palette
    /// handing a query over, an app's own search box when the toolbar is
    /// off. The list shows the shortcuts whose name, group, description or
    /// chord contains the text (case-insensitive); an empty string shows
    /// everything.
    pub fn with_filter(mut self, filter: Signal<String>) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Bind "Modified only" to `signal`: while it is `true` the list shows
    /// only the shortcuts the user changed. The toolbar's checkbox edits
    /// the same signal.
    pub fn modified_only(mut self, signal: Signal<bool>) -> Self {
        self.modified_only = Some(signal);
        self
    }

    /// Show the toolbar (search field, find by keys, "Modified only",
    /// "Reset all…"). On by default. Turn it off when the surrounding UI
    /// already has a search field, and drive the list through
    /// [`with_filter`](Self::with_filter) and
    /// [`modified_only`](Self::modified_only).
    pub fn toolbar(mut self, show: bool) -> Self {
        self.toolbar = show;
        self
    }

    /// Ask before a rebind takes a chord from another shortcut. On by
    /// default: the colliding rebind shows "{chord} is already used by
    /// {command}" under the row with Reassign and Cancel, and nothing
    /// changes until the user answers. With `false` the other shortcut
    /// loses the chord at once, and a screen reader is told which one.
    pub fn confirm_conflicts(mut self, yes: bool) -> Self {
        self.confirm_conflicts = yes;
        self
    }

    /// Register a callback fired whenever a rebind collides with an
    /// existing binding, **regardless** of [`confirm_conflicts`]. The
    /// callback receives the displaced shortcut's id, name, and the
    /// chord, so the app can surface a toast. It fires before the
    /// displaced binding is removed (when asking first, before the user
    /// has answered).
    ///
    /// [`confirm_conflicts`]: Self::confirm_conflicts
    pub fn on_conflict(mut self, f: impl Fn(&ShortcutConflict) + 'static) -> Self {
        self.on_conflict = Some(Rc::new(f));
        self
    }

    fn toolbar_row(&self, ctx: &mut BuildContext, shared: &Rc<Shared>) -> WidgetId {
        let search = ctx.add(
            SearchField::new(shared.filter.clone())
                .placeholder(tr_widget!(shortcut_settings_search_placeholder()))
                .label(tr_widget!(shortcut_settings_search_label())),
        );
        shared.search_field.set(Some(search));

        let find = {
            let shared = shared.clone();
            IconButton::new((BuiltInIcons::global().keyboard)())
                .toggle(shared.finding.clone())
                .tooltip(tr_widget!(shortcut_settings_find_by_keys()))
                .on_activate_fn(move |ctx: &mut EventContext| {
                    // The toggle has already flipped `finding`; put it back
                    // and let `start_finding` decide from the state it saw.
                    shared.finding.set(!shared.finding.get());
                    start_finding(&shared, ctx);
                })
        };

        let modified_only = Checkbox::new(shared.modified_only.clone())
            .label(tr_widget!(shortcut_settings_modified_only()));

        let reset_all = {
            let shared = shared.clone();
            Button::new(tr_widget!(shortcut_settings_reset_all()))
                .enabled(shared.has_overrides.clone())
                .on_activate_fn(move |_ctx: &mut EventContext| {
                    shared.cancel_capture();
                    shared.focus.set(Some(FocusKey::ResetAllCancel));
                    shared.confirm_reset_all.set(true);
                })
        };
        let reset_all = ctx.add(reset_all);
        shared.reset_all_button.set(Some(reset_all));

        // The field has a width of its own rather than the toolbar's: the
        // list under it stops where its last column ends, and a field running
        // on to the far edge would leave the toolbar's controls stranded
        // there, away from the rows they act on. It gives way on a narrow
        // panel before anything else does.
        let search = Shrinkable::new()
            .min_width(SEARCH_MIN_WIDTH)
            .child(FixedSize::new().width(SEARCH_WIDTH).child(search));
        ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(search)
                .child(find)
                .child(modified_only)
                .child(reset_all)
                .child(Spacer::new()),
        )
    }
}

impl std::fmt::Debug for ShortcutSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShortcutSettings").finish()
    }
}

impl Widget for ShortcutSettings {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let shared = Rc::new(Shared::new(
            self.filter
                .clone()
                .unwrap_or_else(|| Signal::new(String::new())),
            self.modified_only
                .clone()
                .unwrap_or_else(|| Signal::new(false)),
            self.confirm_conflicts,
            self.on_conflict.clone(),
        ));
        // A rebuild replaces the state, and dropping the old one drops any
        // capture it had armed.
        self.shared = Some(shared.clone());

        let mut column = VStack::new().spacing(12.0);
        if self.toolbar {
            column = column.child(self.toolbar_row(ctx, &shared));
        }
        column = column.child(ShortcutTable::new(shared));
        let root = ctx.add(column);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Group);
        builder.set_name(
            tr_widget!(a11y_shortcut_settings_name())
                .resolve_now()
                .as_str(),
        );
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
