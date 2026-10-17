// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Resolved palettes against the golden vectors
//! `tools/kde_golden/vectors/resolve.json`, measured from KDE's own
//! libraries and Qt 6.11.1.
//!
//! Each scheme of the vectors carries its input (the configuration text of
//! a synthetic scheme, the parsed groups of an installed one), so the
//! scheme is read from that input here and resolved, and every colour is
//! compared at 8 bits: for every state and set, the 8 foregrounds, the 8
//! backgrounds, the 2 decorations and the 5 shades, and for every state
//! the 20 palette roles.
//!
//! One documented divergence is left out of the comparison: where a scheme
//! stores an empty or unknown colour, KDE resolves an invalid colour and
//! Teksilo uses the key's default instead (section b.9 of the
//! specification). The vectors record such a colour as `{"valid": false}`.
//! The test pins the exact list of those stored colours, checks that
//! Teksilo gives their default, and skips only what derives from them: that
//! foreground in each state, the background tinted toward it and the
//! palette role read from it.
//!
//! The vectors live outside the crate, so this file is left out of the
//! published package (the `exclude` list in `Cargo.toml`).

use serde_json::Value;
use teksilo_kde_scheme::{
    BackgroundRole, ColorSet, ColorState, DecorationRole, ForegroundRole, KdeColor, KdeColorScheme,
    PaletteRole, ShadeRole,
};

const RESOLVE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/resolve.json"
));

fn vectors() -> Value {
    serde_json::from_str(RESOLVE_JSON).expect("resolve.json parses")
}

fn schemes(all: &Value) -> &[Value] {
    let list = all["schemes"]
        .as_array()
        .expect("resolve.json has a list of schemes");
    assert!(!list.is_empty(), "resolve.json lists no scheme");
    list
}

fn name(scheme: &Value) -> &str {
    scheme["name"].as_str().expect("a scheme has a name")
}

fn find<'a>(all: &'a Value, wanted: &str) -> &'a Value {
    schemes(all)
        .iter()
        .find(|s| name(s) == wanted)
        .unwrap_or_else(|| panic!("resolve.json has no scheme {wanted}"))
}

/// The configuration text of a scheme's input: the verbatim text of a
/// synthetic scheme, or the groups of an installed one written back as
/// text. A nested group is recorded with its path joined by `][`, so
/// bracketing the recorded name gives the header line.
fn input_text(scheme: &Value) -> String {
    if let Some(text) = scheme["source_text"].as_str() {
        return text.to_owned();
    }
    let groups = scheme["input"].as_object().expect("an input object");
    assert!(!groups.is_empty(), "{} has an empty input", name(scheme));
    let mut out = String::new();
    for (group, entries) in groups {
        out.push('[');
        out.push_str(group);
        out.push_str("]\n");
        for (key, value) in entries.as_object().expect("a group object") {
            out.push_str(key);
            out.push('=');
            out.push_str(value.as_str().expect("a value is a string"));
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

fn read_scheme(scheme: &Value) -> KdeColorScheme {
    KdeColorScheme::parse(&input_text(scheme)).0
}

fn hex(c: KdeColor) -> String {
    let [r, g, b, a] = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

/// A golden `{hex, u16}` colour's 8-bit hex, checked to be the 8-bit
/// projection of its 16-bit channels.
fn golden_hex(value: &Value) -> String {
    let want = value["hex"].as_str().expect("a hex string").to_owned();
    let channels: Vec<u16> = value["u16"]
        .as_array()
        .expect("a u16 list")
        .iter()
        .map(|v| u16::try_from(v.as_u64().expect("an integer")).expect("16 bits"))
        .collect();
    let from_u16 = KdeColor::rgba16(channels[0], channels[1], channels[2], channels[3]);
    assert_eq!(hex(from_u16), want, "golden hex and u16 disagree");
    want
}

fn state_key(state: ColorState) -> &'static str {
    match state {
        ColorState::Active => "Active",
        ColorState::Inactive => "Inactive",
        ColorState::Disabled => "Disabled",
    }
}

fn set_key(set: ColorSet) -> &'static str {
    match set {
        ColorSet::View => "View",
        ColorSet::Window => "Window",
        ColorSet::Button => "Button",
        ColorSet::Selection => "Selection",
        ColorSet::Tooltip => "Tooltip",
        ColorSet::Complementary => "Complementary",
        ColorSet::Header => "Header",
    }
}

fn foreground_key(role: ForegroundRole) -> &'static str {
    match role {
        ForegroundRole::Normal => "Normal",
        ForegroundRole::Inactive => "Inactive",
        ForegroundRole::Active => "Active",
        ForegroundRole::Link => "Link",
        ForegroundRole::Visited => "Visited",
        ForegroundRole::Negative => "Negative",
        ForegroundRole::Neutral => "Neutral",
        ForegroundRole::Positive => "Positive",
    }
}

fn background_key(role: BackgroundRole) -> &'static str {
    match role {
        BackgroundRole::Normal => "Normal",
        BackgroundRole::Alternate => "Alternate",
        BackgroundRole::Active => "Active",
        BackgroundRole::Link => "Link",
        BackgroundRole::Visited => "Visited",
        BackgroundRole::Negative => "Negative",
        BackgroundRole::Neutral => "Neutral",
        BackgroundRole::Positive => "Positive",
    }
}

