// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! User-facing rebindable keyboard shortcuts.
//!
//! The shortcut system has three layers:
//!
//! - [`KeyStroke`] — a single keyboard chord (key + modifiers).
//! - [`Shortcut`] — a first-class, rebindable record with a stable
//!   string id, localizable metadata, one or two keystrokes, a scope,
//!   and an `on_activate` closure that produces an
//!   [`Intent`] at activation time.
//! - [`ShortcutRegistry`] — a two-layer store: widget-declared
//!   defaults (refreshed every build) plus persisted user overrides.
//!   The effective view merges them.
//!
//! Dispatch: a keystroke is looked up in the registry, the matching
//! shortcut's `on_activate` produces an intent, and the framework walks
//! **source-widget → root** invoking [`Action`](crate::action::Action)
//! handlers along the way. When several bindings share a chord, the one
//! whose scope covers the current focus is selected first, so a `Scoped`
//! binding outside the focused subtree never shadows an applicable
//! `Global` one.

use crate::event::{Key, Modifiers};
use crate::intent::Intent;
use crate::signal::{Prop, Signal};
use crate::widget::EventContext;
use crate::widget_id::WidgetId;
use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// KeyStroke
// ---------------------------------------------------------------------------

/// A single keyboard chord: a key plus its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct KeyStroke {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl KeyStroke {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    /// A chord on `Ctrl`.
    ///
    /// Read as a *declared* shortcut default this carries the cross-platform
    /// primary-accelerator convention: see
    /// [`with_command_convention`](Self::with_command_convention) for what the
    /// registry does with it on macOS. Use [`command`](Self::command) when you
    /// want that intent stated outright, and
    /// [`new`](Self::new) with [`Modifiers::CTRL`] plus
    /// [`ShortcutBuilder::literal_modifiers`] when you mean physical Control on
    /// every platform.
    pub fn ctrl(key: Key) -> Self {
        Self::new(key, Modifiers::CTRL)
    }

    pub fn ctrl_shift(key: Key) -> Self {
        Self::new(key, Modifiers::CTRL | Modifiers::SHIFT)
    }

    /// A chord on the platform's **primary accelerator**: ⌘S on macOS, Ctrl+S
    /// on Windows and Linux. See [`Modifiers::COMMAND`].
    ///
    /// Equivalent to [`ctrl`](Self::ctrl) once a declared shortcut has been
    /// resolved, but it says so at the call site — which matters for the chords
    /// built outside the registry, like the copy/paste labels a context menu
    /// renders for itself.
    pub fn command(key: Key) -> Self {
        Self::new(key, Modifiers::COMMAND)
    }

    /// A chord on the platform's primary accelerator plus `Shift`: ⇧⌘Z on
    /// macOS, Ctrl+Shift+Z on Windows and Linux.
    pub fn command_shift(key: Key) -> Self {
        Self::new(key, Modifiers::COMMAND | Modifiers::SHIFT)
    }

    pub fn alt(key: Key) -> Self {
        Self::new(key, Modifiers::ALT)
    }

    /// This chord with a declared `Ctrl` reinterpreted as the platform's
    /// primary accelerator ([`Modifiers::COMMAND`]) — ⌘ on macOS, unchanged
    /// everywhere else.
    ///
    /// This is the convention Qt spells `Qt::CTRL` and the one Teksilo's native
    /// menu bar has always applied when turning a declared chord into an
    /// `NSMenuItem` key equivalent. [`ShortcutRegistry`] applies it to every
    /// **declared** default, so an app that writes `KeyStroke::ctrl(Key::F)`
    /// once gets Ctrl+F on Windows and Linux and ⌘F on macOS — the same chord
    /// the menu row already advertised.
    ///
    /// It is deliberately *not* applied to a **user override**: a chord the
    /// user captured in a settings UI is a literal statement of intent, and
    /// rewriting it would make physical ⌃F unbindable on macOS.
    ///
    /// Idempotent, and a no-op for a chord that already names `Super`, so
    /// `Ctrl+Super` survives as the genuine ⌃⌘ two-modifier chord.
    pub fn with_command_convention(self) -> Self {
        self.with_command_convention_using(Modifiers::COMMAND)
    }

    /// The accelerator-parameterised core of
    /// [`with_command_convention`](Self::with_command_convention) — pass
    /// [`Modifiers::SUPER`] to ask how macOS reads the chord, [`Modifiers::CTRL`]
    /// for Windows and Linux.
    ///
    /// Split out for the same reason as
    /// [`Modifiers::with_command_convention_using`]: the convention's whole
    /// purpose is behaviour that differs by platform, so both branches have to
    /// stay reachable from one host's test run.
    pub(crate) fn with_command_convention_using(self, command: Modifiers) -> Self {
        Self::new(
            self.key,
            self.modifiers.with_command_convention_using(command),
        )
    }
}

impl fmt::Display for KeyStroke {
    // Plain "Ctrl+S" form ("Cmd+S" for a Super chord on macOS, where the key
    // is named Command). Widgets that display shortcuts to users should
    // use `teksilo_widgets::keystroke_format::format_keystroke()` instead,
    // which handles platform-specific symbols (⌘ on macOS) and locale-
    // aware modifier names ("Strg" in German) via teksilo-i18n.
    // See architecture §11.2.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.modifiers, self.key)
    }
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

/// Whether a shortcut fires regardless of focus position or only when
/// focus is inside a specific widget's subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutScope {
    /// Reachable regardless of focus. Used by app-level shortcuts.
    Global,
    /// Reachable only when focus is inside this widget (or one of its
    /// descendants). Used by widget-declared shortcuts — the widget
    /// that calls `register_shortcut` scopes to itself by default but
    /// may scope to any [`WidgetId`] it knows (child, sibling, etc.).
    Scoped(WidgetId),
}

/// Whether two shortcut scopes can be active at the same time — the
/// basis for [`ShortcutRegistry::find_conflict`]. A `Global` shortcut is
/// active everywhere, so it can collide with anything; two `Scoped`
/// shortcuts collide only when scoped to the same widget. (Distinct
/// `Scoped` ids whose subtrees happen to nest are treated as disjoint —
/// the registry has no tree to prove containment.)
fn scopes_can_collide(a: ShortcutScope, b: ShortcutScope) -> bool {
    match (a, b) {
        (ShortcutScope::Scoped(x), ShortcutScope::Scoped(y)) => x == y,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Shortcut
// ---------------------------------------------------------------------------

/// Closure signature for a shortcut's activation handler.
///
/// Receives the matched [`KeyStroke`] (so the closure can branch on
/// which chord fired, e.g. Ctrl+1 vs Ctrl+2) and a mutable
/// [`EventContext`]. Returns the [`Intent`] to dispatch.
pub type ShortcutOnActivate = Box<dyn FnMut(KeyStroke, &mut EventContext) -> Intent>;

/// Closure signature for a key-capture callback.
///
/// Runs when the next `KeyDown` event bypasses shortcut resolution.
/// Receives the captured keystroke, mutable access to the registry
/// (for rebinds), and a mutable [`EventContext`] (so the handler can
/// emit commands, send intents, dismiss overlays, etc.).
pub type KeyCaptureCallback = Box<dyn FnOnce(KeyStroke, &mut ShortcutRegistry, &mut EventContext)>;

/// Shared cell behind [`CaptureHandle`] and the tree's active capture
/// slot. `None` once the capture has fired or been cancelled.
pub(crate) type KeyCaptureSlot = std::rc::Rc<std::cell::RefCell<Option<KeyCaptureCallback>>>;

/// RAII handle for an armed key-capture session.
///
/// Dropping the handle cancels the capture if it has not already
/// fired — the pattern matches
/// [`ObserverHandle`](crate::signal::ObserverHandle) elsewhere in the
/// framework. Widgets that arm a capture should hold onto the handle
/// (typically in their own state) so destruction of the widget tears
/// the capture down cleanly.
///
/// The handle refers to **its own** slot, not whatever capture is
/// currently armed; calling `begin_key_capture` twice creates two
/// independent slots. Dropping the old handle cancels only the old
/// slot (already orphaned by the new call) — it cannot race-cancel a
/// newer capture.
#[must_use = "key capture is cancelled when the CaptureHandle is dropped"]
pub struct CaptureHandle {
    slot: KeyCaptureSlot,
}

impl std::fmt::Debug for CaptureHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureHandle")
            .field("armed", &self.slot.borrow().is_some())
            .finish()
    }
}

impl CaptureHandle {
    pub(crate) fn new(slot: KeyCaptureSlot) -> Self {
        Self { slot }
    }

    /// Whether this handle's capture is still armed (`true`) or has
    /// already fired / been cancelled (`false`).
    pub fn is_armed(&self) -> bool {
        self.slot.borrow().is_some()
    }

