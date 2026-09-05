// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `MenuItemStyle` impl driven by paint-recipe data.
//!
//! `RecipeMenuItemStyle` ships the IntUI menu-row chrome:
//! transparent at rest, accent-subtle tint on hover (or highlight via
//! keyboard navigation), pressed surface tint while clicked. The row
//! is composed as `[leading?] [icon-label gap] [label] [Spacer]
//! [trailing?]`, padded vertically to `item_height` and horizontally
//! to `item_padding_horizontal`. The trailing slot's right edge IS
//! the row's right edge — the slot widget should reserve its own
//! right-padding column (typical: shortcut + chevron column inside
//! a HStack).
//!
//! Apps that want a different look (Windows-11 row, macOS pull-down,
//! brutalist square row) write their own `impl MenuItemStyle` block.

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{MenuItemMetrics, MenuItemStyle, MenuItemStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, SurfaceRole};

use crate::primitives::{Expand, FixedSize, HStack, MinSize, Padding, RectWidget, Spacer, ZStack};

// IntUI design tokens for MenuItem / MenuList rows. The recipe owns
// its own dimensions. The MenuList / MenuBar / ComboBox panel widgets
// import these constants when they need menu-related row dimensions
// (item height, separator height). The menu *panel* surface (corner
// radius, border, shadow density) is owned by `PopoverStyle` (the
// `Menu` variant).
pub const MENU_ITEM_HEIGHT: f32 = 24.0;
/// Right-side padding column (also used as chevron column width).
pub const MENU_ITEM_PADDING_HORIZONTAL: f32 = 12.0;
/// Leading-side padding before the icon/check column.
pub const MENU_ITEM_PADDING_LEADING: f32 = 6.0;
pub const MENU_ICON_COLUMN_WIDTH: f32 = 16.0;
pub const MENU_ICON_LABEL_GAP: f32 = 6.0;
pub const MENU_SHORTCUT_LEFT_GAP: f32 = 24.0;
pub const MENU_SEPARATOR_HEIGHT: f32 = 9.0;
/// Corner radius of the per-row hover / pressed highlight rect.
pub const MENU_ITEM_CORNER_RADIUS: f32 = 8.0;

/// Recipe dimensions for [`RecipeMenuItemStyle`].
///
/// All fields default to the corresponding module-level `pub const`.
/// Override individual fields to tune the menu row without writing a full
/// custom `MenuItemStyle` impl.
///
/// Four of them describe parts of the row this style does not build itself
/// — the leading icon column and the trailing chevron column are slots
/// `MenuItem` fills before any style runs, and the separator belongs to
/// `MenuSeparator`. They reach those call sites through
/// [`MenuItemStyle::metrics`], so a style that delegates its row here
/// should delegate `metrics` here too.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuItemRecipe {
    pub item_height: f32,
    pub padding_horizontal: f32,
    pub padding_leading: f32,
    pub icon_column_width: f32,
    pub icon_label_gap: f32,
    pub shortcut_left_gap: f32,
    pub separator_height: f32,
    pub item_corner_radius: f32,
}

impl Default for MenuItemRecipe {
    fn default() -> Self {
        Self {
            item_height: MENU_ITEM_HEIGHT,
            padding_horizontal: MENU_ITEM_PADDING_HORIZONTAL,
            padding_leading: MENU_ITEM_PADDING_LEADING,
            icon_column_width: MENU_ICON_COLUMN_WIDTH,
            icon_label_gap: MENU_ICON_LABEL_GAP,
            shortcut_left_gap: MENU_SHORTCUT_LEFT_GAP,
            separator_height: MENU_SEPARATOR_HEIGHT,
            item_corner_radius: MENU_ITEM_CORNER_RADIUS,
        }
    }
}

/// Default `MenuItemStyle` shipped with Teksilo. Chrome roles come from
/// the active theme.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeMenuItemStyle {
    pub recipe: MenuItemRecipe,
}

impl RecipeMenuItemStyle {
    pub fn new(recipe: MenuItemRecipe) -> Self {
        Self { recipe }
    }
}

impl MenuItemStyle for RecipeMenuItemStyle {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let recipe = self.recipe;

        // Row composition: leading | gap | label | Spacer | trailing.
        // HStack spacing is 0 — the only inter-child gap is between
        // leading and label (`icon_label_gap`); everything else is
        // tight (Spacer handles stretch).
        let mut row = HStack::new().spacing(0.0);

        if let Some(leading) = cfg.leading {
            row = row.add_child(leading);
            // Explicit icon-to-label gap. Fixed-width Spacer rather than
            // HStack::spacing so we don't inject gaps around every other
            // child (which would push the trailing slot inward).
            let gap_spacer = ctx.add(Spacer::new());
            let gap = ctx.add(
                FixedSize::new()
                    .width(recipe.icon_label_gap)
                    .height(1.0_f32)
                    .child_id(gap_spacer),
            );
            row = row.add_child(gap);
        }

