//! Tooltip content registry.
//!
//! A central map from **tooltip keys** (short identifiers) to
//! [`TooltipContent`] — the translatable strings + optional shortcut
//! metadata that rich-tooltip widgets resolve at hover time.
//!
//! Two entry points:
//! - [`TooltipContent::new`] (plus `.with_more` / `.with_shortcut_label`)
//!   builds an entry at app boot.
//! - [`install_tooltip_registry`] freezes a `Vec<TooltipContent>` into a
//!   thread-local registry that the tooltip widget reads from.
//!
//! The registry is populated once by
//! `BastydeAppBuilder::register_tooltips(...)` before the first frame
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

use std::cell::RefCell;
use std::collections::HashMap;

use bastyde_i18n::LocalizedString;

/// One tooltip content entry.
///
/// Every tooltip may optionally carry a long-form "more" body (revealed
/// by the Accordion disclosure inside a sticky rich tooltip) and a
/// keyboard shortcut hint.
///
/// The shortcut is currently a literal label override. Registry-backed
/// auto-lookup against the new [`ShortcutRegistry`] lands via a
/// shortcut-id field once registry-backed lookup is wired.
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
    /// verbatim when set; takes precedence over [`shortcut_id`].
    pub shortcut_label: Option<String>,
    /// Registered shortcut id — the tooltip renders the effective
    /// primary keystroke from the tree's
    /// [`ShortcutRegistry`](bastyde_core::shortcut::ShortcutRegistry) and
    /// tracks user rebinds automatically.
    pub shortcut_id: Option<&'static str>,
}

impl std::fmt::Debug for TooltipContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TooltipContent")
            .field("key", &self.key)
            .field("has_more", &self.more.is_some())
            .field("shortcut_label", &self.shortcut_label)
            .field("shortcut_id", &self.shortcut_id)
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
            shortcut_id: self.shortcut_id,
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
            shortcut_id: None,
        }
    }

    /// Attach a long-form body revealed by the Accordion disclosure.
    pub fn with_more(mut self, more: LocalizedString) -> Self {
        self.more = Some(more);
        self
    }

    /// Attach a manual shortcut label override ("Ctrl+Shift+S",
    /// "Hold ⇧ + drag", …). Takes precedence over
    /// [`TooltipContent::for_shortcut`].
    pub fn with_shortcut_label(mut self, s: impl Into<String>) -> Self {
        self.shortcut_label = Some(s.into());
        self
    }

    /// Bind the tooltip's trailing chip to a registered shortcut id.
    /// The tooltip widget reads the effective primary keystroke from
    /// the tree's shortcut registry and refreshes on user rebinds.
    pub fn for_shortcut(mut self, id: &'static str) -> Self {
        self.shortcut_id = Some(id);
        self
    }

    pub fn has_more(&self) -> bool {
        self.more.is_some()
    }

    pub fn has_shortcut(&self) -> bool {
        self.shortcut_label.is_some() || self.shortcut_id.is_some()
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
// `install_tooltip_registry` from `BastydeAppBuilder::register_tooltips`
// at app boot and is read-only afterwards. The "install once" invariant
// is enforced at runtime (debug-build panic on double-install) rather
// than by the cell type, so tests can reset it without `unsafe` — see
// `_reset_tooltip_registry`. Mirrors the `RefCell<Option<_>>` pattern
// used by `bastyde-i18n`'s thread-local manager slot.
thread_local! {
    static TOOLTIP_REGISTRY: RefCell<Option<TooltipRegistry>> = const { RefCell::new(None) };
}

/// Install the tooltip registry for the current thread. Called once
/// by [`BastydeAppBuilder::register_tooltips`] before the first frame
/// builds. Panics in debug builds on double-install; logs and keeps
/// the first installation in release.
///
/// [`BastydeAppBuilder::register_tooltips`]: bastyde_app::BastydeAppBuilder::register_tooltips
pub fn install_tooltip_registry(contents: Vec<TooltipContent>) {
    let reg = TooltipRegistry {
        by_key: contents.into_iter().map(|c| (c.key.clone(), c)).collect(),
    };
    TOOLTIP_REGISTRY.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            // Debug: enforce the install-once invariant. Release: keep
            // the first installation and ignore the second.
            debug_assert!(false, "tooltip registry already installed");
            return;
        }
        *slot = Some(reg);
    });
}

/// Read-side helper used by the tooltip widget. Runs `f` with a
/// borrowed reference to the installed registry. Returns `None` if no
/// registry has been installed yet (early bootstrap, headless tests).
pub fn with_tooltip_registry<R>(f: impl FnOnce(&TooltipRegistry) -> R) -> Option<R> {
    TOOLTIP_REGISTRY.with(|slot| slot.borrow().as_ref().map(f))
}

/// Test-only helper: reset the thread-local registry. Not exposed in
/// release builds — tests clone-install-read then move on.
#[cfg(test)]
pub(crate) fn _reset_tooltip_registry() {
    TOOLTIP_REGISTRY.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_recognizes_colon_prefix() {
        assert_eq!(TooltipRegistry::parse_url(":foo"), Some("foo"));
        assert_eq!(
            TooltipRegistry::parse_url(":autosave-details"),
            Some("autosave-details")
        );
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
        install_tooltip_registry(vec![TooltipContent::new(
            "docs",
            LocalizedString::literal("Documentation"),
        )]);

        let body = with_tooltip_registry(|r| r.resolve_url(":docs").map(|c| c.text.resolve_now()))
            .flatten();
        assert_eq!(body.as_deref(), Some("Documentation"));

        let missing = with_tooltip_registry(|r| r.resolve_url(":nope").is_some());
        assert_eq!(missing, Some(false));

        let non_tooltip = with_tooltip_registry(|r| r.resolve_url("http://x").is_some());
        assert_eq!(non_tooltip, Some(false));

        _reset_tooltip_registry();
    }

    // NOTE: `with_command_stores_type_erased_ref` test removed along
    // with the `command` field; registry-backed shortcut resolution
    // returns once registry-backed shortcut lookup is wired.
}
