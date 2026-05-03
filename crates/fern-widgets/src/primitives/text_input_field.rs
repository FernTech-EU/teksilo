//! `TextInputField` — editable single-line text surface primitive.
//!
//! This is the raw editing primitive that powers the styled
//! [`TextInput`](crate::text_input::TextInput) composite and any
//! other widget that needs inline editable text — [`SpinBox`] being
//! the primary second consumer.
//!
//! Unlike `TextInput`, `TextInputField` paints no frame, no
//! placeholder overlay, no validation border, and hosts no trailing
//! slots: it is the focusable text area only. Compose it yourself
//! with `RectWidget`, `Padding`, icons, clear buttons, etc. to
//! build a styled control. Focus indication is the composite's
//! responsibility — the Int UI convention is to thicken the
//! enclosing frame's border to `focus_ring_width` and recolor it
//! to the accent focus-ring color.
//!
//! Features:
//! - Bound `Signal<String>` for two-way text binding.
//! - Full keyboard editing (arrow keys, Home/End, Backspace/Delete,
//!   Ctrl+X/C/V, Ctrl+A, Ctrl+Z/Y), IME commit, and pointer caret
//!   positioning and drag-select.
//! - Optional per-character input filter
//!   ([`TextInputField::char_filter`]), max-length cap
//!   ([`TextInputField::max_length`]), and read-only mode
//!   ([`TextInputField::read_only`]).
//! - Commit hooks: Enter fires
//!   [`on_submit_fn`](TextInputField::on_submit_fn) and focus loss
//!   fires [`on_blur_fn`](TextInputField::on_blur_fn).
//! - Non-editable trailing
//!   [`suffix`](TextInputField::suffix), rendered flush-right inside
//!   the field's bounds (Qt's `QSpinBox::suffix`). Caret cannot
//!   enter it; clicks past the text end clamp to the last
//!   character.
//! - Right-click context menu (Cut / Copy / Paste / Select All).
//! - AccessKit `Role::TextInput` with value, selection, and
//!   character/word boundary metadata.
//!
//! # Example
//!
//! ```ignore
//! let text = ctx.signal(String::new());
//! ctx.add(
//!     TextInputField::new(text.clone())
//!         .placeholder("Enter a name…")
//!         .char_filter(|c| !c.is_ascii_digit())
//!         .on_submit_fn(|ctx| ctx.send_intent(MyIntent::Save)),
//! );
//! ```
//!
//! [`SpinBox`]: crate::spin_box::SpinBox

mod keyboard;
pub mod mask;
mod mouse;
pub(crate) mod state;
pub mod validator;

use std::rc::Rc;

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::EventResponse;
use fern_core::signal::{Prop, Signal};
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_text::text_document::{SelectionType, TextDocument};
use fern_text::{CursorDisplay, RichTextEngine, SharedTypesetter};
use fern_tokens::TextStyle;

use crate::button::InteractionState;
use crate::menu_item::MenuItem;
use crate::menu_list::{MenuList, MenuSeparator};
use crate::rich_text::paint::{PaintParams, paint_frame};

pub(crate) use self::state::{CharFilter, CommandFactory};
use self::state::{SharedState, TextInputConfig, TextInputState, sync_cursor_signals};

pub use self::mask::{InputMask, MaskClass, MaskError, MaskPosition};
pub use self::validator::{ValidationFeedback, ValidationOutcome, ValidatorFn};

/// Caret blink half-period (same as RichTextEditor).
const CARET_BLINK_INTERVAL: f32 = 0.5;

/// Debounce window for coalesced signal emission.
const DEBOUNCE_WINDOW_SECS: f32 = 0.150;

/// Horizontal scroll margin in pixels. The caret stays at least this
/// far from the left/right edge of the viewport.
const SCROLL_MARGIN: f32 = 4.0;

/// Default text-area height when the caller does not override it
/// via [`TextInputField::text_height`]. Picked to match the Int UI
/// `text_field.height` token minus 2×border — the value the
/// `TextInput` composite reports — so a bare `TextInputField`
/// added to a tree without its composite still looks right.
const DEFAULT_TEXT_HEIGHT: f32 = 20.0;

/// Editable single-line text surface primitive.
///
/// See the [module docs](self) for the full feature list and a
/// compositional example.
pub struct TextInputField {
    // ── Configuration (builder methods, consumed in build) ───────────
    text: Signal<String>,
    enabled: bool,
    read_only: bool,
    max_length: Option<usize>,
    placeholder: String,
    on_submit: Option<CommandFactory>,
    on_blur: Option<CommandFactory>,
    char_filter: Option<CharFilter>,
    /// Fixed trailing label rendered inside the field's border.
    /// Accepts both plain strings and `Signal<String>` — when bound,
    /// the field re-measures the suffix and relayouts each time the
    /// signal fires, so composites like `SpinBox` can derive the
    /// suffix from the widget state (e.g. hide it while
    /// `special_value_text` is active).
    suffix: Prop<String>,
    text_height: Option<f32>,
    external_interaction: Option<Signal<InteractionState>>,

    /// Optional input mask. When set, the field auto-derives a
    /// placeholder template (`__/__/____` for `99/99/9999`) and
    /// rejects non-fitting characters via a position-aware filter
    /// composed with the user's `char_filter`. See [`InputMask`] for
    /// the grammar.
    mask: Option<InputMask>,
    /// Visible char used for unfilled editable positions in the mask
    /// template. Defaults to the theme's
    /// `text_field.mask_placeholder_char` (typically `_`).
    mask_placeholder_override: Option<char>,
    /// Validator closure called on every commit (Enter, Tab-out,
    /// blur). Returns a [`ValidationOutcome`] that drives
    /// [`feedback`](Self::validation_feedback_signal).
    validator: Option<ValidatorFn>,
    /// Published feedback signal. Composites bind to this to render
    /// the inline validation strip below the field.
    feedback: Signal<ValidationFeedback>,

