// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a screen reader is told when a widget tree changes, for tests.
//!
//! Reading a node's name after a key press says what a screen reader *could*
//! find if it looked. It says nothing about what it is *told*, and a user who
//! cannot see the screen hears only what they are told: a day that is right
//! in the tree and never announced is silence, and the same month announced
//! through two channels is heard twice. Both have shipped in the calendar
//! with every name-reading test green.
//!
//! A headless tree has no platform adapter, so this runs each accessibility
//! update through `accesskit_consumer`, the crate all three adapters are
//! built on, and keeps the change events they turn into speech. Each kind
//! below says which adapter code raises it, read from
//! `accesskit_atspi_common-0.20.0`, `accesskit_windows-0.35.0` and
//! `accesskit_macos-0.27.0`.
//!
//! Some events are raised and deliberately not kept: a change of name, value,
//! checked state or number on a node that is not the focus. Orca speaks a
//! name change only for its locus of focus (`orca/scripts/default.py`,
//! `onNameChanged`, Orca 46.1), and a checked state or a number likewise
//! (`onCheckedChanged`, `onPressedChanged`, `onValueChanged`, which also
//! speaks a progress bar's number wherever it is; that is not modelled), and
//! AT-SPI carries no string value at all (`accesskit_atspi_common`
//! `node.rs:650-658` raises only a numeric one). [`Listener::toggled`] reads
//! the checked state of a node that is not the focus, as a reader who goes
//! to it finds it. That NVDA and VoiceOver likewise speak these changes only
//! on their focus object is not verified here: neither's source is on this
//! machine.
//!
//! A node the adapter removed stays dead to Orca. `accesskit_atspi_common`
//! announces a node that leaves the filtered tree as defunct (`adapter.rs`,
//! `remove_node`), libatspi keeps the state for the path, and Orca 46.1 drops
//! every event from it (`event_manager.py:796-798`, "Ignoring defunct
//! object"). What such a node tells is kept as [`Heard::Dropped`]. The
//! listener reads what `teksilo-app` hands the adapter,
//! [`WidgetTree::deliver_accessibility`], so a node that comes back under a
//! new id is heard as the adapter hears it.
//!
//! Two things a reader can be told are not modelled, so a test about either
//! must not rest on this alone. A state change on the focus other than its
//! checked state: Orca says "selected" when Space selects its locus of focus
//! (`onSelectedChanged`). And the descendants of a node that stops being
//! hidden: AT-SPI adds that whole subtree in one walk and announces each live
//! node in it (`accesskit_atspi_common` `adapter.rs`, `add_subtree`), where
//! the consumer reports only the node whose own data changed.

use std::collections::HashSet;

use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeChangeHandler, common_filter};
use teksilo_core::accesskit::{NodeId, Role, Toggled, TreeUpdate};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

