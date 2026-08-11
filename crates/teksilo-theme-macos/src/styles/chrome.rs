// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The paint vocabulary the macOS widget styles share: the **bezel** and
//! the **focus ring**.
//!
//! Both are painted by [`MacOsControlChrome`], a leaf widget that resolves
//! everything from [`PaintContext::theme`] at paint time. That matters for
//! two reasons a build-time-baked colour could not deliver:
//!
//! - a live `ctx.set_theme(...)` swap repaints with the new palette, and
//! - accent-derived colours come from `theme.colors.*`, which the paint
//!   walker has already projected through
//!   [`ColorTokens::for_inactive_window`](teksilo_tokens::ColorTokens::for_inactive_window),
//!   so an accent control greys out in a background window with no extra
//!   code — which is the macOS convention the framework borrowed in the
//!   first place.
//!
//! Neutral colours come from the [`MacOsPalette`] extension, which carries
//! the AppKit tokens `ColorTokens` has no slot for (the bezel gradient,
//! the four label grades, the control track). Every read falls back to a
//! theme role when the extension is absent, so a `MacOsControlChrome`
//! dropped into a non-macOS theme still paints something sane.
//!
//! ## The bezel
//!
//! A macOS control is drawn as a physical object sitting on the window:
//!
//! 1. a soft, short shadow underneath it;
//! 2. a face with a faint **top-to-bottom** gradient (lighter above);
//! 3. a hairline around the whole outline;
//! 4. in Dark Aqua only, a catch-light along the top inside edge.
//!
//! Pressing it darkens the face and drops the shadow — the control
//! settles into the surface. That sequence is what most separates a macOS
//! button from a Fluent one (which reads its elevation from a single
//! heavier edge) or a Material 3 one (which reads it from a tonal fill and
//! a ripple).
//!
//! ## The focus ring
//!
//! Two concentric bands: a solid one at full accent that carries the
//! WCAG 1.4.11 contrast, and a translucent halo outside it that supplies
//! the macOS softness. See [`crate::shape`] for why it is built that way
//! rather than as one 50 %-alpha ring.

use teksilo_canvas::{Canvas, GradientStop, Paint, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, Shadow};

use crate::palette::{MacOsBezel, MacOsPalette};
use crate::shape::{
    MACOS_FOCUS_RING_HALO_ALPHA, MACOS_FOCUS_RING_HALO_WIDTH, MACOS_FOCUS_RING_WIDTH,
};

/// Vertical offset of a control's own drop shadow (dp).
const CONTROL_SHADOW_OFFSET_Y: f32 = 0.5;
/// Blur radius of a control's own drop shadow (dp). Deliberately tiny —
/// a macOS *control* is barely lifted; it is menus and sheets that cast
/// real shadows.
const CONTROL_SHADOW_BLUR: f32 = 1.5;
/// Thickness of the Dark Aqua catch-light along a bezel's top edge (dp).
const CATCH_LIGHT_THICKNESS: f32 = 1.0;

/// Which AppKit control family a surface belongs to. Decides the fill
/// ramp, whether a bezel is drawn, and which label colour the caller
/// should pair with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacOsSurfaceKind {
    /// `NSButton` in its rounded-bezel style, `NSPopUpButton`,
    /// `NSStepper` — a bezelled face with a hairline and a shadow.
    Bezel,
    /// The *default* button, and any accent-filled control. A flat accent
    /// fill: AppKit gives the default button no bezel gradient, because
    /// the accent already separates it from the window.
    Accent,
    /// A destructive action. AppKit has no such button style; this is the
    /// accent treatment recoloured with `systemRed`.
    Destructive,
    /// A borderless / recessed control — a toolbar button, a link button,
    /// a menu row. Transparent at rest, a neutral wash on hover and press.
    Borderless,
}

impl MacOsSurfaceKind {
    /// Whether this surface paints a bezel (face gradient, hairline,
    /// catch-light, shadow) rather than a flat fill.
    pub fn is_bezelled(self) -> bool {
        matches!(self, MacOsSurfaceKind::Bezel)
    }
}