fn decoration_key(role: DecorationRole) -> &'static str {
    match role {
        DecorationRole::Focus => "Focus",
        DecorationRole::Hover => "Hover",
    }
}

fn shade_key(role: ShadeRole) -> &'static str {
    match role {
        ShadeRole::Light => "Light",
        ShadeRole::Midlight => "Midlight",
        ShadeRole::Mid => "Mid",
        ShadeRole::Dark => "Dark",
        ShadeRole::Shadow => "Shadow",
    }
}

fn palette_key(role: PaletteRole) -> &'static str {
    match role {
        PaletteRole::Window => "Window",
        PaletteRole::WindowText => "WindowText",
        PaletteRole::Base => "Base",
        PaletteRole::Text => "Text",
        PaletteRole::AlternateBase => "AlternateBase",
        PaletteRole::Link => "Link",
        PaletteRole::LinkVisited => "LinkVisited",
        PaletteRole::PlaceholderText => "PlaceholderText",
        PaletteRole::Button => "Button",
        PaletteRole::ButtonText => "ButtonText",
        PaletteRole::Highlight => "Highlight",
        PaletteRole::HighlightedText => "HighlightedText",
        PaletteRole::Accent => "Accent",
        PaletteRole::TooltipBase => "ToolTipBase",
        PaletteRole::TooltipText => "ToolTipText",
        PaletteRole::Light => "Light",
        PaletteRole::Midlight => "Midlight",
        PaletteRole::Mid => "Mid",
        PaletteRole::Dark => "Dark",
        PaletteRole::Shadow => "Shadow",
    }
}

fn check(failures: &mut Vec<String>, what: String, got: KdeColor, expected: &Value) {
    let want = golden_hex(expected);
    if hex(got) != want {
        failures.push(format!("{what}: got {}, expected {want}", hex(got)));
    }
}

fn is_invalid(value: &Value) -> bool {
    value["valid"] == Value::Bool(false)
}

/// The stored foregrounds KDE resolves as an invalid colour, as (set,
/// foreground) pairs: the Active colour of the set is recorded invalid.
/// Any other invalid Active colour (a background or a decoration) fails,
/// since the skipping rule below would not cover what derives from it.
fn divergent_foregrounds(scheme: &Value) -> Vec<(ColorSet, ForegroundRole)> {
    let mut out = Vec::new();
    for set in ColorSet::ALL {
        let active = &scheme["sets"][set_key(set)]["Active"];
        for role in ForegroundRole::ALL {
            if is_invalid(&active["foreground"][foreground_key(role)]) {
                out.push((set, role));
            }
        }
        for role in BackgroundRole::ALL {
            assert!(
                !is_invalid(&active["background"][background_key(role)]),
                "{} {} background {} is invalid",
                name(scheme),
                set_key(set),
                background_key(role)
            );
        }
        for role in DecorationRole::ALL {
            assert!(
                !is_invalid(&active["decoration"][decoration_key(role)]),
                "{} {} decoration {} is invalid",
                name(scheme),
                set_key(set),
                decoration_key(role)
            );
        }
    }
    out
}