    // ── Internal (set during build) ─────────────────────────────────
    state: Option<SharedState>,
    /// Interaction signal actually used at runtime. Either the one
    /// supplied by a wrapping composite via
    /// [`TextInputField::interaction_signal`] or a fresh one owned
    /// by the field. Read by the focus handler to repaint a
    /// parent's focus ring / border on gain/loss.
    interaction: Signal<InteractionState>,
    /// Mirror of the inner state's `cursor_position` for external
    /// readers. Wired in `build()` via a `ctx.effect`. Composing
    /// widgets that need the caret (e.g. `DateEdit` for segment
    /// stepping) read this via [`TextInputField::caret_position`].
    caret_position: Signal<usize>,
    /// Late-bound handle to the inner `SharedState`, populated in
    /// `build()`. Lets composing widgets capture a `caret_setter`
    /// closure BEFORE the field is moved into the tree, then call
    /// it later to programmatically reposition the caret. Required
    /// because the inner state doesn't exist before `build()` runs,
    /// but the composing widget loses ownership of `self` once it
    /// hands the field to `ctx.add(...)`.
    state_slot: std::rc::Rc<std::cell::RefCell<Option<SharedState>>>,
    /// Natural intrinsic width in logical pixels, cached at the end
    /// of `build()`. When an [`InputMask`] is set, this measures the
    /// mask's empty template (e.g. `__/__/____`) in the theme body
    /// font and adds a small caret slack — so a date / time / phone
    /// field reports a width that matches its content envelope
    /// instead of the generic 200 dp fallback. Composing widgets
    /// like `DateEdit` rely on this so their unconstrained natural
    /// width tracks the format pattern.
    natural_width: f32,
}

impl std::fmt::Debug for TextInputField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputField")
            .field("placeholder", &self.placeholder)
            .field("enabled", &self.enabled)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl TextInputField {
    /// Construct a new field bound to `text`.
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            enabled: true,
            read_only: false,
            max_length: None,
            placeholder: String::new(),
            on_submit: None,
            on_blur: None,
            char_filter: None,
            suffix: Prop::Static(String::new()),
            text_height: None,
            external_interaction: None,
            mask: None,
            mask_placeholder_override: None,
            validator: None,
            feedback: Signal::new(ValidationFeedback::Pristine),
            state: None,
            interaction: Signal::new(InteractionState::Idle),
            caret_position: Signal::new(0),
            state_slot: std::rc::Rc::new(std::cell::RefCell::new(None)),
            natural_width: 200.0,
        }
    }

    /// Declarative placeholder string. The field itself paints
    /// nothing for placeholder — that visual is the composite
    /// parent's responsibility (`TextInput` overlays a
    /// `TextWidget`). The string is still stored here and published
    /// via AccessKit's `placeholder` property so screen readers
    /// announce it.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Disable input and AccessKit interaction.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Mark the field read-only. Caret and selection still work;
    /// inserts, deletes, paste, undo/redo, and cut are all no-ops.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Hard cap on document length in `char`s (grapheme count is
    /// approximated — each `char` counts as one unit, matching
    /// `String::chars().count()`).
    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    /// Closure fired on `Enter`. Unlike `on_blur_fn`, this does
    /// not move focus — the field stays focused and the caret
    /// stays where it was.
    pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_submit = Some(Box::new(f));
        self
    }

    /// Closure fired once per focus-loss, after selection/scroll
    /// have been reset. SpinBox-style callers parse and reformat
    /// here; validators revalidate here.
    pub fn on_blur_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_blur = Some(Box::new(f));
        self
    }

    /// Per-character input-filter predicate. Applied uniformly to
    /// keyboard input, IME commits, and clipboard paste so a filtered
    /// field cannot receive disallowed characters through any path.
    /// Composes with `max_length` and the built-in control/newline
    /// strip (filter runs after the strip). Whole-string validity
    /// (e.g. "at most one decimal point") is a commit-time concern
    /// for `on_blur` / `on_submit`.
    pub fn char_filter(mut self, f: impl Fn(char) -> bool + 'static) -> Self {
        self.char_filter = Some(Rc::new(f));
        self
    }

    /// Static non-editable trailing string rendered flush-right
    /// inside the field's bounds (Qt's `QSpinBox::suffix`). The
    /// caret cannot enter the suffix; clicks past the text end
    /// position the caret at the last editable character.
    ///
    /// For a suffix that changes at runtime (e.g. toggled on/off
    /// by surrounding widget state), use
    /// [`bind_suffix`](Self::bind_suffix) with a `Signal<String>`.
    pub fn suffix(mut self, text: impl Into<String>) -> Self {
        self.suffix = Prop::Static(text.into());
        self
    }

    /// Bind the non-editable trailing string to a reactive
    /// `Signal<String>`. The field re-measures the suffix glyphs
    /// and relayouts the editable text viewport each time the
    /// signal fires, so the transition is seamless.
    ///
    /// Typical use: a `SpinBox` with `special_value_text` binds
    /// an empty string to the suffix whenever the value equals
    /// `min`, and the configured unit string otherwise.
    pub fn bind_suffix(mut self, signal: Signal<String>) -> Self {
        self.suffix = Prop::Bound(signal);
        self
    }

    /// Override the intrinsic text-area height. The field is a
    /// pure leaf with no theme lookup of its own; by default it
    /// reports [`DEFAULT_TEXT_HEIGHT`]. A wrapping composite like
    /// `TextInput` passes its theme's `text_field.height` minus
    /// border + padding here so the visuals line up with the
    /// rest of the form.
    pub fn text_height(mut self, height: f32) -> Self {
        self.text_height = Some(height);
        self
    }

    /// Bind an externally-owned `InteractionState` signal. The
    /// field writes `Focused` on focus gain and `Idle` on loss;
    /// other states (`Hovered`, `Pressed`, `Disabled`) are the
    /// composite's responsibility. When unset, the field owns a
    /// private signal that observers can still read via
    /// [`interaction`](TextInputField::interaction), but composites
    /// that drive a focus ring or border color usually want to
    /// push their own.
    pub fn interaction_signal(mut self, signal: Signal<InteractionState>) -> Self {
        self.external_interaction = Some(signal);
        self
    }

    /// Set an input mask (Qt grammar). Constrains accepted characters
    /// per position, auto-derives the empty-state template
    /// (`__/__/____` for `99/99/9999`), and routes typed chars
    /// through the mask's class filter.
    ///
    /// Composes with [`char_filter`](Self::char_filter): a char must
    /// pass *both* the mask's per-position class AND the user's
    /// `char_filter` to be accepted.
    ///
    /// On parse error (only the trailing-backslash case in practice),
    /// the mask is silently dropped — the field falls back to its
    /// no-mask behaviour rather than panicking.
    pub fn input_mask(mut self, mask: impl AsRef<str>) -> Self {
        match InputMask::parse(mask.as_ref()) {
            Ok(m) => self.mask = Some(m),
            Err(_) => self.mask = None,
        }
        self
    }

    /// Override the visible character used for unfilled editable mask
    /// positions. Default: the theme's
    /// `text_field.mask_placeholder_char` (typically `_`).
    pub fn mask_placeholder(mut self, c: char) -> Self {
        self.mask_placeholder_override = Some(c);
        self
    }

    /// Install a validator. The closure runs on every commit (Enter,
    /// Tab-out, focus loss) and returns a [`ValidationOutcome`] that
    /// drives [`validation_feedback_signal`](Self::validation_feedback_signal).
    ///
    /// **Does not run per-keystroke** — that's [`char_filter`](Self::char_filter)'s
    /// job. Mixing per-keystroke text rewriting with validation
    /// produces caret-jump bugs and is explicitly out of scope.
    pub fn validator(
        mut self,
        f: impl Fn(&str) -> ValidationOutcome + 'static,
    ) -> Self {
        self.validator = Some(Rc::new(f));
        self
    }

    /// Reactive handle on the published [`ValidationFeedback`] state.
    /// Composites bind to this to render the inline feedback strip
    /// below the field. Always present; reads `Pristine` until the
    /// first commit (or forever if no validator is installed).
    pub fn validation_feedback_signal(&self) -> Signal<ValidationFeedback> {
        self.feedback.clone()
    }

    /// The `Signal<String>` this field is bound to.
    pub fn text(&self) -> Signal<String> {
        self.text.clone()
    }

    /// The interaction signal this field writes on focus changes.
    /// Call before inserting the field into the tree.
    pub fn interaction(&self) -> Signal<InteractionState> {
        self.interaction.clone()
    }

    /// Reactive caret position in the field's text (in `usize` char
    /// offsets). Updates after every keyboard or pointer action that
    /// moves the cursor. Used by composing widgets that need to know
    /// where the caret is — e.g. `DateEdit` reads this to figure out
    /// which date segment Up/Down should step.
    pub fn caret_position(&self) -> Signal<usize> {
        self.caret_position.clone()
    }

    /// Returns a callable that programmatically sets the caret
    /// position (in char offsets) on the field. Capture this on the
    /// builder BEFORE `ctx.add(...)` consumes the field; call it
    /// after a programmatic text rewrite to restore the caret to the
    /// right column instead of leaving it at the document end (the
    /// default behaviour of `cursor.insert_text`).
    ///
    /// The returned closure becomes a no-op until `build()` runs;
    /// after build it walks the field's inner state and moves the
    /// document cursor to `position`, clamped to the document
    /// length. Used by `DateEdit` / `TimeEdit` segment-stepping to
    /// keep the caret within its current segment after Up/Down.
    pub fn caret_setter(&self) -> std::rc::Rc<dyn Fn(usize)> {
        let slot = self.state_slot.clone();
        std::rc::Rc::new(move |position: usize| {
            if let Some(state) = slot.borrow().as_ref() {
                let st = state.borrow();
                st.cursor.set_position(
                    position,
                    fern_text::text_document::MoveMode::MoveAnchor,
                );
                let actual = st.cursor.position();
                if st.cursor_position.get() != actual {
                    st.cursor_position.set(actual);
                }
            }
        })
    }
}

