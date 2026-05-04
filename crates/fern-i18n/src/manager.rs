//! `I18nManager` — owns Fluent bundles and the active-locale signals.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fern_core::environment::LayoutDirection;
use fern_core::signal::Signal;
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

use crate::config::{I18nConfig, TestLocaleEntry};
use crate::direction::rtl_from_locale;

/// Outcome of a `set_locale` call. Used by app code to decide whether a
/// composite rebuild is needed (LTR↔RTL direction change requires it; same
/// direction does not).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocaleSwitchOutcome {
    pub direction_changed: bool,
}

/// The framework's i18n state. One instance per application; installed on
/// the thread-local by `FernAppBuilder` (Phase B).
///
/// Holds three bundle maps:
/// - `app_bundles`: application strings, populated from `compile_in` /
///   `test_messages`.
/// - `widget_bundles`: framework strings populated by
///   `register_framework_locales` (Phase E will wire fern-widgets to this).
/// - `widget_overrides`: application-supplied overrides for framework
///   strings (slot reserved; populated by `override_widget_strings` in a
///   future phase).
pub struct I18nManager {
    app_bundles: RefCell<HashMap<LanguageIdentifier, FluentBundle<FluentResource>>>,
    widget_bundles: RefCell<HashMap<LanguageIdentifier, FluentBundle<FluentResource>>>,
    widget_overrides: RefCell<HashMap<LanguageIdentifier, FluentBundle<FluentResource>>>,
    active: Signal<LanguageIdentifier>,
    direction: Signal<LayoutDirection>,
    version: Signal<u64>,
    source_locale: LanguageIdentifier,
    supported: Vec<LanguageIdentifier>,
}

impl I18nManager {
    /// Build a manager from the application's config.
    ///
    /// Constructs one `FluentBundle` per locale defined in `compile_in` and
    /// `test_messages`. The active locale starts at the source locale; call
    /// `set_locale` once after construction (typically with the result of
    /// `resolve_initial_locale`) to set the user-facing locale.
    ///
    /// Any framework bundles registered via `I18nConfig::framework_locales`
    /// are registered into `widget_bundles` at the same time, so the
    /// single `from_config` call yields a fully-populated manager.
    pub fn from_config(cfg: &I18nConfig) -> Rc<Self> {
        let mut app_bundles: HashMap<LanguageIdentifier, FluentBundle<FluentResource>> =
            HashMap::new();

        for entry in &cfg.compile_in {
            let bundle = build_bundle_from_resources(&entry.locale, &entry.resources);
            app_bundles.insert(entry.locale.clone(), bundle);
        }

        for entry in &cfg.test_messages {
            let bundle = build_bundle_from_test_messages(entry);
            app_bundles.insert(entry.locale.clone(), bundle);
        }

        let initial_direction = rtl_from_locale(&cfg.source_locale);

        let mgr = Rc::new(Self {
            app_bundles: RefCell::new(app_bundles),
            widget_bundles: RefCell::new(HashMap::new()),
            widget_overrides: RefCell::new(HashMap::new()),
            active: Signal::new(cfg.source_locale.clone()),
            direction: Signal::new(initial_direction),
            version: Signal::new(0),
            source_locale: cfg.source_locale.clone(),
            supported: cfg.supported_locales.clone(),
        });

        for slice in &cfg.framework_locales {
            mgr.register_framework_locales(std::slice::from_ref(slice));
        }

        for slice in &cfg.widget_overrides {
            mgr.register_widget_overrides(std::slice::from_ref(slice));
        }

        mgr
    }