    /// Cancel this handle's capture explicitly. Equivalent to dropping
    /// the handle; exposed for callers that want the cancellation to
    /// happen at a precise point rather than at scope end.
    pub fn cancel(self) {
        // Drop runs on scope exit; no explicit body needed.
        drop(self);
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        // Clear only *this* slot's callback — other capture sessions
        // (created by later calls to `begin_key_capture`) live in
        // their own `Rc<RefCell<...>>` instances.
        self.slot.borrow_mut().take();
    }
}

/// A user-facing, rebindable keyboard shortcut.
///
/// Shortcuts are declared by widgets at `build()` time and held in a
/// [`ShortcutRegistry`]. User rebindings (from a settings UI) stored
/// as `overrides` in the registry always win over the `primary`/
/// `secondary` declared here; those fields represent the **default**.
///
/// `name` and `description` are [`Prop<String>`] so they can track
/// locale changes through a `Signal<String>` without teksilo-core
/// depending on teksilo-i18n. Apps convert their `LocalizedString`
/// values to a `Signal<String>` at registration time.
pub struct Shortcut {
    /// Stable id used for persistence, registry lookup and menu/tooltip
    /// references. Must be unique within a [`ShortcutRegistry`].
    /// Hierarchical dot-style is the convention: `"editor.format.bold"`.
    pub id: &'static str,
    /// User-visible label shown in menus and the settings UI.
    pub name: Prop<String>,
    /// Optional settings-UI category, e.g. `"editor.format"`. When
    /// `None`, UIs should derive it from `id` (segment before last `.`).
    pub category: Option<&'static str>,
    /// Tooltip / detail text for the settings UI.
    pub description: Option<Prop<String>>,
    /// Default primary keystroke. Overridden at lookup time by the
    /// registry's override layer when the user has rebound this id.
    pub primary: Option<KeyStroke>,
    /// Default secondary (alternate) keystroke. Many apps let a single
    /// logical shortcut have two bindings (e.g. Ctrl+S and F12).
    pub secondary: Option<KeyStroke>,
    /// Optional explicit intent name emitted on activation. When
    /// `None`, the produced intent's name equals `id`.
    pub intent: Option<&'static str>,
    /// Produces the [`Intent`] to dispatch when this shortcut fires.
    /// When `None`, the registry synthesizes a no-parameter intent
    /// using [`Shortcut::intent_name`].
    pub on_activate: Option<ShortcutOnActivate>,
    /// Whether this shortcut is global or scoped to a widget subtree.
    /// Widget-declared shortcuts default to `Scoped(self_id)`;
    /// app-level declarations use `Global`.
    pub scope: ShortcutScope,
    /// Whether a matching, *disabled* [`Action`](crate::action::Action)
    /// on a widget should let the intent keep walking up the focus
    /// chain. `true` (the default) lets outer ancestors serve as
    /// fallbacks; `false` treats disabled as "owned, just dormant."
    pub propagate_when_disabled: bool,
    /// Reactive "is this shortcut currently live?" predicate.
    /// `None` means always enabled. When the signal resolves to
    /// `false`, the shortcut is treated **as if not registered** —
    /// the keystroke falls through to the focused widget's normal
    /// `on_key` dispatch and `on_activate` is **not** invoked.
    pub enabled_when: Option<Prop<bool>>,
    /// Take the declared chords literally instead of applying the
    /// primary-accelerator convention — see
    /// [`ShortcutBuilder::literal_modifiers`].
    pub literal_modifiers: bool,
}

impl fmt::Debug for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shortcut")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("category", &self.category)
            .field("description", &self.description)
            .field("primary", &self.primary)
            .field("secondary", &self.secondary)
            .field("intent", &self.intent)
            .field(
                "on_activate",
                &self.on_activate.as_ref().map(|_| "<closure>"),
            )
            .field("scope", &self.scope)
            .field("propagate_when_disabled", &self.propagate_when_disabled)
            .field("enabled_when", &self.enabled_when.is_some())
            .field("literal_modifiers", &self.literal_modifiers)
            .finish()
    }
}

impl Shortcut {
    /// Start building a shortcut with a stable id.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: &'static str) -> ShortcutBuilder {
        ShortcutBuilder {
            inner: Shortcut {
                id,
                name: Prop::Static(String::new()),
                category: None,
                description: None,
                primary: None,
                secondary: None,
                intent: None,
                on_activate: None,
                scope: ShortcutScope::Global,
                propagate_when_disabled: true,
                enabled_when: None,
                literal_modifiers: false,
            },
        }
    }

    /// Resolve the current enabled state. `true` when no predicate is
    /// set; otherwise reads the signal.
    pub fn is_enabled(&self) -> bool {
        self.enabled_when.as_ref().map(|s| s.get()).unwrap_or(true)
    }

    /// Intent name this shortcut produces. Falls back to `id` when no
    /// explicit intent is set.
    pub fn intent_name(&self) -> &'static str {
        self.intent.unwrap_or(self.id)
    }

    /// Whether `keystroke` matches this shortcut's primary or
    /// secondary default. The registry uses the **effective**
    /// keystrokes (defaults merged with overrides) during live
    /// lookups instead of this.
    ///
    /// Compares against the *resolved* defaults, so on macOS a chord
    /// declared `Ctrl+S` matches a pressed ⌘S — see
    /// [`declared_keystrokes`](Self::declared_keystrokes).
    pub fn matches_default(&self, keystroke: KeyStroke) -> bool {
        self.matches_default_using(keystroke, Modifiers::COMMAND)
    }

    /// [`matches_default`](Self::matches_default) against an explicit
    /// accelerator — see [`declared_keystrokes_using`](Self::declared_keystrokes_using).
    pub(crate) fn matches_default_using(&self, keystroke: KeyStroke, command: Modifiers) -> bool {
        let (primary, secondary) = self.declared_keystrokes_using(command);
        primary == Some(keystroke) || secondary == Some(keystroke)
    }

    /// This shortcut's declared chords as the platform actually reads
    /// them: unchanged when [`literal_modifiers`](Self::literal_modifiers)
    /// is set, otherwise passed through
    /// [`KeyStroke::with_command_convention`] so a declared `Ctrl` means ⌘
    /// on macOS.
    ///
    /// The registry layers user overrides on top of these; an override is
    /// never rewritten.
    pub fn declared_keystrokes(&self) -> (Option<KeyStroke>, Option<KeyStroke>) {
        self.declared_keystrokes_using(Modifiers::COMMAND)
    }

    /// [`declared_keystrokes`](Self::declared_keystrokes) resolved against an
    /// explicit accelerator: [`Modifiers::SUPER`] reads the declaration the way
    /// macOS does, [`Modifiers::CTRL`] the way Windows and Linux do.
    ///
    /// The registry always calls the current platform's form. This twin exists
    /// so both branches are testable from either host — notably the one a
    /// Linux CI can never observe, that a declared `Ctrl` chord resolves *away*
    /// from physical ⌃ on macOS and so must not fire on it.
    pub(crate) fn declared_keystrokes_using(
        &self,
        command: Modifiers,
    ) -> (Option<KeyStroke>, Option<KeyStroke>) {
        if self.literal_modifiers {
            (self.primary, self.secondary)
        } else {
            (
                self.primary
                    .map(|k| k.with_command_convention_using(command)),
                self.secondary
                    .map(|k| k.with_command_convention_using(command)),
            )
        }
    }
}

/// Fluent builder for [`Shortcut`]. Default scope is `Global`; use
/// [`ShortcutBuilder::scope`] or [`ShortcutBuilder::scope_to`] for a
/// scoped shortcut (the typical widget-declared case).
pub struct ShortcutBuilder {
    inner: Shortcut,
}

impl ShortcutBuilder {
    /// User-visible label. Accepts a static `String`/`&str` or a reactive
    /// `Signal<String>` / `Prop<String>` (for localized names driven by
    /// teksilo-i18n's `LocalizedString`).
    pub fn name(mut self, name: impl Into<Prop<String>>) -> Self {
        self.inner.name = name.into();
        self
    }

    pub fn category(mut self, category: &'static str) -> Self {
        self.inner.category = Some(category);
        self
    }

    /// Take the declared chords **literally** — no primary-accelerator
    /// convention, on any platform.
    ///
    /// By default a declared `Ctrl` chord is read as "the platform's
    /// accelerator" and becomes ⌘ on macOS (see
    /// [`KeyStroke::with_command_convention`]), which is what almost every
    /// command wants. A few chords are genuinely Control on macOS too, and for
    /// those the rewrite would be wrong or fatal:
    ///
    /// - **Ctrl+Tab** cycles tabs on macOS as well; ⌘Tab belongs to the
    ///   application switcher and never reaches an app at all.
    /// - Chords whose ⌘ form the system takes first — ⌘Space (Spotlight),
    ///   ⌘⇥, ⌘H, ⌘Q — where the rewritten chord would simply never arrive.
    ///
    /// Declaring those with `literal_modifiers` keeps them on Control
    /// everywhere, including macOS.
    pub fn literal_modifiers(mut self) -> Self {
        self.inner.literal_modifiers = true;
        self
    }

