// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
mod widget_impl;

use std::rc::Rc;
use teksilo_i18n::tr_widget;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{RetainedText, TextRunSource, push_text_runs};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key};
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_text::text_document::{SelectionType, TextDocument};
use teksilo_text::{CursorAffinity, CursorDisplay, RichTextEngine, SharedTypesetter};
use teksilo_tokens::TextStyle;

use crate::button::InteractionState;
use crate::keystroke_format::format_keystroke;
use crate::menu_item::MenuItem;
use crate::menu_list::{MenuList, MenuSeparator};
use crate::rich_text::paint::{PaintParams, paint_frame};

pub(crate) use self::state::{CharFilter, CommandFactory};
use self::state::{SharedState, TextInputConfig, TextInputState, sync_cursor_signals};

pub use self::mask::{InputMask, MaskClass, MaskError, MaskPosition};
pub use self::validator::{ValidationFeedback, ValidationOutcome, ValidatorFn};

// The caret blink period and the debounce window are shared with every other
// text surface — see `common::editor_runtime`. They used to be re-declared
// here as private constants ("same as RichTextEditor", said the comment),
// which is exactly the kind of duplication that drifts silently: two carets
// blinking at different rates is invisible to tests and obvious to users.
use crate::common::editor_runtime::CaretPolicy;

/// Horizontal scroll margin in pixels. The caret stays at least this
/// far from the left/right edge of the viewport.
const SCROLL_MARGIN: f32 = 4.0;

/// Default text-area height when the caller does not override it
/// via [`TextInputField::text_height`]. Picked to match the Int UI
/// `text_field.height` token minus 2×border — the value the
/// `TextInput` composite reports — so a bare `TextInputField`
/// added to a tree without its composite still looks right.
const DEFAULT_TEXT_HEIGHT: f32 = 20.0;

/// The semantic purpose of a text field, surfaced to assistive technology as
/// a specialised AccessKit role (WCAG 1.3.5 Identify Input Purpose / EN 301 549).
///
/// This is the in-framework-achievable part of SC 1.3.5: a screen reader
/// announces "email, edit text" instead of a generic "edit text". The FULL
/// HTML `autocomplete`-token vocabulary (`given-name`, `postal-code`,
/// `cc-number`, …) that drives OS/browser autofill has **no representation in
/// AccessKit 0.24** and therefore cannot be exposed from Teksilo — see
/// `docs/a11y/a11y_issues.md`. Password entry is configured via
/// [`TextInputField::secure`], not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputPurpose {
    /// Ordinary free text (`Role::TextInput`).
    #[default]
    Normal,
    /// Email address (`Role::EmailInput`).
    Email,
    /// Telephone number (`Role::PhoneNumberInput`).
    Phone,
    /// URL (`Role::UrlInput`).
    Url,
    /// Numeric entry — e.g. a quantity or code (`Role::NumberInput`).
    Number,
    /// Search query (`Role::SearchInput`).
    Search,
}

impl InputPurpose {
    /// The AccessKit role for a non-secure field with this purpose.
    pub(crate) fn to_role(self) -> teksilo_core::accesskit::Role {
        use teksilo_core::accesskit::Role;
        match self {
            InputPurpose::Normal => Role::TextInput,
            InputPurpose::Email => Role::EmailInput,
            InputPurpose::Phone => Role::PhoneNumberInput,
            InputPurpose::Url => Role::UrlInput,
            InputPurpose::Number => Role::NumberInput,
            InputPurpose::Search => Role::SearchInput,
        }
    }
}

/// How a secure ([`TextInputField::secure`]) field echoes typed
/// characters. Mirrors Qt's `QLineEdit::EchoMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EchoMode {
    /// Replace every character with the echo glyph (default `'•'`).
    /// The plaintext stays in the bound `Signal<String>` but never
    /// reaches the text engine while masked.
    #[default]
    Masked,
    /// Show nothing at all — not even the length. The caret stays at
    /// the start. Qt's `NoEcho`.
    NoEcho,
    /// Show plaintext while the field is focused (being edited) and
    /// re-mask on blur. Qt's `PasswordEchoOnEdit`.
    RevealWhileTyping,
}

/// How a *revealed* secure field reports to assistive technology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AtRevealPolicy {
    /// When revealed, expose the field as a normal `Role::TextInput`
    /// carrying the plaintext value — matching what is visibly on
    /// screen and the web `type=password ↔ type=text` swap. When
    /// masked, it reverts to `Role::PasswordInput`. (Default.)
    #[default]
    SwapRole,
    /// Always report `Role::PasswordInput` and never expose plaintext
    /// to assistive tech, even while visually revealed. Higher
    /// confidentiality at the cost of consistency with the screen.
    AlwaysProtected,
}

