//! Tooltip content registry.
//!
//! A central map from **tooltip keys** (short identifiers) to
//! [`TooltipContent`] — the translatable strings + optional shortcut
//! metadata that rich-tooltip widgets resolve at hover time.
//!
//! Two entry points:
//! - [`TooltipContent::new`] (plus `.with_more` / `.with_shortcut_label`
//!   / `.with_command`) builds an entry at app boot.
//! - [`install_tooltip_registry`] freezes a `Vec<TooltipContent>` into a
//!   thread-local registry that the tooltip widget reads from.
//!
//! The registry is populated once by
//! `FernAppBuilder::register_tooltips(...)` before the first frame
//! builds and is read-only for the rest of the process lifetime.
//!
//! # URL scheme
//!
//! Inline links inside tooltip body text address other tooltip entries
//! via the `:key` URL prefix. A link written as `[2 minutes](:autosave-details)`
//! in a translated string becomes a hover-trigger for the tooltip
//! registered under the `"autosave-details"` key. Use
//! [`TooltipRegistry::parse_url`] to recognize `:key` URLs; every other
//! URL scheme (`http://`, `mailto:`, …) passes through unmodified.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::rc::Rc;

use fern_core::app_command::AppCommand;
use fern_i18n::LocalizedString;

/// One tooltip content entry.
///
/// Every tooltip may optionally carry a long-form "more" body (revealed
/// by the Accordion disclosure inside a sticky rich tooltip) and a
/// keyboard shortcut hint.
///
/// The shortcut follows the same pattern as [`MenuItem`]
/// (`crates/fern-widgets/src/menu_item.rs`): either a literal label
/// override, **or** a type-erased [`AppCommand`] whose current binding
/// is looked up in the live `ShortcutMap` at render time via
/// `BuildContext::shortcut_label_for_any`. The literal override wins
/// when both are set.
///
/// [`MenuItem`]: crate::menu_item::MenuItem
pub struct TooltipContent {
    /// Stable identifier. Referenced from link targets as `[label](:key)`.
    pub key: String,
    /// Primary body — rendered through TextWidget with inline markup
    /// enabled, so it may contain further `[label](:other-key)` links.
    pub text: LocalizedString,
    /// Optional long-form content, revealed by the Accordion disclosure
    /// inside a sticky tooltip. Same inline-markup pipeline as `text`.
    pub more: Option<LocalizedString>,
    /// Manual shortcut label override (e.g. "Ctrl+Shift+S"). Used
    /// verbatim when set.
    pub shortcut_label: Option<String>,
    /// Type-erased command reference. When set and `shortcut_label`
    /// isn't, the tooltip widget reverse-looks-up the current binding
    /// in the ShortcutMap — the same `ctx.shortcut_label_for_any` API
    /// MenuItem uses.
    ///
    /// Stored as `Rc<dyn Any>` so `TooltipContent` clones cheaply out
    /// of the registry on every hover.
    pub command: Option<Rc<dyn std::any::Any>>,
}

impl std::fmt::Debug for TooltipContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TooltipContent")
            .field("key", &self.key)
            .field("has_more", &self.more.is_some())
            .field("shortcut_label", &self.shortcut_label)
            .field("has_command", &self.command.is_some())
            .finish()
    }
}

impl Clone for TooltipContent {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            text: self.text.clone(),
            more: self.more.clone(),
            shortcut_label: self.shortcut_label.clone(),
            command: self.command.clone(),
        }
    }
}

impl TooltipContent {
    /// Build an entry with a primary body and no extras.
    pub fn new(key: impl Into<String>, text: LocalizedString) -> Self {
        Self {
            key: key.into(),
            text,
            more: None,
            shortcut_label: None,
            command: None,
        }
    }

    /// Attach a long-form body revealed by the Accordion disclosure.
    pub fn with_more(mut self, more: LocalizedString) -> Self {
        self.more = Some(more);
        self
    }