/// One thing a screen reader is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Heard {
    /// Focus moved, and this is the name of the node that took it, after
    /// `active_descendant` is followed (`accesskit_consumer` `tree.rs:537-543`).
    /// AT-SPI raises `state-changed:focused` on it (`accesskit_atspi_common`
    /// `adapter.rs:324-340`), UIA a focus-changed event (`accesskit_windows`
    /// `adapter.rs:341-345`), macOS `FocusedUIElementChanged`
    /// (`accesskit_macos` `event.rs:319-326`). Every reader speaks it.
    Focus(String),
    /// A live region spoke this text. On all three platforms that is a live
    /// node entering the filtered tree with a name, or a live node's name
    /// changing (`accesskit_atspi_common` `adapter.rs:72-77`, `node.rs:610-622`;
    /// `accesskit_windows` `adapter.rs:256-263`, `314-324`; `accesskit_macos`
    /// `event.rs:236-241`, `300-310`). UIA and macOS also speak it when only
    /// the `live` setting changed, which is kept here too. The setting is
    /// inherited: `accesskit_consumer`'s `NodeRef::live` (`node.rs:906-910`)
    /// hands it to every descendant that sets none.
    Live(String),
    /// The node that holds focus was renamed. Orca speaks it as a change of
    /// its locus of focus (`onNameChanged`), whether or not the node is live,
    /// so a focused live node renamed is heard twice.
    FocusName(String),
    /// The node that holds focus took a new value. UIA and macOS raise it
    /// (`accesskit_windows` `node.rs:1346`, `accesskit_macos`
    /// `event.rs:262-289`); AT-SPI does not, for a string.
    FocusValue(String),
    /// The node that holds focus was checked, cleared or made mixed. AT-SPI
    /// raises `state-changed:checked`, `:pressed` (a toggle button) or
    /// `:indeterminate` on it (`accesskit_atspi_common` `node.rs:366-374`,
    /// `594-608`), UIA a toggle-state property change (`accesskit_windows`
    /// `node.rs:743`, `1333-1334`), macOS `AXValueChanged` (`accesskit_macos`
    /// `node.rs:344-351`, `event.rs:268-288`). Orca speaks it for its locus of
    /// focus (`onCheckedChanged`, `onPressedChanged`), a radio button's only
    /// after a Space it saw itself.
    FocusToggled(Toggled),
    /// The node that holds focus took a new number, written as Orca speaks it
    /// (`orca/ax_value.py`, `get_current_value_text`): with every digit of the
    /// `f64`'s shortest form below 1, rounded to a whole number from 1 up.
    /// AT-SPI raises `property-change:accessible-value` (`node.rs:651-658`),
    /// UIA a range-value property change (`accesskit_windows` `node.rs:743`,
    /// `1356-1357`), macOS `AXValueChanged` (`accesskit_macos` `node.rs:361`).
    /// Orca speaks it for its locus of focus (`onValueChanged`).
    FocusNumber(String),
    /// Told from a node whose id the adapter had already removed. AT-SPI
    /// announced that id defunct as the node left, libatspi keeps it so, and
    /// Orca drops the event ("Ignoring defunct object"): the reader hears
    /// nothing.
    Dropped(Box<Heard>),
}

/// A number as Orca 46.1 speaks a node's value (`ax_value.py`,
/// `get_current_value_text`, with no value text, which AT-SPI never carries).
fn as_orca_speaks(value: f64) -> String {
    if value.abs() < 1.0 && value != 0.0 {
        // Python's `str` of a float is its shortest round-trip form, as
        // Rust's `Display` is.
        value.to_string()
    } else {
        format!("{value:.0}")
    }
}

/// A screen reader attached to a tree, hearing what each change tells it.
pub(crate) struct Listener {
    platform: Tree,
    /// Every id the adapter has removed.
    defunct: HashSet<NodeId>,
}

/// What `teksilo-app` hands the adapter on a frame.
fn delivered(tree: &mut WidgetTree) -> TreeUpdate {
    let _ = tree.sync_accessibility();
    tree.deliver_accessibility(true)
}

impl Listener {
    /// Start listening to `tree` as it stands now. Nothing already in it is
    /// heard: a reader attaching to a window is not told what was said before.
    pub(crate) fn attach(tree: &mut WidgetTree) -> Self {
        let platform = Tree::new(delivered(tree), true);
        let mut listener = Self {
            platform,
            defunct: HashSet::new(),
        };
        // Let any message queued while the tree was being built go by
        // unheard: the framework's announcer puts one message in the tree an
        // update.
        let _ = listener.heard(tree);
        listener
    }

    /// Everything the reader is told between the last call and now.
    ///
    /// The tree is synced four times, as a running application syncs on
    /// the frames that follow a change: the framework's announcer puts one
    /// queued message in the tree an update, and none in an update that
    /// moves focus, so a change that moves focus and queues two needs three
    /// syncs to be heard whole, and the fourth shows that nothing is said
    /// again.
    pub(crate) fn heard(&mut self, tree: &mut WidgetTree) -> Vec<Heard> {
        let mut handler = Handler {
            heard: Vec::new(),
            defunct: &mut self.defunct,
        };
        for _ in 0..4 {
            let update = delivered(tree);
            self.platform
                .update_and_process_changes(update, &mut handler);
        }
        handler.heard
    }

    /// The node the reader holds as its focus at the last call, after
    /// `active_descendant` is followed, as its id and whether it is selected.
    /// The same id before and after a change means the reader's focus node
    /// survived it, and a `selected` that differs is the state change AT-SPI
    /// raises on it (`accesskit_atspi_common` `node.rs:594-607`).
    pub(crate) fn focus(&self) -> Option<(teksilo_core::accesskit::NodeId, Option<bool>)> {
        let state = self.platform.state();
        state
            .focus()
            .map(|node| (node.locate().0, node.is_selected()))
    }

