// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Breeze metrics for the widgets whose stock `Recipe*Style` already paints
//! the right shape: they only need Breeze's numbers.
//!
//! Every style here is the shipped Teksilo implementation built with a
//! Breeze recipe, not a reimplementation, the way the macOS preset's
//! `styles::metrics` builds its own. Each has a `_for(&InputTokens)` factory,
//! which [`crate::light`] and the other constructors install and the
//! density projection reruns, and a zero-argument twin at the Compact
//! density.
//!
//! Three Breeze rules give almost every value:
//!
//! - **One radius.** A frame in the page (a group box, a generic frame, a
//!   button) and a window (a menu, a tooltip, a dialog) round at the 5 px
//!   frame radius, [`BREEZE_CONTROL_RADIUS`] and [`BREEZE_POPUP_RADIUS`],
//!   outlined in the 1 px pen, [`BREEZE_PEN_WIDTH`] (spec k.1, k.5.19,
//!   k.5.20, k.6, k.7).
//! - **Three layout gaps.** A window's content lies 10 px inside it, a
//!   nested group's 6 px inside its frame, and siblings stand 6 px apart
//!   ([`LAYOUT_TOP_LEVEL_MARGIN`], [`LAYOUT_CHILD_MARGIN`],
//!   [`LAYOUT_SPACING`], spec k.1 and k.7).
//! - **A popup list is a menu.** The rows of a search field's suggestion
//!   list lie where Breeze lays out the items of a menu without icons
//!   (spec k.5.13).
//!
//! A gap goes through the density's `spacing`, as every Breeze gap does in
//! [`crate::styles::metrics`]; a radius or a pen is painted and does not
//! scale. `[unpublished]` KDE publishes no touch ladder, so the ladder is
//! the framework's generic one.
//!
//! ## What is left at the stock recipe
//!
//! A field is set only when Breeze has a number for it **and** the widget
//! renders it. The rest keep the stock recipe's value, for one of four
//! reasons.
//!
//! - **The widget does not render it.** `engineering/docs/density-projection-gaps.md`
//!   lists the projected fields a widget composes from the recipe module's
//!   constant instead of the style, and a crate test holds every factory
//!   here to that list: a value written there would never reach the
//!   screen. So the segmented control's height and paddings, the
//!   calendar's cell size, header height, outer padding, section gap and
//!   cell gap, the search field's row height, panel padding and
//!   input-to-panel gap, the toast's title and action gaps, the banner's
//!   title gap and every avatar size stay the recipe's. The same holds for
//!   the fields nothing reads at all, projected or not: the banner's and
//!   toast's glyph sizes, the search field's glyph, glyph slot and two
//!   radii, the calendar's navigation icon and arrow radius, and the
//!   avatar's default ring width.
//! - **Breeze has no counterpart.** The specification records no Breeze
//!   snackbar, toast, badge, avatar or segmented control, and no rule for a
//!   dialog's minimum width, a toast's or a snackbar's padding, a badge's padding, the
//!   avatar's shape or the drag-insertion line of a list. Those keep the
//!   framework's values rather than invented ones.
//! - **Breeze's value would cost a target.** The calendar's header keeps
//!   the recipe's gap between its arrows, narrower than Breeze's 6 px layout
//!   spacing, which at the Touch density would leave the month title
//!   narrower than WCAG 2.2's 24 px target.
//! - **Breeze's rule gives the stock value.** The calendar's navigation
//!   arrow is a flat tool button around its chevron, the chevron plus 6 px
//!   on each side ([`TOOL_BUTTON_MARGIN_WIDTH`]): around the 12 px chevron
//!   the widget draws, that is the recipe's own 24.
//!
//! A card keeps the recipe's elevation too: Breeze casts no shadow from a
//! frame in the page (spec k.5.19), so an app that asks for an `Elevated`
//! card is asking for something Breeze does not draw, and gets the
//! framework's, in Breeze's shadow colours.

