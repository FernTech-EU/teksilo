// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The KDE Plasma reader behind `ThemeMode::Native` and the light or dark
//! preference.
//!
//! Everything KDE-specific is in `teksilo-kde-scheme`: where the files are
//! ([`KdeConfigLocations`]), how they cascade ([`KConfig::cascade`]), which
//! colour scheme is in effect and how the user's accent applies to it
//! ([`KdeDesktop::from_layers`]). This module only finds which of the
//! listed files exist, reads them through the `read` closure it is given,
//! and hands their contents over.
//!
//! The readers are pure: the environment arrives as a getter and the files
//! through `read`, so the tests run over fixture trees held in memory. The
//! two impure helpers at the end ([`process_env`], [`read_file`]) are what
//! the real queries pass in.
//!
//! What is read, per `engineering/docs/kde-breeze-spec.md`:
//!
//! - the `kdeglobals` cascade (b.7): `/etc/kde5rc`, every `system.kdeglobals`
//!   then every `kdeglobals` of `$XDG_CONFIG_DIRS` (with
//!   `$XDG_CONFIG_HOME/kdedefaults` first in a KDE session), then the user's
//!   own files, with Kiosk immutability (b.6);
//! - the colours: the `[Colors:*]` groups of `kdeglobals` when it has a
//!   `[Colors:View]` group, which already carry any accent the user chose;
//!   otherwise the look-and-feel package's colours, else the scheme named
//!   by `[General] ColorScheme`, else Breeze Light (b.8), with `[General]
//!   AccentColor` then applied to it when it is a colour and not
//!   transparent (n.13);
//! - the fonts, `[KDE] AnimationDurationFactor` and `[KDE] widgetStyle`
//!   (j.1, j.2, j.4), and the `breezerc` cascade (j.6).
//!
//! `forceFontDPI` is not read: no Plasma 6.8 component reads it (j.3).

use std::path::Path;

use teksilo_kde_scheme::{
    ConfigLayerPath, KConfig, KdeConfigLocations, KdeDesktop, KdeDesktopLayers,
};
use teksilo_tokens::OsThemeColors;

use super::NativeDesktop;

/// An environment getter: a variable's value, `None` when unset.
pub(crate) type EnvVar<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The KDE desktop's settings, or `None` when no file of the `kdeglobals`
/// cascade can be read.
///
/// `var` gives the environment `[$e]` entries expand in; `read` returns a
/// file's bytes, `None` for a file that does not exist or cannot be read.
pub(crate) fn discover_kde(
    locations: &KdeConfigLocations,
    var: EnvVar<'_>,
    read: impl Fn(&Path) -> Option<Vec<u8>>,
) -> Option<KdeDesktop> {
    let colours = ColourFiles::read(locations, var, &read)?;
    let breezerc = read_cascade(&locations.breezerc_cascade(), &read);
    Some(KdeDesktop::from_layers(
        &KdeDesktopLayers {
            kdeglobals: &colours.kdeglobals,
            breezerc: &breezerc,
            scheme_file: colours.scheme_file.as_ref(),
        },
        &locations.expansion_env(var),
    ))
}

/// Whether the KDE colour scheme in effect is dark
/// ([`KdeColorScheme::is_dark`](teksilo_kde_scheme::KdeColorScheme::is_dark)),
/// reading only the `kdeglobals` cascade and the scheme file; `None` when no
/// file of the `kdeglobals` cascade can be read.
///
/// The scheme is the one [`discover_kde`] finds, accent included, so the
/// two never disagree.
pub(crate) fn kde_prefers_dark(
    locations: &KdeConfigLocations,
    var: EnvVar<'_>,
    read: impl Fn(&Path) -> Option<Vec<u8>>,
) -> Option<bool> {
    let colours = ColourFiles::read(locations, var, &read)?;
    let desktop = KdeDesktop::from_layers(
        &KdeDesktopLayers {
            kdeglobals: &colours.kdeglobals,
            breezerc: &[],
            scheme_file: colours.scheme_file.as_ref(),
        },
        &locations.expansion_env(var),
    );
    Some(desktop.scheme.is_dark())
}

