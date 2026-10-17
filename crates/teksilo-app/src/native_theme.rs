// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The one place [`ThemeMode::Native`](crate::ThemeMode::Native) turns what
//! the desktop reports into a [`Theme`].
//!
//! An app can hand the window manager a [`NativeThemeFactory`] through
//! [`TeksiloAppBuilder::native_theme`](crate::TeksiloAppBuilder::native_theme):
//! asked with a [`NativeThemeRequest`] whenever Native mode needs a theme (a
//! window opening, the desktop's light or dark state changing, a switch to
//! Native mode), it can answer with a theme of its own, for example a preset
//! that brings its own geometry and chrome along with the desktop's colours.
//! When there is no factory, or it answers `None`, the theme is IntUI
//! coloured by the desktop ([`presets::intui::from_os`]).
//!
//! The callers stamp the result with the id `"system"` (the window manager)
//! or a density (the previewer) themselves; [`resolve_native_theme`] does
//! neither.

use teksilo_core::Theme;
use teksilo_core::presets;
use teksilo_platform::os_theme::NativeDesktop;
use teksilo_tokens::{ColorSchemePreference, OsThemeColors, TargetDensity};

/// What [`ThemeMode::Native`](crate::ThemeMode::Native) asks a
/// [`NativeThemeFactory`] for a theme with.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct NativeThemeRequest<'a> {
    /// What the desktop reports: its colours and, on a KDE Plasma session,
    /// its settings.
    pub desktop: &'a NativeDesktop,
    /// Whether the theme is to be dark.
    pub dark: bool,
    /// The target density the theme is wanted at.
    pub density: TargetDensity,
}

impl<'a> NativeThemeRequest<'a> {
    /// A request for a theme over `desktop`, dark or light, at `density`.
    pub fn new(desktop: &'a NativeDesktop, dark: bool, density: TargetDensity) -> Self {
        NativeThemeRequest {
            desktop,
            dark,
            density,
        }
    }
}

