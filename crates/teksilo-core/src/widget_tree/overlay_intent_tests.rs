// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An intent sent from an overlay's content walks on from the widget the
//! overlay is attached to.
//!
//! An overlay's content is a detached root: its arena parent chain ends at
//! the content, so a walk up from a menu row, a popover's button or a field in
//! a dialog used to stop there, and only window-global actions could take its
//! intent. The walk now continues at the overlay's anchor, and up from there,
//! through every overlay the anchor itself sits in (a submenu's trigger is a
//! row of its parent menu). A scoped shortcut does not: a popup is a window of
//! its own for the keyboard, so focus in a popover anchored inside the
//! shortcut's scope is outside it, and the key goes to the popover's field.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;

use crate::event::{EventResponse, Key, Modifiers, WidgetEvent};
use crate::overlay::{DismissBehavior, OverlayPlacement, OverlayRequest};
use crate::shortcut::{KeyStroke, Shortcut};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::window::NoopWindowOps;
use crate::{Action, BuildContext, Intent, LayoutContext, LayoutResponse, Widget, WidgetId};

use super::WidgetTree;

type Calls = Rc<RefCell<Vec<&'static str>>>;

/// The window's root: a scoped action for `test.save`, a scoped shortcut
/// (Ctrl+S) for it, and two children: the button overlays are attached to,
/// and an editor answering `test.save` itself.
#[derive(Debug)]
struct Root {
    calls: Calls,
    button: Rc<RefCell<Option<WidgetId>>>,
    editor: Rc<RefCell<Option<WidgetId>>>,
}

/// A focusable widget answering `test.save` with "editor".
#[derive(Debug)]
struct Editor {
    calls: Calls,
}

impl Widget for Editor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let calls = self.calls.clone();
        ctx.register_action(Action::new("test.save").on_invoke(move |_, _| {
            calls.borrow_mut().push("editor");
        }));
        ctx.apply_self_handlers(crate::widget_builder::HandlerSet::new().focusable(true));
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 20.0).into()
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let calls = self.calls.clone();
        ctx.register_action(Action::new("test.save").on_invoke(move |_, _| {
            calls.borrow_mut().push("root");
        }));
        ctx.register_shortcut(
            Shortcut::new("test.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        let button = ctx.add(FillWidget::new().focusable());
        *self.button.borrow_mut() = Some(button);
        let editor = ctx.add(Editor {
            calls: self.calls.clone(),
        });
        *self.editor.borrow_mut() = Some(editor);
        vec![button, editor]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(400.0, 300.0).into()
    }
}

/// A window with its root, the root's button and editor, and the calls
/// their actions made.
struct Window {
    tree: WidgetTree,
    button: WidgetId,
    editor: WidgetId,
    calls: Calls,
}

impl Window {
    fn new() -> Self {
        let calls = Calls::default();
        let button = Rc::new(RefCell::new(None));
        let editor = Rc::new(RefCell::new(None));
        let mut tree = WidgetTree::new();
        tree.add(Root {
            calls: calls.clone(),
            button: button.clone(),
            editor: editor.clone(),
        });
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let button = button.borrow().expect("the root built its button");
        let editor = editor.borrow().expect("the root built its editor");
        Self {
            tree,
            button,
            editor,
            calls,
        }
    }

    /// Show `content`, a detached root, as an overlay attached to `anchor`.
    fn show(&mut self, content: WidgetId, anchor: WidgetId) {
        self.tree.show_overlay(OverlayRequest::new(
            content,
            anchor,
            OverlayPlacement::Below,
            DismissBehavior::Manual,
        ));
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
    }

    /// A focusable field in a new popup attached to `anchor`, focused.
    fn focused_popup(&mut self, anchor: WidgetId) -> WidgetId {
        let field = self.tree.add(FillWidget::new().focusable());
        let content = self.tree.add(StackWidget::new().child(field));
        self.show(content, anchor);
        self.tree.focus(field);
        field
    }

    fn send_save_from_focus(&mut self) {
        self.tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.send_intent_from_focus(Intent::new("test.save"));
        });
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

#[test]
fn focus_in_an_overlay_reaches_a_scoped_action_on_the_root() {
    let mut window = Window::new();
    let button = window.button;
    window.focused_popup(button);
    window.send_save_from_focus();
    assert_eq!(window.calls(), ["root"]);
}

