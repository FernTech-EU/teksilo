//! Default `SegmentedControlStyle` impl driven by paint-recipe data.
//!
//! `RecipeSegmentedControlStyle` ports the IntUI segmented-control
//! chrome exactly: the rounded frame, per-segment hover tint, the
//! selected-segment surface + border (accent when focused, inactive
//! when not), per-segment label rendering via `canvas.draw_text`, and
//! the keyboard focus ring drawn outside the visual envelope. The
//! recipe builds a single `SegmentedControlChrome` widget that paints
//! all of this from the config's state signals — repainting only when
//! the bindings flip.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::focus::FocusOrigin;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{SegmentedControlStyle, SegmentedControlStyleConfig};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

// IntUI design tokens for SegmentedControl. The recipe owns its own
// dimensions.
pub const SEGMENTED_CONTROL_HEIGHT: f32 = 24.0;
pub const SEGMENTED_CONTROL_PADDING_HORIZONTAL: f32 = 12.0;
pub const SEGMENTED_CONTROL_PADDING_VERTICAL: f32 = 6.0;
pub const SEGMENTED_CONTROL_CORNER_RADIUS: f32 = 3.0;
pub const SEGMENTED_CONTROL_BORDER_WIDTH: f32 = 1.0;

/// Default `SegmentedControlStyle` shipped with Bastyde.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSegmentedControlStyle;

impl SegmentedControlStyle for RecipeSegmentedControlStyle {
    fn make_body(&self, cfg: &SegmentedControlStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(SegmentedControlChrome {
            labels: cfg.labels.clone(),
            selected: cfg.selected.clone(),
            hovered_segment: cfg.hovered_segment.clone(),
            focus_origin: cfg.focus_origin.clone(),
            is_enabled: cfg.is_enabled.clone(),
        })
    }
}

/// Internal recipe widget that paints the full segmented-control
/// chrome (frame, per-segment hover, selected, labels, focus ring).
/// Mirrors the pre-migration `SegmentedControl::paint` exactly,
/// reading state from the same signals the widget owns.
struct SegmentedControlChrome {
    labels: Vec<String>,
    selected: Signal<usize>,
    hovered_segment: Signal<Option<usize>>,
    focus_origin: Signal<Option<FocusOrigin>>,
    /// Reactive — re-paints on arena `enabled_state` flip.
    is_enabled: Signal<bool>,
}

impl std::fmt::Debug for SegmentedControlChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedControlChrome")
            .field("segments", &self.labels.len())
            .finish()
    }
}

impl SegmentedControlChrome {
    fn compute_visual(&self, bounds: Rect, theme: &bastyde_core::Theme) -> Rect {
        let envelope = theme.shape.focus_ring_offset + theme.shape.focus_ring_width;
        Rect::new(
            bounds.x + envelope,
            bounds.y + envelope,
            (bounds.width - envelope * 2.0).max(0.0),
            (bounds.height - envelope * 2.0).max(0.0),
        )
    }

    fn compute_inner(visual: Rect, bw: f32) -> Rect {
        Rect::new(
            visual.x + bw,
            visual.y + bw,
            (visual.width - bw * 2.0).max(0.0),
            (visual.height - bw * 2.0).max(0.0),
        )
    }

    fn segment_rect(index: usize, inner: Rect, n: usize) -> Rect {
        let w = if n == 0 { 0.0 } else { inner.width / n as f32 };
        Rect::new(inner.x + index as f32 * w, inner.y, w, inner.height)
    }
}

impl Widget for SegmentedControlChrome {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        // Repaint on any state-signal change. Label changes are
        // structural and live on the parent `SegmentedControl`'s
        // rebuild path, so we don't bind to a labels signal here.
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
        let n = self.labels.len();
        if n == 0 {
            return;
        }

        let visual = self.compute_visual(bounds, ctx.theme);
        let bw = SEGMENTED_CONTROL_BORDER_WIDTH;
        let inner = Self::compute_inner(visual, bw);

        let selected = self.selected.get();
        let hovered = self.hovered_segment.get();
        let focus_origin = self.focus_origin.get();
        let focused = focus_origin.is_some();
        let keyboard_focused = focus_origin == Some(FocusOrigin::Keyboard);
        let frame_cr = CornerRadius::uniform(SEGMENTED_CONTROL_CORNER_RADIUS);
        // Snapshot the reactive enabled-state once per paint. The
        // chrome subscribed to this signal in build() so a flip
        // re-paints with the new palette.
        let is_enabled = self.is_enabled.get();

