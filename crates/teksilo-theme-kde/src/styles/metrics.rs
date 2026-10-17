// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The dimensions the Breeze widget styles are built with, at a density.
//!
//! Every height here starts from a Breeze rule in [`crate::geometry`]: the
//! text-driven ones from the theme's body line box, the icon-driven ones
//! from an icon size. A height of 24 or more is a target, so it goes through
//! the density ladder's `dp(.., Target)`, which leaves it alone at Compact
//! and raises it to the density's target size above it (`[unpublished]`:
//! KDE publishes no touch ladder, so the ladder is Teksilo's generic one).
//! A gap or padding goes through `spacing`. At Compact every value is
//! Breeze's own.

use teksilo_core::styles::density::{dp, spacing};
use teksilo_tokens::{InputTokens, TargetRole, TypographyTokens};

use crate::geometry::{
    BUTTON_MARGIN_WIDTH, BreezeGeometry, CHECKBOX_FOCUS_MARGIN, CHECKBOX_ITEM_SPACING,
    CHECKBOX_SIZE, FRAME_WIDTH, LARGE_ICON_SIZE, LINE_EDIT_FRAME_WIDTH, MEDIUM_ICON_SIZE,
    TOOL_BUTTON_MARGIN_WIDTH,
};
use crate::shape::BREEZE_PEN_WIDTH;

/// `[M]` The square a check box or radio button indicator is painted in.
pub const INDICATOR_SIZE: f32 = CHECKBOX_SIZE;
/// `[Hp]` How far the box (or the radio disc) lies inside the indicator: the
/// indicator's focus margin.
pub const INDICATOR_BOX_INSET: f32 = CHECKBOX_FOCUS_MARGIN;
/// `[Hp]` The side of the box as stroked: the indicator inset by 2, then by
/// half the pen. 15.
pub const INDICATOR_BOX_STROKED: f32 =
    INDICATOR_SIZE - 2.0 * INDICATOR_BOX_INSET - BREEZE_PEN_WIDTH;
/// `[M]` Where a check box's or radio button's label starts, from the
/// indicator's leading edge: the indicator and Breeze's 4 px label spacing.
pub const INDICATOR_LABEL_X: f32 = CHECKBOX_SIZE + CHECKBOX_ITEM_SPACING;
/// `[Hp]` The width of the pen a check mark is drawn with.
pub const CHECK_MARK_PEN: f32 = 2.0;
/// `[Hp]` The side of each of the three squares of a partial check box.
pub const PARTIAL_DOT_SIZE: f32 = 2.0;
/// `[Hp]` The distance between the centres of the partial squares.
pub const PARTIAL_DOT_SPACING: f32 = 4.0;
/// `[Hp]` The radius of a selected radio button's dot: `floor((18 - 12) /
/// 2)`, a 6 px dot.
pub const RADIO_DOT_RADIUS: f32 = 3.0;
/// `[Hp]` How much of the highlight fills a checked box or a selected disc
/// over the button colour.
pub const INDICATOR_CHECKED_WASH: f32 = 0.3;
/// `[Hp]` How much a pressed indicator's button colour is darkened, in Qt's
/// percent (d.11 of the specification).
pub const INDICATOR_PRESSED_DARKER: i32 = 110;

/// `[K]` The height of a switch: Kirigami's grid unit.
pub const SWITCH_HEIGHT: f32 = 18.0;
/// `[Qd]` The width of a switch: twice its height.
pub const SWITCH_WIDTH: f32 = 2.0 * SWITCH_HEIGHT;
/// `[Qd]` How far the track lies inside the switch above and below: `floor(18
/// / 6)`.
pub const SWITCH_TRACK_INSET: f32 = 3.0;
/// `[Qd]` The track's radius: `round(12 / 2)`.
pub const SWITCH_TRACK_RADIUS: f32 = 6.0;
/// `[Qd]` The handle's radius: `floor(18 / 2)`, a circle of the switch's
/// full height.
pub const SWITCH_HANDLE_RADIUS: f32 = 9.0;
/// `[Qd]` How much of the button text the track is filled with over the
/// button colour.
pub const SWITCH_TRACK_TEXT: f64 = 0.1;
/// `[Qd]` The alpha of the highlight that fills a checked track up to the
/// handle.
pub const SWITCH_CHECKED_ALPHA: f32 = 0.5;
/// `[qt]` The speed Qt's smoothed animation moves the handle at, at most, in
/// pixels a second: a full travel of 18 px takes 90 ms rather than the
/// 100 ms duration.
pub const SWITCH_HANDLE_VELOCITY: f32 = 200.0;

// The switch is twice as wide as tall, its track centred inside it.
const _: () = assert!(SWITCH_WIDTH == 2.0 * SWITCH_HEIGHT);
const _: () = assert!(SWITCH_TRACK_RADIUS * 2.0 == SWITCH_HEIGHT - 2.0 * SWITCH_TRACK_INSET);
const _: () = assert!(SWITCH_HANDLE_RADIUS * 2.0 == SWITCH_HEIGHT);
// The box is the 15 px square of the specification.
const _: () = assert!(INDICATOR_BOX_STROKED == 15.0);