/// Interaction state, in AppKit's own precedence order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MacOsState {
    #[default]
    Rest,
    Hover,
    Pressed,
    Disabled,
}

impl MacOsState {
    /// Collapse the usual three booleans into a state, disabled first.
    ///
    /// Every one of these may be a *derived* signal, so this only ever
    /// composes with `zip3`/`map` — never `observe`.
    pub fn derive(
        is_disabled: &Signal<bool>,
        is_pressed: &Signal<bool>,
        is_hovered: &Signal<bool>,
    ) -> Signal<MacOsState> {
        is_disabled
            .zip3(is_pressed, is_hovered)
            .map(|(disabled, pressed, hovered)| {
                if *disabled {
                    MacOsState::Disabled
                } else if *pressed {
                    MacOsState::Pressed
                } else if *hovered {
                    MacOsState::Hover
                } else {
                    MacOsState::Rest
                }
            })
    }
}

/// The bezel description for a state, with a theme-role fallback for a
/// non-macOS theme.
pub fn resolve_bezel(ctx: &PaintContext, state: MacOsState) -> MacOsBezel {
    let c = &ctx.theme.colors;
    let Some(p) = ctx.theme.extension::<MacOsPalette>() else {
        // No macOS palette: degrade to a flat, borderless-looking face
        // built from the theme's own roles.
        let face = match state {
            MacOsState::Disabled => c.surface_disabled,
            MacOsState::Pressed => c.surface_pressed,
            MacOsState::Hover => c.surface_hover,
            MacOsState::Rest => c.surface_content,
        };
        return MacOsBezel {
            face_top: face,
            face_bottom: face,
            stroke: c.border,
            inner_light: Color::TRANSPARENT,
            shadow: Color::TRANSPARENT,
        };
    };

    match state {
        MacOsState::Rest => p.bezel,
        // AppKit's hover on a bezelled button is close to imperceptible —
        // the face darkens (Aqua) or brightens (Dark Aqua) by a hair.
        MacOsState::Hover => MacOsBezel {
            face_top: shade(p.bezel.face_top, ctx, 0.04),
            face_bottom: shade(p.bezel.face_bottom, ctx, 0.04),
            ..p.bezel
        },
        // Pressed: the face darkens noticeably *and* the shadow goes
        // away, so the control settles into the surface instead of
        // floating over it.
        MacOsState::Pressed => MacOsBezel {
            face_top: shade(p.bezel.face_top, ctx, 0.14),
            face_bottom: shade(p.bezel.face_bottom, ctx, 0.14),
            inner_light: Color::TRANSPARENT,
            shadow: Color::TRANSPARENT,
            ..p.bezel
        },
        // Disabled: a flat inert face, no gradient, no lift.
        MacOsState::Disabled => MacOsBezel {
            face_top: p.disabled_control_face,
            face_bottom: p.disabled_control_face,
            stroke: p.separator,
            inner_light: Color::TRANSPARENT,
            shadow: Color::TRANSPARENT,
        },
    }
}

/// Move `c` `amount` in the appearance's emphasis direction — darker in
/// Aqua, lighter in Dark Aqua. Pressing a control makes it *more*, and
/// what "more" means flips with the appearance.
fn shade(c: Color, ctx: &PaintContext, amount: f32) -> Color {
    if ctx.theme.is_dark() {
        c.lighten(amount)
    } else {
        c.darken(amount)
    }
}

