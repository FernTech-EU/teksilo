// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze menu item.
//!
//! A row is transparent at rest. Hovered or reached with the arrow keys it
//! is washed with the focus colour at 30 % and outlined in the focus colour
//! moved 15 % toward the window text, a 1 px outline stroked on the row
//! inset by half the pen at radius 4.5; held down it is filled with the
//! solid colour and its label and shortcut turn to the text on it
//! ([`MenuItemStyle::pressed_label_role`]), while a highlighted label stays
//! the window text. The highlight covers the whole row, which `MenuList`
//! already places 4 px inside the menu's frame, Breeze's menu margin. A
//! disabled row is never highlighted.
//!
//! The row is 29 px at Plasma's default font: `max(H, 20, 16) + 1` and the
//! 4 px item margin above and below, through the density ladder. Its layout
//! is Breeze's for a menu that shows icons, because `MenuItem` reserves its
//! leading column on every row: the icon column 12 px inside the row (the
//! 4 px margin and the 8 px text margin), 16 px wide, the text 3 px after
//! it, and at the trailing edge a 20 px arrow box 4 px inside the row with
//! the text ending 3 px before it. A separator is 5 px, its 1 px line in the
//! middle. The row arithmetic is the shipped [`RecipeMenuItemStyle`]'s,
//! built with these numbers ([`breeze_menu_item_recipe_for`]); its own
//! highlight is held off, so the one painted here is the only one.
//!
//! The style stores the recipe it was built with from the theme's input
//! tokens and typography, so [`MenuItemStyle::metrics`], which `MenuItem`
//! and `MenuSeparator` read for the parts they build themselves, follows
//! the density the theme was projected to.
//!
//! ## Where this departs from Breeze
//!
//! - **D5, the highlight outline.** The outline marks the row the keyboard
//!   is on, so it is the menu's focus indicator. Breeze Light's, 57,154,204
//!   (its focus colour 61,174,233 moved toward the window text), is 2.9:1
//!   on the menu's frame background and 2.8:1 on the window; it is lifted in
//!   luma, keeping its hue, until it reaches SC 1.4.11's 3:1 (4.5:1 under
//!   high contrast) on the window, the view and the frame background
//!   ([`BreezeMenuColors::outline`]). Breeze Dark's passes and is kept.
//! - **D1, the pressed fill.** Breeze fills a pressed row with the focus
//!   colour and writes its text in the selection text. Teksilo's selection
//!   text is chosen against the accent (D1 of the colour mapping), so the
//!   pressed row is filled and outlined with `accent`, which the label's
//!   [`TextRole::OnAccent`] is readable on. In the stock Breeze schemes the
//!   focus colour and the accent are the same colour; they part when an
//!   accent colour or the high-contrast projection changes the accent.
//!
//! What `MenuItem`, `MenuSeparator` and `MenuList` build themselves is
//! theirs, and the crate's known limitations list where it differs from
//! Breeze: the shortcut's colour, the separator's length, section titles,
//! a menu without icons and the check mark.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{MenuItemMetrics, MenuItemStyle, MenuItemStyleConfig, Theme};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::math::lift_luma_until;
use teksilo_kde_scheme::{ColorState, KdeColor};
use teksilo_tokens::{Color, CornerRadius, InputTokens, TextRole, TypographyTokens};
use teksilo_widgets::primitives::{MinSize, ZStack};
use teksilo_widgets::styles::{MenuItemRecipe, RecipeMenuItemStyle};

use crate::color::{AA, ENHANCED};
use crate::palette::KdePalette;
use crate::shape::BREEZE_STROKED_FRAME_RADIUS;
use crate::styles::metrics::{MENU_HIGHLIGHT_ALPHA, breeze_menu_metrics_for};

/// The Breeze [`MenuItemRecipe`] at the density of `tokens`, for
/// `typography`: the row arithmetic of [`RecipeMenuItemStyle`] with the
/// dimensions of [`crate::styles::metrics::BreezeMenuMetrics`].
pub fn breeze_menu_item_recipe_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> MenuItemRecipe {
    let m = breeze_menu_metrics_for(tokens, typography);
    MenuItemRecipe {
        item_height: m.item_height,
        // The trailing column is the row's right-hand padding: the shortcut
        // and the arrow live in it.
        padding_horizontal: m.trailing_column,
        padding_leading: m.padding_leading,
        icon_column_width: m.icon_column,
        icon_label_gap: m.icon_label_gap,
        shortcut_left_gap: m.shortcut_gap,
        separator_height: m.separator_height,
        // `[Hp]` Only the radius of the recipe's own highlight, which this
        // style holds off.
        item_corner_radius: BREEZE_STROKED_FRAME_RADIUS,
    }
}