use teksilo_core::styles::density::spacing;
use teksilo_tokens::InputTokens;
use teksilo_widgets::styles::{
    AvatarRecipe, BadgeRecipe, BannerRecipe, CalendarRecipe, CardRecipe, DialogRecipe, LinkRecipe,
    PanelRecipe, RecipeAvatarStyle, RecipeBadgeStyle, RecipeBannerStyle, RecipeCalendarStyle,
    RecipeCardStyle, RecipeDialogStyle, RecipeLinkStyle, RecipeListContainerStyle,
    RecipePanelStyle, RecipeSearchFieldStyle, RecipeSegmentedControlStyle, RecipeSnackbarStyle,
    RecipeSplitterStyle, RecipeToastStyle, SearchFieldRecipe, SegmentedControlRecipe,
    SnackbarRecipe, SplitterRecipe, ToastRecipe,
};

#[cfg(doc)]
use crate::geometry::TOOL_BUTTON_MARGIN_WIDTH;
use crate::geometry::{
    LAYOUT_CHILD_MARGIN, LAYOUT_SPACING, LAYOUT_TOP_LEVEL_MARGIN, MENU_ITEM_EXTRA_LEFT_MARGIN,
    MENU_ITEM_MARGIN, MENU_ITEM_TEXT_LEFT_MARGIN, SPLITTER_WIDTH,
};
use crate::shape::{
    BREEZE_CONTROL_RADIUS, BREEZE_FOCUS_RING_WIDTH, BREEZE_PEN_WIDTH, BREEZE_POPUP_RADIUS,
};

/// `[derived]` How far a suggestion's text lies inside its row, on either
/// side: a menu item's contents start 4 px inside the item and 8 px more on
/// the leading side, and 4 px more again in a menu without icons (spec
/// k.5.13). Teksilo pads both sides alike. 16 at Compact.
pub const SUGGESTION_TEXT_INSET: f32 =
    MENU_ITEM_MARGIN + MENU_ITEM_TEXT_LEFT_MARGIN + MENU_ITEM_EXTRA_LEFT_MARGIN;

const _: () = assert!(SUGGESTION_TEXT_INSET == 16.0);

/// `[derived]` The radius of the centre line of a link's focus ring: the
/// ring's inner edge rounds at the frame radius and its outer edge at the
/// frame radius plus the ring's width (spec k.5.8: 5 and 7), so the line
/// halfway between them rounds at the frame radius plus half the width.
/// 6 px.
pub const LINK_FOCUS_RING_RADIUS: f32 = BREEZE_CONTROL_RADIUS + BREEZE_FOCUS_RING_WIDTH / 2.0;

const _: () = assert!(LINK_FOCUS_RING_RADIUS == 6.0);

// ── Frames in the page ──────────────────────────────────────────────────

/// `Card`: Breeze's group box (spec k.5.19), a frame at the frame radius
/// whose contents Qt lays out with the nested-layout margin.
pub fn breeze_card_style_for(tokens: &InputTokens) -> RecipeCardStyle {
    RecipeCardStyle::new(CardRecipe {
        // `[derived]` A framed group's layout takes the child margin (k.1,
        // k.4 "Dialog margin ... child margin 6").
        padding: spacing(LAYOUT_CHILD_MARGIN, tokens),
        // `[M]` The frame radius.
        corner_radius: BREEZE_CONTROL_RADIUS,
        // `[M]` The pen of every frame.
        border_width: BREEZE_PEN_WIDTH,
        ..CardRecipe::for_tokens(tokens)
    })
}

/// `Panel`: Breeze's group box, as [`breeze_card_style_for`].
pub fn breeze_panel_style_for(tokens: &InputTokens) -> RecipePanelStyle {
    RecipePanelStyle::new(PanelRecipe {
        // `[derived]` A framed group's layout takes the child margin.
        padding: spacing(LAYOUT_CHILD_MARGIN, tokens),
        // `[M]` The frame radius.
        corner_radius: BREEZE_CONTROL_RADIUS,
        // `[M]` The pen of every frame.
        border_width: BREEZE_PEN_WIDTH,
    })
}

