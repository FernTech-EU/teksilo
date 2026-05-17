//! Integration test for the `compile_in_locales!` declarative sugar
//! (architecture §12.4). Verifies that the macro expansion produces a
//! slice of the same shape as `compile_in` expects, and that an
//! `I18nManager` built from it resolves messages correctly across
//! multiple locales and multiple files per locale.

use bastyde_i18n::{I18nConfig, I18nManager, LanguageIdentifier, compile_in_locales};

fn lid(s: &str) -> LanguageIdentifier {
    s.parse().unwrap()
}

#[test]
fn sugar_expands_to_matching_shape() {
    // The macro expansion must have the exact same type as the
    // `&[(&str, &[&str])]` that `compile_in` takes — otherwise the
    // call below wouldn't typecheck.
    let slice = compile_in_locales!(
        base = "compile_in_locales_fixture/",
        locales = ["en-US", "fr-FR"],
        files = ["main.ftl", "auth.ftl"],
    );

    let cfg = I18nConfig::new()
        .source_locale(lid("en-US"))
        .supported_locales([lid("en-US"), lid("fr-FR")])
        .compile_in(slice);
    let mgr = I18nManager::from_config(&cfg);

    // en-US (source, active by default) resolves both files.
    assert_eq!(mgr.resolve_app("greeting", &[]), "Hello");
    assert_eq!(mgr.resolve_app("login", &[]), "Log in");

    // Switch to fr-FR: both files present.
    mgr.set_locale(lid("fr-FR"));
    assert_eq!(mgr.resolve_app("greeting", &[]), "Bonjour");
    assert_eq!(mgr.resolve_app("login", &[]), "Connexion");
}

#[test]
fn single_locale_single_file_also_works() {
    // Degenerate case: one locale, one file. The macro still
    // produces a well-formed slice.
    let slice = compile_in_locales!(
        base = "compile_in_locales_fixture/",
        locales = ["en-US"],
        files = ["main.ftl"],
    );

    let cfg = I18nConfig::new()
        .source_locale(lid("en-US"))
        .supported_locales([lid("en-US")])
        .compile_in(slice);
    let mgr = I18nManager::from_config(&cfg);
    assert_eq!(mgr.resolve_app("greeting", &[]), "Hello");
}