    /// Attach a manual shortcut label override ("Ctrl+Shift+S",
    /// "Hold ⇧ + drag", …). Takes precedence over command-based lookup.
    pub fn with_shortcut_label(mut self, s: impl Into<String>) -> Self {
        self.shortcut_label = Some(s.into());
        self
    }

    /// Bind the tooltip to a command so its shortcut label reflects the
    /// live [`ShortcutMap`](fern_core::shortcut::ShortcutMap) binding at
    /// render time. The command is stored type-erased so
    /// `TooltipContent` isn't generic over the app's command enum.
    pub fn with_command<C: AppCommand + 'static>(mut self, cmd: C) -> Self {
        self.command = Some(Rc::new(cmd));
        self
    }

    pub fn has_more(&self) -> bool {
        self.more.is_some()
    }

    pub fn has_shortcut(&self) -> bool {
        self.shortcut_label.is_some() || self.command.is_some()
    }
}

/// Frozen, read-only registry keyed by tooltip id.
#[derive(Default)]
pub struct TooltipRegistry {
    by_key: HashMap<String, TooltipContent>,
}

impl TooltipRegistry {
    /// Look up an entry by its stable key.
    pub fn get(&self, key: &str) -> Option<&TooltipContent> {
        self.by_key.get(key)
    }

    /// Parse a link URL as a tooltip key. Returns `Some(key)` when the
    /// URL starts with the `:` prefix, `None` otherwise (so ordinary
    /// http / mailto links pass through unmodified).
    pub fn parse_url(url: &str) -> Option<&str> {
        url.strip_prefix(':')
    }

    /// Resolve a link URL straight to a content entry, if the URL is a
    /// tooltip key and the key is registered.
    pub fn resolve_url(&self, url: &str) -> Option<&TooltipContent> {
        Self::parse_url(url).and_then(|k| self.get(k))
    }

