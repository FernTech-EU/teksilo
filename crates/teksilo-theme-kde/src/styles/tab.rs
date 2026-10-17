// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze tab, in a `TabBar` or a `TabWidget`.
//!
//! The selected tab is filled with the window colour, merging into the page
//! below it, outlined on its two sides in the window colour moved toward the
//! window text by the frame contrast, and open on the side of the page. A
//! 3 px highlight bar runs along its outer edge, its two outer corners
//! rounded at the 5 px frame radius. The outline is stroked at radius 4.5,
//! so its outer edge turns through the same 5 px corner: the curve the bar
//! starts runs on into the outline and the face below the bar, rows 3 and 4
//! of a 30 px tab, before the sides straighten. An unselected tab is the window colour
//! darkened 120 %, square; hovered, the window colour moved 20 % toward the
//! hover colour. A disabled tab paints the disabled colours.
//!
//! The bar is on the edge the bar's `active_indicator` names: the top of a
//! horizontal tab and the leading side of a vertical one by default, the
//! bottom or the trailing side for `InnerEdge`; the open side is always the
//! opposite one. A tab is 30 px at Plasma's default font, through the density
//! ladder, its title 8 px inside its edges ([`BreezeTabMetrics`]). The strip
//! behind the tabs, its separator and the drop indicator of a reorder drag
//! are the shipped [`RecipeTabStyle`]'s, built with these numbers.
//!
//! Plasma's samples of a standalone tab bar fill the selected tab with the
//! window colour; the specification gives the frame background for a tab
//! bar that frames a tab widget's panel. A Teksilo tab's page is the window,
//! so the window colour is used, as measured.
//!
//! ## Where this departs from Breeze
//!
//! - **Keyboard focus** draws the 2 px focus frame (D5) flush inside the
//!   tab, as the framework's tabs keep their ring inside so a neighbouring
//!   tab cannot cover it; Breeze draws no focus frame on a tab.
//! - **The highlight bar is not lifted.** It is Breeze's highlight, as are a
//!   slider's fill and a check box's wash; the selected tab is also told by
//!   its fill, its outline and its open side.

use teksilo_canvas::{Canvas, Path, Point, Rect, SizeProposal, arc_to_cubics};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    TabBarChromeConfig, TabBarOrientation, TabIndicatorPosition, TabStyle, TabStyleConfig, Theme,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::math::hsv_darker;
use teksilo_kde_scheme::{ColorState, KdeColor, PaletteRole};
use teksilo_tokens::{Color, InputTokens, TypographyTokens};
use teksilo_widgets::primitives::{HStack, ZStack};
use teksilo_widgets::styles::{RecipeTabStyle, TabRecipe};

use crate::palette::KdePalette;
use crate::styles::chrome::{kde_mix, paint_focus_frame};
use crate::styles::metrics::{
    BreezeTabMetrics, TAB_BAR_RADIUS, TAB_HOVER_MIX, TAB_OUTLINE_RADIUS, TAB_UNSELECTED_DARKER,
    breeze_tab_metrics_for,
};

/// Breeze `TabStyle`.
///
/// [`Default`] is the Compact density and the Breeze typography; the preset
/// installs [`BreezeTabStyle::for_theme`] of the theme it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeTabStyle {
    /// The tab dimensions this style lays out and reports.
    pub metrics: BreezeTabMetrics,
    /// The recipe the bar's strip is built with.
    pub recipe: TabRecipe,
}

impl BreezeTabStyle {
    /// The style at the density of `tokens`, for `typography`.
    pub fn for_tokens(tokens: &InputTokens, typography: &TypographyTokens) -> Self {
        let metrics = breeze_tab_metrics_for(tokens, typography);
        Self {
            metrics,
            recipe: TabRecipe {
                editor_height: metrics.height,
                padding_horizontal: metrics.padding_horizontal,
                ..TabRecipe::for_tokens(tokens)
            },
        }
    }

    /// The style at the density and typography of `theme`.
    pub fn for_theme(theme: &Theme) -> Self {
        Self::for_tokens(&theme.input, &theme.typography)
    }
}

impl Default for BreezeTabStyle {
    fn default() -> Self {
        Self::for_tokens(
            &InputTokens::default(),
            &crate::typography::breeze_typography(),
        )
    }
}

