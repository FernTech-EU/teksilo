// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A modal, as a screen reader meets it.
//!
//! An in-tree modal (a `MessageBox`, a `Dialog`) is a `Centered` overlay over
//! a full-viewport scrim. The scrim keeps the pointer off the page behind it
//! and the Tab cycle stays inside it, but `tools/reader/` found the page open
//! to a screen reader's own requests (dialogs-08, radioclose-M1): an AT-SPI
//! click on a button behind "Save changes?" opened a second box over it, a
//! click on a checkbox behind "Close window?" toggled it, and `grab_focus`
//! moved focus onto a control behind the box, where the next real key
//! activated it.
//!
//! Nothing below Teksilo stops those requests. AccessKit's only word for a
//! modal is `Node::set_modal`, a state every adapter reports and none acts on:
//! `accesskit_atspi_common` maps it to `State::Modal` (`node.rs:342-344`),
//! `accesskit_windows` answers it as the window pattern's `IsModal`
//! (`node.rs:735-741`), `accesskit_macos` as `isAccessibilityModal`
//! (`node.rs:1194-1197`), and each routes a request to any node in the tree.
//! So the tree refuses a request whose target is behind the modal.
//!
//! The page itself stays in the tree. Taking it out, as a browser takes out
//! what a modal dialog makes inert, removes every node on it from the tree the
//! adapters walk, and AT-SPI declares a removed node defunct for good: with
//! the page left out while "Save changes?" was up, `tools/reader/` measured
//! Orca 46.1 ignoring the focus event of the opener the page came back with
//! ("Ignoring defunct object"), so closing the box went silent.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use accesskit::{Action, NodeId, Role};
use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeChangeHandler, common_filter};
use teksilo_canvas::SizeProposal;