/// Editable single-line text surface primitive.
///
/// See the [module docs](self) for the full feature list and a
/// compositional example.
pub struct TextInputField {
    // ── Configuration (builder methods, consumed in build) ───────────
    text: Signal<String>,
    /// Enabled state, static or reactive; forwarded to the arena at build
    /// time.
    enabled: Prop<bool>,
    read_only: bool,
    max_length: Option<usize>,
    placeholder: String,
    on_submit: Option<CommandFactory>,
    on_access_set_value: Option<super::text_input_field::state::AccessSetValue>,
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

    // ── Secure / password masking (set via `secure`) ────────────────
    secure: bool,
    echo_mode: EchoMode,
    echo_char: char,
    revealed: Option<Signal<bool>>,
    at_reveal_policy: AtRevealPolicy,
    allow_copy: bool,

    /// Semantic purpose → specialised AT role (WCAG 1.3.5). Ignored while the
    /// field is `secure` (password role wins).
    input_purpose: InputPurpose,

    /// ARIA combobox wiring — see [`active_descendant`](Self::active_descendant).
    active_descendant: Option<Signal<Option<WidgetId>>>,
    /// The listbox this field drives, if any — see
    /// [`controls`](Self::controls).
    controls: Option<Signal<Option<WidgetId>>>,

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
    /// Minted with the widget, not with its state, so a [`TextFieldHandle`]
    /// taken before `build` observes the signal the built widget writes.
    focus_signal: Signal<bool>,
    /// Natural intrinsic width in logical pixels, cached at the end
    /// of `build()`. When an [`InputMask`] is set, this measures the
    /// mask's empty template (e.g. `__/__/____`) in the theme body
    /// font and adds a small caret slack — so a date / time / phone
    /// field reports a width that matches its content envelope
    /// instead of the generic 200 dp fallback. Composing widgets
    /// like `DateEdit` rely on this so their unconstrained natural
    /// width tracks the format pattern.
    natural_width: f32,
    /// What the field's text measured, kept for the accessibility pass.
    ///
    /// A reader reviewing the field by character, word or line needs
    /// per-character extents, and the editing engine's own layout is not
    /// reachable from `accessibility()`. Written by `place_children` — the
    /// first pass that knows the final width — and again by `paint`, because
    /// a keystroke dirties the field at `RepaintOnly` / `AccessibilityOnly`
    /// and never relayouts: geometry taken from `place_children` alone would
    /// be one edit stale for as long as anyone is typing.
    retained: Rc<std::cell::RefCell<Option<RetainedText>>>,
}

