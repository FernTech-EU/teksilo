// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where an item chosen in the native (macOS) menu bar acts.
//!
//! The bar shows one window's menu, and the item knows that window: its
//! *owner*, whose tree built the menu and holds the state the item's action
//! captured (a checkable entry's signal, a closure over the window's widgets).
//! The command itself is for the window the user is working in, the one with
//! the OS focus: macOS keeps the previous bar up while a window with no menu
//! of its own (a secondary window, a modal dialog) is key, and ⌘ equivalents
//! go to that bar too. So the activation is split:
//!
//! - its **action** (with the state write a check or radio entry makes) runs
//!   in the owner's tree, anchored at the content root, as an app callback
//!   is: an overlay it shows belongs to the window, not to a popover that
//!   happens to hold focus;
//! - its **intent** is sent from the focused widget of the window with the
//!   OS focus ([`native_menu_intent_window`]), after the action, as the
//!   item's shortcut would be: Edit ▸ Copy copies in the focused field of the
//!   dialog in front. A row is input, as a key is: while a dialog presented
//!   in that window (a modal overlay) is open, its intent stops at the
//!   dialog ([`Intent::kept_in_modal`]), and what the window leaves goes to
//!   the application tree, never to the page behind, as AppKit disables a
//!   document's items while a modal panel is up.
//!
//! In the usual case the two windows are one, and both run in one context.

use teksilo_core::telemetry::IntentSource;
use teksilo_core::{Intent, WidgetTree, WindowOps};
use teksilo_platform::native_menu::MenuActionFn;

use crate::window_config::TeksiloWindowId;

/// The window a native menu item's intent goes to: `focused`, the window
/// with the OS focus now, whether or not it has a menu of its own; `owner`,
/// the window whose menu the item is in, when no window has the focus (every
/// window minimised, the app still active).
pub(crate) fn native_menu_intent_window(
    owner: TeksiloWindowId,
    focused: Option<TeksiloWindowId>,
) -> TeksiloWindowId {
    focused.unwrap_or(owner)
}

/// Every intent a row of the native menu bar would have handled in `tree`,
/// the window with the OS focus, sorted: what the walk from where
/// [`fire_native_menu_item`] sends a row's intent finds, kept in an open
/// in-window dialog as a key's is, so a row greys out exactly when choosing
/// it would do nothing.
pub(crate) fn native_menu_handled_intents(tree: &WidgetTree) -> Vec<&'static str> {
    tree.live_command_focus()
        .map(|source| tree.handled_intents_kept_in_modal_from(source))
        .unwrap_or_default()
}

/// Whether a row of the native menu bar whose intent is `name` would be
/// handled in `tree`, the window with the OS focus, as
/// [`native_menu_handled_intents`] answers.
pub(crate) fn native_menu_handles(tree: &WidgetTree, name: &str) -> bool {
    tree.live_command_focus()
        .is_some_and(|source| tree.can_handle_kept_in_modal_from(source, name))
}

/// Run part of a native menu item's activation in `tree`, tagged
/// [`IntentSource::Menu`]: `action` anchored at the tree's content root,
/// then `intent` sent from its focused widget (from the content root when
/// nothing is focused), [kept](Intent::kept_in_modal) in a dialog presented
/// in the window, as a key's is. The route passes the action to the owner's
/// tree and the intent to the focused window's, or both to one tree.
pub(crate) fn fire_native_menu_item(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    action: Option<&MenuActionFn>,
    intent: Option<&'static str>,
) {
    fire_menu_row(
        tree,
        ops,
        action,
        intent.map(|name| Intent::new(name).kept_in_modal()),
    );
}

/// Run part of the activation of a row of the application menu chosen in a
/// window's own menu bar, as [`fire_native_menu_item`] does for the native
/// bar, except that its intent goes on past a dialog open in the window, as
/// any in-window menu row's does.
pub(crate) fn fire_window_menu_row(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    action: Option<&MenuActionFn>,
    intent: Option<&'static str>,
) {
    fire_menu_row(tree, ops, action, intent.map(Intent::new));
}

