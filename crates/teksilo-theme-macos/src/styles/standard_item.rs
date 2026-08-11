// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS list / tree / sidebar row, with its selection **capsule**.
//!
//! Since Big Sur a selected row is a rounded rectangle *inset from the
//! view's edges* — filled with `selectedContentBackgroundColor` and
//! carrying `alternateSelectedControlTextColor` (white) — rather than the
//! full-bleed square-cornered bar of every macOS before it. Apple's own
//! wording: "highlighted elements now have rounded corners". It is the
//! single most recognisable element of a macOS list.
//!
//! When the view loses keyboard focus, or its window goes inactive, the
//! capsule drops to the neutral
//! `unemphasizedSelectedContentBackgroundColor` grey and the label reverts
//! to `labelColor` — AppKit uses one "unemphasized" appearance for both
//! states, which is exactly the pairing Teksilo's
//! `SurfaceRole::SelectedInactive` models.
//!
//! ## Why the chrome is painted here rather than delegated
//!
//! The shipped [`RecipeStandardItemStyle`] paints its selection background
//! from `SurfaceRole::Selected`, and this preset deliberately maps that
//! token to a **wash** rather than a solid accent: it is also the fill of
//! the `TableView` / `TreeTableView` selection band and of `GridView`
//! tiles, whose cell content is app-supplied and resolves
//! `TextRole::Primary` — a saturated fill there would leave dark text at
//! roughly 3.5:1. See the crate's colour projection.
//!
//! So the delegate is handed interaction signals that never fire and
//! paints no chrome at all, and `MacOsSelectionCapsule` draws the real
//! one underneath the row: the solid accent capsule where this style owns
//! both halves, and the hover and press washes too, so a *selected* row
//! does not lighten under the pointer the way the shipped cascade would
//! make it. The label flip is declared through
//! [`StandardItemStyle::selected_label_role`].

use teksilo_canvas::{Canvas, Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{StandardItemStyle, StandardItemStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, TextRole};
use teksilo_widgets::styles::{RecipeStandardItemStyle, StandardItemRecipe};

use crate::shape::MACOS_FOCUS_RING_WIDTH;
use crate::styles::chrome::paint_focus_ring;

/// `NSTableView.rowHeight` on macOS 11+ (it was 17 dp before Big Sur).
const ROW_HEIGHT: f32 = 24.0;
/// A row carrying a subtitle: the single-line height plus a Callout line
/// box and a hair of air.
const ROW_HEIGHT_TWO_LINE: f32 = 40.0;
/// Sidebar / list icon size (dp).
const ICON_SIZE: f32 = 16.0;
/// The selection capsule's radius (dp).
const CAPSULE_RADIUS: f32 = 5.0;
/// How far the capsule floats inside the row's full width (dp).
const CAPSULE_INSET: f32 = 5.0;
/// The row's content gutter, measured from the **row's** edge — not the
/// capsule's.
///
/// `bg_horizontal_inset` moves only the background rect; the shipped
/// recipe pads its content from the full row bounds independently. So to
/// leave [`LABEL_INSET_IN_CAPSULE`] of air between the capsule's edge and
/// the label, the content gutter has to carry the capsule inset too.
const PADDING_H: f32 = CAPSULE_INSET + LABEL_INSET_IN_CAPSULE;
/// Air between the capsule's leading edge and the label inside it (dp).
const LABEL_INSET_IN_CAPSULE: f32 = 8.0;

// The capsule has to leave breathing room inside the row it marks…
const _: () = assert!(CAPSULE_RADIUS < ROW_HEIGHT * 0.5);
const _: () = assert!(ROW_HEIGHT < ROW_HEIGHT_TWO_LINE);
// …and the label has to sit *inside* the capsule, not on its edge.
const _: () = assert!(PADDING_H > CAPSULE_INSET);

/// The macOS [`StandardItemRecipe`] — public so an app can tune one
/// dimension without rebuilding the style.
pub fn macos_standard_item_recipe() -> StandardItemRecipe {
    StandardItemRecipe {
        icon_size: ICON_SIZE,
        padding_horizontal: PADDING_H,
        padding_vertical: 2.0,
        min_height_single_line: ROW_HEIGHT,
        min_height_two_line: ROW_HEIGHT_TWO_LINE,
        tree_indent_step: 16.0,
        item_corner_radius: CAPSULE_RADIUS,
        bg_horizontal_inset: CAPSULE_INSET,
        focus_ring_width: MACOS_FOCUS_RING_WIDTH,
        // The delegate paints no chrome at all here — see the module doc.
        selection_edge_width: 0.0,
        ..StandardItemRecipe::default()
    }
}

/// macOS `StandardItemStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsStandardItemStyle;

impl StandardItemStyle for MacOsStandardItemStyle {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let capsule = ctx.add(MacOsSelectionCapsule {
            is_selected: cfg.is_selected.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_pressed: cfg.is_pressed.clone(),
            is_disabled: cfg.is_disabled.clone(),
            emphasised: cfg.is_focused.and(&cfg.is_window_active),
            show_ring: cfg
                .is_selected
                .zip3(&cfg.is_focused, &cfg.is_focus_visible)
                .map(|(sel, foc, vis)| *sel && *foc && *vis),
        });

        // Hand the delegate a row with no interaction of its own: the
        // capsule below owns every wash, so the two cannot double up.
        let quiet = Signal::new(false);
        let inner_cfg = StandardItemStyleConfig {
            content: cfg.content,
            is_selected: quiet.clone(),
            is_hovered: quiet.clone(),
            is_pressed: quiet,
            is_focused: cfg.is_focused.clone(),
            is_focus_visible: cfg.is_focus_visible.clone(),
            is_disabled: cfg.is_disabled.clone(),
            is_window_active: cfg.is_window_active.clone(),
        };
        let row =
            RecipeStandardItemStyle::new(macos_standard_item_recipe()).make_body(&inner_cfg, ctx);

        ctx.add(MacOsRowFrame { capsule, row })
    }

    fn selected_label_role(&self) -> Option<TextRole> {
        // `alternateSelectedControlTextColor` — white on the accent
        // capsule. See `crate::styles::menu_item` for why this cannot be
        // done from `make_body`.
        Some(TextRole::OnAccent)
    }
}

