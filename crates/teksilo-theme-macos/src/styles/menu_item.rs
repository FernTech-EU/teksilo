// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS menu row.
//!
//! One difference from every other preset, and it is not subtle: a
//! highlighted macOS menu row fills with the **accent colour** and its
//! label turns **white**. Fluent uses a neutral `SubtleFill` wash; IntUI
//! uses a light accent tint; macOS commits to the whole row. An
//! accent-tinted-but-not-filled menu row is one of the fastest ways to
//! make a Mac app look not-Mac.
//!
//! Since Big Sur the fill is a **rounded capsule inset from the menu's
//! edges** rather than a full-bleed bar, which is why the backdrop here is
//! padded rather than stretched.
//!
//! Flipping the label is what
//! [`MenuItemStyle::highlighted_label_role`] exists for: `MenuItem` builds
//! its label long before a style's `make_body` runs, so a style that fills
//! the row cannot recolour the text afterwards. It declares
//! [`TextRole::OnAccent`] and the widget composes it into the label's and
//! the shortcut's colour signals.
//!
//! The row *layout* — the reserved icon/check column, the shortcut
//! gutter, the separator — is `MenuItem`-specific and identical across
//! design languages, so it is delegated to [`RecipeMenuItemStyle`] with
//! macOS metrics. The delegate's own interaction tint is suppressed by
//! handing it signals that never fire, exactly as the Fluent menu row
//! does, so the highlight painted here is the only one.

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{MenuItemStyle, MenuItemStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, SurfaceRole, TextRole};
use teksilo_widgets::primitives::{Padding, RectWidget, ZStack};
use teksilo_widgets::styles::{MenuItemRecipe, RecipeMenuItemStyle};

/// Row height (dp) — a 16 dp Body line box plus 3 dp of air top and
/// bottom, the same arithmetic the push button uses.
const ITEM_HEIGHT: f32 = 22.0;
/// Leading / trailing gutter (dp).
const PADDING_H: f32 = 10.0;
/// The checkmark / icon column (dp).
const ICON_COLUMN: f32 = 14.0;
/// Gap from the icon column to the label, so an icon-less row's label
/// still lines up with an icon'd one.
const ICON_LABEL_GAP: f32 = 8.0;
/// Minimum gutter between a label and its shortcut chip (dp). macOS menus
/// keep accelerators well clear of the title.
const SHORTCUT_GAP: f32 = 32.0;
/// A hairline with 5 dp of air either side.
const SEPARATOR_HEIGHT: f32 = 11.0;
/// The highlight capsule's radius (dp).
const HIGHLIGHT_RADIUS: f32 = 4.0;
/// How far the capsule floats inside the row's full width (dp).
const HIGHLIGHT_INSET: f32 = 5.0;

// The capsule has to stay a capsule, not swallow the row's gutters.
const _: () = assert!(HIGHLIGHT_INSET < PADDING_H);

/// The macOS [`MenuItemRecipe`] — public so an app can start from it and
/// tune one dimension without rebuilding the whole style.
pub fn macos_menu_item_recipe() -> MenuItemRecipe {
    MenuItemRecipe {
        item_height: ITEM_HEIGHT,
        padding_horizontal: PADDING_H,
        padding_leading: PADDING_H,
        icon_column_width: ICON_COLUMN,
        icon_label_gap: ICON_LABEL_GAP,
        shortcut_left_gap: SHORTCUT_GAP,
        separator_height: SEPARATOR_HEIGHT,
        item_corner_radius: HIGHLIGHT_RADIUS,
    }
}

/// macOS `MenuItemStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsMenuItemStyle;

impl MenuItemStyle for MacOsMenuItemStyle {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let bg = row_surface(
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_highlighted,
            &cfg.is_disabled,
        );
        let backdrop = ctx.add(
            RectWidget::new()
                .background(bg)
                .corner_radius(CornerRadius::uniform(HIGHLIGHT_RADIUS)),
        );
        // Inset so the capsule floats inside the menu rather than running
        // edge to edge — the Big Sur menu shape.
        let inset_backdrop =
            ctx.add(Padding::new(0.0, HIGHLIGHT_INSET, 0.0, HIGHLIGHT_INSET).child_id(backdrop));

        // Delegate the row arithmetic, with the interaction signals held
        // low so the shipped recipe's own tint never paints under ours.
        let quiet = Signal::new(false);
        let inner_cfg = MenuItemStyleConfig {
            label: cfg.label,
            leading: cfg.leading,
            trailing: cfg.trailing,
            is_hovered: quiet.clone(),
            is_pressed: quiet.clone(),
            is_focused: cfg.is_focused.clone(),
            is_disabled: cfg.is_disabled.clone(),
            is_highlighted: quiet,
        };
        let row = RecipeMenuItemStyle::new(macos_menu_item_recipe()).make_body(&inner_cfg, ctx);

