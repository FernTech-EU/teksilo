// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `SplitterStyle` impl — the IntUI divider-handle look.
//!
//! Reproduces the old `SplitView` chrome: a thin static line at the
//! gutter's center (so the divider never disappears), with a thicker
//! focus-color line that cross-fades in on hover-dwell and snaps to full
//! strength on keyboard focus or drag.
//!
//! The visual body is a small private leaf (`SplitterHandleBody`) — same
//! "leaf body" choice as `RecipeSliderStyle`. Custom `SplitterStyle`
//! impls compose their own body instead.
//!
//! # The resting line is unconditional, and that is load-bearing
//!
//! The divider a user has to aim at is painted whether or not anything is
//! hovering it — `fill_rect(line_rect(line_thickness), colors.border)` runs
//! before any state is consulted. The 400 ms hover dwell only fades in a
//! *second*, focus-coloured line that keyboard focus and an active drag both
//! show instantly.
//!
//! So the splitter owes a touch user no reveal: there is nothing hidden behind
//! hover to reveal. What it owed was a **grab**, and that is
//! [`SplitterHandle`](crate::splitter)'s `Widget::hit_outset` — a hit-only
//! widening of the 6 dp gutter to the density's target size that leaves this
//! paint untouched at every density. The cursor change over the gutter is a
//! mouse affordance and stays exactly as it was.
//!
//! An earlier reading of the hover census had this style hiding the divider
//! until hover and owing a touch reveal; the source says otherwise, and
//! `the_resting_line_is_painted_with_no_interaction_at_all` below keeps it
//! that way.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SplitterStyle, SplitterStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{InputTokens, Orientation};

/// Thickness of the always-present resting divider line, in dp.
pub const SPLITTER_DIVIDER_LINE_THICKNESS: f32 = 1.0;

/// Fraction of the hover-dwell animation spent fully transparent before
/// the focus line fades in (300 ms hold within the 400 ms dwell). Maps
/// the handle's linear `hover_progress` 0→1 onto a delayed alpha ramp.
const HOVER_DWELL_DELAY_FRAC: f32 = 0.75;

/// Configurable dimensions for [`RecipeSplitterStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitterRecipe {
    /// Thickness of the always-present resting divider line, in dp.
    pub divider_line_thickness: f32,
}

impl SplitterRecipe {
    /// This recipe's dimensions resolved against a density's [`InputTokens`].
    ///
    /// [`Default`] is `for_tokens(&InputTokens::default())` — the Compact
    /// ladder — so the shipped values below are the Compact column by
    /// construction and cannot drift from it.
    ///
    /// Every dimension this recipe carries is a decoration (a corner radius, a
    /// hairline, a glyph metric), so the parameter is unused: the density
    /// ladder never moves any of them. It is taken all the same, so every
    /// recipe is constructed the same way at its widget's build site.
    pub fn for_tokens(_tokens: &InputTokens) -> Self {
        Self {
            divider_line_thickness: SPLITTER_DIVIDER_LINE_THICKNESS,
        }
    }
}

impl Default for SplitterRecipe {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

/// Default `SplitterStyle` shipped with Teksilo. Colors come from
/// `theme.colors.{border, focus_ring}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSplitterStyle {
    pub recipe: SplitterRecipe,
}

impl RecipeSplitterStyle {
    pub fn new(recipe: SplitterRecipe) -> Self {
        Self { recipe }
    }

    /// This style with every dimension resolved against a density's
    /// [`InputTokens`], as `RecipeSplitterStyle::for_tokens(&ctx.theme().input)` at
    /// the widget's own build site.
    ///
    /// [`Default`] is the `TargetDensity::Compact` projection, so a Compact
    /// tree gets exactly the values this module documents.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            recipe: SplitterRecipe::for_tokens(tokens),
        }
    }
}

impl SplitterStyle for RecipeSplitterStyle {
    fn make_handle(&self, cfg: &SplitterStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(SplitterHandleBody {
            orientation: cfg.orientation,
            is_dragging: cfg.is_dragging.clone(),
            is_disabled: cfg.is_disabled.clone(),
            focus_origin: cfg.focus_origin.clone(),
            hover_progress: cfg.hover_progress.clone(),
            recipe: self.recipe,
        })
    }
}

/// Internal leaf that paints the divider line + focus indicator.
struct SplitterHandleBody {
    orientation: Orientation,
    is_dragging: Signal<bool>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    hover_progress: Signal<f32>,
    recipe: SplitterRecipe,
}