    /// Description. Accepts a static `String`/`&str` or a reactive
    /// `Signal<String>` / `Prop<String>`.
    pub fn description(mut self, description: impl Into<Prop<String>>) -> Self {
        self.inner.description = Some(description.into());
        self
    }

    pub fn primary(mut self, keystroke: KeyStroke) -> Self {
        self.inner.primary = Some(keystroke);
        self
    }

    pub fn secondary(mut self, keystroke: KeyStroke) -> Self {
        self.inner.secondary = Some(keystroke);
        self
    }

    /// Override the intent name; defaults to the shortcut's `id`.
    pub fn intent(mut self, intent: &'static str) -> Self {
        self.inner.intent = Some(intent);
        self
    }

    /// Provide a closure that produces the [`Intent`] at activation
    /// time. Use when the intent's parameters depend on the matched
    /// keystroke or runtime state.
    ///
    /// The closure may return any `Into<Intent>` — typically an
    /// [`IntentKind`](crate::intent::IntentKind) enum variant, which
    /// converts via the blanket `impl<K: IntentKind> From<K> for Intent`.
    pub fn on_activate<R>(
        mut self,
        mut f: impl FnMut(KeyStroke, &mut EventContext) -> R + 'static,
    ) -> Self
    where
        R: Into<Intent>,
    {
        self.inner.on_activate = Some(Box::new(move |ks, ctx| f(ks, ctx).into()));
        self
    }

    /// Scope the shortcut to a specific widget's subtree. Equivalent
    /// to `.scope(ShortcutScope::Scoped(id))`.
    pub fn scope_to(mut self, id: WidgetId) -> Self {
        self.inner.scope = ShortcutScope::Scoped(id);
        self
    }

    pub fn scope(mut self, scope: ShortcutScope) -> Self {
        self.inner.scope = scope;
        self
    }

    /// Explicit global scope (the builder's default).
    pub fn global(mut self) -> Self {
        self.inner.scope = ShortcutScope::Global;
        self
    }

    /// Control how a disabled matching [`Action`](crate::action::Action)
    /// behaves during dispatch. `true` (default): propagate to
    /// ancestors. `false`: consume the intent at that level.
    pub fn propagate_when_disabled(mut self, propagate: bool) -> Self {
        self.inner.propagate_when_disabled = propagate;
        self
    }

    /// Reactive predicate that controls whether the shortcut is
    /// *live*. When the signal holds `false`, the shortcut is
    /// treated as if it were not registered — the keystroke falls
    /// through to the focused widget's normal `on_key` dispatch.
    ///
    /// Typical use: a rich-text editor registering
    /// `editor.format.bold` with `enabled_when(has_selection)` so
    /// Ctrl+B only fires when there is something to embolden.
    pub fn enabled_when(mut self, signal: impl Into<Prop<bool>>) -> Self {
        self.inner.enabled_when = Some(signal.into());
        self
    }

    pub fn build(self) -> Shortcut {
        self.inner
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Per-slot user override state.
///
/// Each slot (primary / secondary) independently records whether the
/// user has touched it. `Default` means "fall back to the widget's
/// declared default at effective-lookup time" (so a later
/// re-registration with a different default flows through
/// automatically). `Bound(ks)` locks the slot to a specific chord,
/// and `Unbound` locks the slot to *no* chord.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SlotOverride {
    /// User hasn't touched this slot; use the shortcut's current
    /// declared default.
    #[default]
    Default,
    /// User explicitly bound the slot to this chord.
    Bound(KeyStroke),
    /// User explicitly unbound the slot.
    Unbound,
}

impl SlotOverride {
    /// Resolve this override against a fallback default from the
    /// shortcut's declaration site.
    pub fn resolve(self, fallback: Option<KeyStroke>) -> Option<KeyStroke> {
        match self {
            SlotOverride::Default => fallback,
            SlotOverride::Bound(ks) => Some(ks),
            SlotOverride::Unbound => None,
        }
    }

    /// Whether the user has touched this slot.
    pub fn is_touched(self) -> bool {
        !matches!(self, SlotOverride::Default)
    }
}

/// Per-shortcut user override (populated by the settings UI and
/// persisted to disk). Per-slot semantics: each field records either
/// a user edit or a delegation to the shortcut's declared default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyStrokeOverride {
    pub primary: SlotOverride,
    pub secondary: SlotOverride,
}

impl KeyStrokeOverride {
    /// Whether the override has any user-touched slot. Entries with
    /// all-Default slots are effectively empty and removed by
    /// `clear_override`-like paths.
    pub fn is_empty(self) -> bool {
        !self.primary.is_touched() && !self.secondary.is_touched()
    }
}

/// The merged, read-only view of a shortcut with user overrides applied.
///
/// Menus, tooltips and dispatch consume this shape. Fields borrow from
/// the registry so the consumer pays no clone cost.
///
/// `enabled` is the snapshot of the shortcut's `enabled_when` signal
/// at construction time — convenient for settings UIs that render
/// greyed-out rows for currently-inapplicable shortcuts. Dispatch
/// does not need to inspect it because
/// [`ShortcutRegistry::find_by_keystroke`] already filters disabled
/// shortcuts out.
#[derive(Debug, Clone, Copy)]
pub struct EffectiveShortcut<'a> {
    pub shortcut: &'a Shortcut,
    pub primary: Option<KeyStroke>,
    pub secondary: Option<KeyStroke>,
    pub enabled: bool,
}

impl EffectiveShortcut<'_> {
    pub fn matches(&self, keystroke: KeyStroke) -> bool {
        self.primary == Some(keystroke) || self.secondary == Some(keystroke)
    }
}

/// Two-layer registry of [`Shortcut`]s.
///
/// - `defaults` holds the records registered by widgets during their
///   `build()`. Re-registering the same id **upserts**: code-owned
///   fields are updated, the user override (if any) is preserved.
/// - `overrides` holds user-supplied keystroke rebindings keyed by
///   shortcut id. Overrides persist across widget rebuilds and even
///   when the corresponding default is temporarily unregistered —
///   graveyard semantics, so a widget that disappears and reappears
///   keeps its user-customised bindings.
///
/// Every mutation bumps [`ShortcutRegistry::version`] so consumers
/// (menu labels, settings UIs) can observe that signal and re-read
/// through [`ShortcutRegistry::effective`].
pub struct ShortcutRegistry {
    defaults: HashMap<&'static str, Shortcut>,
    overrides: HashMap<String, KeyStrokeOverride>,
    /// Reverse map from owner widget to the ids it registered. Used
    /// by [`ShortcutRegistry::unregister_all_for_owner`] when a widget
    /// is destroyed. Owner is distinct from scope: a widget can own a
    /// globally-scoped shortcut and still have it cleaned up when the
    /// widget goes away.
    by_owner: HashMap<WidgetId, Vec<&'static str>>,
    /// Reverse map from id to its owner, for cheap symmetric cleanup
    /// when a shortcut is re-registered by a different owner (rare,
    /// but the indices must stay consistent).
    owner_by_id: HashMap<&'static str, WidgetId>,
    version: Signal<u64>,
    /// Per-id reactive resolved *primary* keystroke, created lazily the
    /// first time a widget asks to observe an id (see
    /// [`ShortcutRegistry::effective_primary_signal`]). Each mutation
    /// refreshes only the ids it actually touched (with an equality
    /// guard), so registering or rebinding one shortcut never notifies
    /// the observers of an unrelated id. This is what lets a menu item
    /// bind its accelerator as a *leaf* value that repaints in place,
    /// instead of hard-rebuilding on the coarse global [`Self::version`]
    /// signal (which would tear down the item and drop clicks).
    resolved: HashMap<&'static str, Signal<Option<KeyStroke>>>,
}

impl Default for ShortcutRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ShortcutRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShortcutRegistry")
            .field("defaults", &self.defaults)
            .field("overrides", &self.overrides)
            .field("by_owner", &self.by_owner)
            .field("version", &self.version.get())
            .finish()
    }
}

impl ShortcutRegistry {
    pub fn new() -> Self {
        Self {
            defaults: HashMap::new(),
            overrides: HashMap::new(),
            by_owner: HashMap::new(),
            owner_by_id: HashMap::new(),
            version: Signal::new(0),
            resolved: HashMap::new(),
        }
    }

    /// A reactive handle that ticks on every mutation (register,
    /// unregister, rebind, put_override). Menus and settings widgets
    /// observe it to refresh derived state.
    pub fn version(&self) -> &Signal<u64> {
        &self.version
    }