impl TabStyle for BreezeTabStyle {
    fn make_body(&self, cfg: &TabStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let painter = ctx.add(BreezeTabShape {
            is_active: cfg.is_active.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_focused: cfg.is_focused.clone(),
            is_disabled: cfg.is_disabled.clone(),
            edge: bar_edge(cfg.orientation, cfg.indicator_position),
            indicator: self.metrics.indicator,
        });
        let mut row = HStack::new();
        if let Some(id) = cfg.leading {
            row = row.child(id);
        }
        row = row.child(cfg.label);
        if let Some(id) = cfg.trailing {
            row = row.child(id);
        }
        let row = ctx.add(row);
        ctx.add(ZStack::new().child(painter).child(row))
    }

    fn make_bar(&self, cfg: &TabBarChromeConfig, ctx: &mut BuildContext) -> WidgetId {
        RecipeTabStyle::new(self.recipe).make_bar(cfg, ctx)
    }

    /// [`BreezeTabMetrics::height`], 30 at Compact. The style was built at a
    /// density, so `tokens` is not consulted again.
    fn editor_height(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.height
    }

    /// [`BreezeTabMetrics::padding_horizontal`], 8 at Compact.
    fn padding_horizontal(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.padding_horizontal
    }
}

/// Which edge of a tab carries the highlight bar, before right-to-left
/// mirroring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBarEdge {
    /// The top of a horizontal tab.
    Top,
    /// The bottom of a horizontal tab.
    Bottom,
    /// The leading side of a vertical tab.
    Leading,
    /// The trailing side of a vertical tab.
    Trailing,
}

/// The edge a tab's bar is on, for the bar's orientation and indicator
/// position.
pub fn bar_edge(orientation: TabBarOrientation, position: TabIndicatorPosition) -> TabBarEdge {
    match (orientation, position) {
        (TabBarOrientation::Horizontal, TabIndicatorPosition::OuterEdge) => TabBarEdge::Top,
        (TabBarOrientation::Horizontal, TabIndicatorPosition::InnerEdge) => TabBarEdge::Bottom,
        (TabBarOrientation::Vertical, TabIndicatorPosition::OuterEdge) => TabBarEdge::Leading,
        (TabBarOrientation::Vertical, TabIndicatorPosition::InnerEdge) => TabBarEdge::Trailing,
    }
}

/// The physical side of a tab: the bar's edge with leading and trailing
/// resolved for a layout direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

fn side_of(edge: TabBarEdge, direction: LayoutDirection) -> Side {
    let rtl = direction == LayoutDirection::RightToLeft;
    match edge {
        TabBarEdge::Top => Side::Top,
        TabBarEdge::Bottom => Side::Bottom,
        TabBarEdge::Leading if rtl => Side::Right,
        TabBarEdge::Leading => Side::Left,
        TabBarEdge::Trailing if rtl => Side::Left,
        TabBarEdge::Trailing => Side::Right,
    }
}

/// A selected tab's length along the edge carrying its bar, and its depth
/// away from that edge.
fn extent(bounds: Rect, side: Side) -> (f32, f32) {
    match side {
        Side::Top | Side::Bottom => (bounds.width, bounds.height),
        Side::Left | Side::Right => (bounds.height, bounds.width),
    }
}

/// The point `u` along the bar's edge of a tab in `bounds` and `v` in from
/// that edge, on the screen.
fn to_screen(bounds: Rect, side: Side, u: f32, v: f32) -> Point {
    match side {
        Side::Top => Point::new(bounds.x + u, bounds.y + v),
        Side::Bottom => Point::new(bounds.x + u, bounds.y + bounds.height - v),
        Side::Left => Point::new(bounds.x + v, bounds.y + u),
        Side::Right => Point::new(bounds.x + bounds.width - v, bounds.y + u),
    }
}

/// A rectangle in a tab's own frame, `u` along the bar's edge and `v` in
/// from it, whose two corners on the bar's edge are rounded at `radius`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OuterRounded {
    u0: f32,
    u1: f32,
    v0: f32,
    v1: f32,
    radius: f32,
}

impl OuterRounded {
    /// The whole tab, its outer corners rounded at `radius`.
    fn tab(len: f32, depth: f32, radius: f32) -> Self {
        Self {
            u0: 0.0,
            u1: len,
            v0: 0.0,
            v1: depth,
            radius,
        }
    }
}

