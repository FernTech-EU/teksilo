// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `native_theme_kde`: the factory it installs gives Breeze for a Breeze
//! desktop and nothing for any other.

#![cfg(feature = "theme-kde")]

use teksilo::kde_native_install::kde_native_theme;
use teksilo::platform::os_theme::NativeDesktop;
use teksilo::prelude::kde::scheme::{KdeColorScheme, KdeDesktop};
use teksilo::prelude::*;
use teksilo::tokens::{ColorSchemePreference, OsThemeColors, TargetDensity};

fn desktop(scheme: KdeColorScheme, widget_style: Option<&str>) -> NativeDesktop {
    let mut kde = KdeDesktop::new(scheme);
    kde.widget_style = widget_style.map(str::to_owned);
    let mut native = NativeDesktop::new(OsThemeColors::new(ColorSchemePreference::Light));
    native.kde = Some(kde);
    native
}

#[test]
fn a_breeze_desktop_gets_breeze_geometry_at_the_requested_density() {
    for style in [None, Some("Breeze"), Some("breeze")] {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let native = desktop(KdeColorScheme::breeze_light(), style);
            let request = NativeThemeRequest::new(&native, false, density);
            let theme = kde_native_theme(&request)
                .unwrap_or_else(|| panic!("a Breeze desktop ({style:?}) must be answered"));
            assert_eq!(theme.input.density, density, "{style:?}");
            assert_eq!(
                theme.shape,
                kde::light().shape,
                "Breeze's shapes for {style:?}"
            );
        }
    }
}

#[test]
fn a_fusion_desktop_is_not_answered() {
    let native = desktop(KdeColorScheme::breeze_light(), Some("Fusion"));
    let request = NativeThemeRequest::new(&native, false, TargetDensity::Compact);
    assert!(kde_native_theme(&request).is_none());
}

#[test]
fn a_desktop_without_kde_settings_is_not_answered() {
    let native = NativeDesktop::new(OsThemeColors::new(ColorSchemePreference::Dark));
    let request = NativeThemeRequest::new(&native, true, TargetDensity::Compact);
    assert!(kde_native_theme(&request).is_none());
}

#[test]
fn the_theme_carries_the_id_of_the_scheme_kde_judges() {
    let light = desktop(KdeColorScheme::breeze_light(), None);
    let dark = desktop(KdeColorScheme::breeze_dark(), None);
    let id = |native: &NativeDesktop, is_dark| {
        let request = NativeThemeRequest::new(native, is_dark, TargetDensity::Compact);
        kde_native_theme(&request).expect("Breeze").id
    };
    assert_eq!(id(&light, false).as_str(), "kde.light");
    assert_eq!(id(&dark, true).as_str(), "kde.dark");
}

#[test]
fn native_theme_kde_installs_the_kde_factory_on_the_builder() {
    let bare = TeksiloAppBuilder::new();
    assert!(
        bare.configured_native_theme().is_none(),
        "a builder starts with no native-theme factory"
    );

    let builder = bare.native_theme_kde();
    let factory = builder
        .configured_native_theme()
        .expect("native_theme_kde installs a factory");

    // The installed factory answers a Breeze desktop with Breeze's shapes at
    // the requested density, in the id of the scheme KDE judges ...
    let breeze = desktop(KdeColorScheme::breeze_dark(), None);
    let request = NativeThemeRequest::new(&breeze, true, TargetDensity::Touch);
    let theme = factory(&request).expect("a Breeze desktop is answered");
    assert_eq!(theme.shape, kde::dark().shape);
    assert_eq!(theme.input.density, TargetDensity::Touch);
    assert_eq!(theme.id.as_str(), "kde.dark");

    // ... and declines a desktop that is not Breeze, so the seam falls back.
    let fusion = desktop(KdeColorScheme::breeze_light(), Some("Fusion"));
    let request = NativeThemeRequest::new(&fusion, false, TargetDensity::Compact);
    assert!(factory(&request).is_none());
}

#[test]
fn a_later_factory_replaces_the_kde_one() {
    let builder = TeksiloAppBuilder::new()
        .native_theme_kde()
        .native_theme(|_| None);
    let factory = builder.configured_native_theme().expect("a factory");
    let breeze = desktop(KdeColorScheme::breeze_light(), None);
    let request = NativeThemeRequest::new(&breeze, false, TargetDensity::Compact);
    assert!(factory(&request).is_none());
}
