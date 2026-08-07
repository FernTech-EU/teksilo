// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Integration tests for the Phase B i18n wiring on `TeksiloAppBuilder` and
//! `HeadlessApp`. Verifies that:
//!
//! - Registering an `I18nConfig` installs the manager on the thread-local.
//! - `headless.set_locale(...)` flips text resolution and the tree's
//!   layout direction in lockstep with the manager.
//! - LTR↔LTR transitions do not flip direction; LTR↔RTL transitions do.
//! - `LocalizedString::to_signal()` produced inside an installed app
//!   re-resolves on locale change.

use teksilo_app::TeksiloAppBuilder;
use teksilo_core::environment::LayoutDirection;
use teksilo_i18n::{I18nConfig, LanguageIdentifier, LocalizedString, localized, resolve_message};

fn lid(s: &str) -> LanguageIdentifier {
    s.parse().unwrap()
}

fn build_trilingual_headless() -> teksilo_app::HeadlessApp {
    TeksiloAppBuilder::new()
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

    let locale = teksilo_i18n::current_locale().expect("manager not installed");
    assert_eq!(locale.get().to_string(), "en-US");

    let direction = teksilo_i18n::current_direction().expect("direction signal missing");
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
fn access_label_at_name_follows_locale_after_set_locale() {
    use teksilo_canvas::SizeProposal;
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::widget_builder::WidgetBuilder;

    // Minimal leaf so the test doesn't depend on any concrete widget's chrome.
    #[derive(Debug)]
    struct Probe;
    impl Widget for Probe {
        fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
    }

    let mut app = build_trilingual_headless();

    // A reactive *translated* AT label — the same shape `tr!(...)` produces
    // (a `localized` closure that `Into<Prop<String>>` turns into a
    // locale-observing `Prop::Bound`).
    let id = app
        .tree
        .add(Probe.access_label(localized(|| resolve_message("greeting", &[]))));
    app.tree.layout(SizeProposal::exact(100.0, 100.0));

    // The *cached* AccessKit tree (what the platform adapter consumes) must
    // carry the current-locale label. We search node labels inline so the
    // test needn't name `accesskit` directly.
    let u0 = app.tree.sync_accessibility();
    assert!(
        u0.nodes
            .iter()
            .any(|(_, n)| n.label() == Some("Hello, World!")),
        "initial AT label"
    );
    assert_eq!(
        app.tree.accessibility_node(id).name(),
        Some("Hello, World!")
    );

    // Same-direction switch (en-US -> fr-FR): no composite rebuild happens,
    // so the AT tree must re-walk on the locale-version change for the
    // announced label to follow the locale. Regression guard for the
    // `sync_accessibility` locale check + the `Prop<String>` override store.
    app.set_locale(lid("fr-FR"));
    let u1 = app.tree.sync_accessibility();
    assert!(
        u1.nodes
            .iter()
            .any(|(_, n)| n.label() == Some("Bonjour, le monde !")),
        "cached AT label must update to fr-FR"
    );
    assert!(
        !u1.nodes
            .iter()
            .any(|(_, n)| n.label() == Some("Hello, World!")),
        "stale en-US AT label must be gone after re-walk"
    );
    assert_eq!(
        app.tree.accessibility_node(id).name(),
        Some("Bonjour, le monde !")
    );
}

#[test]
fn no_i18n_config_keeps_manager_unset_and_set_locale_is_noop() {
    teksilo_i18n::thread_local::clear();

    let mut app = TeksiloAppBuilder::new().build_headless();
    assert!(app.i18n_manager().is_none());

    // No-op: there's no manager and no recorded locale.
    app.set_locale(lid("fr-FR"));
    assert_eq!(app.tree.layout_direction(), LayoutDirection::LeftToRight);
    // resolve_message without a manager returns the literal key.
    assert_eq!(resolve_message("anything", &[]), "anything");
}