/// `[Hp]` The part of `shape` between `from` and `to` in from the bar's
/// edge, as a path on the screen. Its two sides follow the rounded corners
/// wherever the band crosses them, so a band cut below the bar keeps the
/// curve the bar started.
fn band_path(bounds: Rect, side: Side, shape: OuterRounded, from: f32, to: f32) -> Path {
    let OuterRounded {
        u0,
        u1,
        v0,
        v1,
        radius,
    } = shape;
    let mut path = Path::new();
    let (from, to) = (from.max(v0), to.min(v1));
    if to <= from || u1 <= u0 {
        return path;
    }
    let r = radius.min((u1 - u0) * 0.5).min(v1 - v0).max(0.0);
    let cy = v0 + r;
    // How far round a corner the side is at `v`: 90 degrees on the edge, 0
    // where the corner meets the straight side.
    let lift = |v: f32| -> f32 {
        if r <= 0.0 {
            0.0
        } else {
            ((cy - v) / r).clamp(0.0, 1.0).asin().to_degrees()
        }
    };
    let inset = |v: f32| -> f32 {
        if r <= 0.0 || v >= cy {
            0.0
        } else {
            r - (r * r - (cy - v) * (cy - v)).max(0.0).sqrt()
        }
    };
    let pt = |u: f32, v: f32| to_screen(bounds, side, u, v);
    let arc = |path: &mut Path, cx: f32, start: f32, sweep: f32| {
        let square = Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r);
        for seg in arc_to_cubics(square, start, sweep) {
            path.line_to(pt(seg.from.x, seg.from.y));
            path.cubic_to(
                pt(seg.control1.x, seg.control1.y),
                pt(seg.control2.x, seg.control2.y),
                pt(seg.to.x, seg.to.y),
            );
        }
    };
    let curved = r > 0.0 && from < cy;
    let knee = to.min(cy);
    path.move_to(pt(u0 + inset(from), from));
    if curved {
        // Down the leading corner, from `from` toward the straight side.
        arc(
            &mut path,
            u0 + r,
            180.0 + lift(from),
            lift(knee) - lift(from),
        );
    }
    path.line_to(pt(u0 + inset(to), to));
    path.line_to(pt(u1 - inset(to), to));
    if curved {
        path.line_to(pt(u1 - inset(knee), knee));
        // Up the trailing corner, back to `from`.
        arc(&mut path, u1 - r, -lift(knee), lift(knee) - lift(from));
    }
    path.line_to(pt(u1 - inset(from), from));
    path.close();
    path
}

/// The three paths of a selected tab in `bounds` with the bar `bar` thick
/// along the side `side` and a `pen` wide outline: the bar, the outline and
/// the face, the last two from the bar to the open side.
///
/// The bar is the tab's outer edge rounded at [`TAB_BAR_RADIUS`]. The
/// outline is the tab stroked at [`TAB_OUTLINE_RADIUS`]: its outer edge
/// that radius plus half the pen, its inner edge, the face, that radius
/// less half the pen, so both turn through the corner below the bar.
fn selected_paths(bounds: Rect, side: Side, bar: f32, pen: f32) -> [Path; 3] {
    let (len, depth) = extent(bounds, side);
    let bar = bar.min(depth).max(0.0);
    let bar_path = band_path(
        bounds,
        side,
        OuterRounded::tab(len, depth, TAB_BAR_RADIUS),
        0.0,
        bar,
    );
    let pen = pen.max(0.0);
    let outer = OuterRounded::tab(len, depth, TAB_OUTLINE_RADIUS + pen * 0.5);
    let outline = band_path(bounds, side, outer, bar, depth);
    let face = if pen > 0.0 {
        let inner = OuterRounded {
            u0: pen,
            u1: len - pen,
            v0: pen,
            v1: depth,
            radius: (TAB_OUTLINE_RADIUS - pen * 0.5).max(0.0),
        };
        band_path(bounds, side, inner, bar, depth)
    } else {
        outline.clone()
    };
    [bar_path, outline, face]
}

