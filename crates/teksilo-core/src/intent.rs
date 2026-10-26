// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Runtime intents dispatched by shortcuts and programmatic callers.
//!
//! An [`Intent`] is the unit of "something wants to happen" in the
//! action system. It pairs a stable name (the intent string) with an
//! optional type-erased payload that handlers downcast when they
//! recognize the intent. Intents are produced by
//! [`Shortcut`](crate::shortcut::Shortcut)s at activation time, by
//! widgets via `ctx.send_intent`, or by programmatic callers.
//!
//! They dispatch through the widget tree by walking
//! **source-widget → root**: each ancestor's
//! [`Action`](crate::action::Action) whose `intent` name matches
//! gets a chance to consume the intent or propagate it.
//!
//! ## Typed DTOs via [`IntentKind`]
//!
//! Apps that want typo-safe construction and handler-side
//! exhaustiveness define an enum and implement [`IntentKind`] (by
//! hand or via `#[derive(IntentKind)]` from `teksilo-macros`). The
//! whole variant — including any fields it carries — is stored as
//! the intent's payload; handlers recover it via
//! [`Intent::payload`] or [`IntentKind::from_intent`].

use std::any::Any;
use std::rc::Rc;

/// A runtime intent dispatched through the widget tree.
///
/// The `name` is the stable dispatch key (matched against
/// [`Action::intent`](crate::action::Action)). The optional
/// `payload` carries any type the sender wants to attach — recover
/// it with [`Intent::payload::<T>`] when the handler knows the
/// expected type (typically via `IntentKind::from_intent`).
///
/// The `source` field records where the intent originated — set by
/// the framework's standard activation paths (button taps, menu
/// selects, shortcut chords, gesture recognizers) so analytics can
/// answer "which surface drives this intent?". See
/// [`crate::telemetry::IntentSource`].
pub struct Intent {
    /// Stable intent name. Usually matches the originating
    /// [`Shortcut`](crate::shortcut::Shortcut)'s `intent_name()`.
    pub name: &'static str,
    /// Origin of the intent. The framework's activation paths
    /// (button, menu, shortcut, gesture) set this; programmatic
    /// callers default to `Programmatic` via [`Intent::new`].
    /// Read by the dispatch-tap to fill the `source` prop on
    /// `intent.dispatched` events.
    pub source: crate::telemetry::IntentSource,
    payload: Option<Rc<dyn Any>>,
    /// The window whose tree it was sent in, noted by that tree.
    origin_window: Option<crate::window::TeksiloWindowId>,
    /// The activation token the event it was sent for carried, noted when
    /// its tree keeps it for another; see
    /// [`activation_token`](Self::activation_token).
    activation_token: Option<String>,
    /// How many times it, or the routed intents whose actions sent it, have
    /// crossed from one tree to another.
    crossings: usize,
    /// Whether an open modal overlay keeps it, as it keeps a key's; see
    /// [`kept_in_modal`](Self::kept_in_modal).
    kept_in_modal: bool,
}