impl std::fmt::Debug for TextInputField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputField")
            .field("placeholder", &self.placeholder)
            .field("enabled", &self.enabled.get())
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl TextInputField {
    /// Construct a new field bound to `text`.
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            enabled: Prop::Static(true),
            read_only: false,
            max_length: None,
            placeholder: String::new(),
            on_submit: None,
            on_access_set_value: None,
            on_blur: None,
            char_filter: None,
            suffix: Prop::Static(String::new()),
            text_height: None,
            external_interaction: None,
            mask: None,
            mask_placeholder_override: None,
            validator: None,
            feedback: Signal::new(ValidationFeedback::Pristine),
            secure: false,
            echo_mode: EchoMode::Masked,
            echo_char: '\u{2022}',
            revealed: None,
            at_reveal_policy: AtRevealPolicy::SwapRole,
            allow_copy: true,
            input_purpose: InputPurpose::Normal,
            active_descendant: None,
            controls: None,
            state: None,
            interaction: Signal::new(InteractionState::Idle),
            caret_position: Signal::new(0),
            state_slot: std::rc::Rc::new(std::cell::RefCell::new(None)),
            focus_signal: Signal::new(false),
            natural_width: 200.0,
            retained: Rc::new(std::cell::RefCell::new(None)),
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

    /// Set the enabled state, statically or reactively. Disabled blocks
    /// input and AccessKit interaction. Forwarded to the arena at build
    /// time.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
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

    /// Handle an assistive technology's whole-value write, given the string it
    /// set.
    ///
    /// Leave it unset for a field whose bound `Signal<String>` **is** the
    /// value: the write has already landed and there is nothing to derive.
    ///
    /// Install one for a field whose text is a *projection* of a typed value,
    /// as `SpinBox` and the date and time editors are. There the string is
    /// only a display of the real value, so without this an
    /// `Action::SetValue` resolved against the inner text node changes what is
    /// shown, leaves the typed value stale until the next blur, and never
    /// fires the host's change callback — an assistive technology or an
    /// automation client sees a success and the wrong value. The composite's
    /// own node handles `SetValue` properly; this closes the same door on the
    /// text node beneath it.
    ///
    /// The string is handed over rather than read back from the bound signal
    /// because the document→signal sync is deferred to the next frame tick, so
    /// a host reading the signal here would parse the text from *before* this
    /// edit and revert.
    pub fn on_access_set_value(
        mut self,
        f: impl Fn(&str, &mut EventContext) -> bool + 'static,
    ) -> Self {
        self.on_access_set_value = Some(std::rc::Rc::new(f));
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
    /// Accepts a static `String`/`&str` or a reactive `Signal<String>` /
    /// `Prop<String>`; when bound, the field re-measures the suffix glyphs
    /// and relayouts the editable text viewport each time the signal fires.
    /// Typical use: a `SpinBox` with `special_value_text` binds an empty
    /// string to the suffix whenever the value equals `min`, and the
    /// configured unit string otherwise.
    pub fn suffix(mut self, text: impl Into<Prop<String>>) -> Self {
        self.suffix = text.into();
        self
    }

    /// Override the intrinsic text-area height. The field is a
    /// pure leaf with no theme lookup of its own; by default it
    /// reports `DEFAULT_TEXT_HEIGHT`. A wrapping composite like
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
    pub fn validator(mut self, f: impl Fn(&str) -> ValidationOutcome + 'static) -> Self {
        self.validator = Some(Rc::new(f));
        self
    }

    /// Turn this into a secure (password) field with the given
    /// [`EchoMode`]. Masking happens at the text-engine layer (one echo
    /// glyph per source `char`), so the plaintext never reaches the
    /// shaper or glyph atlas while masked, and caret / selection /
    /// hit-test stay correct. Also defaults `allow_copy` to `false` and
    /// opts the focused node out of OS IME composition. Pair with
    /// [`revealed`](Self::revealed) for a reveal toggle.
    pub fn secure(mut self, echo_mode: EchoMode) -> Self {
        self.secure = true;
        self.echo_mode = echo_mode;
        self.allow_copy = false;
        self
    }

    /// Declare the field's semantic [`InputPurpose`] (WCAG 1.3.5), which
    /// selects a specialised AccessKit role (`EmailInput`, `PhoneNumberInput`,
    /// …) so screen readers announce the field's kind. Ignored while `secure`
    /// (the password role wins). Does not change IME behaviour — winit's
    /// `ImePurpose` has no email/number/url variants — nor drive OS autofill,
    /// which AccessKit cannot express (see `docs/a11y/a11y_issues.md`).
    pub fn input_purpose(mut self, purpose: InputPurpose) -> Self {
        self.input_purpose = purpose;
        self
    }

    /// Publish `active_descendant` pointing at the row a *separate* list is
    /// currently highlighting — the ARIA combobox pattern.
    ///
    /// Keyboard focus stays in this field while arrow keys move a highlight
    /// through a listbox elsewhere in the tree (a command palette, a
    /// type-ahead picker, a suggestion popup). Assistive technology follows
    /// the focused node's active descendant, so the announcement has to be
    /// published **here**, on the node that actually holds focus — not on the
    /// composite ancestor that owns the list. Without it the arrow keys move a
    /// highlight that is announced to nobody.
    ///
    /// Bound at `AccessibilityOnly`, so moving the highlight re-walks the AT
    /// tree without a rebuild or a repaint. Pair with [`controls`](Self::controls).
    pub fn active_descendant(mut self, active: Signal<Option<WidgetId>>) -> Self {
        self.active_descendant = Some(active);
        self
    }

    /// Publish a `controls` relation to the listbox this field drives, so an
    /// AT client can navigate from the input to the list it is filtering.
    /// The companion of [`active_descendant`](Self::active_descendant).
    pub fn controls(mut self, listbox: Signal<Option<WidgetId>>) -> Self {
        self.controls = Some(listbox);
        self
    }

    /// Override the masking glyph (default `'•'`, U+2022). Any
    /// uniform-width character works; the engine emits exactly one per
    /// source `char`.
    pub fn echo_char(mut self, c: char) -> Self {
        self.echo_char = c;
        self
    }

    /// Bind the reveal toggle. When the signal is `true` the field
    /// shows plaintext regardless of [`EchoMode`]; when `false` it
    /// masks. Shared with the eye [`IconButton::visibility_toggle`].
    ///
    /// [`IconButton::visibility_toggle`]: crate::IconButton::visibility_toggle
    pub fn revealed(mut self, revealed: Signal<bool>) -> Self {
        self.revealed = Some(revealed);
        self
    }

    /// How a *revealed* secure field reports to assistive tech. Default
    /// [`AtRevealPolicy::SwapRole`].
    pub fn at_reveal_policy(mut self, policy: AtRevealPolicy) -> Self {
        self.at_reveal_policy = policy;
        self
    }

    /// Permit (or forbid) copy / cut. Plain fields default `true`;
    /// [`secure`](Self::secure) flips the default to `false`. Even when
    /// `false`, copy is allowed while the field is revealed.
    pub fn allow_copy(mut self, allow: bool) -> Self {
        self.allow_copy = allow;
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

    /// Adopt an existing handle instead of minting one.
    ///
    /// For a composing widget — `TextInput` wraps this field — that must hand
    /// out a handle of its own **before** it builds the field it will delegate
    /// to. Sharing the slot and the focus signal makes the wrapper's handle and
    /// the field's the same handle, rather than two that agree by accident.
    pub fn share_handle(mut self, handle: &TextFieldHandle) -> Self {
        self.state_slot = handle.slot.clone();
        self.focus_signal = handle.focus_signal.clone();
        self
    }

    /// A live handle on this field, valid before and after `build`.
    ///
    /// The counterpart of `RichTextEditor::handle`, and the reason it exists:
    /// an application that routes Undo, Cut, Copy, Paste and Select All to
    /// "whichever text surface holds the caret" has to be able to *drive* every
    /// such surface, not only the rich editors. Without this, a menu built for
    /// those commands can only grey them out over a rename field or a search
    /// box while the field's own key handling still works — a menu that lies
    /// about what the keyboard can do.
    ///
    /// Like `caret_setter`, the handle reaches its state through the slot the
    /// widget late-populates, so it may be taken while the tree is being
    /// described and used once it is live.
    pub fn handle(&self) -> TextFieldHandle {
        TextFieldHandle {
            slot: self.state_slot.clone(),
            focus_signal: self.focus_signal.clone(),
        }
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
                st.cursor
                    .set_position(position, teksilo_text::text_document::MoveMode::MoveAnchor);
                let actual = st.cursor.position();
                if st.cursor_position.get() != actual {
                    st.cursor_position.set(actual);
                }
            }
        })
    }
}