/// An app's own answer to a [`NativeThemeRequest`]: `Some` theme to use, or
/// `None` to keep the default, IntUI coloured by the desktop.
///
/// Installed with
/// [`TeksiloAppBuilder::native_theme`](crate::TeksiloAppBuilder::native_theme).
pub type NativeThemeFactory = Box<dyn Fn(&NativeThemeRequest<'_>) -> Option<Theme>>;

/// The theme [`ThemeMode::Native`](crate::ThemeMode::Native) uses for
/// `request`: the `factory`'s answer when there is a factory and it answers
/// `Some`, otherwise [`presets::intui::from_os`] over the desktop's colour
/// report with `request.dark`.
///
/// The theme is returned as the factory or the preset built it: no id is set
/// and no density applied.
pub fn resolve_native_theme(
    factory: Option<&NativeThemeFactory>,
    request: &NativeThemeRequest<'_>,
) -> Theme {
    factory
        .and_then(|factory| factory(request))
        .unwrap_or_else(|| presets::intui::from_os(&request.desktop.os, request.dark))
}

/// The density the window manager asks Native-mode themes at: the one every
/// preset is built with, which is also the density its Native themes have
/// always had.
const WINDOW_MANAGER_DENSITY: TargetDensity = TargetDensity::Compact;

/// The Native-mode theme the window manager applies when the desktop's
/// appearance is re-read (a light or dark change, a switch to Native mode),
/// with the id `"system"`.
///
/// A desktop that states a light or dark preference decides `dark` itself.
/// One that states none is answered from `hint` (the winit light or dark
/// state; light when there is none) with a report of that preference alone:
/// its colours are not used.
pub(crate) fn theme_for_appearance_change(
    factory: Option<&NativeThemeFactory>,
    desktop: &NativeDesktop,
    hint: Option<bool>,
) -> Theme {
    let theme = match desktop.os.color_scheme {
        ColorSchemePreference::Dark | ColorSchemePreference::Light => resolve_native_theme(
            factory,
            &NativeThemeRequest::new(
                desktop,
                desktop.os.color_scheme.is_dark(),
                WINDOW_MANAGER_DENSITY,
            ),
        ),
        ColorSchemePreference::NoPreference => {
            let dark = hint.unwrap_or(false);
            let stated = NativeDesktop::new(OsThemeColors::new(if dark {
                ColorSchemePreference::Dark
            } else {
                ColorSchemePreference::Light
            }));
            resolve_native_theme(
                factory,
                &NativeThemeRequest::new(&stated, dark, WINDOW_MANAGER_DENSITY),
            )
        }
    };
    theme.with_id("system")
}

/// The Native-mode theme the window manager builds a new window with, with
/// the id `"system"`: dark exactly when the desktop states a dark
/// preference.
pub(crate) fn theme_for_new_window(
    factory: Option<&NativeThemeFactory>,
    desktop: &NativeDesktop,
) -> Theme {
    resolve_native_theme(
        factory,
        &NativeThemeRequest::new(
            desktop,
            desktop.os.color_scheme.is_dark(),
            WINDOW_MANAGER_DENSITY,
        ),
    )
    .with_id("system")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_tokens::{Color, ColorTokens};

    /// `apply_os_theme`'s Native arm as it was before this module, given the
    /// already-resolved winit hint.
    fn old_appearance_change(os: &OsThemeColors, hint: Option<bool>) -> Theme {
        match os.color_scheme {
            ColorSchemePreference::Dark => Theme {
                colors: ColorTokens::from_os_colors(os),
                ..presets::intui::dark()
            },
            ColorSchemePreference::Light => Theme {
                colors: ColorTokens::from_os_colors(os),
                ..presets::intui::light()
            },
            ColorSchemePreference::NoPreference => {
                if hint.unwrap_or(false) {
                    presets::intui::dark()
                } else {
                    presets::intui::light()
                }
            }
        }
        .with_id("system")
    }

    /// The window-creation Native arm as it was before this module.
    fn old_new_window(os: &OsThemeColors) -> Theme {
        let base = if os.color_scheme.is_dark() {
            presets::intui::dark()
        } else {
            presets::intui::light()
        };
        Theme {
            colors: ColorTokens::from_os_colors(os),
            ..base
        }
        .with_id("system")
    }

    /// Reports for every preference, with no colours, with the colours every
    /// desktop reader fills, and with the view and semantic colours as well.
    fn reports() -> Vec<OsThemeColors> {
        let mut out = Vec::new();
        for scheme in [
            ColorSchemePreference::Light,
            ColorSchemePreference::Dark,
            ColorSchemePreference::NoPreference,
        ] {
            let bare = OsThemeColors::new(scheme);

            let mut accent_only = OsThemeColors::new(scheme);
            accent_only.accent = Some(Color::from_rgb(0.24, 0.68, 0.91));

            let mut legacy = OsThemeColors::new(scheme);
            legacy.accent = Some(Color::from_rgb(0.24, 0.68, 0.91));
            legacy.window_bg = Some(Color::from_rgb(0.94, 0.94, 0.95));
            legacy.window_fg = Some(Color::from_rgb(0.14, 0.15, 0.16));
            legacy.button_bg = Some(Color::from_rgb(0.99, 0.99, 0.99));
            legacy.button_fg = Some(Color::from_rgb(0.14, 0.15, 0.16));
            legacy.selection_bg = Some(Color::from_rgb(0.24, 0.68, 0.91));
            legacy.selection_fg = Some(Color::WHITE);
            legacy.tooltip_bg = Some(Color::from_rgb(0.97, 0.97, 0.97));
            legacy.tooltip_fg = Some(Color::from_rgb(0.14, 0.15, 0.16));

            let mut full = legacy.clone();
            full.view_bg = Some(Color::WHITE);
            full.view_fg = Some(Color::from_rgb(0.12, 0.12, 0.12));
            full.alternate_bg = Some(Color::from_rgb(0.97, 0.97, 0.97));
            full.inactive_fg = Some(Color::from_rgb(0.44, 0.45, 0.46));
            full.link = Some(Color::from_rgb(0.16, 0.5, 0.73));
            full.link_visited = Some(Color::from_rgb(0.61, 0.35, 0.71));
            full.error = Some(Color::from_rgb(0.85, 0.27, 0.33));
            full.warning = Some(Color::from_rgb(0.96, 0.45, 0.0));
            full.success = Some(Color::from_rgb(0.15, 0.68, 0.38));
            full.focus = Some(Color::from_rgb(0.24, 0.68, 0.91));

            out.extend([bare, accent_only, legacy, full]);
        }
        out
    }

    fn assert_same(new: &Theme, old: &Theme, what: &str) {
        assert_eq!(new.id, old.id, "{what}: id");
        assert_eq!(new.colors, old.colors, "{what}: colours");
        assert_eq!(new.input, old.input, "{what}: input tokens");
        assert_eq!(new, old, "{what}: theme");
    }

    #[test]
    fn an_appearance_change_resolves_as_before() {
        for os in reports() {
            for hint in [None, Some(false), Some(true)] {
                let desktop = NativeDesktop::new(os.clone());
                let new = theme_for_appearance_change(None, &desktop, hint);
                let old = old_appearance_change(&os, hint);
                assert_same(&new, &old, &format!("{os:?} with hint {hint:?}"));
            }
        }
    }

    #[test]
    fn a_new_window_resolves_as_before() {
        for os in reports() {
            let desktop = NativeDesktop::new(os.clone());
            assert_same(
                &theme_for_new_window(None, &desktop),
                &old_new_window(&os),
                &format!("{os:?}"),
            );
        }
    }

    #[test]
    fn without_a_factory_the_theme_is_intui_from_the_report() {
        for os in reports() {
            for dark in [false, true] {
                let desktop = NativeDesktop::new(os.clone());
                let request = NativeThemeRequest::new(&desktop, dark, TargetDensity::Touch);
                assert_eq!(
                    resolve_native_theme(None, &request),
                    presets::intui::from_os(&os, dark),
                    "{os:?}, dark {dark}"
                );
            }
        }
    }

    #[test]
    fn a_factory_answering_some_wins() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let factory: NativeThemeFactory = Box::new(move |request: &NativeThemeRequest<'_>| {
            log.borrow_mut()
                .push((request.desktop.clone(), request.dark, request.density));
            Some(presets::intui::dark().with_id("app.native"))
        });

        let mut os = OsThemeColors::new(ColorSchemePreference::Light);
        os.accent = Some(Color::from_rgb(0.9, 0.2, 0.1));
        let desktop = NativeDesktop::new(os);
        let request = NativeThemeRequest::new(&desktop, false, TargetDensity::Comfortable);
        assert_eq!(
            resolve_native_theme(Some(&factory), &request),
            presets::intui::dark().with_id("app.native")
        );
        assert_eq!(
            *seen.borrow(),
            vec![(desktop.clone(), false, TargetDensity::Comfortable)]
        );

        // The window manager stamps its id over the factory's.
        let theme = theme_for_new_window(Some(&factory), &desktop);
        assert_eq!(theme, presets::intui::dark().with_id("system"));
        let theme = theme_for_appearance_change(Some(&factory), &desktop, Some(true));
        assert_eq!(theme, presets::intui::dark().with_id("system"));
        assert_eq!(seen.borrow().len(), 3);
        assert!(!seen.borrow()[1].1);
        assert_eq!(seen.borrow()[1].2, TargetDensity::Compact);
        // A desktop stating a light preference is light whatever the hint.
        assert!(!seen.borrow()[2].1);
    }

    #[test]
    fn a_factory_answering_none_falls_back() {
        let calls = Rc::new(RefCell::new(0));
        let count = calls.clone();
        let factory: NativeThemeFactory = Box::new(move |_: &NativeThemeRequest<'_>| {
            *count.borrow_mut() += 1;
            None
        });

        for os in reports() {
            let desktop = NativeDesktop::new(os.clone());
            for dark in [false, true] {
                let request = NativeThemeRequest::new(&desktop, dark, TargetDensity::Compact);
                assert_eq!(
                    resolve_native_theme(Some(&factory), &request),
                    resolve_native_theme(None, &request),
                    "{os:?}, dark {dark}"
                );
            }
            for hint in [None, Some(false), Some(true)] {
                assert_eq!(
                    theme_for_appearance_change(Some(&factory), &desktop, hint),
                    old_appearance_change(&os, hint),
                    "{os:?} with hint {hint:?}"
                );
            }
            assert_eq!(
                theme_for_new_window(Some(&factory), &desktop),
                old_new_window(&os),
                "{os:?}"
            );
        }
        assert_eq!(*calls.borrow(), reports().len() * (2 + 3 + 1));
    }

    #[test]
    fn a_desktop_with_no_preference_is_asked_for_by_the_hint() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let factory: NativeThemeFactory = Box::new(move |request: &NativeThemeRequest<'_>| {
            log.borrow_mut()
                .push((request.desktop.os.clone(), request.dark));
            None
        });
        let mut os = OsThemeColors::new(ColorSchemePreference::NoPreference);
        os.accent = Some(Color::from_rgb(0.9, 0.2, 0.1));
        let desktop = NativeDesktop::new(os);

        theme_for_appearance_change(Some(&factory), &desktop, Some(true));
        theme_for_appearance_change(Some(&factory), &desktop, None);
        assert_eq!(
            *seen.borrow(),
            vec![
                (OsThemeColors::new(ColorSchemePreference::Dark), true),
                (OsThemeColors::new(ColorSchemePreference::Light), false),
            ]
        );
    }
}
