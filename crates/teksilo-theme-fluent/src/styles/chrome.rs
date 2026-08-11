// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The paint vocabulary the Fluent widget styles share: the control
//! surface (fill + hairline + elevation edge) and the two-tone focus ring.
//!
//! Both are painted by [`FluentControlChrome`], a leaf widget that resolves
//! everything from [`PaintContext::theme`] at paint time. That matters for
//! two reasons a build-time-baked colour could not deliver:
//!
//! - a live `ctx.set_theme(...)` swap repaints with the new palette, and
//! - accent-derived colours come from `theme.colors.*`, which the paint
//!   walker has already projected through
//!   [`ColorTokens::for_inactive_window`](teksilo_tokens::ColorTokens::for_inactive_window),
//!   so an accent button greys out in a background window with no extra code.
//!
//! Neutral colours come from the [`FluentPalette`] extension, which carries
//! the WinUI tokens `ColorTokens` has no slot for (the graded control fills,
//! the on-accent strokes). Every read falls back to a theme role when the
//! extension is absent, so a `FluentControlChrome` dropped into a non-Fluent
//! theme still paints something sane.

use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius};

use crate::palette::{FluentEdgeSide, FluentPalette};

/// Which WinUI button/control family a surface belongs to. Decides the fill
/// ramp, the hairline colour, and whether an elevation edge is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluentSurfaceKind {
    /// `Button` / `ComboBox` / `TextBox` — `ControlFillColor*` over a
    /// `ControlStrokeColorDefault` hairline with the neutral elevation edge.
    Standard,
    /// `AccentButton` — the accent fill ramp, on-accent strokes, and the
    /// bottom elevation edge in both appearances.
    Accent,
    /// `AccentButton` recoloured for a destructive action. Fluent has no
    /// such button; the fill family is `SystemFillColorCritical` graded by
    /// the same 90 % / 80 % opacity steps WinUI applies to the accent one.
    Critical,
    /// `SubtleButton` / `HyperlinkButton` / menu and list rows —
    /// transparent at rest, `SubtleFillColor*` on hover and press, no
    /// hairline and no elevation edge.
    Subtle,
}

/// Interaction state, in WinUI's own precedence order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FluentState {
    #[default]
    Rest,
    Hover,
    Pressed,
    Disabled,
}

impl FluentState {
    /// Collapse the usual four booleans into a state, disabled first.
    pub fn derive(
        is_disabled: &Signal<bool>,
        is_pressed: &Signal<bool>,
        is_hovered: &Signal<bool>,
    ) -> Signal<FluentState> {
        is_disabled
            .zip3(is_pressed, is_hovered)
            .map(|(disabled, pressed, hovered)| {
                if *disabled {
                    FluentState::Disabled
                } else if *pressed {
                    FluentState::Pressed
                } else if *hovered {
                    FluentState::Hover
                } else {
                    FluentState::Rest
                }
            })
    }
}

/// Resolve the fill a surface paints for a state.
pub fn surface_fill(
    kind: FluentSurfaceKind,
    state: FluentState,
    ctx: &PaintContext,
) -> Option<Color> {
    let colors = &ctx.theme.colors;
    let p = ctx.theme.extension::<FluentPalette>();
    match kind {
        FluentSurfaceKind::Standard => {
            let p = p?;
            Some(match state {
                FluentState::Rest => p.control_fill_default,
                FluentState::Hover => p.control_fill_secondary,
                FluentState::Pressed => p.control_fill_tertiary,
                FluentState::Disabled => p.control_fill_disabled,
            })
        }
        FluentSurfaceKind::Accent => Some(match state {
            // These four already carry Fluent's 100 / 90 / 80 % accent ramp
            // and the literal disabled fill — and, unlike the extension,
            // they are desaturated for an inactive window.
            FluentState::Rest => colors.accent,
            FluentState::Hover => colors.accent_hover,
            FluentState::Pressed => colors.accent_pressed,
            FluentState::Disabled => colors.accent_disabled,
        }),
        FluentSurfaceKind::Critical => {
            let base = colors.surface_main;
            Some(match state {
                FluentState::Rest => colors.status_error_fg,
                FluentState::Hover => {
                    crate::palette::over(colors.status_error_fg.with_alpha(0.9), base)
                }
                FluentState::Pressed => {
                    crate::palette::over(colors.status_error_fg.with_alpha(0.8), base)
                }
                FluentState::Disabled => colors.accent_disabled,
            })
        }
        FluentSurfaceKind::Subtle => {
            let p = p?;
            Some(match state {
                FluentState::Rest | FluentState::Disabled => p.subtle_fill_transparent,
                FluentState::Hover => p.subtle_fill_secondary,
                FluentState::Pressed => p.subtle_fill_tertiary,
            })
        }
    }
}

/// The hairline a surface strokes around its whole outline, if any.
fn surface_hairline(kind: FluentSurfaceKind, ctx: &PaintContext) -> Option<Color> {
    let p = ctx.theme.extension::<FluentPalette>();
    match kind {
        FluentSurfaceKind::Standard => {
            Some(p.map_or(ctx.theme.colors.border, |p| p.control_stroke_default))
        }
        FluentSurfaceKind::Accent | FluentSurfaceKind::Critical => {
            Some(p?.control_stroke_on_accent_default)
        }
        FluentSurfaceKind::Subtle => None,
    }
}