        row = row.add_child(cfg.label);
        // MinSize ensures the shortcut never abuts the label when the menu
        // is narrower than label + shortcut combined.
        row = row.child(MinSize::width(recipe.shortcut_left_gap).child(Spacer::new()));

        if let Some(trailing) = cfg.trailing {
            row = row.add_child(trailing);
        }

        let row_id = ctx.add(row);

        // Padding: vertical derived so the row has the full
        // `item_height` after the body line height. Horizontal: only
        // left padding here — the trailing slot is responsible for
        // its own right-padding column (matches the pre-refactor
        // MenuItem semantic so submenu and regular items line up).
        let body = &ctx.theme().typography.body;
        let body_line = body.size * body.line_height;
        let pad_v = ((recipe.item_height - body_line) * 0.5).max(0.0);
        let padding =
            ctx.add(Padding::new(pad_v, 0.0, pad_v, recipe.padding_leading).child_id(row_id));

        // Background — Hover / Highlighted both use AccentSubtle (the
        // same row tint), Pressed uses Pressed, Disabled stays
        // Transparent.
        let bg_role = bg_signal(
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_highlighted,
            &cfg.is_disabled,
        );
        let bg = ctx.add(
            RectWidget::new()
                .background(bg_role)
                .corner_radius(CornerRadius::uniform(recipe.item_corner_radius)),
        );

        let stack = ctx.add(ZStack::new().add_child(bg).add_child(padding));

        // Claim the full width the row is given.
        //
        // The row's own arithmetic already assumes this — the trailing slot's
        // right edge IS the row's right edge, and every label in a menu lines
        // up on one leading inset — but a bare `ZStack` reports its children's
        // *intrinsic* max width, so it only filled by accident: the internal
        // `Spacer` made the padded content flexible, and the enclosing
        // `MenuList` stretched it.
        //
        // That accident stops the moment a preset *layers* this row (Fluent
        // and macOS both stack it over their own highlight rect to swap one
        // colour). The outer `ZStack` measures this one at its intrinsic
        // width, finds it narrower than the panel, and CENTRES it — so every
        // label landed on a different inset, ragged by half the difference
        // between its own text width and the widest row's. `respect_intrinsic`
        // keeps the natural width as the floor, so the panel still sizes to
        // its widest item; the fill is what the row was always meant to do.
        ctx.add(Expand::horizontal().respect_intrinsic().child_id(stack))
    }

    fn metrics(&self) -> MenuItemMetrics {
        MenuItemMetrics {
            icon_column_width: self.recipe.icon_column_width,
            trailing_column_width: self.recipe.padding_horizontal,
            separator_height: self.recipe.separator_height,
            item_height: self.recipe.item_height,
        }
    }
}