/// `Banner`: an in-page notice, framed as a group box is and laid out as a
/// row of a nested layout.
pub fn breeze_banner_style_for(tokens: &InputTokens) -> RecipeBannerStyle {
    RecipeBannerStyle::new(BannerRecipe {
        // `[derived]` The child margin on every side, as a framed group.
        padding_horizontal: spacing(LAYOUT_CHILD_MARGIN, tokens),
        padding_vertical: spacing(LAYOUT_CHILD_MARGIN, tokens),
        // `[derived]` A frame in the page rounds at the frame radius.
        corner_radius: BREEZE_CONTROL_RADIUS,
        // `[M]` The layout spacing between the glyph and the text.
        content_gap: spacing(LAYOUT_SPACING, tokens),
        ..BannerRecipe::for_tokens(tokens)
    })
}

/// `Link`: the focus frame Breeze draws around a focused control, and an
/// underline in the 1 px pen.
///
/// Breeze's focus frame is a 2 px ring from 1 px inside the control to 1 px
/// outside it, its inner edge at the frame radius and its outer edge 2 px
/// further out (spec k.5.8). The recipe strokes the ring centred on the
/// link's bounds in the theme's focus-ring width, which is that ring when
/// the stroke's centre line rounds at [`LINK_FOCUS_RING_RADIUS`].
pub fn breeze_link_style_for(_tokens: &InputTokens) -> RecipeLinkStyle {
    RecipeLinkStyle::new(LinkRecipe {
        // `[derived]` The ring's centre line, on the link's bounds.
        corner_radius: LINK_FOCUS_RING_RADIUS,
        // `[derived]` Drawn in the pen of every Breeze line.
        underline_thickness: BREEZE_PEN_WIDTH,
    })
}

/// `Badge`: Breeze draws no pill, so a badge rounds at the frame radius like
/// every other Breeze frame (spec k.7).
pub fn breeze_badge_style_for(tokens: &InputTokens) -> RecipeBadgeStyle {
    RecipeBadgeStyle::new(BadgeRecipe {
        // `[derived]` The frame radius, in place of a pill.
        corner_radius: BREEZE_CONTROL_RADIUS,
        ..BadgeRecipe::for_tokens(tokens)
    })
}

/// `SegmentedControl`: the frame of a Breeze button, at the frame radius in
/// the 1 px pen.
///
/// Only the radius and the pen: the control lays its height and paddings out
/// from the recipe module's constants, so a value written here would not
/// reach the screen (`density-projection-gaps.md`, section 2).
pub fn breeze_segmented_control_style_for(tokens: &InputTokens) -> RecipeSegmentedControlStyle {
    RecipeSegmentedControlStyle::new(SegmentedControlRecipe {
        // `[M]` The frame radius.
        corner_radius: BREEZE_CONTROL_RADIUS,
        // `[M]` The pen of every frame.
        border_width: BREEZE_PEN_WIDTH,
        ..SegmentedControlRecipe::for_tokens(tokens)
    })
}

/// `Splitter`: Breeze's 1 px splitter handle (spec k.5.21).
///
/// The handle Breeze grabs is a 12 px proxy around the line (`breezerc`
/// `SplitterProxyWidth`); the `Splitter` widget widens its own gutter for the
/// pointer, which the style is not asked about.
pub fn breeze_splitter_style_for(_tokens: &InputTokens) -> RecipeSplitterStyle {
    RecipeSplitterStyle::new(SplitterRecipe {
        // `[M]` The splitter width.
        divider_line_thickness: SPLITTER_WIDTH,
    })
}

/// `ListView` and `TreeView` containers: the stock style, whose one
/// decoration is the drag-insertion line in the accent.
///
/// `[unpublished]` The spec records no Breeze drop indicator, so the line
/// keeps the framework's 2 px; the style carries no dimension of its own and
/// the projection is the identity.
pub fn breeze_list_container_style_for(tokens: &InputTokens) -> RecipeListContainerStyle {
    RecipeListContainerStyle::for_tokens(tokens)
}

