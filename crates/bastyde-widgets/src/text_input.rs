//! `TextInput` — styled single-line text field composite.
//!
//! Wraps the [`TextInputField`](crate::primitives::TextInputField)
//! editing primitive in a bordered, padded frame with placeholder
//! overlay, validation, optional clear button, and leading/trailing
//! slots. All actual text editing is delegated to the field: every
//! configuration method here has a direct counterpart on the
//! primitive.
//!
//! Most applications want `TextInput`. Choose
//! [`TextInputField`](crate::primitives::TextInputField) directly
//! when you're building a composite of your own that already
//! supplies its frame — `SpinBox` is the canonical in-tree example.
//!
//! # Example
//!
//! ```ignore
//! let search = ctx.signal(String::new());
//! TextInput::new(search.clone())
//!     .placeholder("Search...")
//!     .show_clear_button(true)
//!     .leading_slot(IconWidget::from_svg(SEARCH_ICON))
//!     .on_submit_fn(|ctx| ctx.send_intent(AppIntent::Search))
//! ```

#[cfg(test)]
mod tests;

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    SharedTextInputStyle, TextInputStyle, TextInputStyleConfig, TextInputValidationLevel,
};
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::button::InteractionState;
use crate::primitives::text_input_field::{TextInputField, ValidationFeedback};
use crate::primitives::validation_strip::ValidationStrip;
use crate::primitives::{Expand, HStack, MinSize, Padding, TextWidget, VStack, ZStack};
use crate::tooltip::{self, RichTooltipSource};

// Re-export the variant enum at module top so callers can write
// `TextInput::new(text).variant(TextInputVariant::Filled)` without a
// deeper import path.
pub use bastyde_core::styles::TextInputVariant;

/// Validation state for the text input field.
#[derive(Debug, Clone, Default)]
pub enum ValidationState {
    #[default]
    None,
    Error(String),
    Warning(String),
    /// Last commit was auto-corrected; the field's value has already
    /// been replaced with the normalized form. The composite renders
    /// the message in secondary text and tints the border accent
    /// briefly (decay-managed by the framework's frame loop, not a
    /// concern of this enum).
    Corrected(String),
}

/// Styled single-line text input composite.
///
/// See the [module-level documentation](self) for usage examples.
pub struct TextInput {
    // ── Configuration forwarded to the inner TextInputField ─────────
    text: Signal<String>,
    placeholder: String,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    read_only: bool,
    max_length: Option<usize>,
    on_submit: Option<Box<dyn Fn(&mut EventContext)>>,
    on_blur: Option<Box<dyn Fn(&mut EventContext)>>,
    char_filter: Option<std::rc::Rc<dyn Fn(char) -> bool>>,
    suffix: String,
    /// Optional input-mask grammar string (Qt syntax). Forwarded
    /// 1:1 to `TextInputField::input_mask`. Used by composing
    /// widgets like `DateEdit` that need a position-aware filter
    /// + auto-derived placeholder template (`__/__/____`).
    input_mask: Option<String>,
    /// Optional validator closure. Forwarded 1:1 to
    /// `TextInputField::validator`. Runs on commit (Enter, Tab-out,
    /// blur). Set this AND `bind_validation_feedback` together for
    /// the standard validator → feedback display pattern.
    validator: Option<crate::primitives::text_input_field::ValidatorFn>,
    /// Captured pre-build so composing widgets can read live caret
    /// position (DateEdit-style segment-stepping). Populated by
    /// `caret_position()` on first call; the inner field's own
    /// signal is mirrored into it during `build`.
    caret_position_slot: std::rc::Rc<std::cell::RefCell<Option<Signal<usize>>>>,
    /// Same idea as `caret_position_slot` but for the setter
    /// closure. Captured pre-build by `caret_setter()`.
    caret_setter_slot: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn(usize)>>>>,
    /// Mirrored from the inner field's `validation_feedback_signal`
    /// during `build`. Composing widgets that install a `validator`
    /// read this to compose feedback across multiple fields (range
    /// editor's worse-of-two ladder, etc.).
    feedback_signal: Signal<ValidationFeedback>,