impl Widget for TextInputField {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Resolve the interaction signal (external override wins).
        if let Some(signal) = self.external_interaction.take() {
            self.interaction = signal;
        }

        // Resolve the mask placeholder character. Caller override wins;
        // otherwise pull from the theme.
        let theme_snapshot = ctx.theme_signal().get();
        let mask_placeholder_char = self
            .mask_placeholder_override
            .unwrap_or(theme_snapshot.components.text_field.mask_placeholder_char);

        // Auto-derive placeholder from mask when none was explicitly
        // set: an empty masked field paints `__/__/____` rather than
        // a blank surface, giving the user a self-documenting template.
        if self.placeholder.is_empty() {
            if let Some(ref m) = self.mask {
                self.placeholder = m.empty_template(mask_placeholder_char);
            }
        }

        // Cache mask-aware natural width. When a mask is set, the
        // visual content envelope is exactly the empty template
        // (`__/__/____` for `99/99/9999`); measure it in the theme
        // body font + a small caret slack so the field's intrinsic
        // tracks its real content instead of the generic 200 dp
        // fallback. This is what lets composing widgets like
        // `DateEdit` report a natural width that follows the format
        // pattern instead of the 200 dp default. Without a mask the
        // 200 dp fallback (set in `new()`) stays.
        if let Some(ref m) = self.mask {
            let template = m.empty_template(mask_placeholder_char);
            let style = &theme_snapshot.typography.body;
            let measured = measure_width_px(ctx, &template, style);
            // Slack: half a digit so the caret never sits flush against
            // the right border on the last position.
            let slack = style.size * 0.3;
            self.natural_width = measured + slack;
        }

        // Compose the user's char_filter with the mask's class filter.
        // The mask doesn't know the cursor position here (this is a
        // pre-position filter), so it accepts any char that fits *any*
        // editable position class — a permissive gate that catches
        // gross mismatches (typing "a" into a digits-only mask) without
        // requiring per-keystroke position tracking. Per-position
        // gating happens at commit time via the validator.
        if let Some(ref mask) = self.mask {
            let mask_for_filter = mask.clone();
            let user_filter = self.char_filter.take();
            let combined: CharFilter = Rc::new(move |c: char| {
                // Always allow fixed-separator characters (they're
                // legitimate input even if user types them — the
                // formatter consumes them).
                let in_mask_class = mask_for_filter.positions().any(|p| match p {
                    MaskPosition::Editable { class, .. } => class.accepts(c),
                    MaskPosition::Fixed(sep) => *sep == c,
                });
                if !in_mask_class {
                    return false;
                }
                match user_filter.as_ref() {
                    Some(f) => f(c),
                    None => true,
                }
            });
            self.char_filter = Some(combined);
        }

