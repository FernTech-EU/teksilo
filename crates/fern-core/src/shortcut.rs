//! User-facing rebindable keyboard shortcuts.
//!
//! The shortcut system has three layers:
//!
//! - [`KeyStroke`] — a single keyboard chord (key + modifiers).
//! - [`Shortcut`] — a first-class, rebindable record with a stable
//!   string id, localizable metadata, one or two keystrokes, a scope,
//!   and an `on_activate` closure that produces an
//!   [`Intent`](crate::intent::Intent) at activation time.
//! - [`ShortcutRegistry`] — a two-layer store: widget-declared
//!   defaults (refreshed every build) plus persisted user overrides.
//!   The effective view merges them.
//!
//! Dispatch (wired in a later step): a keystroke is looked up in the
//! registry, the matching shortcut's `on_activate` produces an intent,
//! and the framework walks **source-widget → root** invoking
//! [`Action`](crate::action::Action) handlers along the way.

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
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct KeyStroke {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl KeyStroke {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    pub fn ctrl(key: Key) -> Self {
        Self::new(key, Modifiers::CTRL)
    }

    pub fn ctrl_shift(key: Key) -> Self {
        Self::new(key, Modifiers::CTRL | Modifiers::SHIFT)
    }

    pub fn alt(key: Key) -> Self {
        Self::new(key, Modifiers::ALT)
    }
}

impl fmt::Display for KeyStroke {
    // Plain "Ctrl+S" form. Widgets that display shortcuts to users should
    // use `fern_widgets::keystroke_format::format_keystroke()` instead,
    // which handles platform-specific symbols (⌘ on macOS) and locale-
    // aware modifier names ("Strg" in German) via fern-i18n.
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
/// Runs when the next [`KeyDown`] bypasses shortcut resolution.
/// Receives the captured keystroke, mutable access to the registry
/// (for rebinds), and a mutable [`EventContext`] (so the handler can
/// emit commands, send intents, dismiss overlays, etc.).
pub type KeyCaptureCallback =
    Box<dyn FnOnce(KeyStroke, &mut ShortcutRegistry, &mut EventContext)>;

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
/// locale changes through a `Signal<String>` without fern-core
/// depending on fern-i18n. Apps convert their `LocalizedString`
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
    pub enabled_when: Option<Signal<bool>>,
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
            .finish()
    }
}

impl Shortcut {
    /// Start building a shortcut with a stable id.
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
    pub fn matches_default(&self, keystroke: KeyStroke) -> bool {
        self.primary == Some(keystroke) || self.secondary == Some(keystroke)
    }
}

/// Fluent builder for [`Shortcut`]. Default scope is `Global`; use
/// [`ShortcutBuilder::scope`] or [`ShortcutBuilder::scope_to`] for a
/// scoped shortcut (the typical widget-declared case).
pub struct ShortcutBuilder {
    inner: Shortcut,
}