    /// Register framework-internal locales (called by `FernAppBuilder` from
    /// `fern_widgets::framework_locales()` in Phase E). Each entry is a
    /// `(locale_tag, &[ftl_resource_str])` pair.
    pub fn register_framework_locales(&self, slice: &[(&str, &[&'static str])]) {
        let mut bundles = self.widget_bundles.borrow_mut();
        for (tag, resources) in slice {
            let Ok(loc): Result<LanguageIdentifier, _> = tag.parse() else {
                eprintln!("fern-i18n: skipping invalid framework locale tag {tag}");
                continue;
            };
            let bundle = build_bundle_from_resources(&loc, resources);
            bundles.insert(loc, bundle);
        }
    }

    /// Register application overrides for framework strings
    /// (architecture §12.13.4). Each entry becomes a bundle in
    /// `widget_overrides`, which is consulted before `widget_bundles`
    /// in the §12.13.5 lookup precedence.
    pub fn register_widget_overrides(&self, slice: &[(&str, &[&'static str])]) {
        let mut bundles = self.widget_overrides.borrow_mut();
        for (tag, resources) in slice {
            let Ok(loc): Result<LanguageIdentifier, _> = tag.parse() else {
                eprintln!("fern-i18n: skipping invalid widget-override locale tag {tag}");
                continue;
            };
            let bundle = build_bundle_from_resources(&loc, resources);
            bundles.insert(loc, bundle);
        }
    }

    /// Resolve the initial locale per the §12.5 precedence:
    /// `user_locale` → OS auto-detect (with partial matching) → fallback.
    pub fn resolve_initial_locale(cfg: &I18nConfig) -> LanguageIdentifier {
        if let Some(user) = &cfg.user_locale
            && cfg.supported_locales.contains(user)
        {
            return user.clone();
        }

        if cfg.auto_detect_os
            && let Some(os_str) = sys_locale::get_locale()
            && let Ok(parsed) = os_str.parse::<LanguageIdentifier>()
        {
            if cfg.supported_locales.contains(&parsed) {
                return parsed;
            }
            if let Some(matched) = cfg
                .supported_locales
                .iter()
                .find(|s| s.matches(&parsed, true, false))
            {
                return matched.clone();
            }
        }

        cfg.fallback_locale.clone()
    }

    /// Switch the active locale, increment the version signal, and report
    /// whether the layout direction flipped. Validates against
    /// `supported_locales` — if the requested locale isn't supported, the
    /// switch is a no-op and returns `direction_changed: false`.
    pub fn set_locale(&self, loc: LanguageIdentifier) -> LocaleSwitchOutcome {
        if !self.supported.is_empty() && !self.supported.contains(&loc) {
            return LocaleSwitchOutcome::default();
        }
        if self.active.get() == loc {
            return LocaleSwitchOutcome::default();
        }
        let old_dir = self.direction.get();
        let new_dir = rtl_from_locale(&loc);
        self.active.set(loc);
        if new_dir != old_dir {
            self.direction.set(new_dir);
        }
        let v = self.version.get();
        self.version.set(v + 1);
        LocaleSwitchOutcome {
            direction_changed: new_dir != old_dir,
        }
    }

    /// Bump only the version signal — used on hot-reload (§12.7) where the
    /// active locale and direction are unchanged but the bundle contents
    /// have been replaced and observers need to re-resolve.
    pub fn bump_version(&self) {
        let v = self.version.get();
        self.version.set(v + 1);
    }

    /// Reload an application bundle from disk. Replaces the in-memory
    /// bundle for `locale` with the parsed contents of `path`, then
    /// bumps the version signal so every `LocalizedString::to_signal()`
    /// observer re-resolves. Does **not** rebuild composite widgets
    /// (per §12.7: hot-reload must not rebuild because the active
    /// locale and direction are unchanged — only the bundle content
    /// changed, and reactive bindings are sufficient to propagate).
    ///
    /// On parse error the previous bundle is kept and the error is
    /// returned; no version bump happens. The file watcher wrapper in
    /// `file_watcher.rs` handles logging.
    pub fn reload_from_path(
        &self,
        locale: &LanguageIdentifier,
        path: &std::path::Path,
    ) -> Result<(), ReloadError> {
        let contents = std::fs::read_to_string(path).map_err(ReloadError::Io)?;
        let resource =
            FluentResource::try_new(contents).map_err(|(_, errs)| ReloadError::Parse(errs))?;
        let mut bundle = FluentBundle::new(vec![locale.clone()]);
        configure_bundle(&mut bundle);
        bundle
            .add_resource(resource)
            .map_err(ReloadError::AddResource)?;
        self.app_bundles.borrow_mut().insert(locale.clone(), bundle);
        self.bump_version();
        Ok(())
    }

    pub fn version_signal(&self) -> &Signal<u64> {
        &self.version
    }

    pub fn locale_signal(&self) -> &Signal<LanguageIdentifier> {
        &self.active
    }

    pub fn direction_signal(&self) -> &Signal<LayoutDirection> {
        &self.direction
    }

    pub fn source_locale(&self) -> &LanguageIdentifier {
        &self.source_locale
    }

    /// All locales the app declared as supported via
    /// `I18nConfig::supported_locales`. Read by the debug inspector's
    /// Locale tab to populate its switcher dropdown.
    pub fn supported_locales(&self) -> &[LanguageIdentifier] {
        &self.supported
    }

    /// Resolve an application string (`tr!`). Looks up the active locale's
    /// bundle, then the source-locale bundle, then returns the literal key
    /// as a placeholder if neither found one.
    pub fn resolve_app(&self, key: &str, args: &[(&str, FluentValue<'_>)]) -> String {
        let active = self.active.get();
        let bundles = self.app_bundles.borrow();
        if let Some(bundle) = bundles.get(&active)
            && let Some(text) = format_message(bundle, key, args)
        {
            return text;
        }
        if active != self.source_locale
            && let Some(bundle) = bundles.get(&self.source_locale)
            && let Some(text) = format_message(bundle, key, args)
        {
            return text;
        }
        eprintln!("fern-i18n: missing key `{key}` in app bundles");
        key.to_string()
    }

    /// Resolve a framework string (`tr_widget!`). Lookup precedence
    /// (§12.13.5): app override active → framework active → app override
    /// source → framework source → key placeholder.
    pub fn resolve_widget(&self, key: &str, args: &[(&str, FluentValue<'_>)]) -> String {
        let active = self.active.get();
        let overrides = self.widget_overrides.borrow();
        let widgets = self.widget_bundles.borrow();

        if let Some(bundle) = overrides.get(&active)
            && let Some(text) = format_message(bundle, key, args)
        {
            return text;
        }
        if let Some(bundle) = widgets.get(&active)
            && let Some(text) = format_message(bundle, key, args)
        {
            return text;
        }
        if active != self.source_locale {
            if let Some(bundle) = overrides.get(&self.source_locale)
                && let Some(text) = format_message(bundle, key, args)
            {
                return text;
            }
            if let Some(bundle) = widgets.get(&self.source_locale)
                && let Some(text) = format_message(bundle, key, args)
            {
                return text;
            }
        }
        eprintln!("fern-i18n: missing key `{key}` in widget bundles");
        key.to_string()
    }
}

/// Errors returned by `I18nManager::reload_from_path`. The watcher keeps
/// the previous bundle intact on any failure and logs the details.
#[derive(Debug)]
pub enum ReloadError {
    Io(std::io::Error),
    Parse(Vec<fluent_syntax::parser::ParserError>),
    AddResource(Vec<fluent_bundle::FluentError>),
}

impl std::fmt::Display for ReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReloadError::Io(e) => write!(f, "failed to read .ftl file: {e}"),
            ReloadError::Parse(errs) => write!(f, "Fluent parse errors: {errs:?}"),
            ReloadError::AddResource(errs) => {
                write!(f, "errors adding resource to bundle: {errs:?}")
            }
        }
    }
}

impl std::error::Error for ReloadError {}

/// Apply the framework-wide bundle configuration: turn off isolating
/// directional marks (so we don't sprinkle U+2068/U+2069 around every
/// interpolation), install our locale-aware number formatter, and
/// register the `DATETIME()` Fluent function. Called from every site
/// that creates a `FluentBundle` so all three bundle maps (app, widget,
/// widget-overrides) share identical behaviour.
fn configure_bundle(bundle: &mut FluentBundle<FluentResource>) {
    bundle.set_use_isolating(false);
    // `FluentBundle::new` does NOT auto-register Fluent's built-in
    // functions; without this call `{ NUMBER($v) }` resolves to the
    // literal `{NUMBER()}` placeholder. Upstream's `add_builtins`
    // currently registers only `NUMBER` (DATETIME is a TODO over there).
    if let Err(e) = bundle.add_builtins() {
        eprintln!("fern-i18n: failed to register Fluent builtins: {e:?}");
    }
    bundle.set_formatter(Some(crate::format::fern_format_callback));
    if let Err(e) = bundle.add_function("DATETIME", crate::format::datetime_function) {
        eprintln!("fern-i18n: failed to register DATETIME function: {e:?}");
    }
}

fn build_bundle_from_resources(
    locale: &LanguageIdentifier,
    resources: &[&'static str],
) -> FluentBundle<FluentResource> {
    let mut bundle = FluentBundle::new(vec![locale.clone()]);
    configure_bundle(&mut bundle);
    for src in resources {
        match FluentResource::try_new((*src).to_string()) {
            Ok(res) => {
                if let Err(errs) = bundle.add_resource(res) {
                    eprintln!("fern-i18n: errors adding resource for {locale}: {errs:?}");
                }
            }
            Err((_res, errs)) => {
                eprintln!("fern-i18n: parse errors in resource for {locale}: {errs:?}");
            }
        }
    }
    bundle
}

fn build_bundle_from_test_messages(entry: &TestLocaleEntry) -> FluentBundle<FluentResource> {
    let mut bundle = FluentBundle::new(vec![entry.locale.clone()]);
    configure_bundle(&mut bundle);
    let mut combined = String::new();
    for (k, v) in &entry.messages {
        combined.push_str(k);
        combined.push_str(" = ");
        combined.push_str(v);
        combined.push('\n');
    }
    match FluentResource::try_new(combined) {
        Ok(res) => {
            if let Err(errs) = bundle.add_resource(res) {
                eprintln!(
                    "fern-i18n: errors building test bundle for {}: {errs:?}",
                    entry.locale
                );
            }
        }
        Err((_res, errs)) => {
            eprintln!(
                "fern-i18n: parse errors in test bundle for {}: {errs:?}",
                entry.locale
            );
        }
    }
    bundle
}

fn format_message(
    bundle: &FluentBundle<FluentResource>,
    key: &str,
    args: &[(&str, FluentValue<'_>)],
) -> Option<String> {
    let msg = bundle.get_message(key)?;
    let pattern = msg.value()?;
    let fluent_args = if args.is_empty() {
        None
    } else {
        let mut fa = FluentArgs::new();
        for (k, v) in args {
            fa.set(*k, v.clone());
        }
        Some(fa)
    };
    let mut errors = Vec::new();
    let cow = bundle.format_pattern(pattern, fluent_args.as_ref(), &mut errors);
    Some(cow.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lid(s: &str) -> LanguageIdentifier {
        s.parse().unwrap()
    }

    #[test]
    fn test_only_resolves_messages() {
        let cfg = I18nConfig::test_only("en-US", &[("greeting", "Hello")]);
        let mgr = I18nManager::from_config(&cfg);
        assert_eq!(mgr.resolve_app("greeting", &[]), "Hello");
    }

    #[test]
    fn switch_locale_changes_resolution() {
        let cfg = I18nConfig::test_only("en-US", &[("greeting", "Hello")])
            .with_locale("fr-FR", &[("greeting", "Bonjour")]);
        let mgr = I18nManager::from_config(&cfg);
        assert_eq!(mgr.resolve_app("greeting", &[]), "Hello");

        let outcome = mgr.set_locale(lid("fr-FR"));
        assert!(!outcome.direction_changed);
        assert_eq!(mgr.resolve_app("greeting", &[]), "Bonjour");
    }

    #[test]
    fn switch_to_rtl_reports_direction_change() {
        let cfg = I18nConfig::test_only("en-US", &[("k", "v")])
            .with_locale("ar-SA", &[("k", "ع")]);
        let mgr = I18nManager::from_config(&cfg);
        assert_eq!(*mgr.direction_signal().get_ref(), LayoutDirection::LeftToRight);

        let outcome = mgr.set_locale(lid("ar-SA"));
        assert!(outcome.direction_changed);
        assert_eq!(*mgr.direction_signal().get_ref(), LayoutDirection::RightToLeft);

        let back = mgr.set_locale(lid("en-US"));
        assert!(back.direction_changed);
    }

    #[test]
    fn ltr_to_ltr_no_direction_change() {
        let cfg = I18nConfig::test_only("en-US", &[("k", "v")])
            .with_locale("fr-FR", &[("k", "v")]);
        let mgr = I18nManager::from_config(&cfg);
        let outcome = mgr.set_locale(lid("fr-FR"));
        assert!(!outcome.direction_changed);
    }

    #[test]
    fn version_increments_on_locale_change() {
        let cfg = I18nConfig::test_only("en-US", &[("k", "v")])
            .with_locale("fr-FR", &[("k", "v")]);
        let mgr = I18nManager::from_config(&cfg);
        let v0 = mgr.version_signal().get();
        mgr.set_locale(lid("fr-FR"));
        let v1 = mgr.version_signal().get();
        assert_eq!(v1, v0 + 1);
    }

    #[test]
    fn missing_in_active_falls_back_to_source() {
        let cfg = I18nConfig::test_only("en-US", &[("only_in_en", "English")])
            .with_locale("fr-FR", &[("other_key", "Autre")]);
        let mgr = I18nManager::from_config(&cfg);
        mgr.set_locale(lid("fr-FR"));
        assert_eq!(mgr.resolve_app("only_in_en", &[]), "English");
    }

    #[test]
    fn missing_everywhere_returns_key_placeholder() {
        let cfg = I18nConfig::test_only("en-US", &[("greeting", "Hello")]);
        let mgr = I18nManager::from_config(&cfg);
        assert_eq!(mgr.resolve_app("nope", &[]), "nope");
    }

    #[test]
    fn args_are_substituted() {
        let cfg = I18nConfig::test_only("en-US", &[("welcome", "Hello, { $name }!")]);
        let mgr = I18nManager::from_config(&cfg);
        let args = [("name", FluentValue::from("Alice"))];
        assert_eq!(mgr.resolve_app("welcome", &args), "Hello, Alice!");
    }

    #[test]
    fn unsupported_locale_is_noop() {
        let cfg = I18nConfig::test_only("en-US", &[("k", "v")]);
        let mgr = I18nManager::from_config(&cfg);
        let outcome = mgr.set_locale(lid("de-DE"));
        assert!(!outcome.direction_changed);
        assert_eq!(mgr.locale_signal().get().to_string(), "en-US");
    }

    #[test]
    fn resolve_initial_locale_user_choice_wins() {
        let cfg = I18nConfig::new()
            .supported_locales([lid("en-US"), lid("fr-FR")])
            .user_locale(Some(lid("fr-FR")));
        assert_eq!(I18nManager::resolve_initial_locale(&cfg), lid("fr-FR"));
    }

    #[test]
    fn resolve_initial_locale_user_unsupported_falls_back() {
        let cfg = I18nConfig::new()
            .supported_locales([lid("en-US"), lid("fr-FR")])
            .user_locale(Some(lid("de-DE")))
            .auto_detect_os_locale(false)
            .fallback_locale(lid("en-US"));
        assert_eq!(I18nManager::resolve_initial_locale(&cfg), lid("en-US"));
    }

    #[test]
    fn resolve_initial_locale_falls_back_when_no_user_no_os() {
        let cfg = I18nConfig::new()
            .supported_locales([lid("en-US"), lid("fr-FR")])
            .auto_detect_os_locale(false)
            .fallback_locale(lid("fr-FR"));
        assert_eq!(I18nManager::resolve_initial_locale(&cfg), lid("fr-FR"));
    }

    #[test]
    fn bump_version_only_increments_version() {
        let cfg = I18nConfig::test_only("en-US", &[("k", "v")]);
        let mgr = I18nManager::from_config(&cfg);
        let v0 = mgr.version_signal().get();
        let loc0 = mgr.locale_signal().get();
        mgr.bump_version();
        assert_eq!(mgr.version_signal().get(), v0 + 1);
        assert_eq!(mgr.locale_signal().get(), loc0);
    }

    #[test]
    fn reload_from_path_replaces_bundle_and_bumps_version() {
        let dir = std::env::temp_dir().join(format!(
            "fern-i18n-reload-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fr-FR.ftl");
        std::fs::write(&path, "greeting = Bonjour\n").unwrap();

        let cfg = I18nConfig::test_only("en-US", &[("greeting", "Hello")])
            .with_locale("fr-FR", &[("greeting", "Salut")]);
        let mgr = I18nManager::from_config(&cfg);
        mgr.set_locale(lid("fr-FR"));
        assert_eq!(mgr.resolve_app("greeting", &[]), "Salut");

        let v_before = mgr.version_signal().get();
        mgr.reload_from_path(&lid("fr-FR"), &path)
            .expect("reload succeeds");
        assert_eq!(mgr.version_signal().get(), v_before + 1);
        assert_eq!(mgr.resolve_app("greeting", &[]), "Bonjour");

        // Edit the file and reload again — observers see the new value.
        std::fs::write(&path, "greeting = Coucou\n").unwrap();
        mgr.reload_from_path(&lid("fr-FR"), &path).unwrap();
        assert_eq!(mgr.resolve_app("greeting", &[]), "Coucou");

        // Active locale and direction must not change.
        assert_eq!(mgr.locale_signal().get().to_string(), "fr-FR");
        assert_eq!(*mgr.direction_signal().get_ref(), LayoutDirection::LeftToRight);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn widget_overrides_take_priority_over_framework_bundle() {
        // Framework bundle ships "Status" in en-US.
        static FRAMEWORK: &[(&str, &[&str])] = &[
            ("en-US", &["a11y-status-bar-name = Status\n"]),
        ];
        // App override replaces it with "System status".
        static OVERRIDE: &[(&str, &[&str])] = &[
            ("en-US", &["a11y-status-bar-name = System status\n"]),
        ];

        let cfg = I18nConfig::test_only("en-US", &[("app-k", "v")])
            .framework_locales(FRAMEWORK)
            .override_widget_strings(OVERRIDE);
        let mgr = I18nManager::from_config(&cfg);

        // `tr_widget!` resolution routes through `resolve_widget`, which
        // checks overrides first. The override wins.
        assert_eq!(
            mgr.resolve_widget("a11y-status-bar-name", &[]),
            "System status"
        );
    }

    #[test]
    fn widget_overrides_fall_through_to_framework_when_key_missing() {
        static FRAMEWORK: &[(&str, &[&str])] = &[
            ("en-US", &["a11y-status-bar-name = Status\na11y-dialog-name = Dialog\n"]),
        ];
        // Override only redefines one key; the other falls through to
        // the framework bundle.
        static OVERRIDE: &[(&str, &[&str])] = &[
            ("en-US", &["a11y-status-bar-name = System status\n"]),
        ];

        let cfg = I18nConfig::test_only("en-US", &[("app-k", "v")])
            .framework_locales(FRAMEWORK)
            .override_widget_strings(OVERRIDE);
        let mgr = I18nManager::from_config(&cfg);

        assert_eq!(
            mgr.resolve_widget("a11y-status-bar-name", &[]),
            "System status"
        );
        // Not overridden — framework value.
        assert_eq!(mgr.resolve_widget("a11y-dialog-name", &[]), "Dialog");
    }

    #[test]
    fn reload_from_path_malformed_file_returns_error_and_keeps_old_bundle() {
        let dir = std::env::temp_dir().join(format!(
            "fern-i18n-reload-bad-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fr-FR.ftl");
        // Deliberately malformed: missing `=` on first line.
        std::fs::write(&path, "this is not a valid ftl file\n").unwrap();

        let cfg = I18nConfig::test_only("en-US", &[("k", "v")])
            .with_locale("fr-FR", &[("k", "valeur")]);
        let mgr = I18nManager::from_config(&cfg);
        mgr.set_locale(lid("fr-FR"));
        let v_before = mgr.version_signal().get();

        let outcome = mgr.reload_from_path(&lid("fr-FR"), &path);
        assert!(outcome.is_err());
        // Version unchanged — no observers should re-resolve.
        assert_eq!(mgr.version_signal().get(), v_before);
        // Previous bundle still resolves.
        assert_eq!(mgr.resolve_app("k", &[]), "valeur");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