    /// Whether the reader can find a node of `role` named `name` without
    /// being told about it, walking the tree as it stood at the last call as
    /// the adapters walk it (`common_filter`). A node that is published only
    /// once it holds focus fails this, and object navigation and flat review
    /// never reach it.
    pub(crate) fn finds(&self, role: Role, name: &str) -> bool {
        fn walk(node: NodeRef<'_>, role: Role, name: &str) -> bool {
            (node.role() == role && node.label().as_deref() == Some(name))
                || node
                    .filtered_children(&common_filter)
                    .any(|child| walk(child, role, name))
        }
        walk(self.platform.state().root(), role, name)
    }

    /// The checked state the reader is given for the node of `role` named
    /// `name`, in the tree as it stood at the last call: what a screen reader
    /// reads when it asks, focused or not. `None` when no such node is
    /// reachable, or when it has no checked state.
    pub(crate) fn toggled(&self, role: Role, name: &str) -> Option<Toggled> {
        fn walk<'a>(node: NodeRef<'a>, role: Role, name: &str) -> Option<NodeRef<'a>> {
            if node.role() == role && node.label().as_deref() == Some(name) {
                return Some(node);
            }
            node.filtered_children(&common_filter)
                .find_map(|child| walk(child, role, name))
        }
        walk(self.platform.state().root(), role, name).and_then(|node| node.toggled())
    }

    /// The nodes a reader can reach, walking the tree as it stood at the last
    /// call, under an id the adapter had removed before: dead to Orca, which
    /// drops every event from them. Each as its role and name.
    pub(crate) fn dead(&self) -> Vec<String> {
        fn walk(node: NodeRef<'_>, defunct: &HashSet<NodeId>, dead: &mut Vec<String>) {
            if defunct.contains(&node.locate().0) {
                dead.push(format!(
                    "{:?} {:?}",
                    node.role(),
                    node.label().unwrap_or_default()
                ));
            }
            for child in node.filtered_children(&common_filter) {
                walk(child, defunct, dead);
            }
        }
        let mut dead = Vec::new();
        walk(self.platform.state().root(), &self.defunct, &mut dead);
        dead
    }

    /// The description a reader gets for the first node of `role` it can
    /// reach, walking the tree as it stood at the last call as the adapters
    /// walk it (`common_filter`). The consumer takes a description from the
    /// node's own property alone (`node.rs:796-800`), and that is what AT-SPI
    /// `Description`, UIA `FullDescription` and macOS `accessibilityHelp`
    /// read. `None` when no such node is reachable, or it has no description.
    pub(crate) fn description_of(&self, role: Role) -> Option<String> {
        fn walk(node: NodeRef<'_>, role: Role) -> Option<NodeRef<'_>> {
            if node.role() == role {
                return Some(node);
            }
            node.filtered_children(&common_filter)
                .find_map(|child| walk(child, role))
        }
        walk(self.platform.state().root(), role).and_then(|node| node.description())
    }

    /// The tree as the adapters held it at the last call, for a test that
    /// reads a property no change event carries: a relation, or what AT-SPI's
    /// Selection interface counts (`accesskit_atspi_common` `node.rs`,
    /// `n_selected_children`, over `NodeRef::items`).
    pub(crate) fn platform(&self) -> &Tree {
        &self.platform
    }

    /// The widget behind the node of `role` named `name` that the reader can
    /// find, as [`finds`](Self::finds) looks for it, so a test can act on what
    /// the reader was offered (a menu row, say). Only on a node's first
    /// appearance: one handed to the adapter under a new id, having come back
    /// (`WidgetTree::deliver_accessibility`), does not map back to its widget.
    pub(crate) fn widget(&self, role: Role, name: &str) -> Option<WidgetId> {
        fn walk(node: NodeRef<'_>, role: Role, name: &str) -> Option<WidgetId> {
            if node.role() == role && node.label().as_deref() == Some(name) {
                return teksilo_core::accessibility::node_id_to_widget_id_maybe(node.locate().0);
            }
            node.filtered_children(&common_filter)
                .find_map(|child| walk(child, role, name))
        }
        walk(self.platform.state().root(), role, name)
    }

    /// What AT-SPI's `Text` interface answers for the first text input the
    /// reader can reach, as the tree stood at the last call: the caret offset
    /// and the selection, read the way `accesskit_atspi_common` reads them
    /// (`node.rs`, `caret_offset` and `selection`). `None` when no text input
    /// with text is in the tree.
    pub(crate) fn text_input(&self) -> Option<TextAnswer> {
        fn find<'a>(node: NodeRef<'a>) -> Option<NodeRef<'a>> {
            if node.is_text_input() && node.supports_text_ranges() {
                return Some(node);
            }
            node.filtered_children(&common_filter).find_map(find)
        }
        let node = find(self.platform.state().root())?;
        let caret = node.text_selection_focus()?.to_global_usv_index();
        let selection = node
            .text_selection()
            .filter(|range| !range.is_degenerate())
            .map(|range| {
                (
                    range.start().to_global_usv_index(),
                    range.end().to_global_usv_index(),
                )
            });
        Some(TextAnswer { caret, selection })
    }
}

