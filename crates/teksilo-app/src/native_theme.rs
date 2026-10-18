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
//! [`resolve_native_theme`] sets no id and applies no density. The window
//! manager resolves every theme that follows the OS, under
//! [`ThemeMode::FollowSystem`](crate::ThemeMode::FollowSystem) as under Native
//! mode, through one function too: it gives the theme the id `"system"` and
//! the density of the window it is for, and a re-read of the desktop that
//! changes nothing a theme was resolved from sets no theme. The previewer
//! applies its own density.

use teksilo_core::Theme;
use teksilo_core::presets;
use teksilo_platform::os_theme::NativeDesktop;
use teksilo_tokens::{ColorSchemePreference, TargetDensity};

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

/// The desktop reads a theme that follows the OS is made from, and the
/// accessibility preferences read again with it when the desktop changes.
/// The window manager reads the platform's own ([`OsQueries::PLATFORM`]);
/// its tests hand it fixed answers.
#[derive(Clone, Copy)]
pub(crate) struct OsQueries {
    /// What the desktop reports for [`ThemeMode::Native`](crate::ThemeMode::Native).
    pub(crate) native_desktop: fn() -> NativeDesktop,
    /// The desktop's light or dark preference, for
    /// [`ThemeMode::FollowSystem`](crate::ThemeMode::FollowSystem).
    pub(crate) color_scheme: fn() -> ColorSchemePreference,
    /// The accessibility preferences, among them reduced motion, read again
    /// whenever the desktop's appearance is heard to change.
    pub(crate) accessibility: fn() -> teksilo_platform::AccessibilityPreferences,
}

impl OsQueries {
    /// The platform's reads,
    /// [`query_native_desktop`](teksilo_platform::os_theme::query_native_desktop),
    /// [`query_color_scheme`](teksilo_platform::os_theme::query_color_scheme)
    /// and
    /// [`AccessibilityPreferences::query`](teksilo_platform::AccessibilityPreferences::query).
    pub(crate) const PLATFORM: OsQueries = OsQueries {
        native_desktop: teksilo_platform::os_theme::query_native_desktop,
        color_scheme: teksilo_platform::os_theme::query_color_scheme,
        accessibility: teksilo_platform::AccessibilityPreferences::query,
    };

    /// Whether [`ThemeMode::FollowSystem`](crate::ThemeMode::FollowSystem)
    /// is dark: the desktop's light or dark preference when it states one,
    /// otherwise `hint`, the window system's light or dark state, with no
    /// hint meaning light. A window that opens and a change heard later both
    /// decide here, so they cannot disagree.
    pub(crate) fn follow_system_dark(self, hint: Option<bool>) -> bool {
        match (self.color_scheme)() {
            ColorSchemePreference::Dark => true,
            ColorSchemePreference::Light => false,
            ColorSchemePreference::NoPreference => hint.unwrap_or(false),
        }
    }
}

/// What a theme that follows the OS is resolved from, apart from the density
/// it is wanted at.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OsThemeSource {
    /// [`ThemeMode::FollowSystem`](crate::ThemeMode::FollowSystem): the IntUI
    /// light or dark preset.
    FollowSystem {
        /// Whether the theme is dark.
        dark: bool,
    },
    /// [`ThemeMode::Native`](crate::ThemeMode::Native): the desktop as it was
    /// read, and whether the theme is dark.
    Native {
        /// What the desktop reported, boxed: a KDE desktop carries its whole
        /// colour scheme.
        desktop: Box<NativeDesktop>,
        /// Whether the theme is dark.
        dark: bool,
    },
}

impl OsThemeSource {
    /// The IntUI light or dark preset.
    pub(crate) fn follow_system(dark: bool) -> Self {
        OsThemeSource::FollowSystem { dark }
    }

    /// The theme Native mode makes of `desktop`. A desktop that states a
    /// light or dark preference decides `dark` itself; one that states none
    /// (macOS, Windows, a Linux desktop the portal answers for with no
    /// preference) follows `hint`, the window system's light or dark state,
    /// and is light when there is none.
    pub(crate) fn native(desktop: NativeDesktop, hint: Option<bool>) -> Self {
        let dark = match desktop.os.color_scheme {
            ColorSchemePreference::Dark => true,
            ColorSchemePreference::Light => false,
            ColorSchemePreference::NoPreference => hint.unwrap_or(false),
        };
        OsThemeSource::Native {
            desktop: Box::new(desktop),
            dark,
        }
    }