/// Stacks the selection capsule under the delegated row, stretching
/// **both** to the frame's bounds.
///
/// The obvious composition — a `ZStack` — is wrong here, and silently so.
/// `ZStack::layout_response` measures its children at an *unspecified*
/// width on purpose (so a greedy background cannot inflate the stack and a
/// shrinkable label cannot truncate during intrinsic measurement). The
/// delegated row's content sits behind an `Expand`, whose basis is zero,
/// so its natural width is just the padding — a `ZStack` wrapper would
/// centre a 20 dp row inside a 1200 dp list and spill its label straight
/// out the side. `StandardListItem` avoids this by placing its single root
/// child at full bounds; this frame does the same for two.
struct MacOsRowFrame {
    capsule: WidgetId,
    row: WidgetId,
}

impl std::fmt::Debug for MacOsRowFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsRowFrame").finish()
    }
}

impl Widget for MacOsRowFrame {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The capsule is listed first so it paints *behind* the row.
        vec![self.capsule, self.row]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // The row owns the frame's size — including its shrink weight and
        // compression floor, so a row inside a narrowing container still
        // truncates rather than overflowing. The capsule is pure overlay.
        ctx.child_layout_response(self.row, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.capsule, self.row]
    }
}

/// Paints a row's capsule: the accent selection, the unemphasised grey
/// fallback, the hover and press washes, and the keyboard focus ring.
///
/// A leaf rather than a `Padding`/`RectWidget` composition: a virtualized
/// list realizes and recycles these constantly, and one node per row beats
/// three.
struct MacOsSelectionCapsule {
    is_selected: Signal<bool>,
    is_hovered: Signal<bool>,
    is_pressed: Signal<bool>,
    is_disabled: Signal<bool>,
    /// Selected rows are vivid only while the view holds keyboard focus
    /// **and** the window is active; AppKit calls the other case
    /// "unemphasized" and uses one grey for both.
    emphasised: Signal<bool>,
    show_ring: Signal<bool>,
}

impl std::fmt::Debug for MacOsSelectionCapsule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsSelectionCapsule").finish()
    }
}

impl Widget for MacOsSelectionCapsule {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [
            &self.is_selected,
            &self.is_hovered,
            &self.is_pressed,
            &self.is_disabled,
            &self.emphasised,
            &self.show_ring,
        ] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
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
        let capsule = Rect::new(
            bounds.x + CAPSULE_INSET,
            bounds.y,
            (bounds.width - CAPSULE_INSET * 2.0).max(0.0),
            bounds.height,
        );
        if capsule.width <= 0.0 {
            return;
        }
        let corner = CornerRadius::uniform(CAPSULE_RADIUS);

        if let Some(fill) = self.fill(ctx)
            && fill.a() > 0.0
        {
            canvas.fill_rounded_rect(capsule, corner, fill);
        }

        if self.show_ring.get() {
            paint_focus_ring(canvas, capsule, CAPSULE_RADIUS, ctx);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Purely decorative — selection is announced by the row itself.
        builder.set_hidden();
    }
}

