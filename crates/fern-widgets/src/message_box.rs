//! MessageBox — QMessageBox-style alert dialog.
//!
//! A higher-level surface built on top of [`ModalContainer`]
//! for the classic "tell the user something and ask for a response"
//! pattern: unsaved-changes prompts, error surfaces, confirmation
//! dialogs, and informational notices. Mirrors QMessageBox (Qt),
//! NSAlert (AppKit), and SwiftUI's `.alert(...)` while staying inside
//! FernUI's idioms — closure result handlers, `Signal`/`Prop`
//! reactivity, `Intent`/`Action`/`Shortcut` routing for keyboard
//! defaults, and AccessKit `Role::AlertDialog` accessibility.
//!
//! ## Quick tour
//!
//! ```ignore
//! use fern_ui::prelude::*;
//! use fern_ui::widgets::{MessageBox, MessageBoxButtons, StandardButton};
//!
//! fn on_close(ctx: &mut EventContext) {
//!     MessageBox::question_literal("Save changes?")
//!         .text_literal("You have unsaved changes in report.skrib.")
//!         .informative_text_literal("Your changes will be lost if you don't save them.")
//!         .buttons(MessageBoxButtons::SaveDiscardCancel)
//!         .default_button(StandardButton::Save)
//!         .escape_button(StandardButton::Cancel)
//!         .on_result(|r, ctx| match r.button {
//!             StandardButton::Save => save_and_close(ctx),
//!             StandardButton::Discard => close(ctx),
//!             _ => {}
//!         })
//!         .present(ctx);
//! }
//! # fn save_and_close(_: &mut EventContext) {}
//! # fn close(_: &mut EventContext) {}
//! ```
//!
//! ## Severity
//!
//! [`MessageBoxSeverity`] controls the icon drawn beside the title and
//! its tint:
//!
//! - `Information` — info glyph, `status_info_fg` tint.
//! - `Question` — question mark glyph, `accent` tint.
//! - `Warning` — exclamation triangle, `status_warning_fg` tint.
//! - `Critical` — X-mark circle, `status_error_fg` tint. Also disables
//!   click-outside dismissal (Qt convention).
//! - `None` — no icon, no tint.
//!
//! Severity is conveyed through the icon + title + text. Per FernUI's
//! Int UI baseline, buttons are **never** colored as "destructive":
//! destructive intent lives in the dialog's severity and wording, not
//! in the button. See [`crate::button`] for details.
//!
//! ## Default & escape buttons
//!
//! - `default_button` — activated by Enter (widget-scoped shortcut) and
//!   receives initial focus on open (via `ModalRequest::focus_target`
//!   plus `Widget::initial_focus_hint`). Styled with
//!   `ButtonVariant::Default`.
//! - `escape_button` — activated by Escape. The fallback logic (for
//!   presets with no explicit `escape_button`) picks: explicit
//!   `escape_button` → first `Reject`-role button → `Cancel` → last
//!   button.
//!
//! ## Result reporting
//!
//! [`MessageBox::on_result`] takes `impl Fn(MessageBoxResult,
//! &mut EventContext) + 'static`. The callback fires exactly once — on
//! button activation or Escape dismissal — then the modal is closed by
//! the framework.
//!
//! ## Accessibility
//!
//! The widget exposes `Role::AlertDialog` (distinct from
//! `ModalContainer`'s `Role::Dialog`), with `set_modal()`,
//! `set_live(Live::Assertive)`, `set_name(title)`, and
//! `set_description(text + informative_text)` so screen readers
//! announce the dialog and its body on open.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use fern_canvas::{Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::action::Action;
use fern_core::build_context::BuildContext;
use fern_core::event::{Key, Modifiers};
use fern_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use fern_core::shortcut::{KeyStroke, Shortcut};
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_i18n::LocalizedString;
use fern_tokens::{Color, VAlignment};

use crate::accordion::Accordion;
use crate::button::{Button, ButtonVariant};
use crate::checkbox::Checkbox;
use crate::dialog::ModalContainer;
use crate::primitives::icon_widget::IconMode;
use crate::primitives::{HStack, IconWidget, Spacer, TextWidget, VStack};

// ── Severity ────────────────────────────────────────────────────────

/// Alert severity level. Drives the icon glyph + tint shown beside the
/// title, and (for `Critical`) whether click-outside dismiss is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageBoxSeverity {
    /// No icon. Use for plain notices where an icon would be noise.
    #[default]
    None,
    /// Informational notice — blue circle with "i" glyph.
    Information,
    /// Confirmation prompt — accent-tinted circle with "?" glyph.
    Question,
    /// Non-fatal warning — amber triangle with "!" glyph.
    Warning,
    /// Critical error — red circle with an "X" glyph. Click-outside
    /// dismissal is disabled (Escape still works).
    Critical,
}

// ── Standard button catalog ─────────────────────────────────────────

/// Semantic role of a message-box button. Used for fallback escape
/// resolution (`Reject` wins when no explicit escape button is set).
/// Unlike some toolkits, FernUI deliberately does **not** render
/// `Destructive` buttons with a red fill — the dialog's severity icon
/// and wording carry that signal. See [`crate::button`] for the
/// framework-level rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonRole {
    /// Confirms / proceeds. Ok, Yes, Save, Open, Apply, Retry.
    Accept,
    /// Bails out. Cancel, Close, No, Abort.
    Reject,
    /// Data-loss action. Discard. (Same visuals as Regular — the
    /// severity of the surrounding MessageBox carries the warning.)
    Destructive,
    /// Side action. Help, Reset, RestoreDefaults, Ignore, and the
    /// "to all" variants.
    Action,
}

