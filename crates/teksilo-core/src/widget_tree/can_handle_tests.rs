// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Whether an intent sent from focus would be handled, asked without
//! running anything.
//!
//! [`WidgetTree::can_handle`] looks where a dispatch would: the actions on the
//! walk from the logical focus to the root, the window's global actions (only
//! those registered inside a modal the focus is in), then what the application
//! tree takes. With a menu open, focus is in the menu,
//! and the logical focus is the widget the menu gives it back to, where a
//! command chosen there is sent from.

use std::rc::Rc;

use teksilo_canvas::SizeProposal;

use crate::intent::IntentResponse;
use crate::overlay::{DismissBehavior, OverlayId, OverlayPlacement, OverlayRequest};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::{
    Action, ApplicationScope, BuildContext, LayoutContext, LayoutResponse, Signal, Widget, WidgetId,
};

use super::WidgetTree;

/// The window's root: a scoped action for `test.root`, a global one for
/// `test.global`, a disabled one for `test.disabled`, one that declines
/// `test.declines`, and two children: a button menus are attached to, and
/// an editor answering `test.editor`.
#[derive(Debug)]
struct Root {
    ids: Rc<std::cell::RefCell<Vec<WidgetId>>>,
}

#[derive(Debug)]
struct Editor;

impl Widget for Editor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_action(Action::new("test.editor").on_invoke(|_, _| unreachable!()));
        ctx.apply_self_handlers(crate::widget_builder::HandlerSet::new().focusable(true));
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 20.0).into()
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_action(Action::new("test.root").on_invoke(|_, _| unreachable!()));
        ctx.register_action(
            Action::new("test.disabled")
                .enabled_when(Signal::new(false))
                .on_invoke(|_, _| unreachable!()),
        );
        ctx.register_action(
            Action::new("test.declines")
                .on_invoke_with_response(|_, _| -> IntentResponse { unreachable!() }),
        );
        ctx.register_action_global(Action::new("test.global").on_invoke(|_, _| unreachable!()));
        let button = ctx.add(FillWidget::new().focusable());
        let editor = ctx.add(Editor);
        self.ids.borrow_mut().extend([button, editor]);
        vec![button, editor]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(400.0, 300.0).into()
    }
}

struct Window {
    tree: WidgetTree,
    button: WidgetId,
    editor: WidgetId,
}