#[test]
fn the_walk_crosses_nested_overlays() {
    let mut window = Window::new();
    let button = window.button;
    // A menu attached to the button, and a submenu attached to a row of it.
    let row = window.tree.add(FillWidget::new().focusable());
    let menu = window.tree.add(StackWidget::new().child(row));
    window.show(menu, button);
    window.focused_popup(row);
    window.send_save_from_focus();
    assert_eq!(window.calls(), ["root"]);
}

/// Run `f` on a thread of its own and return what it returns, failing after
/// ten seconds rather than hanging: a walk that loops is a failure, not a
/// test run that never ends.
fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(f());
    });
    result
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the walk did not end")
}

#[test]
fn an_overlay_anchored_inside_itself_ends_the_walk() {
    let (chain, field, content, calls) = bounded(|| {
        let mut window = Window::new();
        let field = window.tree.add(FillWidget::new().focusable());
        let content = window.tree.add(StackWidget::new().child(field));
        window.show(content, field);
        window.tree.focus(field);
        let chain = window.tree.intent_walk(field, &[]).0;
        window.send_save_from_focus();
        (chain, field, content, window.calls())
    });
    assert_eq!(chain, [field, content], "back at the field, the walk ends");
    assert!(calls.is_empty(), "nothing on the way takes it");
}

/// Two overlays each attached to a widget in the other: the walk crosses
/// from one to the other once, and ends when it comes back.
#[test]
fn two_overlays_attached_to_each_other_end_the_walk() {
    let (chain, ids, calls) = bounded(|| {
        let mut window = Window::new();
        let first_field = window.tree.add(FillWidget::new().focusable());
        let first = window.tree.add(StackWidget::new().child(first_field));
        let second_field = window.tree.add(FillWidget::new().focusable());
        let second = window.tree.add(StackWidget::new().child(second_field));
        window.show(first, second_field);
        window.show(second, first_field);
        window.tree.focus(first_field);
        let chain = window.tree.intent_walk(first_field, &[]).0;
        window.send_save_from_focus();
        (
            chain,
            [first_field, first, second_field, second],
            window.calls(),
        )
    });
    assert_eq!(chain, ids, "each overlay crossed once");
    assert!(calls.is_empty(), "the root is not on the way");
}