/// The Qt-modeled catalog of standard buttons. Each variant resolves
/// to a localized label, a semantic [`ButtonRole`], and a stable
/// intent-name string used internally for shortcut/action routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardButton {
    Ok,
    Cancel,
    Close,
    Yes,
    No,
    YesToAll,
    NoToAll,
    Save,
    SaveAll,
    Discard,
    Apply,
    Reset,
    RestoreDefaults,
    Abort,
    Retry,
    Ignore,
    Open,
    Help,
}

impl StandardButton {
    /// The button's semantic role — used internally by MessageBox's
    /// escape-button fallback resolution, and available to callers that
    /// want to inspect a `MessageBoxButton`'s role.
    pub fn role(self) -> ButtonRole {
        match self {
            Self::Ok | Self::Yes | Self::YesToAll | Self::Save | Self::SaveAll
            | Self::Apply | Self::Retry | Self::Open => ButtonRole::Accept,
            Self::Cancel | Self::Close | Self::No | Self::NoToAll | Self::Abort => {
                ButtonRole::Reject
            }
            Self::Discard => ButtonRole::Destructive,
            Self::Reset | Self::RestoreDefaults | Self::Ignore | Self::Help => ButtonRole::Action,
        }
    }

    /// Stable string id used as both the shortcut id and the intent
    /// name for routing default/escape key activations. Scoped to a
    /// MessageBox instance via widget-scoped shortcut registration, so
    /// the same id is safe to reuse across instances.
    pub fn intent_name(self) -> &'static str {
        match self {
            Self::Ok => "messagebox.btn.ok",
            Self::Cancel => "messagebox.btn.cancel",
            Self::Close => "messagebox.btn.close",
            Self::Yes => "messagebox.btn.yes",
            Self::No => "messagebox.btn.no",
            Self::YesToAll => "messagebox.btn.yes_to_all",
            Self::NoToAll => "messagebox.btn.no_to_all",
            Self::Save => "messagebox.btn.save",
            Self::SaveAll => "messagebox.btn.save_all",
            Self::Discard => "messagebox.btn.discard",
            Self::Apply => "messagebox.btn.apply",
            Self::Reset => "messagebox.btn.reset",
            Self::RestoreDefaults => "messagebox.btn.restore_defaults",
            Self::Abort => "messagebox.btn.abort",
            Self::Retry => "messagebox.btn.retry",
            Self::Ignore => "messagebox.btn.ignore",
            Self::Open => "messagebox.btn.open",
            Self::Help => "messagebox.btn.help",
        }
    }

    /// Default label for the button. Resolved through the Fluent
    /// catalog via `tr_widget!` so apps can override per-locale.
    pub fn default_label(self) -> LocalizedString {
        match self {
            Self::Ok => fern_i18n::tr_widget!(messagebox_btn_ok()),
            Self::Cancel => fern_i18n::tr_widget!(messagebox_btn_cancel()),
            Self::Close => fern_i18n::tr_widget!(messagebox_btn_close()),
            Self::Yes => fern_i18n::tr_widget!(messagebox_btn_yes()),
            Self::No => fern_i18n::tr_widget!(messagebox_btn_no()),
            Self::YesToAll => fern_i18n::tr_widget!(messagebox_btn_yes_to_all()),
            Self::NoToAll => fern_i18n::tr_widget!(messagebox_btn_no_to_all()),
            Self::Save => fern_i18n::tr_widget!(messagebox_btn_save()),
            Self::SaveAll => fern_i18n::tr_widget!(messagebox_btn_save_all()),
            Self::Discard => fern_i18n::tr_widget!(messagebox_btn_discard()),
            Self::Apply => fern_i18n::tr_widget!(messagebox_btn_apply()),
            Self::Reset => fern_i18n::tr_widget!(messagebox_btn_reset()),
            Self::RestoreDefaults => fern_i18n::tr_widget!(messagebox_btn_restore_defaults()),
            Self::Abort => fern_i18n::tr_widget!(messagebox_btn_abort()),
            Self::Retry => fern_i18n::tr_widget!(messagebox_btn_retry()),
            Self::Ignore => fern_i18n::tr_widget!(messagebox_btn_ignore()),
            Self::Open => fern_i18n::tr_widget!(messagebox_btn_open()),
            Self::Help => fern_i18n::tr_widget!(messagebox_btn_help()),
        }
    }
}

/// A single button placement inside a MessageBox, including an optional
/// per-instance label override. Callers usually build these via
/// [`From<StandardButton>`] (`StandardButton::Ok.into()`), or
/// construct them manually when `Custom` is needed.
#[derive(Debug, Clone)]
pub struct MessageBoxButton {
    /// Which standard button this is — drives role, intent name, and
    /// (when `label_override` is `None`) the label.
    pub kind: StandardButton,
    /// Optional explicit label that overrides `kind.default_label()`.
    /// Use sparingly: prefer translating the default via Fluent rather
    /// than hard-coding per-call labels.
    pub label_override: Option<LocalizedString>,
}

impl MessageBoxButton {
    /// Build a button from a `StandardButton` with the default label.
    pub fn standard(kind: StandardButton) -> Self {
        Self {
            kind,
            label_override: None,
        }
    }

