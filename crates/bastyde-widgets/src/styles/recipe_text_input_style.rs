// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `TextInputStyle` impl driven by paint-recipe data.
//!
//! `RecipeTextInputStyle` ships the IntUI chrome: a bordered rectangle
//! around the editor area with a horizontal-padding inset, the border
//! thickening and recolouring on focus, and validation tints (error /
//! warning / corrected) overriding focus when set. The validation
//! strip below the field is the widget's responsibility — the trait
//! recipe is just the bordered frame.
//!
//! The recipe describes border / fill / corner radius only; the rest
//! stays on the widget. Caret blinking, IME composition, placeholder
//! layering, leading / trailing slots, clear button, the
//! ValidationStrip below — all stay on the public `TextInput` widget.
//!
//! Variants:
//!
//! - `Outlined` (default) — 1 dp border in the theme's default border
//!   role; thickens to `focus_ring_width` on focus.
//! - `Filled` — accent-subtle background, no border. Material 3 style.
//! - `Underline` — transparent surface with a single bottom border.
//!   For now this is rendered as Outlined with the same border on all
//!   sides; a true bottom-only stroke arrives once `BorderPosition` /
//!   per-side stroke recipes land.
//! - `Bare` — no chrome at all. Returns the editor verbatim. Used by
//!   parents that own the chrome themselves (search fields, combo box
//!   filter input).

use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    TextInputStyle, TextInputStyleConfig, TextInputValidationLevel, TextInputVariant,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::primitives::{MinSize, Padding, RectWidget, ZStack};

// IntUI design tokens for TextInput / TextInputField (also used by
// SpinBox, DateEdit, DateRangeEdit, DateTimeEdit since they share the
// same form-field baseline). The recipe and form-field composers own
// these constants.
pub const TEXT_FIELD_HEIGHT: f32 = 28.0;
pub const TEXT_FIELD_PADDING_HORIZONTAL: f32 = 4.0;
pub const TEXT_FIELD_PADDING_VERTICAL: f32 = 4.0;
pub const TEXT_FIELD_BORDER_WIDTH: f32 = 1.0;
pub const TEXT_FIELD_CORNER_RADIUS: f32 = 4.0;
pub const TEXT_FIELD_CARET_WIDTH: f32 = 1.0;
pub const TEXT_FIELD_VALIDATION_STRIP_GAP: f32 = 4.0;
pub const TEXT_FIELD_ERROR_PULSE_DURATION_MS: u32 = 240;
pub const TEXT_FIELD_CORRECTED_PULSE_DURATION_MS: u32 = 1500;
pub const TEXT_FIELD_MASK_PLACEHOLDER_CHAR: char = '_';

/// Default `TextInputStyle` shipped with Bastyde.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeTextInputStyle;

impl TextInputStyle for RecipeTextInputStyle {
    fn make_body(&self, cfg: &TextInputStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme();
        let border_width = TEXT_FIELD_BORDER_WIDTH;
        let focus_ring_width = theme.shape.focus_ring_width;
        let padding_h = TEXT_FIELD_PADDING_HORIZONTAL;
        let corner_radius = TEXT_FIELD_CORNER_RADIUS;
        let height = TEXT_FIELD_HEIGHT;

        // Bare variant: no chrome at all. Just hand the editor back
        // wrapped in a MinSize so consumers still get a predictable
        // intrinsic height.
        if matches!(cfg.variant, TextInputVariant::Bare) {
            return ctx.add(MinSize::new(0.0, height).child_id(cfg.editor));
        }

        // Derived border role: validation outcome trumps focus, and
        // focus trumps default. Mirrors the legacy `derive_border_role`
        // closure that lived on the widget.
        let border_role = derive_border_role(cfg.is_focused.clone(), cfg.validation.clone());

        // Border width: thickens to focus_ring_width when focused,
        // regardless of validation. For `Filled`, force 0.
        let variant = cfg.variant;
        let border_width_signal = cfg.is_focused.map(move |focused| match variant {
            TextInputVariant::Filled => 0.0,
            _ => {
                if *focused {
                    focus_ring_width
                } else {
                    border_width
                }
            }
        });

        // Background role: Filled uses accent-subtle (a faint tint),
        // every other variant uses the standard content surface.
        let bg_role = match cfg.variant {
            TextInputVariant::Filled => SurfaceRole::Hover,
            _ => SurfaceRole::Content,
        };

        let bg = RectWidget::new()
            .background(bg_role)
            .border_color(border_role)
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(corner_radius));
        let bg_id = ctx.add(bg);

        // Horizontal-only padding so leading / trailing slots inside
        // the editor row sit flush against top and bottom of the frame.
        let padded_id = ctx.add(Padding::new(0.0, padding_h, 0.0, padding_h).child_id(cfg.editor));

        let zstack_id = ctx.add(ZStack::new().add_child(bg_id).add_child(padded_id));
        ctx.add(MinSize::new(0.0, height).child_id(zstack_id))
    }
}

/// Derive the border role from focus + validation. Validation tints
/// override focus tint, so a typo in a focused field still reads as an
/// error rather than as "focused and fine".
fn derive_border_role(
    is_focused: Signal<bool>,
    validation: Signal<TextInputValidationLevel>,
) -> Signal<BorderRole> {
    is_focused
        .zip(&validation)
        .map(|(focused, level)| match *level {
            TextInputValidationLevel::Error => BorderRole::Error,
            TextInputValidationLevel::Warning => BorderRole::Warning,
            // Corrected: accent tint (matches the IntUI "we changed
            // something — look here briefly" cue). The decay back to
            // default is driven by the widget setting the validation
            // signal back to None after the corrected pulse.
            TextInputValidationLevel::Corrected | TextInputValidationLevel::Info => {
                BorderRole::Focused
            }
            TextInputValidationLevel::None => {
                if *focused {
                    BorderRole::Focused
                } else {
                    BorderRole::Default
                }
            }
        })
}
