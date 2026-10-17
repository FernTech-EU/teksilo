// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The cascade, the file locations and `[$e]` expansion. The environment
//! is always injected as a closure; nothing here reads or changes the
//! process environment.

use std::collections::HashMap;
use std::path::PathBuf;

use teksilo_kde_scheme::{
    ConfigLayerPath, KConfig, KdeConfigLocations, KdeDesktop, KdeDesktopLayers, LayerKind,
    SchemeOrigin, is_kde_session,
};

/// An environment getter over a fixed table.
fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let table: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |name| table.get(name).cloned()
}

fn layer(text: &str) -> KConfig {
    let (config, diagnostics) = KConfig::parse(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    config
}

fn user(text: &str) -> KConfig {
    layer(text).with_kind(LayerKind::User)
}

fn paths(layers: &[ConfigLayerPath]) -> Vec<(String, LayerKind)> {
    layers
        .iter()
        .map(|l| (l.path.to_string_lossy().replace('\\', "/"), l.kind))
        .collect()
}

// ---- Precedence -------------------------------------------------------

#[test]
fn a_higher_layer_wins_over_a_lower_one() {
    let merged = KConfig::cascade(&[layer("[G]\nA=low\nB=low\n"), user("[G]\nA=high\n")]);
    assert_eq!(merged.get(&["G"], "A"), Some("high"));
    assert_eq!(merged.get(&["G"], "B"), Some("low"));
}

#[test]
fn a_deleted_entry_hides_the_lower_value_until_a_higher_layer_sets_it_again() {
    let hidden = KConfig::cascade(&[layer("[G]\nA=low\n"), layer("[G]\nA[$d]\n")]);
    assert_eq!(hidden.get(&["G"], "A"), None);
    let back = KConfig::cascade(&[
        layer("[G]\nA=low\n"),
        layer("[G]\nA[$d]\n"),
        user("[G]\nA=again\n"),
    ]);
    assert_eq!(back.get(&["G"], "A"), Some("again"));
}

#[test]
fn an_immutable_entry_blocks_every_higher_layer() {
    let merged = KConfig::cascade(&[
        layer("[KDE]\nwidgetStyle[$i]=Breeze\n"),
        layer("[KDE]\nwidgetStyle=Fusion\n"),
        user("[KDE]\nwidgetStyle=Oxygen\n"),
    ]);
    assert_eq!(merged.get(&["KDE"], "widgetStyle"), Some("Breeze"));
}

#[test]
fn an_immutable_group_ignores_every_entry_of_the_user_layer() {
    let merged = KConfig::cascade(&[
        layer("[General][$i]\nColorScheme=BreezeLight\n"),
        user("[General]\nColorScheme=BreezeDark\nNew=1\n"),
    ]);
    assert_eq!(merged.get(&["General"], "ColorScheme"), Some("BreezeLight"));
    assert_eq!(merged.get(&["General"], "New"), None);
}

#[test]
fn an_immutable_group_takes_no_new_keys_from_a_higher_default_layer() {
    let merged = KConfig::cascade(&[
        layer("[General][$i]\nColorScheme=BreezeLight\n"),
        layer("[General]\nNew=1\n"),
    ]);
    assert_eq!(merged.get(&["General"], "New"), None);
    assert_eq!(merged.get(&["General"], "ColorScheme"), Some("BreezeLight"));
}

#[test]
fn a_lower_immutable_group_does_not_lock_its_nested_groups() {
    let merged = KConfig::cascade(&[
        layer("[Colors:Header][$i]\nA=1\n[Colors:Header][Inactive]\nB=1\n"),
        user("[Colors:Header][Inactive]\nB=2\n"),
    ]);
    assert_eq!(merged.get(&["Colors:Header", "Inactive"], "B"), Some("2"));
}

#[test]
fn a_file_immutable_layer_stops_every_higher_layer() {
    let merged = KConfig::cascade(&[
        layer("[General]\nA=1\n[$i]\n[KDE]\nB=1\n"),
        layer("[General]\nA=2\nC=2\n"),
        user("[Other]\nD=3\n"),
    ]);
    assert_eq!(merged.get(&["General"], "A"), Some("1"));
    assert_eq!(merged.get(&["General"], "C"), None);
    assert_eq!(merged.get(&["Other"], "D"), None);
    assert!(merged.is_file_immutable());
    // The merge stays a lower layer that blocks anything cascaded on it.
    let again = KConfig::cascade(&[merged, user("[General]\nC=3\n")]);
    assert_eq!(again.get(&["General"], "C"), None);
}

#[test]
fn a_file_can_add_to_its_own_immutable_group_later() {
    let merged = KConfig::cascade(&[layer("[G][$i]\nA=1\n[Other]\nX=1\n[G]\nB=2\n")]);
    assert_eq!(merged.get(&["G"], "B"), Some("2"));
    assert!(merged.is_group_immutable(&["G"]));
}

// ---- This machine's layout -------------------------------------------

/// A Plasma session where `ColorScheme` lives only in
/// `~/.config/kdedefaults/kdeglobals`, `/etc/xdg/kdeglobals` names the
/// look-and-feel package, and the user's own `kdeglobals` holds the colour
/// groups but no `ColorScheme`.
#[test]
fn the_kdedefaults_layer_supplies_the_scheme_name_under_the_user_file() {
    let locations = KdeConfigLocations::from_env(env(&[
        ("HOME", "/home/ada"),
        ("XDG_CONFIG_DIRS", "/home/ada/.config/kdedefaults:/etc/xdg"),
        ("XDG_CURRENT_DESKTOP", "KDE"),
        ("KDE_FULL_SESSION", "true"),
    ]));
    assert_eq!(
        paths(&locations.kdeglobals_cascade()),
        vec![
            ("/etc/kde5rc".to_owned(), LayerKind::Default),
            ("/etc/xdg/system.kdeglobals".to_owned(), LayerKind::Default),
            (
                "/home/ada/.config/kdedefaults/system.kdeglobals".to_owned(),
                LayerKind::Default
            ),
            ("/etc/xdg/kdeglobals".to_owned(), LayerKind::Default),
            (
                "/home/ada/.config/kdedefaults/kdeglobals".to_owned(),
                LayerKind::Default
            ),
            (
                "/home/ada/.config/system.kdeglobals".to_owned(),
                LayerKind::Default
            ),
            ("/home/ada/.config/kdeglobals".to_owned(), LayerKind::User),
        ]
    );

    // Only three of those files exist.
    let files: HashMap<&str, &str> = [
        (
            "/etc/xdg/kdeglobals",
            "[General]\nXftHintStyle=hintslight\n\n[KDE]\nLookAndFeelPackage=org.example.desktop\nSingleClick=false\n",
        ),
        (
            "/home/ada/.config/kdedefaults/kdeglobals",
            "[General]\nColorScheme=BreezeLight\n\n[Icons]\nTheme=breeze\n\n[KDE]\nwidgetStyle=Breeze\n",
        ),
        (
            "/home/ada/.config/kdeglobals",
            "[Colors:View]\nBackgroundNormal=255,255,255\n\n[KDE]\nLookAndFeelPackage=org.kde.breeze.desktop\ncontrast=4\nframeContrast=0.2\n",
        ),
    ]
    .into_iter()
    .collect();
    let layers: Vec<KConfig> = locations
        .kdeglobals_cascade()
        .into_iter()
        .filter_map(|l| {
            let key = l.path.to_string_lossy().replace('\\', "/");
            files
                .get(key.as_str())
                .map(|text| layer(text).with_kind(l.kind))
        })
        .collect();
    assert_eq!(layers.len(), 3);

    let merged = KConfig::cascade(&layers);
    assert_eq!(merged.get(&["General"], "ColorScheme"), Some("BreezeLight"));
    assert_eq!(
        merged.get(&["KDE"], "LookAndFeelPackage"),
        Some("org.kde.breeze.desktop")
    );
    assert_eq!(merged.get(&["KDE"], "widgetStyle"), Some("Breeze"));
    assert_eq!(merged.get(&["General"], "XftHintStyle"), Some("hintslight"));
    assert!(merged.group_exists(&["Colors:View"]));
}

#[test]
fn a_kde_session_prepends_kdedefaults_when_the_environment_lacks_it() {
    let locations = KdeConfigLocations::from_env(env(&[
        ("HOME", "/home/ada"),
        ("XDG_CURRENT_DESKTOP", "KDE"),
    ]));
    assert_eq!(
        locations.config_dirs,
        vec![
            PathBuf::from("/home/ada/.config/kdedefaults"),
            PathBuf::from("/etc/xdg")
        ]
    );
}

#[test]
fn outside_a_kde_session_kdedefaults_is_not_added() {
    let locations = KdeConfigLocations::from_env(env(&[
        ("HOME", "/home/ada"),
        ("XDG_CURRENT_DESKTOP", "GNOME"),
    ]));
    assert_eq!(locations.config_dirs, vec![PathBuf::from("/etc/xdg")]);
}

#[test]
fn kdedefaults_is_not_listed_twice() {
    let locations = KdeConfigLocations::from_env(env(&[
        ("HOME", "/home/ada"),
        ("XDG_CONFIG_DIRS", "/etc/xdg:/home/ada/.config/kdedefaults/"),
        ("KDE_FULL_SESSION", "true"),
    ]));
    assert_eq!(locations.config_dirs.len(), 2);
    assert_eq!(locations.config_dirs[0], PathBuf::from("/etc/xdg"));
}

// ---- Locations -------------------------------------------------------

#[test]
fn xdg_defaults_apply_when_the_variables_are_unset_empty_or_relative() {
    let locations = KdeConfigLocations::from_env(env(&[
        ("HOME", "/home/ada"),
        ("XDG_CONFIG_HOME", ""),
        ("XDG_DATA_HOME", "relative/data"),
        ("XDG_CONFIG_DIRS", "relative:"),
    ]));
    assert_eq!(
        locations.config_home,
        Some(PathBuf::from("/home/ada/.config"))
    );
    assert_eq!(
        locations.data_home,
        Some(PathBuf::from("/home/ada/.local/share"))
    );
    assert_eq!(
        locations.cache_home,
        Some(PathBuf::from("/home/ada/.cache"))
    );
    assert_eq!(locations.config_dirs, vec![PathBuf::from("/etc/xdg")]);
    assert_eq!(
        locations.data_dirs,
        vec![
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share")
        ]
    );
    assert!(!locations.kde_session);
}

#[test]
fn explicit_xdg_variables_are_used_in_the_order_listed() {
    let locations = KdeConfigLocations::from_env(env(&[
        ("XDG_CONFIG_HOME", "/cfg"),
        ("XDG_CONFIG_DIRS", "/a::relative:/b"),
        ("XDG_DATA_HOME", "/data"),
        ("XDG_DATA_DIRS", "/d1:/d2"),
    ]));
    assert_eq!(locations.home, None);
    assert_eq!(
        locations.config_dirs,
        vec![PathBuf::from("/a"), PathBuf::from("/b")]
    );
    assert_eq!(
        paths(&locations.breezerc_cascade()),
        vec![
            ("/b/breezerc".to_owned(), LayerKind::Default),
            ("/a/breezerc".to_owned(), LayerKind::Default),
            ("/cfg/breezerc".to_owned(), LayerKind::User),
        ]
    );
    let candidates: Vec<String> = locations
        .color_scheme_file("BreezeDark")
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        candidates,
        vec![
            "/data/color-schemes/BreezeDark.colors",
            "/d1/color-schemes/BreezeDark.colors",
            "/d2/color-schemes/BreezeDark.colors",
        ]
    );
}