/// Whether `kdeglobals [KDE] AnimationDurationFactor` is 0, which KDE
/// reports as reduced motion
/// ([`KdeDesktop::prefers_reduced_motion`]), reading only the `kdeglobals`
/// cascade; `None` when no file of it can be read.
pub(crate) fn kde_prefers_reduced_motion(
    locations: &KdeConfigLocations,
    var: EnvVar<'_>,
    read: impl Fn(&Path) -> Option<Vec<u8>>,
) -> Option<bool> {
    let kdeglobals = read_cascade(&locations.kdeglobals_cascade(), &read);
    if kdeglobals.is_empty() {
        return None;
    }
    let desktop = KdeDesktop::from_layers(
        &KdeDesktopLayers {
            kdeglobals: &kdeglobals,
            ..KdeDesktopLayers::default()
        },
        &locations.expansion_env(var),
    );
    Some(desktop.prefers_reduced_motion())
}

/// The native desktop of a KDE session.
///
/// With a readable `kdeglobals` the colour report is the scheme's own
/// ([`KdeColorScheme::os_theme_colors`](teksilo_kde_scheme::KdeColorScheme::os_theme_colors)),
/// accent included, and `without_kdeglobals` is not called. Without one,
/// the report is whatever `without_kdeglobals` gives (the portal's light or
/// dark preference and accent colour), and the desktop carries no
/// [`KdeDesktop`].
pub(crate) fn native_desktop(
    kde: Option<KdeDesktop>,
    without_kdeglobals: impl FnOnce() -> OsThemeColors,
) -> NativeDesktop {
    match kde {
        Some(desktop) => NativeDesktop {
            os: desktop.scheme.os_theme_colors(),
            kde: Some(desktop),
        },
        None => NativeDesktop::new(without_kdeglobals()),
    }
}

/// The files a colour scheme is read from: the `kdeglobals` cascade, and
/// the scheme file when `kdeglobals` does not carry the colours itself.
struct ColourFiles {
    kdeglobals: Vec<KConfig>,
    scheme_file: Option<KConfig>,
}

impl ColourFiles {
    /// `None` when no file of the `kdeglobals` cascade can be read.
    fn read(
        locations: &KdeConfigLocations,
        var: EnvVar<'_>,
        read: &impl Fn(&Path) -> Option<Vec<u8>>,
    ) -> Option<ColourFiles> {
        let kdeglobals = read_cascade(&locations.kdeglobals_cascade(), read);
        if kdeglobals.is_empty() {
            return None;
        }
        let merged = KConfig::cascade(&kdeglobals);
        // The first candidate that can be read is the scheme file.
        let scheme_file = locations
            .scheme_fallback_candidates(&merged, &locations.expansion_env(var))
            .and_then(|candidates| candidates.iter().find_map(|path| read(path)))
            .map(|bytes| KConfig::parse_bytes(&bytes).0);
        Some(ColourFiles {
            kdeglobals,
            scheme_file,
        })
    }
}

/// Every readable file of a cascade, parsed and marked with its layer
/// kind, lowest priority first. A file that cannot be read is skipped, as
/// KDE skips a file that does not exist; parse diagnostics are dropped, the
/// offending lines having been skipped.
fn read_cascade(
    layers: &[ConfigLayerPath],
    read: &impl Fn(&Path) -> Option<Vec<u8>>,
) -> Vec<KConfig> {
    layers
        .iter()
        .filter_map(|layer| {
            read(&layer.path).map(|bytes| KConfig::parse_bytes(&bytes).0.with_kind(layer.kind))
        })
        .collect()
}

