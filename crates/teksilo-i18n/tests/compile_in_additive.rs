// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `compile_in` accumulates, and repeats of one locale merge into that
//! locale's single bundle.
//!
//! This is what lets an application compose its own catalogue with catalogues
//! shipped by extensions, plugins or sibling crates: each contributor calls
//! `compile_in` with its own `.ftl` resources, and none of them has to know
//! the others exist.
//!
//! Before this behaviour existed, `compile_in` assigned rather than extended
//! *and* `from_config` inserted one bundle per entry — so a single extension
//! contributing one string to `en-US` silently deleted the application's
//! entire `en-US` catalogue. Every assertion here would have failed by
//! resolving to the bare key.

use teksilo_i18n::{I18nConfig, I18nManager, LanguageIdentifier};

fn lid(s: &str) -> LanguageIdentifier {
    s.parse().unwrap()
}

const APP_EN: &str = "app-title = Skribisto\napp-quit = Quit\n";
const APP_FR: &str = "app-title = Skribisto\napp-quit = Quitter\n";
const EXT_EN: &str = "ext-panel = Structure\n";
const EXT_FR: &str = "ext-panel = Structure narrative\n";
const OTHER_EXT_EN: &str = "other-badge = Drift\n";

#[test]
fn two_calls_for_one_locale_merge() {
    let cfg = I18nConfig::new()
        .source_locale(lid("en-US"))
        .supported_locales([lid("en-US")])
        .compile_in(&[("en-US", &[APP_EN])])
        .compile_in(&[("en-US", &[EXT_EN])]);
    let mgr = I18nManager::from_config(&cfg);

    // The application's catalogue survives the extension's registration…
    assert_eq!(mgr.resolve_app("app-title", &[]), "Skribisto");
    assert_eq!(mgr.resolve_app("app-quit", &[]), "Quit");
    // …and the extension's own string is reachable from the same bundle.
    assert_eq!(mgr.resolve_app("ext-panel", &[]), "Structure");
}

#[test]
fn many_contributors_all_survive() {
    // Two is the case that used to break; N is the case that has to keep
    // working, because the extension list is a loop, not a pair.
    let cfg = I18nConfig::new()
        .source_locale(lid("en-US"))
        .supported_locales([lid("en-US")])
        .compile_in(&[("en-US", &[APP_EN])])
        .compile_in(&[("en-US", &[EXT_EN])])
        .compile_in(&[("en-US", &[OTHER_EXT_EN])]);
    let mgr = I18nManager::from_config(&cfg);

    assert_eq!(mgr.resolve_app("app-title", &[]), "Skribisto");
    assert_eq!(mgr.resolve_app("ext-panel", &[]), "Structure");
    assert_eq!(mgr.resolve_app("other-badge", &[]), "Drift");
}

#[test]
fn merging_is_per_locale_and_survives_a_locale_switch() {
    // An extension ships both locales, in one call, the way an application
    // does. Each locale's bundle must gain only its own resources.
    let cfg = I18nConfig::new()
        .source_locale(lid("en-US"))
        .supported_locales([lid("en-US"), lid("fr-FR")])
        .compile_in(&[("en-US", &[APP_EN]), ("fr-FR", &[APP_FR])])
        .compile_in(&[("en-US", &[EXT_EN]), ("fr-FR", &[EXT_FR])]);
    let mgr = I18nManager::from_config(&cfg);

    assert_eq!(mgr.resolve_app("app-quit", &[]), "Quit");
    assert_eq!(mgr.resolve_app("ext-panel", &[]), "Structure");

    mgr.set_locale(lid("fr-FR"));
    assert_eq!(mgr.resolve_app("app-quit", &[]), "Quitter");
    assert_eq!(mgr.resolve_app("ext-panel", &[]), "Structure narrative");
}

#[test]
fn a_locale_only_an_extension_supplies_is_still_built() {
    // The application ships en-US only; the extension adds fr-FR. The new
    // locale must get a bundle of its own rather than being folded into the
    // application's.
    let cfg = I18nConfig::new()
        .source_locale(lid("en-US"))
        .supported_locales([lid("en-US"), lid("fr-FR")])
        .compile_in(&[("en-US", &[APP_EN])])
        .compile_in(&[("fr-FR", &[APP_FR, EXT_FR])]);
    let mgr = I18nManager::from_config(&cfg);

    mgr.set_locale(lid("fr-FR"));
    assert_eq!(mgr.resolve_app("app-quit", &[]), "Quitter");
    assert_eq!(mgr.resolve_app("ext-panel", &[]), "Structure narrative");
}

#[test]
fn first_registration_wins_on_key_collision() {
    // Fluent's `add_resource` keeps the first definition of a key and reports
    // the duplicate, so registration order *is* the collision rule. Pinned
    // because `compile_in`'s documentation promises it: contributors are told
    // to namespace their keys rather than rely on overriding the application.
    const SHADOW: &str = "app-title = Hijacked\n";

    let cfg = I18nConfig::new()
        .source_locale(lid("en-US"))
        .supported_locales([lid("en-US")])
        .compile_in(&[("en-US", &[APP_EN])])
        .compile_in(&[("en-US", &[SHADOW])]);
    let mgr = I18nManager::from_config(&cfg);

    assert_eq!(mgr.resolve_app("app-title", &[]), "Skribisto");
}

#[test]
fn a_single_call_still_behaves_exactly_as_before() {
    // The overwhelmingly common shape — one application, one call, several
    // locales — must be untouched by the accumulate change.
    let cfg = I18nConfig::new()
        .source_locale(lid("en-US"))
        .supported_locales([lid("en-US"), lid("fr-FR")])
        .compile_in(&[("en-US", &[APP_EN]), ("fr-FR", &[APP_FR])]);
    let mgr = I18nManager::from_config(&cfg);

    assert_eq!(mgr.resolve_app("app-quit", &[]), "Quit");
    mgr.set_locale(lid("fr-FR"));
    assert_eq!(mgr.resolve_app("app-quit", &[]), "Quitter");
}