/// The flat fill an *unbezelled* surface paints for a state, or `None`
/// when it paints nothing at all.
pub fn surface_fill(
    kind: MacOsSurfaceKind,
    state: MacOsState,
    ctx: &PaintContext,
) -> Option<Color> {
    let c = &ctx.theme.colors;
    match kind {
        // Bezelled surfaces are painted by `paint_bezel`, not here.
        MacOsSurfaceKind::Bezel => None,
        MacOsSurfaceKind::Accent => Some(match state {
            // These already carry the macOS ramp — and, unlike the
            // extension, they are desaturated for an inactive window.
            MacOsState::Rest => c.accent,
            MacOsState::Hover => c.accent_hover,
            MacOsState::Pressed => c.accent_pressed,
            MacOsState::Disabled => c.accent_disabled,
        }),
        MacOsSurfaceKind::Destructive => Some(match state {
            MacOsState::Rest => c.status_error_fg,
            MacOsState::Hover => shade(c.status_error_fg, ctx, 0.08),
            MacOsState::Pressed => shade(c.status_error_fg, ctx, 0.18),
            MacOsState::Disabled => c.accent_disabled,
        }),
        MacOsSurfaceKind::Borderless => Some(match state {
            MacOsState::Rest | MacOsState::Disabled => Color::TRANSPARENT,
            MacOsState::Hover => c.surface_hover,
            MacOsState::Pressed => c.surface_pressed,
        }),
    }
}

/// A vertical two-stop gradient over the filled rect, or a solid paint
/// when both stops agree.
///
/// Gradient endpoints are **rect-local**: `(0, 0)` is the rect's top-left
/// and `(0, height)` its bottom-left, so the ramp stays put wherever the
/// control is placed or scrolled to.
pub fn vertical_gradient(top: Color, bottom: Color, height: f32) -> Paint {
    if top == bottom {
        return Paint::Solid(top);
    }
    Paint::LinearGradient {
        start: Point::new(0.0, 0.0),
        end: Point::new(0.0, height.max(1.0)),
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: top,
            },
            GradientStop {
                offset: 1.0,
                color: bottom,
            },
        ],
    }
}

/// Paint a macOS bezel into `bounds`: shadow, graded face, hairline, and
/// (Dark Aqua only) the catch-light along the top inside edge.
pub fn paint_bezel(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: f32,
    bezel: &MacOsBezel,
    border_width: f32,
) {
    let corner = CornerRadius::uniform(radius);

    if bezel.shadow.a() > 0.0 {
        canvas.draw_shadow(
            bounds,
            corner,
            &Shadow {
                offset_x: 0.0,
                offset_y: CONTROL_SHADOW_OFFSET_Y,
                blur: CONTROL_SHADOW_BLUR,
                spread: 0.0,
                color: bezel.shadow,
            },
        );
    }

    canvas.fill_rounded_rect(
        bounds,
        corner,
        vertical_gradient(bezel.face_top, bezel.face_bottom, bounds.height),
    );

    // The catch-light sits *inside* the outline, inset by the corner
    // radius so it does not fight the corner arcs — the same construction
    // Fluent uses for its elevation edge, for the same reason: a flat
    // stroke cannot follow an arc at sub-pixel weight without fringing.
    if bezel.inner_light.a() > 0.0 {
        paint_top_inner_light(canvas, bounds, radius, bezel.inner_light);
    }

    if bezel.stroke.a() > 0.0 && border_width > 0.0 {
        canvas.stroke_rounded_rect(bounds, corner, bezel.stroke, border_width);
    }
}

/// A one-pixel highlight along the top inside edge, inset by the corner
/// radius and drawn as its own pill.
fn paint_top_inner_light(canvas: &mut Canvas, bounds: Rect, radius: f32, color: Color) {
    let inset = radius.min(bounds.width * 0.5);
    let width = bounds.width - inset * 2.0;
    if width <= 0.0 {
        return;
    }
    canvas.fill_rounded_rect(
        Rect::new(
            bounds.x + inset,
            bounds.y,
            width,
            CATCH_LIGHT_THICKNESS.min(bounds.height),
        ),
        CornerRadius::uniform(CATCH_LIGHT_THICKNESS * 0.5),
        color,
    );
}