/// [`breeze_menu_item_recipe_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_menu_item_recipe() -> MenuItemRecipe {
    breeze_menu_item_recipe_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// Breeze `MenuItemStyle`.
///
/// [`Default`] is the Compact density and the Breeze typography; the preset
/// installs [`BreezeMenuItemStyle::for_theme`] of the theme it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeMenuItemStyle {
    /// The row dimensions this style lays out and reports.
    pub recipe: MenuItemRecipe,
}

impl BreezeMenuItemStyle {
    /// The style with `recipe`'s dimensions.
    pub fn new(recipe: MenuItemRecipe) -> Self {
        Self { recipe }
    }

    /// The style at the density and typography of `theme`.
    pub fn for_theme(theme: &Theme) -> Self {
        Self::new(breeze_menu_item_recipe_for(&theme.input, &theme.typography))
    }
}

impl Default for BreezeMenuItemStyle {
    fn default() -> Self {
        Self::new(breeze_menu_item_recipe())
    }
}

impl MenuItemStyle for BreezeMenuItemStyle {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let state = MenuRowState::derive(
            &cfg.is_disabled,
            &cfg.is_pressed,
            &cfg.is_hovered,
            &cfg.is_highlighted,
        );
        let highlight = ctx.add(BreezeMenuHighlight { state });
        // The recipe's row, with its interaction signals held low so its own
        // highlight never paints under this one.
        let quiet = Signal::new(false);
        let inner = MenuItemStyleConfig {
            label: cfg.label,
            leading: cfg.leading,
            trailing: cfg.trailing,
            is_hovered: quiet.clone(),
            is_pressed: quiet.clone(),
            is_focused: cfg.is_focused.clone(),
            is_disabled: cfg.is_disabled.clone(),
            is_highlighted: quiet,
        };
        let row = RecipeMenuItemStyle::new(self.recipe).make_body(&inner, ctx);
        let stack = ctx.add(ZStack::new().child(highlight).child(row));
        // `[S]` The row is at least its height whatever its label measures:
        // Breeze's rule takes the larger of the line box and 20 px.
        ctx.add(MinSize::new(0.0, self.recipe.item_height).child(stack))
    }

    fn metrics(&self) -> MenuItemMetrics {
        RecipeMenuItemStyle::new(self.recipe).metrics()
    }

    /// `[wcag]` D1: the text on the pressed row's accent fill. Breeze writes
    /// a highlighted row in the window text, which the 30 % wash keeps
    /// readable, so [`MenuItemStyle::highlighted_label_role`] stays `None`.
    fn pressed_label_role(&self) -> Option<TextRole> {
        Some(TextRole::OnAccent)
    }
}

/// A menu row's interaction state, in Breeze's precedence order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuRowState {
    /// Neither hovered nor reached by the keyboard.
    #[default]
    Rest,
    /// Hovered, reached with the arrow keys, or a submenu trigger whose
    /// submenu is open.
    Highlighted,
    /// Held down.
    Pressed,
    /// Disabled: never highlighted.
    Disabled,
}

impl MenuRowState {
    /// Collapse the style configuration's booleans into a state: disabled
    /// first, then pressed, then hovered or highlighted.
    pub fn derive(
        is_disabled: &Signal<bool>,
        is_pressed: &Signal<bool>,
        is_hovered: &Signal<bool>,
        is_highlighted: &Signal<bool>,
    ) -> Signal<MenuRowState> {
        is_disabled
            .zip3(is_pressed, is_hovered)
            .zip(is_highlighted)
            .map(|((disabled, pressed, hovered), highlighted)| {
                if *disabled {
                    MenuRowState::Disabled
                } else if *pressed {
                    MenuRowState::Pressed
                } else if *hovered || *highlighted {
                    MenuRowState::Highlighted
                } else {
                    MenuRowState::Rest
                }
            })
    }
}