use crate::accessibility::widget_id_to_node_id;
use crate::overlay::{
    DismissBehavior, OverlayBand, OverlayId, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;
use crate::window::NoopWindowOps;

/// A control a reader can name, focus and press. `pressed` counts presses.
fn control(tree: &mut WidgetTree, name: &str, pressed: &Rc<Cell<u32>>) -> WidgetId {
    let pressed = pressed.clone();
    tree.add(
        FillWidget::new()
            .label(name)
            .focusable()
            .access_role(Role::Button)
            .access_action(Action::Click, move |_ctx| pressed.set(pressed.get() + 1)),
    )
}

fn overlay(content_id: WidgetId, anchor: WidgetId, placement: OverlayPlacement) -> OverlayRequest {
    OverlayRequest {
        content_id,
        anchor,
        placement,
        dismiss: DismissBehavior::Manual,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: None,
        fade_duration: None,
    }
}

/// A page with two controls, and a modal to put over it.
struct Scene {
    tree: WidgetTree,
    welcome: WidgetId,
    delete: WidgetId,
    save: WidgetId,
    panel: WidgetId,
    scrim: WidgetId,
    /// Presses on the page's controls.
    page_pressed: Rc<Cell<u32>>,
    /// Presses on the modal's control.
    modal_pressed: Rc<Cell<u32>>,
}

impl Scene {
    /// The page, laid out, with focus on its first control and no modal yet.
    fn page() -> Self {
        let mut tree = WidgetTree::new();
        let page_pressed = Rc::new(Cell::new(0));
        let modal_pressed = Rc::new(Cell::new(0));
        let welcome = control(&mut tree, "Welcome", &page_pressed);
        let delete = control(&mut tree, "Delete file?", &page_pressed);
        tree.add(StackWidget::new().child(welcome).child(delete));
        let save = control(&mut tree, "Save", &modal_pressed);
        let panel = tree.add(StackWidget::new().child(save));
        tree.set_dormant(panel);
        let scrim = tree.add(FillWidget::new());
        tree.set_dormant(scrim);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(welcome);
        Self {
            tree,
            welcome,
            delete,
            save,
            panel,
            scrim,
            page_pressed,
            modal_pressed,
        }
    }

    /// The page with the modal up.
    fn with_modal() -> (Self, OverlayId) {
        let mut scene = Self::page();
        let modal = scene.open();
        (scene, modal)
    }

    /// Present the modal as the app presents one (`teksilo-app`,
    /// `present_in_tree_modal_request`): a scrim, then the centred panel,
    /// then focus into it.
    fn open(&mut self) -> OverlayId {
        let tree = &mut self.tree;
        tree.activate(self.scrim);
        let scrim = tree.show_overlay(overlay(
            self.scrim,
            self.welcome,
            OverlayPlacement::FullViewport,
        ));
        tree.activate(self.panel);
        let modal = tree.show_overlay(overlay(
            self.panel,
            self.welcome,
            OverlayPlacement::Centered,
        ));
        tree.overlay_manager_mut()
            .set_parent_overlay(scrim, Some(modal));
        tree.overlay_manager_mut()
            .set_top_focus_restore(self.welcome);
        tree.focus(self.save);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        modal
    }

    /// Ask for `action` on `id` as a platform adapter's request arrives, and
    /// say whether anything took it.
    fn request(&mut self, id: WidgetId, action: Action) -> bool {
        self.tree
            .dispatch_access_action(widget_id_to_node_id(id), action, None, &mut NoopWindowOps)
    }
}

#[test]
fn an_at_click_behind_a_modal_does_nothing() {
    let (mut scene, modal) = Scene::with_modal();

    assert!(
        !scene.request(scene.welcome, Action::Click),
        "a click on a control behind the modal is refused, and reported unhandled"
    );
    assert_eq!(
        scene.page_pressed.get(),
        0,
        "the control behind the modal must not run: behind 'Save changes?' it opened a \
         second box, behind 'Close window?' it toggled the document's checkbox"
    );

    assert!(scene.request(scene.save, Action::Click));
    assert_eq!(scene.modal_pressed.get(), 1, "the modal's own control runs");

    scene.tree.dismiss_overlay(modal);
    assert!(scene.request(scene.welcome, Action::Click));
    assert_eq!(
        scene.page_pressed.get(),
        1,
        "and the page runs again once it closes"
    );
}

#[test]
fn an_at_focus_request_behind_a_modal_leaves_focus_in_it() {
    let (mut scene, _) = Scene::with_modal();

    assert!(!scene.request(scene.delete, Action::Focus));
    assert_eq!(
        scene.tree.focused(),
        Some(scene.save),
        "focus stays in the modal: moved behind it, the next real key activates a \
         control the modal is covering"
    );
}

#[test]
fn what_opens_over_the_modal_takes_requests() {
    let (mut scene, _) = Scene::with_modal();
    let pressed = Rc::new(Cell::new(0));
    // A drop-down opened from inside the modal.
    let option = control(&mut scene.tree, "Option", &pressed);
    let list = scene.tree.add(StackWidget::new().child(option));
    scene
        .tree
        .show_overlay(overlay(list, scene.save, OverlayPlacement::Below));
    // Raised over the modal from elsewhere, as an application shows a
    // snackbar while the modal is up: above the scrim, so the pointer reaches
    // it, and so may a reader.
    let undo = control(&mut scene.tree, "Undo", &pressed);
    let snackbar = scene.tree.add(StackWidget::new().child(undo));
    scene.tree.show_overlay(overlay(
        snackbar,
        scene.welcome,
        OverlayPlacement::BottomCenter,
    ));
    scene.tree.layout(SizeProposal::exact(400.0, 300.0));

    assert!(scene.request(option, Action::Click));
    assert!(
        scene.request(undo, Action::Click),
        "an overlay above the modal takes requests wherever it is anchored"
    );
    assert_eq!(pressed.get(), 2);
    assert!(!scene.request(scene.welcome, Action::Click));
}

/// A text field's selection handles sit in a band *below* every menu and
/// dialog, so a field inside the modal has its handles under the modal in
/// the stack. They belong to the field, not to the page.
#[test]
fn a_lower_band_overlay_takes_requests_when_anchored_in_the_modal() {
    let (mut scene, _) = Scene::with_modal();
    let pressed = Rc::new(Cell::new(0));
    let handle = control(&mut scene.tree, "Selection start", &pressed);
    let handles = scene.tree.add(StackWidget::new().child(handle));
    scene.tree.show_overlay_in_band(
        overlay(handles, scene.save, OverlayPlacement::FullViewport),
        OverlayBand::TextAffordance,
    );
    let stray = control(&mut scene.tree, "Page handle", &pressed);
    let stray_layer = scene.tree.add(StackWidget::new().child(stray));
    scene.tree.show_overlay_in_band(
        overlay(stray_layer, scene.welcome, OverlayPlacement::FullViewport),
        OverlayBand::TextAffordance,
    );
    scene.tree.layout(SizeProposal::exact(400.0, 300.0));

    assert!(
        scene.request(handle, Action::Click),
        "the handles of a field inside the modal are the modal's"
    );
    assert!(
        !scene.request(stray, Action::Click),
        "the handles of a field on the page are behind it"
    );
    assert_eq!(pressed.get(), 1);
}

/// Why the refusal is at the request and not in the tree: a node the adapter
/// removes is defunct to AT-SPI for good, so a page left out of the tree while
/// the modal is up comes back dead to Orca when it closes.
#[test]
fn a_modal_removes_nothing_from_the_page() {
    let mut scene = Scene::page();
    let mut reader = Tree::new(scene.tree.sync_accessibility(), true);
    let mut removed = Removed::default();

    let modal = scene.open();
    reader.update_and_process_changes(scene.tree.sync_accessibility(), &mut removed);
    scene.tree.dismiss_overlay(modal);
    scene.tree.focus(scene.welcome);
    reader.update_and_process_changes(scene.tree.sync_accessibility(), &mut removed);

    for page in [scene.welcome, scene.delete] {
        assert!(
            !removed.0.contains(&widget_id_to_node_id(page)),
            "a node on the page left the adapter's tree while the modal was up: AT-SPI \
             declares it defunct, and Orca ignores it from then on"
        );
    }
    assert_eq!(
        reader
            .state()
            .focus()
            .and_then(|node| node.label())
            .as_deref(),
        Some("Welcome")
    );
}

/// The nodes that left the tree the adapters walk, which AT-SPI announces as
/// defunct (`accesskit_atspi_common` `adapter.rs`, `remove_node`).
#[derive(Default)]
struct Removed(HashSet<NodeId>);

fn included(node: &NodeRef) -> bool {
    common_filter(node) == FilterResult::Include
}

impl TreeChangeHandler for Removed {
    fn node_added(&mut self, _: &NodeRef) {}
    fn node_updated(&mut self, old: &NodeRef, new: &NodeRef) {
        if included(old) && !included(new) {
            self.0.insert(old.locate().0);
        }
    }
    fn focus_moved(&mut self, _: Option<&NodeRef>, _: Option<&NodeRef>) {}
    fn node_removed(&mut self, node: &NodeRef) {
        if included(node) {
            self.0.insert(node.locate().0);
        }
    }
}
