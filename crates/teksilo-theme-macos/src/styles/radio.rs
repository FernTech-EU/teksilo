// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS radio button.
//!
//! A **14 dp** circle — the checkbox's twin, and the same miniature
//! [bezel](crate::styles::chrome) while unselected. Selected, it fills
//! with the accent and shows a **4.5 dp** white dot.
//!
//! That dot is the interesting number. Fluent's is 12 dp inside a 20 dp
//! ring, and it swells to 14 on hover and shrinks to 10 under the press —
//! the glyph *is* Fluent's interaction language. AppKit's is a small, fixed
//! pip that never moves: a macOS radio says "selected" and nothing else,
//! and animating it would read as the wrong platform.
//!
//! It also means this style needs no animation at all, which sidesteps the
//! trap the Fluent radio documents: every signal on `RadioStyleConfig` is
//! a *derived* projection of the widget's own state, and `Signal::observe`
//! — which `BuildContext::effect` is built on — only accepts a mutable
//! signal. A style that installed a tween here would compile, pass its
//! unit tests, and panic on the first frame a radio was built.
//!
//! The unselected outline is `border_strong` rather than the bezel's own
//! hairline, for the WCAG reason documented on
//! [`crate::styles::checkbox`].

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{RadioStyle, RadioStyleConfig, RadioVariant};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius};

use crate::shape::MACOS_SMALL_CONTROL_SIZE;
use crate::styles::chrome::{MacOsState, paint_bezel, paint_focus_ring, resolve_bezel};

/// Outer diameter (dp) at the regular control size — the checkbox's twin.
const OUTER: f32 = MACOS_SMALL_CONTROL_SIZE;
/// The selected pip (dp). Fixed: AppKit does not resize it on hover or
/// press.
const DOT: f32 = 4.5;
/// Outline thickness while unselected (dp).
const STROKE: f32 = 1.0;
/// Corner radius used by the square / rounded variants.
const CORNER: f32 = 3.5;

// The pip has to clear the ring's stroke on both sides…
const _: () = assert!(DOT + STROKE * 2.0 <= OUTER);
// …and stay small enough to read as a pip rather than a filled circle.
const _: () = assert!(DOT < OUTER * 0.5);

/// macOS `RadioStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsRadioStyle;

impl RadioStyle for MacOsRadioStyle {
    fn make_body(&self, cfg: &RadioStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let state = MacOsState::derive(&cfg.is_disabled, &cfg.is_pressed, &cfg.is_hovered);
        let show_ring = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);

        ctx.add(MacOsRadioBody {
            is_selected: cfg.is_selected.clone(),
            state,
            show_ring,
            variant: cfg.variant,
        })
    }
}

struct MacOsRadioBody {
    is_selected: Signal<bool>,
    state: Signal<MacOsState>,
    show_ring: Signal<bool>,
    variant: RadioVariant,
}

impl std::fmt::Debug for MacOsRadioBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsRadioBody")
            .field("variant", &self.variant)
            .finish()
    }
}

/// The radius a variant paints at.
fn variant_radius(variant: RadioVariant) -> f32 {
    match variant {
        RadioVariant::Circle => OUTER * 0.5,
        RadioVariant::Rounded => CORNER * 2.0,
        RadioVariant::Square => CORNER,
    }
}

impl Widget for MacOsRadioBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_selected
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.state.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.show_ring
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(OUTER, OUTER).into()
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
        let selected = self.is_selected.get();
        let state = self.state.get();
        let c = &ctx.theme.colors;
        let radius = variant_radius(self.variant);
        let corner = CornerRadius::uniform(radius);

        if selected {
            let fill = match state {
                MacOsState::Rest => c.accent,
                MacOsState::Hover => c.accent_hover,
                MacOsState::Pressed => c.accent_pressed,
                MacOsState::Disabled => c.accent_disabled,
            };
            if fill.a() > 0.0 {
                canvas.fill_rounded_rect(bounds, corner, fill);
            }
        } else {
            let bezel = resolve_bezel(ctx, state);
            paint_bezel(canvas, bounds, radius, &bezel, ctx.theme.shape.border_width);
            let outline = if state == MacOsState::Disabled {
                c.border_disabled
            } else {
                c.border_strong
            };
            if outline.a() > 0.0 {
                canvas.stroke_rounded_rect(bounds, corner, outline, STROKE);
            }
        }

        if self.show_ring.get() {
            paint_focus_ring(canvas, bounds, radius, ctx);
        }

        if selected {
            let dot: Color = if state == MacOsState::Disabled {
                c.text_disabled
            } else {
                c.text_on_accent
            };
            canvas.fill_circle(
                Point::new(
                    bounds.x + bounds.width * 0.5,
                    bounds.y + bounds.height * 0.5,
                ),
                DOT * 0.5,
                dot,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_is_the_regular_size_macos_radio() {
        // Containment and pip-proportion are compile-time invariants above.
        assert_eq!(OUTER, 14.0);
        assert_eq!(DOT, 4.5);
        assert_eq!(STROKE, 1.0);
    }

    /// The proportion that distinguishes an AppKit radio from a WinUI one.
    #[test]
    fn the_pip_is_small_where_fluents_glyph_is_large() {
        let macos_ratio = DOT / OUTER;
        let fluent_ratio = 12.0 / 20.0;
        assert!(
            macos_ratio < fluent_ratio * 0.75,
            "the pip is {macos_ratio}, close to Fluent's {fluent_ratio}"
        );
    }

    #[test]
    fn the_radio_and_the_checkbox_are_the_same_size() {
        // AppKit draws them as a matched pair; a mismatch is immediately
        // visible in a form that mixes both. Both read the one shared
        // constant, so this pins that neither has drifted off it.
        assert_eq!(OUTER, MACOS_SMALL_CONTROL_SIZE);
    }

    #[test]
    fn variants_move_only_the_radius() {
        assert_eq!(variant_radius(RadioVariant::Circle), OUTER * 0.5);
        assert_eq!(variant_radius(RadioVariant::Square), CORNER);
        assert!(variant_radius(RadioVariant::Rounded) > CORNER);
    }
}