/// The process environment, for the real queries.
pub(crate) fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Reads a file of the real file system; any failure reads as absent.
pub(crate) fn read_file(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

#[cfg(test)]
mod tests {
    //! Fixture trees held in memory: no test reads the real home, the real
    //! `/etc/xdg` or the process environment.

    use std::collections::HashMap;
    use std::path::PathBuf;

    use teksilo_kde_scheme::{KdeColor, KdeColorScheme, SchemeOrigin};
    use teksilo_tokens::{Color, ColorSchemePreference};

    use super::*;

    /// A file tree in memory.
    #[derive(Default)]
    struct Tree(HashMap<PathBuf, Vec<u8>>);

    impl Tree {
        fn file(mut self, path: &str, text: &str) -> Tree {
            self.0.insert(PathBuf::from(path), text.as_bytes().to_vec());
            self
        }

        fn read(&self) -> impl Fn(&Path) -> Option<Vec<u8>> + '_ {
            |path| self.0.get(path).cloned()
        }
    }

    /// An environment getter over a fixed table.
    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let table: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| table.get(name).cloned()
    }

    /// A KDE session of user ada, with the default XDG directories.
    fn kde_env() -> impl Fn(&str) -> Option<String> + use<> {
        env(&[("HOME", "/home/ada"), ("XDG_CURRENT_DESKTOP", "KDE")])
    }

    fn discover(tree: &Tree, var: EnvVar<'_>) -> Option<KdeDesktop> {
        discover_kde(&KdeConfigLocations::from_env(var), var, tree.read())
    }

    fn prefers_dark(tree: &Tree) -> Option<bool> {
        let var = kde_env();
        kde_prefers_dark(&KdeConfigLocations::from_env(&var), &var, tree.read())
    }

    fn reduced_motion(tree: &Tree) -> Option<bool> {
        let var = kde_env();
        kde_prefers_reduced_motion(&KdeConfigLocations::from_env(&var), &var, tree.read())
    }

    const USER_KDEGLOBALS: &str = "/home/ada/.config/kdeglobals";
    const KDEDEFAULTS: &str = "/home/ada/.config/kdedefaults/kdeglobals";
    const ETC_KDEGLOBALS: &str = "/etc/xdg/kdeglobals";
    const LIGHT_FILE: &str = "/usr/share/color-schemes/BreezeLight.colors";
    const DARK_FILE: &str = "/usr/share/color-schemes/BreezeDark.colors";
    const LOOK_AND_FEEL: &str =
        "/usr/share/plasma/look-and-feel/org.kde.breeze.desktop/contents/colors";

    fn light_text() -> String {
        KdeColorScheme::breeze_light().to_kconfig_text()
    }

    fn dark_text() -> String {
        KdeColorScheme::breeze_dark().to_kconfig_text()
    }

    fn rgb(r: u8, g: u8, b: u8) -> Option<Color> {
        Some(KdeColor::rgb8(r, g, b).to_color())
    }

    // ---- kdeglobals carries the colours --------------------------------

    #[test]
    fn a_full_kdeglobals_gives_its_colours_fonts_and_settings() {
        let kdeglobals = format!(
            "{}\n[General]\nColorScheme=BreezeDark\nAccentColor=233,61,0\n\
             font=Inter,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n\
             fixed=JetBrains Mono,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n\
             [KDE]\nAnimationDurationFactor=0.5\nwidgetStyle=Breeze\nforceFontDPI=144\n",
            dark_text()
        );
        let tree = Tree::default()
            .file(USER_KDEGLOBALS, &kdeglobals)
            .file("/home/ada/.config/breezerc", "[Style]\nMenuOpacity=80\n")
            // A scheme file is not read when kdeglobals has the colours.
            .file(DARK_FILE, &light_text());
        let desktop = discover(&tree, &kde_env()).expect("a readable kdeglobals");

        assert_eq!(desktop.scheme_origin, SchemeOrigin::Kdeglobals);
        let os = desktop.scheme.os_theme_colors();
        assert_eq!(os, KdeColorScheme::breeze_dark().os_theme_colors());
        assert_eq!(os.color_scheme, ColorSchemePreference::Dark);
        // The accent is the selection colour already in the groups; the
        // stored AccentColor is recorded but not applied again.
        assert_eq!(os.accent, rgb(61, 174, 233));
        assert_eq!(desktop.user_accent, Some(KdeColor::rgb8(233, 61, 0)));

        assert_eq!(desktop.fonts.general.family, "Inter");
        assert_eq!(desktop.fonts.general.point_size, Some(11.0));
        assert_eq!(desktop.fonts.fixed.family, "JetBrains Mono");
        assert_eq!(desktop.fonts.small.family, "Noto Sans");
        assert_eq!(desktop.animation_duration_factor, Some(0.5));
        assert_eq!(desktop.widget_style.as_deref(), Some("Breeze"));
        assert!(desktop.uses_breeze_style());
        assert_eq!(desktop.breeze.menu_opacity, 80);
    }

    #[test]
    fn kdeglobals_colours_are_read_as_written_and_decide_dark() {
        // The fixture of the reader this one replaced: a scheme name that
        // says dark, and the colours to go with it.
        let tree = Tree::default().file(
            USER_KDEGLOBALS,
            "[General]\nColorScheme=Breeze-Dark\n\n\
             [Colors:View]\nBackgroundNormal=35,38,41\n\n\
             [Colors:Window]\nBackgroundNormal=49,54,59\nForegroundNormal=239,240,241\n\
             DecorationFocus=61,174,233\n",
        );
        let os = discover(&tree, &kde_env())
            .unwrap()
            .scheme
            .os_theme_colors();
        assert_eq!(os.window_bg, rgb(49, 54, 59));
        assert_eq!(os.window_fg, rgb(239, 240, 241));
        assert_eq!(os.view_bg, rgb(35, 38, 41));
        assert_eq!(os.color_scheme, ColorSchemePreference::Dark);
        // The accent is the selection colour, not the window's focus
        // decoration: the Selection set is absent, so KDE's default.
        let defaults = KdeColorScheme::kcolorscheme_defaults();
        let selection = defaults.set(teksilo_kde_scheme::ColorSet::Selection);
        assert_eq!(os.accent, Some(selection.background_normal.to_color()));
        assert_eq!(prefers_dark(&tree), Some(true));
    }

    #[test]
    fn no_kdeglobals_at_all_is_no_kde_desktop() {
        let tree = Tree::default().file(LIGHT_FILE, &light_text());
        assert_eq!(discover(&tree, &kde_env()), None);
        assert_eq!(prefers_dark(&tree), None);
        assert_eq!(reduced_motion(&tree), None);
    }

    // ---- The cascade ---------------------------------------------------

    #[test]
    fn this_machines_layout_reads_kdedefaults_over_etc_xdg() {
        // The scheme is named only in kdedefaults and /etc/xdg; the user's
        // kdeglobals has fonts but no colours.
        let tree = Tree::default()
            .file(KDEDEFAULTS, "[General]\nColorScheme=BreezeDark\n")
            .file(
                ETC_KDEGLOBALS,
                "[General]\nColorScheme=BreezeLight\n[KDE]\nwidgetStyle=Breeze\n",
            )
            .file(
                USER_KDEGLOBALS,
                "[General]\nfont=Noto Sans,12,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n",
            )
            .file(LIGHT_FILE, &light_text())
            .file(DARK_FILE, &dark_text());

        let desktop = discover(&tree, &kde_env()).unwrap();
        assert_eq!(desktop.scheme_origin, SchemeOrigin::SchemeFile);
        assert!(desktop.scheme.is_dark());
        assert_eq!(
            desktop.scheme.os_theme_colors(),
            KdeColorScheme::breeze_dark().os_theme_colors()
        );
        assert_eq!(desktop.fonts.general.point_size, Some(12.0));
        assert_eq!(desktop.widget_style.as_deref(), Some("Breeze"));
        assert_eq!(prefers_dark(&tree), Some(true));

        // Outside a KDE session kdedefaults is not prepended, so /etc/xdg
        // names the scheme.
        let outside = env(&[("HOME", "/home/ada")]);
        let desktop = discover(&tree, &outside).unwrap();
        assert!(!desktop.scheme.is_dark());

        // A session that lists XDG_CONFIG_DIRS itself is followed exactly,
        // kdedefaults still first.
        let listed = env(&[
            ("HOME", "/home/ada"),
            ("XDG_CURRENT_DESKTOP", "KDE"),
            ("XDG_CONFIG_DIRS", "/opt/xdg:/etc/xdg"),
        ]);
        let tree = tree.file(
            "/opt/xdg/kdeglobals",
            "[General]\nColorScheme=BreezeLight\n",
        );
        assert!(discover(&tree, &listed).unwrap().scheme.is_dark());
    }

    #[test]
    fn the_scheme_file_is_looked_for_in_the_data_home_first() {
        let mine = dark_text().replace("BackgroundNormal=32,35,38", "BackgroundNormal=10,20,30");
        let tree = Tree::default()
            .file(USER_KDEGLOBALS, "[General]\nColorScheme=BreezeDark\n")
            .file(DARK_FILE, &dark_text())
            .file(
                "/home/ada/.local/share/color-schemes/BreezeDark.colors",
                &mine,
            );
        let os = discover(&tree, &kde_env())
            .unwrap()
            .scheme
            .os_theme_colors();
        assert_eq!(os.window_bg, rgb(10, 20, 30));
    }

    #[test]
    fn the_look_and_feel_colours_come_before_the_named_scheme() {
        let tree = Tree::default()
            .file(USER_KDEGLOBALS, "[General]\nColorScheme=BreezeLight\n")
            .file(LIGHT_FILE, &light_text())
            .file(LOOK_AND_FEEL, &dark_text());
        let desktop = discover(&tree, &kde_env()).unwrap();
        assert_eq!(desktop.scheme_origin, SchemeOrigin::SchemeFile);
        assert!(desktop.scheme.is_dark());
    }

    #[test]
    fn without_any_scheme_file_breeze_light_is_built_in() {
        let tree = Tree::default().file(USER_KDEGLOBALS, "[General]\nColorScheme=Missing\n");
        let desktop = discover(&tree, &kde_env()).unwrap();
        assert_eq!(desktop.scheme_origin, SchemeOrigin::BuiltIn);
        assert_eq!(
            desktop.scheme.os_theme_colors(),
            KdeColorScheme::breeze_light().os_theme_colors()
        );
        assert_eq!(prefers_dark(&tree), Some(false));
    }

    #[test]
    fn an_immutable_group_in_a_lower_layer_freezes_it_for_the_user() {
        let tree = Tree::default()
            .file(
                ETC_KDEGLOBALS,
                "[KDE][$i]\nwidgetStyle=Fusion\n\n[General]\nColorScheme[$i]=BreezeLight\n",
            )
            .file(
                USER_KDEGLOBALS,
                "[KDE]\nwidgetStyle=Oxygen\nAnimationDurationFactor=0\n\n\
                 [General]\nColorScheme=BreezeDark\nfixed=Hack,12,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n",
            )
            .file(LIGHT_FILE, &light_text())
            .file(DARK_FILE, &dark_text());
        let desktop = discover(&tree, &kde_env()).unwrap();
        // The user's whole [KDE] group is ignored, keys the lower layer did
        // not set included.
        assert_eq!(desktop.widget_style.as_deref(), Some("Fusion"));
        assert!(!desktop.uses_breeze_style());
        assert_eq!(desktop.animation_duration_factor, None);
        assert!(!desktop.prefers_reduced_motion());
        // Only the immutable ColorScheme entry is frozen in [General].
        assert!(!desktop.scheme.is_dark());
        assert_eq!(desktop.fonts.fixed.point_size, Some(12.0));
        assert_eq!(prefers_dark(&tree), Some(false));
        assert_eq!(reduced_motion(&tree), Some(false));
    }

    // ---- The accent when kdeglobals has no colours ----------------------

    fn accent_tree(accent_line: &str) -> Tree {
        Tree::default()
            .file(
                USER_KDEGLOBALS,
                &format!("[General]\nColorScheme=BreezeLight\n{accent_line}\n"),
            )
            .file(LIGHT_FILE, &light_text())
    }

    #[test]
    fn a_valid_accent_colour_is_applied_to_the_scheme_file() {
        let accent = KdeColor::rgb8(233, 61, 0);
        let desktop = discover(&accent_tree("AccentColor=233,61,0"), &kde_env()).unwrap();
        assert_eq!(desktop.scheme_origin, SchemeOrigin::SchemeFile);
        assert_eq!(desktop.user_accent, Some(accent));
        let os = desktop.scheme.os_theme_colors();
        assert_eq!(
            os,
            KdeColorScheme::breeze_light()
                .with_accent(accent)
                .os_theme_colors()
        );
        assert_ne!(os, KdeColorScheme::breeze_light().os_theme_colors());
        // Focus decorations take the accent itself.
        assert_eq!(os.focus, Some(accent.to_color()));

        // The hex form is the same colour.
        let hex = discover(&accent_tree("AccentColor=#e93d00"), &kde_env()).unwrap();
        assert_eq!(hex.scheme.os_theme_colors(), os);
    }

    #[test]
    fn a_valid_accent_colour_is_applied_to_built_in_breeze_light() {
        let accent = KdeColor::rgb8(146, 110, 228);
        let tree = Tree::default().file(USER_KDEGLOBALS, "[General]\nAccentColor=146,110,228\n");
        let desktop = discover(&tree, &kde_env()).unwrap();
        assert_eq!(desktop.scheme_origin, SchemeOrigin::BuiltIn);
        assert_eq!(
            desktop.scheme.os_theme_colors(),
            KdeColorScheme::breeze_light()
                .with_accent(accent)
                .os_theme_colors()
        );
    }

    #[test]
    fn a_transparent_invalid_or_absent_accent_colour_is_no_accent() {
        let plain = KdeColorScheme::breeze_light().os_theme_colors();
        for line in [
            "AccentColor=transparent",
            "AccentColor=0,0,0,0",
            "AccentColor=invalid",
            "AccentColor=",
            "AccentColor=12,34",
            "AccentColor=300,0,0",
            "AccentColor=notacolour",
            "",
        ] {
            let desktop = discover(&accent_tree(line), &kde_env()).unwrap();
            assert_eq!(desktop.user_accent, None, "{line:?}");
            assert_eq!(desktop.scheme.os_theme_colors(), plain, "{line:?}");
        }
    }

    // ---- Accent precedence ---------------------------------------------

    #[test]
    fn kdeglobals_wins_over_the_portal_accent() {
        let portal = || {
            let mut os = OsThemeColors::new(ColorSchemePreference::Dark);
            os.accent = Some(Color::from_rgb(1.0, 0.0, 0.0));
            os
        };

        let desktop = discover(&accent_tree(""), &kde_env());
        let native = native_desktop(desktop.clone(), || {
            panic!("the portal is not asked when kdeglobals can be read")
        });
        assert_eq!(native.kde, desktop);
        assert_eq!(native.os, KdeColorScheme::breeze_light().os_theme_colors());
        assert_eq!(native.os.accent, rgb(61, 174, 233));

        // Without a readable kdeglobals the portal's report is the one.
        let native = native_desktop(None, portal);
        assert_eq!(native.kde, None);
        assert_eq!(native.os, portal());
    }

    // ---- Dark detection --------------------------------------------------

    #[test]
    fn dark_is_decided_by_the_colours_not_the_scheme_name() {
        let light = accent_tree("");
        assert_eq!(prefers_dark(&light), Some(false));

        let dark = Tree::default()
            .file(USER_KDEGLOBALS, "[General]\nColorScheme=BreezeDark\n")
            .file(DARK_FILE, &dark_text());
        assert_eq!(prefers_dark(&dark), Some(true));

        // A light scheme whose name says dark is light.
        let misnamed = Tree::default()
            .file(USER_KDEGLOBALS, "[General]\nColorScheme=MyDarkScheme\n")
            .file(
                "/usr/share/color-schemes/MyDarkScheme.colors",
                &light_text(),
            );
        assert_eq!(prefers_dark(&misnamed), Some(false));

        // Twilight-like: Breeze Light's views under Breeze Dark's window. The
        // window decides, so it is dark.
        let twilight = format!(
            "{}\n[General]\nColorScheme=BreezeTwilight\n\n\
             [Colors:Window]\nBackgroundNormal=32,35,38\nForegroundNormal=252,252,252\n",
            light_text()
        );
        let twilight = Tree::default().file(USER_KDEGLOBALS, &twilight);
        assert_eq!(prefers_dark(&twilight), Some(true));
        let os = discover(&twilight, &kde_env())
            .unwrap()
            .scheme
            .os_theme_colors();
        assert_eq!(os.color_scheme, ColorSchemePreference::Dark);
        assert_eq!(os.view_bg, rgb(255, 255, 255));
    }

    // ---- Reduced motion --------------------------------------------------

    #[test]
    fn reduced_motion_follows_an_animation_factor_of_zero() {
        let with = |factor: &str| {
            Tree::default().file(
                USER_KDEGLOBALS,
                &format!("[KDE]\nAnimationDurationFactor={factor}\n"),
            )
        };
        assert_eq!(reduced_motion(&with("0")), Some(true));
        assert_eq!(reduced_motion(&with("0.0")), Some(true));
        assert_eq!(reduced_motion(&with("1")), Some(false));
        assert_eq!(reduced_motion(&with("0.25")), Some(false));
        let absent = Tree::default().file(USER_KDEGLOBALS, "[KDE]\nwidgetStyle=Breeze\n");
        assert_eq!(reduced_motion(&absent), Some(false));

        // The same answer as the full reader's.
        let var = kde_env();
        let desktop = discover(&with("0"), &var).unwrap();
        assert!(desktop.prefers_reduced_motion());
    }
}
