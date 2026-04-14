//! `LocalizedString` — the developer-facing handle the `tr!` macro produces.
//!
//! It packages a closure that resolves to a `String`. Calling `to_signal()`
//! turns it into a reactive `Signal<String>` that re-runs the resolver on
//! every translation version increment. The reactivity lives in
//! `to_signal()`, not in the LocalizedString itself.

use std::rc::Rc;

use fern_core::signal::{Prop, Signal};

use crate::thread_local::current_version_signal;

/// A reactive *recipe* for a translatable string. Becomes a live binding
/// only when a widget consumes it via `to_signal()`.
#[derive(Clone)]
pub struct LocalizedString {
    resolver: Rc<dyn Fn() -> String + 'static>,
}

impl std::fmt::Debug for LocalizedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalizedString")
            .field("resolved", &(self.resolver)())
            .finish()
    }
}

/// Construct a `LocalizedString` from a resolver closure. The `tr!` proc
/// macro expands to a call to this function.
pub fn localized<F: Fn() -> String + 'static>(resolver: F) -> LocalizedString {
    LocalizedString {
        resolver: Rc::new(resolver),
    }
}

impl LocalizedString {
    /// Construct a non-translated literal. Used for debug labels, internal
    /// names, and other strings that are intentionally not localized. The
    /// resulting `LocalizedString` does not observe locale changes — its
    /// content is fixed for the lifetime of the application.
    pub fn literal(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            resolver: Rc::new(move || text.clone()),
        }
    }

    /// Resolve the current value once, without producing a reactive signal.
    pub fn resolve_now(&self) -> String {
        (self.resolver)()
    }

    /// Convert into a reactive `Signal<String>`. If a translation manager
    /// is installed on this thread, the signal observes the manager's
    /// version signal and re-resolves on every increment. If no manager
    /// is installed (typical in lower-level widget tests), the signal is
    /// a static snapshot of the current resolution.
    pub fn to_signal(&self) -> Signal<String> {
        let initial = (self.resolver)();
        let signal = Signal::new(initial);

        if let Some(version) = current_version_signal() {
            let resolver = self.resolver.clone();
            let target = signal.clone();
            let handle = version.observe(move |_| {
                target.set((resolver)());
            });
            // Forget the handle so the observer lives for the lifetime
            // of the returned Signal — the binding system / arena is the
            // ultimate owner. Dropping this LocalizedString must not
            // unsubscribe the signal from version updates.
            std::mem::forget(handle);
        }

        signal
    }
}

impl From<String> for LocalizedString {
    fn from(text: String) -> Self {
        Self::literal(text)
    }
}

/// Convert a `LocalizedString` into a `Prop<String>` suitable for binding
/// to a widget's text property. If an `I18nManager` is installed on this
/// thread, the result is `Prop::Bound` around a `Signal<String>` that
/// re-resolves on every locale change. Otherwise it is a static snapshot.
///
/// Widget constructors use this to turn a `tr!(...)` result into their
/// internal `Prop<String>` storage.
impl From<LocalizedString> for Prop<String> {
    fn from(ls: LocalizedString) -> Self {
        match current_version_signal() {
            Some(_) => Prop::Bound(ls.to_signal()),
            None => Prop::Static(ls.resolve_now()),
        }
    }
}

// Note: deliberately no `impl From<&str>` — see §12.3 of the architecture
// document. Untranslated literals must be wrapped explicitly via
// `LocalizedString::literal(...)` or produced by `tr!(...)`, so that a
// grep for those names finds every untranslated string in one pass.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::I18nConfig;
    use crate::manager::I18nManager;
    use crate::thread_local::{clear, install};
    use unic_langid::LanguageIdentifier;

    fn lid(s: &str) -> LanguageIdentifier {
        s.parse().unwrap()
    }

    #[test]
    fn literal_resolves_to_fixed_text() {
        let ls = LocalizedString::literal("Debug");
        assert_eq!(ls.resolve_now(), "Debug");
    }

    #[test]
    fn from_string_produces_literal() {
        let ls: LocalizedString = String::from("hello").into();
        assert_eq!(ls.resolve_now(), "hello");
    }

    #[test]
    fn to_signal_without_manager_returns_static() {
        clear();
        let ls = LocalizedString::literal("Static");
        let sig = ls.to_signal();
        assert_eq!(sig.get(), "Static");
    }

    #[test]
    fn to_signal_observes_version_increments() {
        clear();
        let cfg = I18nConfig::test_only("en-US", &[("greeting", "Hello")])
            .with_locale("fr-FR", &[("greeting", "Bonjour")]);
        let mgr = I18nManager::from_config(&cfg);
        install(mgr.clone());

        let ls = localized(|| {
            crate::resolve::resolve_message("greeting", &[])
        });
        let sig = ls.to_signal();
        assert_eq!(sig.get(), "Hello");

        mgr.set_locale(lid("fr-FR"));
        assert_eq!(sig.get(), "Bonjour");

        mgr.set_locale(lid("en-US"));
        assert_eq!(sig.get(), "Hello");

        clear();
    }
}