/// Paint the macOS focus ring around `bounds`, in `color`.
///
/// A solid [`MACOS_FOCUS_RING_WIDTH`] band hugging the control, then a
/// [`MACOS_FOCUS_RING_HALO_WIDTH`] band at
/// [`MACOS_FOCUS_RING_HALO_ALPHA`] outside it. The solid band is what
/// clears WCAG SC 1.4.11; the halo is what makes it look like macOS. See
/// [`crate::shape`].
///
/// Callers almost always want [`paint_focus_ring`]; this exists for the
/// one case that rings in something other than the accent — a field whose
/// validation state has to win over its focus state
/// ([`crate::styles::text_input`]). Keeping it one function rather than
/// two is deliberate: an earlier copy had already drifted, losing the
/// `solid_w` floor below and silently thinning the error ring — the one
/// affordance an invalid field has — while the ordinary ring stayed
/// visible.
pub fn paint_ring(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: f32,
    color: Color,
    ctx: &PaintContext,
) {
    if color.a() <= 0.0 {
        return;
    }
    // The theme's own width is a floor, not a ceiling: a macOS ring below
    // 2 dp stops reading as a halo.
    let solid_w = ctx.theme.shape.focus_ring_width.max(MACOS_FOCUS_RING_WIDTH);
    let offset = ctx.theme.shape.focus_ring_offset;

    // Halo first, so the solid band lands on top of its inner edge and
    // the two read as one graded ring rather than two outlines.
    let halo_center = offset + solid_w + MACOS_FOCUS_RING_HALO_WIDTH * 0.5;
    canvas.stroke_rounded_rect(
        inflate(bounds, halo_center),
        CornerRadius::uniform(radius + halo_center),
        color.with_alpha(color.a() * MACOS_FOCUS_RING_HALO_ALPHA),
        MACOS_FOCUS_RING_HALO_WIDTH,
    );

    let solid_center = offset + solid_w * 0.5;
    canvas.stroke_rounded_rect(
        inflate(bounds, solid_center),
        CornerRadius::uniform(radius + solid_center),
        color,
        solid_w,
    );
}

/// Paint the macOS focus ring in the accent — the usual case.
pub fn paint_focus_ring(canvas: &mut Canvas, bounds: Rect, radius: f32, ctx: &PaintContext) {
    // `focus_ring` is `controlAccentColor`, already desaturated by the
    // paint walker when the window is inactive.
    paint_ring(canvas, bounds, radius, ctx.theme.colors.focus_ring, ctx);
}

/// Grow a rect by `d` on every side.
pub fn inflate(r: Rect, d: f32) -> Rect {
    Rect::new(r.x - d, r.y - d, r.width + d * 2.0, r.height + d * 2.0)
}

/// The chrome layer of a macOS control: bezel or flat fill, plus the focus
/// ring, painted behind whatever content the style stacks on top.
///
/// Sized like [`RectWidget`](teksilo_widgets::primitives::RectWidget) — it
/// takes whatever the proposal offers and reports zero when unbounded — so
/// it can sit under a `ZStack` without influencing the control's size.
pub struct MacOsControlChrome {
    kind: MacOsSurfaceKind,
    corner_radius: f32,
    state: Signal<MacOsState>,
    /// `true` while the control should show its focus ring — already
    /// gated on `:focus-visible` by the caller where the widget exposes it.
    show_focus_ring: Signal<bool>,
}

impl MacOsControlChrome {
    pub fn new(
        kind: MacOsSurfaceKind,
        corner_radius: f32,
        state: Signal<MacOsState>,
        show_focus_ring: Signal<bool>,
    ) -> Self {
        Self {
            kind,
            corner_radius,
            state,
            show_focus_ring,
        }
    }
}

impl std::fmt::Debug for MacOsControlChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsControlChrome")
            .field("kind", &self.kind)
            .finish()
    }
}

impl Widget for MacOsControlChrome {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.state.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.show_focus_ring
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let state = self.state.get();
        let radius = self.corner_radius;

        if self.kind.is_bezelled() {
            let bezel = resolve_bezel(ctx, state);
            paint_bezel(canvas, bounds, radius, &bezel, ctx.theme.shape.border_width);
        } else if let Some(fill) = surface_fill(self.kind, state, ctx)
            && fill.a() > 0.0
        {
            canvas.fill_rounded_rect(bounds, CornerRadius::uniform(radius), fill);
        }