impl ShortcutBuilder {
    /// Static user-visible label.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.inner.name = Prop::Static(name.into());
        self
    }

    /// Reactive user-visible label (for localized names driven by a
    /// `Signal<String>` fed from fern-i18n's `LocalizedString`).
    pub fn bind_name(mut self, name: impl Into<Prop<String>>) -> Self {
        self.inner.name = name.into();
        self
    }

    pub fn category(mut self, category: &'static str) -> Self {
        self.inner.category = Some(category);
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.inner.description = Some(Prop::Static(description.into()));
        self
    }

    pub fn bind_description(mut self, description: impl Into<Prop<String>>) -> Self {
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
    pub fn enabled_when(mut self, signal: Signal<bool>) -> Self {
        self.inner.enabled_when = Some(signal);
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
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
)]
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
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
)]
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
        }
    }

    /// A reactive handle that ticks on every mutation (register,
    /// unregister, rebind, put_override). Menus and settings widgets
    /// observe it to refresh derived state.
    pub fn version(&self) -> &Signal<u64> {
        &self.version
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
        previous
    }

    /// Upsert a shortcut default owned by `owner`. When `owner` is
    /// destroyed, the framework calls
    /// [`ShortcutRegistry::unregister_all_for_owner`] to remove this
    /// registration. Preserves user overrides identically to
    /// [`ShortcutRegistry::register`].
    pub fn register_owned(
        &mut self,
        shortcut: Shortcut,
        owner: WidgetId,
    ) -> Option<Shortcut> {
        let id = shortcut.id;
        let previous = self.defaults.insert(id, shortcut);
        self.detach_owner_index(id);
        self.by_owner.entry(owner).or_default().push(id);
        self.owner_by_id.insert(id, owner);
        self.bump_version();
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
        self.overrides.insert(id.into(), override_);
        self.bump_version();
    }

    /// Set the primary slot of the override for `id`. The secondary
    /// slot is left untouched — with per-slot [`SlotOverride`]
    /// semantics the untouched slot continues to delegate to
    /// whatever default the shortcut currently declares.
    pub fn rebind_primary(&mut self, id: impl Into<String>, keystroke: Option<KeyStroke>) {
        let id = id.into();
        let entry = self.overrides.entry(id).or_default();
        entry.primary = match keystroke {
            Some(ks) => SlotOverride::Bound(ks),
            None => SlotOverride::Unbound,
        };
        self.bump_version();
    }

    /// Set the secondary slot of the override for `id`. The primary
    /// slot stays in whatever state it was (`Default` or user-set).
    pub fn rebind_secondary(&mut self, id: impl Into<String>, keystroke: Option<KeyStroke>) {
        let id = id.into();
        let entry = self.overrides.entry(id).or_default();
        entry.secondary = match keystroke {
            Some(ks) => SlotOverride::Bound(ks),
            None => SlotOverride::Unbound,
        };
        self.bump_version();
    }

    /// Drop the user override for `id`, restoring the declared defaults.
    pub fn clear_override(&mut self, id: &str) {
        if self.overrides.remove(id).is_some() {
            self.bump_version();
        }
    }

    /// Clear every override. Restores the declared defaults for all
    /// registered shortcuts. Graveyard entries are dropped too.
    pub fn clear_all_overrides(&mut self) {
        if !self.overrides.is_empty() {
            self.overrides.clear();
            self.bump_version();
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

    /// Find the first shortcut id currently bound to `keystroke`,
    /// excluding `excluding_id` if given. Used by settings UIs to
    /// auto-unbind conflicts when the user rebinds a chord. Includes
    /// disabled shortcuts — a chord is "taken" regardless of whether
    /// its current binding is live.
    pub fn find_conflict(
        &self,
        keystroke: KeyStroke,
        excluding_id: Option<&str>,
    ) -> Option<&'static str> {
        self.defaults.iter().find_map(|(&id, shortcut)| {
            if excluding_id == Some(id) {
                return None;
            }
            let (primary, secondary) = self.resolved_keystrokes(id, shortcut);
            if primary == Some(keystroke) || secondary == Some(keystroke) {
                Some(id)
            } else {
                None
            }
        })
    }

    /// First effective shortcut whose primary or secondary keystroke
    /// matches **and** is currently enabled. Disabled shortcuts are
    /// invisible to the dispatcher — the keystroke falls through to
    /// the focused widget's normal `on_key` handling, matching the
    /// "treated as if not registered" semantic advertised by
    /// [`Shortcut::enabled_when`].
    pub fn find_by_keystroke(&self, keystroke: KeyStroke) -> Option<EffectiveShortcut<'_>> {
        self.iter_effective()
            .find(|s| s.enabled && s.matches(keystroke))
    }

    fn resolved_keystrokes(
        &self,
        id: &str,
        shortcut: &Shortcut,
    ) -> (Option<KeyStroke>, Option<KeyStroke>) {
        let ov = self.overrides.get(id).copied().unwrap_or_default();
        (
            ov.primary.resolve(shortcut.primary),
            ov.secondary.resolve(shortcut.secondary),
        )
    }

    fn bump_version(&self) {
        self.version.set(self.version.get().wrapping_add(1));
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

        let e = Shortcut::new("baz").scope(ShortcutScope::Scoped(id)).build();
        assert_eq!(e.scope, ShortcutScope::Scoped(id));

        let back = Shortcut::new("qux").scope_to(id).global().build();
        assert_eq!(back.scope, ShortcutScope::Global);
    }

    #[test]
    fn shortcut_matches_default_primary_and_secondary() {
        let s = Shortcut::new("edit.undo")
            .primary(KeyStroke::ctrl(Key::Z))
            .secondary(KeyStroke::alt(Key::Backspace))
            .build();
        assert!(s.matches_default(KeyStroke::ctrl(Key::Z)));
        assert!(s.matches_default(KeyStroke::alt(Key::Backspace)));
        assert!(!s.matches_default(KeyStroke::ctrl(Key::Y)));
    }

    // --- Registry: upsert preserves user overrides ---------------------

    #[test]
    fn register_upserts_and_preserves_override() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        // User rebinds Ctrl+S → Ctrl+Shift+S.
        reg.rebind_primary("app.save", Some(KeyStroke::ctrl_shift(Key::S)));
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            Some(KeyStroke::ctrl_shift(Key::S))
        );

        // Widget rebuilds, re-registers with the same defaults. The
        // user override must survive.
        reg.register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            Some(KeyStroke::ctrl_shift(Key::S))
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
        assert_eq!(eff.primary, Some(KeyStroke::ctrl_shift(Key::S)));
        assert_eq!(eff.shortcut.name.get(), "Save (renamed)");
    }

    #[test]
    fn clear_override_restores_default() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        reg.rebind_primary("app.save", Some(KeyStroke::ctrl_shift(Key::S)));
        reg.clear_override("app.save");
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            Some(KeyStroke::ctrl(Key::S))
        );
    }

    // --- Registry: graveyard -------------------------------------------

    #[test]
    fn override_survives_unregister_and_reregister() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("editor.format.bold")
                .primary(KeyStroke::ctrl(Key::B))
                .build(),
        );
        reg.rebind_primary(
            "editor.format.bold",
            Some(KeyStroke::ctrl_shift(Key::B)),
        );

        reg.unregister("editor.format.bold");
        assert!(reg.effective("editor.format.bold").is_none());
        assert_eq!(
            reg.override_for("editor.format.bold").unwrap().primary,
            SlotOverride::Bound(KeyStroke::ctrl_shift(Key::B))
        );

        reg.register(
            Shortcut::new("editor.format.bold")
                .primary(KeyStroke::ctrl(Key::B))
                .build(),
        );
        assert_eq!(
            reg.effective("editor.format.bold").unwrap().primary,
            Some(KeyStroke::ctrl_shift(Key::B))
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

        reg.rebind_primary("a", Some(KeyStroke::ctrl(Key::A)));
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

    // --- Registry: find_by_keystroke honors overrides -----------------

    #[test]
    fn find_by_keystroke_uses_effective() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::ctrl(Key::S))
                .map(|s| s.shortcut.id),
            Some("app.save")
        );

        reg.rebind_primary("app.save", Some(KeyStroke::ctrl_shift(Key::S)));

        assert!(reg.find_by_keystroke(KeyStroke::ctrl(Key::S)).is_none());
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::ctrl_shift(Key::S))
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
                .primary(KeyStroke::ctrl(Key::B))
                .build(),
            editor,
        );
        reg.register_owned(
            Shortcut::new("editor.format.italic")
                .primary(KeyStroke::ctrl(Key::I))
                .build(),
            editor,
        );
        reg.register_owned(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
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
        let s = Shortcut::new("foo")
            .enabled_when(enabled.clone())
            .build();
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
                .primary(KeyStroke::ctrl(Key::S))
                .enabled_when(enabled.clone())
                .build(),
        );
        // Disabled → invisible to dispatch.
        assert!(reg.find_by_keystroke(KeyStroke::ctrl(Key::S)).is_none());

        // Enable → dispatch sees it.
        enabled.set(true);
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::ctrl(Key::S))
                .map(|s| s.shortcut.id),
            Some("app.save")
        );
    }

    #[test]
    fn overrides_round_trip_through_export_import() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        reg.rebind_primary("app.save", Some(KeyStroke::ctrl_shift(Key::S)));

        let snapshot = reg.export_overrides();
        assert_eq!(snapshot.len(), 1);

        // Fresh registry, same defaults but no overrides yet.
        let mut reg2 = ShortcutRegistry::new();
        reg2.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        assert_eq!(
            reg2.effective("app.save").unwrap().primary,
            Some(KeyStroke::ctrl(Key::S))
        );

        reg2.import_overrides(snapshot);
        assert_eq!(
            reg2.effective("app.save").unwrap().primary,
            Some(KeyStroke::ctrl_shift(Key::S))
        );
    }

    #[test]
    fn clear_all_overrides_restores_every_default() {
        let mut reg = ShortcutRegistry::new();
        reg.register(Shortcut::new("a").primary(KeyStroke::ctrl(Key::A)).build());
        reg.register(Shortcut::new("b").primary(KeyStroke::ctrl(Key::B)).build());
        reg.rebind_primary("a", Some(KeyStroke::alt(Key::A)));
        reg.rebind_primary("b", Some(KeyStroke::alt(Key::B)));

        reg.clear_all_overrides();
        assert_eq!(
            reg.effective("a").unwrap().primary,
            Some(KeyStroke::ctrl(Key::A))
        );
        assert_eq!(
            reg.effective("b").unwrap().primary,
            Some(KeyStroke::ctrl(Key::B))
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
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );

        reg.rebind_primary("foo", Some(KeyStroke::ctrl_shift(Key::S)));

        // Widget re-registers with a NEW default secondary. The
        // untouched secondary slot must pick this up — that is the
        // whole point of per-slot Default delegation.
        reg.register(
            Shortcut::new("foo")
                .primary(KeyStroke::ctrl(Key::S))
                .secondary(KeyStroke::alt(Key::S))
                .build(),
        );

        let eff = reg.effective("foo").unwrap();
        assert_eq!(eff.primary, Some(KeyStroke::ctrl_shift(Key::S)));
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
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        reg.rebind_primary("foo", None);
        assert_eq!(reg.effective("foo").unwrap().primary, None);

        // `clear_override` goes back to default.
        reg.clear_override("foo");
        assert_eq!(
            reg.effective("foo").unwrap().primary,
            Some(KeyStroke::ctrl(Key::S))
        );
    }

    #[test]
    fn find_conflict_skips_excluded_id_and_respects_overrides() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("a")
                .primary(KeyStroke::ctrl(Key::X))
                .build(),
        );
        reg.register(
            Shortcut::new("b")
                .primary(KeyStroke::ctrl(Key::Y))
                .build(),
        );

        // Ctrl+X is bound to "a"; looking for it while excluding "a"
        // returns None, including it returns Some("a").
        assert_eq!(
            reg.find_conflict(KeyStroke::ctrl(Key::X), Some("a")),
            None
        );
        assert_eq!(
            reg.find_conflict(KeyStroke::ctrl(Key::X), None),
            Some("a")
        );
        // Ctrl+Z is bound to nothing.
        assert_eq!(reg.find_conflict(KeyStroke::ctrl(Key::Z), None), None);

        // User rebinds "b" to Ctrl+X — that becomes the new conflict
        // for Ctrl+X (overrides outrank defaults).
        reg.rebind_primary("b", Some(KeyStroke::ctrl(Key::X)));
        assert_eq!(
            reg.find_conflict(KeyStroke::ctrl(Key::X), Some("b")),
            Some("a")
        );
        assert_eq!(
            reg.find_conflict(KeyStroke::ctrl(Key::X), Some("a")),
            Some("b")
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
                .primary(KeyStroke::ctrl(Key::S))
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
                .primary(KeyStroke::ctrl(Key::Z))
                .secondary(KeyStroke::alt(Key::Backspace))
                .build(),
        );
        assert_eq!(
            reg.find_by_keystroke(KeyStroke::alt(Key::Backspace))
                .map(|s| s.shortcut.id),
            Some("edit.undo")
        );
    }
}