/// The set and foreground a palette role reads, for the roles read from a
/// foreground.
fn palette_foreground(role: PaletteRole) -> Option<(ColorSet, ForegroundRole)> {
    match role {
        PaletteRole::WindowText => Some((ColorSet::Window, ForegroundRole::Normal)),
        PaletteRole::Text => Some((ColorSet::View, ForegroundRole::Normal)),
        PaletteRole::Link => Some((ColorSet::View, ForegroundRole::Link)),
        PaletteRole::LinkVisited => Some((ColorSet::View, ForegroundRole::Visited)),
        PaletteRole::PlaceholderText => Some((ColorSet::View, ForegroundRole::Inactive)),
        PaletteRole::ButtonText => Some((ColorSet::Button, ForegroundRole::Normal)),
        PaletteRole::HighlightedText => Some((ColorSet::Selection, ForegroundRole::Normal)),
        PaletteRole::TooltipText => Some((ColorSet::Tooltip, ForegroundRole::Normal)),
        _ => None,
    }
}

#[test]
fn the_invalid_colours_are_exactly_the_known_ones_and_take_their_default() {
    let all = vectors();
    let mut found = Vec::new();
    for scheme in schemes(&all) {
        for (set, role) in divergent_foregrounds(scheme) {
            found.push((name(scheme).to_owned(), set, role));
        }
    }
    assert_eq!(
        found,
        vec![
            // `ForegroundLink=` (an empty value).
            (
                "SyntheticMalformed".to_owned(),
                ColorSet::View,
                ForegroundRole::Link
            ),
            // `ForegroundVisited=rebeccapurple`, not an SVG 1.1 keyword.
            (
                "SyntheticValueForms".to_owned(),
                ColorSet::View,
                ForegroundRole::Visited
            ),
        ]
    );
    let view_default = KdeColorScheme::kcolorscheme_defaults();
    let view_default = view_default.set(ColorSet::View);
    let malformed = read_scheme(find(&all, "SyntheticMalformed"));
    assert_eq!(
        malformed.set(ColorSet::View).foreground_link,
        view_default.foreground_link
    );
    let forms = read_scheme(find(&all, "SyntheticValueForms"));
    assert_eq!(
        forms.set(ColorSet::View).foreground_visited,
        view_default.foreground_visited
    );
}