        ctx.add(ZStack::new().add_child(inset_backdrop).add_child(row))
    }

    fn highlighted_label_role(&self) -> Option<TextRole> {
        // `selectedMenuItemTextColor` — white on the accent fill, in both
        // appearances. Without this the label would stay `labelColor` and
        // sit at roughly 3.5:1 on a saturated blue.
        Some(TextRole::OnAccent)
    }
}

/// `Pressed > Hover | Highlighted > nothing`, with disabled always inert.
///
/// AppKit does not distinguish a pressed menu row from a highlighted one
/// visually — the row is already fully filled — so press only deepens the
/// accent by one step.
fn row_surface(
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_highlighted: &Signal<bool>,
    is_disabled: &Signal<bool>,
) -> Signal<SurfaceRole> {
    is_pressed
        .zip3(is_hovered, is_highlighted)
        .zip(is_disabled)
        .map(|((pressed, hovered, highlighted), disabled)| {
            if *disabled {
                SurfaceRole::Transparent
            } else if *pressed {
                SurfaceRole::AccentPressed
            } else if *hovered || *highlighted {
                SurfaceRole::Accent
            } else {
                SurfaceRole::Transparent
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::ColorTokens;

    #[test]
    fn the_recipe_carries_the_macos_menu_metrics() {
        let r = macos_menu_item_recipe();
        assert_eq!(r.item_height, 22.0);
        assert_eq!(r.padding_horizontal, PADDING_H);
        assert_eq!(r.padding_leading, PADDING_H);
        assert_eq!(r.item_corner_radius, HIGHLIGHT_RADIUS);
    }

    /// A macOS menu is *tighter* than a Fluent one (37 dp) and near the
    /// IntUI default — density is the whole point of the platform's menus.
    #[test]
    fn rows_are_tighter_than_the_fluent_menu() {
        assert!(macos_menu_item_recipe().item_height < 37.0);
    }

    #[test]
    fn the_highlight_is_the_accent_not_a_neutral_wash() {
        // The single loudest difference from the sibling presets.
        let hovered = Signal::new(true);
        let quiet = Signal::new(false);
        let role = row_surface(&quiet, &hovered, &quiet, &quiet);
        assert_eq!(role.get(), SurfaceRole::Accent);
    }

    #[test]
    fn the_surface_cascade_is_ordered() {
        let pressed = Signal::new(false);
        let hovered = Signal::new(false);
        let highlighted = Signal::new(false);
        let disabled = Signal::new(false);
        let role = row_surface(&pressed, &hovered, &highlighted, &disabled);

        assert_eq!(role.get(), SurfaceRole::Transparent);
        // Keyboard highlight alone fills the row, exactly like hover.
        highlighted.set(true);
        assert_eq!(role.get(), SurfaceRole::Accent);
        hovered.set(true);
        assert_eq!(role.get(), SurfaceRole::Accent);
        pressed.set(true);
        assert_eq!(role.get(), SurfaceRole::AccentPressed);
        disabled.set(true);
        assert_eq!(role.get(), SurfaceRole::Transparent, "disabled beats all");
    }

    /// The pairing the whole style depends on.
    ///
    /// The flip is what makes a filled row legible, and it is **Aqua**
    /// that needs it: `labelColor` there is 85 % *black*, which lands at
    /// roughly 3.5:1 on a saturated accent. In Dark Aqua `labelColor` is
    /// already white, so the flip is close to a no-op — which is worth
    /// pinning, because it means anyone testing this on a dark screen
    /// alone would conclude the hook was unnecessary.
    #[test]
    fn the_highlighted_label_clears_contrast_on_the_fill() {
        assert_eq!(
            MacOsMenuItemStyle.highlighted_label_role(),
            Some(TextRole::OnAccent)
        );
        for theme in [crate::light(), crate::dark()] {
            let c: &ColorTokens = &theme.colors;
            let flipped = crate::palette::over(TextRole::OnAccent.resolve(c), c.accent);
            assert!(
                flipped.contrast_ratio(c.accent) >= 4.5,
                "the flipped label is only {:.2}:1",
                flipped.contrast_ratio(c.accent)
            );
        }

        // …and in Aqua specifically, not flipping would fail.
        let c = crate::light().colors;
        let unflipped = crate::palette::over(TextRole::Primary.resolve(&c), c.accent);
        assert!(
            unflipped.contrast_ratio(c.accent) < 4.5,
            "an Aqua label no longer needs flipping — the hook could go"
        );
    }
}