    /// Override the default translated label.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label_override = Some(label.into());
        self
    }

    /// Raw-string label override — mirrors `Button::new_literal`.
    #[doc(hidden)]
    pub fn label_literal(mut self, label: impl Into<String>) -> Self {
        self.label_override = Some(LocalizedString::literal(label));
        self
    }

    fn resolved_label(&self) -> LocalizedString {
        self.label_override
            .clone()
            .unwrap_or_else(|| self.kind.default_label())
    }
}

impl From<StandardButton> for MessageBoxButton {
    fn from(kind: StandardButton) -> Self {
        Self::standard(kind)
    }
}

/// Pre-built button bundles covering the common MessageBox shapes.
/// Custom combinations go through [`MessageBox::add_button`] or
/// [`MessageBoxButtons::Custom`].
#[derive(Debug, Clone)]
pub enum MessageBoxButtons {
    /// Just Ok.
    Ok,
    /// Ok + Cancel, Ok default, Cancel escape.
    OkCancel,
    /// Yes + No, Yes default, No escape.
    YesNo,
    /// Yes + No + Cancel, Yes default, Cancel escape.
    YesNoCancel,
    /// The unsaved-changes triad: Save + Discard + Cancel.
    SaveDiscardCancel,
    /// The error-recovery triad: Retry + Ignore + Abort.
    RetryIgnoreAbort,
    /// Explicit list. MessageBox preserves the order as the visual
    /// button order (leading Spacer pushes all buttons to the trailing
    /// edge; default button may appear anywhere).
    Custom(Vec<MessageBoxButton>),
}

impl MessageBoxButtons {
    fn into_buttons(self) -> Vec<MessageBoxButton> {
        match self {
            Self::Ok => vec![StandardButton::Ok.into()],
            Self::OkCancel => vec![StandardButton::Cancel.into(), StandardButton::Ok.into()],
            Self::YesNo => vec![StandardButton::No.into(), StandardButton::Yes.into()],
            Self::YesNoCancel => vec![
                StandardButton::Cancel.into(),
                StandardButton::No.into(),
                StandardButton::Yes.into(),
            ],
            Self::SaveDiscardCancel => vec![
                StandardButton::Discard.into(),
                StandardButton::Cancel.into(),
                StandardButton::Save.into(),
            ],
            Self::RetryIgnoreAbort => vec![
                StandardButton::Abort.into(),
                StandardButton::Ignore.into(),
                StandardButton::Retry.into(),
            ],
            Self::Custom(items) => items,
        }
    }

    /// Default button hint derived from the preset. Callers that want
    /// a different default override via `MessageBox::default_button`.
    fn preset_default(&self) -> Option<StandardButton> {
        match self {
            Self::Ok => Some(StandardButton::Ok),
            Self::OkCancel => Some(StandardButton::Ok),
            Self::YesNo => Some(StandardButton::Yes),
            Self::YesNoCancel => Some(StandardButton::Yes),
            Self::SaveDiscardCancel => Some(StandardButton::Save),
            Self::RetryIgnoreAbort => Some(StandardButton::Retry),
            Self::Custom(_) => None,
        }
    }

    /// Escape button hint derived from the preset.
    fn preset_escape(&self) -> Option<StandardButton> {
        match self {
            Self::Ok => Some(StandardButton::Ok),
            Self::OkCancel => Some(StandardButton::Cancel),
            Self::YesNo => Some(StandardButton::No),
            Self::YesNoCancel => Some(StandardButton::Cancel),
            Self::SaveDiscardCancel => Some(StandardButton::Cancel),
            Self::RetryIgnoreAbort => Some(StandardButton::Abort),
            Self::Custom(_) => None,
        }
    }
}

// ── Result ──────────────────────────────────────────────────────────

/// Report passed to [`MessageBox::on_result`] when the dialog closes.
#[derive(Debug, Clone, Copy)]
pub struct MessageBoxResult {
    /// Which button fired — either by click, Enter (default button),
    /// or Escape (escape button resolution).
    pub button: StandardButton,
    /// State of the "Don't show again" checkbox at dismiss time, when
    /// one was configured via [`MessageBox::show_again_checkbox`] or
    /// [`MessageBox::show_again_checkbox_state`]. `false` when no
    /// checkbox was attached.
    pub checkbox_checked: bool,
    /// `true` when the user dismissed via Escape (or scrim-click, when
    /// permitted) rather than clicking a button directly.
    pub dismissed_by_escape: bool,
}

const SEVERITY_ICON_SIZE: f32 = 48.0;

const DEFAULT_INTENT_NAME: &str = "messagebox.accept_default";
const ESCAPE_INTENT_NAME: &str = "messagebox.escape";

fn severity_icon(severity: MessageBoxSeverity) -> Option<Path> {
    let size = SEVERITY_ICON_SIZE;
    let center = Point::new(size / 2.0, size / 2.0);
    match severity {
        MessageBoxSeverity::None => None,
        MessageBoxSeverity::Information
        | MessageBoxSeverity::Question
        | MessageBoxSeverity::Critical => Some(Path::circle(center, size / 2.0)),
        MessageBoxSeverity::Warning => {
            let mut path = Path::new();
            path.move_to(Point::new(size / 2.0, 2.0));
            path.line_to(Point::new(size - 2.0, size - 2.0));
            path.line_to(Point::new(2.0, size - 2.0));
            path.close();
            Some(path)
        }
    }
}

