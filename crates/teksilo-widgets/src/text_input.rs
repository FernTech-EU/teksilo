// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TextInput` — styled single-line text field composite.
//!
//! Wraps the [`TextInputField`]
//! editing primitive in a bordered, padded frame with placeholder
//! overlay, validation, optional clear button, and leading/trailing
//! slots. All actual text editing is delegated to the field: every
//! configuration method here has a direct counterpart on the
//! primitive.
//!
//! Most applications want `TextInput`. Choose
//! [`TextInputField`] directly
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
//!
//! ## Touch and pen
//!
//! The trailing clear affordance is 16 dp of paint and a 24 dp target: raising
//! its box would widen every field in the workspace at Compact, so the
//! shortfall is made up between the pointer and the arena through
//! `Widget::hit_outset` — declared by the slot that takes the tap, because the
//! ring around an outset resolves to the declaring node rather than to a
//! descendant, and by a direct child of the row, because an outset never
//! escapes its parent. The slot keeps its 16 dp while the affordance is hidden
//! so the row does not jump, and withdraws its outset while there is nothing to
//! clear. The caret and selection behaviour of the field itself belongs to the
//! touch-text package.

mod widget_impl;

#[cfg(test)]
mod tests;

use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{
    SharedTextInputStyle, TextInputStyle, TextInputStyleConfig, TextInputValidationLevel,
};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Alignment, TextRole, TextStyleRole};

use crate::button::InteractionState;
use crate::primitives::text_input_field::{TextInputField, ValidationFeedback};
use crate::primitives::validation_strip::ValidationStrip;
use crate::primitives::{Expand, HStack, MinSize, Padding, Shrinkable, TextWidget, VStack, ZStack};
use crate::tooltip::{self, RichTooltipSource};

// Re-export the variant enum at module top so callers can write
// `TextInput::new(text).variant(TextInputVariant::Filled)` without a
// deeper import path.
pub use teksilo_core::styles::TextInputVariant;
use teksilo_i18n::LocalizedString;

/// Validation state for the text input field.
///
/// Drives the inline feedback strip and border tint of [`TextInput`].
#[derive(Debug, Clone, Default)]
pub enum ValidationState {
    /// No validation message — the field is pristine or valid.
    #[default]
    None,
    /// The committed value is invalid; `LocalizedString` is shown in red below the field.
    Error(LocalizedString),
    /// The committed value is suspicious but accepted; `LocalizedString` is shown as a warning.
    Warning(LocalizedString),
    /// Last commit was auto-corrected; the field's value has already
    /// been replaced with the normalized form. The composite renders
    /// the message in secondary text and tints the border accent
    /// briefly (decay-managed by the framework's frame loop, not a
    /// concern of this enum).
    Corrected(LocalizedString),
}

/// Styled single-line text input composite.
///
/// See the [module-level documentation](self) for usage examples.
pub struct TextInput {
    // ── Configuration forwarded to the inner TextInputField ─────────
    text: Signal<String>,
    placeholder: LocalizedString,
    /// Enabled state, static or reactive; forwarded to the arena and the
    /// inner `TextInputField` at build time.
    enabled: Prop<bool>,
    read_only: bool,
    max_length: Option<usize>,
    on_submit: Option<Box<dyn Fn(&mut EventContext)>>,
    on_access_set_value: Option<std::rc::Rc<dyn Fn(&str, &mut EventContext) -> bool>>,
    on_blur: Option<Box<dyn Fn(&mut EventContext)>>,
    char_filter: Option<std::rc::Rc<dyn Fn(char) -> bool>>,
    suffix: String,
    /// Optional input-mask grammar string (Qt syntax). Forwarded
    /// 1:1 to `TextInputField::input_mask`. Used by composing
    /// widgets like `DateEdit` that need a position-aware filter
    /// + auto-derived placeholder template (`__/__/____`).
    input_mask: Option<String>,
    /// Semantic input purpose (WCAG 1.3.5) forwarded to the inner
    /// `TextInputField` to select a specialised AT role.
    input_purpose: crate::primitives::text_input_field::InputPurpose,
    /// ARIA combobox wiring, forwarded verbatim to the inner
    /// `TextInputField` (the node that actually holds focus).
    active_descendant: Option<Signal<Option<WidgetId>>>,
    controls: Option<Signal<Option<WidgetId>>>,
    /// Optional validator closure. Forwarded 1:1 to
    /// `TextInputField::validator`. Runs on commit (Enter, Tab-out,
    /// blur). Set this AND `validation_feedback` together for
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
    /// Handed out by [`Self::handle`] before build, adopted by the inner field
    /// at build time — so the two are one handle, not two that agree by luck.
    field_handle: crate::primitives::TextFieldHandle,
    /// Arena id of the inner `TextInputField`, filled in by `build`.
    /// Shared, so a handle taken before `ctx.add` sees it afterwards.
    field_id_slot: std::rc::Rc<std::cell::Cell<Option<WidgetId>>>,
    /// Mirrored from the inner field's `validation_feedback_signal`
    /// during `build`. Composing widgets that install a `validator`
    /// read this to compose feedback across multiple fields (range
    /// editor's worse-of-two ladder, etc.).
    feedback_signal: Signal<ValidationFeedback>,