/// A row that, pressed with Enter, closes the overlay it is in and sends
/// `test.save` from itself, as a popover's button does.
fn closing_row(content: Rc<RefCell<Option<WidgetId>>>) -> impl Widget {
    use crate::widget_builder::WidgetBuilder;
    FillWidget::new()
        .focusable()
        .on_key(move |event, ctx| match event {
            WidgetEvent::KeyDown {
                key: Key::Enter, ..
            } => {
                let content = content.borrow().expect("the row is in an overlay");
                ctx.dismiss_overlay_by_content(content);
                ctx.send_intent(Intent::new("test.save"));
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        })
}

#[test]
fn an_intent_sent_as_its_overlay_closes_still_reaches_the_anchor() {
    let mut window = Window::new();
    let button = window.button;
    let content_slot = Rc::new(RefCell::new(None));
    let row = window.tree.add(closing_row(content_slot.clone()));
    let content = window.tree.add(StackWidget::new().child(row));
    *content_slot.borrow_mut() = Some(content);
    window.show(content, button);
    window.tree.focus(row);

    window.tree.press_key(Key::Enter, Modifiers::NONE);

    assert!(
        window
            .tree
            .overlay_manager()
            .find_by_content(content)
            .is_none(),
        "the row closed its overlay"
    );
    assert_eq!(window.calls(), ["root"]);
}

/// A scoped shortcut does not cross from a popup to the widget it is
/// attached to, as a key pressed in a Qt popup (a window of its own) or a GTK
/// popover (whose entry hears it first) does not: a list's Delete must not
/// fire from the field of a rename popover attached to a row. Its intent,
/// sent from the same focus, still crosses.
#[test]
fn a_scoped_shortcut_does_not_cross_from_an_overlay_to_its_anchor() {
    let mut window = Window::new();
    let button = window.button;
    let field = window.focused_popup(button);
    window.tree.press_key(Key::S, Modifiers::COMMAND);
    assert!(window.calls().is_empty(), "the root's Ctrl+S stays outside");
    assert_eq!(window.tree.focused(), Some(field));
    window.send_save_from_focus();
    assert_eq!(window.calls(), ["root"], "the intent still crosses");
}

#[test]
fn a_scoped_shortcut_does_not_apply_to_an_overlay_attached_outside_its_scope() {
    let mut window = Window::new();
    // A second root, outside the first one's scope, with its own widget to
    // attach to.
    let elsewhere = window.tree.add(FillWidget::new());
    window.tree.layout(SizeProposal::exact(400.0, 300.0));
    window.focused_popup(elsewhere);
    window.tree.press_key(Key::S, Modifiers::COMMAND);
    assert!(window.calls().is_empty());
}

/// A menu row that, chosen with Enter, closes its menu the way `close`
/// says and sends `test.save` from focus, as a menu-bar row does.
fn row_sending_from_focus(close: fn(&mut crate::widget::EventContext)) -> impl Widget {
    use crate::widget_builder::WidgetBuilder;
    FillWidget::new()
        .focusable()
        .on_key(move |event, ctx| match event {
            WidgetEvent::KeyDown {
                key: Key::Enter, ..
            } => {
                close(ctx);
                ctx.send_intent_from_focus(Intent::new("test.save"));
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        })
}

/// A menu attached to the window's button holding a row made by `row`,
/// opened over the editor (focus goes back there when the menu closes),
/// with focus on the row. Returns the row.
fn menu_over_the_editor(window: &mut Window, row: impl Widget + 'static) -> WidgetId {
    let (button, editor) = (window.button, window.editor);
    let row = window.tree.add(row);
    let menu = window.tree.add(StackWidget::new().child(row));
    window.tree.focus(editor);
    window.show(menu, button);
    window
        .tree
        .overlay_manager_mut()
        .set_top_focus_restore(editor);
    window.tree.focus(row);
    row
}

#[test]
fn a_rows_intent_from_focus_starts_where_closing_its_menu_put_focus() {
    let mut window = Window::new();
    menu_over_the_editor(
        &mut window,
        row_sending_from_focus(|ctx| ctx.dismiss_self_overlay_chain()),
    );
    window.tree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(window.tree.focused(), Some(window.editor));
    assert_eq!(
        window.calls(),
        ["editor"],
        "the editor focus went back to answers, not the menu's button"
    );
}

#[test]
fn with_no_focus_left_a_rows_intent_from_focus_crosses_its_closed_menu() {
    let mut window = Window::new();
    let button = window.button;
    let row = window
        .tree
        .add(row_sending_from_focus(|ctx| ctx.dismiss_all_overlays()));
    let menu = window.tree.add(StackWidget::new().child(row));
    window.show(menu, button);
    window.tree.focus(row);
    window.tree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(window.tree.focused(), None, "nothing handed focus back");
    assert_eq!(window.calls(), ["root"], "from the row, through the button");
}

/// A panel answering `test.save` with "panel", holding a focusable field whose
/// context-menu factory sends `test.save` and declines to show a menu.
#[derive(Debug)]
struct Panel {
    calls: Calls,
    field: Rc<RefCell<Option<WidgetId>>>,
}

impl Widget for Panel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::widget_builder::WidgetBuilder;
        let calls = self.calls.clone();
        ctx.register_action(Action::new("test.save").on_invoke(move |_, _| {
            calls.borrow_mut().push("panel");
        }));
        let field = ctx.add(FillWidget::new().focusable().context_menu(|_, ctx| {
            ctx.send_intent(Intent::new("test.save"));
            None
        }));
        *self.field.borrow_mut() = Some(field);
        vec![field]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 20.0).into()
    }
}

/// The window's root over a [`Panel`], answering `test.save` with "root".
#[derive(Debug)]
struct PanelRoot {
    calls: Calls,
    field: Rc<RefCell<Option<WidgetId>>>,
}

impl Widget for PanelRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let calls = self.calls.clone();
        ctx.register_action(Action::new("test.save").on_invoke(move |_, _| {
            calls.borrow_mut().push("root");
        }));
        vec![ctx.add(Panel {
            calls: self.calls.clone(),
            field: self.field.clone(),
        })]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(400.0, 300.0).into()
    }
}

