//! `Rotate` — wraps a child and applies a 2D rotation to its entire
//! subtree, driven by an external `Prop<f32>` of radians. Layout-
//! stable: the wrapper reports the child's natural size at all
//! angles; only the visual content rotates within the slot.
//!
//! ```ignore
//! let angle = ctx.animated_signal(0.0);
//! ctx.add(Rotate::new(angle.clone()).child(chevron));
//! // Animate to 90° on expand:
//! angle.animate_to(std::f32::consts::FRAC_PI_2, Duration::from_millis(150), Easing::EaseOut);
//! ```
//!
//! No internal animation — the caller owns the angle signal and pairs
//! it with `Signal::animate_to` (or `ctx.animate()`) for animated
//! rotations. This keeps the widget composable: bind it to interaction
//! state for hover-on rotation, to an animated signal for spinning
//! loaders, to a constant for static decorative rotation.
//!
//! Use cases: animated chevrons (the disclosure-state pattern, today
//! faked by visibility-toggling two static chevron icons), spinning
//! loaders not covered by [`Spinner`](crate::Spinner), "shake your
//! head no" rotation feedback, dial controls.
//!
//! ## Reduced motion
//!
//! Rotate doesn't introduce motion — it just applies whatever the
//! caller's angle signal currently holds. Reduced-motion handling
//! belongs at the *caller's* `animate_to` site (use `to_or_snap` or
//! gate the animation behind `prefers_reduced_motion`).

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal, Transform2D};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use super::scale::ScaleOrigin;

/// Rotation matrix `T(pivot) * R(theta) * T(-pivot)`.
fn pivoted_rotation(pivot: Point, theta: f32) -> Transform2D {
    let (s, c) = theta.sin_cos();
    Transform2D {
        m: [
            c,
            s,
            -s,
            c,
            pivot.x * (1.0 - c) + pivot.y * s,
            pivot.y * (1.0 - c) - pivot.x * s,
        ],
    }
}

/// Wraps a child and rotates its subtree by an externally-driven
/// `Prop<f32>` (radians).
pub struct Rotate {
    angle: Prop<f32>,
    origin: ScaleOrigin,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// The transform matrix the render walker reads. Recomputed from
    /// (angle, bounds) in `place_children`, plus by an effect on
    /// `angle` when bounds are known but angle changes between
    /// layouts.
    transform_signal: Option<Signal<Transform2D>>,
    natural_size: Cell<Size>,
    last_bounds: Rc<Cell<Rect>>,
    last_is_rtl: Rc<Cell<bool>>,
}

impl Rotate {
    /// Build a rotate wrapper bound to `angle` (radians). Default
    /// pivot: `Center`.
    pub fn new(angle: impl Into<Prop<f32>>) -> Self {
        Self {
            angle: angle.into(),
            origin: ScaleOrigin::Center,
            pending_child: None,
            child_id: None,
            transform_signal: None,
            natural_size: Cell::new(Size::ZERO),
            last_bounds: Rc::new(Cell::new(Rect::ZERO)),
            last_is_rtl: Rc::new(Cell::new(false)),
        }
    }

    /// Pivot point for the rotation. Default `Center`.
    pub fn origin(mut self, origin: ScaleOrigin) -> Self {
        self.origin = origin;
        self
    }

    /// Inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Pre-registered child by `WidgetId`.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }
}

impl std::fmt::Debug for Rotate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rotate")
            .field("origin", &self.origin)
            .finish()
    }
}

impl Widget for Rotate {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let Some(child_id) = self.child_id else {
            return vec![];
        };

        let transform_signal = ctx.signal(Transform2D::IDENTITY);
        let id = ctx.self_id();
        ctx.set_transform(id, transform_signal.clone());

        // Recompute on angle changes — reads bounds captured by
        // place_children, writes to transform_signal which marks self
        // for repaint via the RepaintOnly binding in set_transform.
        if let Prop::Bound(angle_signal) = &self.angle {
            // Register the user-provided signal with the tree's
            // animation scheduler. Without this, an animation-capable
            // `Signal::new_animated(0.0)` (typical pattern when the
            // signal is created outside any build context) would have
            // its `animate_to` requests silently dropped — the
            // scheduler only ticks registered signals.
            // `register_animated_signal` is a no-op for signals that
            // aren't animation-capable, so this is always safe.
            ctx.register_animated_signal(angle_signal);

            let last_bounds = self.last_bounds.clone();
            let last_is_rtl = self.last_is_rtl.clone();
            let origin = self.origin;
            let transform_for_observer = transform_signal.clone();
            ctx.effect(angle_signal, move |&theta| {
                let bounds = last_bounds.get();
                let pivot = origin.pivot_world(bounds, last_is_rtl.get());
                transform_for_observer.set(pivoted_rotation(pivot, theta));
            });
        }