impl TextInputField {
    /// Shape the field's text once more and keep the geometry.
    ///
    /// The whole line is measured with no width cap: the field scrolls
    /// rather than ellipsizes, so every character has an extent even while
    /// it sits outside the viewport, and `accessibility` slides the result
    /// by the scroll offset.
    fn retain_text_geometry(
        &self,
        bounds: Rect,
        style: &TextStyle,
        backend: &Rc<std::cell::RefCell<dyn teksilo_canvas::TextBackend>>,
        base_direction: teksilo_core::accesskit::TextDirection,
    ) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let (text, masked) = {
            let st = state.borrow();
            (
                st.document.to_plain_text().unwrap_or_default(),
                st.should_mask(),
            )
        };
        if masked {
            // Masking happens inside the editing engine precisely so the
            // secret never reaches the shaper or the glyph atlas; measuring
            // it here would put it there. A masked field is also
            // `Role::PasswordInput`, whose branch emits no runs to carry
            // geometry anyway.
            *self.retained.borrow_mut() = None;
            return;
        }
        let layout = backend.borrow_mut().layout_single_line(&text, style, None);
        *self.retained.borrow_mut() = Some(RetainedText {
            text,
            geometry: layout.geometry.clone(),
            bounds,
            base_direction,
        });
    }

    /// Borrow the shared state. Panics if called before `build()`
    /// has run — the state is allocated in `build()` from the
    /// builder config.
    fn state(&self) -> &SharedState {
        self.state
            .as_ref()
            .expect("TextInputField::state called before build")
    }
}

