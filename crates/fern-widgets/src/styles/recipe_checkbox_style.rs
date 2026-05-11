//! Default `CheckboxStyle` impl driven by paint-recipe data.
//!
//! `RecipeCheckboxStyle` ships the IntUI look out of the box; apps that
//! want a different design language (Material 3 round checkbox, glyph
//! badge, custom check shape) write their own `impl CheckboxStyle` block
//! and install it per-call (`Checkbox::style(...)`) or theme-wide
//! (step 8's `ComponentStyles.checkbox = Rc::new(MyCheckbox)`).
//!
//! The visual body is a small leaf widget ([`CheckboxBody`]) that paints
//! the box + check / dash glyph directly onto the canvas. Same trade-off
//! as `RecipeToggleStyle`: a leaf keeps paint-cost parity with the
//! pre-refactor Checkbox; custom impls are free to compose primitives
//! (`RectWidget` + `IconWidget` in a `ZStack`) if they prefer.

use fern_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::focus::FocusOrigin;
use fern_core::signal::Signal;
use fern_core::styles::{CheckboxState, CheckboxStyle, CheckboxStyleConfig, CheckboxVariant};
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

// IntUI design tokens for Checkbox. Moved here in Step 7 of the
// styling refactor — the recipe owns its own dimensions instead of
// reading from `theme.components.checkbox`.
pub const CHECKBOX_BOX_VISUAL_SIZE: f32 = 19.0;
pub const CHECKBOX_BOX_HIT_AREA: f32 = 24.0;
pub const CHECKBOX_LABEL_GAP: f32 = 6.0;
pub const CHECKBOX_CORNER_RADIUS: f32 = 3.0;

/// Default `CheckboxStyle` shipped with FernUI. Colors come from
/// `theme.colors.{accent, accent_hover, accent_pressed, accent_disabled,
/// border, border_strong, border_focused, text_on_accent, text_disabled}`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeCheckboxStyle;

impl CheckboxStyle for RecipeCheckboxStyle {
    fn make_body(&self, cfg: &CheckboxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
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

        ctx.add(CheckboxBody {
            state: cfg.state.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_pressed: cfg.is_pressed.clone(),
            is_disabled: cfg.is_disabled.clone(),
            focus_origin,
            variant: cfg.variant,
        })
    }
}

struct CheckboxBody {
    state: Signal<CheckboxState>,
    is_hovered: Signal<bool>,
    is_pressed: Signal<bool>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    variant: CheckboxVariant,
}

impl std::fmt::Debug for CheckboxBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckboxBody")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for CheckboxBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.state
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
        Size::new(CHECKBOX_BOX_VISUAL_SIZE, CHECKBOX_BOX_VISUAL_SIZE).into()
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
        let state = self.state.get();
        let is_filled = matches!(state, CheckboxState::Checked | CheckboxState::Indeterminate);
        let disabled = self.is_disabled.get();
        let pressed = self.is_pressed.get();
        let hovered = self.is_hovered.get();
        let focused = self.focus_origin.get().is_some();

        // Box background — Accent family when filled, transparent
        // otherwise. Disabled overrides everything.
        let bg = if disabled {
            if is_filled {
                colors.accent_disabled
            } else {
                Color::TRANSPARENT
            }
        } else if is_filled {
            if pressed {
                colors.accent_pressed
            } else if hovered {
                colors.accent_hover
            } else {
                colors.accent
            }
        } else {
            Color::TRANSPARENT
        };

        // Border — focus wins over everything (a filled focused checkbox
        // still shows the accent border, since there's no external ring).
        let border_color = if focused {
            colors.border_focused
        } else if disabled {
            colors.accent_disabled
        } else if is_filled {
            Color::TRANSPARENT
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

        // Variant-specific corner shape. Square uses theme corner_radius,
        // Rounded doubles it for a softer look, Circle uses half-size for
        // a perfect circle.
        let corner = match self.variant {
            CheckboxVariant::Square => CornerRadius::uniform(CHECKBOX_CORNER_RADIUS),
            CheckboxVariant::Rounded => CornerRadius::uniform(CHECKBOX_CORNER_RADIUS * 2.0),
            CheckboxVariant::Circle => CornerRadius::uniform(CHECKBOX_BOX_VISUAL_SIZE / 2.0),
        };

        canvas.fill_rounded_rect(bounds, corner, bg);
        if border_width > 0.0 && border_color != Color::TRANSPARENT {
            canvas.stroke_rounded_rect(bounds, corner, border_color, border_width);
        }

        // Glyph — check or dash, depending on state. Painted at 75% of
        // the box, centered.
        if matches!(state, CheckboxState::Unchecked) {
            return;
        }
        let glyph_color = if disabled {
            colors.text_disabled
        } else {
            colors.text_on_accent
        };
        let glyph_size = CHECKBOX_BOX_VISUAL_SIZE * 0.75;
        let glyph_rect = Rect::new(
            bounds.x + (bounds.width - glyph_size) / 2.0,
            bounds.y + (bounds.height - glyph_size) / 2.0,
            glyph_size,
            glyph_size,
        );
        let stroke_w = (glyph_size * 0.18).max(1.5);

        let mut path = Path::new();
        match state {
            CheckboxState::Checked => {
                // Checkmark: V-shape from (0.2, 0.55) → (0.43, 0.78) → (0.8, 0.28).
                path.move_to(Point::new(
                    glyph_rect.x + glyph_size * 0.20,
                    glyph_rect.y + glyph_size * 0.55,
                ));
                path.line_to(Point::new(
                    glyph_rect.x + glyph_size * 0.43,
                    glyph_rect.y + glyph_size * 0.78,
                ));
                path.line_to(Point::new(
                    glyph_rect.x + glyph_size * 0.80,
                    glyph_rect.y + glyph_size * 0.28,
                ));
            }
            CheckboxState::Indeterminate => {
                // Horizontal dash centered in the box.
                path.move_to(Point::new(
                    glyph_rect.x + glyph_size * 0.20,
                    glyph_rect.y + glyph_size * 0.50,
                ));
                path.line_to(Point::new(
                    glyph_rect.x + glyph_size * 0.80,
                    glyph_rect.y + glyph_size * 0.50,
                ));
            }
            CheckboxState::Unchecked => unreachable!(),
        }
        canvas.stroke_path(&path, glyph_color, stroke_w);
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Accessibility lives on the parent Checkbox widget; the body is
        // presentational.
    }
}