#[test]
fn every_scheme_resolves_to_the_golden_colours() {
    let all = vectors();
    let mut failures = Vec::new();
    let mut compared = 0_usize;
    for scheme in schemes(&all) {
        let n = name(scheme);
        let palette = read_scheme(scheme).resolve();
        let divergent = divergent_foregrounds(scheme);
        for state in ColorState::ALL {
            let sk = state_key(state);
            for set in ColorSet::ALL {
                let golden = &scheme["sets"][set_key(set)][sk];
                assert!(golden.is_object(), "{n} has no {} {sk} set", set_key(set));
                let got = palette.set(state, set);
                let at = |kind: &str, key: &str| format!("{n} {sk} {} {kind} {key}", set_key(set));
                for role in ForegroundRole::ALL {
                    if divergent.contains(&(set, role)) {
                        continue;
                    }
                    let key = foreground_key(role);
                    check(
                        &mut failures,
                        at("foreground", key),
                        got.foreground(role),
                        &golden["foreground"][key],
                    );
                    compared += 1;
                }
                for role in BackgroundRole::ALL {
                    if role
                        .tinted_toward()
                        .is_some_and(|f| divergent.contains(&(set, f)))
                    {
                        continue;
                    }
                    let key = background_key(role);
                    check(
                        &mut failures,
                        at("background", key),
                        got.background(role),
                        &golden["background"][key],
                    );
                    compared += 1;
                }
                for role in DecorationRole::ALL {
                    let key = decoration_key(role);
                    check(
                        &mut failures,
                        at("decoration", key),
                        got.decoration(role),
                        &golden["decoration"][key],
                    );
                    compared += 1;
                }
                for role in ShadeRole::ALL {
                    let key = shade_key(role);
                    check(
                        &mut failures,
                        at("shade", key),
                        got.shade(role),
                        &golden["shade"][key],
                    );
                    compared += 1;
                }
            }
            let golden = &scheme["palette"][sk];
            for role in PaletteRole::ALL {
                if palette_foreground(role).is_some_and(|pair| divergent.contains(&pair)) {
                    continue;
                }
                let key = palette_key(role);
                check(
                    &mut failures,
                    format!("{n} {sk} palette {key}"),
                    palette.role(state, role),
                    &golden[key],
                );
                compared += 1;
            }
            // The one role the palette does not take from the scheme: KDE
            // leaves it at the toolkit's opaque white.
            assert_eq!(golden_hex(&golden["BrightText"]), "#ffffffff");
        }
    }
    assert!(compared > 0, "nothing was compared");
    assert!(
        failures.is_empty(),
        "{} of {compared} colours differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_golden_set_and_palette_role_is_compared() {
    let all = vectors();
    let scheme = &schemes(&all)[0];
    let set = &scheme["sets"]["Window"]["Active"];
    let count = |v: &Value| v.as_object().expect("an object").len();
    assert_eq!(count(&scheme["sets"]), ColorSet::ALL.len());
    assert_eq!(count(&scheme["sets"]["Window"]), ColorState::ALL.len());
    assert_eq!(count(&set["foreground"]), ForegroundRole::ALL.len());
    assert_eq!(count(&set["background"]), BackgroundRole::ALL.len());
    assert_eq!(count(&set["decoration"]), DecorationRole::ALL.len());
    assert_eq!(count(&set["shade"]), ShadeRole::ALL.len());
    // The twenty roles and the white BrightText.
    assert_eq!(
        count(&scheme["palette"]["Active"]),
        PaletteRole::ALL.len() + 1
    );
}

#[test]
fn the_contrast_value_is_the_recorded_one() {
    let all = vectors();
    for scheme in schemes(&all) {
        let want = scheme["contrastF"].as_f64().expect("contrastF is a number");
        let got = read_scheme(scheme).contrast_value();
        assert_eq!(got, want, "{}", name(scheme));
    }
}

#[test]
fn the_stock_constants_resolve_like_the_vectors() {
    let all = vectors();
    for (wanted, constant) in [
        ("BreezeLight", KdeColorScheme::breeze_light()),
        ("BreezeDark", KdeColorScheme::breeze_dark()),
        ("BreezeClassic", KdeColorScheme::breeze_classic()),
    ] {
        assert_eq!(
            constant.resolve(),
            read_scheme(find(&all, wanted)).resolve(),
            "{wanted}"
        );
    }
    assert_eq!(
        KdeColorScheme::kcolorscheme_defaults().resolve(),
        read_scheme(find(&all, "SyntheticEmpty")).resolve()
    );
}

/// Whether each scheme is dark. Only the Active window background decides.
#[test]
fn is_dark_follows_the_window_background() {
    let all = vectors();
    let expected = [
        ("BreezeLight", false),
        ("BreezeDark", true),
        ("BreezeClassic", false),
        ("Oxygen", false),
        ("OxygenCold", false),
        ("OxygenDark", true),
        ("SyntheticNoHeader", false),
        ("SyntheticInactiveEffects", true),
        ("SyntheticInactiveSubgroups", false),
        ("SyntheticAccentKeys", false),
        ("SyntheticEmpty", false),
        ("SyntheticMalformed", false),
        // Window 200,190,180: grey (2200 + 3040 + 900) / 32 = 191, just
        // below the threshold.
        ("SyntheticValueForms", true),
    ];
    assert_eq!(expected.len(), schemes(&all).len(), "a scheme is unlisted");
    for (wanted, dark) in expected {
        assert_eq!(read_scheme(find(&all, wanted)).is_dark(), dark, "{wanted}");
    }
}

/// The input of `base` with one group replaced by the same group of
/// `donor`.
fn with_group_from(base: &Value, donor: &Value, group: &str) -> KdeColorScheme {
    let mut mixed = base.clone();
    mixed["input"][group] = donor["input"][group].clone();
    read_scheme(&mixed)
}

/// A Twilight-like scheme (Breeze Light with Breeze Dark's window) is dark
/// although its content views are light, and the reverse is light.
#[test]
fn is_dark_ignores_the_content_views() {
    let all = vectors();
    let light = find(&all, "BreezeLight");
    let dark = find(&all, "BreezeDark");

    let twilight = with_group_from(light, dark, "Colors:Window");
    assert_eq!(
        twilight.set(ColorSet::Window).background_normal.to_rgba8(),
        [32, 35, 38, 255]
    );
    assert_eq!(
        twilight.set(ColorSet::View).background_normal.to_rgba8(),
        [255, 255, 255, 255]
    );
    assert!(twilight.is_dark());

    let reverse = with_group_from(dark, light, "Colors:Window");
    assert_eq!(
        reverse.set(ColorSet::View).background_normal.to_rgba8(),
        [20, 22, 24, 255]
    );
    assert!(!reverse.is_dark());

    // SyntheticMalformed's window is a light grey under a saturated orange
    // view.
    let malformed = read_scheme(find(&all, "SyntheticMalformed"));
    assert_eq!(
        malformed.set(ColorSet::View).background_normal.to_rgba8(),
        [255, 136, 0, 255]
    );
    assert!(!malformed.is_dark());
}
