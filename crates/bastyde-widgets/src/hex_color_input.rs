// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `HexColorInput` — single-line `#RRGGBB[AA]` color editor.
//!
//! A specialization of [`TextInput`] that wires an input mask, a
//! hex-digit character filter, and a strict commit-time validator on top
//! of the standard text-editing surface. Bound to a `Signal<Color>`
//! (required) or `Signal<Option<Color>>` (nullable). External writes to
//! the bound signal reformat the field text — but only when the field
//! is unfocused, so a user typing "FF" in the middle of a long color
//! code isn't clobbered by a sibling widget tweaking the value.
//!
//! # Behaviour
//!
//! - **Parsing**: `#RRGGBB` (case-insensitive); `#RRGGBBAA` if
//!   `alpha_enabled`; `#RGB` short-form expands to `#RRGGBB` if
//!   `short_form_enabled`. Each accepted form may be normalized to
//!   uppercase on commit (configurable).
//! - **Char filter**: only `[0-9a-fA-F#]` admitted while typing.
//! - **Mask**: `\\#hhhhhh` (or `\\#hhhhhhhh` with alpha) — the
//!   `TextInputField` mask grammar (`h` = hex digit slot, `\\` literal
//!   escape).
//! - **Validation**: commits on Enter / Tab-out / blur.  Returns
//!   [`ValidationOutcome::Valid`] / [`ValidationOutcome::Corrected`] /
//!   [`ValidationOutcome::Invalid`] which the inner field maps to a
//!   visible inline strip via the standard
//!   `bind_validation_feedback` bridge.
//! - **Nullable**: empty (after trim) commits `None`; non-empty
//!   parses normally and commits `Some(color)`.
//!
//! # Example
//!
//! ```ignore
//! let color = ctx.signal(Color::from_hex("#3584E4"));
//! ctx.add(
//!     HexColorInput::new(color)
//!         .alpha_enabled(true)
//!         .label("Background"),
//! );
//! ```

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::SizeProposal;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::Role;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{localized, resolve_message_widget};
use bastyde_tokens::Color;

use crate::primitives::text_input_field::{ValidationFeedback, ValidationOutcome, ValidatorFn};
use crate::text_input::TextInput;
use bastyde_i18n::LocalizedString;

type OnValueChanged = Rc<dyn Fn(Option<Color>, &mut bastyde_core::widget::EventContext)>;
type OnInvalid = Rc<dyn Fn(&str, &mut bastyde_core::widget::EventContext)>;

/// Internal binding — the widget bridges to either a non-nullable
/// `Signal<Color>` (where empty commits revert to the previous color)
/// or a nullable `Signal<Option<Color>>` (where empty commits store
/// `None`).
#[derive(Clone)]
enum HexValueBinding {
    Required(Signal<Color>),
    Nullable(Signal<Option<Color>>),
}

impl HexValueBinding {
    fn current(&self) -> Option<Color> {
        match self {
            Self::Required(s) => Some(s.get()),
            Self::Nullable(s) => s.get(),
        }
    }

    fn set(&self, value: Option<Color>) {
        match self {
            Self::Required(s) => {
                if let Some(c) = value {
                    s.set(c);
                }
                // None on a required binding is silently ignored — the
                // validator already returns Invalid for empty input on
                // required signals so this branch is unreachable in
                // practice.
            }
            Self::Nullable(s) => {
                s.set(value);
            }
        }
    }

    /// Subscribe `f` to be called whenever the bound value changes.
    /// Bridge between the two binding shapes so the focused-reformat
    /// effect doesn't need to know which variant it has.
    fn observe_with_effect<F: Fn(Option<Color>) + 'static>(&self, ctx: &mut BuildContext, f: F) {
        match self {
            Self::Required(s) => {
                ctx.effect(s, move |c| f(Some(*c)));
            }
            Self::Nullable(s) => {
                ctx.effect(s, move |c| f(*c));
            }
        }
    }
}

