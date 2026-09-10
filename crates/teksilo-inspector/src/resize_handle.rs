// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Top-edge resize handle for the inspector panel.
//!
//! A 6-pixel-tall horizontal strip sitting between the user-root area
//! and the panel switcher. While dragging, every `PointerMove` resizes
//! `state.panel_height` so the handle's top edge tracks the cursor
//! exactly — see `HighlightLayer` for the math used to keep the
//! widget-local frame stable under live layout.
//!
//! **6 dp is the paint, not the target.** A finger cannot land on a 6 dp strip,
//! and growing the strip would put a fat grey band across a debug panel at
//! every density. Two things give it reach, and neither is an outset of its own:
//!
//! * at **Touch** density the shell wraps it in a `TouchTarget`, so the *slot*
//!   is target-sized with the 6 dp paint centred in it (`shell.rs`);
//! * a near miss anywhere else is served by the framework's own miss-only slop
//!   pass, which tops a small control up as far as the density allows.
//!
//! A `Widget::hit_outset` was tried here and **removed as provably inert**: an
//! outset is only ever offered the points its own ancestors' bounds contain, so
//! a wrapper that hugs a 6 dp strip claims nothing, and inside the Touch slot
//! the slop pass reaches the strip on its own. Deleting the outset left every
//! test green, which is the only evidence that settles it.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::widget::{CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, CornerRadius, InputTokens};

use crate::state::{InspectorState, MAX_PANEL_HEIGHT, MIN_PANEL_HEIGHT};

/// Visual height of the resize strip in logical pixels.
pub(crate) const HANDLE_HEIGHT: f32 = 6.0;

/// The height the strip's *slot* occupies at `tokens`' density.
///
/// The strip paints [`HANDLE_HEIGHT`] at every density; at Touch the
/// `TouchTarget` wrapper around it reserves the density's target size and
/// centres the paint in it. The shell reserves this, not the paint, or the panel
/// is squeezed by the difference.
pub(crate) fn slot_height(tokens: &InputTokens) -> f32 {
    if tokens.density == teksilo_tokens::TargetDensity::Touch {
        HANDLE_HEIGHT.max(tokens.target_size)
    } else {
        HANDLE_HEIGHT
    }
}

/// How much the panel height moves per assistive-technology increment.
///
/// The handle advertises `Increment` / `Decrement` because it is a real
/// manipulator and an AT user has no drag; the step is the same order as a
/// keyboard arrow on a splitter, which is the control this is.
const AT_STEP: f32 = 16.0;

pub(crate) struct ResizeHandle {
    state: InspectorState,
}

impl ResizeHandle {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for ResizeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResizeHandle").finish()
    }
}

impl Widget for ResizeHandle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(false)
            .cursor(CursorIcon::RowResize)
            .on_pointer_event(move |event, ctx| match event {
                WidgetEvent::PointerDown {
                    position,
                    button: PointerButton::Primary,
                    ..
                } => {
                    // `position` is window-local. Snapshot both the
                    // anchor window-y and the panel's height at the
                    // press moment — every PointerMove computes a
                    // *total* delta against these, never deltas
                    // against the live (already-updated) height.
                    state
                        .panel_drag_anchor
                        .set(Some((position.y, state.panel_height.get())));
                    ctx.capture_pointer();
                    EventResponse::Handled
                }
                WidgetEvent::PointerMove { position } => {
                    // The anchor says "I started a resize"; `owns_pointer` says
                    // "and I still own the press" — capture is an arbitration
                    // act, so a handle that lost it stops driving.
                    if let Some((anchor_y, start_h)) =
                        state.panel_drag_anchor.get().filter(|_| ctx.owns_pointer())
                    {
                        // Cursor moved UP from anchor → grow the
                        // panel by that amount; cursor moved DOWN →
                        // shrink. Total height is always derived from
                        // `start_h`, so the handle's top edge tracks
                        // the cursor 1:1.
                        let new_height = (start_h + (anchor_y - position.y))
                            .clamp(MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT);
                        if (new_height - state.panel_height.get()).abs() > f32::EPSILON {
                            state.panel_height.set(new_height);
                        }
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
                WidgetEvent::PointerUp { .. } => {
                    if state.panel_drag_anchor.get().is_some() {
                        state.panel_drag_anchor.set(None);
                        ctx.release_pointer();
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
                _ => EventResponse::Ignored,
            });

        // The two advertised actions, honoured. An assistive-technology user
        // has no drag, so this is the only way they can resize the panel.
        let at_state = self.state.clone();
        let handlers = handlers.on_access_action(move |action, _ctx| {
            use teksilo_core::accesskit::Action;
            let delta = match action {
                Action::Increment => AT_STEP,
                Action::Decrement => -AT_STEP,
                _ => return EventResponse::Ignored,
            };
            let next =
                (at_state.panel_height.get() + delta).clamp(MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT);
            if (next - at_state.panel_height.get()).abs() > f32::EPSILON {
                at_state.panel_height.set(next);
            }
            EventResponse::Handled
        });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // The strip is 6 dp tall, full stop — not "6 dp unless something proposes
        // more". It used to fill the proposed height, which was the same thing
        // while a `FixedSize` pinned the row to 6 dp, and stopped being the same
        // thing the moment the `TouchTarget` slot around it grew: the strip
        // filled the 44 dp slot instead of being centred in it, and the outset
        // that is supposed to claim that slot had nothing left to claim.
        teksilo_canvas::Size::new(proposal.width.unwrap_or(0.0), HANDLE_HEIGHT).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Faint divider line so the user can see (and target) the
        // handle. Uses the theme's border color for consistency with
        // the panel below.
        let color = BorderRole::Default.resolve(&ctx.theme.colors);
        let stripe = Rect::new(
            bounds.x,
            bounds.y + (bounds.height * 0.5 - 0.5),
            bounds.width,
            1.0,
        );
        canvas.fill_rounded_rect(stripe, CornerRadius::ZERO, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A real manipulator, so it says what it is and what it can do. The two
        // actions are honoured below — an advertised action a widget does not
        // execute reads to AT as an operable control that is not one.
        builder.set_role(teksilo_core::accesskit::Role::Splitter);
        builder.set_name("Inspector panel height");
        builder.set_orientation(teksilo_core::accesskit::Orientation::Horizontal);
        builder.set_numeric_value(self.state.panel_height.get() as f64);
        builder.set_min_numeric_value(MIN_PANEL_HEIGHT as f64);
        builder.set_max_numeric_value(MAX_PANEL_HEIGHT as f64);
        builder.set_numeric_value_step(AT_STEP as f64);
        builder.add_action(teksilo_core::accesskit::Action::Increment);
        builder.add_action(teksilo_core::accesskit::Action::Decrement);
    }
}