#[test]
fn kde_skip_kderc_leaves_out_etc_kde5rc() {
    let locations = KdeConfigLocations::from_env(env(&[("HOME", "/h"), ("KDE_SKIP_KDERC", "1")]));
    assert!(
        !paths(&locations.kdeglobals_cascade())
            .iter()
            .any(|(p, _)| p == "/etc/kde5rc")
    );
}

#[test]
fn without_a_home_there_is_no_user_layer() {
    let locations = KdeConfigLocations::from_env(env(&[]));
    assert!(
        locations
            .kdeglobals_cascade()
            .iter()
            .all(|l| l.kind == LayerKind::Default)
    );
    assert_eq!(locations.data_file_candidates("x").len(), 2);
}

#[test]
fn a_kde_color_scheme_path_variable_is_not_read() {
    let with = KdeConfigLocations::from_env(env(&[
        ("HOME", "/home/ada"),
        ("KDE_COLOR_SCHEME_PATH", "/tmp/Evil.colors"),
    ]));
    let without = KdeConfigLocations::from_env(env(&[("HOME", "/home/ada")]));
    assert_eq!(with, without);
    assert!(
        with.color_scheme_file("BreezeLight")
            .iter()
            .all(|p| !p.to_string_lossy().contains("Evil"))
    );
}

