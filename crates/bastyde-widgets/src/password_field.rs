//! `PasswordField` — secure single-line text entry with a reveal
//! toggle, masking, Caps Lock warning, and clipboard protection.
//!
//! A thin, ergonomic preset over a secure
//! [`TextInputField`](crate::primitives::TextInputField) composed
//! `SpinBox`-style: the field + an embedded reveal button live inside
//! one bordered frame with a unified focus halo. Masking happens at the
//! text-engine layer (one echo glyph per source `char`), so the
//! plaintext never reaches the shaper or glyph atlas while masked, and
//! caret / selection / hit-test stay correct.
//!
//! Feature parity target: Qt `QLineEdit` echo modes, SwiftUI
//! `SecureField`, WinUI `PasswordBox` / `PasswordRevealMode`, and the
//! Android `password_toggle`.
//!
//! # Example
//!
//! ```ignore
//! let password = ctx.signal(String::new());
//! PasswordField::new(password.clone())
//!     .label(tr!(password()))               // or .label_literal("Password")
//!     .placeholder(tr!(password_hint()))    // i18n-first; `_literal` twins bypass i18n
//!     .validator(|s| if s.len() >= 8 {
//!         ValidationOutcome::Valid
//!     } else {
//!         ValidationOutcome::Invalid { message: "Too short".into() }
//!     })
//! ```

#[cfg(test)]
mod tests;

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, SizeProposal};
use bastyde_core::accesskit::{Live, Role};
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    SharedTextInputStyle, TextInputStyle, TextInputStyleConfig, TextInputValidationLevel,
    TextInputVariant,
};
use bastyde_core::widget::{CursorIcon, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::icon_button::{BuiltInIcons, IconButton};
use crate::primitives::text_input_field::{TextInputField, ValidationFeedback, ValidationOutcome};
use crate::primitives::validation_strip::ValidationStrip;
use crate::primitives::{Center, Expand, HStack, MinSize, Padding, TextWidget, VStack, ZStack};
use crate::tooltip::{self, RichTooltipSource};

// Re-export the masking enums so callers can write
// `PasswordField::new(p).echo_mode(EchoMode::RevealWhileTyping)` from a
// single import path.
pub use crate::primitives::text_input_field::{AtRevealPolicy, EchoMode};

/// The caps-lock indicator glyph: U+21EA UPWARDS WHITE ARROW FROM BAR,
/// the conventional Caps Lock symbol (also used by macOS).
const CAPS_LOCK_GLYPH: &str = "\u{21EA}";

/// How the reveal affordance behaves. Mirrors WinUI's
/// `PasswordRevealMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RevealMode {
    /// A click (or Space / Enter while focused) flips between masked and
    /// revealed. Backed by [`IconButton::visibility_toggle`]; fully
    /// keyboard- and screen-reader-accessible. (Default.)
    #[default]
    Toggle,
    /// Press-and-hold to reveal, release to re-mask (WinUI "Peek").
    /// Pointer-oriented; prefer [`Toggle`](Self::Toggle) for keyboard
    /// accessibility.
    Hold,
    /// No reveal button — the field is always masked per its
    /// [`EchoMode`].
    None,
}

/// Secure single-line text entry. See the [module docs](self).
pub struct PasswordField {
    text: Signal<String>,
    placeholder: String,
    label: String,
    initial_enabled: bool,
    read_only: bool,
    max_length: Option<usize>,
    char_filter: Option<Rc<dyn Fn(char) -> bool>>,
    validator: Option<Rc<dyn Fn(&str) -> ValidationOutcome>>,
    on_submit: Option<Box<dyn Fn(&mut bastyde_core::widget::EventContext)>>,
    on_blur: Option<Box<dyn Fn(&mut bastyde_core::widget::EventContext)>>,
    min_width: Option<f32>,
    variant: TextInputVariant,
    style_override: Option<SharedTextInputStyle>,

    // ── Secure-specific ─────────────────────────────────────────────
    echo_mode: EchoMode,
    echo_char: char,
    reveal_mode: RevealMode,
    revealed: Option<Signal<bool>>,
    allow_copy: bool,
    caps_lock_warning: bool,
    at_reveal_policy: AtRevealPolicy,

    // ── Tooltips (mutually exclusive, last-call-wins) ───────────────
    tooltip_text: Option<String>,
    rich_tooltip_source: Option<RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn Widget>>,