    /// This source asked for at `density`.
    pub(crate) fn at(self, density: TargetDensity) -> OsThemeInputs {
        OsThemeInputs {
            source: self,
            density,
        }
    }
}

/// Everything a theme that follows the OS is resolved from: equal inputs
/// resolve to equal themes, so a re-read that gives the inputs a theme was
/// last resolved from needs no new theme.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OsThemeInputs {
    /// The mode's reading of the desktop.
    pub(crate) source: OsThemeSource,
    /// The density the theme is wanted at.
    pub(crate) density: TargetDensity,
}

/// The theme that follows the OS for `inputs`, with the id `"system"`: the
/// theme the source resolves to, projected onto `inputs.density` by
/// [`Theme::with_density`].
///
/// Under [`OsThemeSource::Native`] the theme is [`resolve_native_theme`]'s.
/// A desktop that states no preference is asked for with its report
/// stating the light or dark preference `dark` chose, so the factory and the
/// colours of [`presets::intui::from_os`] are those of a light or dark
/// desktop.
pub(crate) fn resolve_os_theme(
    factory: Option<&NativeThemeFactory>,
    inputs: &OsThemeInputs,
) -> Theme {
    let theme = match &inputs.source {
        OsThemeSource::FollowSystem { dark: true } => presets::intui::dark(),
        OsThemeSource::FollowSystem { dark: false } => presets::intui::light(),
        OsThemeSource::Native { desktop, dark } => {
            if desktop.os.color_scheme == ColorSchemePreference::NoPreference {
                let mut stated = NativeDesktop::clone(desktop);
                stated.os.color_scheme = if *dark {
                    ColorSchemePreference::Dark
                } else {
                    ColorSchemePreference::Light
                };
                resolve_native_theme(
                    factory,
                    &NativeThemeRequest::new(&stated, *dark, inputs.density),
                )
            } else {
                resolve_native_theme(
                    factory,
                    &NativeThemeRequest::new(desktop, *dark, inputs.density),
                )
            }
        }
    };
    theme.with_density(inputs.density).with_id("system")
}

/// One re-read of the OS appearance, brought to every theme that follows it:
/// each window's and the window manager's own.
///
/// Each of those remembers the [`OsThemeInputs`] its theme was last resolved
/// from. [`theme_for`](Self::theme_for) answers `None` for one whose inputs,
/// at its own density, are unchanged, so it is not set again, and resolves
/// each density once however many windows share it.
pub(crate) struct OsThemeUpdate<'a> {
    /// The app's own Native-mode theme, if any.
    factory: Option<&'a NativeThemeFactory>,
    /// What the OS was read as.
    source: OsThemeSource,
    /// The themes resolved so far, by density.
    resolved: Vec<(TargetDensity, Theme)>,
}

impl<'a> OsThemeUpdate<'a> {
    /// An update to the themes `source` resolves to.
    pub(crate) fn new(factory: Option<&'a NativeThemeFactory>, source: OsThemeSource) -> Self {
        OsThemeUpdate {
            factory,
            source,
            resolved: Vec::new(),
        }
    }