impl std::fmt::Debug for SplitterHandleBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitterHandleBody")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for SplitterHandleBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_dragging
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.hover_progress
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // The host handle assigns exact bounds; just resolve the proposal.
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
        let colors = &ctx.theme.colors;
        let enabled = !self.is_disabled.get();

        let line_thickness = self.recipe.divider_line_thickness.max(1.0);
        let focus_thickness = (line_thickness * 3.0).max(line_thickness + 2.0);

        let line_rect = |thickness: f32| match self.orientation {
            // Horizontal splitter → vertical handle bar (line runs down).
            Orientation::Horizontal => Rect::new(
                bounds.x + (bounds.width - thickness) / 2.0,
                bounds.y,
                thickness,
                bounds.height,
            ),
            Orientation::Vertical => Rect::new(
                bounds.x,
                bounds.y + (bounds.height - thickness) / 2.0,
                bounds.width,
                thickness,
            ),
        };

        // Resting line — always present.
        canvas.fill_rect(line_rect(line_thickness), colors.border);

        // Focus indicator: instant on keyboard focus / drag, hover-dwell
        // fade-in otherwise.
        let focus_alpha = if !enabled {
            0.0
        } else if self.focus_origin.get() == Some(FocusOrigin::Keyboard) || self.is_dragging.get() {
            1.0
        } else {
            let p = self.hover_progress.get();
            ((p - HOVER_DWELL_DELAY_FRAC) / (1.0 - HOVER_DWELL_DELAY_FRAC)).clamp(0.0, 1.0)
        };

        if focus_alpha > 0.0 {
            canvas.fill_rect(
                line_rect(focus_thickness),
                colors.focus_ring.with_alpha(focus_alpha),
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the SplitterHandle owns the Role::Splitter node.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use teksilo_canvas::{Size, SizeProposal};
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_tokens::{Orientation, TargetDensity};

    use crate::splitter::{PaneDescriptor, SPLITTER_GUTTER_THICKNESS, Splitter, SplitterModel};

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    fn splitter_tree(density: TargetDensity) -> (WidgetTree, teksilo_core::styles::Theme) {
        let theme = teksilo_core::presets::intui::light().with_density(density);
        let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
        let model = SplitterModel::from_panes(
            vec![
                PaneDescriptor::new()
                    .size(avail * 0.5)
                    .min_size(0.0)
                    .stretch(0.0),
                PaneDescriptor::new()
                    .size(avail * 0.5)
                    .min_size(0.0)
                    .stretch(0.0),
            ],
            Orientation::Horizontal,
        );
        let mut tree = WidgetTree::new()
            .with_theme(theme.clone())
            .with_text_backend(Rc::new(std::cell::RefCell::new(
                teksilo_canvas::MockTextBackend::new(),
            )));
        tree.add(
            Splitter::new(model)
                .pane(FixedLeaf(100.0, 40.0))
                .pane(FixedLeaf(100.0, 40.0)),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        (tree, theme)
    }

    /// The correction this package was told not to undo: the divider is drawn
    /// with **no** pointer anywhere near it, so touch is owed a grab and not a
    /// reveal. Asserted against the rendered frame rather than against the
    /// source, so a future refactor that moves the fill behind a hover check
    /// fails here.
    #[test]
    fn the_resting_line_is_painted_with_no_interaction_at_all() {
        let (mut tree, theme) = splitter_tree(TargetDensity::Compact);
        let frame = tree.render();
        let border = theme.colors.border;
        let painted = frame.decorations.iter().any(|d| {
            let [x, _y, w, h] = d.rect;
            // The gutter's own hairline: one logical pixel wide, tall, and
            // sitting at the divider between the two 197 dp panes.
            (w - super::SPLITTER_DIVIDER_LINE_THICKNESS).abs() < 0.01
                && h > 100.0
                && x > 190.0
                && x < 210.0
                && (d.color[0] - border.r()).abs() < 0.001
                && (d.color[1] - border.g()).abs() < 0.001
                && (d.color[2] - border.b()).abs() < 0.001
                && d.color[3] > 0.0
        });
        assert!(
            painted,
            "the resting divider must be painted with nothing hovering it"
        );
    }

    /// …and it stays exactly one hairline at every density: the divider is a
    /// decoration, so the density ladder never grows it. What grows is the
    /// hit band, which paints nothing.
    #[test]
    fn the_resting_line_is_a_hairline_at_every_density() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let recipe = super::SplitterRecipe::for_tokens(
                &teksilo_tokens::InputTokens::for_density(density),
            );
            assert_eq!(
                recipe.divider_line_thickness,
                super::SPLITTER_DIVIDER_LINE_THICKNESS,
                "{density:?}"
            );
        }
    }
}