    /// A reactive handle to the **effective primary keystroke** for a
    /// single shortcut `id`, created lazily and seeded with the current
    /// value. It ticks only when *that* id's resolved primary actually
    /// changes — registering, unregistering or rebinding any *other*
    /// shortcut leaves it untouched.
    ///
    /// This is the granular counterpart to [`Self::version`]: a widget
    /// that displays one shortcut's accelerator (a menu item, a
    /// tooltip) should bind this and update its label as a leaf value,
    /// rather than observing the coarse global version and rebuilding.
    pub fn effective_primary_signal(&mut self, id: &'static str) -> Signal<Option<KeyStroke>> {
        if let Some(sig) = self.resolved.get(id) {
            return sig.clone();
        }
        let current = self
            .defaults
            .get(id)
            .and_then(|s| self.resolved_keystrokes(id, s).0);
        let sig = Signal::new(current);
        self.resolved.insert(id, sig.clone());
        sig
    }

    /// Upsert a shortcut default without an owner. If `id` already
    /// exists this **replaces** the code-owned fields but
    /// **preserves any existing user override**.
    ///
    /// Use [`ShortcutRegistry::register_owned`] to tie the lifetime
    /// of a registration to a widget (so arena destroy can clean up
    /// automatically).
    pub fn register(&mut self, shortcut: Shortcut) -> Option<Shortcut> {
        let id = shortcut.id;
        let previous = self.defaults.insert(id, shortcut);
        // If a previous registration had an owner, the new anonymous
        // registration reassigns ownership away. Drop the old owner
        // index entry so `unregister_all_for_owner` stays accurate.
        self.detach_owner_index(id);
        self.bump_version();
        self.refresh_resolved(id);
        previous
    }

    /// Upsert a shortcut default owned by `owner`. When `owner` is
    /// destroyed, the framework calls
    /// [`ShortcutRegistry::unregister_all_for_owner`] to remove this
    /// registration. Preserves user overrides identically to
    /// [`ShortcutRegistry::register`].
    pub fn register_owned(&mut self, shortcut: Shortcut, owner: WidgetId) -> Option<Shortcut> {
        let id = shortcut.id;
        let previous = self.defaults.insert(id, shortcut);
        self.detach_owner_index(id);
        self.by_owner.entry(owner).or_default().push(id);
        self.owner_by_id.insert(id, owner);
        self.bump_version();
        self.refresh_resolved(id);
        previous
    }

    /// Remove a default by id. The user override for that id (if any)
    /// stays in the graveyard so it can be re-applied if the shortcut
    /// is later re-registered.
    pub fn unregister(&mut self, id: &str) -> Option<Shortcut> {
        let removed = self.defaults.remove(id);
        if removed.is_some() {
            self.detach_owner_index(id);
            self.bump_version();
            self.refresh_resolved(id);
        }
        removed
    }

    /// Remove every shortcut registered by `owner`. Called by the
    /// widget tree when a widget is destroyed — keeps the registry
    /// from leaking entries whose `on_activate` closures may capture
    /// state owned by the destroyed widget.
    pub fn unregister_all_for_owner(&mut self, owner: WidgetId) {
        let Some(ids) = self.by_owner.remove(&owner) else {
            return;
        };
        let mut any = false;
        for id in ids {
            if self.defaults.remove(id).is_some() {
                any = true;
            }
            self.owner_by_id.remove(id);
            // Its effective primary is now gone — push that to any observer.
            self.refresh_resolved(id);
        }
        if any {
            self.bump_version();
        }
    }

    /// The widget id that currently owns `id`, if any.
    pub fn owner_of(&self, id: &str) -> Option<WidgetId> {
        self.owner_by_id.get(id).copied()
    }

    pub fn len(&self) -> usize {
        self.defaults.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defaults.is_empty()
    }

    /// Iterate the raw defaults (no overrides merged). Most UI code
    /// wants [`ShortcutRegistry::iter_effective`] instead.
    pub fn iter_defaults(&self) -> impl Iterator<Item = &Shortcut> {
        self.defaults.values()
    }

    /// Borrow the raw default record.
    pub fn get_default(&self, id: &str) -> Option<&Shortcut> {
        self.defaults.get(id)
    }

    /// Invoke the registered `on_activate` closure for the shortcut
    /// with the given `id`. Returns the produced [`Intent`] (or a
    /// synthesized no-parameter intent when the shortcut has no
    /// custom `on_activate`), or `None` if `id` is not registered.
    ///
    /// Deliberately split from `find_by_keystroke` so the dispatcher
    /// can check scope and `enabled_when` **before** the closure
    /// runs — otherwise a scope mismatch or disabled predicate would
    /// silently drop any side effects the closure put in `ctx`.
    pub(crate) fn invoke_on_activate(
        &mut self,
        id: &str,
        keystroke: KeyStroke,
        ctx: &mut EventContext,
    ) -> Option<Intent> {
        let shortcut = self.defaults.get_mut(id)?;
        let intent_name = shortcut.intent_name();
        let intent = match &mut shortcut.on_activate {
            Some(handler) => handler(keystroke, ctx),
            None => Intent::new(intent_name),
        };
        Some(intent)
    }

    /// Current override for `id`, if any.
    pub fn override_for(&self, id: &str) -> Option<KeyStrokeOverride> {
        self.overrides.get(id).copied()
    }

    /// Set the full override for `id`. Intended for loading persisted
    /// user preferences at app startup.
    pub fn put_override(&mut self, id: impl Into<String>, override_: KeyStrokeOverride) {
        let id = id.into();
        self.overrides.insert(id.clone(), override_);
        self.bump_version();
        self.refresh_resolved(&id);
    }

    /// Set the primary slot of the override for `id`. The secondary
    /// slot is left untouched — with per-slot [`SlotOverride`]
    /// semantics the untouched slot continues to delegate to
    /// whatever default the shortcut currently declares.
    pub fn rebind_primary(&mut self, id: impl Into<String>, keystroke: Option<KeyStroke>) {
        let id = id.into();
        let entry = self.overrides.entry(id.clone()).or_default();
        entry.primary = match keystroke {
            Some(ks) => SlotOverride::Bound(ks),
            None => SlotOverride::Unbound,
        };
        self.bump_version();
        self.refresh_resolved(&id);
    }

    /// Set the secondary slot of the override for `id`. The primary
    /// slot stays in whatever state it was (`Default` or user-set).
    pub fn rebind_secondary(&mut self, id: impl Into<String>, keystroke: Option<KeyStroke>) {
        let id = id.into();
        let entry = self.overrides.entry(id.clone()).or_default();
        entry.secondary = match keystroke {
            Some(ks) => SlotOverride::Bound(ks),
            None => SlotOverride::Unbound,
        };
        self.bump_version();
        // Secondary-only change: the primary signal is guarded, so this
        // is a no-op for primary observers — kept for uniformity.
        self.refresh_resolved(&id);
    }

    /// Drop the user override for `id`, restoring the declared defaults.
    pub fn clear_override(&mut self, id: &str) {
        if self.overrides.remove(id).is_some() {
            self.bump_version();
            self.refresh_resolved(id);
        }
    }

    /// Clear every override. Restores the declared defaults for all
    /// registered shortcuts. Graveyard entries are dropped too.
    pub fn clear_all_overrides(&mut self) {
        if !self.overrides.is_empty() {
            self.overrides.clear();
            self.bump_version();
            self.refresh_all_resolved();
        }
    }

    /// Snapshot of the full override map, suitable for persisting to
    /// disk. Cloned intentionally so callers can serialize without
    /// holding a borrow on the registry.
    pub fn export_overrides(&self) -> HashMap<String, KeyStrokeOverride> {
        self.overrides.clone()
    }

    /// Replace the entire override map from a persisted snapshot.
    /// Typically called once at app startup after loading user
    /// preferences from disk. Overrides for ids that are not yet
    /// registered are kept in the graveyard so they apply whenever
    /// the widget that declares the corresponding default shows up.
    pub fn import_overrides(&mut self, overrides: HashMap<String, KeyStrokeOverride>) {
        self.overrides = overrides;
        self.bump_version();
        self.refresh_all_resolved();
    }

