//! Integration tests for the Phase B i18n wiring on `FernAppBuilder` and
//! `HeadlessApp`. Verifies that:
//!
//! - Registering an `I18nConfig` installs the manager on the thread-local.
//! - `headless.set_locale(...)` flips text resolution and the tree's
//!   layout direction in lockstep with the manager.
//! - LTR↔LTR transitions do not flip direction; LTR↔RTL transitions do.
//! - `LocalizedString::to_signal()` produced inside an installed app
//!   re-resolves on locale change.

use fern_app::FernAppBuilder;
use fern_core::environment::LayoutDirection;
use fern_i18n::{I18nConfig, LanguageIdentifier, LocalizedString, localized, resolve_message};

fn lid(s: &str) -> LanguageIdentifier {
    s.parse().unwrap()
}

fn build_trilingual_headless() -> fern_app::HeadlessApp {
    FernAppBuilder::new()
        .i18n(
            I18nConfig::test_only("en-US", &[("greeting", "Hello, World!")])
                .with_locale("fr-FR", &[("greeting", "Bonjour, le monde !")])
                .with_locale("ar-SA", &[("greeting", "مرحبا بالعالم")]),
        )
        .build_headless()
}

#[test]
fn i18n_manager_is_installed_after_build_headless() {
    let _app = build_trilingual_headless();

    let locale = fern_i18n::current_locale().expect("manager not installed");
    assert_eq!(locale.get().to_string(), "en-US");

    let direction = fern_i18n::current_direction().expect("direction signal missing");
    assert_eq!(direction.get(), LayoutDirection::LeftToRight);
}

#[test]
fn resolve_message_picks_up_initial_locale() {
    let _app = build_trilingual_headless();
    assert_eq!(resolve_message("greeting", &[]), "Hello, World!");
}

#[test]
fn set_locale_switches_resolution_and_direction() {
    let mut app = build_trilingual_headless();
    assert_eq!(resolve_message("greeting", &[]), "Hello, World!");
    assert_eq!(app.tree.layout_direction(), LayoutDirection::LeftToRight);

    app.set_locale(lid("fr-FR"));
    assert_eq!(resolve_message("greeting", &[]), "Bonjour, le monde !");
    // Same direction, must not flip.
    assert_eq!(app.tree.layout_direction(), LayoutDirection::LeftToRight);

    app.set_locale(lid("ar-SA"));
    assert_eq!(resolve_message("greeting", &[]), "مرحبا بالعالم");
    // Direction must flip to RTL.
    assert_eq!(app.tree.layout_direction(), LayoutDirection::RightToLeft);

    app.set_locale(lid("en-US"));
    assert_eq!(resolve_message("greeting", &[]), "Hello, World!");
    // Back to LTR.
    assert_eq!(app.tree.layout_direction(), LayoutDirection::LeftToRight);
}

#[test]
fn localized_string_to_signal_observes_locale_change() {
    let mut app = build_trilingual_headless();

    let ls: LocalizedString = localized(|| resolve_message("greeting", &[]));
    let sig = ls.to_signal();
    assert_eq!(sig.get(), "Hello, World!");

    app.set_locale(lid("fr-FR"));
    assert_eq!(sig.get(), "Bonjour, le monde !");

    app.set_locale(lid("ar-SA"));
    assert_eq!(sig.get(), "مرحبا بالعالم");
}

#[test]
fn no_i18n_config_keeps_manager_unset_and_set_locale_is_noop() {
    fern_i18n::thread_local::clear();

    let mut app = FernAppBuilder::new().build_headless();
    assert!(app.i18n_manager().is_none());

    // No-op: there's no manager and no recorded locale.
    app.set_locale(lid("fr-FR"));
    assert_eq!(app.tree.layout_direction(), LayoutDirection::LeftToRight);
    // resolve_message without a manager returns the literal key.
    assert_eq!(resolve_message("anything", &[]), "anything");
}