/// The reading direction the field's runs are announced with.
///
/// The single-line editing engine reports no per-segment direction, so the
/// ambient layout direction is the only answer available; a right-to-left
/// field therefore announces right-to-left even for Latin content, which is
/// what the surrounding UI does too.
fn base_text_direction(
    direction: teksilo_core::environment::LayoutDirection,
) -> teksilo_core::accesskit::TextDirection {
    match direction {
        teksilo_core::environment::LayoutDirection::RightToLeft => {
            teksilo_core::accesskit::TextDirection::RightToLeft
        }
        _ => teksilo_core::accesskit::TextDirection::LeftToRight,
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
    // Single-line input: no wrap, affinity is a no-op.
    let caret = st.engine.caret_rect(pos, CursorAffinity::Downstream);
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
fn paint_suffix_glyphs(canvas: &mut Canvas, frame: &teksilo_text::RenderFrame, origin: Point) {
    use teksilo_canvas::GlyphQuad as CanvasGlyphQuad;
    for g in frame.glyphs.iter() {
        let quad = CanvasGlyphQuad {
            screen: [
                g.screen[0] + origin.x,
                g.screen[1] + origin.y,
                g.screen[2],
                g.screen[3],
            ],
            atlas: g.atlas,
            color: g.color,
            is_color: g.is_color,
        };
        canvas.draw_glyph_quad(quad);
    }
}

/// The band a selection is painted in. Two axes decide it, and they do *not*
/// decide it the same way:
///
/// | field focus | window | band |
/// | --- | --- | --- |
/// | focused | active | vivid `selection_bg_active` |
/// | focused | inactive | muted `selection_bg_inactive` |
/// | not focused | either | nothing — fully transparent |
///
/// **An entry that does not hold focus paints no selection**, which is what
/// every native single-line field does. A Win32 edit control hides the
/// selection on focus-out unless it was created with `ES_NOHIDESEL`, and
/// WinForms spells the same default `TextBoxBase.HideSelection = true`.
/// `QLineEdit::focusOutEvent` goes further and calls `deselect()` outright for
/// every focus reason except `ActiveWindowFocusReason` and `PopupFocusReason`.
/// On macOS an `NSTextField` that stops being first responder has its shared
/// field editor detached, so there is no selection left to draw. GTK's entry is
/// the one toolkit that keeps a defocused selection lit, and that has been
/// filed against it as a papercut rather than defended as a design.
///
/// The *window* axis is the one where dimming, not hiding, is correct — and
/// the same three toolkits say so: Qt's carve-out for `ActiveWindowFocusReason`
/// exists precisely so a focused field keeps its selection when the window goes
/// to the background, and AppKit renders it there in
/// `unemphasizedSelectedTextBackgroundColor`. Losing the window is not the same
/// event as losing the caret.
///
/// Multi-line editors (`RichTextEditor`, `CodeEditor`, `LogView`) are
/// deliberately **not** on this rule: `QTextEdit` / `NSTextView` / every code
/// editor keep a visible selection in a blurred view, because there the
/// selection is a region of a document the user is working with rather than a
/// transient edit state.
///
/// The selection *state* survives blur either way — the `on_focus(false)` arm
/// spells out why (the right-click Copy path needs it) — this decides only what
/// is drawn.
fn field_selection_color(
    colors: &teksilo_tokens::ColorTokens,
    window_active: bool,
    has_focus: bool,
) -> [f32; 4] {
    match (has_focus, window_active) {
        (false, _) => [0.0; 4],
        (true, true) => colors.selection_bg_active.to_array(),
        (true, false) => colors.selection_bg_inactive.to_array(),
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

    // Blink only when focused AND the host window is active — the caret hides
    // in an inactive window (the universal desktop convention). The else-branch
    // below then turns it off, since `!blinking_active` now also covers the
    // window-inactive case.
    let caret_active = state.has_focus && state.window_active;
    let caret_visible = state.caret_visible.clone();
    let wake = state.frame_wake_at.clone();
    // A single-line field always blinks (no read-only/static presets), so it
    // hands the shared machine a fixed `Blinking` policy.
    state.blink.tick(
        CaretPolicy::Blinking,
        caret_active,
        &caret_visible,
        wake.as_ref(),
    );

    if state.needs_full_layout && state.viewport_width > 0.0 {
        state.layout_full_masked();
        state.needs_full_layout = false;
        state.content_dirty = true;
    }

    if state.pending_text_changed {
        let new_text = state.document.to_plain_text().unwrap_or_default();
        if state.text_signal.get() != new_text {
            state.deferred_text_update = Some(new_text);
        }
    }

    if state.debounce.tick(delta) {
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
    }
    let debounce_work = state.pending_text_changed || state.pending_undo_redo.is_some();

    had_events || debounce_work
}

/// Handle AccessKit actions (SetValue, SetTextSelection, Focus).
fn handle_access_action(
    state: &SharedState,
    action: teksilo_core::accesskit::Action,
    data: Option<teksilo_core::accesskit::ActionData>,
    ctx: &mut EventContext,
) -> EventResponse {
    use teksilo_core::accesskit::{Action, ActionData};

    match (action, data) {
        (Action::SetTextSelection, Some(ActionData::SetTextSelection(sel))) => {
            let st = state.borrow();
            st.cursor.set_position(
                sel.anchor.character_index,
                teksilo_text::text_document::MoveMode::MoveAnchor,
            );
            st.cursor.set_position(
                sel.focus.character_index,
                teksilo_text::text_document::MoveMode::KeepAnchor,
            );
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        // Both write arms are gated on the field being editable. The node
        // advertises neither action while `read_only` is set, but an adapter
        // dispatches what the technology asks for rather than what the node
        // offered — AT-SPI publishes `EditableText` off the interface set, not
        // off the action list — so a read-only `SpinBox`, `DateEdit`,
        // `TimeEdit`, `DateTimeEdit` or `DateRangeEdit` had its value rewritten
        // by anything that tried. Refusing here is the only place that covers
        // every host at once, and it reports the refusal instead of a silent
        // no-op.
        (Action::SetValue | Action::ReplaceSelectedText, _) if state.borrow().read_only => {
            EventResponse::Ignored
        }
        (Action::SetValue, Some(ActionData::Value(value))) => {
            // Snapshot the document before overwriting it. A host that refuses
            // the string has to be able to put the field back, and it cannot do
            // it from its own side: its revert writes the bound
            // `Signal<String>`, which still holds the *pre-edit* display at
            // this point (the document→signal sync is deferred to the next
            // frame tick), so the write is a no-op and the rejected string is
            // what the deferred sync then publishes.
            let before = {
                let st = state.borrow();
                st.document.to_plain_text().unwrap_or_default()
            };
            let st = state.borrow();
            st.cursor.select(SelectionType::Document);
            let _ = st.cursor.insert_text(value.as_ref());
            // A composite whose text only *projects* a typed value handles the
            // write itself, because an assistive technology's `SetValue` is a
            // finished edit, not a keystroke: without it a `SpinBox` would
            // show the new number, keep the old value until the next blur, and
            // never fire `on_value_changed`. It is handed the string, not left
            // to read the bound signal, which this edit has not synced yet.
            let host = st.on_access_set_value.clone();
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            // The host owns the verdict: it parses, clamps and — on a string it
            // cannot read — reverts the display to the value the composite
            // still holds. Reporting `Handled` regardless told the technology a
            // write had landed when the field had just thrown it away, so
            // Orca's value entry and macOS's `setAccessibilityValue:` both read
            // back success on `"twelve"`.
            match host {
                Some(host) if !host(value.as_ref(), ctx) => {
                    // Refused, so the field must not keep the refused string:
                    // reporting `Ignored` over a document still showing it is
                    // the same lie the other way round.
                    let st = state.borrow();
                    st.cursor.select(SelectionType::Document);
                    let _ = st.cursor.insert_text(&before);
                    drop(st);
                    sync_cursor_signals(state);
                    EventResponse::Ignored
                }
                _ => EventResponse::Handled,
            }
        }
        (Action::ReplaceSelectedText, Some(ActionData::Value(value))) => {
            // Insert at the caret, replacing the active selection (if
            // any) — NOT the whole document like `SetValue`. This is the
            // AT-SPI (Linux) / UIA (Windows) braille-keyboard and
            // dictation insertion path; macOS routes insertion through
            // `SetValue` instead, so this never fires there. We advertise
            // the action in `accessibility()`, so we must service it.
            let st = state.borrow();
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

/// Build a fresh right-click context menu widget. Called from the
/// `.context_menu(...)` factory on every right-click, so each open
/// reads live `has_selection` / `is_empty` state when computing each
/// item's enabled flag.
fn build_context_menu_widget(state: &SharedState) -> Box<dyn Widget> {
    let st = state.borrow();
    let has_selection = st.cursor.has_selection();
    let doc_non_empty = !st.document.to_plain_text().unwrap_or_default().is_empty();
    // Secure fields suppress Cut / Copy while masked (still allowed when
    // revealed or when the developer opted in via `allow_copy`).
    let copy_allowed = st.copy_allowed();
    drop(st);

    let state_cut = state.clone();
    let state_copy = state.clone();
    let state_paste = state.clone();
    let state_select_all = state.clone();

    Box::new(
        MenuList::new()
            .item(
                MenuItem::new(tr_widget!(menu_cut()))
                    .shortcut_label(format_keystroke(KeyStroke::command(Key::X)))
                    .enabled(has_selection && copy_allowed)
                    .on_activate_fn(move |ctx| {
                        {
                            let mut st = state_cut.borrow_mut();
                            keyboard::clipboard_cut(&mut st, ctx);
                        }
                        sync_cursor_signals(&state_cut);
                        ctx.request_frame();
                    }),
            )
            .item(
                MenuItem::new(tr_widget!(menu_copy()))
                    .shortcut_label(format_keystroke(KeyStroke::command(Key::C)))
                    .enabled(has_selection && copy_allowed)
                    .on_activate_fn(move |ctx| {
                        let mut st = state_copy.borrow_mut();
                        keyboard::clipboard_copy(&mut st, ctx);
                    }),
            )
            .item(
                MenuItem::new(tr_widget!(menu_paste()))
                    .shortcut_label(format_keystroke(KeyStroke::command(Key::V)))
                    .on_activate_fn(move |ctx| {
                        {
                            let mut st = state_paste.borrow_mut();
                            keyboard::clipboard_paste(&mut st, ctx);
                        }
                        sync_cursor_signals(&state_paste);
                        ctx.request_frame();
                    }),
            )
            .item(MenuSeparator)
            .item(
                MenuItem::new(tr_widget!(menu_select_all()))
                    .shortcut_label(format_keystroke(KeyStroke::command(Key::A)))
                    .enabled(doc_non_empty)
                    .on_activate_fn(move |ctx| {
                        {
                            let st = state_select_all.borrow();
                            st.cursor.select(SelectionType::Document);
                        }
                        sync_cursor_signals(&state_select_all);
                        ctx.request_frame();
                    }),
            ),
    )
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

/// Build the worst-case-glyph version of an [`InputMask`] for
/// natural-width measurement: every editable slot holds the widest
/// plausible character its class can accept, and every fixed slot
/// holds its literal. Used by `build()` to size the field's
/// intrinsic envelope so a fully-typed value never overflows the
/// reported natural width.
///
/// Per-class worst-case glyph (Inter and most UI sans-serifs):
/// - `Digit` → `0` (tabular figures are constant-width, but `0` is
///   representative for fonts that aren't)
/// - `Letter` / `Alphanumeric` / `Any` → `M` (widest cap glyph)
/// - `HexDigit` → `0`
fn worst_case_template(mask: &InputMask) -> String {
    let mut s = String::with_capacity(mask.len());
    for pos in mask.positions() {
        match pos {
            MaskPosition::Editable { class, .. } => {
                s.push(match class {
                    MaskClass::Digit | MaskClass::HexDigit => '0',
                    MaskClass::Letter | MaskClass::Alphanumeric | MaskClass::Any => 'M',
                });
            }
            MaskPosition::Fixed(c) => s.push(*c),
        }
    }
    s
}

/// Measure the advance width of `text` in logical pixels using the
/// app-wide `SharedTypesetter` (the same backend the field paints
/// with). Falls back to a per-character-class heuristic when no
/// typesetter is installed (headless tests) so the caller still gets
/// a non-zero width and any natural-width / cap logic behaves
/// reasonably even there. The fallback weights match Inter's body
/// proportions closely enough that the difference between an
/// underscore and a wide cap glyph (`M`) shows up in headless tests
/// — important for verifying the worst-case-glyph mask measurement
/// without booting a typesetter.
fn measure_width_px(ctx: &mut BuildContext, text: &str, style: &TextStyle) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    if let Some(ts) = ctx.app_state::<SharedTypesetter>() {
        let backend = ts.as_text_backend();
        let layout = backend.borrow_mut().layout_single_line(text, style, None);
        return layout.width;
    }
    let em = style.size;
    text.chars()
        .map(|c| match c {
            ' ' => 0.30,
            '_' => 0.45,
            ':' | '.' | ',' | ';' | '/' | '|' | '!' | 'i' | 'l' | 'I' => 0.30,
            '0'..='9' => 0.55,
            'M' | 'W' | 'm' | 'w' => 0.85,
            'A'..='Z' => 0.65,
            'a'..='z' => 0.50,
            _ => 0.55,
        })
        .map(|w: f32| w * em)
        .sum()
}

#[cfg(test)]
mod text_run_tests {
    use super::*;
    use std::cell::RefCell;
    use teksilo_canvas::{MockTextBackend, SizeProposal};
    use teksilo_core::accesskit::Role;
    use teksilo_core::signal::Signal;
    use teksilo_core::widget_id::WidgetId;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_text::text_document::MoveMode;

    fn tree_with_mock_backend() -> WidgetTree {
        WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light())
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
    }

    fn state_of(tree: &WidgetTree, id: WidgetId) -> SharedState {
        tree.widget_as_any(id)
            .and_then(|w| w.downcast_ref::<TextInputField>())
            .map(|field| field.state().clone())
            .expect("a built TextInputField")
    }

    #[test]
    fn a_caret_past_255_chars_lands_in_the_second_chunk() {
        // `accesskit_consumer` probes a position's character index as a `u8`,
        // so the emitter splits a long line into 255-character runs. The
        // field still speaks in document character offsets, and a caret at
        // 260 has to resolve to the run that holds it — reporting it against
        // the first run would put the caret 255 characters behind the text.
        let mut tree = tree_with_mock_backend();
        let id = tree.add(TextInputField::new(Signal::new("a".repeat(300))));
        tree.layout(SizeProposal::exact(200.0, 20.0));

        state_of(&tree, id)
            .borrow_mut()
            .cursor
            .set_position(260, MoveMode::MoveAnchor);

        let update = tree.sync_accessibility();
        let (_, input) = update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::TextInput)
            .expect("the field reports a text-input node");
        let runs = input.children();
        assert_eq!(runs.len(), 2, "300 characters split at the 255 cap");

        let selection = input
            .text_selection()
            .expect("the field exposes its caret to assistive technology");
        assert_eq!(selection.focus.node, runs[1]);
        assert_eq!(selection.focus.character_index, 5);
    }

    #[test]
    fn a_protected_field_emits_no_text_runs() {
        // A run publishes the character count, the per-character extents and
        // the word boundaries of what it carries. On a masked field that is a
        // description of the password, so the protected branch emits the
        // bullet string and nothing else.
        let mut tree = tree_with_mock_backend();
        let _id = tree
            .add(TextInputField::new(Signal::new("hunter2".to_string())).secure(EchoMode::Masked));
        tree.layout(SizeProposal::exact(200.0, 20.0));

        let update = tree.sync_accessibility();
        let (_, field) = update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::PasswordInput)
            .expect("a masked field reports Role::PasswordInput");
        assert_eq!(field.value(), Some("•••••••"));
        assert!(
            field.children().is_empty(),
            "a masked field must own no text runs"
        );
        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::TextRun),
            "no text run may be emitted anywhere for a masked field"
        );
        assert!(
            field.text_selection().is_none(),
            "the caret model stays opaque so no structure about the secret leaks"
        );
    }
}

#[cfg(test)]
mod window_active_tests;

/// **A key the platform decorates with control text must still bubble.**
///
/// These dispatch `KeyDown` with the `text` a real keyboard carries. Every
/// synthetic helper in the workspace sends `text: None`, which skips the branch
/// under test entirely — so a test written with `press_key` passes on the bug.
#[cfg(test)]
mod key_text_bubbling_tests;

/// A live handle on a [`TextInputField`] — its text-editing commands, for a
/// caller outside the widget.
///
/// Every method is a no-op before the field is built (and after it is
/// destroyed), which is the honest answer rather than a panic: a menu row bound
/// to a field that is no longer on screen should do nothing, not crash.
#[derive(Clone)]
pub struct TextFieldHandle {
    slot: std::rc::Rc<std::cell::RefCell<Option<SharedState>>>,
    focus_signal: Signal<bool>,
}

impl std::fmt::Debug for TextFieldHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldHandle")
            .field("live", &self.slot.borrow().is_some())
            .field("focused", &self.focus_signal.get())
            .finish()
    }
}