    // ── Internal ────────────────────────────────────────────────────
    revealed_signal: Signal<bool>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for PasswordField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordField")
            .field("label", &self.label)
            .field("echo_mode", &self.echo_mode)
            .field("reveal_mode", &self.reveal_mode)
            .finish_non_exhaustive()
    }
}

impl PasswordField {
    /// Construct a secure field bound to `password`.
    pub fn new(password: Signal<String>) -> Self {
        Self {
            text: password,
            placeholder: String::new(),
            label: String::new(),
            initial_enabled: true,
            read_only: false,
            max_length: None,
            char_filter: None,
            validator: None,
            on_submit: None,
            on_blur: None,
            min_width: None,
            variant: TextInputVariant::default(),
            style_override: None,
            echo_mode: EchoMode::Masked,
            echo_char: '\u{2022}',
            reveal_mode: RevealMode::Toggle,
            revealed: None,
            allow_copy: false,
            caps_lock_warning: true,
            at_reveal_policy: AtRevealPolicy::SwapRole,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            revealed_signal: Signal::new(false),
            root_child_id: None,
        }
    }

    /// Placeholder shown when empty. Never masked.
    pub fn placeholder(mut self, text: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = text.into();
        self.placeholder = ls.resolve_now();
        self
    }

    /// Accessible name, applied to the `Role::PasswordInput` field node.
    /// Strongly recommended for screen-reader users.
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.label = ls.resolve_now();
        self
    }

    /// Initial enabled state. Use `ctx.enabled_when(id, signal)` for
    /// reactivity.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Read-only: selection works, edits don't.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Hard cap on length in `char`s.
    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    /// Per-character input filter (applied to keystrokes, IME commits,
    /// and paste).
    pub fn char_filter(mut self, f: impl Fn(char) -> bool + 'static) -> Self {
        self.char_filter = Some(Rc::new(f));
        self
    }

    /// Commit-time validator (Enter / blur). Drives the inline
    /// validation strip and `aria-invalid`.
    pub fn validator(mut self, f: impl Fn(&str) -> ValidationOutcome + 'static) -> Self {
        self.validator = Some(Rc::new(f));
        self
    }

    /// Fired on Enter (focus stays put).
    pub fn on_submit_fn(
        mut self,
        f: impl Fn(&mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_submit = Some(Box::new(f));
        self
    }

    /// Fired once per focus-loss.
    pub fn on_blur_fn(
        mut self,
        f: impl Fn(&mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_blur = Some(Box::new(f));
        self
    }

    /// Minimum frame width (logical px). Default 65.
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = Some(width);
        self
    }

    /// Frame variant (Outlined / Filled / Underline / Bare).
    pub fn variant(mut self, variant: TextInputVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Per-instance style override.
    pub fn style(mut self, style: impl TextInputStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Override the masking glyph (default `'•'`).
    pub fn echo_char(mut self, c: char) -> Self {
        self.echo_char = c;
        self
    }

    /// Set the [`EchoMode`] (default [`EchoMode::Masked`]).
    pub fn echo_mode(mut self, mode: EchoMode) -> Self {
        self.echo_mode = mode;
        self
    }

    /// Set the [`RevealMode`] (default [`RevealMode::Toggle`]).
    pub fn reveal_mode(mut self, mode: RevealMode) -> Self {
        self.reveal_mode = mode;
        self
    }

    /// Bind an external reveal signal (shared with other UI, observed
    /// for analytics, or driven programmatically). Defaults to an
    /// internal signal exposed via [`revealed_signal`](Self::revealed_signal).
    pub fn bind_revealed(mut self, revealed: Signal<bool>) -> Self {
        self.revealed = Some(revealed);
        self
    }

    /// Permit copy / cut even while masked (default `false`). Copy is
    /// always allowed while revealed regardless of this flag.
    pub fn allow_copy(mut self, allow: bool) -> Self {
        self.allow_copy = allow;
        self
    }

    /// Show a Caps Lock warning when focused with Caps Lock on (default
    /// `true`). The warning is announced to screen readers via a polite
    /// live region.
    pub fn caps_lock_warning(mut self, on: bool) -> Self {
        self.caps_lock_warning = on;
        self
    }

    /// How a *revealed* field reports to assistive tech (default
    /// [`AtRevealPolicy::SwapRole`]).
    pub fn at_reveal_policy(mut self, policy: AtRevealPolicy) -> Self {
        self.at_reveal_policy = policy;
        self
    }

    /// Registry-keyed rich tooltip.
    pub fn rich_tooltip_key(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Inline rich tooltip.
    pub fn rich_tooltip(mut self, content: tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Composite (arbitrary-widget) tooltip.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// The reveal-state signal (`true` = plaintext shown). Useful to
    /// observe or drive reveal programmatically.
    pub fn revealed_signal(&self) -> Signal<bool> {
        self.revealed
            .clone()
            .unwrap_or_else(|| self.revealed_signal.clone())
    }

    /// The bound password signal.
    pub fn text(&self) -> Signal<String> {
        self.text.clone()
    }
}

impl Widget for PasswordField {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::styles::recipe_text_input_style as field_dims;

        let self_id = ctx.self_id();
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }

        // Reveal signal: external binding wins, else the internal one.
        let revealed = self
            .revealed
            .clone()
            .unwrap_or_else(|| self.revealed_signal.clone());

        // Unified halo signals — lit when the field OR the reveal button
        // is focused / hovered (strict-descendant `focus_within` /
        // `hover_within` on the editor row).
        let focused = ctx.signal(false);
        let hovered = ctx.signal(false);

        let inner_height =
            (field_dims::TEXT_FIELD_HEIGHT - 2.0 * field_dims::TEXT_FIELD_BORDER_WIDTH).max(0.0);
        let text_area_height =
            (inner_height - 2.0 * field_dims::TEXT_FIELD_PADDING_VERTICAL).max(0.0);

        // ── Secure inner field ──────────────────────────────────────
        let mut field = TextInputField::new(self.text.clone())
            .enabled(self.initial_enabled)
            .read_only(self.read_only)
            .placeholder(self.placeholder.clone())
            .text_height(text_area_height)
            .secure(self.echo_mode)
            .echo_char(self.echo_char)
            .at_reveal_policy(self.at_reveal_policy)
            .allow_copy(self.allow_copy)
            .bind_revealed(revealed.clone());
        if let Some(max) = self.max_length {
            field = field.max_length(max);
        }
        if let Some(f) = self.char_filter.take() {
            field = field.char_filter(move |c| (f)(c));
        }
        if let Some(cb) = self.on_submit.take() {
            field = field.on_submit_fn(move |ctx| (cb)(ctx));
        }
        if let Some(cb) = self.on_blur.take() {
            field = field.on_blur_fn(move |ctx| (cb)(ctx));
        }
        if let Some(validator) = self.validator.take() {
            field = field.validator(move |s| (validator)(s));
        }
        let inner_feedback = field.validation_feedback_signal();

        // The field carries the `Role::PasswordInput` AT node, so the
        // accessible name belongs on it.
        let field_id = if self.label.is_empty() {
            ctx.add(field)
        } else {
            ctx.add(field.access_label(self.label.clone()))
        };

        let padded_field = ctx.add(
            Padding::new(
                field_dims::TEXT_FIELD_PADDING_VERTICAL,
                0.0,
                field_dims::TEXT_FIELD_PADDING_VERTICAL,
                0.0,
            )
            .child_id(field_id),
        );

        // Placeholder overlay (never masked) shares the field's column.
        let text_column_id = if self.placeholder.is_empty() {
            ctx.add(
                Expand::horizontal()
                    .respect_intrinsic()
                    .child_id(padded_field),
            )
        } else {
            let ph = TextWidget::new(self.placeholder.clone())
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary)
                .single_line()
                .a11y_hidden();
            let ph_id = ctx.add(
                Expand::new()
                    .respect_intrinsic()
                    .child(Center::new().child(ph)),
            );
            let text_for_vis = self.text.clone();
            let visible = text_for_vis.map(|t| t.is_empty());
            ctx.visible_when(ph_id, visible);
            ctx.add(
                Expand::horizontal()
                    .respect_intrinsic()
                    .child(ZStack::new().add_child(ph_id).add_child(padded_field)),
            )
        };

        // ── Editor row: [text_column] [caps?] [reveal?] ─────────────
        let mut row = HStack::new().spacing(4.0);
        row = row.add_child(text_column_id);

        // Caps Lock warning glyph + polite live region.
        if self.caps_lock_warning
            && let Some(window) = ctx.window()
        {
            let caps = window.caps_lock().clone();
            let warn = TextWidget::new(lit!(CAPS_LOCK_GLYPH))
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary)
                .single_line()
                .access_role(Role::Status)
                .access_live(Live::Polite)
                .access_label(bastyde_i18n::tr_widget!(a11y_caps_lock_on()));
            let warn_id = ctx.add(warn);
            let visible = caps.zip(&focused).map(|(c, f)| *c && *f);
            ctx.visible_when(warn_id, visible);
            row = row.add_child(warn_id);
        }

        // Reveal affordance.
        match self.reveal_mode {
            RevealMode::Toggle => {
                let reveal = IconButton::visibility_toggle(revealed.clone())
                    .embedded()
                    .focusable(true)
                    .access_label(bastyde_i18n::tr_widget!(a11y_password_reveal()));
                row = row.add_child(ctx.add(reveal));
            }
            RevealMode::Hold => {
                let icon = (BuiltInIcons::global().eye)();
                let revealed_hold = revealed.clone();
                let hold = MinSize::new(24.0, 24.0)
                    .child(Center::new().child(icon))
                    .on_pointer_event(move |event, ctx| match event {
                        WidgetEvent::PointerDown { .. } => {
                            revealed_hold.set(true);
                            ctx.request_frame();
                            EventResponse::Handled
                        }
                        WidgetEvent::PointerUp { .. } | WidgetEvent::PointerLeave => {
                            revealed_hold.set(false);
                            ctx.request_frame();
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    })
                    .cursor(CursorIcon::Pointer)
                    .access_role(Role::Button)
                    .access_label(bastyde_i18n::tr_widget!(a11y_password_reveal()));
                row = row.add_child(ctx.add(hold));
            }
            RevealMode::None => {}
        }

        let row_id = ctx.add(
            row.focus_within(focused.clone())
                .hover_within(hovered.clone()),
        );

        // ── Frame chrome via the (reused) TextInputStyle ────────────
        let effective_enabled = ctx.effective_enabled_signal(self_id);
        let is_disabled = effective_enabled.map(|on| !*on);
        let validation_level = inner_feedback.map(|fb| match fb {
            ValidationFeedback::Invalid { .. } => TextInputValidationLevel::Error,
            ValidationFeedback::Corrected { .. } => TextInputValidationLevel::Corrected,
            ValidationFeedback::Pristine | ValidationFeedback::Valid => {
                TextInputValidationLevel::None
            }
        });

        let style: SharedTextInputStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.text_input.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeTextInputStyle));

        let cfg = TextInputStyleConfig {
            editor: row_id,
            is_focused: focused.clone(),
            is_hovered: hovered.clone(),
            is_disabled,
            validation: validation_level,
            variant: self.variant,
        };
        let chrome_id = style.make_body(&cfg, ctx);

        let min_w = self.min_width.unwrap_or(65.0);
        let frame_id =
            ctx.add(MinSize::new(min_w, field_dims::TEXT_FIELD_HEIGHT).child_id(chrome_id));

        // ── Inline validation strip ─────────────────────────────────
        let strip_id = ctx.add(ValidationStrip::new(inner_feedback));

        let root_id = ctx.add(
            VStack::new()
                .spacing(field_dims::TEXT_FIELD_VALIDATION_STRIP_GAP)
                .add_child(frame_id)
                .add_child(strip_id),
        );

        // Tooltips — mutually exclusive (setters clear the others).
        if let Some(content) = self.composite_tooltip_content.take() {
            tooltip::attach_composite_tooltip_boxed(
                ctx,
                root_id,
                content,
                tooltip::DEFAULT_COMPOSITE_TOOLTIP_DELAY,
            );
        } else if let Some(source) = self.rich_tooltip_source.take() {
            tooltip::attach_rich_tooltip_source(
                ctx,
                root_id,
                source,
                tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
            );
        } else if let Some(ref text) = self.tooltip_text {
            let tooltip_id = ctx.add(crate::tooltip::TooltipWidget::new(lit!(text)));
            ctx.attach_tooltip(root_id, tooltip_id, std::time::Duration::from_millis(500));
        }

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(p) = children.first_mut() {
            p.origin = Point::new(bounds.x, bounds.y);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