fn fire_menu_row(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    action: Option<&MenuActionFn>,
    intent: Option<Intent>,
) {
    tree.run_with_event_context(ops, |ctx| {
        ctx.with_intent_source(IntentSource::Menu, |ctx| {
            if let Some(action) = action {
                action(ctx);
            }
            if let Some(intent) = intent {
                ctx.send_intent_from_focus(intent);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::overlay::{DismissBehavior, OverlayPlacement, OverlayRequest};
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::widget_builder::HandlerSet;
    use teksilo_core::window::NoopWindowOps;
    use teksilo_core::{Action, WidgetId};

    fn win(raw: u64) -> TeksiloWindowId {
        TeksiloWindowId::new(raw)
    }

    #[test]
    fn the_intent_goes_to_the_focused_window_even_without_a_menu_of_its_own() {
        // Window 1 installed the menu; window 2, a dialog with none, is key.
        assert_eq!(native_menu_intent_window(win(1), Some(win(2))), win(2));
    }

    #[test]
    fn with_no_window_focused_the_intent_goes_to_the_menus_window() {
        assert_eq!(native_menu_intent_window(win(1), None), win(1));
        assert_eq!(native_menu_intent_window(win(1), Some(win(1))), win(1));
    }

    type Heard = Rc<RefCell<Vec<&'static str>>>;

    /// A focusable widget answering `intent` with `name`.
    #[derive(Debug)]
    struct Copier {
        name: &'static str,
        intent: &'static str,
        heard: Heard,
        focusable: bool,
        children: Vec<Copier>,
        ids: Rc<RefCell<Vec<WidgetId>>>,
    }

    impl Widget for Copier {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let (name, heard) = (self.name, self.heard.clone());
            ctx.register_action(Action::new(self.intent).on_invoke(move |_, _| {
                heard.borrow_mut().push(name);
            }));
            ctx.apply_self_handlers(HandlerSet::new().focusable(self.focusable));
            self.ids.borrow_mut().push(ctx.self_id());
            std::mem::take(&mut self.children)
                .into_iter()
                .map(|child| ctx.add(child))
                .collect()
        }

        fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            proposal.resolve(100.0, 20.0).into()
        }
    }

    /// A window's tree: a root answering `test.copy` as "root" with an
    /// editor under it answering as "editor". Returns the tree, the root and
    /// the editor.
    fn window(heard: &Heard) -> (WidgetTree, WidgetId, WidgetId) {
        let ids = Rc::new(RefCell::new(Vec::new()));
        let editor = Copier {
            name: "editor",
            intent: "test.copy",
            heard: heard.clone(),
            focusable: true,
            children: Vec::new(),
            ids: ids.clone(),
        };
        let root = Copier {
            name: "root",
            intent: "test.copy",
            heard: heard.clone(),
            focusable: false,
            children: vec![editor],
            ids: ids.clone(),
        };
        let mut tree = WidgetTree::new();
        let root = tree.add(root);
        tree.set_content_root(root);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let editor = ids.borrow()[1];
        (tree, root, editor)
    }

    #[test]
    fn the_intent_starts_at_the_focused_widget() {
        let heard = Heard::default();
        let (mut tree, _, editor) = window(&heard);
        tree.focus(editor);
        fire_native_menu_item(&mut tree, &mut NoopWindowOps, None, Some("test.copy"));
        assert_eq!(*heard.borrow(), ["editor"]);
    }

    #[test]
    fn with_nothing_focused_the_intent_starts_at_the_windows_root() {
        let heard = Heard::default();
        let (mut tree, _, _) = window(&heard);
        fire_native_menu_item(&mut tree, &mut NoopWindowOps, None, Some("test.copy"));
        assert_eq!(*heard.borrow(), ["root"]);
    }

    /// The intent names the window it was sent in, the one
    /// [`native_menu_intent_window`] chose, also once it crosses to the
    /// application tree.
    #[test]
    fn the_intent_names_the_window_it_was_sent_in() {
        let heard = Heard::default();
        let (mut tree, _, _) = window(&heard);
        tree.set_records_unclaimed_intents(true);
        tree.set_window_state(teksilo_core::WindowState::new(
            teksilo_core::WindowStateInit {
                id: win(2),
                string_id: None,
                placement: teksilo_core::WindowPlacement::Floating,
                title: String::new(),
                size: (400, 300),
                position: (0, 0),
                focused: true,
                resizable: true,
                always_on_top: false,
            },
        ));
        fire_native_menu_item(&mut tree, &mut NoopWindowOps, None, Some("test.new"));
        let kept = tree.take_unclaimed_intents();
        assert_eq!(kept.len(), 1, "nothing in the window takes it");
        assert_eq!(kept[0].origin_window(), Some(win(2)));
    }

    /// A window whose editor answers `test.editor_copy` and whose popover,
    /// attached to a toolbar button and opened over the editor, holds focus
    /// in a field answering `test.field_copy`. Returns the tree.
    fn window_with_a_focused_popover(heard: &Heard) -> WidgetTree {
        let ids = Rc::new(RefCell::new(Vec::new()));
        let root = Copier {
            name: "root",
            intent: "test.copy",
            heard: heard.clone(),
            focusable: false,
            children: vec![
                Copier {
                    name: "button",
                    intent: "test.copy",
                    heard: heard.clone(),
                    focusable: true,
                    children: Vec::new(),
                    ids: ids.clone(),
                },
                Copier {
                    name: "editor",
                    intent: "test.editor_copy",
                    heard: heard.clone(),
                    focusable: true,
                    children: Vec::new(),
                    ids: ids.clone(),
                },
            ],
            ids: ids.clone(),
        };
        let mut tree = WidgetTree::new();
        let root = tree.add(root);
        tree.set_content_root(root);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let (button, editor) = (ids.borrow()[1], ids.borrow()[2]);
        // Only the editor and the field answer their own copy.
        let field = tree.add(Copier {
            name: "field",
            intent: "test.field_copy",
            heard: heard.clone(),
            focusable: true,
            children: Vec::new(),
            ids: ids.clone(),
        });
        tree.focus(editor);
        tree.show_overlay(OverlayRequest::new(
            field,
            button,
            OverlayPlacement::Below,
            DismissBehavior::Manual,
        ));
        tree.overlay_manager_mut().set_top_focus_restore(editor);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(field);
        tree
    }

    /// The bar greys a row exactly when choosing it would do nothing: its
    /// availability is asked from the widget its intent is sent from, the
    /// focused field in a popover, not from the editor the popover gives
    /// focus back to (which an in-window menu row asks from). Both ways:
    /// what the field takes is enabled, what only the editor takes greys.
    #[test]
    fn a_row_greys_from_the_widget_its_intent_is_sent_from() {
        for (name, by) in [("test.field_copy", "field"), ("test.editor_copy", "editor")] {
            let heard = Heard::default();
            let mut tree = window_with_a_focused_popover(&heard);
            let handled = native_menu_handled_intents(&tree);
            let available = native_menu_handles(&tree, name);
            assert_eq!(handled.contains(&name), available, "{name}: one answer");
            fire_native_menu_item(&mut tree, &mut NoopWindowOps, None, Some(name));
            let ran = heard.borrow().contains(&by);
            assert_eq!(
                available, ran,
                "{name}: the row says {available}, choosing it ran {by}: {ran}"
            );
        }
    }

    /// A page answering `doc.save` at its root, with a dialog presented in
    /// the window over it (a modal overlay attached to the page's editor)
    /// whose field answers `dialog.own` and holds focus. Returns the tree.
    fn window_with_a_dialog_open(heard: &Heard) -> WidgetTree {
        let ids = Rc::new(RefCell::new(Vec::new()));
        let root = Copier {
            name: "page",
            intent: "doc.save",
            heard: heard.clone(),
            focusable: false,
            children: vec![Copier {
                name: "editor",
                intent: "editor.copy",
                heard: heard.clone(),
                focusable: true,
                children: Vec::new(),
                ids: ids.clone(),
            }],
            ids: ids.clone(),
        };
        let mut tree = WidgetTree::new();
        let root = tree.add(root);
        tree.set_content_root(root);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let editor = ids.borrow()[1];
        tree.focus(editor);
        let field = tree.add(Copier {
            name: "dialog",
            intent: "dialog.own",
            heard: heard.clone(),
            focusable: true,
            children: Vec::new(),
            ids: ids.clone(),
        });
        tree.show_overlay(OverlayRequest::new(
            field,
            editor,
            OverlayPlacement::Centered,
            DismissBehavior::Manual,
        ));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(field);
        tree
    }

    /// A row of the native menu bar is input, as a key is: while a dialog
    /// presented in the key window is open, its intent stops at the dialog.
    /// A row only the page behind takes greys out and choosing it runs
    /// nothing there; one the dialog takes stays enabled and runs in it. The
    /// same intent sent by a button of the dialog does reach the page.
    #[test]
    fn a_row_chosen_while_an_in_window_dialog_is_open_stops_at_the_dialog() {
        let heard = Heard::default();
        let mut tree = window_with_a_dialog_open(&heard);
        assert!(
            !native_menu_handles(&tree, "doc.save"),
            "the page's row greys"
        );
        assert!(!native_menu_handled_intents(&tree).contains(&"doc.save"));
        fire_native_menu_item(&mut tree, &mut NoopWindowOps, None, Some("doc.save"));
        assert!(
            heard.borrow().is_empty(),
            "choosing it ran {:?}",
            heard.borrow()
        );

        assert!(
            native_menu_handles(&tree, "dialog.own"),
            "the dialog's row stays"
        );
        assert!(native_menu_handled_intents(&tree).contains(&"dialog.own"));
        fire_native_menu_item(&mut tree, &mut NoopWindowOps, None, Some("dialog.own"));
        assert_eq!(*heard.borrow(), ["dialog"]);

        heard.borrow_mut().clear();
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.send_intent_from_focus(Intent::new("doc.save"));
        });
        assert_eq!(*heard.borrow(), ["page"], "a button's intent walks on");
    }

    /// With the dialog open, a row only the application tree takes stays
    /// enabled, and its intent is kept for the application tree, as a key's
    /// is.
    #[test]
    fn a_row_the_application_tree_takes_stays_enabled_over_a_dialog() {
        let heard = Heard::default();
        let mut tree = window_with_a_dialog_open(&heard);
        let mut app = WidgetTree::new();
        let app_root = app.add(Copier {
            name: "application",
            intent: "app.new",
            heard: heard.clone(),
            focusable: false,
            children: Vec::new(),
            ids: Rc::default(),
        });
        app.set_content_root(app_root);
        let scope = teksilo_core::ApplicationScope::new();
        app.publish_application_scope(&scope);
        tree.set_application_scope(Some(scope));
        tree.set_records_unclaimed_intents(true);
        assert!(native_menu_handles(&tree, "app.new"));
        fire_native_menu_item(&mut tree, &mut NoopWindowOps, None, Some("app.new"));
        let kept = tree.take_unclaimed_intents();
        assert_eq!(kept.iter().map(|i| i.name).collect::<Vec<_>>(), ["app.new"]);
    }

    #[test]
    fn an_action_shows_its_overlay_from_the_root_not_from_a_focused_popover() {
        let heard = Heard::default();
        let (mut tree, root, editor) = window(&heard);
        // A popover attached to the editor, with focus in it.
        let field = tree.add(teksilo_widgets::primitives::Spacer::new());
        tree.show_overlay(OverlayRequest::new(
            field,
            editor,
            OverlayPlacement::Below,
            DismissBehavior::Manual,
        ));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(field);

        // The item's action opens a panel of its own.
        let panel = tree.add(teksilo_widgets::primitives::Spacer::new());
        let shown = Rc::new(Cell::new(false));
        let action: MenuActionFn = {
            let shown = shown.clone();
            Rc::new(move |ctx| {
                ctx.show_overlay(OverlayRequest::new(
                    panel,
                    root,
                    OverlayPlacement::Centered,
                    DismissBehavior::Manual,
                ));
                shown.set(true);
            })
        };
        fire_native_menu_item(&mut tree, &mut NoopWindowOps, Some(&action), None);
        assert!(shown.get());
        assert!(tree.overlay_manager().find_by_content(panel).is_some());

        // Closing the popover leaves the panel: it does not belong to it.
        let popover = tree.overlay_manager().find_by_content(field).unwrap();
        tree.dismiss_overlay(popover);
        assert!(
            tree.overlay_manager().find_by_content(panel).is_some(),
            "the panel went with the popover that happened to hold focus"
        );
    }
}