        // Build the shared state from the configured builder values.
        let mut on_submit = self.on_submit.take().map(Rc::new);
        let mut on_blur = self.on_blur.take().map(Rc::new);

        // Wrap commit callbacks with the validator pipeline. The
        // wrapping closure: snapshots the bound text, runs the
        // validator, applies the outcome (writes feedback, mutates
        // text on `Corrected`), then chains the user's callback so
        // composites can react to the now-updated state.
        if let Some(validator) = self.validator.clone() {
            let bound_text = self.text.clone();
            let feedback = self.feedback.clone();
            let prev_on_blur = on_blur.take();
            on_blur = Some(Rc::new(Box::new({
                let validator = validator.clone();
                let feedback = feedback.clone();
                let bound_text = bound_text.clone();
                move |evt_ctx: &mut EventContext| {
                    run_validator_and_apply(&validator, &bound_text, &feedback);
                    if let Some(cb) = prev_on_blur.as_ref() {
                        cb(evt_ctx);
                    }
                }
            }) as CommandFactory));
            let prev_on_submit = on_submit.take();
            on_submit = Some(Rc::new(Box::new({
                let validator = validator.clone();
                let feedback = feedback.clone();
                let bound_text = bound_text.clone();
                move |evt_ctx: &mut EventContext| {
                    run_validator_and_apply(&validator, &bound_text, &feedback);
                    if let Some(cb) = prev_on_submit.as_ref() {
                        cb(evt_ctx);
                    }
                }
            }) as CommandFactory));
        }

        let initial_text = self.text.get();
        let read_only_effective = self.read_only || !self.enabled;

        let initial_suffix = self.suffix.get();
        let shared_state = TextInputState::new(TextInputConfig {
            initial_text,
            max_length: self.max_length,
            read_only: read_only_effective,
            on_submit,
            on_blur,
            char_filter: self.char_filter.take(),
            placeholder: self.placeholder.clone(),
            suffix: initial_suffix,
        });
        self.state = Some(shared_state.clone());
        // Late-populate the slot so `caret_setter()` closures captured
        // before build can now reach the inner state. Idempotent on
        // rebuild — overwrites the slot with the freshly created
        // SharedState.
        *self.state_slot.borrow_mut() = Some(shared_state.clone());

        // Reset feedback to Pristine whenever the user types — prior
        // Invalid / Corrected announcements should clear as soon as
        // the user starts editing again so they don't shout stale
        // errors at someone trying to fix them.
        {
            let feedback = self.feedback.clone();
            ctx.effect(&self.text, move |_| {
                if !matches!(feedback.get(), ValidationFeedback::Pristine) {
                    feedback.set(ValidationFeedback::Pristine);
                }
            });
        }

        // Mirror the inner state's `cursor_position` onto the field's
        // public `caret_position` so callers of `caret_position()` see
        // live caret updates. The state's signal is keyed by the
        // shared state's identity (created in `TextInputState::new`),
        // not by the field's; this effect bridges the two.
        {
            let inner = shared_state.borrow().cursor_position.clone();
            let outer = self.caret_position.clone();
            outer.set(inner.get());
            ctx.effect(&inner, move |pos| {
                if outer.get() != *pos {
                    outer.set(*pos);
                }
            });
        }