/// The colours a Breeze tab is painted in, in one paint state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeTabColors {
    /// `[measured-qt]` The selected tab's fill: the window colour.
    pub selected: Color,
    /// `[Hp]` The selected tab's outline: its fill moved toward the window
    /// text by the frame contrast.
    pub outline: Color,
    /// `[Hp]` The highlight bar: the highlight of the tab's colour group.
    pub bar: Color,
    /// `[Hp]` An unselected tab: the window colour darkened 120 %.
    pub unselected: Color,
    /// `[Hp]` A hovered unselected tab: the window colour moved 20 % toward
    /// the hover colour.
    pub hovered: Color,
}

impl BreezeTabColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference, for an enabled
    /// or a disabled tab.
    ///
    /// The window colour and the highlight are the theme's projected tokens
    /// (the disabled palette's window for a disabled tab); the outline and
    /// the hover colour come from [`KdePalette`]. Without it the outline is
    /// the border, an unselected tab the sunken surface and a hovered one the
    /// hover surface.
    pub fn resolve(
        theme: &Theme,
        window_active: bool,
        high_contrast: bool,
        disabled: bool,
    ) -> BreezeTabColors {
        let c = &theme.colors;
        let bar = if disabled {
            c.accent_disabled
        } else {
            c.accent
        };
        let Some(palette) = theme.extension::<KdePalette>() else {
            return BreezeTabColors {
                selected: c.surface_main,
                outline: c.border,
                bar,
                unselected: c.surface_sunken,
                hovered: c.surface_hover,
            };
        };
        let state = if disabled {
            ColorState::Disabled
        } else if window_active {
            ColorState::Active
        } else {
            ColorState::Inactive
        };
        let window = if disabled {
            palette.role(ColorState::Disabled, PaletteRole::Window)
        } else {
            c.surface_main
        };
        let b = palette.breeze_colors(state, high_contrast);
        BreezeTabColors {
            selected: window,
            outline: b.frame_outline,
            bar,
            unselected: hsv_darker(KdeColor::from_color(window), TAB_UNSELECTED_DARKER).to_color(),
            hovered: kde_mix(window, b.hover, TAB_HOVER_MIX),
        }
    }
}

/// The chrome of a Breeze tab, painted behind its title.
struct BreezeTabShape {
    is_active: Signal<bool>,
    is_hovered: Signal<bool>,
    is_focused: Signal<bool>,
    is_disabled: Signal<bool>,
    edge: TabBarEdge,
    indicator: f32,
}

impl std::fmt::Debug for BreezeTabShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeTabShape").finish()
    }
}

