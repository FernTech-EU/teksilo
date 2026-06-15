// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Free functions the `tr!` / `tr_widget!` macros expand into.
//!
//! Each function reaches the active `I18nManager` through the thread-local
//! and delegates to its `resolve_app` / `resolve_widget` method. If no
//! manager is installed, returns the literal key as a placeholder.

use fluent_bundle::FluentValue;

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
}