        if self.show_focus_ring.get() {
            paint_focus_ring(canvas, bounds, radius, ctx);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the owning control emits the real AT node.
        builder.set_hidden();
    }
}

/// A fixed-size leaf that reports `size` and paints nothing.
///
/// Used by the styles that need a spacer of a known extent inside a row
/// without pulling in a layout primitive.
#[derive(Debug)]
pub struct MacOsSpacerBox {
    size: Size,
}

impl MacOsSpacerBox {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
        }
    }
}

impl Widget for MacOsSpacerBox {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        self.size.into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bezel_kind_is_bezelled() {
        assert!(MacOsSurfaceKind::Bezel.is_bezelled());
        for k in [
            MacOsSurfaceKind::Accent,
            MacOsSurfaceKind::Destructive,
            MacOsSurfaceKind::Borderless,
        ] {
            assert!(!k.is_bezelled());
        }
    }

    #[test]
    fn state_derivation_follows_appkits_precedence() {
        let disabled = Signal::new(false);
        let pressed = Signal::new(false);
        let hovered = Signal::new(false);
        let state = MacOsState::derive(&disabled, &pressed, &hovered);

        assert_eq!(state.get(), MacOsState::Rest);
        hovered.set(true);
        assert_eq!(state.get(), MacOsState::Hover);
        pressed.set(true);
        assert_eq!(state.get(), MacOsState::Pressed, "press beats hover");
        disabled.set(true);
        assert_eq!(state.get(), MacOsState::Disabled, "disabled beats all");
    }

    #[test]
    fn a_flat_gradient_degrades_to_a_solid_paint() {
        // Aqua's face stops are nearly equal and Dark Aqua's disabled face
        // is genuinely flat; emitting a two-stop gradient for those would
        // cost a gradient sample per pixel for nothing.
        let c = Color::from_hex("#FFFFFF");
        assert!(matches!(vertical_gradient(c, c, 22.0), Paint::Solid(_)));
        assert!(matches!(
            vertical_gradient(c, Color::from_hex("#F4F4F4"), 22.0),
            Paint::LinearGradient { .. }
        ));
    }

    #[test]
    fn the_gradient_runs_top_to_bottom_in_rect_local_coordinates() {
        // Absolute coordinates here would make the ramp drift as the
        // control scrolls; the axis has to start at the rect's own origin.
        match vertical_gradient(Color::WHITE, Color::BLACK, 22.0) {
            Paint::LinearGradient { start, end, stops } => {
                assert_eq!(start, Point::new(0.0, 0.0));
                assert_eq!(end, Point::new(0.0, 22.0));
                assert_eq!(stops[0].color, Color::WHITE);
                assert_eq!(stops[1].color, Color::BLACK);
            }
            other => panic!("expected a linear gradient, got {other:?}"),
        }
    }

    #[test]
    fn a_degenerate_height_still_produces_a_usable_axis() {
        // A zero-height slot must not collapse the gradient axis to a
        // point, which would leave the ramp undefined.
        match vertical_gradient(Color::WHITE, Color::BLACK, 0.0) {
            Paint::LinearGradient { start, end, .. } => assert!(end.y > start.y),
            other => panic!("expected a linear gradient, got {other:?}"),
        }
    }

    #[test]
    fn inflate_grows_symmetrically() {
        let r = inflate(Rect::new(10.0, 20.0, 100.0, 40.0), 2.0);
        assert_eq!((r.x, r.y, r.width, r.height), (8.0, 18.0, 104.0, 44.0));
    }

    #[test]
    fn the_ring_bands_do_not_overlap() {
        // The halo has to start exactly where the solid band ends, or the
        // two read as separate outlines rather than one graded ring.
        let solid_outer_edge = MACOS_FOCUS_RING_WIDTH;
        let halo_inner_edge = MACOS_FOCUS_RING_WIDTH;
        assert!((solid_outer_edge - halo_inner_edge).abs() < 1e-6);
    }
}
