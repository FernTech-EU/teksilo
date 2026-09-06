// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fluent metrics for the widgets whose shipped `Recipe*Style` already
//! paints the right *shape* — they only need WinUI's numbers.
//!
//! Every style here is the stock Teksilo implementation constructed with a
//! Fluent recipe, not a reimplementation. That is the whole point of the
//! Tier-2 recipe layer: a design language that agrees with the composition
//! and disagrees only about dimensions should not have to fork the paint
//! code. The widgets that genuinely disagree about *structure* — the
//! button's elevation edge, the switch's outline, the field's focus
//! underline, the list row's pill — live in their own modules.
//!
//! Two rules from [Geometry in Windows 11][geo] drive most of the values:
//! in-page and bar elements round at `ControlCornerRadius` (4 dp), things
//! that float round at `OverlayCornerRadius` (8 dp). The documented
//! exception is the tooltip, which floats but rounds at 4 dp because it is
//! small.
//!
//! Widgets not listed here keep their shipped recipe. That is deliberate
//! rather than unfinished: `radius_control` / `radius_popup` in
//! [`crate::shape`] already reach most of them, and inventing WinUI values
//! for controls WinUI has no counterpart to (an avatar, a colour picker, a
//! splitter grip) would be decoration, not fidelity.
//!
//! [geo]: https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/geometry

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Prop;
use teksilo_core::styles::density::{dp, spacing};
use teksilo_core::styles::{CardStyle, CardStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{InputTokens, TargetRole};
use teksilo_widgets::styles::{
    BadgeRecipe, BannerRecipe, ComboBoxRecipe, DialogRecipe, IconButtonRecipe, LinkRecipe,
    PanelRecipe, PopoverRecipe, ProgressBarRecipe, RecipeBadgeStyle, RecipeBannerStyle,
    RecipeCardStyle, RecipeComboBoxStyle, RecipeDialogStyle, RecipeIconButtonStyle,
    RecipeLinkStyle, RecipePanelStyle, RecipePopoverStyle, RecipeProgressBarStyle,
    RecipeScrollBarStyle, RecipeSegmentedControlStyle, RecipeSnackbarStyle, RecipeTabStyle,
    RecipeTableStyle, RecipeToastStyle, RecipeTooltipStyle, ScrollBarRecipe,
    SegmentedControlRecipe, SnackbarRecipe, TabRecipe, TableRecipe, ToastRecipe, TooltipRecipe,
};

use crate::shape::{
    FLUENT_CONTROL_CORNER_RADIUS as R_CONTROL, FLUENT_OVERLAY_CORNER_RADIUS as R_OVERLAY,
};

/// The standard Fluent control height (dp) — `TextControlThemeMinHeight`,
/// `CheckBoxHeight`, `MenuFlyoutThemeMinHeight`, `TabViewItemMinHeight`,
/// `SliderHorizontalHeight` all agree on it.
pub const FLUENT_CONTROL_HEIGHT: f32 = 32.0;

// ── Containers ──────────────────────────────────────────────────────────

/// `Card` — `OverlayCornerRadius`, since a card is a top-level container.
///
/// Delegates to the shipped card frame and injects the radius only when the
/// caller has not overridden it, the same shape the Material 3 preset uses.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentCardStyle;

impl CardStyle for FluentCardStyle {
    fn make_body(&self, cfg: &CardStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let mut cfg = cfg.clone();
        if cfg.corner_radius_override.is_none() {
            cfg.corner_radius_override = Some(Prop::Static(R_OVERLAY));
        }
        RecipeCardStyle::default().make_body(&cfg, ctx)
    }
}

/// `Panel` — a grouped in-page surface, so `OverlayCornerRadius` as well;
/// `SettingsExpander` and the WinUI Gallery's grouped cards both round at
/// the larger radius.
pub fn fluent_panel_style_for(tokens: &InputTokens) -> RecipePanelStyle {
    RecipePanelStyle::new(PanelRecipe {
        corner_radius: R_OVERLAY,
        border_width: 1.0,
        ..PanelRecipe::for_tokens(tokens)
    })
}

// ── Overlays ────────────────────────────────────────────────────────────

/// `Popover` / flyout — `OverlayCornerRadius`, `FlyoutContentPadding`
/// (16 dp), and a 1 dp `SurfaceStrokeColorFlyout` hairline. Menus reuse the
/// same radius; their rows supply their own padding.
pub fn fluent_popover_style_for(tokens: &InputTokens) -> RecipePopoverStyle {
    RecipePopoverStyle::new(PopoverRecipe {
        padding: 16.0,
        corner_radius: R_OVERLAY,
        border_width: 1.0,
        menu_popup_corner_radius: R_OVERLAY,
        ..PopoverRecipe::for_tokens(tokens)
    })
}

/// `Tooltip` — the documented radius exception: an overlay that rounds at
/// 4 dp "due to its small size". `ToolTipBorderPadding` is `9,6,9,8` and
/// `ToolTipMaxWidth` is 320.
pub fn fluent_tooltip_style_for(tokens: &InputTokens) -> RecipeTooltipStyle {
    RecipeTooltipStyle::new(TooltipRecipe {
        padding_horizontal: 9.0,
        padding_vertical: 7.0,
        corner_radius: R_CONTROL,
        max_width: 320.0,
        ..TooltipRecipe::for_tokens(tokens)
    })
}

/// `Dialog` — `ContentDialogPadding` 24, `ContentDialogMinWidth` 320,
/// `OverlayCornerRadius`.
pub fn fluent_dialog_style_for(tokens: &InputTokens) -> RecipeDialogStyle {
    RecipeDialogStyle::new(DialogRecipe {
        content_padding: spacing(24.0, tokens),
        min_width: dp(320.0, TargetRole::Target, tokens),
        corner_radius: R_OVERLAY,
    })
}

/// `Snackbar` — a floating notification, so the overlay radius.
pub fn fluent_snackbar_style_for(tokens: &InputTokens) -> RecipeSnackbarStyle {
    RecipeSnackbarStyle::new(SnackbarRecipe {
        corner_radius: R_OVERLAY,
        ..SnackbarRecipe::for_tokens(tokens)
    })
}

/// `Toast` — likewise floating.
pub fn fluent_toast_style_for(tokens: &InputTokens) -> RecipeToastStyle {
    RecipeToastStyle::new(ToastRecipe {
        corner_radius: R_OVERLAY,
        ..ToastRecipe::for_tokens(tokens)
    })
}

/// `Banner` — the `InfoBar` analogue. In-page, so the control radius.
pub fn fluent_banner_style_for(tokens: &InputTokens) -> RecipeBannerStyle {
    RecipeBannerStyle::new(BannerRecipe {
        corner_radius: R_CONTROL,
        ..BannerRecipe::for_tokens(tokens)
    })
}

// ── In-page controls ────────────────────────────────────────────────────

/// `ComboBox` — 32 dp tall, `ButtonPadding`-style 11 dp gutters, 4 dp.
pub fn fluent_combo_box_style_for(tokens: &InputTokens) -> RecipeComboBoxStyle {
    RecipeComboBoxStyle::new(ComboBoxRecipe {
        height: FLUENT_CONTROL_HEIGHT,
        padding_horizontal: 11.0,
        corner_radius: R_CONTROL,
        ..ComboBoxRecipe::for_tokens(tokens)
    })
}

/// `IconButton` — the `AppBarButton` / subtle icon button: a 32 dp square
/// with a 16 dp glyph at the control radius.
pub fn fluent_icon_button_style_for(tokens: &InputTokens) -> RecipeIconButtonStyle {
    RecipeIconButtonStyle::new(IconButtonRecipe {
        size_default: FLUENT_CONTROL_HEIGHT,
        icon_size: 16.0,
        corner_radius: R_CONTROL,
        ..IconButtonRecipe::for_tokens(tokens)
    })
}

/// `Link` — `HyperlinkButton` rounds at the control radius and underlines
/// at a hairline.
pub fn fluent_link_style_for(_tokens: &InputTokens) -> RecipeLinkStyle {
    RecipeLinkStyle::new(LinkRecipe {
        corner_radius: R_CONTROL,
        underline_thickness: 1.0,
    })
}

/// `SegmentedControl` — the `SelectorBar` shape: 32 dp tall, 4 dp, hairline.
pub fn fluent_segmented_control_style_for(tokens: &InputTokens) -> RecipeSegmentedControlStyle {
    RecipeSegmentedControlStyle::new(SegmentedControlRecipe {
        height: FLUENT_CONTROL_HEIGHT,
        corner_radius: R_CONTROL,
        border_width: 1.0,
        ..SegmentedControlRecipe::for_tokens(tokens)
    })
}

/// `Badge` — the `InfoBadge` is a pill, so the radius is left at the
/// baseline's fully-rounded value; only the padding is tightened to
/// Fluent's denser chip.
pub fn fluent_badge_style_for(tokens: &InputTokens) -> RecipeBadgeStyle {
    RecipeBadgeStyle::new(BadgeRecipe {
        padding_horizontal: 8.0,
        padding_vertical: 2.0,
        ..BadgeRecipe::for_tokens(tokens)
    })
}

/// `ProgressBar` — a bar element, so the control radius. The renderer
/// clamps a radius to half the shorter side, so a thin bar reads as a
/// capsule and a thick one as a 4 dp rounded rect, which is exactly the
/// Fluent behaviour.
pub fn fluent_progress_bar_style_for(_tokens: &InputTokens) -> RecipeProgressBarStyle {
    RecipeProgressBarStyle::new(ProgressBarRecipe {
        corner_radius: R_CONTROL,
    })
}

// ── Chrome ──────────────────────────────────────────────────────────────

/// `ScrollBar` — Fluent's "conscious / unconscious" bar. `ScrollBarSize` is
/// a fixed 12 dp lane whose layout never changes, so content never reflows;
/// what animates is the thumb inside it, from a 2 dp resting rail to a 6 dp
/// hover thumb. (WinUI expresses that as an 8 → 12 dp rectangle stroked
/// with a 6 dp transparent border, which leaves the same 2 → 6 dp of
/// visible fill.) `ScrollBarHorizontalThumbMinWidth` is 30.
pub fn fluent_scroll_bar_style_for(tokens: &InputTokens) -> RecipeScrollBarStyle {
    RecipeScrollBarStyle::new(ScrollBarRecipe {
        // The lane's thickness is fixed at every density: WinUI's own layout
        // never reflows for it, and the coarse grab comes from `hit_outset`
        // over an unchanged visual.
        thickness_idle: 2.0,
        thickness_hover: 6.0,
        min_thumb_length: dp(30.0, TargetRole::Target, tokens),
        corner_radius: 3.0,
    })
}

/// `TabBar` — `TabViewItemMinHeight` 32, `TabViewItemHeaderPadding` 8, and
/// a 3 dp active indicator (the `NavigationViewSelectionIndicator`
/// thickness Fluent uses wherever a selection underline appears).
pub fn fluent_tab_style_for(tokens: &InputTokens) -> RecipeTabStyle {
    RecipeTabStyle::new(TabRecipe {
        editor_height: FLUENT_CONTROL_HEIGHT,
        tool_window_height: FLUENT_CONTROL_HEIGHT,
        padding_horizontal: 8.0,
        underline_active: 3.0,
        close_button_size: 16.0,
        ..TabRecipe::for_tokens(tokens)
    })
}

/// `TableView` / `TreeTableView` — rows and header on the `ListViewItem`
/// 40 dp rhythm, 12 dp cell gutters, 4 dp corners.
pub fn fluent_table_style_for(tokens: &InputTokens) -> RecipeTableStyle {
    RecipeTableStyle::new(TableRecipe {
        row_height: 40.0,
        header_height: 40.0,
        cell_padding_horizontal: 12.0,
        corner_radius: R_CONTROL,
        tree_indent_per_level: 16.0,
        ..TableRecipe::for_tokens(tokens)
    })
}
/// [`fluent_panel_style_for`] at the default Compact density.
pub fn fluent_panel_style() -> RecipePanelStyle {
    fluent_panel_style_for(&InputTokens::default())
}
/// [`fluent_popover_style_for`] at the default Compact density.
pub fn fluent_popover_style() -> RecipePopoverStyle {
    fluent_popover_style_for(&InputTokens::default())
}
/// [`fluent_tooltip_style_for`] at the default Compact density.
pub fn fluent_tooltip_style() -> RecipeTooltipStyle {
    fluent_tooltip_style_for(&InputTokens::default())
}
/// [`fluent_dialog_style_for`] at the default Compact density.
pub fn fluent_dialog_style() -> RecipeDialogStyle {
    fluent_dialog_style_for(&InputTokens::default())
}
/// [`fluent_snackbar_style_for`] at the default Compact density.
pub fn fluent_snackbar_style() -> RecipeSnackbarStyle {
    fluent_snackbar_style_for(&InputTokens::default())
}
/// [`fluent_toast_style_for`] at the default Compact density.
pub fn fluent_toast_style() -> RecipeToastStyle {
    fluent_toast_style_for(&InputTokens::default())
}
/// [`fluent_banner_style_for`] at the default Compact density.
pub fn fluent_banner_style() -> RecipeBannerStyle {
    fluent_banner_style_for(&InputTokens::default())
}
/// [`fluent_combo_box_style_for`] at the default Compact density.
pub fn fluent_combo_box_style() -> RecipeComboBoxStyle {
    fluent_combo_box_style_for(&InputTokens::default())
}
/// [`fluent_icon_button_style_for`] at the default Compact density.
pub fn fluent_icon_button_style() -> RecipeIconButtonStyle {
    fluent_icon_button_style_for(&InputTokens::default())
}
/// [`fluent_link_style_for`] at the default Compact density.
pub fn fluent_link_style() -> RecipeLinkStyle {
    fluent_link_style_for(&InputTokens::default())
}
/// [`fluent_segmented_control_style_for`] at the default Compact density.
pub fn fluent_segmented_control_style() -> RecipeSegmentedControlStyle {
    fluent_segmented_control_style_for(&InputTokens::default())
}
/// [`fluent_badge_style_for`] at the default Compact density.
pub fn fluent_badge_style() -> RecipeBadgeStyle {
    fluent_badge_style_for(&InputTokens::default())
}
/// [`fluent_progress_bar_style_for`] at the default Compact density.
pub fn fluent_progress_bar_style() -> RecipeProgressBarStyle {
    fluent_progress_bar_style_for(&InputTokens::default())
}
/// [`fluent_scroll_bar_style_for`] at the default Compact density.
pub fn fluent_scroll_bar_style() -> RecipeScrollBarStyle {
    fluent_scroll_bar_style_for(&InputTokens::default())
}
/// [`fluent_tab_style_for`] at the default Compact density.
pub fn fluent_tab_style() -> RecipeTabStyle {
    fluent_tab_style_for(&InputTokens::default())
}
/// [`fluent_table_style_for`] at the default Compact density.
pub fn fluent_table_style() -> RecipeTableStyle {
    fluent_table_style_for(&InputTokens::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_surfaces_use_the_overlay_radius() {
        assert_eq!(fluent_popover_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(
            fluent_popover_style().recipe.menu_popup_corner_radius,
            R_OVERLAY
        );
        assert_eq!(fluent_dialog_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(fluent_snackbar_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(fluent_toast_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(fluent_panel_style().recipe.corner_radius, R_OVERLAY);
    }

    #[test]
    fn in_page_and_bar_elements_use_the_control_radius() {
        assert_eq!(fluent_combo_box_style().recipe.corner_radius, R_CONTROL);
        assert_eq!(fluent_icon_button_style().recipe.corner_radius, R_CONTROL);
        assert_eq!(fluent_link_style().recipe.corner_radius, R_CONTROL);
        assert_eq!(fluent_banner_style().recipe.corner_radius, R_CONTROL);
        assert_eq!(fluent_table_style().recipe.corner_radius, R_CONTROL);
        assert_eq!(
            fluent_segmented_control_style().recipe.corner_radius,
            R_CONTROL
        );
        assert_eq!(fluent_progress_bar_style().recipe.corner_radius, R_CONTROL);
    }

    #[test]
    fn the_tooltip_is_the_documented_radius_exception() {
        // It floats, but rounds at 4 dp because it is small.
        assert_eq!(fluent_tooltip_style().recipe.corner_radius, R_CONTROL);
        assert_ne!(fluent_tooltip_style().recipe.corner_radius, R_OVERLAY);
        assert_eq!(fluent_tooltip_style().recipe.max_width, 320.0);
    }

    #[test]
    fn interactive_controls_share_the_thirty_two_dp_height() {
        assert_eq!(fluent_combo_box_style().recipe.height, 32.0);
        assert_eq!(fluent_icon_button_style().recipe.size_default, 32.0);
        assert_eq!(fluent_segmented_control_style().recipe.height, 32.0);
        assert_eq!(fluent_tab_style().recipe.editor_height, 32.0);
    }

    #[test]
    fn scroll_bar_grows_from_a_rail_to_a_thumb() {
        let r = fluent_scroll_bar_style().recipe;
        assert_eq!(r.thickness_idle, 2.0);
        assert_eq!(r.thickness_hover, 6.0);
        assert!(r.thickness_hover > r.thickness_idle);
        assert_eq!(r.min_thumb_length, 30.0);
    }

    #[test]
    fn table_rows_match_the_list_row_rhythm() {
        let t = fluent_table_style().recipe;
        assert_eq!(t.row_height, 40.0);
        assert_eq!(t.header_height, t.row_height);
    }
}
