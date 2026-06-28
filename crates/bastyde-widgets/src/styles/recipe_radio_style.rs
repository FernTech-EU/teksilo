// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `RadioStyle` impl driven by paint-recipe data.
//!
//! `RecipeRadioStyle` ships the IntUI look out of the box; apps that
//! want a different design language (Material 3 radio with growing
//! ripple, brutalist square, custom mark) write their own
//! `impl RadioStyle` block and install it per-call
//! (`RadioButton::style(...)`) or theme-wide
//! (`theme.style_slots.radio = Some(Rc::new(MyRadio))`).
//!
//! The body is a small leaf widget (`RadioBody`) that paints the
//! outer ring + inner dot directly to canvas — same trade-off as
//! `RecipeCheckboxStyle` / `RecipeToggleStyle`.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::focus::FocusOrigin;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{RadioStyle, RadioStyleConfig, RadioVariant};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{Color, CornerRadius};

// IntUI design tokens for RadioButton. The recipe owns its own dimensions.
pub const RADIO_VISUAL_SIZE: f32 = 19.0;
pub const RADIO_HIT_AREA: f32 = 24.0;
pub const RADIO_LABEL_GAP: f32 = 6.0;
pub const RADIO_INNER_DOT_SIZE: f32 = 7.0;

/// Dimension recipe for `RecipeRadioStyle`. All tuneable sizes are collected
/// here so an app can pass a customised `RadioRecipe` to `RecipeRadioStyle::new`
/// without writing a full `RadioStyle` impl.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadioRecipe {
    pub visual_size: f32,
    pub hit_area: f32,
    pub label_gap: f32,
    pub inner_dot_size: f32,
}

impl Default for RadioRecipe {
    fn default() -> Self {
        Self {
            visual_size: RADIO_VISUAL_SIZE,
            hit_area: RADIO_HIT_AREA,
            label_gap: RADIO_LABEL_GAP,
            inner_dot_size: RADIO_INNER_DOT_SIZE,
        }
    }
}

/// Default `RadioStyle` shipped with Bastyde. Colors come from
/// `theme.colors.{accent, accent_hover, accent_pressed, accent_disabled,
/// border, border_strong, border_focused}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeRadioStyle {
    pub recipe: RadioRecipe,
}

impl RecipeRadioStyle {
    pub fn new(recipe: RadioRecipe) -> Self {
        Self { recipe }
    }
}

impl RadioStyle for RecipeRadioStyle {
    fn make_body(&self, cfg: &RadioStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let focus_origin = cfg
            .is_focused
            .zip(&cfg.is_hovered)
            .map(|(focused, hovered)| {
                if *focused {
                    Some(if *hovered {
                        FocusOrigin::Pointer
                    } else {
                        FocusOrigin::Keyboard
                    })
                } else {
                    None
                }
            });

        ctx.add(RadioBody {
            is_selected: cfg.is_selected.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_pressed: cfg.is_pressed.clone(),
            is_disabled: cfg.is_disabled.clone(),
            focus_origin,
            variant: cfg.variant,
            recipe: self.recipe,
        })
    }
}

struct RadioBody {
    is_selected: Signal<bool>,
    is_hovered: Signal<bool>,
    is_pressed: Signal<bool>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    variant: RadioVariant,
    recipe: RadioRecipe,
}

impl std::fmt::Debug for RadioBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioBody")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for RadioBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_selected
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_hovered
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_pressed
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(self.recipe.visual_size, self.recipe.visual_size).into()
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
        let selected = self.is_selected.get();
        let disabled = self.is_disabled.get();
        let pressed = self.is_pressed.get();
        let hovered = self.is_hovered.get();
        let focused = self.focus_origin.get().is_some();

        // Border color — focus wins over everything (a selected focused
        // radio still shows the accent ring; there's no external ring).
        let border_color = if focused {
            colors.border_focused
        } else if disabled {
            colors.accent_disabled
        } else if selected {
            colors.accent
        } else if hovered {
            colors.border_strong
        } else {
            colors.border
        };

        let border_width = if focused {
            ctx.theme.shape.focus_ring_width
        } else {
            ctx.theme.shape.border_width
        };

        // Variant-specific outer corner shape.
        let outer_corner = match self.variant {
            RadioVariant::Circle => CornerRadius::uniform(self.recipe.visual_size / 2.0),
            RadioVariant::Square => CornerRadius::uniform(0.0),
            RadioVariant::Rounded => CornerRadius::uniform(self.recipe.visual_size * 0.25),
        };

        // Outer ring background — transparent (the ring is the border).
        canvas.fill_rounded_rect(bounds, outer_corner, Color::TRANSPARENT);
        if border_width > 0.0 && border_color != Color::TRANSPARENT {
            canvas.stroke_rounded_rect(bounds, outer_corner, border_color, border_width);
        }

        // Inner dot — only when selected. The Square variant uses a small
        // inner square instead of a dot (some accessibility kits prefer
        // the distinguishability vs. a circle).
        if !selected {
            return;
        }
        let dot_color = if disabled {
            colors.accent_disabled
        } else if pressed {
            colors.accent_pressed
        } else if hovered {
            colors.accent_hover
        } else {
            colors.accent
        };
        let dot_size = self.recipe.inner_dot_size;
        let dot_rect = Rect::new(
            bounds.x + (bounds.width - dot_size) / 2.0,
            bounds.y + (bounds.height - dot_size) / 2.0,
            dot_size,
            dot_size,
        );
        let dot_corner = match self.variant {
            RadioVariant::Circle => CornerRadius::uniform(dot_size / 2.0),
            RadioVariant::Square => CornerRadius::uniform(0.0),
            RadioVariant::Rounded => CornerRadius::uniform(dot_size * 0.25),
        };
        canvas.fill_rounded_rect(dot_rect, dot_corner, dot_color);
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // AT semantics live on the parent RadioButton; the body is
        // presentational.
    }
}