#[test]
fn the_intent_of_a_declining_context_menu_factory_starts_where_the_menu_was_asked_for() {
    let calls = Calls::default();
    let field = Rc::new(RefCell::new(None));
    let mut tree = WidgetTree::new();
    tree.add(PanelRoot {
        calls: calls.clone(),
        field: field.clone(),
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(field.borrow().expect("the panel built its field"));
    tree.press_key(Key::F10, Modifiers::SHIFT);
    assert_eq!(
        *calls.borrow(),
        ["panel"],
        "the walk starts at the field, so the panel around it answers first"
    );
}

// An open modal overlay (a dialog, a command palette) is a barrier: the walk
// from its content stops there, as a native modal window's tree never reaches
// its parent's actions. Its own actions take its intents, and of the window's
// global actions and shortcuts only those registered inside it apply: a
// native modal window's tree has no others. The application tree still takes
// the intents nothing in the window takes. Once the modal has closed, an
// intent sent as it closed walks on as from any other overlay.

/// Registers a window-global action for `test.global` and a global shortcut
/// (Ctrl+G) for it, recording "global".
#[derive(Debug)]
struct GlobalCommands {
    calls: Calls,
}

impl Widget for GlobalCommands {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let calls = self.calls.clone();
        ctx.register_action_global(Action::new("test.global").on_invoke(move |_, _| {
            calls.borrow_mut().push("global");
        }));
        ctx.register_shortcut_global(
            Shortcut::new("test.global")
                .primary(KeyStroke::ctrl(Key::G))
                .build(),
        );
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

impl Window {
    /// A focusable field in a new modal attached to `anchor`, focused.
    fn focused_modal(&mut self, anchor: WidgetId) -> (WidgetId, WidgetId) {
        let field = self.tree.add(FillWidget::new().focusable());
        let content = self.tree.add(StackWidget::new().child(field));
        self.tree.show_overlay(OverlayRequest::new(
            content,
            anchor,
            OverlayPlacement::Centered,
            DismissBehavior::Manual,
        ));
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        self.tree.focus(field);
        (content, field)
    }

    fn with_global_commands(mut self) -> Self {
        self.tree.add(GlobalCommands {
            calls: self.calls.clone(),
        });
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        self
    }
}

#[test]
fn focus_in_a_modal_reaches_no_scoped_action_outside_it() {
    let mut window = Window::new();
    let button = window.button;
    window.focused_modal(button);
    window.send_save_from_focus();
    assert!(
        window.calls().is_empty(),
        "the root's scoped action is behind the modal"
    );
}

#[test]
fn a_popover_inside_a_modal_stops_at_the_modal() {
    let mut window = Window::new();
    let button = window.button;
    let (_, field) = window.focused_modal(button);
    window.focused_popup(field);
    window.send_save_from_focus();
    assert!(window.calls().is_empty());
}

#[test]
fn a_scoped_shortcut_outside_a_modal_does_not_apply_to_focus_in_it() {
    let mut window = Window::new();
    let button = window.button;
    window.focused_modal(button);
    window.tree.press_key(Key::S, Modifiers::COMMAND);
    assert!(window.calls().is_empty());
}

#[test]
fn a_modals_intent_reaches_no_global_action_of_the_page() {
    let mut window = Window::new().with_global_commands();
    let button = window.button;
    window.focused_modal(button);
    window
        .tree
        .run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.send_intent_from_focus(Intent::new("test.global"));
        });
    assert!(window.calls().is_empty());
}

#[test]
fn a_global_shortcut_of_the_page_does_not_fire_with_focus_in_a_modal() {
    let mut window = Window::new().with_global_commands();
    let button = window.button;
    window.focused_modal(button);
    window.tree.press_key(Key::G, Modifiers::COMMAND);
    assert!(window.calls().is_empty());
}

/// With a modal open and nothing focused (focus cleared by a click on the
/// dialog's background, or the dialog took none), a key is judged as if focus
/// were in the topmost modal: the page's global shortcut stays out.
#[test]
fn a_global_shortcut_of_the_page_does_not_fire_under_a_modal_with_nothing_focused() {
    let mut window = Window::new().with_global_commands();
    let button = window.button;
    window.focused_modal(button);
    window.tree.clear_focus();
    assert_eq!(window.tree.focused(), None);
    window.tree.press_key(Key::G, Modifiers::COMMAND);
    assert!(window.calls().is_empty());
}

/// A command sent from focus with nothing focused while a modal is up (a
/// native menu item, an app callback) starts in the modal, as if focus were
/// there, not at the page's root behind it.
#[test]
fn an_intent_from_focus_with_nothing_focused_under_a_modal_starts_in_it() {
    let mut window = Window::new().with_global_commands();
    let button = window.button;
    window.focused_modal(button);
    window.tree.clear_focus();
    window
        .tree
        .run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.send_intent_from_focus(Intent::new("test.save"));
            ctx.send_intent_from_focus(Intent::new("test.global"));
        });
    assert!(
        window.calls().is_empty(),
        "neither the root nor the page's global"
    );
}