        // 1. Outer frame.
        let frame_border = if !is_enabled {
            colors.border
        } else {
            colors.border_strong
        };
        canvas.stroke_rounded_rect(visual, frame_cr, frame_border, bw);

        // 2. Non-selected segments — hover tint + label.
        for i in 0..n {
            if i == selected {
                continue;
            }
            let rect = Self::segment_rect(i, inner, n);
            if is_enabled && hovered == Some(i) {
                canvas.fill_rounded_rect(rect, frame_cr, colors.surface_hover);
            }
            let text_color = if !is_enabled {
                colors.text_disabled
            } else {
                colors.text_primary
            };
            let text_rect = Rect::new(
                rect.x + SEGMENTED_CONTROL_PADDING_HORIZONTAL,
                rect.y + SEGMENTED_CONTROL_PADDING_VERTICAL,
                (rect.width - SEGMENTED_CONTROL_PADDING_HORIZONTAL * 2.0).max(0.0),
                (rect.height - SEGMENTED_CONTROL_PADDING_VERTICAL * 2.0).max(0.0),
            );
            canvas.draw_text(
                &self.labels[i],
                text_rect,
                &ctx.theme.typography.small,
                text_color,
            );
        }

        // 3. Selected segment — extended by `bw` on all sides so the
        //    stroke covers the frame border AND any adjacent hover
        //    tint on middle segments.
        if selected < n {
            let sel_base = Self::segment_rect(selected, inner, n);
            let sel = Rect::new(
                sel_base.x - bw,
                sel_base.y - bw,
                sel_base.width + bw * 2.0,
                sel_base.height + bw * 2.0,
            );
            let (sel_bg, sel_border, sel_text) = if !is_enabled {
                (
                    colors.surface_selected_inactive,
                    colors.border,
                    colors.text_disabled,
                )
            } else if focused {
                (colors.accent, colors.accent, colors.text_on_accent)
            } else {
                (
                    colors.surface_selected_inactive,
                    colors.border_strong,
                    colors.text_primary,
                )
            };
            canvas.fill_rounded_rect(sel, frame_cr, sel_bg);
            canvas.stroke_rounded_rect(sel, frame_cr, sel_border, bw);

            let text_rect = Rect::new(
                sel.x + SEGMENTED_CONTROL_PADDING_HORIZONTAL,
                sel.y + SEGMENTED_CONTROL_PADDING_VERTICAL,
                (sel.width - SEGMENTED_CONTROL_PADDING_HORIZONTAL * 2.0).max(0.0),
                (sel.height - SEGMENTED_CONTROL_PADDING_VERTICAL * 2.0).max(0.0),
            );
            canvas.draw_text(
                &self.labels[selected],
                text_rect,
                &ctx.theme.typography.small,
                sel_text,
            );
        }

        // 4. Focus ring — drawn OUTSIDE the visual, inside the reserved envelope.
        if keyboard_focused {
            let half_stroke = shape.focus_ring_width * 0.5;
            let ring_rect = Rect::new(
                bounds.x + half_stroke,
                bounds.y + half_stroke,
                (bounds.width - half_stroke * 2.0).max(0.0),
                (bounds.height - half_stroke * 2.0).max(0.0),
            );
            let ring_radius =
                SEGMENTED_CONTROL_CORNER_RADIUS + shape.focus_ring_offset + half_stroke;
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
        // `Role::RadioGroup` and the per-segment `SegmentButton`
        // children emit `Role::RadioButton`.
        builder.set_hidden();
    }
}

/// Compute the natural width the segmented-control should claim from
/// its parent. Exposed so the widget's `layout_response` can size to
/// the recipe's fallback-width policy when no explicit width is
/// proposed (kept here so a custom style that ships its own
/// width-estimate function can keep both sides in sync).
pub fn estimate_segmented_width(labels: &[&str], fallback_char_width: f32) -> f32 {
    let n = labels.len();
    if n == 0 {
        return 0.0;
    }
    let max_label_width = labels
        .iter()
        .map(|l| l.len() as f32 * fallback_char_width)
        .fold(0.0_f32, f32::max);
    (max_label_width + SEGMENTED_CONTROL_PADDING_HORIZONTAL * 2.0) * n as f32
}