#[test]
fn kde_session_detection_reads_the_desktop_list_and_the_full_session_flag() {
    assert!(is_kde_session(env(&[("XDG_CURRENT_DESKTOP", "KDE")])));
    assert!(is_kde_session(env(&[(
        "XDG_CURRENT_DESKTOP",
        "ubuntu:KDE"
    )])));
    assert!(is_kde_session(env(&[("KDE_FULL_SESSION", "true")])));
    assert!(!is_kde_session(env(&[("XDG_CURRENT_DESKTOP", "KDEx")])));
    assert!(!is_kde_session(env(&[("XDG_CURRENT_DESKTOP", "kde")])));
    assert!(!is_kde_session(env(&[("KDE_FULL_SESSION", "1")])));
    assert!(!is_kde_session(env(&[])));
}

#[test]
fn the_scheme_fallback_is_needed_only_without_a_view_group() {
    let locations =
        KdeConfigLocations::from_env(env(&[("HOME", "/h"), ("XDG_DATA_DIRS", "/usr/share")]));
    let var = env(&[]);
    let expansion = locations.expansion_env(&var);

    let with_colors = layer("[Colors:View]\nBackgroundNormal=1,2,3\n");
    assert_eq!(
        locations.scheme_fallback_candidates(&with_colors, &expansion),
        None
    );

    let named =
        layer("[General]\nColorScheme=BreezeDark\n[KDE]\nLookAndFeelPackage=org.example.desktop\n");
    let candidates: Vec<String> = locations
        .scheme_fallback_candidates(&named, &expansion)
        .unwrap()
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        candidates,
        vec![
            "/h/.local/share/plasma/look-and-feel/org.example.desktop/contents/colors",
            "/usr/share/plasma/look-and-feel/org.example.desktop/contents/colors",
            "/h/.local/share/color-schemes/BreezeDark.colors",
            "/usr/share/color-schemes/BreezeDark.colors",
            "/h/.local/share/color-schemes/BreezeLight.colors",
            "/usr/share/color-schemes/BreezeLight.colors",
        ]
    );

    let defaults: Vec<String> = locations
        .scheme_fallback_candidates(&KConfig::default(), &expansion)
        .unwrap()
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        defaults[0],
        "/h/.local/share/plasma/look-and-feel/org.kde.breeze.desktop/contents/colors"
    );
    assert_eq!(
        defaults[2],
        "/h/.local/share/color-schemes/BreezeLight.colors"
    );
}

