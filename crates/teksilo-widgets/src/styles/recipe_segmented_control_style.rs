// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `SegmentedControlStyle` impl driven by paint-recipe data.
//!
//! `RecipeSegmentedControlStyle` ports the IntUI segmented-control
//! chrome: the rounded frame, per-segment hover tint, the
//! selected-segment surface + border (accent when focused, inactive
//! when not), a divider before the overflow trigger, and the keyboard
//! focus ring drawn outside the visual envelope. The recipe builds a
//! single `SegmentedControlChrome` widget that paints all of this from
//! the config's state signals and the geometry the widget publishes each
//! layout pass — repainting only when the bindings flip.

use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SegmentSlots, SegmentedControlStyle, SegmentedControlStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::CornerRadius;

// IntUI design tokens for SegmentedControl. The recipe owns its own
// dimensions.
pub const SEGMENTED_CONTROL_HEIGHT: f32 = 24.0;
pub const SEGMENTED_CONTROL_PADDING_HORIZONTAL: f32 = 12.0;
pub const SEGMENTED_CONTROL_PADDING_VERTICAL: f32 = 6.0;
pub const SEGMENTED_CONTROL_CORNER_RADIUS: f32 = 3.0;
pub const SEGMENTED_CONTROL_BORDER_WIDTH: f32 = 1.0;

/// Tuneable dimensions for [`RecipeSegmentedControlStyle`].
///
/// All fields default to the corresponding `SEGMENTED_CONTROL_*` consts so
/// a `RecipeSegmentedControlStyle::default()` is identical to the original
/// hard-coded behaviour.  Pass a customised `SegmentedControlRecipe` to
/// `RecipeSegmentedControlStyle::new(recipe)` to override individual dims.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentedControlRecipe {
    pub height: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub border_width: f32,
}

impl Default for SegmentedControlRecipe {
    fn default() -> Self {
        Self {
            height: SEGMENTED_CONTROL_HEIGHT,
            padding_horizontal: SEGMENTED_CONTROL_PADDING_HORIZONTAL,
            padding_vertical: SEGMENTED_CONTROL_PADDING_VERTICAL,
            corner_radius: SEGMENTED_CONTROL_CORNER_RADIUS,
            border_width: SEGMENTED_CONTROL_BORDER_WIDTH,
        }
    }
}

/// Default `SegmentedControlStyle` shipped with Teksilo.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSegmentedControlStyle {
    pub recipe: SegmentedControlRecipe,
}

impl RecipeSegmentedControlStyle {
    pub fn new(recipe: SegmentedControlRecipe) -> Self {
        Self { recipe }
    }
}

impl SegmentedControlStyle for RecipeSegmentedControlStyle {
    fn make_body(&self, cfg: &SegmentedControlStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(SegmentedControlChrome {
            slots: cfg.slots.clone(),
            selected: cfg.selected.clone(),
            hovered_segment: cfg.hovered_segment.clone(),
            focus_origin: cfg.focus_origin.clone(),
            is_enabled: cfg.is_enabled.clone(),
            recipe: self.recipe,
        })
    }
}

/// Internal recipe widget that paints the segmented-control chrome
/// *behind* the segment cells: the rounded frame, per-segment hover
/// tint, the selected-segment surface + border, the overflow divider,
/// and the keyboard focus ring. Labels and icons are composed widgets
/// the `SegmentedControl` places on top — the chrome draws no text or
/// icons.
struct SegmentedControlChrome {
    slots: SegmentSlots,
    selected: Signal<usize>,
    hovered_segment: Signal<Option<usize>>,
    focus_origin: Signal<Option<FocusOrigin>>,
    /// Reactive — re-paints on arena `enabled_state` flip.
    is_enabled: Signal<bool>,
    recipe: SegmentedControlRecipe,
}

impl std::fmt::Debug for SegmentedControlChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedControlChrome")
            .field("slots", &self.slots.len())
            .finish()
    }
}

