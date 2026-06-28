// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `ButtonStyle` impl driven by `ButtonRecipe` data per
//! variant. Holds a `HashMap<ButtonVariant, ButtonRecipe>` and looks
//! up the recipe at paint time.
//!
//! Apps retheme buttons by constructing a fresh map of recipes (e.g.
//! a Material-3 ButtonRecipe set with rounded-edge filled buttons)
//! and installing it per-call (`Button::style(MyStyle)`) or
//! theme-wide (`theme.style_slots.button = Some(Rc::new(MyStyle))`).
//! The Button widget never sees the recipe; it only knows about the
//! trait.

use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{EdgeInsets, Size};
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    BorderRecipe, ButtonRecipe, ButtonStyle, ButtonStyleConfig, ButtonVariant, FillRecipe,
    PerStateRecipe, RecipeColor, ShapeRecipe, WidgetState,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, Color, CornerRadius, SurfaceRole, TextRole};

use crate::primitives::{MinSize, Padding, RectWidget, ZStack};

// IntUI design tokens for Button. The recipe and its consumers own
// these constants. Most button dimensions live inside per-variant
// `ButtonRecipe`s (padding, min size, etc.); the constants below
// cover globals (icon size, the icon ↔ label gap) that aren't
// variant-specific.
pub const BUTTON_HEIGHT: f32 = 24.0;
pub const BUTTON_MIN_WIDTH: f32 = 72.0;
pub const BUTTON_PADDING_HORIZONTAL: f32 = 14.0;
pub const BUTTON_PADDING_VERTICAL: f32 = 0.0;
pub const BUTTON_CORNER_RADIUS: f32 = 4.0;
pub const BUTTON_BORDER_WIDTH: f32 = 1.0;
pub const BUTTON_ICON_SIZE: f32 = 16.0;
pub const BUTTON_ICON_LABEL_GAP: f32 = 4.0;

/// Default `ButtonStyle` shipped with Bastyde.
///
/// Holds a `HashMap<ButtonVariant, ButtonRecipe>`. Variants that
/// aren't explicitly populated fall back to `ButtonVariant::Plain`
/// (the Int UI house default).
///
/// `label_roles` optionally redirects the label/icon color per variant
/// (see [`ButtonStyle::label_text_role`]). It is empty in the IntUI
/// default — the `Button`'s built-in mapping is kept — but
/// design-language presets (Material 3) populate it so e.g. text and
/// outlined buttons read in the accent color.
#[derive(Debug, Clone)]
pub struct RecipeButtonStyle {
    pub recipes: HashMap<ButtonVariant, ButtonRecipe>,
    pub label_roles: HashMap<ButtonVariant, TextRole>,
}

impl RecipeButtonStyle {
    /// IntUI's per-variant ButtonRecipe set.
    pub fn intui() -> Self {
        let mut recipes = HashMap::new();
        recipes.insert(ButtonVariant::Filled, intui_filled_recipe());
        // IntUI maps Destructive → Filled (the warning lives in the
        // dialog title/body, not the button).
        recipes.insert(ButtonVariant::Destructive, intui_filled_recipe());
        recipes.insert(ButtonVariant::Plain, intui_plain_recipe());
        // IntUI maps Tinted/Outlined → Plain.
        recipes.insert(ButtonVariant::Tinted, intui_plain_recipe());
        recipes.insert(ButtonVariant::Outlined, intui_plain_recipe());
        recipes.insert(ButtonVariant::Ghost, intui_ghost_recipe());
        // IntUI maps Link → Ghost.
        recipes.insert(ButtonVariant::Link, intui_ghost_recipe());
        // IntUI keeps the Button's built-in label-role mapping.
        Self {
            recipes,
            label_roles: HashMap::new(),
        }
    }
}

impl Default for RecipeButtonStyle {
    fn default() -> Self {
        Self::intui()
    }
}

impl ButtonStyle for RecipeButtonStyle {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Recipe lookup: requested variant > Plain fallback.
        let recipe = self
            .recipes
            .get(&cfg.variant)
            .or_else(|| self.recipes.get(&ButtonVariant::Plain))
            .expect("RecipeButtonStyle must define at least Plain")
            .clone();

        let state_signal = derive_state_signal(cfg);

        // Fill — convert (state, recipe) → reactive Color signal.
        let bg_color = bind_fill(&state_signal, &recipe.fill, ctx);
        let (border_color, border_width) = bind_border(&state_signal, &recipe.border, ctx);

        let radius = match recipe.shape {
            ShapeRecipe::Rect { corner_radius } => corner_radius,
            ShapeRecipe::Pill | ShapeRecipe::Circle => CornerRadius::uniform(9999.0),
        };

        let padding_id = ctx.add(
            Padding::new(
                recipe.padding.top,
                recipe.padding.trailing,
                recipe.padding.bottom,
                recipe.padding.leading,
            )
            .child_id(cfg.label),
        );

        let rect_id = ctx.add(
            RectWidget::new()
                .bind_background(bg_color)
                .bind_border_color(border_color)
                .bind_border_width(border_width)
                .corner_radius(radius),
        );

        let zstack_id = ctx.add(ZStack::new().add_child(rect_id).add_child(padding_id));

        ctx.add(MinSize::new(recipe.min_size.width, recipe.min_size.height).child_id(zstack_id))
    }

    fn label_text_role(&self, variant: ButtonVariant) -> Option<TextRole> {
        self.label_roles.get(&variant).copied()
    }
}

