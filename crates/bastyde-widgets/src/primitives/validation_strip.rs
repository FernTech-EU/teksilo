// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ValidationStrip — a small inline message shown below a text field to
//! surface a validation outcome.
//!
//! Bound to a `Signal<ValidationFeedback>` produced by a
//! [`TextInputField`](super::text_input_field::TextInputField).  The strip
//! renders nothing when the feedback is `Pristine` or `Valid`, and shows a
//! single-line message in the appropriate role when `Invalid` (error colour,
//! `Live::Assertive`) or `Corrected` (secondary text, `Live::Polite`).
//! The strip is layout-stable: in the hidden state it reports zero height so
//! the surrounding layout does not reflow on every commit.
//! It carries `Role::Status` so screen readers announce the message through
//! the appropriate live region without any composite-side wiring.
//!
//! ```ignore
//! // ValidationStrip is constructed with a `Signal<ValidationFeedback>`
//! // obtained from a live `TextInputField` — it needs BuildContext to wire up.
//! // Typical usage inside a composing widget's build():
//! let (field_id, fb_signal) = build_text_input_field(ctx, ...);
//! let strip = ctx.add(ValidationStrip::new(fb_signal));
//! ```

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::{Live, Role};
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::lit;
use bastyde_tokens::{TextRole, TextStyleRole};

use super::TextWidget;
use super::text_input_field::ValidationFeedback;

/// Inline validation-feedback strip. See module docs.
pub struct ValidationStrip {
    feedback: Signal<ValidationFeedback>,
    root_id: Option<WidgetId>,
}

impl std::fmt::Debug for ValidationStrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidationStrip").finish()
    }
}

impl ValidationStrip {
    /// Construct a strip bound to a feedback signal — typically
    /// `field.validation_feedback_signal()` from the same widget.
    pub fn new(feedback: Signal<ValidationFeedback>) -> Self {
        Self {
            feedback,
            root_id: None,
        }
    }
}

impl Widget for ValidationStrip {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Reactive label text + color: the inner TextWidget binds
        // both via signals derived from `feedback`. Pristine / Valid
        // states produce empty text → the TextWidget renders zero
        // size and the strip is invisible.
        // Zip with locale signal so messages re-resolve on locale change.
        let locale_signal = ctx.locale_signal();
        let text_signal = self.feedback.zip(&locale_signal).map(|(fb, _)| match fb {
            ValidationFeedback::Invalid { message }
            | ValidationFeedback::Corrected { message, .. } => message.resolve_now(),
            _ => String::new(),
        });

        // Color: error roles for Invalid; secondary for Corrected
        // (Int UI's "low-key informational" tone). Pristine / Valid
        // also pick secondary but render nothing because the text is
        // empty, so the choice is moot.
        let color_signal: Signal<TextRole> = self.feedback.map(|fb| match fb {
            ValidationFeedback::Invalid { .. } => TextRole::Error,
            _ => TextRole::Secondary,
        });

        let label = TextWidget::new(lit!(""))
            .style(TextStyleRole::Small)
            .text(text_signal)
            .color(bastyde_core::color_prop::ColorProp::DynamicTextRole(
                color_signal,
            ))
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label);
        self.root_id = Some(label_id);

        // Bind feedback at AccessibilityOnly so the strip's AT node
        // refreshes its `Live` region politeness when the outcome
        // changes (Polite for Corrected, Assertive for Invalid).
        let self_id = ctx.self_id();
        self.feedback.bind_to(
            self_id,
            ctx.binding_registry(),
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![label_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Layout-stable empty state: when feedback carries no message
        // (Pristine / Valid), report zero size so the parent's slot
        // collapses entirely. An empty TextWidget would otherwise
        // contribute its style line-height (~12 dp), accumulating
        // across stacks of fields and pushing siblings offscreen.
        if !matches!(
            self.feedback.get(),
            ValidationFeedback::Invalid { .. } | ValidationFeedback::Corrected { .. }
        ) {
            return Size::ZERO.into();
        }
        match self.root_id {
            Some(id) => ctx.child_size(id, proposal).unwrap_or(Size::ZERO),
            None => Size::ZERO,
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Status);
        let fb = self.feedback.get();
        match &fb {
            ValidationFeedback::Invalid { message } => {
                builder.set_name(message.clone());
                builder.set_live(Live::Assertive);
            }
            ValidationFeedback::Corrected { message, .. } => {
                builder.set_name(message.clone());
                builder.set_live(Live::Polite);
            }
            _ => {
                // Empty Status node — present in the AT tree but not
                // announcing anything. Live::Off keeps it silent.
                builder.set_live(Live::Off);
            }
        }
    }
}