// ---- [$e] expansion ----------------------------------------------------

#[test]
fn dollar_e_entries_expand_only_through_the_injected_environment() {
    let locations = KdeConfigLocations::from_env(env(&[("HOME", "/home/ada")]));
    let var = env(&[("HOME", "/home/ada"), ("FONT", "Hack"), ("EMPTY", "")]);
    let expansion = locations.expansion_env(&var);
    let config = layer(
        "[G]\nPlain=$HOME/x\nHome[$e]=$HOME/x\nBraced[$e]=${FONT},10\nDollars[$e]=a$$b\nUnset[$e]=[$NOPE]\n\
         Empty[$e]=[$EMPTY]\nEmptyBraces[$e]=a${}b\nUnclosed[$e]=a${FONT\nTrailing[$e]=cost 5$\n\
         Qt[$e]=$QT_DATA_HOME/x\nUnicode[$e]=$FONTé\n",
    );
    let g = &["G"];
    let read = |key| config.read_string(g, key, &expansion);
    assert_eq!(read("Plain").as_deref(), Some("$HOME/x"));
    assert_eq!(read("Home").as_deref(), Some("/home/ada/x"));
    assert_eq!(read("Braced").as_deref(), Some("Hack,10"));
    assert_eq!(read("Dollars").as_deref(), Some("a$b"));
    assert_eq!(read("Unset").as_deref(), Some("[]"));
    assert_eq!(read("Empty").as_deref(), Some("[]"));
    assert_eq!(read("EmptyBraces").as_deref(), Some("ab"));
    assert_eq!(read("Unclosed").as_deref(), Some("aHack"));
    assert_eq!(read("Trailing").as_deref(), Some("cost 5$"));
    assert_eq!(read("Qt").as_deref(), Some("/home/ada/.local/share/x"));
    // Letters in the Unicode sense continue the name.
    assert_eq!(read("Unicode").as_deref(), Some(""));
    // The raw value is never expanded.
    assert_eq!(config.get(g, "Home"), Some("$HOME/x"));
}