impl TextFieldHandle {
    /// A handle not yet attached to any field — for a composing widget that
    /// hands one out before building the field it will delegate to. Every
    /// method answers "nothing" until [`TextInputField::share_handle`] binds it.
    pub fn detached() -> Self {
        Self {
            slot: std::rc::Rc::new(std::cell::RefCell::new(None)),
            focus_signal: Signal::new(false),
        }
    }

    /// `true` while this field holds the keyboard focus. Observable, so a
    /// router can follow the caret without polling.
    pub fn focused_signal(&self) -> Signal<bool> {
        self.focus_signal.clone()
    }

    /// Is the widget built and still alive?
    pub fn is_live(&self) -> bool {
        self.slot.borrow().is_some()
    }

    fn with<R>(&self, f: impl FnOnce(&mut TextInputState) -> R) -> Option<R> {
        let slot = self.slot.borrow();
        let state = slot.as_ref()?;
        let mut st = state.borrow_mut();
        Some(f(&mut st))
    }

    /// The field's current text.
    pub fn text(&self) -> String {
        self.with(|st| st.document.to_plain_text().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Is any text selected right now?
    pub fn has_selection(&self) -> bool {
        self.with(|st| st.cursor.has_selection()).unwrap_or(false)
    }

    /// May this field's content be copied at all? A password field says no —
    /// see [`TextInputField::allow_copy`].
    pub fn allows_copy(&self) -> bool {
        self.with(|st| st.allow_copy).unwrap_or(false)
    }

    /// Is the field refusing edits? Cut and Paste are meaningless when it is.
    pub fn is_read_only(&self) -> bool {
        self.with(|st| st.read_only).unwrap_or(true)
    }

    /// Select the whole field.
    pub fn select_all(&self) {
        self.with(|st| st.cursor.select(SelectionType::Document));
    }

    /// Copy the selection to the clipboard.
    pub fn copy(&self, ctx: &EventContext) {
        self.with(|st| keyboard::clipboard_copy(st, ctx));
    }

    /// Cut the selection to the clipboard.
    pub fn cut(&self, ctx: &EventContext) {
        self.with(|st| keyboard::clipboard_cut(st, ctx));
    }

    /// Paste over the selection.
    pub fn paste(&self, ctx: &EventContext) {
        self.with(|st| keyboard::clipboard_paste(st, ctx));
    }

    /// Undo this field's own last edit.
    pub fn undo(&self) {
        self.with(|st| {
            let _ = st.document.undo();
        });
    }

    /// Redo this field's own last undone edit.
    pub fn redo(&self) {
        self.with(|st| {
            let _ = st.document.redo();
        });
    }

    /// Is there anything to undo? Debounced like the editor's twin.
    pub fn can_undo(&self) -> Signal<bool> {
        self.with(|st| st.can_undo.clone())
            .unwrap_or_else(|| Signal::new(false))
    }

    /// Is there anything to redo?
    pub fn can_redo(&self) -> Signal<bool> {
        self.with(|st| st.can_redo.clone())
            .unwrap_or_else(|| Signal::new(false))
    }
}

// ── The framework's uniform view of a text-editing widget ────────────────────

impl teksilo_core::text_surface::TextSurface for TextFieldHandle {
    fn can_undo(&self) -> bool {
        TextFieldHandle::can_undo(self).get()
    }

    fn can_redo(&self) -> bool {
        TextFieldHandle::can_redo(self).get()
    }

    fn undo(&self) {
        TextFieldHandle::undo(self);
    }

    fn redo(&self) {
        TextFieldHandle::redo(self);
    }

    fn has_selection(&self) -> bool {
        TextFieldHandle::has_selection(self)
    }

    fn is_read_only(&self) -> bool {
        TextFieldHandle::is_read_only(self)
    }

    fn allows_copy(&self) -> bool {
        TextFieldHandle::allows_copy(self)
    }

    fn cut(&self, ctx: &teksilo_core::widget::EventContext<'_>) {
        TextFieldHandle::cut(self, ctx);
    }

    fn copy(&self, ctx: &teksilo_core::widget::EventContext<'_>) {
        TextFieldHandle::copy(self, ctx);
    }

    fn paste(&self, ctx: &teksilo_core::widget::EventContext<'_>) {
        TextFieldHandle::paste(self, ctx);
    }

    /// A one-line field carries no formatting to strip, so the plain paste
    /// *is* the paste. Answering "nothing" here would make Edit ▸ Paste without
    /// formatting silently dead over a rename box.
    fn paste_plain(&self, ctx: &teksilo_core::widget::EventContext<'_>) {
        TextFieldHandle::paste(self, ctx);
    }

    fn select_all(&self) {
        TextFieldHandle::select_all(self);
    }
}