/// `Calendar`: a day is selected as an item-view row is, so its fill rounds
/// at the frame radius the selection's outer edge has (spec k.5.16), and
/// today's ring is the 1 px pen.
///
/// The header's gap stays the recipe's: Breeze's 6 px layout spacing between
/// its four arrows would take the month title below a target's width at the
/// Touch density, where the arrows are 44 px.
pub fn breeze_calendar_style_for(tokens: &InputTokens) -> RecipeCalendarStyle {
    RecipeCalendarStyle::new(CalendarRecipe {
        // `[derived]` The selection's outer radius: 4.5 stroked, 5 outside.
        cell_radius: BREEZE_CONTROL_RADIUS,
        // `[derived]` A month or a year is selected as a day is.
        zoom_cell_radius: BREEZE_CONTROL_RADIUS,
        // `[M]` The pen of every frame.
        today_ring_width: BREEZE_PEN_WIDTH,
        ..CalendarRecipe::for_tokens(tokens)
    })
}

/// `SearchField`: the suggestion list is a menu without icons, so a row's
/// text lies [`SUGGESTION_TEXT_INSET`] inside it across and the 4 px item
/// margin inside it down (spec k.5.13). The list already lies 4 px inside
/// its panel, the recipe's constant, which is Breeze's menu margin.
pub fn breeze_search_field_style_for(tokens: &InputTokens) -> RecipeSearchFieldStyle {
    RecipeSearchFieldStyle::new(SearchFieldRecipe {
        // `[derived]` A menu item's text inset, without icons.
        row_padding_horizontal: spacing(SUGGESTION_TEXT_INSET, tokens),
        // `[M]` The menu item margin.
        row_padding_vertical: spacing(MENU_ITEM_MARGIN, tokens),
        ..SearchFieldRecipe::for_tokens(tokens)
    })
}

/// `Avatar`: Breeze has no avatar; only the presence dot's outline takes
/// the 1 px pen of every Breeze outline.
pub fn breeze_avatar_style_for(tokens: &InputTokens) -> RecipeAvatarStyle {
    RecipeAvatarStyle::new(AvatarRecipe {
        // `[derived]` The pen of every frame.
        presence_outline_width: BREEZE_PEN_WIDTH,
        ..AvatarRecipe::for_tokens(tokens)
    })
}

// ── Windows ─────────────────────────────────────────────────────────────

/// `Dialog`: a window, rounded as Breeze rounds a window's frame (spec
/// k.6), its content inside the top-level layout margin, Breeze's dialog
/// margin (spec k.4, k.7).
pub fn breeze_dialog_style_for(tokens: &InputTokens) -> RecipeDialogStyle {
    RecipeDialogStyle::new(DialogRecipe {
        // `[M]` The top-level layout margin.
        content_padding: spacing(LAYOUT_TOP_LEVEL_MARGIN, tokens),
        // `[Hp]` A window's radius.
        corner_radius: BREEZE_POPUP_RADIUS,
        ..DialogRecipe::for_tokens(tokens)
    })
}

/// `Snackbar`: a floating surface, so a window's radius.
pub fn breeze_snackbar_style_for(tokens: &InputTokens) -> RecipeSnackbarStyle {
    RecipeSnackbarStyle::new(SnackbarRecipe {
        // `[derived]` A floating surface rounds as a window does.
        corner_radius: BREEZE_POPUP_RADIUS,
        ..SnackbarRecipe::for_tokens(tokens)
    })
}

/// `Toast`: a floating surface, so a window's radius, its glyph, text and
/// close button the layout spacing apart.
pub fn breeze_toast_style_for(tokens: &InputTokens) -> RecipeToastStyle {
    RecipeToastStyle::new(ToastRecipe {
        // `[derived]` A floating surface rounds as a window does.
        corner_radius: BREEZE_POPUP_RADIUS,
        // `[M]` The layout spacing.
        content_gap: spacing(LAYOUT_SPACING, tokens),
        ..ToastRecipe::for_tokens(tokens)
    })
}

// ── Compact twins ───────────────────────────────────────────────────────