        self.transform_signal = Some(transform_signal);

        vec![child_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let Some(child_id) = self.child_id else {
            return proposal.resolve(0.0, 0.0).into();
        };
        let natural = ctx.child_size(child_id, proposal).unwrap_or(Size::ZERO);
        self.natural_size.set(natural);
        natural.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Capture bounds + RTL for the angle observer; publish the
        // current matrix immediately so the first frame paints with
        // the right rotation even before any signal change.
        self.last_bounds.set(bounds);
        self.last_is_rtl.set(ctx.is_rtl());
        if let Some(t_sig) = &self.transform_signal {
            let theta = self.angle.get();
            let pivot = self.origin.pivot_world(bounds, ctx.is_rtl());
            t_sig.set(pivoted_rotation(pivot, theta));
        }

        let natural = self.natural_size.get();
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = natural;
        }
    }

    fn clips_children(&self) -> bool {
        // Intentionally false: rotated content visibly extends past
        // the slot bounds at every non-90°-multiple angle (a 28×28
        // square at 45° has corners ~6 px past the original bounds).
        // Clipping cuts those corners off — the rotation looks like a
        // flickering hexagon mid-tween. Users who need bounded layout
        // can wrap the Rotate in a clipping container (`MaxSize`,
        // `ScrollArea`, …) sized to fit the rotated bounding box.
        false
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Visual-modulation wrapper. Child owns its own a11y.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::TextWidget;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[test]
    fn zero_angle_emits_identity_skip() {
        let angle = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Rotate::new(angle).child(TextWidget::new(lit!("x"))));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let frame = tree.render();
        let push_count = frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, bastyde_canvas::DrawCommand::PushTransform(_)))
            .count();
        assert_eq!(push_count, 0, "zero rotation must skip the transform scope");
    }

    #[test]
    fn nonzero_angle_emits_rotation_matrix() {
        let angle = Signal::new(std::f32::consts::FRAC_PI_2); // 90°
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Rotate::new(angle).child(TextWidget::new(lit!("x"))));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let frame = tree.render();
        let pushes: Vec<&Transform2D> = frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                bastyde_canvas::DrawCommand::PushTransform(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(pushes.len(), 1);
        // 90° rotation: cos=0, sin=1 → matrix linear part = [0, 1, -1, 0].
        assert!(pushes[0].m[0].abs() < 1e-3, "a (cos) should be 0");
        assert!((pushes[0].m[1] - 1.0).abs() < 1e-3, "b (sin) should be 1");
        assert!(
            (pushes[0].m[2] - (-1.0)).abs() < 1e-3,
            "c (-sin) should be -1"
        );
        assert!(pushes[0].m[3].abs() < 1e-3, "d (cos) should be 0");
    }

    #[test]
    fn layout_size_unchanged_by_rotation() {
        // Set the angle to a value via plain signal mutation — no
        // animation infrastructure needed; the assertion here is
        // about layout size, not animation timing.
        let angle = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Rotate::new(angle.clone()).child(TextWidget::new(lit!("hello"))));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let initial = tree.bounds(id).size();

        // Spin 45° instantly. Layout-stable wrapper must not change size.
        angle.set(std::f32::consts::FRAC_PI_4);
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let after = tree.bounds(id).size();
        assert_eq!(initial, after, "rotation must not change layout size");
    }

    #[test]
    fn animate_to_actually_advances_angle_value() {
        // End-to-end: user creates `Signal::new_animated`, builds a
        // Rotate around it, calls `animate_to`. After ticking the
        // scheduler, the signal's *value* must actually have changed
        // (the matrix being pushed must reflect the new angle).
        use std::time::Duration;
        let angle = Signal::new_animated(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Rotate::new(angle.clone()).child(TextWidget::new(lit!("x"))));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        // Pre-condition: angle is 0, no push emitted (identity skip).
        let frame0 = tree.render();
        assert_eq!(
            frame0
                .draw_order
                .iter()
                .filter(|c| matches!(c, bastyde_canvas::DrawCommand::PushTransform(_)))
                .count(),
            0,
            "initial angle 0 → no transform scope"
        );

        angle.animate_to(
            std::f32::consts::FRAC_PI_2,
            Duration::from_millis(100),
            bastyde_tokens::Easing::Linear,
        );
        // Drain pending request → scheduler.
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        // Tick most of the duration.
        tree.tick_animations(Duration::from_millis(80));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        // Angle must have advanced past zero AND past the identity
        // skip threshold; the wrapper must now emit a real push.
        assert!(
            angle.get() > 0.1,
            "angle value must have advanced from 0 (got {})",
            angle.get()
        );
        let frame1 = tree.render();
        let pushes: Vec<&Transform2D> = frame1
            .draw_order
            .iter()
            .filter_map(|c| match c {
                bastyde_canvas::DrawCommand::PushTransform(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(
            pushes.len(),
            1,
            "advanced angle must emit a transform scope"
        );
    }

    #[test]
    fn rotation_pivot_in_zstack_with_center_dot() {
        // Exact mirror of the kit's diagnostic structure:
        // FixedSize(80) > ZStack > [Rotate(RectWidget), Center(FixedSize(6x6)(dot))].
        // The cube fills the ZStack slot; the dot sits at slot center.
        // After rotation, the cube must rotate around the dot.
        use crate::primitives::{Center, FixedSize, RectWidget, ZStack};
        use bastyde_tokens::Color;
        use std::time::Duration;
        let angle = Signal::new_animated(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            FixedSize::new().bind_width(80.0).bind_height(80.0).child(
                ZStack::new()
                    .child(
                        Rotate::new(angle.clone())
                            .child(RectWidget::new().background(Color::from_rgb(0.30, 0.55, 0.85))),
                    )
                    .child(
                        Center::new().child(
                            FixedSize::new()
                                .bind_width(6.0)
                                .bind_height(6.0)
                                .child(RectWidget::new().background(Color::BLACK)),
                        ),
                    ),
            ),
        );
        tree.layout(SizeProposal::exact(120.0, 120.0));

        // Start the rotation, drain pending into the scheduler, tick.
        angle.animate_to(
            std::f32::consts::FRAC_PI_2,
            Duration::from_millis(100),
            bastyde_tokens::Easing::Linear,
        );
        tree.layout(SizeProposal::exact(120.0, 120.0));
        tree.tick_animations(Duration::from_millis(50));
        tree.layout(SizeProposal::exact(120.0, 120.0));

        let frame = tree.render();

        // Pivot recovered from the matrix should match the dot's
        // painted center (since the cube is supposed to rotate around
        // the dot).
        let push = frame
            .draw_order
            .iter()
            .find_map(|c| match c {
                bastyde_canvas::DrawCommand::PushTransform(t) => Some(*t),
                _ => None,
            })
            .expect("rotation must emit a transform scope mid-tween");
        let c = push.m[0];
        let s = push.m[1];
        let tx = push.m[4];
        let ty = push.m[5];
        let det = (1.0 - c) * (1.0 - c) + s * s;
        let recovered_px = ((1.0 - c) * tx - s * ty) / det;
        let recovered_py = (s * tx + (1.0 - c) * ty) / det;

        // Find the dot — black 6x6 rect.
        let black_array = Color::BLACK.to_array();
        let dot = frame
            .shapes
            .iter()
            .find(|sh| sh.color == black_array)
            .expect("dot must paint");
        let dot_center_x = dot.screen[0] + dot.screen[2] * 0.5;
        let dot_center_y = dot.screen[1] + dot.screen[3] * 0.5;

        // Find the cube — the blue rect.
        let blue_array = Color::from_rgb(0.30, 0.55, 0.85).to_array();
        let cube = frame
            .shapes
            .iter()
            .find(|sh| sh.color == blue_array)
            .expect("cube must paint");
        let cube_center_x = cube.screen[0] + cube.screen[2] * 0.5;
        let cube_center_y = cube.screen[1] + cube.screen[3] * 0.5;

        // All three centers (dot, cube, recovered pivot) must coincide.
        let dot_pivot_err =
            (recovered_px - dot_center_x).abs() + (recovered_py - dot_center_y).abs();
        let cube_pivot_err =
            (recovered_px - cube_center_x).abs() + (recovered_py - cube_center_y).abs();
        assert!(
            dot_pivot_err < 1.0,
            "pivot ({}, {}) must match DOT center ({}, {}); err = {}",
            recovered_px,
            recovered_py,
            dot_center_x,
            dot_center_y,
            dot_pivot_err,
        );
        assert!(
            cube_pivot_err < 1.0,
            "pivot ({}, {}) must match CUBE center ({}, {}); err = {}",
            recovered_px,
            recovered_py,
            cube_center_x,
            cube_center_y,
            cube_pivot_err,
        );
    }

    #[test]
    fn rotation_pivot_inside_scroll_area_matches_visual_center() {
        // Closer to the kit's actual structure: the cube is deep
        // inside a ScrollArea > Padding > VStack > ... > HStack chain.
        // ScrollArea positions content children with a `bounds.origin -
        // scroll_offset` shift; a wrong pivot would surface here.
        use crate::primitives::{FixedSize, HStack, Padding, RectWidget, VStack};
        use crate::scroll_area::ScrollArea;
        use bastyde_tokens::Color;
        use std::time::Duration;
        let angle = Signal::new_animated(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            ScrollArea::new().child(
                Padding::uniform(24.0).child(
                    VStack::new()
                        .spacing(20.0)
                        .child(TextWidget::new(lit!("filler 1")))
                        .child(TextWidget::new(lit!("filler 2")))
                        .child(TextWidget::new(lit!("filler 3")))
                        .child(
                            HStack::new()
                                .spacing(12.0)
                                .child(
                                    Rotate::new(angle.clone()).child(
                                        FixedSize::new()
                                            .bind_width(28.0)
                                            .bind_height(28.0)
                                            .child(RectWidget::new().background(Color::RED)),
                                    ),
                                )
                                .child(TextWidget::new(lit!("Rotate 90°"))),
                        ),
                ),
            ),
        );
        tree.layout(SizeProposal::exact(560.0, 720.0));

        angle.animate_to(
            std::f32::consts::FRAC_PI_2,
            Duration::from_millis(100),
            bastyde_tokens::Easing::Linear,
        );
        tree.layout(SizeProposal::exact(560.0, 720.0));
        tree.tick_animations(Duration::from_millis(50));
        tree.layout(SizeProposal::exact(560.0, 720.0));

        let frame = tree.render();
        let push = frame
            .draw_order
            .iter()
            .find_map(|c| match c {
                bastyde_canvas::DrawCommand::PushTransform(t) => Some(*t),
                _ => None,
            })
            .expect("rotation must emit a transform scope mid-tween");
        let c = push.m[0];
        let s = push.m[1];
        let tx = push.m[4];
        let ty = push.m[5];
        let det = (1.0 - c) * (1.0 - c) + s * s;
        let recovered_px = ((1.0 - c) * tx - s * ty) / det;
        let recovered_py = (s * tx + (1.0 - c) * ty) / det;

        let cube_shape = frame
            .shapes
            .iter()
            .find(|s| s.color == Color::RED.to_array())
            .expect("cube must paint a shape");
        let visual_center_x = cube_shape.screen[0] + cube_shape.screen[2] * 0.5;
        let visual_center_y = cube_shape.screen[1] + cube_shape.screen[3] * 0.5;
        let err_x = (recovered_px - visual_center_x).abs();
        let err_y = (recovered_py - visual_center_y).abs();
        assert!(
            err_x < 1.0 && err_y < 1.0,
            "ScrollArea-nested pivot ({}, {}) must match visual center ({}, {}); err = ({}, {})",
            recovered_px,
            recovered_py,
            visual_center_x,
            visual_center_y,
            err_x,
            err_y,
        );
    }

    #[test]
    fn rotation_pivot_in_kit_like_structure_after_animate_to() {
        // Repros the kit example: cube is deep inside a VStack →
        // ... → HStack → Rotate(FixedSize(RectWidget)) chain. After
        // animate_to ticks the angle past zero, the matrix being
        // pushed must use a pivot near the cube's actual world
        // position, not (0, 0) or stale bounds.
        use crate::primitives::{FixedSize, HStack, Padding, RectWidget, VStack};
        use bastyde_tokens::Color;
        use std::time::Duration;
        let angle = Signal::new_animated(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        // Mirror animations-kit: lots of vertical content above the
        // Rotate row, so the cube lives at a non-trivial y offset.
        tree.add(
            Padding::uniform(24.0).child(
                VStack::new()
                    .spacing(20.0)
                    .child(TextWidget::new(lit!("filler")))
                    .child(TextWidget::new(lit!("filler")))
                    .child(TextWidget::new(lit!("filler")))
                    .child(
                        HStack::new()
                            .spacing(12.0)
                            .child(
                                Rotate::new(angle.clone()).child(
                                    FixedSize::new()
                                        .bind_width(28.0)
                                        .bind_height(28.0)
                                        .child(RectWidget::new().background(Color::RED)),
                                ),
                            )
                            .child(TextWidget::new(lit!("Rotate 90°"))),
                    ),
            ),
        );
        tree.layout(SizeProposal {
            width: Some(560.0),
            height: None,
        });

        // Drive the angle past zero.
        angle.animate_to(
            std::f32::consts::FRAC_PI_2,
            Duration::from_millis(100),
            bastyde_tokens::Easing::Linear,
        );
        tree.layout(SizeProposal {
            width: Some(560.0),
            height: None,
        });
        tree.tick_animations(Duration::from_millis(50));
        tree.layout(SizeProposal {
            width: Some(560.0),
            height: None,
        });

        let frame = tree.render();
        let push = frame
            .draw_order
            .iter()
            .find_map(|c| match c {
                bastyde_canvas::DrawCommand::PushTransform(t) => Some(*t),
                _ => None,
            })
            .expect("rotation must emit a transform scope mid-tween");
        // Recover pivot from the matrix as before. Mid-tween (linear,
        // ~50ms of 100) the angle is roughly π/4. cos≈sin≈0.707.
        // tx = px*(1-c) + py*s; ty = py*(1-c) - px*s.
        // Solve: px*(1-c) + py*s = tx; py*(1-c) - px*s = ty.
        // [(1-c) s ; -s (1-c)] * [px;py] = [tx;ty]
        // Determinant = (1-c)² + s² = 2(1-c) for unit rotation.
        let c = push.m[0];
        let s = push.m[1];
        let tx = push.m[4];
        let ty = push.m[5];
        let det = (1.0 - c) * (1.0 - c) + s * s;
        assert!(det > 1e-3, "non-trivial rotation expected");
        let recovered_px = ((1.0 - c) * tx - s * ty) / det;
        let recovered_py = (s * tx + (1.0 - c) * ty) / det;

        // Compare to the cube's actual paint position.
        let cube_shape = frame
            .shapes
            .iter()
            .find(|s| s.color == Color::RED.to_array())
            .expect("cube must paint a shape");
        let visual_center_x = cube_shape.screen[0] + cube_shape.screen[2] * 0.5;
        let visual_center_y = cube_shape.screen[1] + cube_shape.screen[3] * 0.5;
        let err_x = (recovered_px - visual_center_x).abs();
        let err_y = (recovered_py - visual_center_y).abs();
        assert!(
            err_x < 1.0 && err_y < 1.0,
            "pivot ({}, {}) must match cube's visual center ({}, {}); err = ({}, {})",
            recovered_px,
            recovered_py,
            visual_center_x,
            visual_center_y,
            err_x,
            err_y,
        );
    }

    #[test]
    fn rotation_pivot_lands_at_visual_center_when_inside_hstack() {
        // Regression: Rotate's pivot is the wrapper's slot center,
        // and the cube renders at its slot's origin (top-left). When
        // an HStack assigns Rotate a slot whose VERTICAL extent is
        // taller than the cube's natural height (typical: HStack
        // height = max child height, taller siblings stretch the
        // row), Rotate's slot would be taller than 28 — and pivot
        // would drift below the visual center. Confirm the pivot
        // matches the visual rect's actual center.
        use crate::primitives::{FixedSize, HStack, RectWidget};
        use bastyde_tokens::Color;
        let angle = Signal::new(std::f32::consts::FRAC_PI_2); // 90°
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        // Build an HStack with a 28x28 cube AND a much taller sibling
        // so the row's cross-axis stretches past 28.
        tree.add(
            HStack::new()
                .spacing(0.0)
                .child(
                    Rotate::new(angle).child(
                        FixedSize::new()
                            .bind_width(28.0)
                            .bind_height(28.0)
                            .child(RectWidget::new().background(Color::RED)),
                    ),
                )
                .child(
                    FixedSize::new()
                        .bind_width(40.0)
                        .bind_height(80.0)
                        .child(RectWidget::new().background(Color::BLUE)),
                ),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let frame = tree.render();
        let pushes: Vec<&Transform2D> = frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                bastyde_canvas::DrawCommand::PushTransform(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(pushes.len(), 1, "Rotate must emit one push");

        // The matrix is `T(pivot) * R * T(-pivot)` — recover pivot
        // from the translation column. For 90°: cos=0, sin=1 →
        // tx = pivot.x*1 + pivot.y*1 = pivot.x + pivot.y
        // ty = pivot.y*1 - pivot.x*1 = pivot.y - pivot.x
        // → pivot.x = (tx - ty) / 2, pivot.y = (tx + ty) / 2
        let tx = pushes[0].m[4];
        let ty = pushes[0].m[5];
        let recovered_pivot_x = (tx - ty) * 0.5;
        let recovered_pivot_y = (tx + ty) * 0.5;

        // The cube is the FIRST child of HStack. Its slot's top-left
        // is at HStack's origin (0, alignment_offset). Its visual
        // (the FixedSize → RectWidget chain at 28x28) sits at the
        // *cube's slot top-left*. Visual center should be at
        // (slot.x + 14, slot.y + 14). The pivot MUST match.
        // Find the FillWidget's bounds via the rendered shapes' first
        // RED entry — that's the cube's actual paint position.
        let red_array = Color::RED.to_array();
        let cube_shape = frame
            .shapes
            .iter()
            .find(|s| s.color == red_array)
            .expect("cube must paint a shape");
        let visual_center_x = cube_shape.screen[0] + cube_shape.screen[2] * 0.5;
        let visual_center_y = cube_shape.screen[1] + cube_shape.screen[3] * 0.5;

        let pivot_err_x = (recovered_pivot_x - visual_center_x).abs();
        let pivot_err_y = (recovered_pivot_y - visual_center_y).abs();
        assert!(
            pivot_err_x < 0.5 && pivot_err_y < 0.5,
            "rotation pivot ({}, {}) must match visual center ({}, {}); err = ({}, {})",
            recovered_pivot_x,
            recovered_pivot_y,
            visual_center_x,
            visual_center_y,
            pivot_err_x,
            pivot_err_y,
        );
    }

    #[test]
    fn user_provided_animated_signal_is_registered_with_scheduler() {
        // Regression: Rotate accepts a user-provided Signal<f32> via
        // its Prop<f32> argument. If the user creates the signal with
        // `Signal::new_animated(0.0)` (the natural pattern when the
        // signal is constructed outside any build context — e.g. in
        // the example's `build_kit` function), `animate_to` queues a
        // pending request that the scheduler can only pick up if the
        // signal is registered with the tree. Rotate's build() must
        // auto-register so user `animate_to` calls actually drive the
        // angle.
        use std::time::Duration;
        let angle = Signal::new_animated(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Rotate::new(angle.clone()).child(TextWidget::new(lit!("x"))));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        angle.animate_to(
            std::f32::consts::FRAC_PI_2,
            Duration::from_millis(100),
            bastyde_tokens::Easing::Linear,
        );
        // Drain the pending request onto the scheduler.
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        assert!(
            tree.has_active_animations(),
            "user's animate_to on a Signal::new_animated must reach the scheduler"
        );
    }

    #[test]
    fn angle_signal_drives_emitted_matrix() {
        // Bumping the angle signal must update the next frame's
        // PushTransform value — no rebuild required.
        let angle = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(Rotate::new(angle.clone()).child(TextWidget::new(lit!("x"))));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        // Initial: zero angle, no push.
        let frame0 = tree.render();
        assert_eq!(
            frame0
                .draw_order
                .iter()
                .filter(|c| matches!(c, bastyde_canvas::DrawCommand::PushTransform(_)))
                .count(),
            0
        );

        angle.set(std::f32::consts::FRAC_PI_2);
        let frame1 = tree.render();
        let pushes: Vec<&Transform2D> = frame1
            .draw_order
            .iter()
            .filter_map(|c| match c {
                bastyde_canvas::DrawCommand::PushTransform(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(pushes.len(), 1, "rotated subtree should now emit one push");
    }
}
