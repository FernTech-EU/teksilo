//! Default `RadioStyle` impl driven by paint-recipe data.
//!
//! `RecipeRadioStyle` ships the IntUI look out of the box; apps that
//! want a different design language (Material 3 radio with growing
//! ripple, brutalist square, custom mark) write their own
//! `impl RadioStyle` block and install it per-call
//! (`RadioButton::style(...)`) or theme-wide (step 8's
//! `ComponentStyles.radio = Rc::new(MyRadio)`).
//!
//! The body is a small leaf widget ([`RadioBody`]) that paints the
//! outer ring + inner dot directly to canvas — same trade-off as
//! `RecipeCheckboxStyle` / `RecipeToggleStyle`.

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::focus::FocusOrigin;
use fern_core::signal::Signal;
use fern_core::styles::{RadioStyle, RadioStyleConfig, RadioVariant};
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

// IntUI design tokens for RadioButton. Moved here in Step 7 of the
// styling refactor — the recipe owns its own dimensions instead of
// reading from `theme.components.radio`.
pub const RADIO_VISUAL_SIZE: f32 = 19.0;
pub const RADIO_HIT_AREA: f32 = 24.0;
pub const RADIO_LABEL_GAP: f32 = 6.0;
pub const RADIO_INNER_DOT_SIZE: f32 = 7.0;

/// Default `RadioStyle` shipped with FernUI. Colors come from
/// `theme.colors.{accent, accent_hover, accent_pressed, accent_disabled,
/// border, border_strong, border_focused}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeRadioStyle;

impl RadioStyle for RecipeRadioStyle {
    fn make_body(&self, cfg: &RadioStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let focus_origin = cfg.is_focused.zip(&cfg.is_hovered).map(|(focused, hovered)| {
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
        Size::new(RADIO_VISUAL_SIZE, RADIO_VISUAL_SIZE).into()
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
            RadioVariant::Circle => CornerRadius::uniform(RADIO_VISUAL_SIZE / 2.0),
            RadioVariant::Square => CornerRadius::uniform(0.0),
            RadioVariant::Rounded => CornerRadius::uniform(RADIO_VISUAL_SIZE * 0.25),
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
        let dot_size = RADIO_INNER_DOT_SIZE;
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
