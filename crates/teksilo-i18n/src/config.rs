// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `I18nConfig` — application-facing builder for the i18n runtime.

use std::path::PathBuf;

use unic_langid::LanguageIdentifier;

/// Compile-in entry: locale tag plus one or more `&'static str` Fluent
/// resources. Stored after parsing the public `&[(&str, &[&str])]` slice.
#[derive(Clone, Debug)]
pub(crate) struct CompileInEntry {
    pub locale: LanguageIdentifier,
    pub resources: Vec<&'static str>,
}

/// Inline messages used by `I18nConfig::test_only` / `with_locale`.
#[derive(Clone, Debug)]
pub(crate) struct TestLocaleEntry {
    pub locale: LanguageIdentifier,
    pub messages: Vec<(String, String)>,
}

/// Application configuration for internationalization.
///
/// Built up fluently and passed to `TeksiloAppBuilder::i18n(...)` (Phase B).
/// All fields are `pub(crate)` so the manager can read them without exposing
/// internal state to applications.
#[derive(Clone, Debug)]
pub struct I18nConfig {
    pub(crate) source_locale: LanguageIdentifier,
    pub(crate) supported_locales: Vec<LanguageIdentifier>,
    pub(crate) compile_in: Vec<CompileInEntry>,
    pub(crate) test_messages: Vec<TestLocaleEntry>,
    pub(crate) user_locale: Option<LanguageIdentifier>,
    pub(crate) auto_detect_os: bool,
    pub(crate) fallback_locale: LanguageIdentifier,
    pub(crate) runtime_overrides: Vec<(LanguageIdentifier, PathBuf)>,
    /// Framework bundles supplied by libraries like `teksilo-widgets` via
    /// `framework_locales()`. Kept as raw `'static` slices so the i18n
    /// manager can construct the widget bundle at startup exactly like
    /// the application bundle.
    pub(crate) framework_locales: Vec<(&'static str, &'static [&'static str])>,
    /// Application-supplied overrides for framework strings.
    /// Same shape as `framework_locales`, but loaded into
    /// `I18nManager.widget_overrides` — which is consulted *before* the
    /// framework bundle, so an application's per-locale override takes
    /// priority over whatever teksilo-widgets shipped.
    pub(crate) widget_overrides: Vec<(&'static str, &'static [&'static str])>,
}

impl Default for I18nConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl I18nConfig {
    /// Create a new config with default `en-US` source and fallback locales,
    /// OS auto-detection enabled, and no compiled-in resources.
    pub fn new() -> Self {
        let en_us: LanguageIdentifier = "en-US".parse().expect("en-US is a valid locale");
        Self {
            source_locale: en_us.clone(),
            supported_locales: vec![en_us.clone()],
            compile_in: Vec::new(),
            test_messages: Vec::new(),
            user_locale: None,
            auto_detect_os: true,
            fallback_locale: en_us,
            runtime_overrides: Vec::new(),
            framework_locales: Vec::new(),
            widget_overrides: Vec::new(),
        }
    }

    /// Register a framework bundle alongside the application bundle.
    ///
    /// Typical usage: pass `teksilo_widgets::framework_locales()` so the
    /// widget crate's own translatable strings (accessibility labels,
    /// internal messages resolved via `tr_widget!`) are available at
    /// runtime. Multiple calls accumulate — each slice is registered
    /// independently, so an application can combine teksilo-widgets with
    /// third-party widget libraries.
    pub fn framework_locales(
        mut self,
        slice: &'static [(&'static str, &'static [&'static str])],
    ) -> Self {
        self.framework_locales.extend_from_slice(slice);
        self
    }

    /// Register application overrides for framework strings.
    /// Takes the same `&[(&str, &[&str])]` shape as `compile_in` /
    /// `framework_locales`, but the resulting bundles are consulted
    /// *before* the framework bundle. Use this when the application
    /// wants to retranslate or correct a framework-shipped string for
    /// a specific locale — for example, shipping a Japanese translation
    /// of teksilo-widgets' a11y labels when teksilo-widgets itself only
    /// ships English and French.
    ///
    /// Only the keys the application wants to change need to be
    /// defined in the override slice. Keys not present fall back to
    /// the framework's default.
    pub fn override_widget_strings(
        mut self,
        slice: &'static [(&'static str, &'static [&'static str])],
    ) -> Self {
        self.widget_overrides.extend_from_slice(slice);
        self
    }

    pub fn source_locale(mut self, l: LanguageIdentifier) -> Self {
        self.source_locale = l;
        self
    }

    pub fn supported_locales(mut self, it: impl IntoIterator<Item = LanguageIdentifier>) -> Self {
        self.supported_locales = it.into_iter().collect();
        self
    }

    /// Register compiled-in Fluent resources: per-locale arrays of `.ftl`
    /// source strings (typically `include_str!` outputs).
    ///
    /// **Multiple calls accumulate**, exactly like [`Self::framework_locales`],
    /// and repeats of the *same* locale are merged into that locale's single
    /// bundle by [`crate::I18nManager::from_config`]. That is what lets an
    /// application compose its own catalogue with catalogues supplied by
    /// plugins, extensions or sibling crates, each shipping its own `.ftl`
    /// files, without any of them having to know about the others:
    ///
    /// ```ignore
    /// let mut cfg = I18nConfig::new().compile_in(&[("en-US", &[APP_FTL])]);
    /// for ext in extensions {
    ///     cfg = cfg.compile_in(ext.locales());   // merged, not clobbered
    /// }
    /// ```
    ///
    /// Key collisions across merged resources are resolved by Fluent's
    /// `add_resource`: the **first** registration of a key wins and the
    /// duplicate is reported on stderr. Contributors should therefore
    /// namespace their keys (`myext-panel-title`) rather than rely on
    /// registration order.
    pub fn compile_in(mut self, slice: &[(&str, &[&'static str])]) -> Self {
        self.compile_in.extend(slice.iter().map(|(tag, resources)| {
            CompileInEntry {
                locale: tag
                    .parse()
                    .unwrap_or_else(|_| panic!("invalid locale tag in compile_in: {tag}")),
                resources: resources.to_vec(),
            }
        }));
        self
    }

    pub fn user_locale(mut self, loc: Option<LanguageIdentifier>) -> Self {
        self.user_locale = loc;
        self
    }

    pub fn auto_detect_os_locale(mut self, enabled: bool) -> Self {
        self.auto_detect_os = enabled;
        self
    }

    pub fn fallback_locale(mut self, l: LanguageIdentifier) -> Self {
        self.fallback_locale = l;
        self
    }

    /// Watch `path` on disk and rebuild `loc`'s bundle on every change. This
    /// is the translator hot-reload workflow, development only.
    ///
    /// `path` is either a single `.ftl` file **or a directory of them**.
    /// Prefer the directory whenever the locale's catalogue is split across
    /// several files: a bundle is the merge of every resource registered for
    /// its locale, so reloading one file of a set replaces the whole bundle
    /// and every key the other files defined silently falls back to the
    /// source locale. See
    /// [`I18nManager::reload_from_path`](crate::I18nManager::reload_from_path).
    ///
    /// Accumulates, so several locales can be watched at once. Registering
    /// two paths for the *same* locale does not merge them: each change
    /// rebuilds that locale from whichever path fired, so pass the
    /// directory that holds both instead.
    pub fn runtime_override(mut self, loc: LanguageIdentifier, path: PathBuf) -> Self {
        self.runtime_overrides.push((loc, path));
        self
    }

    /// Read-only accessor used by `TeksiloAppBuilder::run` to construct
    /// the hot-reload file watcher after the event loop has started.
    pub fn runtime_overrides(&self) -> &[(LanguageIdentifier, PathBuf)] {
        &self.runtime_overrides
    }

    /// Construct a test config seeded with one source locale and inline
    /// `(key, value)` Fluent message pairs. Used by headless tests instead of
    /// the production `compile_in` path.
    pub fn test_only(source: &str, msgs: &[(&str, &str)]) -> Self {
        let loc: LanguageIdentifier = source.parse().expect("invalid test source locale");
        let entry = TestLocaleEntry {
            locale: loc.clone(),
            messages: msgs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        };
        Self {
            source_locale: loc.clone(),
            supported_locales: vec![loc.clone()],
            compile_in: Vec::new(),
            test_messages: vec![entry],
            user_locale: None,
            auto_detect_os: false,
            fallback_locale: loc,
            runtime_overrides: Vec::new(),
            framework_locales: Vec::new(),
            widget_overrides: Vec::new(),
        }
    }

    /// Add another locale's inline messages to a `test_only` config.
    pub fn with_locale(mut self, loc: &str, msgs: &[(&str, &str)]) -> Self {
        let parsed: LanguageIdentifier = loc.parse().expect("invalid test locale");
        if !self.supported_locales.contains(&parsed) {
            self.supported_locales.push(parsed.clone());
        }
        self.test_messages.push(TestLocaleEntry {
            locale: parsed,
            messages: msgs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_en_us() {
        let cfg = I18nConfig::new();
        assert_eq!(cfg.source_locale.to_string(), "en-US");
        assert_eq!(cfg.fallback_locale.to_string(), "en-US");
        assert!(cfg.auto_detect_os);
    }

    #[test]
    fn compile_in_accumulates_across_calls() {
        // The config half of the extension story: entries pile up rather than
        // the last call replacing the list. (That they then *merge* per locale
        // is `I18nManager::from_config`'s half — see
        // `tests/compile_in_additive.rs`.)
        const A: &str = "a = A\n";
        const B: &str = "b = B\n";
        let cfg = I18nConfig::new()
            .compile_in(&[("en-US", &[A])])
            .compile_in(&[("en-US", &[B]), ("fr-FR", &[B])]);
        assert_eq!(cfg.compile_in.len(), 3);
    }

    #[test]
    fn test_only_seeds_inline_messages() {
        let cfg = I18nConfig::test_only("en-US", &[("greeting", "Hello")])
            .with_locale("fr-FR", &[("greeting", "Bonjour")]);
        assert_eq!(cfg.test_messages.len(), 2);
        assert_eq!(cfg.supported_locales.len(), 2);
        assert!(!cfg.auto_detect_os);
    }
}