/// A text input's caret and selection, as a reader asks for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextAnswer {
    /// `caret_offset`: where the selection's focus end is, in characters.
    pub(crate) caret: usize,
    /// `selection(0)`: start and end in characters, `None` when nothing is
    /// selected (`n_selections` is then 0).
    pub(crate) selection: Option<(usize, usize)>,
}

struct Handler<'a> {
    heard: Vec<Heard>,
    defunct: &'a mut HashSet<NodeId>,
}

fn included(node: &NodeRef) -> bool {
    common_filter(node) == FilterResult::Include
}

impl Handler<'_> {
    /// Keep what `node` told, as dropped when the adapter had removed it.
    fn tell(&mut self, node: &NodeRef, heard: Heard) {
        self.heard.push(if self.defunct.contains(&node.locate().0) {
            Heard::Dropped(Box::new(heard))
        } else {
            heard
        });
    }

    /// `remove_subtree`: a node hidden takes its filtered subtree with it.
    fn remove_subtree(&mut self, node: &NodeRef) {
        for child in node.filtered_children(&common_filter) {
            self.remove_subtree(&child);
        }
        self.defunct.insert(node.locate().0);
    }
}

impl TreeChangeHandler for Handler<'_> {
    fn node_added(&mut self, node: &NodeRef) {
        if included(node) && node.live() != teksilo_core::accesskit::Live::Off {
            if let Some(name) = node.label() {
                self.tell(node, Heard::Live(name));
            }
        }
    }

    fn node_updated(&mut self, old: &NodeRef, new: &NodeRef) {
        if !included(new) {
            // The adapter removes a node that stops passing the filter
            // (`adapter.rs`, `node_updated`).
            match common_filter(new) {
                _ if !included(old) => {}
                FilterResult::ExcludeSubtree => self.remove_subtree(old),
                _ => {
                    self.defunct.insert(old.locate().0);
                }
            }
            return;
        }
        let name = new.label();
        if new.live() != teksilo_core::accesskit::Live::Off
            && let Some(name) = name.clone()
            && (!included(old) || old.label().as_ref() != Some(&name) || old.live() != new.live())
        {
            self.tell(new, Heard::Live(name));
        }
        if !included(old) {
            return;
        }
        if new.is_focused() {
            if let Some(name) = name
                && old.label().as_ref() != Some(&name)
            {
                self.tell(new, Heard::FocusName(name));
            }
            if let Some(value) = new.value()
                && old.value().as_ref() != Some(&value)
            {
                self.tell(new, Heard::FocusValue(value));
            }
            if let Some(toggled) = new.toggled()
                && old.toggled() != Some(toggled)
            {
                self.tell(new, Heard::FocusToggled(toggled));
            }
            // A node that carries its value as text is heard by that text:
            // Orca speaks a spin button's displayed text, not its number
            // (`formatting.py`, SPIN_BUTTON "focused"), and UIA and macOS
            // read the value string. Only a node with no value text, such as
            // a slider, is heard by its number.
            if new.value().is_none()
                && let Some(number) = new.numeric_value()
                && old.numeric_value() != Some(number)
            {
                self.tell(new, Heard::FocusNumber(as_orca_speaks(number)));
            }
        }
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef>, new: Option<&NodeRef>) {
        if let Some(node) = new {
            self.tell(node, Heard::Focus(node.label().unwrap_or_default()));
        }
    }

    fn node_removed(&mut self, node: &NodeRef) {
        if included(node) {
            self.defunct.insert(node.locate().0);
        }
    }
}