/// ... and a global shortcut registered inside the modal applies, its intent
/// starting in the modal.
#[test]
fn a_modals_own_global_shortcut_fires_in_it_with_nothing_focused() {
    let mut window = Window::new();
    let button = window.button;
    let commands = window.tree.add(GlobalCommands {
        calls: window.calls.clone(),
    });
    let content = window.tree.add(StackWidget::new().child(commands));
    window.tree.show_overlay(OverlayRequest::new(
        content,
        button,
        OverlayPlacement::Centered,
        DismissBehavior::Manual,
    ));
    window.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(window.tree.focused(), None);
    window.tree.press_key(Key::G, Modifiers::COMMAND);
    assert_eq!(window.calls(), ["global"]);
}

/// A widget that, tapped, closes the modal it is in and sends what `send`
/// sends, as a dialog's button or a command palette's row does.
struct ClosingButton {
    send: fn(&mut crate::widget::EventContext),
}

impl std::fmt::Debug for ClosingButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClosingButton")
    }
}

impl Widget for ClosingButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let send = self.send;
        ctx.apply_self_handlers(
            crate::widget_builder::HandlerSet::new().on_tap(move |_, ctx| {
                ctx.dismiss_modal();
                send(ctx);
            }),
        );
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 20.0).into()
    }
}

impl Window {
    /// A modal attached to `anchor` holding a [`ClosingButton`] that sends
    /// what `send` sends, clicked. Focus is wherever it was before.
    fn click_closing_button_in_a_modal(
        &mut self,
        anchor: WidgetId,
        send: fn(&mut crate::widget::EventContext),
    ) {
        let closing = self.tree.add(ClosingButton { send });
        let content = self.tree.add(StackWidget::new().child(closing));
        self.tree.show_overlay(OverlayRequest::new(
            content,
            anchor,
            OverlayPlacement::Centered,
            DismissBehavior::Manual,
        ));
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        self.tree.click(closing);
        assert!(self.tree.active_overlays().is_empty(), "the modal closed");
    }
}

/// A modal is a barrier only while it is open: an intent sent as it closes
/// is dispatched once it has, and walks on from the widget it was attached
/// to, as a dialog's button that closes the dialog and then asks the page
/// for something expects.
#[test]
fn an_intent_sent_as_its_modal_closes_walks_on_from_its_anchor() {
    let mut window = Window::new().with_global_commands();
    let button = window.button;
    window.click_closing_button_in_a_modal(button, |ctx| {
        ctx.send_intent(Intent::new("test.save"));
        ctx.send_intent(Intent::new("test.global"));
    });
    assert_eq!(
        window.calls(),
        ["root", "global"],
        "the root above the anchor, and the page's global action"
    );
}

/// Sent from focus as its modal closes with nothing focused before it opened
/// (a command palette opened from a shortcut with nothing focused), the
/// intent starts at the content root, as one with no source widget does: not
/// at the modal's anchor, which focus was never on.
#[test]
fn an_intent_from_focus_sent_as_its_modal_closes_with_nothing_focused_starts_at_the_root() {
    let mut window = Window::new().with_global_commands();
    let editor = window.editor;
    assert_eq!(window.tree.focused(), None);
    window.click_closing_button_in_a_modal(editor, |ctx| {
        ctx.send_intent_from_focus(Intent::new("test.save"));
        ctx.send_intent_from_focus(Intent::new("test.global"));
    });
    assert_eq!(window.tree.focused(), None);
    assert_eq!(window.calls(), ["root", "global"]);
}