fn severity_color(theme: &fern_tokens::Theme, severity: MessageBoxSeverity) -> Color {
    match severity {
        MessageBoxSeverity::None => theme.colors.text_secondary,
        MessageBoxSeverity::Information => theme.colors.status_info_fg,
        MessageBoxSeverity::Question => theme.colors.accent,
        MessageBoxSeverity::Warning => theme.colors.status_warning_fg,
        MessageBoxSeverity::Critical => theme.colors.status_error_fg,
    }
}

// ── Internal runtime state ─────────────────────────────────────────

/// State shared between the MessageBox widget, its footer buttons, and
/// the Enter/Escape shortcut actions. Lives in an `Rc` so all three
/// dispatch paths can read/write the same checkbox state and fire the
/// same result callback exactly once per session.
struct State {
    on_result: RefCell<Option<Box<dyn Fn(MessageBoxResult, &mut EventContext)>>>,
    checkbox: Signal<bool>,
    escape_button: Cell<Option<StandardButton>>,
    default_button: Cell<Option<StandardButton>>,
    /// Fallback list of buttons in the order they were configured —
    /// consulted when neither `escape_button` nor a `Reject`-role
    /// button is set.
    buttons: RefCell<Vec<StandardButton>>,
    /// Guards against multiple result-callback invocations when a
    /// button click races the Escape shortcut.
    fired: Cell<bool>,
}

impl State {
    fn new(checkbox: Signal<bool>) -> Rc<Self> {
        Rc::new(Self {
            on_result: RefCell::new(None),
            checkbox,
            escape_button: Cell::new(None),
            default_button: Cell::new(None),
            buttons: RefCell::new(Vec::new()),
            fired: Cell::new(false),
        })
    }

    fn fire(&self, button: StandardButton, by_escape: bool, ctx: &mut EventContext) {
        if self.fired.replace(true) {
            return;
        }
        let result = MessageBoxResult {
            button,
            checkbox_checked: self.checkbox.get(),
            dismissed_by_escape: by_escape,
        };
        if let Some(handler) = self.on_result.borrow().as_ref() {
            handler(result, ctx);
        }
        ctx.dismiss_modal();
    }

    fn resolve_escape_button(&self) -> Option<StandardButton> {
        if let Some(btn) = self.escape_button.get() {
            return Some(btn);
        }
        let buttons = self.buttons.borrow();
        if let Some(btn) = buttons.iter().find(|b| b.role() == ButtonRole::Reject) {
            return Some(*btn);
        }
        if buttons.iter().any(|b| *b == StandardButton::Cancel) {
            return Some(StandardButton::Cancel);
        }
        buttons.last().copied()
    }
}

// ── The MessageBox widget ──────────────────────────────────────────

/// A QMessageBox-style alert dialog. Constructed via severity-named
/// constructors ([`MessageBox::information`], [`MessageBox::warning`],
/// [`MessageBox::critical`], [`MessageBox::question`],
/// [`MessageBox::plain`]), configured fluently, and presented with
/// [`MessageBox::present`].
pub struct MessageBox {
    severity: MessageBoxSeverity,
    title: String,
    text: Option<String>,
    informative_text: Option<String>,
    detailed_text: Option<String>,
    buttons_config: Option<MessageBoxButtons>,
    extra_buttons: Vec<MessageBoxButton>,
    default_button: Option<StandardButton>,
    escape_button: Option<StandardButton>,
    show_again_label: Option<String>,
    show_again_state: Option<Signal<bool>>,
    on_result: Option<Box<dyn Fn(MessageBoxResult, &mut EventContext)>>,
    default_button_id: Cell<Option<WidgetId>>,
    root_child_id: Option<WidgetId>,
    state: Option<Rc<State>>,
}

impl std::fmt::Debug for MessageBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageBox")
            .field("severity", &self.severity)
            .field("title", &self.title)
            .field("text", &self.text)
            .field("informative_text", &self.informative_text)
            .field("detailed_text", &self.detailed_text)
            .field("default_button", &self.default_button)
            .field("escape_button", &self.escape_button)
            .finish()
    }
}

impl MessageBox {
    fn new_with_severity(severity: MessageBoxSeverity, title: impl Into<LocalizedString>) -> Self {
        let title = title.into().resolve_now();
        Self {
            severity,
            title,
            text: None,
            informative_text: None,
            detailed_text: None,
            buttons_config: None,
            extra_buttons: Vec::new(),
            default_button: None,
            escape_button: None,
            show_again_label: None,
            show_again_state: None,
            on_result: None,
            default_button_id: Cell::new(None),
            root_child_id: None,
            state: None,
        }
    }

    /// Construct an informational MessageBox (`Information` severity).
    pub fn information(title: impl Into<LocalizedString>) -> Self {
        Self::new_with_severity(MessageBoxSeverity::Information, title)
    }

    /// Construct a warning MessageBox (`Warning` severity).
    pub fn warning(title: impl Into<LocalizedString>) -> Self {
        Self::new_with_severity(MessageBoxSeverity::Warning, title)
    }

    /// Construct a critical-error MessageBox (`Critical` severity).
    /// Click-outside dismissal is disabled; use an explicit button or
    /// Escape to close.
    pub fn critical(title: impl Into<LocalizedString>) -> Self {
        Self::new_with_severity(MessageBoxSeverity::Critical, title)
    }

    /// Construct a confirmation / question MessageBox (`Question`
    /// severity).
    pub fn question(title: impl Into<LocalizedString>) -> Self {
        Self::new_with_severity(MessageBoxSeverity::Question, title)
    }