/// Derive a `Signal<WidgetState>` from the four booleans in
/// `ButtonStyleConfig`. Priority chain: Disabled > Pressed > Focused >
/// Hovered > Idle.
fn derive_state_signal(cfg: &ButtonStyleConfig) -> Signal<WidgetState> {
    cfg.is_disabled
        .zip3(&cfg.is_pressed, &cfg.is_hovered)
        .zip(&cfg.is_focused)
        .map(|((disabled, pressed, hovered), focused)| {
            if *disabled {
                WidgetState::Disabled
            } else if *pressed {
                WidgetState::Pressed
            } else if *focused {
                WidgetState::Focused
            } else if *hovered {
                WidgetState::Hovered
            } else {
                WidgetState::Idle
            }
        })
}

fn bind_fill(
    state: &Signal<WidgetState>,
    recipe: &PerStateRecipe<FillRecipe>,
    ctx: &BuildContext,
) -> ColorProp {
    let recipe = recipe.clone();
    let theme_sig = ctx.theme_signal();
    state
        .zip(&theme_sig)
        .map(move |(s, theme)| resolve_fill_to_color(recipe.resolve(*s), &theme.colors))
        .into()
}

fn bind_border(
    state: &Signal<WidgetState>,
    recipe: &PerStateRecipe<BorderRecipe>,
    ctx: &BuildContext,
) -> (ColorProp, Signal<f32>) {
    let recipe_for_color = recipe.clone();
    let recipe_for_width = recipe.clone();
    let theme_sig = ctx.theme_signal();
    let color: ColorProp = state
        .zip(&theme_sig)
        .map(move |(s, theme)| {
            recipe_for_color
                .resolve(*s)
                .color
                .resolve_with(&theme.colors)
        })
        .into();
    let width = state.map(move |s| recipe_for_width.resolve(*s).width);
    (color, width)
}

fn resolve_fill_to_color(fill: &FillRecipe, colors: &bastyde_tokens::ColorTokens) -> Color {
    // Solid / StateLayer / None resolve to a flat color. Gradient
    // variants have no flat form here and fall back to transparent —
    // they are painted via the SDF gradient pipeline once a `PaintProp`
    // carries them (see `resolve_fill_to_paint`).
    fill.resolve_flat(colors).unwrap_or(Color::TRANSPARENT)
}

// ─── IntUI per-variant recipe constructors ──────────────────────────

fn intui_filled_recipe() -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::rounded(4.0),
        fill: PerStateRecipe {
            idle: FillRecipe::solid(SurfaceRole::Accent),
            hover: Some(FillRecipe::solid(SurfaceRole::AccentHover)),
            // Int UI has no distinct pressed state — pressed falls back
            // to hover (pressed → hover → idle). The button now provides
            // the Pressed state on pointer-down (see
            // `build_interaction_handlers`); whether to render it is a
            // per-theme recipe decision, and IntUI declines. Other
            // theme recipes (Material 3, macOS, …) set a distinct
            // pressed fill here.
            pressed: None,
            focused: None,
            disabled: Some(FillRecipe::solid(SurfaceRole::AccentDisabled)),
        },
        border: PerStateRecipe {
            idle: BorderRecipe::solid(0.0, RecipeColor::Border(BorderRole::Transparent)),
            hover: None,
            pressed: None,
            focused: Some(BorderRecipe::solid(
                2.0,
                RecipeColor::Border(BorderRole::Focused),
            )),
            disabled: None,
        },
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(14.0, 0.0),
        min_size: Size::new(72.0, 24.0),
    }
}

fn intui_plain_recipe() -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::rounded(4.0),
        fill: PerStateRecipe {
            idle: FillRecipe::solid(SurfaceRole::Main),
            hover: Some(FillRecipe::solid(SurfaceRole::Hover)),
            // Int UI has no distinct pressed state — falls back to hover.
            pressed: None,
            focused: None,
            disabled: None,
        },
        border: PerStateRecipe {
            idle: BorderRecipe::solid(1.0, RecipeColor::Border(BorderRole::Default)),
            hover: Some(BorderRecipe::solid(
                1.0,
                RecipeColor::Border(BorderRole::Strong),
            )),
            // No distinct pressed border — falls back to the hover border.
            pressed: None,
            focused: Some(BorderRecipe::solid(
                2.0,
                RecipeColor::Border(BorderRole::Focused),
            )),
            disabled: None,
        },
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(14.0, 0.0),
        min_size: Size::new(72.0, 24.0),
    }
}

fn intui_ghost_recipe() -> ButtonRecipe {
    ButtonRecipe {
        shape: ShapeRecipe::rounded(4.0),
        fill: PerStateRecipe {
            idle: FillRecipe::solid(SurfaceRole::Transparent),
            hover: Some(FillRecipe::solid(SurfaceRole::Hover)),
            // Int UI has no distinct pressed state — falls back to hover.
            pressed: None,
            focused: None,
            disabled: None,
        },
        border: PerStateRecipe {
            idle: BorderRecipe::solid(0.0, RecipeColor::Border(BorderRole::Transparent)),
            hover: None,
            pressed: None,
            focused: Some(BorderRecipe::solid(
                2.0,
                RecipeColor::Border(BorderRole::Focused),
            )),
            disabled: None,
        },
        shadow: PerStateRecipe::uniform(None),
        padding: EdgeInsets::symmetric(14.0, 0.0),
        min_size: Size::new(72.0, 24.0),
    }
}

// `Rc::new(RecipeButtonStyle::default())` is a common allocation point
// for callers that need a `SharedButtonStyle`; expose a tiny helper
// so they don't have to repeat the type name.
pub fn shared_intui() -> Rc<dyn ButtonStyle> {
    Rc::new(RecipeButtonStyle::default())
}