/// With focus to give back, closing the modal puts it there and the intent
/// sent from focus starts there.
#[test]
fn an_intent_from_focus_sent_as_its_modal_closes_starts_at_the_focus_given_back() {
    let mut window = Window::new().with_global_commands();
    let (button, editor) = (window.button, window.editor);
    window.tree.focus(editor);
    let closing = window.tree.add(ClosingButton {
        send: |ctx| ctx.send_intent_from_focus(Intent::new("test.save")),
    });
    let content = window.tree.add(StackWidget::new().child(closing));
    window.tree.show_overlay(OverlayRequest::new(
        content,
        button,
        OverlayPlacement::Centered,
        DismissBehavior::Manual,
    ));
    window
        .tree
        .overlay_manager_mut()
        .set_top_focus_restore(editor);
    window.tree.layout(SizeProposal::exact(400.0, 300.0));
    window.tree.click(closing);
    assert_eq!(window.tree.focused(), Some(editor));
    assert_eq!(window.calls(), ["editor"]);
}

#[test]
fn global_actions_and_shortcuts_registered_inside_a_modal_apply_in_it() {
    let mut window = Window::new();
    let button = window.button;
    let field = window.tree.add(FillWidget::new().focusable());
    let commands = window.tree.add(GlobalCommands {
        calls: window.calls.clone(),
    });
    let content = window
        .tree
        .add(StackWidget::new().child(field).child(commands));
    window.tree.show_overlay(OverlayRequest::new(
        content,
        button,
        OverlayPlacement::Centered,
        DismissBehavior::Manual,
    ));
    window.tree.layout(SizeProposal::exact(400.0, 300.0));
    window.tree.focus(field);
    window
        .tree
        .run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.send_intent_from_focus(Intent::new("test.global"));
        });
    window.tree.press_key(Key::G, Modifiers::COMMAND);
    assert_eq!(window.calls(), ["global", "global"]);
}

/// An intent nothing in the window takes still goes to the application tree.
#[test]
fn a_modals_intent_nothing_in_it_takes_goes_to_the_application() {
    let mut window = Window::new().with_global_commands();
    let scope = crate::action::ApplicationScope::new();
    scope.replace(vec![("test.global", None)]);
    window.tree.set_application_scope(Some(scope));
    window.tree.set_records_unclaimed_intents(true);
    let button = window.button;
    window.focused_modal(button);
    window
        .tree
        .run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.send_intent_from_focus(Intent::new("test.global"));
        });
    assert!(window.calls().is_empty(), "not the page's global action");
    let kept: Vec<_> = window
        .tree
        .take_unclaimed_intents()
        .into_iter()
        .map(|intent| intent.name)
        .collect();
    assert_eq!(kept, ["test.global"]);
}

/// An intent sent from outside the modal (programmatically: no input reaches
/// the page while it is up) is the page's and reaches its global actions, as
/// before.
#[test]
fn an_intent_sent_from_outside_a_modal_still_reaches_the_page_globals() {
    let mut window = Window::new().with_global_commands();
    let button = window.button;
    window.focused_modal(button);
    let page = window.tree.roots()[0];
    window
        .tree
        .run_with_event_context_from(Some(page), &mut NoopWindowOps, |ctx| {
            ctx.send_intent(Intent::new("test.global"));
        });
    assert_eq!(window.calls(), ["global"]);
}

#[test]
fn a_popover_attached_inside_a_modal_reaches_the_modals_own_actions() {
    let mut window = Window::new();
    let button = window.button;
    let editor = window.tree.add(Editor {
        calls: window.calls.clone(),
    });
    let content = window.tree.add(StackWidget::new().child(editor));
    window.tree.show_overlay(OverlayRequest::new(
        content,
        button,
        OverlayPlacement::Centered,
        DismissBehavior::Manual,
    ));
    window.tree.layout(SizeProposal::exact(400.0, 300.0));
    window.focused_popup(editor);
    window.send_save_from_focus();
    assert_eq!(
        window.calls(),
        ["editor"],
        "the dialog's editor, not the root"
    );
}
