// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `LocalizedString` — the developer-facing handle the `tr!` macro produces.
//!
//! It packages a closure that resolves to a `String`. Calling `to_signal()`
//! turns it into a reactive `Signal<String>` that re-runs the resolver on
//! every translation version increment. The reactivity lives in
//! `to_signal()`, not in the LocalizedString itself.

use std::rc::Rc;

use teksilo_core::signal::{Prop, Signal};

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
    ///
    /// **Lifetime:** the observer registered on the version signal is
    /// tied to the lifetime of the returned `Signal<String>` via
    /// `attach_keepalive`. When the last clone of the returned signal
    /// drops — e.g., when the widget that bound it is destroyed by a
    /// composite rebuild — the stored `ObserverHandle` drops, which
    /// unsubscribes the callback from the version signal. The observer
    /// closure itself captures a `WeakSignal` rather than a strong
    /// clone, so the closure cannot form a reference cycle that keeps
    /// the target alive. Together these two properties make
    /// `to_signal()` leak-free even across many composite rebuilds.
    pub fn to_signal(&self) -> Signal<String> {
        let initial = (self.resolver)();
        let signal = Signal::new(initial);

        if let Some(version) = current_version_signal() {
            let resolver = self.resolver.clone();
            // `Signal::new` always returns a mutable signal, so
            // downgrade can't fail here — but unwrap defensively.
            let weak = signal
                .downgrade()
                .expect("Signal::new returns a mutable signal, downgrade cannot fail");
            let handle = version.observe(move |_| {
                // `upgrade` returns `None` once every strong clone of
                // the target signal has been dropped; at that point
                // the handle is about to be dropped too (it lives in
                // the target's `keepalive`), so this no-op branch
                // only runs for at most one stray callback invocation
                // between the last drop and the handle's own cleanup.
                if let Some(target) = weak.upgrade() {
                    target.set((resolver)());
                }
            });
            signal.attach_keepalive(handle);
        }

        signal
    }
}

// No `From<&str> / From<String> / From<&String> for LocalizedString`.
// Untranslated literals must be wrapped explicitly via `lit!(...)` (or
// `LocalizedString::literal(...)`), and translated strings via `tr!(...)`,
// so a bare string can never silently become an untranslated label and a
// grep for `lit!` / `tr!` finds every UI string in one pass.

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

/// Eagerly resolve a `LocalizedString` into a plain `String`. Equivalent
/// to calling `resolve_now()`. Enables `tr!(...)` to flow through
/// `impl Into<String>`-bounded APIs (e.g. the accessibility override
/// builder methods in `teksilo-core` that can't reference `LocalizedString`
/// directly because of the dependency direction).
///
/// This conversion is not reactive: locale changes after the conversion
/// are not observed by the produced `String`. Re-resolution happens at
/// the next composite rebuild, which re-runs the builder chain that
/// produced the `String`.
impl From<LocalizedString> for String {
    fn from(ls: LocalizedString) -> String {
        ls.resolve_now()
    }
}

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
    fn literal_accepts_owned_string() {
        let ls = LocalizedString::literal(String::from("hello"));
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

        let ls = localized(|| crate::resolve::resolve_message("greeting", &[]));
        let sig = ls.to_signal();
        assert_eq!(sig.get(), "Hello");

        mgr.set_locale(lid("fr-FR"));
        assert_eq!(sig.get(), "Bonjour");

        mgr.set_locale(lid("en-US"));
        assert_eq!(sig.get(), "Hello");

        clear();
    }

    #[test]
    fn to_signal_observer_unsubscribes_when_signal_drops() {
        // Regression test for the C1 memory leak: before the
        // `WeakSignal` + `attach_keepalive` fix, `to_signal()` called
        // `mem::forget` on the observer handle, leaving the target
        // signal alive forever and causing every subsequent locale
        // change to invoke a stale observer. With the fix, dropping
        // the returned signal releases the keepalive, which drops the
        // handle, which detaches the callback from the version
        // signal. We verify this by counting resolver invocations —
        // after dropping the signal, the resolver must not run again.
        clear();
        let cfg = I18nConfig::test_only("en-US", &[("k", "v")]).with_locale("fr-FR", &[("k", "w")]);
        let mgr = I18nManager::from_config(&cfg);
        install(mgr.clone());

        let count = std::rc::Rc::new(std::cell::Cell::new(0_u32));
        let count_for_resolver = count.clone();
        let ls = localized(move || {
            count_for_resolver.set(count_for_resolver.get() + 1);
            crate::resolve::resolve_message("k", &[])
        });

        let signal = ls.to_signal();
        // Initial resolution runs once inside `to_signal()`.
        assert_eq!(count.get(), 1);

        // A locale change triggers the observer → resolver runs again.
        mgr.set_locale(lid("fr-FR"));
        assert_eq!(count.get(), 2);

        // Drop the signal. The observer's `WeakSignal` can no longer
        // upgrade, and the `ObserverHandle` stored in the signal's
        // `keepalive` was freed when the inner dropped — the callback
        // is removed from the version signal's observer list.
        drop(signal);
        drop(ls);

        // Another locale change: the resolver should NOT run again
        // because the observer has been detached.
        mgr.set_locale(lid("en-US"));
        assert_eq!(
            count.get(),
            2,
            "resolver ran after the signal was dropped — observer was not detached"
        );

        clear();
    }
}