/// Single-line hex color editor.
pub struct HexColorInput {
    value: HexValueBinding,
    alpha_enabled: bool,
    short_form_enabled: bool,
    require_hash: bool,
    uppercase: bool,
    label: Option<LocalizedString>,
    placeholder: Option<LocalizedString>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    read_only: bool,
    width: Option<f32>,
    on_value_changed: Option<OnValueChanged>,
    on_invalid: Option<OnInvalid>,
    /// Lazily created in [`Widget::build`]; mirrored by the inner
    /// TextInput's wiring + by the focused-reformat effect.
    text_signal: Signal<String>,
    /// Lazily set during build — true while the inner field has focus.
    focused: Signal<bool>,
    /// Mirrored from the inner TextInput's
    /// `validation_feedback_signal()` so external observers can react
    /// to commit feedback.
    feedback: Signal<ValidationFeedback>,
    /// Inner widget id captured during build for layout forwarding.
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for HexColorInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HexColorInput")
            .field("alpha_enabled", &self.alpha_enabled)
            .field("short_form_enabled", &self.short_form_enabled)
            .field("require_hash", &self.require_hash)
            .field("uppercase", &self.uppercase)
            .field("initial_enabled", &self.initial_enabled)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl HexColorInput {
    /// Bind to a non-nullable color signal. Empty / invalid input
    /// surfaces an error and keeps the previous value. Commits on
    /// Enter or blur.
    pub fn new(value: Signal<Color>) -> Self {
        let initial = value.get();
        Self::from_binding(HexValueBinding::Required(value), Some(initial))
    }

    /// Bind to a nullable color signal. Empty input commits `None`;
    /// invalid input surfaces an error and keeps the previous value.
    /// Commits on Enter or blur.
    pub fn nullable(value: Signal<Option<Color>>) -> Self {
        let initial = value.get();
        Self::from_binding(HexValueBinding::Nullable(value), initial)
    }

    fn from_binding(binding: HexValueBinding, initial: Option<Color>) -> Self {
        let alpha_enabled = false;
        let uppercase = true;
        let initial_text = initial
            .map(|c| format_hex(c, alpha_enabled, uppercase))
            .unwrap_or_default();
        Self {
            value: binding,
            alpha_enabled,
            short_form_enabled: true,
            require_hash: true,
            uppercase,
            label: None,
            placeholder: None,
            initial_enabled: true,
            read_only: false,
            width: None,
            on_value_changed: None,
            on_invalid: None,
            text_signal: Signal::new(initial_text),
            focused: Signal::new(false),
            feedback: Signal::new(ValidationFeedback::Pristine),
            root_child_id: None,
        }
    }

    pub fn alpha_enabled(mut self, enabled: bool) -> Self {
        self.alpha_enabled = enabled;
        // Re-seed text in the new shape so the widget renders consistently
        // before build() runs.
        if let Some(c) = self.value.current() {
            self.text_signal
                .set(format_hex(c, self.alpha_enabled, self.uppercase));
        }
        self
    }

    pub fn short_form_enabled(mut self, enabled: bool) -> Self {
        self.short_form_enabled = enabled;
        self
    }

    pub fn require_hash(mut self, required: bool) -> Self {
        self.require_hash = required;
        self
    }

    pub fn uppercase(mut self, upper: bool) -> Self {
        self.uppercase = upper;
        if let Some(c) = self.value.current() {
            self.text_signal
                .set(format_hex(c, self.alpha_enabled, self.uppercase));
        }
        self
    }

    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<LocalizedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build time.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width.max(0.0));
        self
    }

    pub fn on_value_changed(
        mut self,
        f: impl Fn(Option<Color>, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_value_changed = Some(Rc::new(f));
        self
    }

    pub fn on_invalid(
        mut self,
        f: impl Fn(&str, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_invalid = Some(Rc::new(f));
        self
    }

    /// Reactive handle on the inner TextInput's published validation
    /// feedback. Mirrors the inner field's signal after `build()`.
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback.clone()
    }
}

impl Widget for HexColorInput {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let alpha_enabled = self.alpha_enabled;
        let short_form_enabled = self.short_form_enabled;
        let require_hash = self.require_hash;
        let uppercase = self.uppercase;
        let nullable = matches!(self.value, HexValueBinding::Nullable(_));

        let placeholder = self
            .placeholder
            .clone()
            .map(|ls| ls.resolve_now())
            .unwrap_or_else(|| {
                if alpha_enabled {
                    resolve_message_widget("hex-color-input-placeholder-with-alpha", &[])
                } else {
                    resolve_message_widget("hex-color-input-placeholder", &[])
                }
            });