#[test]
fn a_command_substitution_is_never_executed() {
    let locations = KdeConfigLocations::from_env(env(&[]));
    let asked = std::cell::RefCell::new(Vec::new());
    let var = |name: &str| {
        asked.borrow_mut().push(name.to_owned());
        None
    };
    let expansion = locations.expansion_env(&var);
    let config = layer("[G]\nCmd[$e]=$(touch /tmp/pwned)x\nBack[$e]=`id`\n");
    assert_eq!(
        config.read_string(&["G"], "Cmd", &expansion).as_deref(),
        Some("(touch /tmp/pwned)x")
    );
    assert_eq!(
        config.read_string(&["G"], "Back", &expansion).as_deref(),
        Some("`id`")
    );
    // Only variable names are looked up, and `(` starts none.
    assert!(asked.borrow().is_empty(), "{:?}", asked.borrow());
}

// ---- A desktop built from these layers --------------------------------

#[test]
fn a_desktop_without_colour_groups_reads_the_scheme_file_over_kdeglobals() {
    let var = env(&[]);
    let locations = KdeConfigLocations::from_env(env(&[]));
    let expansion = locations.expansion_env(&var);
    let kdeglobals = [user(
        "[KDE]\nframeContrast=0.5\n[General]\nAccentColor=233,61,61\n",
    )];
    let scheme_file = layer("[Colors:Window]\nBackgroundNormal=10,20,30\n[KDE]\ncontrast=4\n");
    let desktop = KdeDesktop::from_layers(
        &KdeDesktopLayers {
            kdeglobals: &kdeglobals,
            breezerc: &[],
            scheme_file: Some(&scheme_file),
        },
        &expansion,
    );
    assert_eq!(desktop.scheme_origin, SchemeOrigin::SchemeFile);
    assert_eq!(desktop.scheme.contrast, 4);
    // frameContrast is not in the scheme file, so kdeglobals supplies it.
    assert_eq!(desktop.scheme.frame_contrast, 0.5);
    assert_eq!(
        desktop.scheme.sets[1].background_normal,
        teksilo_kde_scheme::KdeColor::rgb8(10, 20, 30)
    );
    assert_eq!(
        desktop.user_accent,
        Some(teksilo_kde_scheme::KdeColor::rgb8(233, 61, 61))
    );

    let builtin = KdeDesktop::from_layers(
        &KdeDesktopLayers {
            kdeglobals: &kdeglobals,
            breezerc: &[],
            scheme_file: None,
        },
        &expansion,
    );
    assert_eq!(builtin.scheme_origin, SchemeOrigin::BuiltIn);
    assert_eq!(
        builtin.scheme.sets,
        teksilo_kde_scheme::KdeColorScheme::breeze_light().sets
    );
    assert_eq!(builtin.scheme.frame_contrast, 0.5);
}
