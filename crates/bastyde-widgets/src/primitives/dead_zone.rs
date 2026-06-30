// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`DeadZone`] — a gesture **dead zone** wrapper.

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;

/// A layout-transparent wrapper whose subtree is a **gesture dead zone**: a
/// pointer press inside it never arms a drag/swipe recognizer on any ancestor.
///
/// Wrap interactive controls (buttons, a `⋮` options menu, a slider) that sit
/// **inside a draggable / swipeable container** — a dock-panel header, a card, a
/// list row, a scene item — so clicking them, *even with the few pixels of
/// pointer jitter a real click carries*, can never start the ancestor's drag.
/// The container's own drag still works everywhere outside the dead zone. This
/// is the framework counterpart of Electron's `-webkit-app-region: no-drag`.
///
/// It is robust **structurally**, not by a timing-dependent gesture race: it
/// sets the node-level [`gesture_dead_zone`](bastyde_core::widget_builder::WidgetBuilder::gesture_dead_zone)
/// flag, which the framework's drag-arming honours by refusing to arm any
/// ancestor above this node. (It also carries a no-op tap/drag so a press on the
/// dead zone's own bare area — a gap between controls — is absorbed too.)
///
/// ```ignore
/// // A draggable dock header whose action buttons don't drag the panel:
/// HStack::new()
///     .child(title)
///     .child(DeadZone::new().child(
///         HStack::new()
///             .child(IconButton::new(new_icon).on_activate_fn(..))
///             .child(options_button),
///     ))
/// ```
///
/// Layout-transparent: it reports its child's size and fills the child to its
/// own bounds, so dropping it in is size-neutral.
pub struct DeadZone {
    child: Option<WidgetId>,
    pending: Option<Box<dyn Widget>>,
}

impl DeadZone {
    /// A new, empty dead zone. Attach content with [`child`](Self::child) or
    /// [`child_id`](Self::child_id).
    pub fn new() -> Self {
        Self {
            child: None,
            pending: None,
        }
    }

    /// Wrap an inline widget.
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending = Some(Box::new(widget));
        self
    }

    /// Wrap a pre-registered widget by id.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.child = Some(id);
        self
    }
}

impl Default for DeadZone {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DeadZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeadZone").finish()
    }
}

impl Widget for DeadZone {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending.take() {
            self.child = Some(ctx.add_boxed(pending));
        }
        // The structural block (the flag) handles a press on a descendant
        // control; the no-op tap/drag absorbs a press on the dead zone's own
        // bare area (the captured widget is then the dead zone itself, which the
        // existing innermost-can-drag skip catches).
        ctx.apply_self_handlers(
            HandlerSet::new()
                .gesture_dead_zone(true)
                .on_tap(|_e, _ctx| {})
                .on_drag(|_phase, _ctx| {}),
        );
        self.child.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child
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
        self.child.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon_button::IconButton;
    use crate::primitives::IconWidget;
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
    use bastyde_core::widget_builder::WidgetBuilder;
    use bastyde_core::widget_tree::WidgetTree;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn dead_zone_blocks_ancestor_drag_but_lets_the_button_click() {
        // A draggable ancestor with a DeadZone-wrapped button inside it: a
        // jittery press on the button activates it WITHOUT starting the
        // ancestor's drag.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let dragged = Rc::new(Cell::new(false));
        let clicked = Rc::new(Cell::new(false));
        let d = dragged.clone();
        let c = clicked.clone();
        let button = tree
            .add(IconButton::new(IconWidget::checkmark(16.0)).on_activate_fn(move |_| c.set(true)));
        let dead = tree.add(DeadZone::new().child_id(button));
        let ancestor = tree.add(crate::primitives::HStack::new().add_child(dead).on_drag(
            move |phase, _ctx| {
                if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
                    d.set(true);
                }
            },
        ));
        tree.layout(SizeProposal::exact(120.0, 60.0));

        let b = tree.bounds(button);
        let (cx, cy) = (b.x + b.width / 2.0, b.y + b.height / 2.0);
        // Clean click activates the button.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(cx, cy),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(cx, cy),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert!(
            clicked.get(),
            "the button inside the dead zone still activates"
        );

        // A jittery press (down + several small moves + up) must NOT drag the
        // ancestor.
        tree.pointer_down_button(Point::new(cx, cy), PointerButton::Primary);
        for i in 1..=10 {
            tree.pointer_move(Point::new(cx + (i as f32) * 3.0, cy + 1.0));
        }
        tree.pointer_up_button(Point::new(cx + 30.0, cy + 1.0), PointerButton::Primary);
        assert!(
            !dragged.get(),
            "a jittery press on the dead-zone button must not start the ancestor drag"
        );
        let _ = ancestor;
    }
}