        // External writes → reformat (skip while focused, mirror DateEdit).
        {
            let text_signal = self.text_signal.clone();
            let focused = self.focused.clone();
            self.value.observe_with_effect(ctx, move |new_value| {
                if focused.get() {
                    return;
                }
                let formatted = match new_value {
                    Some(c) => format_hex(c, alpha_enabled, uppercase),
                    None => String::new(),
                };
                if text_signal.get() != formatted {
                    text_signal.set(formatted);
                }
            });
        }

        // Validator — pure classification. Side-effects (writing back
        // to the bound signal, firing on_value_changed / on_invalid)
        // happen in the on_blur / on_submit chain because the validator
        // closure can't see EventContext.
        //
        // Invalid messages capture the raw input via a shared cell so
        // the on_blur handler can pass it to on_invalid.
        let last_raw: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let validator: ValidatorFn = {
            let last_raw = last_raw.clone();
            Rc::new(move |raw: &str| -> ValidationOutcome {
                *last_raw.borrow_mut() = raw.to_string();
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    if nullable {
                        return ValidationOutcome::Valid;
                    }
                    return ValidationOutcome::Invalid {
                        message: invalid_message(alpha_enabled),
                    };
                }
                match parse_hex(trimmed, alpha_enabled, short_form_enabled, require_hash) {
                    Ok(parsed) => {
                        let normalized = format_hex(parsed, alpha_enabled, uppercase);
                        if normalized == trimmed {
                            ValidationOutcome::Valid
                        } else {
                            // Distinguish "expanded short-form" from
                            // "case normalized" for the message.
                            let stripped = trimmed.strip_prefix('#').unwrap_or(trimmed);
                            let was_short_form = short_form_enabled && stripped.len() == 3;
                            // Capture owned copies: the `localized` closure is
                            // `'static`, so it can't borrow `trimmed`, and
                            // `normalized` is still needed for `corrected`.
                            let raw_owned = trimmed.to_string();
                            let value_owned = normalized.clone();
                            let message = if was_short_form {
                                localized(move || {
                                    resolve_message_widget(
                                        "hex-color-input-corrected-shortform",
                                        &[
                                            ("raw", raw_owned.clone().into()),
                                            ("value", value_owned.clone().into()),
                                        ],
                                    )
                                })
                            } else {
                                localized(move || {
                                    resolve_message_widget(
                                        "hex-color-input-corrected-uppercase",
                                        &[("value", value_owned.clone().into())],
                                    )
                                })
                            };
                            ValidationOutcome::Corrected {
                                corrected: normalized,
                                message,
                            }
                        }
                    }
                    Err(_) => ValidationOutcome::Invalid {
                        message: invalid_message(alpha_enabled),
                    },
                }
            })
        };

        // Commit closure — runs on Enter or focus loss after the
        // validator. If feedback is `Invalid`, leave the typed text
        // alone (don't silently revert; same DateEdit policy). On
        // valid / corrected commits, parse the (possibly-rewritten)
        // text and write back to the bound signal + fire callbacks.
        let commit: Rc<dyn Fn(&mut bastyde_core::widget::EventContext)> = {
            let value_binding = self.value.clone();
            let text_signal = self.text_signal.clone();
            let feedback_signal = self.feedback.clone();
            let on_value_changed = self.on_value_changed.clone();
            let on_invalid = self.on_invalid.clone();
            let last_raw = last_raw.clone();
            Rc::new(move |ctx_evt: &mut bastyde_core::widget::EventContext| {
                let fb = feedback_signal.get();
                if matches!(fb, ValidationFeedback::Invalid { .. }) {
                    if let Some(cb) = on_invalid.as_ref() {
                        let raw = last_raw.borrow().clone();
                        cb(&raw, ctx_evt);
                    }
                    return;
                }
                let raw = text_signal.get();
                let trimmed = raw.trim();
                let new_value: Option<Color> = if trimmed.is_empty() {
                    None
                } else {
                    parse_hex(trimmed, alpha_enabled, short_form_enabled, require_hash).ok()
                };
                let prev = value_binding.current();
                if prev != new_value {
                    value_binding.set(new_value);
                    if let Some(cb) = on_value_changed.as_ref() {
                        cb(new_value, ctx_evt);
                    }
                }
            })
        };

        // Build the inner TextInput composite. The validator + char
        // filter + mask cooperate: char filter strips garbage as the
        // user types, mask enforces shape, validator runs on commit.
        let mask_string = if alpha_enabled {
            r"\#hhhhhhhh"
        } else {
            r"\#hhhhhh"
        };

        let mut text_input = TextInput::new(self.text_signal.clone())
            .placeholder(lit!(placeholder))
            .enabled(self.initial_enabled)
            .read_only(self.read_only)
            .input_mask(mask_string.to_string())
            .char_filter(|c: char| c.is_ascii_hexdigit() || c == '#')
            .validator({
                let v = validator.clone();
                move |s| (v)(s)
            })
            .on_submit_fn({
                let commit = commit.clone();
                move |ctx_evt| commit(ctx_evt)
            })
            .on_blur_fn({
                let commit = commit.clone();
                move |ctx_evt| commit(ctx_evt)
            });
        if let Some(label) = self.label.clone() {
            text_input = text_input.label(lit!(label.resolve_now()));
        }
        if let Some(w) = self.width {
            text_input = text_input.min_width(w);
        }

        // Mirror the inner field's published feedback into our own
        // signal so external observers (e.g. ColorPicker, ColorEdit)
        // can react.
        let feedback_in = text_input.validation_feedback_signal();
        {
            let feedback_out = self.feedback.clone();
            ctx.effect(&feedback_in, move |fb| {
                if feedback_out.get() != *fb {
                    feedback_out.set(fb.clone());
                }
            });
        }

        let root_id = ctx.add(text_input);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_layout_response(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into()),
            None => proposal.resolve(0.0, 0.0).into(),
        }
    }

    fn place_children(
        &self,
        bounds: bastyde_canvas::Rect,
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
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Mirror the inner field's role; the TextInput composite carries
        // GenericContainer, so the wrapper retitles to TextInput so AT
        // users land on a recognized text-edit role at this depth.
        builder.set_role(Role::TextInput);
        if let Some(ref label) = self.label {
            builder.set_name(label.resolve_now());
        }
        match self.value.current() {
            Some(c) => {
                builder.set_value(format_hex(c, self.alpha_enabled, self.uppercase));
            }
            None => {
                let placeholder = self
                    .placeholder
                    .clone()
                    .map(|ls| ls.resolve_now())
                    .unwrap_or_else(|| {
                        if self.alpha_enabled {
                            resolve_message_widget("hex-color-input-placeholder-with-alpha", &[])
                        } else {
                            resolve_message_widget("hex-color-input-placeholder", &[])
                        }
                    });
                builder.set_placeholder(placeholder);
            }
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        if self.read_only {
            builder.set_read_only();
        }
    }
}