impl Intent {
    /// A parameter-less intent. Defaults to
    /// `IntentSource::Programmatic`; framework activation paths
    /// override via [`Intent::with_source`] before dispatching.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            source: crate::telemetry::IntentSource::Programmatic,
            payload: None,
            origin_window: None,
            activation_token: None,
            crossings: 0,
            kept_in_modal: false,
        }
    }

    /// An intent carrying a typed payload. The payload is stored
    /// type-erased in an `Rc<dyn Any>`; recover it with
    /// [`Intent::payload`].
    pub fn with_payload<T: 'static>(name: &'static str, payload: T) -> Self {
        Self {
            name,
            source: crate::telemetry::IntentSource::Programmatic,
            payload: Some(Rc::new(payload)),
            origin_window: None,
            activation_token: None,
            crossings: 0,
            kept_in_modal: false,
        }
    }

    /// Tag the intent with its origin. Called by the shortcut
    /// activation path right before dispatch; the other framework
    /// activation wrappers (button, menu, gesture, AT) tag through
    /// the dispatcher's `EventContext::current_source` instead. App
    /// code typically doesn't call this directly — use
    /// `EventContext::send_intent` from inside the right handler and
    /// the source is set automatically.
    ///
    /// (Note: `EventContext::send_intent` infers the source from
    /// the handler context where possible; this method is the
    /// escape hatch for callers that need to override.)
    pub fn with_source(mut self, source: crate::telemetry::IntentSource) -> Self {
        self.source = source;
        self
    }

    /// Mark the intent as input the user gave the window, as a key is: an
    /// open modal overlay in its tree (a dialog presented in the window, the
    /// command palette) keeps it, as it keeps a shortcut's intent. Only the
    /// actions inside the modal, and the window-global actions registered
    /// inside it, are offered it; it never walks on to the page behind. What
    /// the window leaves still goes to the application tree. `teksilo-app`
    /// marks the intent of a row chosen in the native menu bar, which AppKit
    /// disables in a document window while a modal panel is up.
    #[doc(hidden)]
    pub fn kept_in_modal(mut self) -> Self {
        self.kept_in_modal = true;
        self
    }

    /// Whether an open modal overlay keeps it: a shortcut's, or one marked
    /// [`kept_in_modal`](Self::kept_in_modal).
    pub(crate) fn stays_in_modal(&self) -> bool {
        self.kept_in_modal || self.source == crate::telemetry::IntentSource::Shortcut
    }

    /// Borrow the payload as `&T`, or `None` if the intent has no
    /// payload or the payload's concrete type doesn't match.
    pub fn payload<T: 'static>(&self) -> Option<&T> {
        let any: &dyn Any = &**self.payload.as_ref()?;
        any.downcast_ref::<T>()
    }

    /// Whether this intent carries any payload (typed or not).
    pub fn has_payload(&self) -> bool {
        self.payload.is_some()
    }

    /// The window it was sent in: the window whose tree dispatched it
    /// first. `None` for an intent sent in the application tree, or in a
    /// tree with no window, and before any tree has dispatched it.
    ///
    /// An intent a window leaves unclaimed keeps it as it crosses, so an
    /// action of the application tree knows which window a command came
    /// from: the one a menu row or a shortcut was used in, the window with
    /// the OS focus for a macOS menu bar item. An intent re-sent from its
    /// action keeps the window it was first sent in.
    #[doc(hidden)]
    pub fn origin_window(&self) -> Option<crate::window::TeksiloWindowId> {
        self.origin_window
    }

    /// Note `window` as the window it was sent in, unless one is noted.
    pub(crate) fn note_origin_window(&mut self, window: Option<crate::window::TeksiloWindowId>) {
        if self.origin_window.is_none() {
            self.origin_window = window;
        }
    }

    /// The activation token it carries across to another tree: the token
    /// of the event it was sent for (see `WithActivationToken` in
    /// `teksilo-app`), noted when its tree kept it unclaimed while that
    /// token was still unused. The tree it is
    /// [routed](crate::WidgetTree::dispatch_routed_intent) to holds it for
    /// the first window its actions open or raise, as the first tree would
    /// have.
    #[doc(hidden)]
    pub fn activation_token(&self) -> Option<&str> {
        self.activation_token.as_deref()
    }

    /// Take the activation token it carries, leaving none.
    pub(crate) fn take_activation_token(&mut self) -> Option<String> {
        self.activation_token.take()
    }

    /// How many crossings between trees led to it: those it made, and
    /// those of the routed intent whose action sent it. See
    /// [`WidgetTree::dispatch_routed_intent`](crate::WidgetTree::dispatch_routed_intent).
    pub(crate) fn crossings(&self) -> usize {
        self.crossings
    }

    /// Note that `crossings` crossings led to it, unless more did.
    pub(crate) fn note_crossings(&mut self, crossings: usize) {
        self.crossings = self.crossings.max(crossings);
    }

    /// Note `token` as the one it carries, unless it carries one.
    pub(crate) fn note_activation_token(&mut self, token: Option<String>) {
        if self.activation_token.is_none() {
            self.activation_token = token;
        }
    }
}

impl Clone for Intent {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            source: self.source,
            // Rc<dyn Any> is cheaply clonable — bumps the refcount.
            payload: self.payload.clone(),
            origin_window: self.origin_window,
            activation_token: self.activation_token.clone(),
            crossings: self.crossings,
            kept_in_modal: self.kept_in_modal,
        }
    }
}

impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Intent")
            .field("name", &self.name)
            .field("source", &self.source)
            .field("has_payload", &self.payload.is_some())
            .finish()
    }
}