    /// Construct a plain MessageBox with no severity icon.
    pub fn plain(title: impl Into<LocalizedString>) -> Self {
        Self::new_with_severity(MessageBoxSeverity::None, title)
    }

    #[doc(hidden)]
    pub fn information_literal(title: impl Into<String>) -> Self {
        Self::information(LocalizedString::literal(title))
    }
    #[doc(hidden)]
    pub fn warning_literal(title: impl Into<String>) -> Self {
        Self::warning(LocalizedString::literal(title))
    }
    #[doc(hidden)]
    pub fn critical_literal(title: impl Into<String>) -> Self {
        Self::critical(LocalizedString::literal(title))
    }
    #[doc(hidden)]
    pub fn question_literal(title: impl Into<String>) -> Self {
        Self::question(LocalizedString::literal(title))
    }
    #[doc(hidden)]
    pub fn plain_literal(title: impl Into<String>) -> Self {
        Self::plain(LocalizedString::literal(title))
    }

    /// Primary message line, rendered in `typography.body` with
    /// `text_primary`. Prefer a short, self-contained sentence —
    /// details belong in `informative_text`.
    pub fn text(mut self, text: impl Into<LocalizedString>) -> Self {
        self.text = Some(text.into().resolve_now());
        self
    }
    #[doc(hidden)]
    pub fn text_literal(self, text: impl Into<String>) -> Self {
        self.text(LocalizedString::literal(text))
    }

    /// Secondary, explanatory text rendered below the primary text in
    /// `typography.body` with `text_secondary`. Matches Qt's
    /// `setInformativeText`.
    pub fn informative_text(mut self, text: impl Into<LocalizedString>) -> Self {
        self.informative_text = Some(text.into().resolve_now());
        self
    }
    #[doc(hidden)]
    pub fn informative_text_literal(self, text: impl Into<String>) -> Self {
        self.informative_text(LocalizedString::literal(text))
    }

    /// Detailed text hidden behind a "Show details" [`Accordion`] —
    /// for technical diagnostics (stack traces, error codes). Matches
    /// Qt's `setDetailedText`.
    pub fn detailed_text(mut self, text: impl Into<LocalizedString>) -> Self {
        self.detailed_text = Some(text.into().resolve_now());
        self
    }
    #[doc(hidden)]
    pub fn detailed_text_literal(self, text: impl Into<String>) -> Self {
        self.detailed_text(LocalizedString::literal(text))
    }

    /// Apply a preset button bundle. Implicitly sets default and
    /// escape buttons for the preset (both can be overridden via
    /// [`MessageBox::default_button`] and
    /// [`MessageBox::escape_button`]).
    pub fn buttons(mut self, preset: MessageBoxButtons) -> Self {
        if self.default_button.is_none() {
            self.default_button = preset.preset_default();
        }
        if self.escape_button.is_none() {
            self.escape_button = preset.preset_escape();
        }
        self.buttons_config = Some(preset);
        self
    }

    /// Append a single button. Use to augment a preset (rare) or to
    /// build a bespoke button row without going through
    /// [`MessageBoxButtons::Custom`].
    pub fn add_button(mut self, button: impl Into<MessageBoxButton>) -> Self {
        self.extra_buttons.push(button.into());
        self
    }

    /// Mark which button activates on Enter and receives initial
    /// focus. Must refer to one of the buttons configured via
    /// `buttons` / `add_button`.
    pub fn default_button(mut self, which: StandardButton) -> Self {
        self.default_button = Some(which);
        self
    }

    /// Mark which button activates on Escape (and scrim-click, when
    /// allowed). Must refer to one of the configured buttons.
    pub fn escape_button(mut self, which: StandardButton) -> Self {
        self.escape_button = Some(which);
        self
    }

    /// Attach a "Don't show again"-style checkbox below the body.
    /// Internally creates a `Signal<bool>` initialized to `false` and
    /// reports its state in [`MessageBoxResult::checkbox_checked`].
    /// For external observation, use
    /// [`MessageBox::show_again_checkbox_state`] instead.
    pub fn show_again_checkbox(mut self, label: impl Into<LocalizedString>) -> Self {
        self.show_again_label = Some(label.into().resolve_now());
        self
    }
    #[doc(hidden)]
    pub fn show_again_checkbox_literal(self, label: impl Into<String>) -> Self {
        self.show_again_checkbox(LocalizedString::literal(label))
    }

    /// Like [`MessageBox::show_again_checkbox`], but with a
    /// caller-owned `Signal<bool>` so the checkbox state survives the
    /// dialog lifetime (useful for "remember my choice" persistence).
    pub fn show_again_checkbox_state(mut self, signal: Signal<bool>) -> Self {
        self.show_again_state = Some(signal);
        self
    }

    /// Register the result callback, invoked exactly once when a
    /// button fires (either by click or by Enter/Escape shortcut).
    pub fn on_result(
        mut self,
        f: impl Fn(MessageBoxResult, &mut EventContext) + 'static,
    ) -> Self {
        self.on_result = Some(Box::new(f));
        self
    }