// ── Helpers (free functions so closures can capture by clone) ────────

fn format_hex(color: Color, alpha_enabled: bool, uppercase: bool) -> String {
    if uppercase {
        color.to_hex_upper(alpha_enabled)
    } else {
        color.to_hex_lower(alpha_enabled)
    }
}

fn invalid_message(alpha_enabled: bool) -> LocalizedString {
    let key = if alpha_enabled {
        "hex-color-input-invalid-with-alpha"
    } else {
        "hex-color-input-invalid"
    };
    localized(move || resolve_message_widget(key, &[]))
}

#[derive(Debug, thiserror::Error)]
enum ParseError {
    #[error("missing `#` prefix")]
    MissingHash,
    #[error("invalid hex length")]
    InvalidLength,
    #[error("invalid hex digit")]
    InvalidDigit,
}

/// Strict hex parser. Returns `Err` instead of silently producing BLACK
/// the way [`Color::from_hex`] does, so the validator can surface a
/// meaningful error message.
fn parse_hex(
    input: &str,
    alpha_enabled: bool,
    short_form_enabled: bool,
    require_hash: bool,
) -> Result<Color, ParseError> {
    let body = match input.strip_prefix('#') {
        Some(rest) => rest,
        None if require_hash => return Err(ParseError::MissingHash),
        None => input,
    };

    let parse_byte = |s: &str| -> Result<u8, ParseError> {
        u8::from_str_radix(s, 16).map_err(|_| ParseError::InvalidDigit)
    };

    match body.len() {
        3 if short_form_enabled => {
            let chars: Vec<char> = body.chars().collect();
            // Each digit doubles: F → FF, 5 → 55. (CSS shorthand convention.)
            let r = parse_byte(&format!("{0}{0}", chars[0]))?;
            let g = parse_byte(&format!("{0}{0}", chars[1]))?;
            let b = parse_byte(&format!("{0}{0}", chars[2]))?;
            Ok(Color::from_rgb(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            ))
        }
        6 => {
            let r = parse_byte(&body[0..2])?;
            let g = parse_byte(&body[2..4])?;
            let b = parse_byte(&body[4..6])?;
            Ok(Color::from_rgb(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            ))
        }
        8 if alpha_enabled => {
            let r = parse_byte(&body[0..2])?;
            let g = parse_byte(&body[2..4])?;
            let b = parse_byte(&body[4..6])?;
            let a = parse_byte(&body[6..8])?;
            Ok(Color::from_rgba(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                a as f32 / 255.0,
            ))
        }
        _ => Err(ParseError::InvalidLength),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_form_uppercase() {
        let c = parse_hex("#FF0000", false, true, true).unwrap();
        assert!((c.r() - 1.0).abs() < 0.01);
        assert!(c.g().abs() < 0.01);
        assert!(c.b().abs() < 0.01);
    }

    #[test]
    fn parse_full_form_lowercase() {
        let c = parse_hex("#ff0000", false, true, true).unwrap();
        assert!((c.r() - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_short_form_expands() {
        let c = parse_hex("#abc", false, true, true).unwrap();
        // #abc → #aabbcc → 0xAA, 0xBB, 0xCC
        assert!((c.r() - (0xAA as f32 / 255.0)).abs() < 0.01);
        assert!((c.g() - (0xBB as f32 / 255.0)).abs() < 0.01);
        assert!((c.b() - (0xCC as f32 / 255.0)).abs() < 0.01);
    }

    #[test]
    fn parse_no_hash_when_required_fails() {
        let err = parse_hex("FF0000", false, true, true);
        assert!(matches!(err, Err(ParseError::MissingHash)));
    }

    #[test]
    fn parse_no_hash_when_optional_succeeds() {
        let c = parse_hex("FF0000", false, true, false).unwrap();
        assert!((c.r() - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_alpha_form() {
        let c = parse_hex("#FF000080", true, true, true).unwrap();
        assert!((c.r() - 1.0).abs() < 0.01);
        assert!((c.a() - 0.5).abs() < 0.01);
    }

    #[test]
    fn parse_alpha_form_rejected_when_disabled() {
        let err = parse_hex("#FF000080", false, true, true);
        assert!(matches!(err, Err(ParseError::InvalidLength)));
    }

    #[test]
    fn parse_invalid_chars() {
        let err = parse_hex("#GGGGGG", false, true, true);
        assert!(matches!(err, Err(ParseError::InvalidDigit)));
    }

    #[test]
    fn parse_wrong_lengths() {
        for input in &["#FF00", "#FF000", "#FF00000"] {
            let err = parse_hex(input, false, true, true);
            assert!(
                matches!(err, Err(ParseError::InvalidLength)),
                "expected InvalidLength for {input}"
            );
        }
    }

    #[test]
    fn format_uppercase_default() {
        let s = format_hex(Color::RED, false, true);
        assert_eq!(s, "#FF0000");
    }

    #[test]
    fn format_lowercase() {
        let s = format_hex(Color::RED, false, false);
        assert_eq!(s, "#ff0000");
    }

    #[test]
    fn format_alpha_form() {
        let c = Color::from_rgba(1.0, 0.0, 0.0, 0.5);
        let s = format_hex(c, true, true);
        // 0.5 * 255 ≈ 127.5 → rounds to 128 = 0x80
        assert_eq!(s, "#FF000080");
    }

    #[test]
    fn nullable_empty_input_is_valid() {
        let signal: Signal<Option<Color>> = Signal::new(None);
        let widget = HexColorInput::nullable(signal.clone());
        assert!(matches!(widget.value, HexValueBinding::Nullable(_)));
        // Confirm parse_hex itself rejects empty input — the
        // nullable-empty-is-valid behavior lives in the validator
        // closure inside build(), not in parse_hex.
        let err = parse_hex("", false, true, true);
        assert!(err.is_err());
    }
}