    // ── Configuration owned by this composite only ──────────────────
    label: Option<String>,
    /// Optional override for the frame's intrinsic minimum width
    /// (default 65 dp). Composing widgets like `DateEdit` /
    /// `TimeEdit` raise this so the frame stays at the design
    /// width even when typed content shrinks. Wired into the inner
    /// `MinSize` wrapper around the ZStack frame — NOT the outer
    /// VStack — so the floor doesn't fight the VStack's
    /// `proposal.width.unwrap_or(max_width)` rule.
    min_width: Option<f32>,
    show_clear_button: bool,
    leading_slot: Option<Box<dyn Widget>>,
    trailing_slot: Option<Box<dyn Widget>>,
    validation: Signal<ValidationState>,
    /// Set by `.bind_validation_feedback(...)`; wired via `ctx.effect`
    /// in `build()` so the bridge outlives construction.
    feedback_to_bridge: Option<Signal<ValidationFeedback>>,
    tooltip_text: Option<String>,
    rich_tooltip_source: Option<RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,

    /// Tier-1 design-language variant. Drives which chrome the active
    /// `TextInputStyle` paints around the editor (Outlined / Filled /
    /// Underline / Bare).
    variant: TextInputVariant,
    /// Per-call style override.
    style_override: Option<SharedTextInputStyle>,

    // ── Internal (set during build) ─────────────────────────────────
    interaction: Signal<InteractionState>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for TextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInput")
            .field("placeholder", &self.placeholder)
            .field("initial_enabled", &self.initial_enabled)
            .finish_non_exhaustive()
    }
}

impl TextInput {
    /// Construct a new text input bound to `text`.
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            placeholder: String::new(),
            initial_enabled: true,
            read_only: false,
            max_length: None,
            on_submit: None,
            on_blur: None,
            char_filter: None,
            suffix: String::new(),
            input_mask: None,
            validator: None,
            caret_position_slot: std::rc::Rc::new(std::cell::RefCell::new(None)),
            caret_setter_slot: std::rc::Rc::new(std::cell::RefCell::new(None)),
            feedback_signal: Signal::new(ValidationFeedback::Pristine),
            label: None,
            min_width: None,
            show_clear_button: false,
            leading_slot: None,
            trailing_slot: None,
            validation: Signal::new(ValidationState::None),
            feedback_to_bridge: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            variant: TextInputVariant::default(),
            style_override: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Pick a Tier-1 design-language variant
    /// ([`TextInputVariant::Outlined`] / `Filled` / `Underline` / `Bare`).
    /// The IntUI default ([`crate::styles::RecipeTextInputStyle`]) honours
    /// `Outlined`, `Filled`, and `Bare`; `Underline` falls back to
    /// `Outlined` until per-side stroke recipes land.
    pub fn variant(mut self, variant: TextInputVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Override the active [`TextInputStyle`] for this widget instance
    /// only. The widget keeps responsibility for caret blinking, IME
    /// composition, the placeholder layering, the leading / trailing
    /// slots and the validation strip — the style only paints the
    /// frame (border / fill / corner radius).
    pub fn style(mut self, style: impl TextInputStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    // ── Builder methods ─────────────────────────────────────────────
    //
    // Every method below that has a direct analogue on
    // `TextInputField` forwards to it 1:1 at build time — the
    // `TextInput` composite just owns the framing around the field.

    pub fn placeholder(mut self, text: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = text.into();
        self.placeholder = ls.resolve_now();
        self
    }

    /// Accessible name for the composite. Propagated to the outer
    /// container's a11y node; the inner `TextInputField` still
    /// carries `Role::TextInput` with the document's value.
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. Use `ctx.enabled_when(input_id, signal)` for reactivity.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    pub fn show_clear_button(mut self, show: bool) -> Self {
        self.show_clear_button = show;
        self
    }

    /// Override the frame's intrinsic minimum width (default 65 dp).
    /// Use to express a design width for date / time / phone-number
    /// fields whose content is well-known and whose collapse to the
    /// generic 65 dp floor would look out of place.
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = Some(w.max(0.0));
        self
    }

    /// Set an arbitrary widget in the leading slot (before the text area).
    /// Typically an `IconButton` or `IconWidget`.
    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot = Some(Box::new(widget));
        self
    }

