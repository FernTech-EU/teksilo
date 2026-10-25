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
//!   dialog in front.
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

/// Run part of a native menu item's activation in `tree`, tagged
/// [`IntentSource::Menu`]: `action` anchored at the tree's content root,
/// then `intent` sent from its focused widget (from the content root when
/// nothing is focused). The route passes the action to the owner's tree and
/// the intent to the focused window's, or both to one tree.
pub(crate) fn fire_native_menu_item(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    action: Option<&MenuActionFn>,
    intent: Option<&'static str>,
) {
    tree.run_with_event_context(ops, |ctx| {
        ctx.with_intent_source(IntentSource::Menu, |ctx| {
            if let Some(action) = action {
                action(ctx);
            }
            if let Some(name) = intent {
                ctx.send_intent_from_focus(Intent::new(name));
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

    /// A focusable widget answering `test.copy` with `name`.
    #[derive(Debug)]
    struct Copier {
        name: &'static str,
        heard: Heard,
        focusable: bool,
        children: Vec<Copier>,
        ids: Rc<RefCell<Vec<WidgetId>>>,
    }

    impl Widget for Copier {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let (name, heard) = (self.name, self.heard.clone());
            ctx.register_action(Action::new("test.copy").on_invoke(move |_, _| {
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
            heard: heard.clone(),
            focusable: true,
            children: Vec::new(),
            ids: ids.clone(),
        };
        let root = Copier {
            name: "root",
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