impl Widget for SegmentedControlChrome {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        // Repaint on any state-signal change. The segment cells own their
        // own (reactive) labels/icons, so the chrome binds only the
        // background-state signals. The slot geometry needs no binding:
        // it is republished during the layout pass that precedes every
        // paint that could have moved it.
        self.selected
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.hovered_segment
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        // Also subscribe to is_enabled so a reactive enable/disable
        // flip via `enabled_when` re-paints the chrome with the
        // dimmed palette.
        self.is_enabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(
            proposal.width.unwrap_or(0.0),
            proposal.height.unwrap_or(0.0),
        )
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;
        let shape = &ctx.theme.shape;
        let bw = self.recipe.border_width;
        let frame_cr = CornerRadius::uniform(self.recipe.corner_radius);

        let selected = self.selected.get();
        let hovered = self.hovered_segment.get();
        let focus_origin = self.focus_origin.get();
        let focused = focus_origin.is_some();
        let keyboard_focused = focus_origin == Some(FocusOrigin::Keyboard);
        // Snapshot the reactive enabled-state once per paint. The
        // chrome subscribed to this signal in build() so a flip
        // re-paints with the new palette.
        let is_enabled = self.is_enabled.get();

        self.slots.with(|geometry| {
            if geometry.segments.is_empty() {
                // Before the first layout pass, or with every segment
                // hidden. Nothing to frame.
                return;
            }

            // 1. Outer frame.
            let frame_border = if !is_enabled {
                colors.border
            } else {
                colors.border_strong
            };
            canvas.stroke_rounded_rect(geometry.frame, frame_cr, frame_border, bw);

            // Resolve the live segment indices carried by the state
            // signals into slot positions. A segment that overflowed
            // while hovered resolves to `None` and simply paints nothing.
            let selected_slot = geometry.order.iter().position(|&s| s == selected);
            let hovered_slot = hovered.and_then(|h| geometry.order.iter().position(|&s| s == h));

            // 2. Non-selected segments — hover tint only (the cell widget
            //    draws the label/icon on top).
            if is_enabled
                && let Some(slot) = hovered_slot
                && Some(slot) != selected_slot
                && let Some(rect) = geometry.segments.get(slot)
            {
                canvas.fill_rounded_rect(*rect, frame_cr, colors.surface_hover);
            }

            // 3. Selected segment — surface + border, extended by `bw` on
            //    all sides so the stroke covers the frame border AND any
            //    adjacent hover tint on middle segments. The label/icon is
            //    drawn by the cell widget; its tint follows this background
            //    reactively (OnAccent when focused).
            if let Some(slot) = selected_slot
                && let Some(base) = geometry.segments.get(slot)
            {
                let sel = Rect::new(
                    base.x - bw,
                    base.y - bw,
                    base.width + bw * 2.0,
                    base.height + bw * 2.0,
                );
                let (sel_bg, sel_border) = if !is_enabled {
                    (colors.surface_selected_inactive, colors.border)
                } else if focused {
                    (colors.accent, colors.accent)
                } else {
                    (colors.surface_selected_inactive, colors.border_strong)
                };
                canvas.fill_rounded_rect(sel, frame_cr, sel_bg);
                canvas.stroke_rounded_rect(sel, frame_cr, sel_border, bw);
            }

            // 4. Divider before the overflow trigger, so the chevron reads
            //    as a slot of the strip rather than a floating button.
            if let Some(overflow) = geometry.overflow {
                canvas.fill_rect(
                    Rect::new(overflow.x, overflow.y, bw, overflow.height),
                    frame_border,
                );
            }
        });

        // 5. Focus ring — drawn OUTSIDE the visual, inside the reserved
        //    envelope. Painted whether or not there are segments, so a
        //    focused empty control still shows where focus is.
        if keyboard_focused {
            let half_stroke = shape.focus_ring_width * 0.5;
            let ring_rect = Rect::new(
                bounds.x + half_stroke,
                bounds.y + half_stroke,
                (bounds.width - half_stroke * 2.0).max(0.0),
                (bounds.height - half_stroke * 2.0).max(0.0),
            );
            let ring_radius = self.recipe.corner_radius + shape.focus_ring_offset + half_stroke;
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(ring_radius),
                colors.focus_ring,
                shape.focus_ring_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the parent `SegmentedControl` emits the
        // `Role::RadioGroup` and the per-segment cells emit
        // `Role::RadioButton`.
        builder.set_hidden();
    }
}