impl MacOsSelectionCapsule {
    /// `disabled > selected > pressed > hovered > nothing`.
    fn fill(&self, ctx: &PaintContext) -> Option<Color> {
        let c = &ctx.theme.colors;
        if self.is_disabled.get() {
            // A disabled row must not look pickable.
            return None;
        }
        if self.is_selected.get() {
            return Some(if self.emphasised.get() {
                // `selectedContentBackgroundColor`, already desaturated by
                // the paint walker in an inactive window.
                c.accent
            } else {
                c.surface_selected_inactive
            });
        }
        if self.is_pressed.get() {
            return Some(c.surface_pressed);
        }
        if self.is_hovered.get() {
            return Some(c.surface_hover);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_height_is_the_big_sur_table_row() {
        // `NSTableView.rowHeight` moved from 17 to 24 in macOS 11.
        let r = macos_standard_item_recipe();
        assert_eq!(r.min_height_single_line, 24.0);
        assert!(r.min_height_two_line > r.min_height_single_line);
        assert_eq!(r.icon_size, ICON_SIZE);
    }

    #[test]
    fn the_capsule_floats_inside_the_row() {
        // Big Sur's "highlighted elements now have rounded corners" — the
        // selection is inset, not full-bleed.
        let r = macos_standard_item_recipe();
        assert!(r.bg_horizontal_inset > 0.0);
        assert_eq!(r.bg_horizontal_inset, CAPSULE_INSET);
        assert_eq!(r.item_corner_radius, CAPSULE_RADIUS);
    }

    /// `bg_horizontal_inset` moves the *background* only — the shipped
    /// recipe pads its content from the full row bounds independently. A
    /// gutter that forgot to account for the capsule inset would put the
    /// label 3 dp from the capsule's edge instead of 8, which reads as a
    /// cramped row and is easy to miss in a screenshot.
    #[test]
    fn the_label_sits_inside_the_capsule_not_on_its_edge() {
        let r = macos_standard_item_recipe();
        assert!(r.padding_horizontal > r.bg_horizontal_inset);
        assert_eq!(
            r.padding_horizontal - r.bg_horizontal_inset,
            LABEL_INSET_IN_CAPSULE
        );
    }

    #[test]
    fn the_delegate_paints_no_chrome_of_its_own() {
        // Both cues are drawn by the capsule; leaving the delegate's
        // selection edge on would double the boundary.
        assert_eq!(macos_standard_item_recipe().selection_edge_width, 0.0);
        // …but the *recipe's* focus-ring width still has to be non-zero,
        // because a caller reading the recipe directly (an app tuning one
        // dimension) would otherwise inherit a ringless row.
        assert!(macos_standard_item_recipe().focus_ring_width > 0.0);
    }

    #[test]
    fn rows_are_denser_than_the_fluent_list() {
        // Fluent's `ListViewItemMinHeight` is 40 dp.
        assert!(macos_standard_item_recipe().min_height_single_line < 40.0);
    }

    /// The pairing the capsule depends on: a solid accent fill is only
    /// legible because the label flips with it.
    ///
    /// As with the menu row, it is **Aqua** that needs the flip —
    /// `labelColor` there is 85 % black and lands near 3.5:1 on the
    /// capsule. Dark Aqua's label is already white.
    #[test]
    fn the_selected_label_clears_contrast_on_the_capsule() {
        assert_eq!(
            MacOsStandardItemStyle.selected_label_role(),
            Some(TextRole::OnAccent)
        );
        for theme in [crate::light(), crate::dark()] {
            let c = &theme.colors;
            let flipped = crate::palette::over(TextRole::OnAccent.resolve(c), c.accent);
            assert!(
                flipped.contrast_ratio(c.accent) >= 4.5,
                "the flipped label is only {:.2}:1",
                flipped.contrast_ratio(c.accent)
            );
            // …and the *unemphasised* capsule keeps the normal label, which
            // has to clear contrast on the grey.
            let normal =
                crate::palette::over(TextRole::Primary.resolve(c), c.surface_selected_inactive);
            assert!(
                normal.contrast_ratio(c.surface_selected_inactive) >= 4.5,
                "the unemphasised capsule leaves its label at {:.2}:1",
                normal.contrast_ratio(c.surface_selected_inactive)
            );
        }

        let c = crate::light().colors;
        let unflipped = crate::palette::over(TextRole::Primary.resolve(&c), c.accent);
        assert!(
            unflipped.contrast_ratio(c.accent) < 4.5,
            "an Aqua label no longer needs flipping — the hook could go"
        );
    }
}