    /// Set an arbitrary widget in the trailing slot (after the text area).
    /// Typically an `IconButton` or `IconWidget`.
    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot = Some(Box::new(widget));
        self
    }

    /// Closure invoked on Enter. Forwarded to `TextInputField`.
    pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_submit = Some(Box::new(f));
        self
    }

    /// Closure invoked on focus loss. Forwarded to `TextInputField`.
    pub fn on_blur_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_blur = Some(Box::new(f));
        self
    }

    /// Per-character input-filter predicate. Forwarded to
    /// `TextInputField`.
    pub fn char_filter(mut self, f: impl Fn(char) -> bool + 'static) -> Self {
        self.char_filter = Some(std::rc::Rc::new(f));
        self
    }

    /// Non-editable trailing string (Qt's `QSpinBox::suffix`).
    /// Forwarded to `TextInputField`.
    pub fn suffix(mut self, text: impl Into<String>) -> Self {
        self.suffix = text.into();
        self
    }

    /// Install an input mask (Qt grammar). Forwarded 1:1 to
    /// [`TextInputField::input_mask`]. Composing widgets like
    /// `DateEdit` use this to project the date format pattern
    /// onto the editing surface.
    pub fn input_mask(mut self, mask: impl Into<String>) -> Self {
        self.input_mask = Some(mask.into());
        self
    }

    /// Install a commit-time validator. Forwarded 1:1 to
    /// [`TextInputField::validator`]. Pair with
    /// [`Self::validation_feedback_signal`] (or
    /// [`Self::bind_validation_feedback`]) to surface the outcome
    /// in the inline strip.
    pub fn validator(
        mut self,
        f: impl Fn(&str) -> crate::primitives::text_input_field::ValidationOutcome + 'static,
    ) -> Self {
        self.validator = Some(std::rc::Rc::new(f));
        self
    }

    /// Reactive caret position. Mirrors the inner field's
    /// [`TextInputField::caret_position`] after `build`. Capture
    /// before `ctx.add(text_input)` — used by composing widgets
    /// (`DateEdit` segment-stepping) that need to know which
    /// segment Up/Down should step.
    pub fn caret_position(&self) -> Signal<usize> {
        let mut slot = self.caret_position_slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Signal::new(0));
        }
        slot.as_ref().unwrap().clone()
    }

    /// Programmatic caret setter. Mirrors the inner field's
    /// [`TextInputField::caret_setter`]. Returns a closure that
    /// is a no-op until `build` runs; afterwards it walks the
    /// inner field's state and moves the document cursor. Capture
    /// before `ctx.add(text_input)`.
    pub fn caret_setter(&self) -> std::rc::Rc<dyn Fn(usize)> {
        let slot = self.caret_setter_slot.clone();
        std::rc::Rc::new(move |position: usize| {
            if let Some(setter) = slot.borrow().as_ref() {
                (setter)(position);
            }
        })
    }

    /// Reactive published validation feedback. Mirrors the inner
    /// field's [`TextInputField::validation_feedback_signal`]
    /// after `build`. Composing widgets observe this to compose
    /// feedback across multiple fields (range editor's
    /// worse-of-two ladder, etc.).
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback_signal.clone()
    }

    pub fn validation(mut self, validation: Signal<ValidationState>) -> Self {
        self.validation = validation;
        self
    }

    /// Bridge a `Signal<ValidationFeedback>` (typically from a
    /// validator-equipped widget like `DateEdit::validation_feedback_signal`
    /// or a custom `TextInputField`) into this composite's
    /// `ValidationState`. The feedback is mirrored on every change,
    /// translating outcomes into the composite's display vocabulary:
    ///
    /// - `Pristine` / `Valid` → `ValidationState::None`
    /// - `Corrected { message, .. }` → `ValidationState::Corrected(message)`
    /// - `Invalid { message }` → `ValidationState::Error(message)`
    pub fn bind_validation_feedback(mut self, feedback: Signal<ValidationFeedback>) -> Self {
        let target = self.validation.clone();
        // Snapshot once now so we observe the current state at construction
        // time too (subsequent changes flow via the field's own commit
        // pipeline; ctx.effect installed in build() does the live tracking).
        target.set(feedback_to_state(&feedback.get()));
        self.feedback_to_bridge = Some(feedback);
        self
    }

    /// Attach a plain tooltip. Accepts `tr!(...)` or `lit!(...)`.
    pub fn tooltip(mut self, text: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    pub fn rich_tooltip_key(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    pub fn rich_tooltip(mut self, content: tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree. See [`Button::composite_tooltip`](crate::button::Button::composite_tooltip).
    pub fn composite_tooltip(
        mut self,
        content: impl bastyde_core::widget::Widget + 'static,
    ) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    // ── Signal accessors (call before add to tree) ──────────────────

    /// The reactive text content signal.
    pub fn text(&self) -> Signal<String> {
        self.text.clone()
    }
}

impl Widget for TextInput {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // TextInput is a heavy composite. We snapshot the theme once for
        // static layout params (padding, border width, field height); the
        // placeholder, clear-icon tint, and border/width are driven by
        // roles and state signals, so theme switches repaint via the
        // paint-time role resolver without riding through a zip here.
        let _theme = ctx.theme();
        use crate::styles::recipe_text_input_style as field_dims;
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let interaction = self.interaction.clone();
        let validation = self.validation.clone();

        // ── Build the inner editing primitive ──────────────────────
        //
        // The inner field owns the bound text signal, the document,
        // engine, caret, clipboard, context menu — everything
        // interactive. The composite just styles it.
        let inner_height =
            (field_dims::TEXT_FIELD_HEIGHT - 2.0 * field_dims::TEXT_FIELD_BORDER_WIDTH).max(0.0);
        let text_area_height =
            (inner_height - 2.0 * field_dims::TEXT_FIELD_PADDING_VERTICAL).max(0.0);

        let mut field = TextInputField::new(self.text.clone())
            .enabled(self.initial_enabled)
            .read_only(self.read_only)
            .placeholder(self.placeholder.clone())
            .text_height(text_area_height)
            .interaction_signal(interaction.clone());
        if let Some(max) = self.max_length {
            field = field.max_length(max);
        }
        if let Some(f) = self.char_filter.take() {
            // Re-wrap the Rc'd closure into a plain closure for the
            // primitive's builder surface, which owns its own Rc.
            field = field.char_filter(move |c| (f)(c));
        }
        if let Some(cb) = self.on_submit.take() {
            field = field.on_submit_fn(move |ctx| (cb)(ctx));
        }
        if let Some(cb) = self.on_blur.take() {
            field = field.on_blur_fn(move |ctx| (cb)(ctx));
        }
        if !self.suffix.is_empty() {
            field = field.suffix(std::mem::take(&mut self.suffix));
        }
        if let Some(mask) = self.input_mask.take() {
            field = field.input_mask(mask);
        }
        let validator_installed = self.validator.is_some();
        if let Some(validator) = self.validator.take() {
            // ValidatorFn is `Rc<dyn Fn(&str) -> ValidationOutcome>`.
            // The primitive's builder takes a fresh closure; wrap the
            // Rc in one so the caller can keep their own clones if
            // they captured it before.
            field = field.validator(move |s| (validator)(s));
        }

        // Expose the field's text signal for downstream reactivity
        // (placeholder visibility, clear-button visibility) before
        // the field is consumed by `ctx.add`.
        let text_signal_for_vis = field.text();

        // Capture the inner field's reactive accessors BEFORE
        // `ctx.add` consumes it, so composing widgets that called
        // `caret_position()` / `caret_setter()` /
        // `validation_feedback_signal()` on us pre-build see live
        // updates through the slots we mirror into.
        let inner_caret = field.caret_position();
        let inner_setter = field.caret_setter();
        let inner_feedback = field.validation_feedback_signal();

        // Text editing area, wrapped in vertical padding so slots
        // (IconButton etc.) sit flush against top/bottom of the
        // inner border area and are vertically centered by the HStack.
        let padded_field = Padding::new(
            field_dims::TEXT_FIELD_PADDING_VERTICAL,
            0.0,
            field_dims::TEXT_FIELD_PADDING_VERTICAL,
            0.0,
        )
        .child(field);

        // The placeholder lives in a local ZStack with the text field so
        // it shares the same column in the HStack — no overlap with
        // leading/trailing slots. The text field is the last ZStack child
        // so it wins hit-testing (ZStack tests children in reverse order).
        // `respect_intrinsic` on these `Expand` wrappers preserves the
        // wrapped field's natural width (≈200 dp from `TextInputField`)
        // as the column's intrinsic width. The enclosing `ZStack`
        // measures its children with an unspecified proposal, so the
        // parent's offered width never reaches the `HStack` during
        // measurement — without auto-basis the column reports 0 dp and
        // the whole composite collapses to `MinSize`'s 65 dp floor.
        let text_column_id = if !self.placeholder.is_empty() {
            // Match the inner TextInputField's text style + single-line
            // behaviour so the placeholder layout box has the same
            // intrinsic height as the rich-text engine's frame. Without
            // `single_line()` the placeholder defaults to Wrap, which
            // can report extra vertical leading space.
            let ph = TextWidget::new(self.placeholder.clone())
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary)
                .single_line()
                .a11y_hidden();
            // Center the placeholder vertically within the column.
            // `Padding(top=padding_vertical, bottom=padding_vertical)`
            // pinned the placeholder to the top of its inset box, but
            // the rich-text engine inside the field paints glyphs with
            // its own line-leading offset, so the two paths drifted
            // by a few pixels. `Center` aligns purely on the layout
            // box midline, which matches the engine's frame midline.
            let ph_id = ctx.add(
                Expand::new()
                    .respect_intrinsic()
                    .child(crate::primitives::Center::new().child(ph)),
            );
            let visible = text_signal_for_vis.map(|t| t.is_empty());
            ctx.visible_when(ph_id, visible);

            ctx.add(
                Expand::horizontal().respect_intrinsic().child(
                    ZStack::new()
                        .add_child(ph_id) // below (placeholder)
                        .child(padded_field), // on top (text field, gets hits)
                ),
            )
        } else {
            ctx.add(Expand::horizontal().respect_intrinsic().child(padded_field))
        };

        // HStack: [leading] [text_column] [clear] [trailing]
        let mut row = HStack::new().spacing(4.0);

        if let Some(leading) = self.leading_slot.take() {
            let leading_id = ctx.add_boxed(leading);
            row = row.add_child(leading_id);
        }

        row = row.add_child(text_column_id);

        // Clear button (opt-in). The clear affordance clears the
        // bound text signal — the field's ext→internal effect
        // picks this up and wipes the document.
        if self.show_clear_button {
            let icon = (crate::icon_button::BuiltInIcons::global().clear)()
                .icon_size(12.0)
                .color(TextRole::Secondary);
            let text_for_clear = self.text.clone();
            let clear_id = ctx.add(
                MinSize::new(16.0, 16.0)
                    .child(crate::primitives::Center::new().child(icon))
                    .on_tap(move |_pos, ctx| {
                        text_for_clear.set(String::new());
                        ctx.request_frame();
                    })
                    .cursor(CursorIcon::Pointer),
            );
            let visible = text_signal_for_vis.map(|t| !t.is_empty());
            ctx.visible_when(clear_id, visible);
            let reserve_id = ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(16.0_f32)
                    .bind_height(16.0_f32)
                    .child_id(clear_id),
            );
            row = row.add_child(reserve_id);
        }

        if let Some(trailing) = self.trailing_slot.take() {
            let trailing_id = ctx.add_boxed(trailing);
            row = row.add_child(trailing_id);
        }

        let row_id = ctx.add(row);

        // Derive the cfg signals the style needs. Map our internal
        // `InteractionState` (5-way) to the trait's 3 boolean signals,
        // and the composite `ValidationState` (carries a message) to
        // the trait's flat `TextInputValidationLevel` enum.
        let is_focused = interaction.map(|s| *s == InteractionState::Focused);
        let is_hovered = interaction.map(|s| *s == InteractionState::Hovered);
        // `is_disabled` derives from the arena (not from interaction).
        let effective_enabled = ctx.effective_enabled_signal(self_id);
        let is_disabled = effective_enabled.map(|on| !*on);
        let validation_level = validation.map(|v| match v {
            ValidationState::None => TextInputValidationLevel::None,
            ValidationState::Error(_) => TextInputValidationLevel::Error,
            ValidationState::Warning(_) => TextInputValidationLevel::Warning,
            ValidationState::Corrected(_) => TextInputValidationLevel::Corrected,
        });

        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeTextInputStyle` default. The style paints the
        // bordered/filled frame + the corner radius + the horizontal
        // padding around the editor row.
        let style: SharedTextInputStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.text_input.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeTextInputStyle));

        let cfg = TextInputStyleConfig {
            editor: row_id,
            is_focused,
            is_hovered,
            is_disabled,
            validation: validation_level,
            variant: self.variant,
        };
        let chrome_id = style.make_body(&cfg, ctx);

        let min_w = self.min_width.unwrap_or(65.0);
        let frame_id =
            ctx.add(MinSize::new(min_w, field_dims::TEXT_FIELD_HEIGHT).child_id(chrome_id));

        // ── Inline validation strip ────────────────────────────────
        // Maps `Signal<ValidationState>` to the `Signal<ValidationFeedback>`
        // that `ValidationStrip` consumes. Empty/Pristine renders nothing
        // (zero height) so the layout doesn't reflow.
        let strip_feedback: Signal<ValidationFeedback> = self.validation.map(|v| match v {
            ValidationState::None => ValidationFeedback::Pristine,
            ValidationState::Error(msg) | ValidationState::Warning(msg) => {
                ValidationFeedback::Invalid {
                    message: msg.clone(),
                }
            }
            ValidationState::Corrected(msg) => ValidationFeedback::Corrected {
                message: msg.clone(),
                since: std::time::Instant::now(),
            },
        });
        let strip_id = ctx.add(ValidationStrip::new(strip_feedback));

        // Wrap frame + strip in a VStack with the configured gap.
        let root_id = ctx.add(
            VStack::new()
                .spacing(field_dims::TEXT_FIELD_VALIDATION_STRIP_GAP)
                .add_child(frame_id)
                .add_child(strip_id),
        );

        // Tooltip — three mutually-exclusive setters; setters clear
        // the others so exactly one branch runs.
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
            let tw = crate::tooltip::TooltipWidget::new(lit!(text));
            let tooltip_id = ctx.add(tw);
            let delay = std::time::Duration::from_millis(500);
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        // The interaction signal no longer carries Disabled — the
        // framework's arena enabled-state is the single source of
        // truth. Style chrome that needs `is_disabled` derives it
        // from `effective_enabled_signal(self_id)`.

        // Bridge `bind_validation_feedback` source → composite state.
        // No dedupe — each commit changes the feedback identity even
        // when the user-visible message stays the same (e.g. repeated
        // Invalid commits), and the strip is cheap to repaint.
        if let Some(src) = self.feedback_to_bridge.clone() {
            let target = self.validation.clone();
            ctx.effect(&src, move |fb| {
                target.set(feedback_to_state(fb));
            });
        } else if validator_installed {
            // Auto-bridge: a validator was installed but no explicit
            // `bind_validation_feedback` source was provided. Mirror
            // the inner field's published outcome into our display
            // state so calling `.validator(...)` on TextInput "just
            // works" — the strip and border respond without a
            // separate `.bind_validation_feedback(...)` call.
            let target = self.validation.clone();
            let src = inner_feedback.clone();
            ctx.effect(&src, move |fb| {
                target.set(feedback_to_state(fb));
            });
        }

        // Mirror inner field accessors into the slots that were
        // captured before build by composing widgets.
        //
        // - caret_position: only mirror if the slot was lazy-initialized
        //   (i.e. someone called `caret_position()` on us pre-build).
        //   Seed with the current value, then forward changes.
        // - caret_setter: store the inner field's setter Rc; the closure
        //   we returned to callers forwards through this slot at call time.
        // - validation_feedback_signal: always mirror (the slot's signal
        //   is created in `new()` and may already have observers).
        if let Some(target) = self.caret_position_slot.borrow().clone() {
            target.set(inner_caret.get());
            ctx.effect(&inner_caret, move |pos| {
                if target.get() != *pos {
                    target.set(*pos);
                }
            });
        }
        *self.caret_setter_slot.borrow_mut() = Some(inner_setter);
        let outer_feedback = self.feedback_signal.clone();
        outer_feedback.set(inner_feedback.get());
        ctx.effect(&inner_feedback, move |fb| {
            outer_feedback.set(fb.clone());
        });

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The inner TextInputField handles Role::TextInput.
        // The outer composite is transparent to a11y except for a
        // pass-through label.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        // Framework a11y walker sets `set_disabled` from arena state.
    }
}

/// Project a `ValidationFeedback` (validator-pipeline outcome) onto a
/// `ValidationState` (composite display state). `Pristine` and `Valid`
/// both clear; `Corrected` and `Invalid` carry their messages through.
fn feedback_to_state(fb: &ValidationFeedback) -> ValidationState {
    match fb {
        ValidationFeedback::Pristine | ValidationFeedback::Valid => ValidationState::None,
        ValidationFeedback::Corrected { message, .. } => {
            ValidationState::Corrected(message.clone())
        }
        ValidationFeedback::Invalid { message } => ValidationState::Error(message.clone()),
    }
}