/// The colours a Breeze menu row is highlighted in, in one paint state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeMenuColors {
    /// `[Hp]` The view's focus decoration of the current window state, which
    /// washes a highlighted row at 30 %.
    pub focus: Color,
    /// `[wcag]` D5: a highlighted row's outline, the focus colour moved 15 %
    /// toward the window text, lifted in luma to the non-text floor on the
    /// window, the view and the frame background.
    pub outline: Color,
    /// `[wcag]` D1: a pressed row's fill and outline, `accent`.
    pub pressed: Color,
}

impl BreezeMenuColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference.
    ///
    /// `theme` is the theme the paint walker hands a widget, already
    /// projected for that state. Without a [`KdePalette`] the focus colour
    /// and the outline are `border_focused`.
    pub fn resolve(theme: &Theme, window_active: bool, high_contrast: bool) -> BreezeMenuColors {
        let c = &theme.colors;
        let Some(palette) = theme.extension::<KdePalette>() else {
            return BreezeMenuColors {
                focus: c.border_focused,
                outline: c.border_focused,
                pressed: c.accent,
            };
        };
        let state = if window_active {
            ColorState::Active
        } else {
            ColorState::Inactive
        };
        let b = palette.breeze_colors(state, high_contrast);
        let floor = if high_contrast { ENHANCED } else { AA }.non_text;
        let on = [c.surface_main, c.surface_content, c.surface_raised].map(KdeColor::from_color);
        BreezeMenuColors {
            focus: b.focus,
            outline: lift_luma_until(KdeColor::from_color(b.focus_outline), &on, floor).to_color(),
            pressed: c.accent,
        }
    }

    /// `[Hp]` A highlighted row's wash: the focus colour at 30 %.
    pub fn wash(&self) -> Color {
        self.focus.with_alpha(MENU_HIGHLIGHT_ALPHA)
    }
}

/// What a menu row paints in one state: a fill and a 1 px outline, each
/// optional.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuRowPaint {
    /// The fill of the row, or nothing.
    pub fill: Option<Color>,
    /// The outline of the row, or nothing.
    pub outline: Option<Color>,
}

/// `[Hp]` What a row paints in `state`, from `c`.
pub fn menu_row_paint(state: MenuRowState, c: &BreezeMenuColors) -> MenuRowPaint {
    match state {
        MenuRowState::Rest | MenuRowState::Disabled => MenuRowPaint {
            fill: None,
            outline: None,
        },
        MenuRowState::Highlighted => MenuRowPaint {
            fill: Some(c.wash()),
            outline: Some(c.outline),
        },
        MenuRowState::Pressed => MenuRowPaint {
            fill: Some(c.pressed),
            outline: Some(c.pressed),
        },
    }
}

/// `[Hp]` The rectangle a row's highlight is filled and stroked on: the row
/// inset by half the `pen`, so the outline lies inside the row.
pub fn highlight_rect(bounds: Rect, pen: f32) -> Rect {
    let d = pen * 0.5;
    Rect::new(
        bounds.x + d,
        bounds.y + d,
        (bounds.width - 2.0 * d).max(0.0),
        (bounds.height - 2.0 * d).max(0.0),
    )
}

/// The highlight layer of a Breeze menu row, painted behind the row's
/// content.
///
/// Sized like `RectWidget`: it takes whatever the proposal offers and
/// reports zero when unbounded, so it does not change the row's size.
struct BreezeMenuHighlight {
    state: Signal<MenuRowState>,
}

impl std::fmt::Debug for BreezeMenuHighlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeMenuHighlight").finish()
    }
}

impl Widget for BreezeMenuHighlight {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        self.state
            .bind_to(id, ctx.binding_registry(), BindingLevel::RepaintOnly);
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
        let colors =
            BreezeMenuColors::resolve(ctx.theme, ctx.window_active, ctx.prefers_high_contrast);
        let paint = menu_row_paint(self.state.get(), &colors);
        paint_menu_row(canvas, bounds, &paint, ctx.theme);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the `MenuItem` emits the real node.
        builder.set_hidden();
    }
}