    /// Present the MessageBox as a modal on top of `ctx`'s current
    /// tree. Consumes `self`; callers who need to present multiple
    /// dialogs with shared config should build a factory closure.
    pub fn present(self, ctx: &mut EventContext) {
        let title = self.title.clone();
        let close_behavior = if self.severity == MessageBoxSeverity::Critical {
            ModalCloseBehavior::EscapeKey
        } else {
            ModalCloseBehavior::EscapeOrClickOutside
        };

        let mut inner = Some(self);
        ctx.present_modal(
            ModalRequest::deferred(move |tree| {
                let mb = inner.take().expect("MessageBox present closure called twice");
                tree.add(ModalContainer::new(mb))
            })
            .presentation(ModalPresentation::Auto)
            .close_behavior(close_behavior)
            .title(title),
        );
    }

    fn resolve_buttons(&mut self) -> Vec<MessageBoxButton> {
        let mut resolved = self
            .buttons_config
            .clone()
            .map(|b| b.into_buttons())
            .unwrap_or_default();
        resolved.extend(self.extra_buttons.iter().cloned());
        if resolved.is_empty() {
            resolved.push(StandardButton::Ok.into());
            if self.default_button.is_none() {
                self.default_button = Some(StandardButton::Ok);
            }
            if self.escape_button.is_none() {
                self.escape_button = Some(StandardButton::Ok);
            }
        }
        resolved
    }
}

impl Widget for MessageBox {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();

        let checkbox_signal = self
            .show_again_state
            .clone()
            .unwrap_or_else(|| ctx.signal(false));
        let state = State::new(checkbox_signal.clone());
        *state.on_result.borrow_mut() = self.on_result.take();

        let buttons = self.resolve_buttons();
        *state.buttons.borrow_mut() = buttons.iter().map(|b| b.kind).collect();
        state.default_button.set(self.default_button);
        state.escape_button.set(self.escape_button);

        let mut header_text_stack = VStack::new().spacing(6.0);
        header_text_stack = header_text_stack.child(
            TextWidget::new_literal(self.title.clone())
                .style(theme.typography.body_bold.clone())
                .color(theme.colors.text_primary),
        );
        if let Some(text) = self.text.clone() {
            header_text_stack = header_text_stack.child(
                TextWidget::new_literal(text)
                    .style(theme.typography.body.clone())
                    .color(theme.colors.text_primary),
            );
        }
        if let Some(info) = self.informative_text.clone() {
            header_text_stack = header_text_stack.child(
                TextWidget::new_literal(info)
                    .style(theme.typography.body.clone())
                    .color(theme.colors.text_secondary),
            );
        }

        let header: Box<dyn Widget> = if let Some(icon_path) = severity_icon(self.severity) {
            Box::new(
                HStack::new()
                    .spacing(16.0)
                    .alignment(VAlignment::Top)
                    .child(
                        IconWidget::from_path(icon_path, SEVERITY_ICON_SIZE)
                            .icon_size(SEVERITY_ICON_SIZE)
                            .mode(IconMode::Tintable)
                            .color(severity_color(&theme, self.severity)),
                    )
                    .child(header_text_stack),
            )
        } else {
            Box::new(header_text_stack)
        };

        let detailed_child: Option<Box<dyn Widget>> = self.detailed_text.clone().map(|text| {
            let expanded = ctx.signal(false);
            let label: LocalizedString = fern_i18n::tr_widget!(messagebox_show_details()).into();
            let body = TextWidget::new_literal(text)
                .style(theme.typography.small.clone())
                .color(theme.colors.text_secondary);
            let accordion: Box<dyn Widget> =
                Box::new(Accordion::new_literal(label.resolve_now(), expanded).content(body));
            accordion
        });

        let checkbox_child: Option<Box<dyn Widget>> = self.show_again_label.clone().map(|label| {
            let cb: Box<dyn Widget> =
                Box::new(Checkbox::new(checkbox_signal.clone()).label_literal(label));
            cb
        });

        let mut footer = HStack::new().spacing(8.0).child(Spacer::new());
        for button_cfg in &buttons {
            let kind = button_cfg.kind;
            let label = button_cfg.resolved_label();
            let variant = if Some(kind) == self.default_button {
                ButtonVariant::Default
            } else {
                ButtonVariant::Regular
            };
            let state_for_btn = state.clone();
            let btn_id = ctx.add(
                Button::new(label)
                    .style(variant)
                    .on_activate_fn(move |ctx| {
                        state_for_btn.fire(kind, false, ctx);
                    }),
            );
            if Some(kind) == self.default_button {
                self.default_button_id.set(Some(btn_id));
            }
            footer = footer.add_child(btn_id);
        }

        let mut stack = VStack::new().spacing(16.0);
        stack = stack.add_child(ctx.add_boxed(header));
        if let Some(det) = detailed_child {
            stack = stack.add_child(ctx.add_boxed(det));
        }
        if let Some(cb) = checkbox_child {
            stack = stack.add_child(ctx.add_boxed(cb));
        }
        let footer_id = ctx.add(footer);
        stack = stack.add_child(footer_id);

        let root = ctx.add(stack);
        self.root_child_id = Some(root);

        {
            let state_enter = state.clone();
            ctx.register_action(
                Action::new(DEFAULT_INTENT_NAME).on_invoke(move |_intent, ctx| {
                    if let Some(kind) = state_enter.default_button.get() {
                        state_enter.fire(kind, false, ctx);
                    }
                }),
            );
            ctx.register_shortcut(
                Shortcut::new(DEFAULT_INTENT_NAME)
                    .primary(KeyStroke::new(Key::Enter, Modifiers::NONE))
                    .build(),
            );
        }
        {
            let state_escape = state.clone();
            ctx.register_action(
                Action::new(ESCAPE_INTENT_NAME).on_invoke(move |_intent, ctx| {
                    if let Some(kind) = state_escape.resolve_escape_button() {
                        state_escape.fire(kind, true, ctx);
                    } else {
                        ctx.dismiss_modal();
                    }
                }),
            );
            ctx.register_shortcut(
                Shortcut::new(ESCAPE_INTENT_NAME)
                    .primary(KeyStroke::new(Key::Escape, Modifiers::NONE))
                    .build(),
            );
        }