        // Bind feedback at AccessibilityOnly so the field's AT node
        // refreshes its `set_invalid` state when feedback changes.
        {
            let self_id = ctx.self_id();
            self.feedback.bind_to(
                self_id,
                ctx.binding_registry(),
                fern_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        let text_signal = shared_state.borrow().text_signal.clone();

        // Sync external text signal → internal state. A programmatic
        // update on the bound signal rewrites the document; the
        // caret ends up at the end of the inserted text (cursor
        // behavior is documented in
        // `text_document::TextCursor::insert_text`).
        {
            let ext = self.text.clone();
            let state_for_sync = shared_state.clone();
            ctx.effect(&ext, move |new_text| {
                let st = state_for_sync.borrow();
                let current = st.document.to_plain_text().unwrap_or_default();
                if current != *new_text {
                    st.cursor.select(SelectionType::Document);
                    let _ = st.cursor.insert_text(new_text);
                }
            });
        }

        // Sync internal text signal → external. Every edit that
        // reaches `text_signal` also updates the caller-owned
        // signal, so observers bound to it see every keystroke
        // (after the debounce in `tick`).
        {
            let ext = self.text.clone();
            ctx.effect(&text_signal, move |new_text| {
                if ext.get() != *new_text {
                    ext.set(new_text.clone());
                }
            });
        }

        // Swap the private engine for one sharing the app's
        // `SharedTypesetter` so glyphs land in the atlas
        // fern-render uploads to the GPU. When no typesetter is
        // installed (headless tests), the pre-built private
        // engine stays in place.
        if let Some(shared) = ctx.app_state::<SharedTypesetter>() {
            let mut st = self.state().borrow_mut();
            let mut engine = RichTextEngine::from_shared(shared.clone());
            engine.set_wrap_mode(fern_text::WrapMode::None);
            st.engine = engine;
            st.needs_full_layout = true;
        }

        // Apply theme colors to the (possibly freshly swapped-in) engine.
        // Setting them before the swap would be lost. The rich-text
        // engine stores colors in GPU-ready form, so we register an
        // effect on the theme signal that re-applies the palette on
        // every theme switch instead of capturing a single snapshot.
        let theme_signal = ctx.theme_signal();
        {
            let theme = theme_signal.get();
            let colors = &theme.colors;
            let mut st = self.state().borrow_mut();
            st.engine.set_text_color(colors.text_primary.to_array());
            st.engine.set_cursor_color(colors.text_primary.to_array());
            st.engine.set_selection_color(colors.selection_bg_active.to_array());
        }
        {
            let state = self.state().clone();
            ctx.effect(&theme_signal, move |theme| {
                let colors = &theme.colors;
                let mut st = state.borrow_mut();
                st.engine.set_text_color(colors.text_primary.to_array());
                st.engine.set_cursor_color(colors.text_primary.to_array());
                st.engine
                    .set_selection_color(colors.selection_bg_active.to_array());
                if let Some(ref mut suffix_engine) = st.suffix_engine {
                    let secondary = colors.text_secondary.to_array();
                    suffix_engine.set_text_color(secondary);
                    suffix_engine.set_cursor_color(secondary);
                }
            });
        }

        // Suffix engine: second independent `RichTextEngine` used
        // to paint the non-editable trailing string (Qt's
        // `QSpinBox` `suffix`). Shares the app's typesetter when
        // available so glyphs land in the same atlas as the main
        // document; falls back to a private engine under headless
        // tests.
        //
        // `suffix_width` is cached on `TextInputState` and drives
        // both the effective text viewport (so the scroll logic
        // keeps the caret visible without sliding text behind the
        // suffix) and the suffix paint origin at the right edge
        // of the field. When the suffix is bound to a signal, a
        // reactive effect below re-lays the engine out each time
        // the signal fires.
        let text_area_height = self.text_height.unwrap_or(DEFAULT_TEXT_HEIGHT).max(1.0);
        let needs_suffix_engine =
            matches!(self.suffix, Prop::Bound(_)) || {
                let st = self.state().borrow();
                !st.suffix.is_empty()
            };
        if needs_suffix_engine {
            let mut suffix_engine = if let Some(shared) = ctx.app_state::<SharedTypesetter>() {
                RichTextEngine::from_shared(shared.clone())
            } else {
                RichTextEngine::private_default()
            };
            suffix_engine.set_wrap_mode(fern_text::WrapMode::None);
            {
                let theme = theme_signal.get();
                let secondary = theme.colors.text_secondary.to_array();
                suffix_engine.set_text_color(secondary);
                suffix_engine.set_cursor_color(secondary);
                suffix_engine.set_selection_color([0.0, 0.0, 0.0, 0.0]);
            }
            suffix_engine.set_viewport(10_000.0, text_area_height);

            {
                let mut st = self.state().borrow_mut();
                st.suffix_engine = Some(suffix_engine);
            }
            // Initial layout from the current suffix value.
            let initial = self.state().borrow().suffix.clone();
            relayout_suffix(self.state(), &initial);
        }

        // Reactive suffix: observe the signal and re-lay out on
        // every change. `Relayout` dirty-tracking ensures the
        // surrounding layout sees the new `suffix_width` and the
        // text viewport narrows/widens accordingly.
        if let Prop::Bound(signal) = &self.suffix {
            let self_id = ctx.self_id();
            signal.bind_to(
                self_id,
                ctx.binding_registry(),
                fern_core::binding::BindingLevel::Relayout,
            );
            let state_for_effect = self.state().clone();
            ctx.effect(signal, move |new_text| {
                relayout_suffix(&state_for_effect, new_text);
            });
        }

        // Bind caret_visible for repaint.
        {
            let st = self.state().borrow();
            let caret_visible = st.caret_visible.clone();
            drop(st);
            let self_id = ctx.self_id();
            caret_visible.bind_to(
                self_id,
                ctx.binding_registry(),
                fern_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // Bind text_signal at RepaintOnly AND AccessibilityOnly.
        //
        // RepaintOnly: when the text changes by any route — local
        // typing, IME, clipboard paste, the ext→internal sync
        // effect firing because a composite parent (SpinBox etc.)
        // drove the bound signal — the field must redraw. During
        // typing the caret-blink signal already keeps the widget
        // repainting, which used to mask a missing repaint trigger
        // on programmatic text changes to an unfocused field. With
        // the explicit bind, no path depends on blink.
        //
        // AccessibilityOnly: screen readers see edits as soon as
        // the text signal updates, independent of whether a paint
        // happens this frame.
        {
            let st = self.state().borrow();
            let text_signal = st.text_signal.clone();
            drop(st);
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            text_signal.bind_to(
                self_id,
                registry,
                fern_core::binding::BindingLevel::RepaintOnly,
            );
            text_signal.bind_to(
                self_id,
                registry,
                fern_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // Stash frame infrastructure handles and self_id.
        {
            let mut st = self.state().borrow_mut();
            st.frame_request = Some(ctx.frame_request_handle());
            st.frame_wake_at = Some(ctx.wake_at_handle());
            st.field_widget_id = Some(ctx.self_id());
        }

        ctx.request_frame();

        // Frame-tick effect: flushes pending chars, drains document
        // events, drives the caret blink, and debounces undo/redo
        // state changes.
        //
        // IMPORTANT: the mutable borrow must be dropped BEFORE
        // setting `text_signal`. Setting it fires observers
        // synchronously, which chain into the ext→internal sync
        // effect that borrows the same state. Holding `borrow_mut`
        // across `signal.set()` would panic.
        {
            let state = self.state().clone();
            let tick_signal = ctx.frame_tick();
            ctx.effect(&tick_signal, move |delta| {
                let (more, pending_text) = {
                    let mut st = state.borrow_mut();
                    let more = tick(&mut st, *delta);
                    st.has_selection.set(st.cursor.has_selection());
                    let pending = st.deferred_text_update.take();
                    (more, pending)
                };
                if let Some(text) = pending_text {
                    let st = state.borrow();
                    if st.text_signal.get() != text {
                        st.text_signal.set(text);
                    }
                }
                if more {
                    let st = state.borrow();
                    if let Some(handle) = &st.frame_request {
                        handle.set(true);
                    }
                }
            });
        }

        // Pre-build the right-click context menu as a dormant widget.
        let context_menu_id = build_context_menu(ctx, self.state());
        self.state().borrow_mut().context_menu_id = Some(context_menu_id);

        // Apply initial disabled state once the state is ready.
        if !self.enabled {
            self.interaction.set(InteractionState::Disabled);
        }

        // Attach handlers. Focus-origin inference mirrors the
        // `Slider` pattern: hover cached, focus event checks hover
        // to distinguish keyboard vs pointer origin for the
        // select-all-on-keyboard-focus rule.
        let hovered = std::rc::Rc::new(std::cell::Cell::new(false));
        let hovered_for_focus = hovered.clone();
        let hovered_for_hover = hovered.clone();
        let enabled = self.enabled;

        let state_for_focus = self.state().clone();
        let interaction_for_focus = self.interaction.clone();
        let state_for_pointer = self.state().clone();
        let state_for_key = self.state().clone();
        let state_for_double = self.state().clone();
        let state_for_triple = self.state().clone();
        let state_for_access = self.state().clone();

        let handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(CursorIcon::Text)
            .on_hover(move |entered, _ctx| {
                hovered_for_hover.set(entered);
            })
            .on_focus(move |gained, ctx| {
                interaction_for_focus.set(if gained {
                    InteractionState::Focused
                } else {
                    InteractionState::Idle
                });

                let mut st = state_for_focus.borrow_mut();
                st.has_focus = gained;
                let mut blur_callback: Option<Rc<CommandFactory>> = None;
                if gained {
                    st.blink_last_toggle = Some(std::time::Instant::now());
                    st.caret_visible.set(true);
                    let is_keyboard = !hovered_for_focus.get();
                    drop(st);
                    if is_keyboard {
                        let st = state_for_focus.borrow();
                        st.cursor.select(SelectionType::Document);
                        drop(st);
                        sync_cursor_signals(&state_for_focus);
                    }
                } else {
                    st.cursor.clear_selection();
                    st.scroll_x = 0.0;
                    st.caret_visible.set(false);
                    st.drag_state = state::DragState::Idle;
                    blur_callback = st.on_blur.clone();
                    drop(st);
                    sync_cursor_signals(&state_for_focus);
                }
                if let Some(cb) = blur_callback {
                    cb(ctx);
                }
                ctx.request_frame();
            })
            .on_pointer_event(move |event, ctx| {
                mouse::handle_pointer_event(&state_for_pointer, event, ctx)
            })
            .on_key(move |event, ctx| keyboard::handle_key(&state_for_key, event, ctx))
            .on_double_tap(move |pos, ctx| {
                mouse::handle_double_tap(&state_for_double, pos, ctx)
            })
            .on_triple_tap(move |pos, ctx| {
                mouse::handle_triple_tap(&state_for_triple, pos, ctx)
            })
            .on_access_action_request(move |action, _target_node, data, ctx| {
                handle_access_action(&state_for_access, action, data, ctx)
            });

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        // Default unwrap is the cached natural width (mask-aware when
        // a mask is set; 200 dp fallback otherwise). Composing widgets
        // that wrap us in a constraint pass `Some(width)` and we use
        // that; the natural width is what surfaces in unconstrained
        // intrinsic queries (ZStack measurement with `unspecified()`,
        // etc.) so the chain reports a sensible content size.
        let w = proposal.width.unwrap_or(self.natural_width).max(0.0);
        let h = self.text_height.unwrap_or(DEFAULT_TEXT_HEIGHT).max(0.0);
        Size::new(w, h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(state) = self.state.as_ref() {
            state.borrow_mut().viewport_width = bounds.width;
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let Some(state) = self.state.as_ref() else { return };
        let mut st = state.borrow_mut();

        st.viewport_origin = Point::new(bounds.x, bounds.y);
        let viewport_changed = (st.viewport_width - bounds.width).abs() > 0.5;
        if viewport_changed {
            st.viewport_width = bounds.width;
            st.needs_full_layout = true;
        }

        let suffix_width = st.suffix_width;
        let text_viewport_width = (bounds.width - suffix_width).max(0.0);

        st.engine.set_viewport(10_000.0, bounds.height);

        if st.needs_full_layout || !st.engine.has_full_layout() {
            let flow = st.document.snapshot_flow();
            st.engine.layout_full(&flow);
            st.needs_full_layout = false;
            st.content_dirty = true;
        }

        let caret_on = st.caret_visible.get() && st.has_focus;
        let cursor_display = CursorDisplay {
            position: st.cursor.position(),
            anchor: st.cursor.anchor(),
            visible: caret_on,
            selected_cells: Vec::new(),
        };
        st.engine.set_cursor(&cursor_display);

        ensure_caret_visible_h(&mut st, text_viewport_width);

        let scroll_x = st.scroll_x;

        let text_clip = Rect::new(bounds.x, bounds.y, text_viewport_width, bounds.height);
        canvas.set_clip(text_clip);

        {
            let state_ref: &mut TextInputState = &mut *st;
            let TextInputState {
                ref mut engine,
                ref document,
                ref mut image_cache,
                ..
            } = *state_ref;

            engine.with_render_frame(|frame| {
                paint_frame(
                    canvas,
                    PaintParams {
                        frame,
                        origin: Point::new(bounds.x - scroll_x, bounds.y),
                        document,
                        image_cache,
                        draw_caret: caret_on,
                    },
                );
            });
        }

        canvas.clear_clip();

        if suffix_width > 0.0
            && let Some(suffix_engine) = st.suffix_engine.as_mut()
        {
            let suffix_clip = Rect::new(
                bounds.x + text_viewport_width,
                bounds.y,
                suffix_width,
                bounds.height,
            );
            canvas.set_clip(suffix_clip);
            let suffix_origin = Point::new(bounds.x + text_viewport_width, bounds.y);
            suffix_engine.with_render_frame(|frame| {
                paint_suffix_glyphs(canvas, frame, suffix_origin);
            });
            canvas.clear_clip();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use fern_core::accesskit::{Action, Role};

        let Some(state) = self.state.as_ref() else { return };
        let st = state.borrow();

        builder.set_role(Role::TextInput);

        let text = st.document.to_plain_text().unwrap_or_default();
        if !text.is_empty() {
            builder.set_value(&text);
        }

        if !st.placeholder.is_empty() {
            builder.set_placeholder(st.placeholder.clone());
        }

        if st.read_only {
            builder.set_read_only();
        }

        let pos = st.cursor.position();
        let anchor = st.cursor.anchor();
        builder.set_text_selection_on_self(anchor, pos);

        let char_lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
        builder.inner_mut().set_character_lengths(char_lengths);

        let word_starts = compute_word_starts(&text);
        if !word_starts.is_empty() {
            builder.inner_mut().set_word_starts(word_starts);
        }

        builder.add_action(Action::Focus);
        if !st.read_only {
            builder.add_action(Action::SetValue);
            builder.add_action(Action::ReplaceSelectedText);
        }
        builder.add_action(Action::SetTextSelection);

        // Validation feedback → accesskit `aria-invalid`. Surface
        // `Invalid` as `Invalid::True`; `Corrected` doesn't carry an
        // invalid marker (the data is now valid) but the composite's
        // Live region announces the correction. The framework's
        // AccessNodeBuilder doesn't yet wrap `set_invalid`, so reach
        // through `inner_mut()` which is the documented escape hatch.
        if self.feedback.get().is_invalid() {
            builder
                .inner_mut()
                .set_invalid(fern_core::accesskit::Invalid::True);
        }
    }
}

impl TextInputField {
    /// Borrow the shared state. Panics if called before `build()`
    /// has run — the state is allocated in `build()` from the
    /// builder config.
    fn state(&self) -> &SharedState {
        self.state
            .as_ref()
            .expect("TextInputField::state called before build")
    }
}

/// Adjust `scroll_x` so the caret stays within the visible viewport.
///
/// `text_viewport_width` is the portion of the viewport reserved for
/// editable text, i.e. `viewport_width - suffix_width`. Callers pass
/// the reduced width explicitly so the scroll never slides text
/// behind the non-editable suffix.
fn ensure_caret_visible_h(st: &mut TextInputState, text_viewport_width: f32) {
    if !st.engine.has_full_layout() || text_viewport_width <= 0.0 {
        return;
    }
    let pos = st.cursor.position();
    let caret = st.engine.caret_rect(pos);
    let caret_x = caret[0];
    let caret_w = caret[2].max(1.0);
    let vw = text_viewport_width;

    if caret_x - st.scroll_x < SCROLL_MARGIN {
        st.scroll_x = (caret_x - SCROLL_MARGIN).max(0.0);
    } else if caret_x + caret_w - st.scroll_x > vw - SCROLL_MARGIN {
        st.scroll_x = caret_x + caret_w - vw + SCROLL_MARGIN;
    }
}

/// Update the cached suffix text and re-run layout on the suffix
/// engine. Called from `build()` for the initial value and from
/// the reactive effect when the bound suffix signal fires.
fn relayout_suffix(state: &SharedState, new_text: &str) {
    let mut st = state.borrow_mut();
    st.suffix = new_text.to_string();
    if new_text.is_empty() {
        st.suffix_width = 0.0;
        // Leave the engine in place (cheap to reuse) but don't
        // lay out — paint skips the suffix when width is zero.
        return;
    }
    let Some(engine) = st.suffix_engine.as_mut() else {
        // No engine allocated (pure-static path that started
        // empty and never became non-empty). Allocate lazily so
        // late signal flips still render.
        return;
    };
    let doc = TextDocument::new();
    let _ = doc.set_plain_text(new_text);
    let flow = doc.snapshot_flow();
    engine.layout_full(&flow);
    st.suffix_width = engine.max_content_width();
}

/// Paint glyphs from a pre-laid-out suffix `RenderFrame` at a fixed
/// origin. Decorations, selection rectangles, and caret are ignored —
/// the suffix is plain non-editable text, so only the glyph pass is
/// needed. Kept inline (rather than reusing `paint_frame`) to avoid
/// the `TextDocument` / `ImageCache` parameters `paint_frame`
/// requires for inline images the suffix never contains.
fn paint_suffix_glyphs(
    canvas: &mut Canvas,
    frame: &fern_text::RenderFrame,
    origin: Point,
) {
    use fern_canvas::GlyphQuad as CanvasGlyphQuad;
    for g in frame.glyphs.iter() {
        let quad = CanvasGlyphQuad {
            screen: [g.screen[0] + origin.x, g.screen[1] + origin.y, g.screen[2], g.screen[3]],
            atlas: g.atlas,
            color: g.color,
            is_color: g.is_color,
        };
        canvas.draw_glyph_quad(quad);
    }
}

/// Simplified frame-loop tick for single-line text input.
fn tick(state: &mut TextInputState, delta: f32) -> bool {
    if !state.pending_chars.is_empty() {
        let batch = std::mem::take(&mut state.pending_chars);
        let _ = state.cursor.insert_text(&batch);
        state.pending_text_changed = true;
    }

    let had_events = state.drain_events();

    let blinking_active = state.has_focus;
    if blinking_active {
        let now = std::time::Instant::now();
        let interval = std::time::Duration::from_secs_f32(CARET_BLINK_INTERVAL);
        match state.blink_last_toggle {
            None => {
                state.blink_last_toggle = Some(now);
            }
            Some(last) if now.saturating_duration_since(last) >= interval => {
                state.blink_last_toggle = Some(now);
                let was = state.caret_visible.get();
                state.caret_visible.set(!was);
            }
            _ => {}
        }
        if let (Some(last), Some(wake)) = (state.blink_last_toggle, &state.frame_wake_at) {
            let next = last + interval;
            let merged = match wake.get() {
                Some(existing) if existing <= next => existing,
                _ => next,
            };
            wake.set(Some(merged));
        }
    } else {
        state.blink_last_toggle = None;
        if state.caret_visible.get() {
            state.caret_visible.set(false);
        }
    }

    if state.needs_full_layout && state.viewport_width > 0.0 {
        let flow = state.document.snapshot_flow();
        state.engine.layout_full(&flow);
        state.needs_full_layout = false;
        state.content_dirty = true;
    }

    if state.pending_text_changed {
        let new_text = state.document.to_plain_text().unwrap_or_default();
        if state.text_signal.get() != new_text {
            state.deferred_text_update = Some(new_text);
        }
    }

    state.debounce_timer += delta;
    let debounce_ready = state.debounce_timer >= DEBOUNCE_WINDOW_SECS;
    if debounce_ready {
        if state.pending_text_changed {
            state.pending_text_changed = false;
        }
        if let Some((cu, cr)) = state.pending_undo_redo.take() {
            if state.can_undo.get() != cu {
                state.can_undo.set(cu);
            }
            if state.can_redo.get() != cr {
                state.can_redo.set(cr);
            }
        }
        state.debounce_timer = 0.0;
    }
    let debounce_work = state.pending_text_changed || state.pending_undo_redo.is_some();

    had_events || debounce_work
}

/// Handle AccessKit actions (SetValue, SetTextSelection, Focus).
fn handle_access_action(
    state: &SharedState,
    action: fern_core::accesskit::Action,
    data: Option<fern_core::accesskit::ActionData>,
    ctx: &mut EventContext,
) -> EventResponse {
    use fern_core::accesskit::{Action, ActionData};

    match (action, data) {
        (Action::SetTextSelection, Some(ActionData::SetTextSelection(sel))) => {
            let st = state.borrow();
            st.cursor.set_position(sel.anchor.character_index, fern_text::text_document::MoveMode::MoveAnchor);
            st.cursor.set_position(sel.focus.character_index, fern_text::text_document::MoveMode::KeepAnchor);
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        (Action::SetValue, Some(ActionData::Value(value))) => {
            let st = state.borrow();
            st.cursor.select(SelectionType::Document);
            let _ = st.cursor.insert_text(value.as_ref());
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        (Action::Focus, _) => {
            if let Some(id) = state.borrow().field_widget_id {
                ctx.request_focus(id);
            }
            EventResponse::Handled
        }
        _ => EventResponse::Ignored,
    }
}

/// Compute word-start character indices for AccessKit.
fn compute_word_starts(text: &str) -> Vec<u8> {
    let mut starts = Vec::new();
    let mut in_word = false;
    for (char_index, ch) in text.chars().enumerate() {
        let is_word_char = ch.is_alphanumeric() || ch == '_';
        if is_word_char && !in_word
            && let Ok(idx) = u8::try_from(char_index)
        {
            starts.push(idx);
        }
        in_word = is_word_char;
    }
    starts
}

/// Pre-build the right-click context menu as a dormant widget.
fn build_context_menu(ctx: &mut BuildContext, state: &SharedState) -> WidgetId {
    let state_cut = state.clone();
    let state_copy = state.clone();
    let state_paste = state.clone();
    let state_select_all = state.clone();

    let menu = MenuList::new()
        .item(
            MenuItem::new_literal("Cut")
                .shortcut_label("Ctrl+X")
                .on_activate_fn(move |ctx| {
                    {
                        let mut st = state_cut.borrow_mut();
                        keyboard::clipboard_cut(&mut st, ctx);
                    }
                    sync_cursor_signals(&state_cut);
                    ctx.request_frame();
                    ctx.dismiss_all_overlays();
                }),
        )
        .item(
            MenuItem::new_literal("Copy")
                .shortcut_label("Ctrl+C")
                .on_activate_fn(move |ctx| {
                    {
                        let mut st = state_copy.borrow_mut();
                        keyboard::clipboard_copy(&mut st, ctx);
                    }
                    ctx.dismiss_all_overlays();
                }),
        )
        .item(
            MenuItem::new_literal("Paste")
                .shortcut_label("Ctrl+V")
                .on_activate_fn(move |ctx| {
                    {
                        let mut st = state_paste.borrow_mut();
                        keyboard::clipboard_paste(&mut st, ctx);
                    }
                    sync_cursor_signals(&state_paste);
                    ctx.request_frame();
                    ctx.dismiss_all_overlays();
                }),
        )
        .item(MenuSeparator)
        .item(
            MenuItem::new_literal("Select All")
                .shortcut_label("Ctrl+A")
                .on_activate_fn(move |ctx| {
                    {
                        let st = state_select_all.borrow();
                        st.cursor.select(SelectionType::Document);
                    }
                    sync_cursor_signals(&state_select_all);
                    ctx.request_frame();
                    ctx.dismiss_all_overlays();
                }),
        );

    let menu_id = ctx.add(menu);
    ctx.set_dormant(menu_id);
    menu_id
}

/// Run the validator on the bound text and update the feedback signal.
///
/// On `Corrected`, also writes the corrected text back to the bound
/// signal — the field's external→internal sync effect picks this up
/// and rewrites the document in the next frame. On `Invalid`, the
/// text is left as-typed; composites that want a "revert on invalid"
/// behaviour observe the feedback signal and rewrite the text from
/// their own source of truth (e.g., `DateEdit` reformats from its
/// `Signal<Option<Date>>`).
fn run_validator_and_apply(
    validator: &ValidatorFn,
    bound_text: &Signal<String>,
    feedback: &Signal<ValidationFeedback>,
) {
    let raw = bound_text.get();
    match validator(&raw) {
        ValidationOutcome::Valid => {
            feedback.set(ValidationFeedback::Valid);
        }
        ValidationOutcome::Corrected { corrected, message } => {
            // Write the corrected text first so observers of the
            // bound signal see the new value before the feedback
            // signal flips. Composites that bind to BOTH signals
            // (rare) will see a consistent pair: text + correction
            // notice describing the change.
            if bound_text.get() != corrected {
                bound_text.set(corrected);
            }
            feedback.set(ValidationFeedback::Corrected {
                message,
                since: std::time::Instant::now(),
            });
        }
        ValidationOutcome::Invalid { message } => {
            feedback.set(ValidationFeedback::Invalid { message });
        }
    }
}

/// Measure the advance width of `text` in logical pixels using the
/// app-wide `SharedTypesetter` (the same backend the field paints
/// with). Falls back to a rough heuristic when no typesetter is
/// installed (headless tests) so the caller still gets a non-zero
/// width and any cap / floor logic behaves reasonably. Mirrors the
/// helper SpinBox uses for `width_chars` measurement.
fn measure_width_px(ctx: &mut BuildContext, text: &str, style: &TextStyle) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    if let Some(ts) = ctx.app_state::<SharedTypesetter>() {
        let backend = ts.as_text_backend();
        let layout = backend.borrow_mut().layout_single_line(text, style, None);
        return layout.width;
    }
    text.chars().count() as f32 * style.size * 0.55
}
