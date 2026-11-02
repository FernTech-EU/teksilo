// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An assistive `Focus` on an item a widget points at through its
//! `active_descendant`.
//!
//! A data view keeps keyboard focus on itself and names its current row or
//! cell to a screen reader as its active descendant. The item must still say
//! it can take focus: `accesskit_consumer` answers UIA's
//! `IsKeyboardFocusable` from the node's own `Action::Focus` alone, so an item
//! that did not advertise it read "not focusable" while it had the focus, and
//! a client that asks for focus on it had nothing to ask. Advertised, the
//! action has to do what the keyboard does: move the widget's cursor to the
//! item and give the widget keyboard focus. The router offers an assistive
//! `Focus` on a node that is not focusable itself to the widgets on its path
//! first, so the owner's handler, installed with [`on_item_focus`], is where
//! it lands.

use teksilo_core::accessibility::node_id_to_widget_id_maybe;
use teksilo_core::accesskit::{Action, ActionData, NodeId};
use teksilo_core::event::EventResponse;
use teksilo_core::widget::EventContext;
use teksilo_core::widget_id::WidgetId;

/// An `on_access_action_request` handler for `owner`, the widget that keeps
/// focus and points at its items.
///
/// `move_to` is given the item's widget id and moves the owner's cursor there,
/// returning `false` for a node that is not one of its items. The handler then
/// asks for keyboard focus on `owner`. Every other action is ignored, so it
/// passes on.
pub(crate) fn on_item_focus(
    owner: WidgetId,
    mut move_to: impl FnMut(WidgetId, &mut EventContext) -> bool + 'static,
) -> impl FnMut(Action, NodeId, Option<ActionData>, &mut EventContext) -> EventResponse + 'static {
    move |action, node, _data, ctx| {
        if action != Action::Focus {
            return EventResponse::Ignored;
        }
        let Some(item) = node_id_to_widget_id_maybe(node) else {
            return EventResponse::Ignored;
        };
        if item == owner || !move_to(item, ctx) {
            return EventResponse::Ignored;
        }
        ctx.request_focus(owner);
        EventResponse::Handled
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! What an adapter makes of an item, asked through `accesskit_consumer`
    //! as the adapters ask it, and an assistive `Focus` on it.

    use teksilo_core::accesskit::{Action, NodeId};
    use teksilo_core::widget_tree::WidgetTree;

    /// Whether `item` reads as focusable to a platform adapter
    /// (`Node::is_focusable`, which UIA's `IsKeyboardFocusable` reports),
    /// asserting it is not the focus itself: the focus always reads as
    /// focusable, whatever it advertises.
    #[track_caller]
    pub(crate) fn reads_as_focusable(tree: &mut WidgetTree, item: NodeId) -> bool {
        let consumer = accesskit_consumer::Tree::new(tree.sync_accessibility(), true);
        let state = consumer.state();
        let mut stack = vec![state.root()];
        while let Some(node) = stack.pop() {
            if node.locate().0 == item {
                assert!(
                    !node.is_focused_in_tree(),
                    "the item is the tree's focus, so it reads as focusable \
                     whatever it advertises"
                );
                return node.is_focusable(&accesskit_consumer::common_filter);
            }
            stack.extend(node.children());
        }
        panic!("{item:?} is not in the accessibility tree");
    }

    /// The node the platform reads as focused: the focus's active descendant,
    /// or the focus itself.
    pub(crate) fn platform_focus(tree: &mut WidgetTree) -> Option<NodeId> {
        let update = tree.sync_accessibility();
        let focus = update.focus;
        update
            .nodes
            .iter()
            .find(|(id, _)| *id == focus)
            .map(|(_, node)| node.active_descendant().unwrap_or(focus))
    }

    /// Dispatch an assistive `Focus` on `item`, as a platform adapter does,
    /// and report whether something took it.
    pub(crate) fn assistive_focus(tree: &mut WidgetTree, item: NodeId) -> bool {
        tree.dispatch_access_action(
            item,
            Action::Focus,
            None,
            &mut teksilo_core::window::NoopWindowOps,
        )
    }
}