    /// Iterate every registered entry. Useful for diagnostics and tests.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &TooltipContent)> {
        self.by_key.iter()
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

// Thread-local storage. The registry is installed once by
// `install_tooltip_registry` from `FernAppBuilder::register_tooltips`
// at app boot and is read-only afterwards. Using `OnceCell` (rather
// than `RefCell<Option<_>>`) enforces the "install once" invariant at
// the type level — double-install panics.
thread_local! {
    static TOOLTIP_REGISTRY: OnceCell<TooltipRegistry> = const { OnceCell::new() };
}

/// Install the tooltip registry for the current thread. Called once
/// by [`FernAppBuilder::register_tooltips`] before the first frame
/// builds. Panics in debug builds on double-install; logs and keeps
/// the first installation in release.
///
/// [`FernAppBuilder::register_tooltips`]: fern_app::FernAppBuilder::register_tooltips
pub fn install_tooltip_registry(contents: Vec<TooltipContent>) {
    let reg = TooltipRegistry {
        by_key: contents.into_iter().map(|c| (c.key.clone(), c)).collect(),
    };
    TOOLTIP_REGISTRY.with(|cell| {
        if cell.set(reg).is_err() {
            #[cfg(debug_assertions)]
            panic!("tooltip registry already installed");
        }
    });
}

/// Read-side helper used by the tooltip widget. Runs `f` with a
/// borrowed reference to the installed registry. Returns `None` if no
/// registry has been installed yet (early bootstrap, headless tests).
pub fn with_tooltip_registry<R>(f: impl FnOnce(&TooltipRegistry) -> R) -> Option<R> {
    TOOLTIP_REGISTRY.with(|cell| cell.get().map(f))
}

/// Test-only helper: reset the thread-local registry. Not exposed in
/// release builds — tests clone-install-read then move on.
#[cfg(test)]
pub(crate) fn _reset_tooltip_registry() {
    TOOLTIP_REGISTRY.with(|cell| {
        // Cheap trick: take() would require Fn(&mut OnceCell), which
        // the std API doesn't offer. Instead we leak by overwriting.
        let new_cell: OnceCell<TooltipRegistry> = OnceCell::new();
        // Safety: we're inside a test and no other code on this thread
        // is holding a reference to the registry contents at this
        // point (tests run serially per thread local).
        #[allow(invalid_reference_casting)]
        unsafe {
            let ptr = cell as *const OnceCell<TooltipRegistry>
                as *mut OnceCell<TooltipRegistry>;
            std::ptr::write(ptr, new_cell);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_recognizes_colon_prefix() {
        assert_eq!(TooltipRegistry::parse_url(":foo"), Some("foo"));
        assert_eq!(TooltipRegistry::parse_url(":autosave-details"), Some("autosave-details"));
    }

    #[test]
    fn parse_url_rejects_non_tooltip_schemes() {
        assert_eq!(TooltipRegistry::parse_url("http://example.com"), None);
        assert_eq!(TooltipRegistry::parse_url("mailto:foo@bar"), None);
        assert_eq!(TooltipRegistry::parse_url(""), None);
        assert_eq!(TooltipRegistry::parse_url("autosave"), None);
    }

    #[test]
    fn parse_url_empty_key_is_some_empty() {
        // Caller is responsible for rejecting empty keys.
        assert_eq!(TooltipRegistry::parse_url(":"), Some(""));
    }

    #[test]
    fn content_builder_chain() {
        let c = TooltipContent::new("save-as", LocalizedString::literal("Save the file as…"))
            .with_shortcut_label("Ctrl+Shift+S");
        assert_eq!(c.key, "save-as");
        assert!(!c.has_more());
        assert!(c.has_shortcut());
        assert_eq!(c.shortcut_label.as_deref(), Some("Ctrl+Shift+S"));
    }

    #[test]
    fn content_with_more_sets_more() {
        let c = TooltipContent::new("autosave", LocalizedString::literal("Autosaves."))
            .with_more(LocalizedString::literal("Every 2 minutes."));
        assert!(c.has_more());
    }

    #[test]
    fn register_and_lookup_roundtrip() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![
            TooltipContent::new("foo", LocalizedString::literal("Foo body")),
            TooltipContent::new("bar", LocalizedString::literal("Bar body"))
                .with_shortcut_label("Ctrl+B"),
        ]);

        let found = with_tooltip_registry(|r| {
            assert_eq!(r.len(), 2);
            let foo = r.get("foo").expect("foo registered");
            assert_eq!(foo.key, "foo");
            let bar = r.get("bar").expect("bar registered");
            assert_eq!(bar.shortcut_label.as_deref(), Some("Ctrl+B"));
            "ok"
        });
        assert_eq!(found, Some("ok"));

        _reset_tooltip_registry();
    }

    #[test]
    fn resolve_url_returns_content_for_registered_key() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![
            TooltipContent::new("docs", LocalizedString::literal("Documentation")),
        ]);

        let body = with_tooltip_registry(|r| {
            r.resolve_url(":docs").map(|c| c.text.resolve_now())
        })
        .flatten();
        assert_eq!(body.as_deref(), Some("Documentation"));

        let missing = with_tooltip_registry(|r| r.resolve_url(":nope").is_some());
        assert_eq!(missing, Some(false));

        let non_tooltip = with_tooltip_registry(|r| r.resolve_url("http://x").is_some());
        assert_eq!(non_tooltip, Some(false));

        _reset_tooltip_registry();
    }

    #[test]
    fn with_command_stores_type_erased_ref() {
        #[derive(Debug, Clone, PartialEq)]
        enum MyCmd {
            Save,
        }
        impl AppCommand for MyCmd {}

        let c = TooltipContent::new("save", LocalizedString::literal("Save"))
            .with_command(MyCmd::Save);
        assert!(c.command.is_some());
        assert!(c.has_shortcut());

        // Downcast back through Any to verify the type-erased payload.
        let cmd_any = c.command.as_ref().unwrap();
        let downcast = cmd_any.downcast_ref::<MyCmd>();
        assert_eq!(downcast, Some(&MyCmd::Save));
    }
}
