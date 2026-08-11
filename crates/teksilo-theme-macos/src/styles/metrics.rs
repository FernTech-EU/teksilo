// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS metrics for the widgets whose shipped `Recipe*Style` already
//! paints the right *shape* — they only need AppKit's numbers.
//!
//! Every style here is the stock Teksilo implementation constructed with a
//! macOS recipe, not a reimplementation. That is the whole point of the
//! Tier-2 recipe layer: a design language that agrees with the composition
//! and disagrees only about dimensions should not have to fork the paint
//! code. The widgets that genuinely disagree about *structure* — the
//! button's bezel, the switch's near-full knob, the field's focus halo,
//! the row's selection capsule — live in their own modules.
//!
//! Two rules drive most of the values. Anything in the page rounds at
//! [`MACOS_CONTROL_CORNER_RADIUS`](crate::shape::MACOS_CONTROL_CORNER_RADIUS) (6 dp) and anything that floats at
//! [`MACOS_OVERLAY_CORNER_RADIUS`](crate::shape::MACOS_OVERLAY_CORNER_RADIUS) (10 dp), with menus at their own
//! measured [`MACOS_MENU_CORNER_RADIUS`](crate::shape::MACOS_MENU_CORNER_RADIUS) (9 dp) and help tags smaller
//! still. And everything a pointer interacts with stands
//! [`MACOS_CONTROL_HEIGHT`](crate::shape::MACOS_CONTROL_HEIGHT) (22 dp) tall — a third shorter than Fluent's
//! 32, which is most of why a macOS window fits more.
//!
//! Widgets not listed here keep their shipped recipe. That is deliberate
//! rather than unfinished: `radius_control` / `radius_popup` in
//! [`crate::shape`] already reach most of them, and inventing AppKit values
//! for controls AppKit has no counterpart to (a colour picker's swatch
//! grid, a splitter grip) would be decoration, not fidelity.

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Prop;
use teksilo_core::styles::{CardStyle, CardStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_widgets::styles::{
    AvatarRecipe, BadgeRecipe, BannerRecipe, CalendarRecipe, ComboBoxRecipe, DialogRecipe,
    IconButtonRecipe, LinkRecipe, PanelRecipe, PopoverRecipe, ProgressBarRecipe, RecipeAvatarStyle,
    RecipeBadgeStyle, RecipeBannerStyle, RecipeCalendarStyle, RecipeCardStyle, RecipeComboBoxStyle,
    RecipeDialogStyle, RecipeIconButtonStyle, RecipeLinkStyle, RecipePanelStyle,
    RecipePopoverStyle, RecipeProgressBarStyle, RecipeScrollBarStyle, RecipeSearchFieldStyle,
    RecipeSegmentedControlStyle, RecipeSnackbarStyle, RecipeTabStyle, RecipeTableStyle,
    RecipeToastStyle, RecipeTooltipStyle, ScrollBarRecipe, SearchFieldRecipe,
    SegmentedControlRecipe, SnackbarRecipe, TabRecipe, TableRecipe, ToastRecipe, TooltipRecipe,
};

use crate::shape::{
    MACOS_CONTROL_CORNER_RADIUS as R_CONTROL, MACOS_CONTROL_HEIGHT as H_CONTROL,
    MACOS_MENU_CORNER_RADIUS as R_MENU, MACOS_OVERLAY_CORNER_RADIUS as R_OVERLAY,
};

/// A help tag's radius (dp). Small even by the control radius' standards —
/// AppKit's tooltip is the one surface that rounds tighter than everything
/// else because of how little of it there is.
pub const MACOS_HELP_TAG_CORNER_RADIUS: f32 = 5.0;

/// `NSTableView.rowHeight` on macOS 11+. Shared by the table, the row
/// styles and the calendar's own grid so a mixed window keeps one rhythm.
pub const MACOS_ROW_HEIGHT: f32 = 24.0;

// ── Containers ──────────────────────────────────────────────────────────

/// `Card` — the overlay radius, since a card is a top-level grouped
/// container in the settings-box sense.
///
/// Delegates to the shipped card frame and injects the radius only when
/// the caller has not overridden it, the same shape the Fluent and
/// Material 3 presets use.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsCardStyle;

impl CardStyle for MacOsCardStyle {
    fn make_body(&self, cfg: &CardStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let mut cfg = cfg.clone();
        if cfg.corner_radius_override.is_none() {
            cfg.corner_radius_override = Some(Prop::Static(R_OVERLAY));
        }
        RecipeCardStyle::default().make_body(&cfg, ctx)
    }
}

/// `Panel` — a grouped in-page surface. macOS settings boxes and grouped
/// table sections round at the same radius a sheet does.
pub fn macos_panel_style() -> RecipePanelStyle {
    RecipePanelStyle::new(PanelRecipe {
        corner_radius: R_OVERLAY,
        border_width: 1.0,
        ..PanelRecipe::default()
    })
}

// ── Overlays ────────────────────────────────────────────────────────────

/// `Popover` — `NSPopover`'s radius and content inset, with menus reusing
/// the measured 9 dp menu radius rather than being rounded off to the
/// overlay one. macOS shadows a floating surface much harder than IntUI
/// does, hence the raised density.
pub fn macos_popover_style() -> RecipePopoverStyle {
    RecipePopoverStyle::new(PopoverRecipe {
        padding: 14.0,
        corner_radius: R_OVERLAY,
        border_width: 1.0,
        menu_popup_corner_radius: R_MENU,
        shadow_density: 0.8,
    })
}

/// `Tooltip` — the macOS *help tag*: a small, tightly-rounded chip with a
/// modest shadow. Narrower than Fluent's 320 dp, because AppKit wraps help
/// text sooner.
pub fn macos_tooltip_style() -> RecipeTooltipStyle {
    RecipeTooltipStyle::new(TooltipRecipe {
        padding_horizontal: 8.0,
        padding_vertical: 5.0,
        corner_radius: MACOS_HELP_TAG_CORNER_RADIUS,
        max_width: 300.0,
        shadow_density: 0.6,
    })
}

/// `Dialog` — an `NSAlert` sheet: narrow, generously padded, and rounded
/// at the sheet radius.
pub fn macos_dialog_style() -> RecipeDialogStyle {
    RecipeDialogStyle::new(DialogRecipe {
        content_padding: 20.0,
        min_width: 260.0,
        corner_radius: R_OVERLAY,
    })
}

/// `Snackbar` — a floating notification, so the overlay radius.
pub fn macos_snackbar_style() -> RecipeSnackbarStyle {
    RecipeSnackbarStyle::new(SnackbarRecipe {
        corner_radius: R_OVERLAY,
        ..SnackbarRecipe::default()
    })
}

/// `Toast` — the Notification Centre banner shape: floating, so the
/// overlay radius.
pub fn macos_toast_style() -> RecipeToastStyle {
    RecipeToastStyle::new(ToastRecipe {
        corner_radius: R_OVERLAY,
        glyph_size: 16.0,
        ..ToastRecipe::default()
    })
}

/// `Banner` — an in-page notice, so the control radius.
pub fn macos_banner_style() -> RecipeBannerStyle {
    RecipeBannerStyle::new(BannerRecipe {
        corner_radius: R_CONTROL,
        glyph_size: 14.0,
        ..BannerRecipe::default()
    })
}

// ── In-page controls ────────────────────────────────────────────────────

/// `ComboBox` — `NSPopUpButton`: the same 22 dp bezel a push button wears,
/// with a chevron column on the trailing edge.
pub fn macos_combo_box_style() -> RecipeComboBoxStyle {
    RecipeComboBoxStyle::new(ComboBoxRecipe {
        height: H_CONTROL,
        padding_horizontal: 8.0,
        arrow_column_width: 20.0,
        corner_radius: R_CONTROL,
    })
}

/// `IconButton` — the toolbar button family. Borderless until hovered, and
/// noticeably smaller than Fluent's 32 dp square at every rung.
pub fn macos_icon_button_style() -> RecipeIconButtonStyle {
    RecipeIconButtonStyle::new(IconButtonRecipe {
        size_compact: 18.0,
        size_default: H_CONTROL,
        size_toolbar: 28.0,
        size_large: 36.0,
        size_hero: 44.0,
        icon_size: 14.0,
        icon_size_toolbar: 16.0,
        icon_size_large: 20.0,
        icon_size_hero: 28.0,
        corner_radius: MACOS_HELP_TAG_CORNER_RADIUS,
    })
}

/// `Link` — AppKit underlines a link at a hairline and gives its hit area
/// a small radius.
pub fn macos_link_style() -> RecipeLinkStyle {
    RecipeLinkStyle::new(LinkRecipe {
        corner_radius: 4.0,
        underline_thickness: 1.0,
    })
}

/// `SegmentedControl` — `NSSegmentedControl`: the push button's height and
/// radius, with each segment carrying the same gutter.
pub fn macos_segmented_control_style() -> RecipeSegmentedControlStyle {
    RecipeSegmentedControlStyle::new(SegmentedControlRecipe {
        height: H_CONTROL,
        padding_horizontal: 10.0,
        padding_vertical: 3.0,
        corner_radius: R_CONTROL,
        border_width: 1.0,
    })
}

/// `Badge` — a pill, so the radius stays at the baseline's fully-rounded
/// value; only the padding tightens to macOS's denser chip.
pub fn macos_badge_style() -> RecipeBadgeStyle {
    RecipeBadgeStyle::new(BadgeRecipe {
        padding_horizontal: 6.0,
        padding_vertical: 1.0,
        ..BadgeRecipe::default()
    })
}

/// `ProgressBar` — a thin capsule. The renderer clamps a radius to half
/// the shorter side, so a slim bar reads as fully rounded (which is what
/// AppKit draws) and a thick one as a 3 dp rounded rect.
pub fn macos_progress_bar_style() -> RecipeProgressBarStyle {
    RecipeProgressBarStyle::new(ProgressBarRecipe { corner_radius: 3.0 })
}

// ── Chrome ──────────────────────────────────────────────────────────────

/// `ScrollBar` — the macOS **overlay scroller**: no track at rest, a
/// translucent thumb that fades in on scroll and widens under the pointer.
/// The colour half of that behaviour lives in the crate's colour
/// projection (`scrollbar_*`); this is the geometry.
pub fn macos_scroll_bar_style() -> RecipeScrollBarStyle {
    RecipeScrollBarStyle::new(ScrollBarRecipe {
        thickness_idle: 6.0,
        thickness_hover: 9.0,
        min_thumb_length: 28.0,
        // Half the hover thickness, so the thumb is a capsule at its
        // widest and stays one as it narrows.
        corner_radius: 4.5,
    })
}

/// `TabBar` — `NSTabView`'s header strip. AppKit marks the selected tab by
/// filling it rather than underlining it, which the shipped recipe cannot
/// express, so the underline is kept thin: it reads as a selection rule
/// rather than a Material-style indicator.
pub fn macos_tab_style() -> RecipeTabStyle {
    RecipeTabStyle::new(TabRecipe {
        editor_height: 26.0,
        tool_window_height: H_CONTROL,
        padding_horizontal: 12.0,
        underline_active: 2.0,
        underline_hover: 2.0,
        close_button_size: 14.0,
    })
}

/// `TableView` / `TreeTableView` — rows and header on the 24 dp
/// `NSTableView` rhythm, with the same 8 dp cell gutter the row styles use.
pub fn macos_table_style() -> RecipeTableStyle {
    RecipeTableStyle::new(TableRecipe {
        row_height: MACOS_ROW_HEIGHT,
        header_height: MACOS_ROW_HEIGHT,
        cell_padding_horizontal: 8.0,
        corner_radius: R_CONTROL,
        tree_indent_per_level: 16.0,
        tree_twist_size: 10.0,
        ..TableRecipe::default()
    })
}

/// `Calendar` — `NSDatePicker`'s graphical style: a compact grid of
/// lightly-rounded day cells.
pub fn macos_calendar_style() -> RecipeCalendarStyle {
    RecipeCalendarStyle::new(CalendarRecipe {
        outer_padding: 8.0,
        header_height: 26.0,
        cell_size: 28.0,
        cell_radius: MACOS_HELP_TAG_CORNER_RADIUS,
        zoom_cell_radius: MACOS_HELP_TAG_CORNER_RADIUS,
        nav_arrow_size: 20.0,
        nav_arrow_radius: MACOS_HELP_TAG_CORNER_RADIUS,
        nav_icon_size: 11.0,
        ..CalendarRecipe::default()
    })
}

/// `SearchField` — `NSSearchField`: a 22 dp field with a small magnifier
/// glyph, and a results panel rounded like a popover.
pub fn macos_search_field_style() -> RecipeSearchFieldStyle {
    RecipeSearchFieldStyle::new(SearchFieldRecipe {
        glyph_size: 13.0,
        row_height: H_CONTROL,
        row_padding_horizontal: 8.0,
        row_padding_vertical: 3.0,
        row_corner_radius: 4.0,
        panel_corner_radius: R_CONTROL,
        ..SearchFieldRecipe::default()
    })
}

/// `Avatar` — macOS shows people as circles (Contacts, Messages, the
/// login window), so the rounded variant is pushed most of the way there
/// and the ring is kept to a hairline-and-a-half.
pub fn macos_avatar_style() -> RecipeAvatarStyle {
    RecipeAvatarStyle::new(AvatarRecipe {
        border_default: 1.5,
        rounded_radius_ratio: 0.35,
        ..AvatarRecipe::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_surfaces_use_the_overlay_radius() {
        assert_eq!(macos_popover_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(macos_dialog_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(macos_snackbar_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(macos_toast_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(macos_panel_style().recipe.corner_radius, R_OVERLAY);
    }

    #[test]
    fn a_menu_keeps_its_own_measured_radius() {
        // Neither the control radius nor the overlay one — the only
        // radius in this preset with a real measurement behind it.
        let r = macos_popover_style().recipe;
        assert_eq!(r.menu_popup_corner_radius, R_MENU);
        assert_ne!(r.menu_popup_corner_radius, r.corner_radius);
        assert_ne!(r.menu_popup_corner_radius, R_CONTROL);
    }

    #[test]
    fn in_page_and_bar_elements_use_the_control_radius() {
        assert_eq!(macos_combo_box_style().recipe.corner_radius, R_CONTROL);
        assert_eq!(macos_banner_style().recipe.corner_radius, R_CONTROL);
        assert_eq!(macos_table_style().recipe.corner_radius, R_CONTROL);
        assert_eq!(
            macos_segmented_control_style().recipe.corner_radius,
            R_CONTROL
        );
        assert_eq!(
            macos_search_field_style().recipe.panel_corner_radius,
            R_CONTROL
        );
    }

    #[test]
    fn the_help_tag_rounds_tighter_than_anything_else() {
        let t = macos_tooltip_style().recipe;
        assert_eq!(t.corner_radius, MACOS_HELP_TAG_CORNER_RADIUS);
        assert!(t.corner_radius < R_CONTROL);
        assert!(t.max_width < 320.0, "Fluent's ToolTipMaxWidth");
    }

    #[test]
    fn interactive_controls_share_the_twenty_two_dp_height() {
        assert_eq!(macos_combo_box_style().recipe.height, H_CONTROL);
        assert_eq!(macos_icon_button_style().recipe.size_default, H_CONTROL);
        assert_eq!(macos_segmented_control_style().recipe.height, H_CONTROL);
        assert_eq!(macos_search_field_style().recipe.row_height, H_CONTROL);
        assert_eq!(macos_tab_style().recipe.tool_window_height, H_CONTROL);
    }

    /// The density claim, pinned against the sibling preset.
    #[test]
    fn every_control_is_shorter_than_its_fluent_counterpart() {
        const FLUENT_CONTROL_HEIGHT: f32 = 32.0;
        for h in [
            macos_combo_box_style().recipe.height,
            macos_icon_button_style().recipe.size_default,
            macos_segmented_control_style().recipe.height,
            macos_tab_style().recipe.tool_window_height,
            macos_table_style().recipe.row_height,
        ] {
            assert!(h < FLUENT_CONTROL_HEIGHT, "{h} is not denser than Fluent");
        }
    }

    #[test]
    fn the_icon_button_size_ladder_is_monotonic() {
        let r = macos_icon_button_style().recipe;
        assert!(r.size_compact < r.size_default);
        assert!(r.size_default < r.size_toolbar);
        assert!(r.size_toolbar < r.size_large);
        assert!(r.size_large < r.size_hero);
        assert!(r.icon_size < r.icon_size_toolbar);
        assert!(r.icon_size_toolbar < r.icon_size_large);
        assert!(r.icon_size_large < r.icon_size_hero);
        // …and every glyph fits inside its button.
        for (icon, size) in [
            (r.icon_size, r.size_default),
            (r.icon_size_toolbar, r.size_toolbar),
            (r.icon_size_large, r.size_large),
            (r.icon_size_hero, r.size_hero),
        ] {
            assert!(
                icon < size,
                "a {icon} dp glyph does not fit a {size} dp button"
            );
        }
    }

    #[test]
    fn the_scroller_widens_rather_than_appearing() {
        let r = macos_scroll_bar_style().recipe;
        assert!(r.thickness_hover > r.thickness_idle);
        // A capsule at its widest: the radius is half the hover thickness.
        assert!((r.corner_radius * 2.0 - r.thickness_hover).abs() < 1e-6);
        assert!(r.min_thumb_length > 0.0);
    }

    #[test]
    fn table_rows_match_the_row_style_rhythm() {
        let t = macos_table_style().recipe;
        assert_eq!(t.row_height, MACOS_ROW_HEIGHT);
        assert_eq!(t.header_height, t.row_height);
        assert_eq!(
            t.row_height,
            crate::styles::standard_item::macos_standard_item_recipe().min_height_single_line,
            "a TableView row and a ListView row must share one rhythm"
        );
        assert_eq!(
            t.tree_indent_per_level,
            crate::styles::standard_item::macos_standard_item_recipe().tree_indent_step,
            "a TreeTableView and a TreeView must indent identically"
        );
    }

    #[test]
    fn the_calendar_cell_is_a_rounded_square_not_a_circle() {
        let c = macos_calendar_style().recipe;
        assert!(c.cell_radius < c.cell_size * 0.5);
        assert_eq!(c.cell_radius, c.zoom_cell_radius);
    }
}