impl Widget for BreezeTabShape {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [
            &self.is_active,
            &self.is_hovered,
            &self.is_focused,
            &self.is_disabled,
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
        let theme = ctx.theme;
        let disabled = self.is_disabled.get();
        let c = BreezeTabColors::resolve(
            theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            disabled,
        );
        if self.is_active.get() {
            let side = side_of(self.edge, ctx.layout_direction);
            let pen = theme.shape.border_width;
            let [bar, outline, face] = selected_paths(bounds, side, self.indicator, pen);
            if !bar.is_empty() {
                canvas.fill_path(&bar, c.bar);
            }
            if pen > 0.0 && !outline.is_empty() {
                canvas.fill_path(&outline, c.outline);
            }
            if !face.is_empty() {
                canvas.fill_path(&face, c.selected);
            }
        } else {
            let fill = if self.is_hovered.get() && !disabled {
                c.hovered
            } else {
                c.unselected
            };
            canvas.fill_rect(bounds, fill);
        }
        if self.is_focused.get() && !disabled {
            // Flush inside the tab: the frame runs 1 px either side of a
            // rectangle 1 px in.
            let inner = Rect::new(
                bounds.x + 1.0,
                bounds.y + 1.0,
                (bounds.width - 2.0).max(0.0),
                (bounds.height - 2.0).max(0.0),
            );
            paint_focus_frame(canvas, inner, theme);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the tab announces itself.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    /// Whether two colours are within one step a channel: Qt's raster
    /// painter rounds an 8-bit blend its own way.
    fn near(a: [u8; 4], b: [u8; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
    }

    #[test]
    fn the_metrics_and_the_strip_recipe_are_breezes() {
        let s = BreezeTabStyle::default();
        let t = InputTokens::default();
        assert_eq!(s.editor_height(&t), 30.0);
        assert_eq!(s.padding_horizontal(&t), 8.0);
        assert_eq!(s.recipe.editor_height, 30.0);
        assert_eq!(s.recipe.padding_horizontal, 8.0);
        for (density, height) in [
            (TargetDensity::Comfortable, 32.0),
            (TargetDensity::Touch, 44.0),
        ] {
            let theme = crate::light().with_density(density);
            let s = BreezeTabStyle::for_theme(&theme);
            assert_eq!(s.editor_height(&theme.input), height, "{density:?}");
        }
    }

    /// The tab colours Plasma painted in Breeze Light and Breeze Dark.
    #[test]
    fn the_tab_colours_are_plasmas() {
        for (theme, selected, outline, unselected, hovered) in [
            (
                crate::light(),
                [239, 240, 241, 255],
                [198, 200, 201, 255],
                [199, 200, 201, 255],
                [203, 227, 240, 255],
            ),
            (
                crate::dark(),
                [32, 35, 38, 255],
                [76, 78, 81, 255],
                [27, 29, 32, 255],
                [38, 63, 77, 255],
            ),
        ] {
            let c = BreezeTabColors::resolve(&theme, true, false, false);
            assert_eq!(rgba8(c.selected), selected);
            assert!(near(rgba8(c.outline), outline), "{:?}", rgba8(c.outline));
            assert!(
                near(rgba8(c.unselected), unselected),
                "{:?}",
                rgba8(c.unselected)
            );
            assert!(near(rgba8(c.hovered), hovered), "{:?}", rgba8(c.hovered));
            assert_eq!(rgba8(c.bar), [61, 174, 233, 255]);
        }
    }

    #[test]
    fn the_bar_follows_the_orientation_the_position_and_the_direction() {
        use LayoutDirection::{LeftToRight, RightToLeft};
        use TabBarOrientation::{Horizontal, Vertical};
        use TabIndicatorPosition::{InnerEdge, OuterEdge};
        assert_eq!(bar_edge(Horizontal, OuterEdge), TabBarEdge::Top);
        assert_eq!(bar_edge(Horizontal, InnerEdge), TabBarEdge::Bottom);
        assert_eq!(bar_edge(Vertical, OuterEdge), TabBarEdge::Leading);
        assert_eq!(bar_edge(Vertical, InnerEdge), TabBarEdge::Trailing);
        assert_eq!(side_of(TabBarEdge::Leading, LeftToRight), Side::Left);
        assert_eq!(side_of(TabBarEdge::Leading, RightToLeft), Side::Right);
        assert_eq!(side_of(TabBarEdge::Trailing, RightToLeft), Side::Left);
        assert_eq!(side_of(TabBarEdge::Top, RightToLeft), Side::Top);
    }

    fn inside(path: &Path, x: f32, y: f32) -> bool {
        path.contains_point(Point::new(x, y), teksilo_canvas::FillRule::Winding, 0.01)
    }

    fn near_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) -> bool {
        (r.x - x).abs() < 0.01
            && (r.y - y).abs() < 0.01
            && (r.width - w).abs() < 0.01
            && (r.height - h).abs() < 0.01
    }

    /// A selected 80 x 30 tab: the bar along the top 3 px, the outline down
    /// the two sides below it, the bottom open, as Plasma's sample.
    #[test]
    fn a_selected_tab_is_a_bar_two_sides_and_an_open_edge() {
        let b = Rect::new(0.0, 0.0, 80.0, 30.0);
        let [bar, outline, face] = selected_paths(b, Side::Top, 3.0, 1.0);
        // The bar's rounded corners keep it clear of the tab's two sides:
        // at its inner edge, 3 px down a 5 px corner, it is 5 - sqrt(21)
        // in from each.
        let corner = 5.0 - 21.0_f32.sqrt();
        let bb = bar.exact_bounds(0.01);
        assert!(
            near_rect(bb, corner, 0.0, 80.0 - 2.0 * corner, 3.0),
            "{bb:?}"
        );
        assert!(near_rect(outline.bounds(), 0.0, 3.0, 80.0, 27.0));
        assert!(near_rect(face.bounds(), 1.0, 3.0, 78.0, 27.0));
        assert!(inside(&bar, 40.0, 1.5));
        assert!(!inside(&bar, 40.0, 3.5));
        // The outline is the 1 px either side of the face, down to the
        // open bottom.
        for y in [15.0, 29.5] {
            assert!(inside(&outline, 0.5, y) && !inside(&face, 0.5, y));
            assert!(inside(&outline, 79.5, y) && !inside(&face, 79.5, y));
            assert!(inside(&face, 40.0, y));
        }
        let [bar, _, face] = selected_paths(Rect::new(0.0, 0.0, 30.0, 80.0), Side::Right, 3.0, 1.0);
        let bb = bar.exact_bounds(0.01);
        assert!(
            near_rect(bb, 27.0, corner, 3.0, 80.0 - 2.0 * corner),
            "{bb:?}"
        );
        assert!(near_rect(face.bounds(), 0.0, 1.0, 27.0, 78.0));
    }

    /// The corner the bar starts goes on turning below it: in rows 3 and 4
    /// the outline's outer edge and the face's edge are still curving, as
    /// in Plasma's sample (column 0 is blended there and only reaches the
    /// outline colour at row 5), rather than stepping to a square body.
    #[test]
    fn the_outline_and_the_face_follow_the_bars_corner() {
        let b = Rect::new(0.0, 0.0, 80.0, 30.0);
        let [bar, outline, face] = selected_paths(b, Side::Top, 3.0, 1.0);
        // The bar's rounded corner.
        assert!(!inside(&bar, 0.3, 0.3));
        assert!(inside(&bar, 2.5, 2.5));
        // Row 3: the outline starts about 0.4 px in, not at the edge.
        assert!(!inside(&outline, 0.1, 3.1));
        assert!(inside(&outline, 0.8, 3.1));
        assert!(!inside(&outline, 79.9, 3.1));
        // Row 4: the edge has nearly reached the side, and the face is still
        // inset more than the straight 1 px pen.
        assert!(inside(&outline, 0.5, 4.5));
        assert!(!inside(&face, 0.9, 4.5));
        assert!(inside(&face, 1.5, 4.5));
        assert!(inside(&face, 1.1, 15.0));
        // Every orientation and direction carries the same corner.
        for (bounds, side, outside) in [
            (Rect::new(0.0, 0.0, 80.0, 30.0), Side::Bottom, (0.1, 26.9)),
            (Rect::new(0.0, 0.0, 30.0, 80.0), Side::Left, (3.1, 0.1)),
            (Rect::new(0.0, 0.0, 30.0, 80.0), Side::Right, (26.9, 79.9)),
        ] {
            let [_, outline, _] = selected_paths(bounds, side, 3.0, 1.0);
            assert!(!inside(&outline, outside.0, outside.1), "{side:?}");
        }
        // At the default 1 px pen the outline's outer edge is the bar's
        // radius.
        assert_eq!(TAB_OUTLINE_RADIUS + 0.5, TAB_BAR_RADIUS);
    }

    #[test]
    fn a_band_without_a_radius_is_a_rectangle() {
        let b = Rect::new(10.0, 20.0, 40.0, 30.0);
        let shape = OuterRounded::tab(40.0, 30.0, 0.0);
        let p = band_path(b, Side::Top, shape, 3.0, 30.0);
        assert!(near_rect(p.bounds(), 10.0, 23.0, 40.0, 27.0));
        assert!(inside(&p, 10.1, 23.1));
        assert!(band_path(b, Side::Top, shape, 30.0, 30.0).is_empty());
    }

    #[test]
    fn a_disabled_tab_paints_the_disabled_colours() {
        let theme = crate::light();
        let c = BreezeTabColors::resolve(&theme, true, false, true);
        assert_eq!(c.bar, theme.colors.accent_disabled);
        let palette = theme.extension::<KdePalette>().unwrap();
        assert_eq!(
            c.selected,
            palette.role(ColorState::Disabled, PaletteRole::Window)
        );
    }

    #[test]
    fn the_tab_paints_from_the_roles_without_the_palette() {
        let mut bare = crate::light();
        bare.extensions.remove::<KdePalette>();
        let c = BreezeTabColors::resolve(&bare, true, false, false);
        assert_eq!(c.outline, bare.colors.border);
        assert_eq!(c.unselected, bare.colors.surface_sunken);
    }
}