/// Paint `paint` into the row `bounds`: the fill, then the outline, both on
/// [`highlight_rect`] at radius 4.5.
pub fn paint_menu_row(canvas: &mut Canvas, bounds: Rect, paint: &MenuRowPaint, theme: &Theme) {
    let pen = theme.shape.border_width;
    let rect = highlight_rect(bounds, pen);
    let corner = CornerRadius::uniform(crate::styles::chrome::frame_radius(&theme.shape));
    if let Some(fill) = paint.fill
        && fill.a() > 0.0
    {
        canvas.fill_rounded_rect(rect, corner, fill);
    }
    if let Some(outline) = paint.outline
        && outline.a() > 0.0
        && pen > 0.0
    {
        canvas.stroke_rounded_rect(rect, corner, outline, pen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::styles::PaintState;
    use teksilo_tokens::TargetDensity;
    use teksilo_tokens::contrast::composite_contrast;

    use crate::PlasmaAccent;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    fn colors(theme: &Theme) -> BreezeMenuColors {
        BreezeMenuColors::resolve(theme, true, false)
    }

    #[test]
    fn the_recipe_carries_breezes_menu_metrics() {
        let r = breeze_menu_item_recipe();
        assert_eq!(r.item_height, 29.0);
        assert_eq!(r.separator_height, 5.0);
        assert_eq!(r.padding_leading, 12.0);
        assert_eq!(r.icon_column_width, 16.0);
        assert_eq!(r.icon_label_gap, 3.0);
        assert_eq!(r.padding_horizontal, 27.0);
        assert_eq!(r.shortcut_left_gap, 16.0);
        assert_eq!(r.item_corner_radius, 4.5);
        assert_eq!(BreezeMenuItemStyle::default().recipe, r);
        assert_eq!(BreezeMenuItemStyle::for_theme(&crate::light()).recipe, r);
    }

    /// The metrics `MenuItem` and `MenuSeparator` read are the recipe's.
    #[test]
    fn the_metrics_are_the_recipes_at_every_density() {
        for (density, item, separator) in [
            (TargetDensity::Compact, 29.0, 5.0),
            (TargetDensity::Comfortable, 32.0, 5.75),
            (TargetDensity::Touch, 44.0, 6.5),
        ] {
            let theme = crate::light().with_density(density);
            let m = BreezeMenuItemStyle::for_theme(&theme).metrics();
            assert_eq!(m.item_height, item, "{density:?}");
            assert!((m.separator_height - separator).abs() < 1e-4, "{density:?}");
            assert_eq!(m.icon_column_width, 16.0);
            assert!(m.trailing_column_width >= 27.0);
        }
    }

    #[test]
    fn the_state_follows_breezes_precedence() {
        let disabled = Signal::new(false);
        let pressed = Signal::new(false);
        let hovered = Signal::new(false);
        let highlighted = Signal::new(false);
        let state = MenuRowState::derive(&disabled, &pressed, &hovered, &highlighted);
        assert_eq!(state.get(), MenuRowState::Rest);
        highlighted.set(true);
        assert_eq!(
            state.get(),
            MenuRowState::Highlighted,
            "the keyboard lights it"
        );
        highlighted.set(false);
        hovered.set(true);
        assert_eq!(
            state.get(),
            MenuRowState::Highlighted,
            "so does the pointer"
        );
        pressed.set(true);
        assert_eq!(state.get(), MenuRowState::Pressed);
        disabled.set(true);
        assert_eq!(state.get(), MenuRowState::Disabled, "disabled beats all");
    }

    #[test]
    fn only_a_highlighted_or_pressed_row_paints() {
        let c = colors(&crate::light());
        let nothing = MenuRowPaint {
            fill: None,
            outline: None,
        };
        assert_eq!(menu_row_paint(MenuRowState::Rest, &c), nothing);
        assert_eq!(menu_row_paint(MenuRowState::Disabled, &c), nothing);
        let lit = menu_row_paint(MenuRowState::Highlighted, &c);
        assert_eq!(lit.fill, Some(c.wash()));
        assert_eq!(lit.outline, Some(c.outline));
        let held = menu_row_paint(MenuRowState::Pressed, &c);
        assert_eq!(held.fill, Some(c.pressed));
        assert_eq!(held.outline, Some(c.pressed));
    }

    /// Breeze Light's and Dark's highlight, against Plasma's pixels: the
    /// wash over the window is 185,220,238 and 40,76,97; the outline is
    /// 57,154,204 and 90,186,236.
    #[test]
    fn the_wash_and_outline_are_breezes() {
        let over = |top: Color, bottom: Color| {
            let a = top.a();
            let ch = |t: f32, b: f32| t * a + b * (1.0 - a);
            Color::new(
                ch(top.r(), bottom.r()),
                ch(top.g(), bottom.g()),
                ch(top.b(), bottom.b()),
                1.0,
            )
        };
        let light = crate::light();
        let c = colors(&light);
        assert_eq!(rgba8(c.focus), [61, 174, 233, 255]);
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1);
        assert!(near(
            rgba8(over(c.wash(), light.colors.surface_main)),
            [185, 220, 238, 255]
        ));
        let dark = crate::dark();
        let d = colors(&dark);
        assert!(near(
            rgba8(over(d.wash(), dark.colors.surface_main)),
            [40, 76, 97, 255]
        ));
        // Breeze Dark's outline reaches 3:1 and is Breeze's own.
        assert_eq!(rgba8(d.outline), [90, 186, 236, 255]);
        assert_eq!(rgba8(c.pressed), rgba8(light.colors.accent));
    }

    /// D5: Breeze Light's outline is lifted to 3:1 on the menu's backdrops,
    /// keeping its hue, and its own outline does not reach it.
    #[test]
    fn the_highlight_outline_is_lifted_to_three_to_one() {
        let t = crate::light();
        let palette = t.extension::<KdePalette>().unwrap();
        let vendor = palette.colors(ColorState::Active).focus_outline;
        assert_eq!(rgba8(vendor), [57, 154, 204, 255]);
        let c = &t.colors;
        let backdrops = [c.surface_main, c.surface_content, c.surface_raised];
        let worst = |x: Color| {
            backdrops
                .iter()
                .map(|b| composite_contrast(x, *b))
                .fold(f32::INFINITY, f32::min)
        };
        assert!(worst(vendor) < 3.0, "premise: {:.2}:1", worst(vendor));
        let lifted = colors(&t).outline;
        assert!(worst(lifted) >= 3.0, "{:.2}:1", worst(lifted));
        let (h0, _, _) = vendor.to_hsv();
        let (h1, _, _) = lifted.to_hsv();
        assert!((h0 - h1).abs() < 2.0, "the hue moved from {h0} to {h1}");
        // Under high contrast the floor is 4.5:1.
        let hc = t.for_paint_state(PaintState {
            window_active: true,
            high_contrast: true,
        });
        let hc_colors = BreezeMenuColors::resolve(&hc, true, true);
        let hc_backdrops = [
            hc.colors.surface_main,
            hc.colors.surface_content,
            hc.colors.surface_raised,
        ];
        for b in hc_backdrops {
            assert!(composite_contrast(hc_colors.outline, b) >= 4.5);
        }
    }

    /// With every Plasma accent, in both schemes and both window states, the
    /// window text stays readable on the wash and the pressed label on the
    /// pressed fill, and the outline reaches 3:1.
    #[test]
    fn every_swatch_keeps_the_rows_readable() {
        for accent in PlasmaAccent::ALL {
            for base in [
                crate::light_with_accent(accent.color()),
                crate::dark_with_accent(accent.color()),
            ] {
                for window_active in [true, false] {
                    let t = base.for_paint_state(PaintState {
                        window_active,
                        high_contrast: false,
                    });
                    let c = BreezeMenuColors::resolve(&t, window_active, false);
                    let panel = t.colors.surface_raised;
                    let wash = {
                        let w = c.wash();
                        let a = w.a();
                        Color::new(
                            w.r() * a + panel.r() * (1.0 - a),
                            w.g() * a + panel.g() * (1.0 - a),
                            w.b() * a + panel.b() * (1.0 - a),
                            1.0,
                        )
                    };
                    assert!(
                        composite_contrast(t.colors.text_primary, wash) >= 4.5,
                        "{accent:?}: the label on the wash"
                    );
                    assert!(
                        composite_contrast(TextRole::OnAccent.resolve(&t.colors), c.pressed) >= 4.5,
                        "{accent:?}: the pressed label"
                    );
                    assert!(composite_contrast(c.outline, panel) >= 3.0);
                }
            }
        }
    }

    #[test]
    fn the_rows_degrade_without_the_palette() {
        let mut bare = crate::light();
        bare.extensions.remove::<KdePalette>();
        let c = colors(&bare);
        assert_eq!(c.focus, bare.colors.border_focused);
        assert_eq!(c.outline, bare.colors.border_focused);
        assert_eq!(c.pressed, bare.colors.accent);
    }

    #[test]
    fn the_highlight_is_stroked_inside_the_row() {
        let r = highlight_rect(Rect::new(4.0, 4.0, 200.0, 29.0), 1.0);
        assert_eq!(r, Rect::new(4.5, 4.5, 199.0, 28.0));
    }
}