    // ── Configuration owned by this composite only ──────────────────
    label: Option<LocalizedString>,
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
    /// Set by `.validation_feedback(...)`; wired via `ctx.effect`
    /// in `build()` so the bridge outlives construction.
    feedback_to_bridge: Option<Signal<ValidationFeedback>>,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn teksilo_core::widget::Widget>>,

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
            .field("enabled", &self.enabled.get())
            .finish_non_exhaustive()
    }
}

impl TextInput {
    /// Construct a new text input bound to `text`.
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            placeholder: LocalizedString::literal(String::new()),
            enabled: Prop::Static(true),
            read_only: false,
            max_length: None,
            on_submit: None,
            on_access_set_value: None,
            on_blur: None,
            char_filter: None,
            suffix: String::new(),
            input_mask: None,
            input_purpose: crate::primitives::text_input_field::InputPurpose::Normal,
            active_descendant: None,
            controls: None,
            validator: None,
            caret_position_slot: std::rc::Rc::new(std::cell::RefCell::new(None)),
            caret_setter_slot: std::rc::Rc::new(std::cell::RefCell::new(None)),
            field_handle: crate::primitives::TextFieldHandle::detached(),
            field_id_slot: std::rc::Rc::new(std::cell::Cell::new(None)),
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

    /// Set the placeholder text shown when the field is empty.
    pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.placeholder = ls;
        self
    }

    /// Accessible name for the field.
    ///
    /// Applied to the inner `TextInputField` — the node that carries
    /// `Role::TextInput`, holds focus, and reports the document's value.
    /// It deliberately does *not* go on the composite's outer node: that
    /// node is a `Role::GenericContainer`, which
    /// `accesskit_consumer::common_filter` drops from the filtered tree
    /// unconditionally, so a name placed there would be invisible to every
    /// screen reader on every platform.
    ///
    /// Stays locale-reactive: a `tr!(...)` name is re-resolved when the
    /// locale changes, without a rebuild.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena and the inner `TextInputField` at build time.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Set the field read-only: text is selectable and copyable but not editable.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Limit the number of Unicode scalar values the field will accept.
    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    /// Show or hide the trailing ✕ button that clears the field text. Default: hidden.
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

    /// Handle an assistive technology's whole-value write, given the string it
    /// set. Forwarded 1:1 to `TextInputField::on_access_set_value`, where the
    /// reasoning lives. Composites whose text projects a typed value —
    /// `SpinBox`, the date and time editors — install one.
    pub fn on_access_set_value(
        mut self,
        f: impl Fn(&str, &mut EventContext) -> bool + 'static,
    ) -> Self {
        self.on_access_set_value = Some(std::rc::Rc::new(f));
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

    /// Declare the field's semantic [`InputPurpose`](crate::primitives::InputPurpose)
    /// (WCAG 1.3.5), forwarded to the inner `TextInputField` to select a
    /// specialised AT role (e.g. `Role::EmailInput`).
    pub fn input_purpose(
        mut self,
        purpose: crate::primitives::text_input_field::InputPurpose,
    ) -> Self {
        self.input_purpose = purpose;
        self
    }

    /// Publish `active_descendant` on the inner field, pointing at the row a
    /// separate listbox is currently highlighting (the ARIA combobox pattern).
    /// Forwarded 1:1 to [`TextInputField::active_descendant`], which is where
    /// it has to land: AT follows the *focused* node's active descendant, and
    /// the inner field is the focusable one.
    pub fn active_descendant(mut self, active: Signal<Option<WidgetId>>) -> Self {
        self.active_descendant = Some(active);
        self
    }

    /// Publish a `controls` relation to the listbox this input drives.
    /// Forwarded 1:1 to [`TextInputField::controls`].
    pub fn controls(mut self, listbox: Signal<Option<WidgetId>>) -> Self {
        self.controls = Some(listbox);
        self
    }

    /// Install a commit-time validator. Forwarded 1:1 to
    /// [`TextInputField::validator`]. Pair with
    /// [`Self::validation_feedback_signal`] (or
    /// [`Self::validation_feedback`]) to surface the outcome
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

    /// A live handle on the inner field — its text-editing commands, for a
    /// host outside the widget.
    ///
    /// Mirrors [`TextInputField::handle`], and exists for the same reason: an
    /// application that routes Undo, Cut, Copy, Paste and Select All to
    /// "whichever text surface holds the caret" must be able to reach *every*
    /// such surface. A `TextInput` that could not be reached would silently
    /// lose its own Ctrl+Z to whatever the host routed the chord at instead.
    ///
    /// Like [`caret_setter`](Self::caret_setter), safe to take before `build`:
    /// the handle reaches the field through a slot the widget fills in.
    pub fn handle(&self) -> crate::primitives::TextFieldHandle {
        self.field_handle.clone()
    }

    /// The arena id of the inner field: the node that holds focus, carries
    /// `Role::TextInput` and reports the document's value.
    ///
    /// A `TextInput` is a composite whose outer node is a
    /// `Role::GenericContainer`. That node is neither focusable nor present in
    /// the filtered accessibility tree, so a host that has to *name* the focus
    /// target cannot use the id `ctx.add` returned it. Two cases need the
    /// name: a form sending focus back to the field a validator refused, and a
    /// modal whose own `initial_focus_hint` picks one field out of several.
    ///
    /// Empty until `build` runs, like [`caret_setter`](Self::caret_setter);
    /// take the handle before `ctx.add(text_input)` and read it after.
    ///
    /// A host that only needs "focus this input, whichever node that is" wants
    /// [`EventContext::request_focus_into`] on the outer id instead, and no
    /// handle at all.
    ///
    /// [`EventContext::request_focus_into`]: teksilo_core::widget::EventContext::request_focus_into
    pub fn field_id(&self) -> std::rc::Rc<std::cell::Cell<Option<WidgetId>>> {
        self.field_id_slot.clone()
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

    /// Bind an external [`ValidationState`] signal directly (e.g. when
    /// validation runs server-side), or set a fixed initial value. Use
    /// [`validation_feedback`](Self::validation_feedback)
    /// when wiring a local validator's output.
    ///
    /// A bound `Signal` becomes the shared write target used internally
    /// (by the validator-feedback bridge) and externally by the caller —
    /// preserving the two-way channel this method has always offered. A
    /// static value seeds a fresh, unshared signal.
    pub fn validation(mut self, validation: impl Into<Prop<ValidationState>>) -> Self {
        self.validation = validation.into().as_signal();
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
    pub fn validation_feedback(mut self, feedback: Signal<ValidationFeedback>) -> Self {
        let target = self.validation.clone();
        // Snapshot once now so we observe the current state at construction
        // time too (subsequent changes flow via the field's own commit
        // pipeline; ctx.effect installed in build() does the live tracking).
        target.set(feedback_to_state(&feedback.get()));
        self.feedback_to_bridge = Some(feedback);
        self
    }

    /// Attach a plain tooltip. Accepts `tr!(...)` or `lit!(...)`.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a registry-driven rich tooltip by key. Mutually exclusive with
    /// `tooltip` and `composite_tooltip` (last call wins).
    pub fn rich_tooltip_key(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach an inline rich tooltip from a pre-built [`tooltip::TooltipContent`].
    /// Mutually exclusive with `tooltip` and `composite_tooltip` (last call wins).
    pub fn rich_tooltip(mut self, content: tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach an inline rich tooltip from a pre-built [`tooltip::TooltipContent`].
    /// Canonical alias for [`Self::rich_tooltip`] — matches the name used by
    /// `Button`, `ComboBox`, and other widgets. Mutually exclusive with
    /// `tooltip` and `composite_tooltip` (last call wins).
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree. See [`Button::composite_tooltip`](crate::button::Button::composite_tooltip).
    pub fn composite_tooltip(
        mut self,
        content: impl teksilo_core::widget::Widget + 'static,
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
