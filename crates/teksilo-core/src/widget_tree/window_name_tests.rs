// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A window's name, as a platform adapter reads it.
//!
//! On AT-SPI the window is the tree's root, and its name is the root node's
//! label: `accesskit_atspi_common` sends it with `window:activate`
//! (`adapter.rs:551-560`) and answers it for the frame. `tools/reader/` found
//! every Teksilo window unnamed there, so Orca 46.1 said "frame." and nothing
//! else as a window came up. Windows and macOS read the title from the native
//! window (`accesskit_windows` `node.rs:1061-1068` hands UIA the window
//! handle's own provider; `accesskit_macos` `node.rs:316-321` drops a root
//! window's title on purpose), so the label changes nothing there.

use accesskit_consumer::Tree;
use teksilo_canvas::SizeProposal;

use crate::test_widgets::FillWidget;
use crate::widget_tree::WidgetTree;
use crate::window::state::WindowStateInit;
use crate::window::{TeksiloWindowId, WindowPlacement, WindowState};

fn window(title: &str) -> (WidgetTree, WindowState) {
    let state = WindowState::new(WindowStateInit {
        id: TeksiloWindowId::new(1),
        string_id: None,
        placement: WindowPlacement::Floating,
        title: title.to_string(),
        size: (800, 600),
        position: (0, 0),
        focused: true,
        resizable: true,
        always_on_top: false,
    });
    let mut tree = WidgetTree::new();
    tree.set_window_state(state.clone());
    tree.add(FillWidget::new().label("content"));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    (tree, state)
}

/// What an adapter names the window: the root's label, through the consumer.
fn window_name(tree: &Tree) -> Option<String> {
    tree.state().root().label()
}

#[test]
fn a_window_is_named_by_its_title() {
    let (mut tree, _state) = window("Dialogs and Popovers");
    let replay = Tree::new(tree.sync_accessibility(), true);
    assert_eq!(
        window_name(&replay).as_deref(),
        Some("Dialogs and Popovers"),
        "the window's root must carry its title, or Orca announces an unnamed frame"
    );
}

/// A title set later (an unsaved marker, a document name) renames the window
/// on the next sync, with nothing else in the tree having changed.
#[test]
fn a_new_title_renames_the_window() {
    let (mut tree, state) = window("Editor");
    let mut replay = Tree::new(tree.sync_accessibility(), true);
    let _ = tree.sync_accessibility();

    state.title().set("report.md - Editor".to_string());
    replay.update_and_process_changes(tree.sync_accessibility(), &mut Ignore);
    assert_eq!(window_name(&replay).as_deref(), Some("report.md - Editor"));
}

/// A blank title is no name: an empty label would be a node named "", which
/// every adapter treats as a name that is there.
#[test]
fn a_blank_title_leaves_the_window_unnamed() {
    let (mut tree, _state) = window("   ");
    let replay = Tree::new(tree.sync_accessibility(), true);
    assert_eq!(window_name(&replay), None);
}

struct Ignore;

impl accesskit_consumer::TreeChangeHandler for Ignore {
    fn node_added(&mut self, _: &accesskit_consumer::NodeRef) {}
    fn node_updated(&mut self, _: &accesskit_consumer::NodeRef, _: &accesskit_consumer::NodeRef) {}
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::NodeRef>,
        _: Option<&accesskit_consumer::NodeRef>,
    ) {
    }
    fn node_removed(&mut self, _: &accesskit_consumer::NodeRef) {}
}
