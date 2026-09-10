// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The inspector's coarse-pointer door: a corner grip that opens the panel on a
//! **hold**.
//!
//! F12 is the only gesture-free way into the inspector, and a tablet has no
//! F12. `InspectorState::toggle` gives an application its own door — a menu
//! item, a debug button — but the framework owes one too, or the inspector is
//! simply unreachable on a machine with no keyboard.
//!
//! # Why a hold, and why a corner
//!
//! A hold, because a *tap* in the corner is a tap the application may want and
//! a debug tool must not take one it does not need. A corner, because the grip
//! has to sit somewhere and this is the corner every desktop already reserves
//! for a resize affordance.
//!
//! # What it costs, exactly
//!
//! The node fills the window but is hittable **only** inside the corner square
//! ([`Widget::hit_shape`]), so a press anywhere else finds the application
//! exactly as it did before. Inside the square the grip takes the press — a
//! tap there does nothing and reaches nothing — which is a real cost, paid
//! only where all three of these hold: a debug build, a session that has
//! already seen a finger (`InspectorState::coarse_pointer`), and a closed
//! panel. A mouse-driven app never mounts it at all, so the programme's
//! mouse-unchanged invariant holds by construction rather than by care.
//!
//! It paints a small chevron so the cost is never silent.

use std::cell::Cell;

use teksilo_canvas::{Canvas, Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius};

use crate::state::InspectorState;

/// Size of the square the grip actually claims, in logical pixels, when the
/// density has not been asked. Raised to the density's target size at build
/// time — this is the floor, not the answer.
const GRIP_BASE: f32 = 24.0;

pub(crate) struct InspectorGrip {
    state: InspectorState,
    /// The grip's own square, in widget-local coordinates, published by
    /// `layout_response` for `hit_shape` and `paint`. `hit_shape` receives a
    /// point and the bounds but no theme and no layout direction, so both have
    /// to be resolved where they are available.
    square: Cell<Rect>,
}

impl InspectorGrip {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            square: Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)),
        }
    }

    /// The corner square in widget-local coordinates: bottom-**trailing**, so
    /// the right-hand corner in an LTR window and the left-hand one in RTL.
    fn corner(size: f32, bounds: Rect, rtl: bool) -> Rect {
        let side = size.min(bounds.width).min(bounds.height);
        let x = if rtl { 0.0 } else { bounds.width - side };
        Rect::new(x, bounds.height - side, side, side)
    }
}

impl std::fmt::Debug for InspectorGrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorGrip").finish()
    }
}

impl Widget for InspectorGrip {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();
        let at_state = state.clone();
        let handlers = HandlerSet::new()
            .focusable(false)
            // The hold is the grip's own, so the tree-owned long-press route
            // never resolves here (a widget's `on_long_press` wins) and no
            // context menu or tooltip can be summoned from the corner.
            .on_long_press(move |_event, _ctx| state.toggle())
            .on_access_action(move |action, _ctx| {
                if matches!(action, teksilo_core::accesskit::Action::Click) {
                    at_state.toggle();
                    return teksilo_core::event::EventResponse::Handled;
                }
                teksilo_core::event::EventResponse::Ignored
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Fill what we are given; report nothing on an intrinsic query so the
        // enclosing ZStack is never inflated by a decoration.
        let w = proposal.width.unwrap_or(0.0);
        let h = proposal.height.unwrap_or(0.0);
        // The grip exists only for a finger, so it takes the density's own
        // target size outright rather than a floor over a smaller paint.
        let size = GRIP_BASE.max(ctx.theme.input.target_size);
        self.square
            .set(Self::corner(size, Rect::new(0.0, 0.0, w, h), ctx.is_rtl()));
        teksilo_canvas::Size::new(w, h).into()
    }

    fn hit_shape(&self, local_point: Point, _bounds: Rect) -> bool {
        self.square.get().contains(local_point)
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let square = self.square.get();
        let rect = Rect::new(
            bounds.x + square.x,
            bounds.y + square.y,
            square.width,
            square.height,
        );
        // Deliberately visible: the grip takes presses in this square, so it
        // must be findable and it must not be a secret.
        canvas.fill_rounded_rect(
            rect,
            CornerRadius::uniform(4.0),
            Color::from_rgba(0.13, 0.55, 1.0, 0.18),
        );
        let bar = Color::from_rgba(0.13, 0.55, 1.0, 0.75);
        let inset = rect.width * 0.25;
        for i in 0..3 {
            let y = rect.y + inset + (i as f32) * (rect.height - inset * 2.0) * 0.5 - 0.5;
            canvas.fill_rounded_rect(
                Rect::new(rect.x + inset, y, rect.width - inset * 2.0, 1.5),
                CornerRadius::ZERO,
                bar,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A real, operable affordance in a debug build: an AT user gets the
        // same door by activating it, which is why the action is advertised and
        // handled rather than left to the hold alone.
        builder.set_role(teksilo_core::accesskit::Role::Button);
        builder.set_name("Open the Teksilo inspector");
        builder.add_action(teksilo_core::accesskit::Action::Click);
    }
}
