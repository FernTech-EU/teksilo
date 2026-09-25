// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Free functions the `tr!` / `tr_widget!` macros expand into.
//!
//! Each function reaches the active `I18nManager` through the thread-local
//! and delegates to its `resolve_app` / `resolve_widget` method. If no
//! manager is installed, returns the literal key as a placeholder; the
//! macro's expansion then puts the source-language text in its place (see
//! [`format_source_fallback`]).

use std::cell::RefCell;
use std::collections::HashMap;

use fluent_bundle::{FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

use crate::manager::{configure_bundle, format_message};
use crate::thread_local::with_active;

/// Runtime entry point for `tr!`. Resolves an application string against
/// the active locale's bundle, falling back to the source locale.
pub fn resolve_message(key: &str, args: &[(&str, FluentValue<'_>)]) -> String {
    with_active(|mgr| mgr.resolve_app(key, args)).unwrap_or_else(|| key.to_string())
}

/// Runtime entry point for `tr_widget!`. Resolves a framework string,
/// applying the app-override → framework lookup precedence.
pub fn resolve_message_widget(key: &str, args: &[(&str, FluentValue<'_>)]) -> String {
    with_active(|mgr| mgr.resolve_widget(key, args)).unwrap_or_else(|| key.to_string())
}

/// A bundle per (language, message source).
type SourceBundles = HashMap<(&'static str, &'static str), FluentBundle<FluentResource>>;

thread_local! {
    /// One bundle per message source and language, built the first time the
    /// message falls back and kept, since a message that falls back once
    /// does so every time it is said.
    static SOURCE_BUNDLES: RefCell<SourceBundles> = RefCell::new(HashMap::new());
}

/// Format `key` from `source`, its own Fluent source, in `language`, the
/// language the source is written in.
///
/// What a `tr!` expansion of a message with a selector, a plural, a function
/// call or a reference returns when nothing resolved it: no `I18nManager` is
/// installed, or its bundles lack the key. The macro cannot rebuild such a
/// message from its parts, as it does a plain one, so it hands over the
/// message's source with every entry it refers to, and this formats it the
/// way the manager would have. Returns `key` if the source does not define
/// it.
///
/// Not called by hand: the macros emit the call.
#[doc(hidden)]
pub fn format_source_fallback(
    language: &'static str,
    source: &'static str,
    key: &str,
    args: &[(&str, FluentValue<'_>)],
) -> String {
    SOURCE_BUNDLES.with(|bundles| {
        let mut bundles = bundles.borrow_mut();
        let bundle = bundles.entry((language, source)).or_insert_with(|| {
            let language: LanguageIdentifier = language
                .parse()
                .unwrap_or_else(|_| "en-US".parse().expect("en-US is a language tag"));
            let mut bundle = FluentBundle::new(vec![language]);
            configure_bundle(&mut bundle);
            match FluentResource::try_new(source.to_string()) {
                Ok(resource) => {
                    if let Err(errs) = bundle.add_resource(resource) {
                        eprintln!("teksilo-i18n: errors adding the source of `{key}`: {errs:?}");
                    }
                }
                Err((_, errs)) => {
                    eprintln!("teksilo-i18n: parse errors in the source of `{key}`: {errs:?}");
                }
            }
            bundle
        });
        format_message(bundle, key, args).unwrap_or_else(|| key.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::I18nConfig;
    use crate::manager::I18nManager;
    use crate::thread_local::{clear, install};

    #[test]
    fn no_manager_returns_key() {
        clear();
        assert_eq!(resolve_message("missing", &[]), "missing");
    }

    #[test]
    fn with_manager_resolves_through_active_locale() {
        clear();
        let cfg = I18nConfig::test_only("en-US", &[("greeting", "Hello")]);
        install(I18nManager::from_config(&cfg));
        assert_eq!(resolve_message("greeting", &[]), "Hello");
        clear();
    }

    #[test]
    fn a_source_fallback_picks_the_plural_of_its_own_language() {
        let source =
            "files =\n    { $n ->\n        [one] One file\n       *[other] { $n } files\n    }\n";
        let say = |n: i64| format_source_fallback("en-US", source, "files", &[("n", n.into())]);
        assert_eq!(say(1), "One file");
        assert_eq!(say(1200), "1,200 files");
    }

    #[test]
    fn a_source_that_lacks_the_key_gives_the_key() {
        assert_eq!(
            format_source_fallback("en-US", "other = Other\n", "missing", &[]),
            "missing"
        );
    }
}