/// The emphasised edge a raised surface draws, and which side it is on.
fn surface_edge(kind: FluentSurfaceKind, ctx: &PaintContext) -> Option<(FluentEdgeSide, Color)> {
    let p = ctx.theme.extension::<FluentPalette>()?;
    match kind {
        FluentSurfaceKind::Standard => Some(p.control_elevation_edge()),
        FluentSurfaceKind::Accent | FluentSurfaceKind::Critical => Some(p.accent_elevation_edge()),
        FluentSurfaceKind::Subtle => None,
    }
}

/// Paint one horizontal edge of a rounded rect as a `thickness`-tall pill
/// inset by the corner radius.
///
/// WinUI gets this edge for free by stroking the whole outline with a
/// gradient brush anchored to a fixed-height band. Teksilo strokes with a
/// flat colour, so the edge is drawn as its own sliver — at 1 dp the
/// difference from a true arc-following stroke is not resolvable, and this
/// way the corner arcs keep the plain hairline colour, exactly as the
/// gradient produces.
pub fn paint_edge(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: f32,
    side: FluentEdgeSide,
    thickness: f32,
    color: Color,
) {
    if thickness <= 0.0 || color.a() <= 0.0 {
        return;
    }
    let inset = radius.min(bounds.width * 0.5);
    let width = bounds.width - inset * 2.0;
    if width <= 0.0 {
        return;
    }
    let y = match side {
        FluentEdgeSide::Top => bounds.y,
        FluentEdgeSide::Bottom => bounds.y + bounds.height - thickness,
    };
    canvas.fill_rounded_rect(
        Rect::new(bounds.x + inset, y, width, thickness),
        CornerRadius::uniform(thickness * 0.5),
        color,
    );
}

/// Paint Fluent's two-tone focus ring around `bounds`.
///
/// `FocusVisualSecondaryThickness` (1 dp, the low-contrast ring) hugs the
/// control; `FocusVisualPrimaryThickness` (2 dp, the high-contrast ring)
/// sits immediately outside it. The primary colour is near-black in light
/// and pure white in dark, so the indicator reads against any background —
/// which is why Fluent does *not* use the accent here.
pub fn paint_focus_ring(canvas: &mut Canvas, bounds: Rect, radius: f32, ctx: &PaintContext) {
    let shape = &ctx.theme.shape;
    let p = ctx.theme.extension::<FluentPalette>();
    let outer_color = p.map_or(ctx.theme.colors.focus_ring, |p| p.focus_stroke_outer);
    let inner_color = p.map_or(ctx.theme.colors.surface_main, |p| p.focus_stroke_inner);
    let inner_w = crate::shape::FLUENT_FOCUS_RING_INNER_WIDTH;
    let outer_w = shape.focus_ring_width;

    // Inner ring: straddles the control edge outward by `inner_w`.
    canvas.stroke_rounded_rect(
        inflate(bounds, inner_w * 0.5),
        CornerRadius::uniform(radius + inner_w * 0.5),
        inner_color,
        inner_w,
    );
    // Outer ring: starts where the inner one ends.
    let offset = inner_w + outer_w * 0.5;
    canvas.stroke_rounded_rect(
        inflate(bounds, offset),
        CornerRadius::uniform(radius + offset),
        outer_color,
        outer_w,
    );
}

/// Grow a rect by `d` on every side.
pub fn inflate(r: Rect, d: f32) -> Rect {
    Rect::new(r.x - d, r.y - d, r.width + d * 2.0, r.height + d * 2.0)
}

/// The chrome layer of a Fluent control: fill, hairline, elevation edge and
/// focus ring, painted behind whatever content the style stacks on top.
///
/// Sized like [`RectWidget`](teksilo_widgets::primitives::RectWidget) — it
/// takes whatever the proposal offers and reports zero when unbounded — so
/// it can sit under a `ZStack` without influencing the control's size.
pub struct FluentControlChrome {
    kind: FluentSurfaceKind,
    corner_radius: f32,
    state: Signal<FluentState>,
    /// `true` while the control should show its focus ring — already gated
    /// on `:focus-visible` by the caller where the widget exposes it.
    show_focus_ring: Signal<bool>,
}

impl FluentControlChrome {
    pub fn new(
        kind: FluentSurfaceKind,
        corner_radius: f32,
        state: Signal<FluentState>,
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

impl std::fmt::Debug for FluentControlChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluentControlChrome")
            .field("kind", &self.kind)
            .finish()
    }
}

impl Widget for FluentControlChrome {
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
        let corner = CornerRadius::uniform(radius);

        if let Some(fill) = surface_fill(self.kind, state, ctx)
            && fill.a() > 0.0
        {
            canvas.fill_rounded_rect(bounds, corner, fill);
        }

        let hairline_w = ctx.theme.shape.border_width;
        if let Some(hairline) = surface_hairline(self.kind, ctx)
            && hairline.a() > 0.0
            && hairline_w > 0.0
        {
            canvas.stroke_rounded_rect(bounds, corner, hairline, hairline_w);
        }

        // WinUI drops a pressed control back to a flat `ControlStrokeColorDefault`
        // outline — the elevation edge is what makes it look raised, so
        // losing it is the press feedback.
        if state != FluentState::Pressed
            && state != FluentState::Disabled
            && let Some((side, color)) = surface_edge(self.kind, ctx)
        {
            paint_edge(canvas, bounds, radius, side, hairline_w, color);
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
pub struct FluentSpacerBox {
    size: Size,
}

impl FluentSpacerBox {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
        }
    }
}

impl Widget for FluentSpacerBox {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        self.size.into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}