/// The width the body of a check box or radio button is laid out at, when
/// the widget puts `label_gap` between it and its label: the label then
/// starts at [`INDICATOR_LABEL_X`], where Breeze puts it.
///
/// With the widget's 6 px gap that is 18, the indicator less the trailing
/// 2 px of its focus margin; the box itself ends at 18, so it always lies in
/// the node, and only the focus frame reaches past it. Never narrower than
/// the box's far edge, so a larger gap moves the label rather than the box
/// out of the node.
pub fn indicator_body_width(label_gap: f32) -> f32 {
    (INDICATOR_LABEL_X - label_gap).max(INDICATOR_SIZE - INDICATOR_BOX_INSET)
}

/// The dimensions of a line edit, the frame a `TextInput` is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeFieldMetrics {
    /// `[S]` `[qt]` The field's height, at least: Qt's inner height
    /// `max(H, 14) + 2` plus the 6 px frame above and below, through the
    /// density ladder. 32 at Compact for the Breeze typography.
    pub height: f32,
    /// `[M]` The space between the frame and the text on the leading and
    /// trailing sides: the 6 px line-edit frame, scaled by the density's
    /// spacing.
    pub padding_horizontal: f32,
}

/// The line-edit dimensions at the density of `tokens`, for `typography`.
pub fn breeze_field_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeFieldMetrics {
    let g = geometry_of(typography);
    BreezeFieldMetrics {
        height: dp(g.line_edit_height, TargetRole::Target, tokens),
        padding_horizontal: spacing(LINE_EDIT_FRAME_WIDTH, tokens),
    }
}

/// [`breeze_field_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_field_metrics() -> BreezeFieldMetrics {
    breeze_field_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// The body line box `H` of `typography`, in whole pixels: the height every
/// text-driven Breeze control is derived from. 18 for the Breeze typography.
pub fn body_line_height(typography: &TypographyTokens) -> f32 {
    (typography.body.size * typography.body.line_height).round()
}

/// The Breeze geometry of `typography`'s body line box.
pub fn geometry_of(typography: &TypographyTokens) -> BreezeGeometry {
    BreezeGeometry::for_line_height(body_line_height(typography))
}

/// The dimensions of a push button, a default button and a flat push button,
/// which share them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeButtonMetrics {
    /// `[S]` The button's height, at least: `H + 16`, through the density
    /// ladder. 34 at Compact for the Breeze typography.
    pub height: f32,
    /// `[S]` The button's width, at least: 84, Breeze's 80 plus the 2 px
    /// frame margin on each side.
    pub min_width: f32,
    /// `[S]` The space between the frame and the label on the leading and
    /// trailing sides: the 6 px button margin and the 2 px frame margin,
    /// scaled by the density's spacing.
    pub padding_horizontal: f32,
    /// `[S]` The same above and below the label. It is part of the height
    /// rule rather than a gap, so it does not scale: the height floor rises
    /// with the density instead.
    pub padding_vertical: f32,
}

/// The button dimensions at the density of `tokens`, for `typography`.
pub fn breeze_button_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeButtonMetrics {
    let g = geometry_of(typography);
    BreezeButtonMetrics {
        height: dp(g.push_button_height, TargetRole::Target, tokens),
        min_width: g.push_button_min_width,
        padding_horizontal: spacing(BUTTON_MARGIN_WIDTH + FRAME_WIDTH, tokens),
        padding_vertical: BUTTON_MARGIN_WIDTH + FRAME_WIDTH,
    }
}

/// [`breeze_button_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_button_metrics() -> BreezeButtonMetrics {
    breeze_button_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// The square an `IconButton` is drawn in at each of its five sizes: Breeze's
/// auto-raise tool button, an icon with the 6 px tool-button margin around
/// it, through the density ladder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeToolButtonSizes {
    /// `[unpublished]` Breeze has no tool button smaller than the one around
    /// a 16 px icon; the compact rung is the density's target size, 24 at
    /// Compact.
    pub compact: f32,
    /// `[S]` A 16 px icon: 28 at Compact.
    pub default: f32,
    /// `[S]` A 22 px icon, the toolbar size: 34 at Compact.
    pub toolbar: f32,
    /// `[derived]` Breeze's rule at Kirigami's 32 px medium icon: 44.
    pub large: f32,
    /// `[derived]` Breeze's rule at Kirigami's 48 px large icon: 60.
    pub hero: f32,
}

/// The tool-button squares at the density of `tokens`.
pub fn breeze_tool_button_sizes_for(tokens: &InputTokens) -> BreezeToolButtonSizes {
    // Icon-driven, so the font does not enter: the default geometry's
    // tool-button heights are those of any font.
    let g = BreezeGeometry::breeze_default();
    let around = |icon: f32| icon + 2.0 * TOOL_BUTTON_MARGIN_WIDTH;
    let target = |h: f32| dp(h, TargetRole::Target, tokens);
    BreezeToolButtonSizes {
        compact: target(tokens.min_target_conformance),
        default: target(g.tool_button_height),
        toolbar: target(g.tool_button_height_medium),
        large: target(around(MEDIUM_ICON_SIZE)),
        hero: target(around(LARGE_ICON_SIZE)),
    }
}