fn bg_signal(
    is_pressed: &Signal<bool>,
    is_hovered: &Signal<bool>,
    is_highlighted: &Signal<bool>,
    is_disabled: &Signal<bool>,
) -> Signal<SurfaceRole> {
    let combined = is_pressed.zip3(is_hovered, is_highlighted);
    combined
        .zip(is_disabled)
        .map(|((pressed, hovered, highlighted), disabled)| {
            if *disabled {
                SurfaceRole::Transparent
            } else if *pressed {
                SurfaceRole::Pressed
            } else if *hovered || *highlighted {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::{MockTextBackend, SizeProposal};
    use teksilo_core::styles::MenuItemStyleConfig;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    use crate::menu_item::MenuItem;
    use crate::menu_list::{MenuList, MenuSeparator};
    use crate::primitives::FixedSize;

    /// A style shaped like the ones the Fluent and macOS presets ship: paint
    /// our own row highlight, then stack the shared recipe row on top of it
    /// to reuse its slot arithmetic.
    #[derive(Debug, Default, Clone, Copy)]
    struct LayeringStyle;

    impl MenuItemStyle for LayeringStyle {
        fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
            let backdrop = ctx.add(RectWidget::new().background(SurfaceRole::Hover));
            let row = RecipeMenuItemStyle::default().make_body(cfg, ctx);
            ctx.add(ZStack::new().add_child(backdrop).add_child(row))
        }
    }

    /// Leading x of each painted text row, keyed by the row's y.
    fn label_left_edges(tree: &mut WidgetTree) -> Vec<(i32, i32)> {
        let mut rows: std::collections::BTreeMap<i32, f32> = std::collections::BTreeMap::new();
        let frame = tree.render();
        for g in &frame.glyphs {
            let e = rows.entry(g.screen[1].round() as i32).or_insert(f32::MAX);
            *e = e.min(g.screen[0]);
        }
        rows.into_iter()
            .map(|(y, x)| (y, x.round() as i32))
            .collect()
    }

    /// Two rows, labels of very different widths, in a menu far wider than
    /// either — under a style that layers the shared row over its own
    /// highlight.
    fn two_row_menu(style: bool) -> Vec<(i32, i32)> {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                MockTextBackend::new(),
            )));
        let mut short = MenuItem::new(lit!("Cut"));
        let mut long = MenuItem::new(lit!("Save all as…"));
        if style {
            short = short.style(LayeringStyle);
            long = long.style(LayeringStyle);
        }
        let menu = MenuList::new().item(short).item(long);
        tree.add(FixedSize::new().width(400.0).child(menu));
        tree.layout(SizeProposal::exact(600.0, 300.0));
        label_left_edges(&mut tree)
    }

    /// Every label in a menu starts on one leading inset — the reserved
    /// icon/check column exists precisely so an icon-less row lines up with
    /// an icon'd one. WinUI, AppKit and Material all leading-align them;
    /// none centres.
    ///
    /// The regression this pins is not the plain row (which was always
    /// right) but the *layered* one. `ZStack` sizes to its children's
    /// intrinsic max and centres a child narrower than its bounds, so a
    /// preset that stacked this row over its own highlight rect got each
    /// label pushed in by half its own slack — "Cut" further right than
    /// "Save all as…", ragged by label width, in Fluent and macOS both.
    #[test]
    fn labels_share_one_leading_inset_under_a_layering_style() {
        let rows = two_row_menu(true);
        assert_eq!(rows.len(), 2, "two rows painted: {rows:?}");
        assert_eq!(
            rows[0].1, rows[1].1,
            "labels of different widths must start at the same x: {rows:?}"
        );
    }

    /// …and the unlayered row is unchanged by the fill, on the same inset.
    #[test]
    fn a_layering_style_lands_on_the_same_inset_as_the_plain_row() {
        let plain = two_row_menu(false);
        let layered = two_row_menu(true);
        assert_eq!(plain, layered, "layering must not move the labels");
    }

    // --- `MenuItemStyle::metrics` ---

    /// A recipe whose every dimension differs from the IntUI default, so a
    /// call site that ignored it and used the module constant is caught.
    fn odd_recipe() -> MenuItemRecipe {
        MenuItemRecipe {
            item_height: 40.0,
            padding_leading: 7.0,
            icon_column_width: 30.0,
            icon_label_gap: 5.0,
            padding_horizontal: 21.0,
            separator_height: 17.0,
            ..MenuItemRecipe::default()
        }
    }

    fn odd_style() -> RecipeMenuItemStyle {
        RecipeMenuItemStyle::new(odd_recipe())
    }

    /// Leading x of the single row's label, which is `padding_leading +
    /// icon_column_width + icon_label_gap` from the row's left edge.
    fn single_row_label_x(style: Option<RecipeMenuItemStyle>) -> i32 {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                MockTextBackend::new(),
            )));
        let mut item = MenuItem::new(lit!("Cut"));
        if let Some(s) = style {
            item = item.style(s);
        }
        tree.add(item);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        label_left_edges(&mut tree)[0].1
    }

    /// The leading column is built by `MenuItem`, before any style runs, so
    /// its width has to be *read from* the style. It used to be the IntUI
    /// module constant, which made `MenuItemRecipe::icon_column_width` a
    /// knob that changed nothing — macOS asked for 14 dp and got 16.
    #[test]
    fn the_icon_column_comes_from_the_active_style() {
        assert_eq!(single_row_label_x(None), 6 + 16 + 6);
        assert_eq!(single_row_label_x(Some(odd_style())), 7 + 30 + 5);
    }

    /// The separator is a sibling widget the style never sees, so it reads
    /// the theme slot — the only style it can share with the rows around it.
    #[test]
    fn the_separator_height_comes_from_the_active_style() {
        for (slot, expected) in [(None, MENU_SEPARATOR_HEIGHT), (Some(odd_style()), 17.0)] {
            let mut theme = teksilo_core::presets::intui::light();
            theme.style_slots.menu_item =
                slot.map(|s| std::rc::Rc::new(s) as teksilo_core::styles::SharedMenuItemStyle);
            let mut tree = WidgetTree::new().with_theme(theme);
            let id = tree.add(MenuSeparator);
            // Height left open: an `exact` proposal stretches the root to the
            // window, which would hide the separator's own answer.
            tree.layout(SizeProposal {
                width: Some(400.0),
                height: None,
            });
            assert_eq!(tree.bounds(id).height, expected);
        }
    }

    /// `metrics` and `make_body` must describe the same row: the trailing
    /// column doubles as the row's right-hand padding, so they share one
    /// recipe field.
    #[test]
    fn the_metrics_mirror_the_recipe() {
        let m = odd_style().metrics();
        assert_eq!(m.icon_column_width, 30.0);
        assert_eq!(m.trailing_column_width, 21.0);
        assert_eq!(m.separator_height, 17.0);
        assert_eq!(m.item_height, 40.0);
    }
}