fn window() -> Window {
    let ids = Rc::default();
    let mut tree = WidgetTree::new();
    tree.add(Root {
        ids: Rc::clone(&ids),
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let ids = ids.borrow();
    Window {
        tree,
        button: ids[0],
        editor: ids[1],
    }
}

impl Window {
    /// A focusable row in an overlay attached to `anchor`, placed as
    /// `placement`, opened over `over` (focus goes back there when it
    /// closes; else the widget focused now) from the overlay `parent`, with
    /// focus on the row. Returns the row and the overlay.
    fn focused_overlay(
        &mut self,
        anchor: WidgetId,
        over: Option<WidgetId>,
        parent: Option<OverlayId>,
        placement: OverlayPlacement,
    ) -> (WidgetId, OverlayId) {
        let row = self.tree.add(FillWidget::new().focusable());
        let content = self.tree.add(StackWidget::new().child(row));
        let restore = over.or(self.tree.focused());
        let mut request = OverlayRequest::new(content, anchor, placement, DismissBehavior::Manual);
        if let Some(parent) = parent {
            request = request.parent_overlay(parent);
        }
        let overlay = self.tree.show_overlay(request);
        if let Some(restore) = restore {
            self.tree
                .overlay_manager_mut()
                .set_top_focus_restore(restore);
        }
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        self.tree.focus(row);
        (row, overlay)
    }
}

#[test]
fn a_scoped_action_on_the_walk_handles_it() {
    let mut window = window();
    window.tree.focus(window.editor);
    assert!(window.tree.can_handle("test.editor"));
    assert!(window.tree.can_handle("test.root"));
}

#[test]
fn a_scoped_action_off_the_walk_does_not() {
    let mut window = window();
    window.tree.focus(window.button);
    assert!(!window.tree.can_handle("test.editor"));
}

#[test]
fn a_global_action_handles_it() {
    let mut window = window();
    window.tree.focus(window.editor);
    assert!(window.tree.can_handle("test.global"));
}

#[test]
fn with_nothing_focused_the_walk_starts_at_the_root() {
    let window = window();
    assert_eq!(window.tree.focused(), None);
    assert!(window.tree.can_handle("test.root"));
    assert!(!window.tree.can_handle("test.editor"));
}

#[test]
fn the_application_scope_handles_what_the_window_does_not() {
    let mut window = window();
    assert!(!window.tree.can_handle("app.new"));
    #[derive(Debug)]
    struct AppRoot;
    impl Widget for AppRoot {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.register_action(Action::new("app.new").on_invoke(|_, _| unreachable!()));
            vec![]
        }
        fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }
    let mut app = WidgetTree::new();
    app.add(AppRoot);
    let scope = ApplicationScope::new();
    app.publish_application_scope(&scope);
    window.tree.set_application_scope(Some(scope));
    assert!(window.tree.can_handle("app.new"));
}

#[test]
fn an_unhandled_intent_is_not() {
    let window = window();
    assert!(!window.tree.can_handle("test.nobody"));
}

#[test]
fn a_disabled_action_does_not_handle_it() {
    let window = window();
    assert!(!window.tree.can_handle("test.disabled"));
}

#[test]
fn an_action_that_may_decline_counts_as_handling_it() {
    // Whether it declines is known only by running it.
    let window = window();
    assert!(window.tree.can_handle("test.declines"));
}

#[test]
fn with_a_menu_open_the_walk_starts_where_the_menu_gives_focus_back() {
    let mut window = window();
    let (button, editor) = (window.button, window.editor);
    let (row, menu) =
        window.focused_overlay(button, Some(editor), None, OverlayPlacement::BelowPreferred);
    assert!(
        window.tree.can_handle("test.editor"),
        "the editor the menu was opened over answers, as a chosen row's command would reach it"
    );
    assert!(
        !window.tree.can_handle_from(row, "test.editor"),
        "from the row itself the walk goes through the button, not the editor"
    );
    // A submenu: focus goes back through its parent menu to the editor.
    window.focused_overlay(row, None, Some(menu), OverlayPlacement::BelowPreferred);
    assert!(window.tree.can_handle("test.editor"));
}

#[test]
fn a_menu_opened_over_nothing_starts_at_its_row() {
    let mut window = window();
    let button = window.button;
    window.focused_overlay(button, None, None, OverlayPlacement::BelowPreferred);
    assert!(
        window.tree.can_handle("test.root"),
        "from the row, across to the button"
    );
    assert!(!window.tree.can_handle("test.editor"));
}

#[test]
fn focus_in_a_modal_reaches_none_of_the_pages_actions() {
    let mut window = window();
    let (button, editor) = (window.button, window.editor);
    window.focused_overlay(button, Some(editor), None, OverlayPlacement::Centered);
    assert!(
        !window.tree.can_handle("test.root"),
        "the modal is a barrier"
    );
    assert!(!window.tree.can_handle("test.editor"));
    assert!(
        !window.tree.can_handle("test.global"),
        "the page's global action is not the modal's"
    );
}

/// Registers a window-global action for `test.modal_global`.
#[derive(Debug)]
struct ModalCommands;

impl Widget for ModalCommands {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_action_global(
            Action::new("test.modal_global").on_invoke(|_, _| unreachable!()),
        );
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[test]
fn focus_in_a_modal_reaches_the_global_actions_registered_inside_it() {
    let mut window = window();
    let button = window.button;
    let (row, overlay) = window.focused_overlay(button, None, None, OverlayPlacement::Centered);
    let content = window
        .tree
        .overlay_manager_mut()
        .overlay(overlay)
        .map(|o| o.content_id)
        .expect("the modal is up");
    window.tree.add_child(content, ModalCommands);
    window.tree.layout(SizeProposal::exact(400.0, 300.0));
    window.tree.focus(row);
    assert!(window.tree.can_handle("test.modal_global"));
    window.tree.focus(window.editor);
    assert!(
        window.tree.can_handle("test.modal_global"),
        "from the page, every global action, as for a programmatic send"
    );
}

/// With a modal up and nothing focused, the answer is the modal's, as if
/// focus were in it: what a shortcut or a command sent from focus would
/// reach then.
#[test]
fn with_a_modal_up_and_nothing_focused_the_page_handles_nothing() {
    let mut window = window();
    let (button, editor) = (window.button, window.editor);
    window.focused_overlay(button, Some(editor), None, OverlayPlacement::Centered);
    window.tree.clear_focus();
    assert_eq!(window.tree.focused(), None);
    assert!(
        !window.tree.can_handle("test.root"),
        "the modal is a barrier"
    );
    assert!(!window.tree.can_handle("test.global"));
}