        self.state = Some(state);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(280.0, 160.0))
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

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::AlertDialog);
        builder.set_name(self.title.clone());
        if let Some(description) = self.accessible_description() {
            builder.set_description(description);
        }
        builder.set_modal();
        builder.set_live(fern_core::accesskit::Live::Assertive);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn accessible_title_hint(&self) -> Option<String> {
        Some(self.title.clone())
    }

    fn initial_focus_hint(&self) -> Option<WidgetId> {
        self.default_button_id.get()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

impl MessageBox {
    fn accessible_description(&self) -> Option<String> {
        match (self.text.as_deref(), self.informative_text.as_deref()) {
            (None, None) => None,
            (Some(t), None) => Some(t.to_string()),
            (None, Some(i)) => Some(i.to_string()),
            (Some(t), Some(i)) => Some(format!("{t}\n{i}")),
        }
    }
}

/// Extension trait on [`EventContext`] for ergonomic MessageBox
/// presentation. Mirrors `ctx.present_modal(...)` for the general
/// case.
pub trait EventContextMessageBoxExt {
    /// Present `mb` as a modal. Equivalent to `mb.present(self)`.
    fn present_message_box(&mut self, mb: MessageBox);
}

impl EventContextMessageBoxExt for EventContext<'_> {
    fn present_message_box(&mut self, mb: MessageBox) {
        mb.present(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::WidgetEvent;
    use fern_core::ModalContent;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    /// Mirrors the focus resolution `fern_app::present_in_tree_modal_request`
    /// applies after the modal content subtree is built. Reproduced here
    /// because `fern-widgets` can't depend on `fern-app`.
    fn present_and_lay_out(tree: &mut WidgetTree, mb: MessageBox) -> WidgetId {
        use crate::button::Button as Btn;
        let mb_cell: Rc<RefCell<Option<MessageBox>>> = Rc::new(RefCell::new(Some(mb)));
        let mb_for_closure = mb_cell.clone();
        let trigger = tree.add(Btn::new_literal("Open").on_activate_fn(move |ctx| {
            if let Some(mb) = mb_for_closure.borrow_mut().take() {
                mb.present(ctx);
            }
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        let content_id = match request.content {
            ModalContent::Deferred(builder) => builder(tree),
            ModalContent::ExistingWidget(_) => panic!("MessageBox must use deferred content"),
        };
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let focus_target = request
            .focus_target
            .filter(|id| tree.is_active(*id) && tree.is_descendant_of(*id, content_id))
            .or_else(|| tree.widget_initial_focus_hint(content_id))
            .or_else(|| tree.first_focusable_descendant(content_id));
        if let Some(id) = focus_target {
            tree.focus(id);
        }
        content_id
    }

    #[test]
    fn present_queues_modal_request() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let mb = MessageBox::information_literal("t")
            .text_literal("x")
            .buttons(MessageBoxButtons::Ok);
        let _content = present_and_lay_out(&mut tree, mb);
        assert!(tree.find_by_label("t").is_some());
    }

    #[test]
    fn critical_uses_escape_only_close_behavior() {
        use crate::button::Button as Btn;
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let mb_cell: Rc<RefCell<Option<MessageBox>>> = Rc::new(RefCell::new(Some(
            MessageBox::critical_literal("Fatal").text_literal("Boom").buttons(MessageBoxButtons::Ok),
        )));
        let mb_for_closure = mb_cell.clone();
        let trigger = tree.add(Btn::new_literal("Open").on_activate_fn(move |ctx| {
            if let Some(mb) = mb_for_closure.borrow_mut().take() {
                mb.present(ctx);
            }
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        assert_eq!(request.close_behavior, ModalCloseBehavior::EscapeKey);
    }

    #[test]
    fn alert_dialog_role_exposed() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let mb = MessageBox::warning_literal("Title")
            .text_literal("Body")
            .buttons(MessageBoxButtons::Ok);
        let content = present_and_lay_out(&mut tree, mb);
        let mb_id = tree.children(content).first().copied().unwrap();
        let info = tree.accessibility_node(mb_id);
        assert_eq!(info.role(), fern_core::accesskit::Role::AlertDialog);
        assert_eq!(info.name(), Some("Title"));
    }

    #[test]
    fn ok_button_fires_result_with_correct_kind() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let captured: Rc<RefCell<Option<MessageBoxResult>>> = Rc::new(RefCell::new(None));
        let captured_for_handler = captured.clone();
        let mb = MessageBox::information_literal("t")
            .text_literal("x")
            .buttons(MessageBoxButtons::Ok)
            .on_result(move |r, _ctx| {
                *captured_for_handler.borrow_mut() = Some(r);
            });
        let _content = present_and_lay_out(&mut tree, mb);
        let ok_id = tree
            .find_by_label(&StandardButton::Ok.default_label().resolve_now())
            .unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(ok_id),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        let result = captured.borrow().expect("result must be captured");
        assert_eq!(result.button, StandardButton::Ok);
        assert!(!result.checkbox_checked);
        assert!(!result.dismissed_by_escape);
    }

    #[test]
    fn default_button_is_focused_on_open() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let mb = MessageBox::question_literal("t")
            .text_literal("x")
            .buttons(MessageBoxButtons::YesNoCancel)
            .default_button(StandardButton::No);
        let _content = present_and_lay_out(&mut tree, mb);
        let no_id = tree
            .find_by_label(&StandardButton::No.default_label().resolve_now())
            .unwrap();
        assert_eq!(tree.focused(), Some(no_id));
    }

    #[test]
    fn enter_fires_default_button_from_any_focus() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let captured: Rc<RefCell<Option<MessageBoxResult>>> = Rc::new(RefCell::new(None));
        let captured_for_handler = captured.clone();
        let mb = MessageBox::question_literal("t")
            .text_literal("x")
            .buttons(MessageBoxButtons::OkCancel)
            .on_result(move |r, _ctx| {
                *captured_for_handler.borrow_mut() = Some(r);
            });
        let _content = present_and_lay_out(&mut tree, mb);
        let cancel_id = tree
            .find_by_label(&StandardButton::Cancel.default_label().resolve_now())
            .unwrap();
        tree.focus(cancel_id);
        tree.press_key(Key::Enter, Modifiers::NONE);
        let result = captured.borrow().expect("result must be captured");
        assert_eq!(result.button, StandardButton::Ok);
        assert!(!result.dismissed_by_escape);
    }

    #[test]
    fn escape_fires_escape_button_and_marks_dismissed() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let captured: Rc<RefCell<Option<MessageBoxResult>>> = Rc::new(RefCell::new(None));
        let captured_for_handler = captured.clone();
        let mb = MessageBox::question_literal("t")
            .text_literal("x")
            .buttons(MessageBoxButtons::YesNoCancel)
            .on_result(move |r, _ctx| {
                *captured_for_handler.borrow_mut() = Some(r);
            });
        let _content = present_and_lay_out(&mut tree, mb);
        tree.press_key(Key::Escape, Modifiers::NONE);
        let result = captured.borrow().expect("result must be captured");
        assert_eq!(result.button, StandardButton::Cancel);
        assert!(result.dismissed_by_escape);
    }

    #[test]
    fn checkbox_state_reported_in_result() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let shared_state = Signal::new(false);
        let captured: Rc<RefCell<Option<MessageBoxResult>>> = Rc::new(RefCell::new(None));
        let captured_for_handler = captured.clone();
        let mb = MessageBox::information_literal("t")
            .text_literal("x")
            .buttons(MessageBoxButtons::Ok)
            .show_again_checkbox_state(shared_state.clone())
            .show_again_checkbox_literal("Don't show again")
            .on_result(move |r, _ctx| {
                *captured_for_handler.borrow_mut() = Some(r);
            });
        let _content = present_and_lay_out(&mut tree, mb);
        shared_state.set(true);
        let ok_id = tree
            .find_by_label(&StandardButton::Ok.default_label().resolve_now())
            .unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(ok_id),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        assert!(captured.borrow().unwrap().checkbox_checked);
    }

    #[test]
    fn accessible_title_hint_propagates_to_container() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let mb = MessageBox::information_literal("Title propagation test")
            .text_literal("Body")
            .buttons(MessageBoxButtons::Ok);
        let content = present_and_lay_out(&mut tree, mb);
        let info = tree.accessibility_node(content);
        assert_eq!(info.role(), fern_core::accesskit::Role::Dialog);
        assert_eq!(info.name(), Some("Title propagation test"));
    }

    #[test]
    fn standard_button_roles_classify_correctly() {
        assert_eq!(StandardButton::Ok.role(), ButtonRole::Accept);
        assert_eq!(StandardButton::Yes.role(), ButtonRole::Accept);
        assert_eq!(StandardButton::Save.role(), ButtonRole::Accept);
        assert_eq!(StandardButton::Cancel.role(), ButtonRole::Reject);
        assert_eq!(StandardButton::No.role(), ButtonRole::Reject);
        assert_eq!(StandardButton::Abort.role(), ButtonRole::Reject);
        assert_eq!(StandardButton::Discard.role(), ButtonRole::Destructive);
        assert_eq!(StandardButton::Help.role(), ButtonRole::Action);
        assert_eq!(StandardButton::Ignore.role(), ButtonRole::Action);
    }

    #[test]
    fn escape_resolution_prefers_explicit_escape_button() {
        let state = State::new(Signal::new(false));
        *state.buttons.borrow_mut() = vec![StandardButton::Save, StandardButton::Discard];
        state.escape_button.set(Some(StandardButton::Discard));
        assert_eq!(state.resolve_escape_button(), Some(StandardButton::Discard));
    }

    #[test]
    fn escape_resolution_falls_back_to_first_reject() {
        let state = State::new(Signal::new(false));
        *state.buttons.borrow_mut() = vec![
            StandardButton::Retry,
            StandardButton::Ignore,
            StandardButton::Abort,
        ];
        state.escape_button.set(None);
        assert_eq!(state.resolve_escape_button(), Some(StandardButton::Abort));
    }

    #[test]
    fn escape_resolution_falls_back_to_last_when_no_reject() {
        let state = State::new(Signal::new(false));
        *state.buttons.borrow_mut() = vec![StandardButton::Ok, StandardButton::Help];
        state.escape_button.set(None);
        assert_eq!(state.resolve_escape_button(), Some(StandardButton::Help));
    }
}