/// Typed DTO bridge between an app's intent enum and the runtime
/// [`Intent`] dispatch type.
///
/// Apps that want compile-time guarantees — typo-safe intent
/// construction, exhaustive matches on recognized intents, a single
/// source of truth for intent names — define an enum and implement
/// this trait (by hand or via `#[derive(IntentKind)]` from
/// `teksilo-macros`).
///
/// ```ignore
/// #[derive(Debug, IntentKind)]
/// enum AppIntent {
///     #[name = "app.save"]        Save,
///     #[name = "app.open"]        Open(PathBuf),
///     #[name = "app.add_item"]    AddItem { id: i64, dto: CreateItemDto },
/// }
///
/// // Send (typo-safe at the enum variant — blanket From<K> for Intent
/// // means no explicit .into_intent() call is needed):
/// ctx.send_intent(AppIntent::Save);
/// ctx.send_intent(AppIntent::Open(path));
///
/// // Handle (exhaustive match, recovers the full variant):
/// Action::new("app.open").on_invoke(|intent, ctx| {
///     if let Some(AppIntent::Open(path)) = AppIntent::from_intent(intent) {
///         open_file(path, ctx);
///     }
/// })
/// ```
///
/// The variant itself — including any fields — is stored as the
/// intent's payload, so any `'static` variant works. Struct
/// variants (`AddItem { .. }`), tuple variants (`Open(PathBuf)`),
/// and unit variants (`Save`) are all supported without restriction.
///
/// `from_intent` returns a reference (`Option<&Self>`), so recovery
/// does not require `Self: Clone`. If an owned variant is needed and
/// the enum derives `Clone`, call `.cloned()` on the result.
pub trait IntentKind: Sized + 'static {
    /// Consume the variant and build the runtime [`Intent`] it
    /// corresponds to. The variant — and any data it carries — is
    /// stored as the intent's type-erased payload.
    fn into_intent(self) -> Intent;

    /// Recognise a runtime intent as one of this enum's variants and
    /// borrow its payload. Returns `None` for foreign intents (names
    /// this enum doesn't cover) and for intents whose payload is
    /// missing or of a different concrete type.
    fn from_intent(intent: &Intent) -> Option<&Self>;
}

/// Blanket conversion from any [`IntentKind`] into a runtime [`Intent`].
///
/// Lets call sites drop the explicit `.into_intent()` hop where an
/// `Into<Intent>` bound is available (for example,
/// [`EventContext::send_intent`](crate::widget::EventContext::send_intent)
/// and [`ShortcutBuilder::on_activate`](crate::shortcut::ShortcutBuilder::on_activate)).
impl<K: IntentKind> From<K> for Intent {
    fn from(kind: K) -> Self {
        kind.into_intent()
    }
}

/// Return value of an [`Action`](crate::action::Action) handler. Controls
/// whether the intent keeps bubbling up to ancestor widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IntentResponse {
    /// Intent was consumed; stop walking up the focus chain.
    #[default]
    Handled,
    /// Intent was observed but not consumed; continue walking up so
    /// ancestor widgets can also react.
    Propagated,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_without_payload() {
        let i = Intent::new("app.save");
        assert_eq!(i.name, "app.save");
        assert!(!i.has_payload());
        assert_eq!(i.payload::<i32>(), None);
    }

    #[test]
    fn typed_payload_round_trip() {
        let i = Intent::with_payload("app.scroll_by", 42_i64);
        assert!(i.has_payload());
        assert_eq!(i.payload::<i64>(), Some(&42_i64));
        // Wrong type: None.
        assert_eq!(i.payload::<String>(), None);
    }

    #[test]
    fn payload_carries_complex_types() {
        #[derive(Debug, PartialEq)]
        struct Dto {
            id: i64,
            name: String,
        }
        let i = Intent::with_payload(
            "app.add_item",
            Dto {
                id: 7,
                name: "Foo".into(),
            },
        );
        assert_eq!(
            i.payload::<Dto>(),
            Some(&Dto {
                id: 7,
                name: "Foo".into(),
            })
        );
    }

    #[test]
    fn default_response_is_handled() {
        assert_eq!(IntentResponse::default(), IntentResponse::Handled);
    }
}