/// [`breeze_tool_button_sizes_for`] at the Compact density.
pub fn breeze_tool_button_sizes() -> BreezeToolButtonSizes {
    breeze_tool_button_sizes_for(&InputTokens::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    fn at(density: TargetDensity) -> InputTokens {
        InputTokens::for_density(density)
    }

    #[test]
    fn the_compact_button_is_breezes() {
        let m = breeze_button_metrics();
        assert_eq!(m.height, 34.0);
        assert_eq!(m.min_width, 84.0);
        assert_eq!(m.padding_horizontal, 8.0);
        assert_eq!(m.padding_vertical, 8.0);
        // The label's 18 px line box and the two gutters make the height.
        let h = body_line_height(&crate::typography::breeze_typography());
        assert_eq!(h, 18.0);
        assert_eq!(h + 2.0 * m.padding_vertical, m.height);
        // The width rule: max(text + 12, 80) + 4 is text + 16 or 84.
        assert_eq!(2.0 * m.padding_horizontal, 12.0 + 2.0 * FRAME_WIDTH);
    }

    #[test]
    fn the_button_rises_with_the_density_and_never_falls() {
        let t = crate::typography::breeze_typography();
        let compact = breeze_button_metrics_for(&at(TargetDensity::Compact), &t);
        let comfortable = breeze_button_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_button_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(comfortable.height, 34.0, "34 already clears 32");
        assert_eq!(touch.height, 44.0);
        assert!((comfortable.padding_horizontal - 9.2).abs() < 1e-4);
        assert!((touch.padding_horizontal - 10.4).abs() < 1e-4);
        for m in [compact, comfortable, touch] {
            assert_eq!(m.min_width, 84.0);
            assert_eq!(m.padding_vertical, 8.0);
        }
    }

    #[test]
    fn a_larger_body_font_makes_a_taller_button() {
        let mut t = crate::typography::breeze_typography();
        // Plasma's 12 pt: a 16 px face on a 22 px line, which Qt makes 38.
        t.body.size = 16.0;
        t.body.line_height = 22.0 / 16.0;
        let m = breeze_button_metrics_for(&InputTokens::default(), &t);
        assert_eq!(m.height, 38.0);
    }

    #[test]
    fn the_tool_buttons_are_breezes_and_grow_with_the_density() {
        let s = breeze_tool_button_sizes();
        assert_eq!(
            (s.compact, s.default, s.toolbar, s.large, s.hero),
            (24.0, 28.0, 34.0, 44.0, 60.0)
        );
        let comfortable = breeze_tool_button_sizes_for(&at(TargetDensity::Comfortable));
        assert_eq!(
            (
                comfortable.compact,
                comfortable.default,
                comfortable.toolbar
            ),
            (32.0, 32.0, 34.0)
        );
        let touch = breeze_tool_button_sizes_for(&at(TargetDensity::Touch));
        assert_eq!(
            (
                touch.compact,
                touch.default,
                touch.toolbar,
                touch.large,
                touch.hero
            ),
            (44.0, 44.0, 44.0, 44.0, 60.0)
        );
    }

    #[test]
    fn the_field_is_breezes_line_edit_and_grows_with_the_density() {
        let m = breeze_field_metrics();
        assert_eq!(m.height, 32.0);
        assert_eq!(m.padding_horizontal, 6.0);
        let t = crate::typography::breeze_typography();
        let comfortable = breeze_field_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_field_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(comfortable.height, 32.0, "32 already clears 32");
        assert_eq!(touch.height, 44.0);
        assert!((touch.padding_horizontal - 7.8).abs() < 1e-4);
    }

    #[test]
    fn the_indicator_puts_the_label_where_breeze_does() {
        assert_eq!(INDICATOR_LABEL_X, 24.0);
        // The widgets' 6 px gap: an 18 px body.
        let gap = teksilo_widgets::styles::recipe_checkbox_style::CHECKBOX_LABEL_GAP;
        assert_eq!(
            gap,
            teksilo_widgets::styles::recipe_radio_style::RADIO_LABEL_GAP
        );
        assert_eq!(indicator_body_width(gap), 18.0);
        assert_eq!(indicator_body_width(gap) + gap, INDICATOR_LABEL_X);
        // The box ends 2 px inside the indicator, so it stays in the body.
        assert_eq!(INDICATOR_SIZE - INDICATOR_BOX_INSET, 18.0);
        assert_eq!(
            indicator_body_width(12.0),
            18.0,
            "never narrower than the box"
        );
        assert_eq!(indicator_body_width(0.0), 24.0);
    }

    #[test]
    fn every_tool_button_clears_the_conformance_floor_without_a_box() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let tokens = at(density);
            let s = breeze_tool_button_sizes_for(&tokens);
            for size in [s.compact, s.default, s.toolbar, s.large, s.hero] {
                assert!(size >= tokens.min_target_conformance, "{density:?}: {size}");
            }
            assert!(s.compact <= s.default && s.default <= s.toolbar);
            assert!(s.toolbar <= s.large && s.large <= s.hero);
        }
    }
}