    /// The theme to set on a target whose theme is at `density` and was last
    /// resolved from `applied` (`None` when it does not follow the OS), or
    /// `None` when `applied` is already this update's inputs at `density`.
    /// `applied` becomes those inputs.
    pub(crate) fn theme_for(
        &mut self,
        applied: &mut Option<OsThemeInputs>,
        density: TargetDensity,
    ) -> Option<Theme> {
        let inputs = self.source.clone().at(density);
        if applied.as_ref() == Some(&inputs) {
            return None;
        }
        let theme = match self.resolved.iter().find(|(at, _)| *at == density) {
            Some((_, theme)) => theme.clone(),
            None => {
                let theme = resolve_os_theme(self.factory, &inputs);
                self.resolved.push((density, theme.clone()));
                theme
            }
        };
        *applied = Some(inputs);
        Some(theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_core::WidgetTree;
    use teksilo_tokens::{Color, ColorTokens, InputTokens, OsThemeColors};

    const DENSITIES: [TargetDensity; 3] = [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ];

    /// The Native-mode theme the window manager made of a desktop stating a
    /// light or dark preference before densities were kept: IntUI light or
    /// dark under the report's colours, at the presets' Compact density.
    fn old_stated(os: &OsThemeColors) -> Theme {
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

    /// Every source the window manager can read: both FollowSystem presets,
    /// and Native over every report with no hint and both hint values.
    fn sources() -> Vec<OsThemeSource> {
        let mut out = vec![
            OsThemeSource::follow_system(false),
            OsThemeSource::follow_system(true),
        ];
        for os in reports() {
            for hint in [None, Some(false), Some(true)] {
                out.push(OsThemeSource::native(NativeDesktop::new(os.clone()), hint));
            }
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
    fn a_desktop_stating_a_preference_resolves_as_before_at_compact() {
        for os in reports() {
            if os.color_scheme == ColorSchemePreference::NoPreference {
                continue;
            }
            for hint in [None, Some(false), Some(true)] {
                let inputs = OsThemeSource::native(NativeDesktop::new(os.clone()), hint)
                    .at(TargetDensity::Compact);
                assert_same(
                    &resolve_os_theme(None, &inputs),
                    &old_stated(&os),
                    &format!("{os:?} with hint {hint:?}"),
                );
            }
        }
    }

    #[test]
    fn follow_system_resolves_to_the_intui_presets() {
        for dark in [false, true] {
            let preset = if dark {
                presets::intui::dark()
            } else {
                presets::intui::light()
            };
            for density in DENSITIES {
                assert_same(
                    &resolve_os_theme(None, &OsThemeSource::follow_system(dark).at(density)),
                    &preset.with_density(density).with_id("system"),
                    &format!("dark {dark} at {density:?}"),
                );
            }
        }
    }

    #[test]
    fn the_density_asked_for_is_the_density_given() {
        for source in sources() {
            let compact = resolve_os_theme(None, &source.clone().at(TargetDensity::Compact));
            assert_eq!(
                compact.input,
                InputTokens::for_density(TargetDensity::Compact)
            );
            for density in DENSITIES {
                let theme = resolve_os_theme(None, &source.clone().at(density));
                assert_eq!(theme.input.density, density, "{source:?} at {density:?}");
                assert_same(
                    &theme,
                    &compact.with_density(density),
                    &format!("{source:?} at {density:?}"),
                );
            }
        }
    }

    /// A density projection that marks the theme it projects, so a test can
    /// tell that it ran.
    fn marking_projection(theme: &Theme, density: TargetDensity) -> Theme {
        let mut colors = theme.colors.clone();
        colors.accent = match density {
            TargetDensity::Compact => Color::from_rgb(1.0, 0.0, 0.0),
            TargetDensity::Comfortable => Color::from_rgb(0.0, 1.0, 0.0),
            TargetDensity::Touch => Color::from_rgb(0.0, 0.0, 1.0),
        };
        Theme {
            input: InputTokens::for_density(density),
            colors,
            ..theme.clone()
        }
    }

    #[test]
    fn a_factory_theme_is_projected_by_its_own_density_projection() {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let log = asked.clone();
        let factory: NativeThemeFactory = Box::new(move |request: &NativeThemeRequest<'_>| {
            log.borrow_mut().push(request.density);
            Some(
                presets::intui::dark()
                    .with_id("app.native")
                    .with_density_projection(marking_projection),
            )
        });
        let desktop = NativeDesktop::new(OsThemeColors::new(ColorSchemePreference::Dark));
        for density in DENSITIES {
            let theme = resolve_os_theme(
                Some(&factory),
                &OsThemeSource::native(desktop.clone(), None).at(density),
            );
            let want = presets::intui::dark()
                .with_id("app.native")
                .with_density_projection(marking_projection)
                .with_density(density)
                .with_id("system");
            assert_eq!(theme.colors, want.colors, "{density:?}");
            assert_eq!(theme, want, "{density:?}");
        }
        assert_eq!(*asked.borrow(), DENSITIES.to_vec());
    }

    #[test]
    fn a_desktop_with_no_preference_follows_the_hint() {
        for os in reports() {
            if os.color_scheme != ColorSchemePreference::NoPreference {
                continue;
            }
            for hint in [None, Some(false), Some(true)] {
                let dark = hint == Some(true);
                let mut stated = os.clone();
                stated.color_scheme = if dark {
                    ColorSchemePreference::Dark
                } else {
                    ColorSchemePreference::Light
                };
                let inputs = OsThemeSource::native(NativeDesktop::new(os.clone()), hint)
                    .at(TargetDensity::Compact);
                let theme = resolve_os_theme(None, &inputs);
                assert_eq!(theme.is_dark(), dark, "{os:?} with hint {hint:?}");
                assert_same(
                    &theme,
                    &old_stated(&stated),
                    &format!("{os:?} with hint {hint:?}"),
                );
            }
        }
        // With no colours, the bare presets.
        let bare = NativeDesktop::new(OsThemeColors::new(ColorSchemePreference::NoPreference));
        let at = |hint| {
            resolve_os_theme(
                None,
                &OsThemeSource::native(bare.clone(), hint).at(TargetDensity::Compact),
            )
        };
        assert_eq!(at(Some(true)), presets::intui::dark().with_id("system"));
        assert_eq!(at(Some(false)), presets::intui::light().with_id("system"));
        assert_eq!(at(None), presets::intui::light().with_id("system"));
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

        // The window manager stamps its id over the factory's, and gives the
        // theme the density it asked for.
        let theme = resolve_os_theme(
            Some(&factory),
            &OsThemeSource::native(desktop.clone(), Some(true)).at(TargetDensity::Touch),
        );
        assert_eq!(
            theme,
            presets::intui::dark()
                .with_density(TargetDensity::Touch)
                .with_id("system")
        );
        assert_eq!(seen.borrow().len(), 2);
        // A desktop stating a light preference is light whatever the hint.
        assert_eq!(seen.borrow()[1], (desktop, false, TargetDensity::Touch));
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
                let inputs = OsThemeSource::native(desktop.clone(), hint).at(TargetDensity::Touch);
                assert_eq!(
                    resolve_os_theme(Some(&factory), &inputs),
                    resolve_os_theme(None, &inputs),
                    "{os:?} with hint {hint:?}"
                );
            }
        }
        assert_eq!(*calls.borrow(), reports().len() * (2 + 3));
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
        let desktop = NativeDesktop::new(os.clone());

        for hint in [Some(true), None] {
            resolve_os_theme(
                Some(&factory),
                &OsThemeSource::native(desktop.clone(), hint).at(TargetDensity::Compact),
            );
        }
        let mut dark = os.clone();
        dark.color_scheme = ColorSchemePreference::Dark;
        let mut light = os;
        light.color_scheme = ColorSchemePreference::Light;
        assert_eq!(*seen.borrow(), vec![(dark, true), (light, false)]);
    }

    /// A window's tree and what its theme was last resolved from.
    struct Target {
        tree: WidgetTree,
        applied: Option<OsThemeInputs>,
    }

    impl Target {
        fn at(density: TargetDensity) -> Self {
            let mut tree = WidgetTree::new();
            tree.set_input_density(density);
            Target {
                tree,
                applied: None,
            }
        }

        /// How many times the tree's theme has been set.
        fn sets(&self) -> u64 {
            self.tree.theme_signal().generation()
        }
    }

    /// Bring `targets` to `source` as the window manager does, and say how
    /// many times the factory was asked.
    fn apply(source: &OsThemeSource, targets: &mut [&mut Target]) -> usize {
        let calls = Rc::new(RefCell::new(0));
        let count = calls.clone();
        let factory: NativeThemeFactory = Box::new(move |_: &NativeThemeRequest<'_>| {
            *count.borrow_mut() += 1;
            None
        });
        let mut update = OsThemeUpdate::new(Some(&factory), source.clone());
        for target in targets.iter_mut() {
            if let Some(theme) = update.theme_for(&mut target.applied, target.tree.input_density())
            {
                target.tree.set_theme(theme);
            }
        }
        *calls.borrow()
    }

    #[test]
    fn each_target_keeps_its_density() {
        let mut desk = OsThemeColors::new(ColorSchemePreference::Dark);
        desk.accent = Some(Color::from_rgb(0.9, 0.2, 0.1));
        let source = OsThemeSource::native(NativeDesktop::new(desk), None);
        for density in DENSITIES {
            let mut target = Target::at(density);
            apply(&source, &mut [&mut target]);
            assert_eq!(target.tree.input_density(), density);
            assert_eq!(
                *target.tree.theme(),
                resolve_os_theme(None, &source.clone().at(TargetDensity::Compact))
                    .with_density(density),
                "{density:?}"
            );
        }
    }

    #[test]
    fn an_unchanged_reading_sets_no_theme() {
        let light = OsThemeSource::native(
            NativeDesktop::new(OsThemeColors::new(ColorSchemePreference::Light)),
            None,
        );
        let mut compact = Target::at(TargetDensity::Compact);
        let mut touch = Target::at(TargetDensity::Touch);
        let mut touch_too = Target::at(TargetDensity::Touch);

        // The first reading sets every theme, resolving each density once.
        let before = [compact.sets(), touch.sets(), touch_too.sets()];
        assert_eq!(
            apply(&light, &mut [&mut compact, &mut touch, &mut touch_too]),
            2
        );
        assert_eq!(
            [compact.sets(), touch.sets(), touch_too.sets()],
            before.map(|sets| sets + 1)
        );

        // The same reading again sets nothing and asks the factory nothing.
        let before = [compact.sets(), touch.sets(), touch_too.sets()];
        assert_eq!(
            apply(&light, &mut [&mut compact, &mut touch, &mut touch_too]),
            0
        );
        assert_eq!([compact.sets(), touch.sets(), touch_too.sets()], before);

        // A changed desktop sets every theme.
        let mut accented = OsThemeColors::new(ColorSchemePreference::Light);
        accented.accent = Some(Color::from_rgb(0.9, 0.2, 0.1));
        let changed = OsThemeSource::native(NativeDesktop::new(accented), None);
        apply(&changed, &mut [&mut compact, &mut touch]);
        assert_eq!(
            [compact.sets(), touch.sets()],
            [before[0] + 1, before[1] + 1]
        );

        // A changed light or dark state sets every theme.
        let no_preference =
            NativeDesktop::new(OsThemeColors::new(ColorSchemePreference::NoPreference));
        let mut target = Target::at(TargetDensity::Compact);
        apply(
            &OsThemeSource::native(no_preference.clone(), Some(false)),
            &mut [&mut target],
        );
        let sets = target.sets();
        apply(
            &OsThemeSource::native(no_preference.clone(), None),
            &mut [&mut target],
        );
        assert_eq!(target.sets(), sets, "no hint reads as light");
        apply(
            &OsThemeSource::native(no_preference, Some(true)),
            &mut [&mut target],
        );
        assert_eq!(target.sets(), sets + 1);
        assert!(target.tree.theme().is_dark());

        let mut follow = Target::at(TargetDensity::Compact);
        apply(&OsThemeSource::follow_system(false), &mut [&mut follow]);
        let sets = follow.sets();
        apply(&OsThemeSource::follow_system(false), &mut [&mut follow]);
        assert_eq!(follow.sets(), sets);
        apply(&OsThemeSource::follow_system(true), &mut [&mut follow]);
        assert_eq!(follow.sets(), sets + 1);

        // A changed density sets that target's theme alone, at its density.
        let before = [compact.sets(), touch.sets()];
        compact.tree.set_input_density(TargetDensity::Comfortable);
        let switched = compact.sets();
        assert_eq!(switched, before[0] + 1);
        apply(&changed, &mut [&mut compact, &mut touch]);
        assert_eq!([compact.sets(), touch.sets()], [switched + 1, before[1]]);
        assert_eq!(compact.tree.input_density(), TargetDensity::Comfortable);
    }
}