/// [`breeze_card_style_for`] at the Compact density.
pub fn breeze_card_style() -> RecipeCardStyle {
    breeze_card_style_for(&InputTokens::default())
}
/// [`breeze_panel_style_for`] at the Compact density.
pub fn breeze_panel_style() -> RecipePanelStyle {
    breeze_panel_style_for(&InputTokens::default())
}
/// [`breeze_banner_style_for`] at the Compact density.
pub fn breeze_banner_style() -> RecipeBannerStyle {
    breeze_banner_style_for(&InputTokens::default())
}
/// [`breeze_link_style_for`] at the Compact density.
pub fn breeze_link_style() -> RecipeLinkStyle {
    breeze_link_style_for(&InputTokens::default())
}
/// [`breeze_badge_style_for`] at the Compact density.
pub fn breeze_badge_style() -> RecipeBadgeStyle {
    breeze_badge_style_for(&InputTokens::default())
}
/// [`breeze_segmented_control_style_for`] at the Compact density.
pub fn breeze_segmented_control_style() -> RecipeSegmentedControlStyle {
    breeze_segmented_control_style_for(&InputTokens::default())
}
/// [`breeze_splitter_style_for`] at the Compact density.
pub fn breeze_splitter_style() -> RecipeSplitterStyle {
    breeze_splitter_style_for(&InputTokens::default())
}
/// [`breeze_list_container_style_for`] at the Compact density.
pub fn breeze_list_container_style() -> RecipeListContainerStyle {
    breeze_list_container_style_for(&InputTokens::default())
}
/// [`breeze_calendar_style_for`] at the Compact density.
pub fn breeze_calendar_style() -> RecipeCalendarStyle {
    breeze_calendar_style_for(&InputTokens::default())
}
/// [`breeze_search_field_style_for`] at the Compact density.
pub fn breeze_search_field_style() -> RecipeSearchFieldStyle {
    breeze_search_field_style_for(&InputTokens::default())
}
/// [`breeze_avatar_style_for`] at the Compact density.
pub fn breeze_avatar_style() -> RecipeAvatarStyle {
    breeze_avatar_style_for(&InputTokens::default())
}
/// [`breeze_dialog_style_for`] at the Compact density.
pub fn breeze_dialog_style() -> RecipeDialogStyle {
    breeze_dialog_style_for(&InputTokens::default())
}
/// [`breeze_snackbar_style_for`] at the Compact density.
pub fn breeze_snackbar_style() -> RecipeSnackbarStyle {
    breeze_snackbar_style_for(&InputTokens::default())
}
/// [`breeze_toast_style_for`] at the Compact density.
pub fn breeze_toast_style() -> RecipeToastStyle {
    breeze_toast_style_for(&InputTokens::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    const DENSITIES: [(TargetDensity, f32); 3] = [
        (TargetDensity::Compact, 1.0),
        (TargetDensity::Comfortable, 1.15),
        (TargetDensity::Touch, 1.3),
    ];

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// The link's ring, stroked centred in the 2 px focus-ring width, has
    /// Breeze's inner radius 5 and outer radius 7 (spec k.5.8); its
    /// underline is the pen.
    #[test]
    fn a_links_focus_ring_has_breezes_inner_and_outer_radii() {
        let link = breeze_link_style().recipe;
        let half = BREEZE_FOCUS_RING_WIDTH / 2.0;
        assert_eq!(link.corner_radius - half, 5.0);
        assert_eq!(link.corner_radius + half, 7.0);
        assert_eq!(link.underline_thickness, 1.0);
    }

    #[test]
    fn every_frame_rounds_at_the_frame_radius_in_the_one_pixel_pen() {
        assert_eq!(breeze_card_style().recipe.corner_radius, 5.0);
        assert_eq!(breeze_card_style().recipe.border_width, 1.0);
        assert_eq!(breeze_panel_style().recipe.corner_radius, 5.0);
        assert_eq!(breeze_panel_style().recipe.border_width, 1.0);
        assert_eq!(breeze_banner_style().recipe.corner_radius, 5.0);
        assert_eq!(breeze_badge_style().recipe.corner_radius, 5.0);
        let s = breeze_segmented_control_style().recipe;
        assert_eq!((s.corner_radius, s.border_width), (5.0, 1.0));
        let c = breeze_calendar_style().recipe;
        assert_eq!((c.cell_radius, c.zoom_cell_radius), (5.0, 5.0));
        assert_eq!(c.today_ring_width, 1.0);
        assert_eq!(breeze_splitter_style().recipe.divider_line_thickness, 1.0);
        assert_eq!(breeze_avatar_style().recipe.presence_outline_width, 1.0);
    }

    #[test]
    fn windows_round_at_the_window_radius() {
        assert_eq!(breeze_dialog_style().recipe.corner_radius, 5.0);
        assert_eq!(breeze_snackbar_style().recipe.corner_radius, 5.0);
        assert_eq!(breeze_toast_style().recipe.corner_radius, 5.0);
    }

    /// The layout gaps at Compact are Breeze's, and every one scales with
    /// the density's spacing; the painted radii and pens do not.
    #[test]
    fn the_gaps_are_breezes_and_follow_the_density() {
        for (density, factor) in DENSITIES {
            let t = InputTokens::for_density(density);
            let child = 6.0 * factor;
            assert!(near(breeze_card_style_for(&t).recipe.padding, child));
            assert!(near(breeze_panel_style_for(&t).recipe.padding, child));
            let b = breeze_banner_style_for(&t).recipe;
            assert!(near(b.padding_horizontal, child) && near(b.padding_vertical, child));
            assert!(near(b.content_gap, 6.0 * factor));
            assert!(near(
                breeze_dialog_style_for(&t).recipe.content_padding,
                10.0 * factor
            ));
            assert!(near(
                breeze_toast_style_for(&t).recipe.content_gap,
                6.0 * factor
            ));
            let s = breeze_search_field_style_for(&t).recipe;
            assert!(near(s.row_padding_horizontal, 16.0 * factor));
            assert!(near(s.row_padding_vertical, 4.0 * factor));
            assert_eq!(breeze_card_style_for(&t).recipe.corner_radius, 5.0);
            assert_eq!(breeze_dialog_style_for(&t).recipe.corner_radius, 5.0);
        }
    }

    /// What Breeze has no number for is the stock recipe's, at every
    /// density.
    #[test]
    fn what_breeze_does_not_say_is_the_stock_recipe() {
        for (density, _) in DENSITIES {
            let t = InputTokens::for_density(density);
            let card = breeze_card_style_for(&t).recipe;
            assert_eq!(
                card.shadow_density,
                CardRecipe::for_tokens(&t).shadow_density
            );
            let dialog = breeze_dialog_style_for(&t).recipe;
            assert_eq!(dialog.min_width, DialogRecipe::for_tokens(&t).min_width);
            let snackbar = breeze_snackbar_style_for(&t).recipe;
            let stock = SnackbarRecipe::for_tokens(&t);
            assert_eq!(
                (snackbar.padding_horizontal, snackbar.padding_vertical),
                (stock.padding_horizontal, stock.padding_vertical)
            );
            let toast = breeze_toast_style_for(&t).recipe;
            let stock = ToastRecipe::for_tokens(&t);
            assert_eq!(
                (toast.padding_horizontal, toast.padding_vertical),
                (stock.padding_horizontal, stock.padding_vertical)
            );
            let badge = breeze_badge_style_for(&t).recipe;
            let stock = BadgeRecipe::for_tokens(&t);
            assert_eq!(
                (badge.padding_horizontal, badge.padding_vertical),
                (stock.padding_horizontal, stock.padding_vertical)
            );
            let avatar = breeze_avatar_style_for(&t).recipe;
            assert_eq!(
                avatar.rounded_radius_ratio,
                AvatarRecipe::for_tokens(&t).rounded_radius_ratio
            );
            assert_eq!(
                breeze_calendar_style_for(&t).recipe.header_gap,
                CalendarRecipe::for_tokens(&t).header_gap
            );
            // Breeze's tool-button rule around the recipe's chevron gives
            // the recipe's own arrow.
            assert_eq!(
                breeze_calendar_style_for(&t).recipe.nav_arrow_size,
                CalendarRecipe::for_tokens(&t).nav_arrow_size
            );
        }
        // The `Calendar` draws its navigation chevrons 12 px wide.
        assert_eq!(
            12.0 + 2.0 * crate::geometry::TOOL_BUTTON_MARGIN_WIDTH,
            CalendarRecipe::default().nav_arrow_size
        );
    }
}