    /// Effective (defaults + overrides) view of `id`. `None` when the
    /// id has no registered default — overrides alone don't manifest
    /// as effective records, but they're kept in the graveyard.
    pub fn effective(&self, id: &str) -> Option<EffectiveShortcut<'_>> {
        let shortcut = self.defaults.get(id)?;
        let (primary, secondary) = self.resolved_keystrokes(id, shortcut);
        Some(EffectiveShortcut {
            shortcut,
            primary,
            secondary,
            enabled: shortcut.is_enabled(),
        })
    }

    /// Iterate all effective shortcuts, including currently-disabled
    /// ones. The per-item `enabled` flag lets settings UIs render
    /// disabled rows greyed out.
    ///
    /// Order is deterministic: sorted by `(category, id)` so repeated
    /// calls produce identical sequences regardless of the internal
    /// `HashMap` insertion order.
    pub fn iter_effective(&self) -> impl Iterator<Item = EffectiveShortcut<'_>> {
        let mut items: Vec<EffectiveShortcut<'_>> = self
            .defaults
            .iter()
            .map(|(id, shortcut)| {
                let (primary, secondary) = self.resolved_keystrokes(id, shortcut);
                EffectiveShortcut {
                    shortcut,
                    primary,
                    secondary,
                    enabled: shortcut.is_enabled(),
                }
            })
            .collect();
        items.sort_by(|a, b| {
            a.shortcut
                .category
                .cmp(&b.shortcut.category)
                .then(a.shortcut.id.cmp(b.shortcut.id))
        });
        items.into_iter()
    }

    /// Find the first shortcut id that **genuinely** conflicts with
    /// `keystroke`, excluding `excluding_id` if given. Used by settings
    /// UIs to auto-unbind conflicts when the user rebinds a chord.
    /// Includes disabled shortcuts — a chord is "taken" regardless of
    /// whether its current binding is live.
    ///
    /// **Scope-aware.** Two shortcuts only conflict when they could be
    /// simultaneously active: either one is [`ShortcutScope::Global`]
    /// (active everywhere), or both are [`ShortcutScope::Scoped`] to the
    /// **same** widget. Two shortcuts scoped to *different* widgets —
    /// e.g. a `Delete` binding in two separate panels — are **not** a
    /// conflict, because the dispatcher resolves them by focus. (The
    /// registry can't see the tree, so two different `Scoped` ids are
    /// assumed disjoint; the rare genuinely-nested overlap is left
    /// unflagged, erring toward allowing the binding.)
    ///
    /// When `excluding_id` is `None` (or names an unregistered id) the
    /// "self" scope is unknown, so every same-chord shortcut is flagged
    /// — the safe, conservative fallback.
    pub fn find_conflict(
        &self,
        keystroke: KeyStroke,
        excluding_id: Option<&str>,
    ) -> Option<&'static str> {
        let self_scope = excluding_id
            .and_then(|eid| self.defaults.get(eid))
            .map(|s| s.scope);
        self.defaults.iter().find_map(|(&id, shortcut)| {
            if excluding_id == Some(id) {
                return None;
            }
            let (primary, secondary) = self.resolved_keystrokes(id, shortcut);
            if primary != Some(keystroke) && secondary != Some(keystroke) {
                return None;
            }
            // Chord matches — apply the scope rule. With no known self
            // scope, flag unconditionally (conservative fallback).
            match self_scope {
                Some(self_scope) if !scopes_can_collide(self_scope, shortcut.scope) => None,
                _ => Some(id),
            }
        })
    }

    /// All effective shortcuts whose primary or secondary keystroke
    /// matches **and** are currently enabled, in the deterministic
    /// `(category, id)` order of [`iter_effective`](Self::iter_effective).
    ///
    /// The dispatcher needs *every* same-chord candidate, not just the
    /// first: the first by id-order may be a `Scoped` binding whose
    /// subtree doesn't contain the current focus (and so must yield to
    /// an applicable `Global` one), or a `Global` binding that should
    /// itself yield to an in-focus `Scoped` one (most-specific-scope
    /// wins). Resolving that needs the widget tree (descendant checks),
    /// which the registry can't see — so it hands back all candidates
    /// and the dispatcher selects with focus in hand.
    pub fn matches_by_keystroke(
        &self,
        keystroke: KeyStroke,
    ) -> impl Iterator<Item = EffectiveShortcut<'_>> {
        self.iter_effective()
            .filter(move |s| s.enabled && s.matches(keystroke))
    }

    /// First effective shortcut whose primary or secondary keystroke
    /// matches **and** is currently enabled. Disabled shortcuts are
    /// invisible to the dispatcher — the keystroke falls through to
    /// the focused widget's normal `on_key` handling, matching the
    /// "treated as if not registered" semantic advertised by
    /// [`Shortcut::enabled_when`].
    ///
    /// Note: this ignores scope applicability — for focus-aware
    /// resolution the dispatcher uses
    /// [`matches_by_keystroke`](Self::matches_by_keystroke) instead.
    pub fn find_by_keystroke(&self, keystroke: KeyStroke) -> Option<EffectiveShortcut<'_>> {
        self.matches_by_keystroke(keystroke).next()
    }

    fn resolved_keystrokes(
        &self,
        id: &str,
        shortcut: &Shortcut,
    ) -> (Option<KeyStroke>, Option<KeyStroke>) {
        let ov = self.overrides.get(id).copied().unwrap_or_default();
        // Declared defaults go through the primary-accelerator convention
        // (`Ctrl` → ⌘ on macOS); user overrides do not. An app author writes
        // one chord for three platforms and means "the accelerator"; a user
        // who captured a chord in a settings UI pressed the keys they meant,
        // and rewriting those would put physical ⌃F out of reach on macOS.
        let (primary, secondary) = shortcut.declared_keystrokes();
        (ov.primary.resolve(primary), ov.secondary.resolve(secondary))
    }

    fn bump_version(&self) {
        self.version.set(self.version.get().wrapping_add(1));
    }

    /// Push the current effective primary keystroke for `id` into its
    /// per-id signal, if one is being observed. Guarded by equality so
    /// a no-op re-registration (e.g. a widget re-declaring the same
    /// shortcut on rebuild) doesn't notify observers. A `&str` is
    /// accepted so the override-keyed (`String`) mutators can call it.
    fn refresh_resolved(&self, id: &str) {
        if let Some(sig) = self.resolved.get(id) {
            let current = self
                .defaults
                .get(id)
                .and_then(|s| self.resolved_keystrokes(id, s).0);
            if sig.get() != current {
                sig.set(current);
            }
        }
    }

    /// Refresh every observed id — for the "reset everything" mutators
    /// (`clear_all_overrides`, `import_overrides`) that can change many
    /// resolutions at once. The per-id equality guard keeps unchanged
    /// ids from notifying.
    fn refresh_all_resolved(&self) {
        let ids: Vec<&'static str> = self.resolved.keys().copied().collect();
        for id in ids {
            self.refresh_resolved(id);
        }
    }

    /// Drop the owner index entries for `id`. Idempotent — safe to call
    /// even when the id has no known owner.
    fn detach_owner_index(&mut self, id: &str) {
        let Some(owner) = self.owner_by_id.remove(id) else {
            return;
        };
        if let Some(vec) = self.by_owner.get_mut(&owner) {
            vec.retain(|entry| *entry != id);
            if vec.is_empty() {
                self.by_owner.remove(&owner);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Shortcut builder & defaults ------------------------------------

    #[test]
    fn builder_defaults_are_sane() {
        let s = Shortcut::new("editor.format.bold").name("Bold").build();
        assert_eq!(s.id, "editor.format.bold");
        assert_eq!(s.intent_name(), "editor.format.bold");
        assert_eq!(s.name.get(), "Bold");
        assert_eq!(s.scope, ShortcutScope::Global);
        assert!(s.propagate_when_disabled);
        assert!(s.primary.is_none());
        assert!(s.secondary.is_none());
    }

    #[test]
    fn builder_intent_overrides_id() {
        let s = Shortcut::new("app.save_as").intent("app.save").build();
        assert_eq!(s.intent_name(), "app.save");
    }

    #[test]
    fn builder_scope_variants() {
        use slotmap::KeyData;
        let id: WidgetId = KeyData::from_ffi(7).into();

        let g = Shortcut::new("foo").build();
        assert_eq!(g.scope, ShortcutScope::Global);

        let s = Shortcut::new("bar").scope_to(id).build();
        assert_eq!(s.scope, ShortcutScope::Scoped(id));

        let e = Shortcut::new("baz")
            .scope(ShortcutScope::Scoped(id))
            .build();
        assert_eq!(e.scope, ShortcutScope::Scoped(id));

        let back = Shortcut::new("qux").scope_to(id).global().build();
        assert_eq!(back.scope, ShortcutScope::Global);
    }

    #[test]
    fn shortcut_matches_default_primary_and_secondary() {
        let s = Shortcut::new("edit.undo")
            .primary(KeyStroke::command(Key::Z))
            .secondary(KeyStroke::alt(Key::Backspace))
            .build();
        assert!(s.matches_default(KeyStroke::command(Key::Z)));
        assert!(s.matches_default(KeyStroke::alt(Key::Backspace)));
        assert!(!s.matches_default(KeyStroke::command(Key::Y)));
    }

    // --- Registry: upsert preserves user overrides ---------------------

    #[test]
    fn register_upserts_and_preserves_override() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        // User rebinds Ctrl+S → Ctrl+Shift+S.
        reg.rebind_primary("app.save", Some(KeyStroke::command_shift(Key::S)));
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            Some(KeyStroke::command_shift(Key::S))
        );

        // Widget rebuilds, re-registers with the same defaults. The
        // user override must survive.
        reg.register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            Some(KeyStroke::command_shift(Key::S))
        );

        // Defaults change (rename + new default keystroke) but the
        // effective primary is still the user's rebinding.
        reg.register(
            Shortcut::new("app.save")
                .name("Save (renamed)")
                .primary(KeyStroke::alt(Key::S))
                .build(),
        );
        let eff = reg.effective("app.save").unwrap();
        assert_eq!(eff.primary, Some(KeyStroke::command_shift(Key::S)));
        assert_eq!(eff.shortcut.name.get(), "Save (renamed)");
    }

    #[test]
    fn clear_override_restores_default() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        reg.rebind_primary("app.save", Some(KeyStroke::command_shift(Key::S)));
        reg.clear_override("app.save");
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            Some(KeyStroke::command(Key::S))
        );
    }

    // --- Registry: graveyard -------------------------------------------

    #[test]
    fn override_survives_unregister_and_reregister() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("editor.format.bold")
                .primary(KeyStroke::command(Key::B))
                .build(),
        );
        reg.rebind_primary("editor.format.bold", Some(KeyStroke::command_shift(Key::B)));

        reg.unregister("editor.format.bold");
        assert!(reg.effective("editor.format.bold").is_none());
        assert_eq!(
            reg.override_for("editor.format.bold").unwrap().primary,
            SlotOverride::Bound(KeyStroke::command_shift(Key::B))
        );

        reg.register(
            Shortcut::new("editor.format.bold")
                .primary(KeyStroke::command(Key::B))
                .build(),
        );
        assert_eq!(
            reg.effective("editor.format.bold").unwrap().primary,
            Some(KeyStroke::command_shift(Key::B))
        );
    }

    // --- Registry: version signal --------------------------------------

    #[test]
    fn version_bumps_on_every_mutation() {
        let mut reg = ShortcutRegistry::new();
        let v0 = reg.version().get();

        reg.register(Shortcut::new("a").build());
        let v1 = reg.version().get();
        assert!(v1 > v0);

        reg.rebind_primary("a", Some(KeyStroke::command(Key::A)));
        let v2 = reg.version().get();
        assert!(v2 > v1);

        reg.rebind_secondary("a", Some(KeyStroke::alt(Key::A)));
        let v3 = reg.version().get();
        assert!(v3 > v2);

        reg.clear_override("a");
        let v4 = reg.version().get();
        assert!(v4 > v3);

        reg.unregister("a");
        let v5 = reg.version().get();
        assert!(v5 > v4);
    }

    // --- Registry: per-id resolved signal (granular reactivity) --------

    #[test]
    fn per_id_signal_seeds_isolates_and_tracks() {
        use std::cell::Cell;
        use std::rc::Rc;

        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("work.new")
                .primary(KeyStroke::command(Key::N))
                .build(),
        );

        // Seeded with the current effective primary.
        let sig = reg.effective_primary_signal("work.new");
        assert_eq!(sig.get(), Some(KeyStroke::command(Key::N)));

        // Count notifications to prove *isolation* from unrelated churn.
        let hits = Rc::new(Cell::new(0usize));
        let _h = {
            let hits = hits.clone();
            sig.observe(move |_| hits.set(hits.get() + 1))
        };

        // Registering / unregistering an UNRELATED id must not notify us —
        // this is the whole point: a scoped shortcut registered in some
        // other widget's build() no longer disturbs this menu item.
        reg.register(
            Shortcut::new("outline.open_to_side")
                .primary(KeyStroke::command(Key::Enter))
                .build(),
        );
        reg.unregister("outline.open_to_side");
        assert_eq!(
            hits.get(),
            0,
            "unrelated shortcut churn must not notify a per-id observer"
        );
        assert_eq!(sig.get(), Some(KeyStroke::command(Key::N)));

        // Rebinding OUR id updates the signal (and notifies exactly once).
        reg.rebind_primary("work.new", Some(KeyStroke::command_shift(Key::N)));
        assert_eq!(sig.get(), Some(KeyStroke::command_shift(Key::N)));
        assert_eq!(hits.get(), 1);

        // Unregistering OUR id resolves to None.
        reg.unregister("work.new");
        assert_eq!(sig.get(), None);
        assert_eq!(hits.get(), 2);
    }

    #[test]
    fn per_id_signal_observed_before_registration_goes_live_on_register() {
        // The menu-open scenario: a widget observes an id whose default is
        // not registered yet; when it later registers, the signal updates —
        // so the accelerator is current whenever the menu next appears,
        // even though the item is never rebuilt for shortcut changes.
        let mut reg = ShortcutRegistry::new();
        let sig = reg.effective_primary_signal("late.cmd");
        assert_eq!(sig.get(), None);

        reg.register(
            Shortcut::new("late.cmd")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        assert_eq!(sig.get(), Some(KeyStroke::command(Key::S)));

        // A user override on top is reflected too.
        reg.rebind_primary("late.cmd", Some(KeyStroke::command_shift(Key::S)));
        assert_eq!(sig.get(), Some(KeyStroke::command_shift(Key::S)));
    }

    // --- Registry: find_by_keystroke honors overrides -----------------

    #[test]
    fn find_by_keystroke_uses_effective() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::command(Key::S))
                .map(|s| s.shortcut.id),
            Some("app.save")
        );

        reg.rebind_primary("app.save", Some(KeyStroke::command_shift(Key::S)));

        assert!(reg.find_by_keystroke(KeyStroke::command(Key::S)).is_none());
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::command_shift(Key::S))
                .map(|s| s.shortcut.id),
            Some("app.save")
        );
    }

    // --- Registry: owner indexing & cleanup ----------------------------

    #[test]
    fn unregister_all_for_owner_removes_only_owner_entries() {
        use slotmap::KeyData;
        let editor: WidgetId = KeyData::from_ffi(1).into();
        let other: WidgetId = KeyData::from_ffi(2).into();

        let mut reg = ShortcutRegistry::new();
        reg.register_owned(
            Shortcut::new("editor.format.bold")
                .primary(KeyStroke::command(Key::B))
                .build(),
            editor,
        );
        reg.register_owned(
            Shortcut::new("editor.format.italic")
                .primary(KeyStroke::command(Key::I))
                .build(),
            editor,
        );
        reg.register_owned(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
                .build(),
            other,
        );
        assert_eq!(reg.len(), 3);

        reg.unregister_all_for_owner(editor);
        assert_eq!(reg.len(), 1);
        assert!(reg.get_default("editor.format.bold").is_none());
        assert!(reg.get_default("editor.format.italic").is_none());
        assert!(reg.get_default("app.save").is_some());
        assert_eq!(reg.owner_of("app.save"), Some(other));
    }

    #[test]
    fn anonymous_register_drops_prior_owner_index() {
        use slotmap::KeyData;
        let editor: WidgetId = KeyData::from_ffi(42).into();

        let mut reg = ShortcutRegistry::new();
        reg.register_owned(Shortcut::new("foo").build(), editor);
        assert_eq!(reg.owner_of("foo"), Some(editor));

        // Anonymous re-registration (e.g., app-level) supersedes the
        // owner link; cleanup for `editor` must no longer drop "foo".
        reg.register(Shortcut::new("foo").build());
        assert_eq!(reg.owner_of("foo"), None);

        reg.unregister_all_for_owner(editor);
        assert!(reg.get_default("foo").is_some());
    }

    #[test]
    fn reregister_with_new_owner_reassigns() {
        use slotmap::KeyData;
        let a: WidgetId = KeyData::from_ffi(10).into();
        let b: WidgetId = KeyData::from_ffi(11).into();

        let mut reg = ShortcutRegistry::new();
        reg.register_owned(Shortcut::new("bar").build(), a);
        reg.register_owned(Shortcut::new("bar").build(), b);
        assert_eq!(reg.owner_of("bar"), Some(b));

        // Cleanup for the original owner should be a no-op now.
        reg.unregister_all_for_owner(a);
        assert!(reg.get_default("bar").is_some());
        reg.unregister_all_for_owner(b);
        assert!(reg.get_default("bar").is_none());
    }

    // --- enabled_when --------------------------------------------------

    #[test]
    fn shortcut_is_enabled_defaults_true() {
        let s = Shortcut::new("foo").build();
        assert!(s.is_enabled());
    }

    #[test]
    fn shortcut_is_enabled_follows_signal() {
        let enabled = Signal::new(false);
        let s = Shortcut::new("foo").enabled_when(enabled.clone()).build();
        assert!(!s.is_enabled());
        enabled.set(true);
        assert!(s.is_enabled());
    }

    #[test]
    fn find_by_keystroke_skips_disabled() {
        let enabled = Signal::new(false);
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
                .enabled_when(enabled.clone())
                .build(),
        );
        // Disabled → invisible to dispatch.
        assert!(reg.find_by_keystroke(KeyStroke::command(Key::S)).is_none());

        // Enable → dispatch sees it.
        enabled.set(true);
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::command(Key::S))
                .map(|s| s.shortcut.id),
            Some("app.save")
        );
    }

    #[test]
    fn overrides_round_trip_through_export_import() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        reg.rebind_primary("app.save", Some(KeyStroke::command_shift(Key::S)));

        let snapshot = reg.export_overrides();
        assert_eq!(snapshot.len(), 1);

        // Fresh registry, same defaults but no overrides yet.
        let mut reg2 = ShortcutRegistry::new();
        reg2.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        assert_eq!(
            reg2.effective("app.save").unwrap().primary,
            Some(KeyStroke::command(Key::S))
        );

        reg2.import_overrides(snapshot);
        assert_eq!(
            reg2.effective("app.save").unwrap().primary,
            Some(KeyStroke::command_shift(Key::S))
        );
    }

    #[test]
    fn clear_all_overrides_restores_every_default() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("a")
                .primary(KeyStroke::command(Key::A))
                .build(),
        );
        reg.register(
            Shortcut::new("b")
                .primary(KeyStroke::command(Key::B))
                .build(),
        );
        reg.rebind_primary("a", Some(KeyStroke::alt(Key::A)));
        reg.rebind_primary("b", Some(KeyStroke::alt(Key::B)));

        reg.clear_all_overrides();
        assert_eq!(
            reg.effective("a").unwrap().primary,
            Some(KeyStroke::command(Key::A))
        );
        assert_eq!(
            reg.effective("b").unwrap().primary,
            Some(KeyStroke::command(Key::B))
        );
    }

    #[test]
    fn untouched_slot_tracks_live_default_after_reregistration() {
        // Per-slot SlotOverride semantics: a rebind on the primary
        // slot MUST leave the secondary slot delegating to the
        // shortcut's current declaration. When the widget later
        // re-registers with a different default secondary, the
        // untouched secondary slot flows through automatically.
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("foo")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );

        reg.rebind_primary("foo", Some(KeyStroke::command_shift(Key::S)));

        // Widget re-registers with a NEW default secondary. The
        // untouched secondary slot must pick this up — that is the
        // whole point of per-slot Default delegation.
        reg.register(
            Shortcut::new("foo")
                .primary(KeyStroke::command(Key::S))
                .secondary(KeyStroke::alt(Key::S))
                .build(),
        );

        let eff = reg.effective("foo").unwrap();
        assert_eq!(eff.primary, Some(KeyStroke::command_shift(Key::S)));
        assert_eq!(
            eff.secondary,
            Some(KeyStroke::alt(Key::S)),
            "untouched secondary slot must reflect the new default"
        );
    }

    #[test]
    fn rebind_primary_none_is_explicit_unbind_not_delegate() {
        // Passing `None` to rebind_primary must set the slot to
        // Unbound (user explicitly said "no binding here"), not
        // Default (which would still fall back to the declared
        // default).
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("foo")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        reg.rebind_primary("foo", None);
        assert_eq!(reg.effective("foo").unwrap().primary, None);

        // `clear_override` goes back to default.
        reg.clear_override("foo");
        assert_eq!(
            reg.effective("foo").unwrap().primary,
            Some(KeyStroke::command(Key::S))
        );
    }

    #[test]
    fn find_conflict_skips_excluded_id_and_respects_overrides() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("a")
                .primary(KeyStroke::command(Key::X))
                .build(),
        );
        reg.register(
            Shortcut::new("b")
                .primary(KeyStroke::command(Key::Y))
                .build(),
        );

        // Ctrl+X is bound to "a"; looking for it while excluding "a"
        // returns None, including it returns Some("a").
        assert_eq!(
            reg.find_conflict(KeyStroke::command(Key::X), Some("a")),
            None
        );
        assert_eq!(
            reg.find_conflict(KeyStroke::command(Key::X), None),
            Some("a")
        );
        // Ctrl+Z is bound to nothing.
        assert_eq!(reg.find_conflict(KeyStroke::command(Key::Z), None), None);

        // User rebinds "b" to Ctrl+X — that becomes the new conflict
        // for Ctrl+X (overrides outrank defaults).
        reg.rebind_primary("b", Some(KeyStroke::command(Key::X)));
        assert_eq!(
            reg.find_conflict(KeyStroke::command(Key::X), Some("b")),
            Some("a")
        );
        assert_eq!(
            reg.find_conflict(KeyStroke::command(Key::X), Some("a")),
            Some("b")
        );
    }

    #[test]
    fn find_conflict_is_scope_aware() {
        use slotmap::KeyData;
        let panel_a: WidgetId = KeyData::from_ffi(11).into();
        let panel_b: WidgetId = KeyData::from_ffi(22).into();

        let mut reg = ShortcutRegistry::new();
        // Same chord (Delete) scoped to two different panels — legitimate,
        // resolved by focus at runtime, NOT a conflict.
        reg.register(
            Shortcut::new("a.delete")
                .scope_to(panel_a)
                .primary(KeyStroke::new(Key::Delete, Modifiers::NONE))
                .build(),
        );
        reg.register(
            Shortcut::new("b.delete")
                .scope_to(panel_b)
                .primary(KeyStroke::new(Key::Delete, Modifiers::NONE))
                .build(),
        );
        assert_eq!(
            reg.find_conflict(
                KeyStroke::new(Key::Delete, Modifiers::NONE),
                Some("a.delete")
            ),
            None,
            "Delete in a different panel scope is not a conflict"
        );

        // A second shortcut scoped to the SAME panel IS a conflict.
        reg.register(
            Shortcut::new("a.delete2")
                .scope_to(panel_a)
                .primary(KeyStroke::new(Key::Delete, Modifiers::NONE))
                .build(),
        );
        assert_eq!(
            reg.find_conflict(
                KeyStroke::new(Key::Delete, Modifiers::NONE),
                Some("a.delete")
            ),
            Some("a.delete2"),
            "same-scope same-chord is a real conflict"
        );

        // A Global shortcut on the same chord collides with everything.
        reg.register(
            Shortcut::new("g.delete")
                .global()
                .primary(KeyStroke::new(Key::Delete, Modifiers::NONE))
                .build(),
        );
        // The global excludes itself and collides with all three scoped
        // bindings; HashMap order makes the exact id arbitrary, so just
        // require it found one of them.
        let hit = reg.find_conflict(
            KeyStroke::new(Key::Delete, Modifiers::NONE),
            Some("g.delete"),
        );
        assert!(
            matches!(hit, Some("a.delete" | "a.delete2" | "b.delete")),
            "a global chord conflicts with any scoped binding, got {hit:?}"
        );
        assert_eq!(
            reg.find_conflict(
                KeyStroke::new(Key::Delete, Modifiers::NONE),
                Some("b.delete")
            ),
            Some("g.delete"),
            "a scoped binding conflicts with a global on the same chord"
        );
    }

    #[test]
    fn iter_effective_order_is_deterministic_by_category_then_id() {
        let mut reg = ShortcutRegistry::new();
        reg.register(Shortcut::new("z.last").category("edit").build());
        reg.register(Shortcut::new("a.first").category("edit").build());
        reg.register(Shortcut::new("m.file").category("app").build());

        let ids: Vec<&str> = reg.iter_effective().map(|e| e.shortcut.id).collect();
        assert_eq!(ids, vec!["m.file", "a.first", "z.last"]);
    }

    #[test]
    fn iter_effective_still_includes_disabled_with_flag() {
        let enabled = Signal::new(false);
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::command(Key::S))
                .enabled_when(enabled.clone())
                .build(),
        );
        let all: Vec<_> = reg.iter_effective().collect();
        assert_eq!(all.len(), 1);
        assert!(!all[0].enabled, "settings UI must see disabled state");

        enabled.set(true);
        let all: Vec<_> = reg.iter_effective().collect();
        assert!(all[0].enabled);
    }

    #[test]
    fn secondary_keystroke_matches_via_effective() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("edit.undo")
                .primary(KeyStroke::command(Key::Z))
                .secondary(KeyStroke::alt(Key::Backspace))
                .build(),
        );
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::alt(Key::Backspace))
                .map(|s| s.shortcut.id),
            Some("edit.undo")
        );
    }

    // --- The primary-accelerator convention -----------------------------
    //
    // `Modifiers::COMMAND` resolves at compile time, so these assert the rule
    // in terms of `KeyStroke::command`, which resolves the same way — true on
    // every host, and on macOS the statement that ⌘F fires a `Ctrl+F`
    // declaration. The platform-parameterised half of the rule (the actual
    // Ctrl→⌘ rewrite, testable from a Linux CI) lives in `event::modifier_tests`.

    #[test]
    fn a_declared_ctrl_chord_resolves_to_the_platform_accelerator() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("editor.find")
                .primary(KeyStroke::ctrl(Key::F))
                .secondary(KeyStroke::ctrl_shift(Key::F))
                .build(),
        );
        let eff = reg.effective("editor.find").unwrap();
        assert_eq!(eff.primary, Some(KeyStroke::command(Key::F)));
        assert_eq!(eff.secondary, Some(KeyStroke::command_shift(Key::F)));

        // The whole point: the accelerator chord dispatches.
        assert!(
            reg.find_by_keystroke(KeyStroke::command(Key::F)).is_some(),
            "the platform's accelerator must fire a Ctrl-declared shortcut"
        );
    }

    #[test]
    fn literal_modifiers_pins_a_declaration_to_physical_control() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("view.next_tab")
                .literal_modifiers()
                .primary(KeyStroke::ctrl(Key::Tab))
                .build(),
        );
        assert_eq!(
            reg.effective("view.next_tab").unwrap().primary,
            Some(KeyStroke::new(Key::Tab, Modifiers::CTRL)),
            "Ctrl+Tab must stay Ctrl+Tab — ⌘⇥ is the macOS application switcher"
        );
    }

    #[test]
    fn a_declared_super_chord_is_left_alone() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("a")
                .primary(KeyStroke::new(Key::S, Modifiers::SUPER))
                .build(),
        );
        reg.register(
            Shortcut::new("b")
                .primary(KeyStroke::new(Key::B, Modifiers::CTRL | Modifiers::SUPER))
                .build(),
        );
        assert_eq!(
            reg.effective("a").unwrap().primary,
            Some(KeyStroke::new(Key::S, Modifiers::SUPER))
        );
        assert_eq!(
            reg.effective("b").unwrap().primary,
            Some(KeyStroke::new(Key::B, Modifiers::CTRL | Modifiers::SUPER)),
            "Ctrl+Super is a genuine two-modifier chord, not a Ctrl to rewrite"
        );
    }

    #[test]
    fn a_user_override_is_taken_literally() {
        // A chord captured in a settings UI is a statement of intent. Rewriting
        // it would make physical Control unbindable on macOS — and would mean
        // the row the user is looking at fires a chord they did not press.
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("editor.find")
                .primary(KeyStroke::ctrl(Key::F))
                .build(),
        );
        let literal_control = KeyStroke::new(Key::G, Modifiers::CTRL);
        reg.rebind_primary("editor.find", Some(literal_control));
        assert_eq!(
            reg.effective("editor.find").unwrap().primary,
            Some(literal_control)
        );

        // Clearing it hands the slot back to the declared default, convention
        // and all.
        reg.clear_override("editor.find");
        assert_eq!(
            reg.effective("editor.find").unwrap().primary,
            Some(KeyStroke::command(Key::F))
        );
    }

    #[test]
    fn find_conflict_sees_the_resolved_chord() {
        // The settings UI hands `find_conflict` the chord the user just
        // pressed. It must recognise that an accelerator chord collides with a
        // Ctrl-declared default — otherwise a second shortcut can be bound to
        // it silently and both would fire.
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("editor.find")
                .primary(KeyStroke::ctrl(Key::F))
                .build(),
        );
        assert_eq!(
            reg.find_conflict(KeyStroke::command(Key::F), None),
            Some("editor.find")
        );
    }

    #[test]
    fn matches_default_follows_the_convention_and_its_opt_out() {
        let converted = Shortcut::new("a").primary(KeyStroke::ctrl(Key::F)).build();
        assert!(converted.matches_default(KeyStroke::command(Key::F)));

        let literal = Shortcut::new("b")
            .literal_modifiers()
            .primary(KeyStroke::ctrl(Key::Tab))
            .build();
        assert!(literal.matches_default(KeyStroke::new(Key::Tab, Modifiers::CTRL)));
    }

    // -----------------------------------------------------------------------
    // Both branches of the primary-accelerator convention, from either host.
    //
    // Every test above reads the declaration through the *current* platform,
    // so on a Linux CI they compare `Ctrl` against `Ctrl` and the macOS half
    // of the convention is never observed. The `_using` twins take the
    // accelerator explicitly — the same split `common::text_nav` uses for
    // caret motion — so the branch that matters most is pinned everywhere:
    // on macOS a declared `Ctrl` chord resolves *away* from physical ⌃, and
    // so must not fire on it.
    // -----------------------------------------------------------------------

    /// The accelerator macOS carries application commands on.
    const MAC: Modifiers = Modifiers::SUPER;
    /// The accelerator Windows and Linux carry them on.
    const PC: Modifiers = Modifiers::CTRL;

    #[test]
    fn the_mac_branch_moves_a_declared_ctrl_chord_off_physical_control() {
        let save = Shortcut::new("app.save")
            .primary(KeyStroke::ctrl(Key::S))
            .build();
        let physical_control = KeyStroke::new(Key::S, Modifiers::CTRL);

        assert_eq!(
            save.declared_keystrokes_using(MAC).0,
            Some(KeyStroke::new(Key::S, Modifiers::SUPER)),
            "a declared Ctrl chord is the platform accelerator: ⌘S on macOS"
        );
        assert!(
            save.matches_default_using(KeyStroke::new(Key::S, Modifiers::SUPER), MAC),
            "⌘S must fire the shortcut the app declared as Ctrl+S"
        );
        assert!(
            !save.matches_default_using(physical_control, MAC),
            "⌃S must NOT fire it — Control is the macOS text system's, and \
             dispatch matches the resolved chord by equality"
        );
    }

    #[test]
    fn the_pc_branch_leaves_a_declared_ctrl_chord_on_physical_control() {
        // The control case for the test above: off macOS the accelerator *is*
        // Control, so the very chord that misses there is the one that hits.
        let save = Shortcut::new("app.save")
            .primary(KeyStroke::ctrl(Key::S))
            .build();
        let physical_control = KeyStroke::new(Key::S, Modifiers::CTRL);

        assert_eq!(
            save.declared_keystrokes_using(PC).0,
            Some(physical_control),
            "nothing is rewritten where Ctrl already is the accelerator"
        );
        assert!(save.matches_default_using(physical_control, PC));
        assert!(
            !save.matches_default_using(KeyStroke::new(Key::S, Modifiers::SUPER), PC),
            "Super is a distinct modifier on Windows and Linux, not the accelerator"
        );
    }

    #[test]
    fn literal_modifiers_keeps_physical_control_reachable_on_the_mac_branch() {
        // Ctrl+Tab cycles tabs on macOS too — ⌘⇥ is the application switcher
        // and never reaches an app. The opt-out has to hold under the branch
        // that would otherwise rewrite it, which is the one Linux can't see.
        let next_tab = Shortcut::new("view.next_tab")
            .literal_modifiers()
            .primary(KeyStroke::ctrl(Key::Tab))
            .build();

        assert!(
            next_tab.matches_default_using(KeyStroke::new(Key::Tab, Modifiers::CTRL), MAC),
            "literal_modifiers must keep ⌃⇥ firing on macOS"
        );
        assert!(
            !next_tab.matches_default_using(KeyStroke::new(Key::Tab, Modifiers::SUPER), MAC),
            "and must not silently answer to ⌘⇥, which the OS takes first"
        );
    }

    #[test]
    fn an_explicit_super_or_ctrl_super_declaration_survives_the_mac_branch() {
        let super_only = Shortcut::new("a")
            .primary(KeyStroke::new(Key::S, Modifiers::SUPER))
            .build();
        assert_eq!(
            super_only.declared_keystrokes_using(MAC).0,
            Some(KeyStroke::new(Key::S, Modifiers::SUPER)),
            "already the accelerator — the rewrite is idempotent, not additive"
        );

        let both = Shortcut::new("b")
            .primary(KeyStroke::new(Key::B, Modifiers::CTRL | Modifiers::SUPER))
            .build();
        assert_eq!(
            both.declared_keystrokes_using(MAC).0,
            Some(KeyStroke::new(Key::B, Modifiers::CTRL | Modifiers::SUPER)),
            "⌃⌘B is a genuine two-modifier chord, not a Ctrl awaiting rewrite"
        );
    }

    #[test]
    fn the_secondary_chord_follows_the_same_branch() {
        let find = Shortcut::new("editor.find")
            .primary(KeyStroke::ctrl(Key::F))
            .secondary(KeyStroke::ctrl_shift(Key::F))
            .build();
        assert_eq!(
            find.declared_keystrokes_using(MAC).1,
            Some(KeyStroke::new(Key::F, Modifiers::SUPER | Modifiers::SHIFT)),
            "the secondary slot is resolved too, not just the primary"
        );
        assert!(
            !find.matches_default_using(
                KeyStroke::new(Key::F, Modifiers::CTRL | Modifiers::SHIFT),
                MAC
            ),
            "⌃⇧F must miss on macOS for the same reason ⌃F does"
        );
    }

    #[test]
    fn the_registry_dispatches_the_current_platform_accelerator_and_only_that() {
        // The end-to-end half: `resolved_keystrokes` is the single place the
        // convention enters the registry, and everything downstream compares
        // against what it returns. Asserting through `find_by_keystroke` pins
        // that composition — expectations differ by host precisely because the
        // behaviour does.
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        assert!(
            reg.find_by_keystroke(KeyStroke::command(Key::S)).is_some(),
            "the platform accelerator must fire a Ctrl-declared shortcut"
        );
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::new(Key::S, Modifiers::CTRL))
                .is_some(),
            !cfg!(target_os = "macos"),
            "physical Control fires it only where Control is the accelerator; \
             on macOS the chord is ⌘S and ⌃S belongs to the text system"
        );
    }
}
