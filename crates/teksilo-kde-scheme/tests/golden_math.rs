// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The colour arithmetic of `teksilo_kde_scheme::math` against the golden
//! vectors `tools/kde_golden/vectors/math.json`, measured from KDE's own
//! libraries and Qt 6.11.1.
//!
//! Every record is checked: real results within 1e-6, stored colours equal
//! in all four 16-bit channels and in the 8-bit hex. The one member left
//! out is `overlay`, which records a painter composition that Teksilo's
//! specification does not describe and no Teksilo code performs.
//!
//! The vectors live outside the crate, so this file is left out of the
//! published package (the `exclude` list in `Cargo.toml`).

use serde_json::Value;
use teksilo_kde_scheme::KdeColor;
use teksilo_kde_scheme::math::{
    self, FloatColor, Hcy, ShadeRole, color_luma_ratio, darken, hsv_darker, hsv_lighter, lighten,
    luma, mix, shade, shade_role, tint,
};

const MATH_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/math.json"
));

fn vectors() -> Value {
    serde_json::from_str(MATH_JSON).expect("math.json parses")
}

fn records<'a>(all: &'a Value, member: &str) -> &'a [Value] {
    let list = all[member]
        .as_array()
        .unwrap_or_else(|| panic!("math.json member {member} is a list"));
    assert!(!list.is_empty(), "math.json member {member} is empty");
    list
}

/// A `#rrggbbaa` golden colour, entering the model from 8 bits.
fn color(value: &Value) -> KdeColor {
    let text = value.as_str().expect("a colour is a string");
    let digits = text.strip_prefix('#').expect("a colour starts with #");
    assert_eq!(digits.len(), 8, "{text} is #rrggbbaa");
    let byte = |i: usize| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).expect("hex digits");
    KdeColor::rgba8(byte(0), byte(1), byte(2), byte(3))
}

fn hex(c: KdeColor) -> String {
    let [r, g, b, a] = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

fn real(value: &Value) -> f64 {
    value.as_f64().expect("a number")
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("an integer")).expect("in range")
}

/// Records a mismatch between a stored colour and a golden `{hex, u16}`.
fn check_color(failures: &mut Vec<String>, what: String, got: KdeColor, expected: &Value) {
    let channels: Vec<u16> = expected["u16"]
        .as_array()
        .expect("a u16 list")
        .iter()
        .map(|v| u16::try_from(v.as_u64().expect("an integer")).expect("16 bits"))
        .collect();
    let want = KdeColor::rgba16(channels[0], channels[1], channels[2], channels[3]);
    let want_hex = expected["hex"].as_str().expect("a hex string");
    if got != want || hex(got) != want_hex {
        failures.push(format!(
            "{what}: got {got:?} {}, expected {want:?} {want_hex}",
            hex(got)
        ));
    }
}

fn check_real(failures: &mut Vec<String>, what: String, got: f64, expected: f64) {
    if (got - expected).abs() > 1e-6 {
        failures.push(format!("{what}: got {got}, expected {expected}"));
    }
}

fn assert_none(member: &str, failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{} of math.json {member} differ; first ones:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn single_colours_have_the_golden_luma_hue_chroma_and_round_trip() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "single") {
        let c = color(&r["in"]);
        let hcy = Hcy::from_color(c);
        let name = r["in"].as_str().unwrap_or_default();
        check_real(
            &mut failures,
            format!("{name} luma"),
            luma(c),
            real(&r["luma"]),
        );
        check_real(
            &mut failures,
            format!("{name} hue"),
            hcy.wrapped_hue(),
            real(&r["hue"]),
        );
        check_real(
            &mut failures,
            format!("{name} chroma"),
            hcy.c,
            real(&r["chroma"]),
        );
        let expected = r["hcy"].as_array().expect("hcy is a list");
        for (i, got) in [hcy.wrapped_hue(), hcy.c, hcy.y, hcy.a]
            .into_iter()
            .enumerate()
        {
            check_real(
                &mut failures,
                format!("{name} hcy[{i}]"),
                got,
                real(&expected[i]),
            );
        }
        check_color(
            &mut failures,
            format!("{name} round trip"),
            hcy.to_color(),
            &r["roundtrip"],
        );
    }
    assert_none("single", failures);
}

#[test]
fn luma_contrast_ratios_match() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "contrast_ratio") {
        let got = color_luma_ratio(color(&r["a"]), color(&r["b"]));
        check_real(&mut failures, format!("{r}"), got, real(&r["out"]));
    }
    assert_none("contrast_ratio", failures);
}

#[test]
fn lighten_darken_and_shade_match() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "lighten") {
        let got = lighten(color(&r["in"]), real(&r["amount"]), real(&r["gain"]));
        check_color(&mut failures, format!("lighten {r}"), got, &r["out"]);
    }
    for r in records(&all, "darken") {
        let got = darken(color(&r["in"]), real(&r["amount"]), real(&r["gain"]));
        check_color(&mut failures, format!("darken {r}"), got, &r["out"]);
    }
    for r in records(&all, "shade") {
        let got = shade(color(&r["in"]), real(&r["luma"]), real(&r["chroma"]));
        check_color(&mut failures, format!("shade {r}"), got, &r["out"]);
    }
    assert_none("lighten, darken and shade", failures);
}

#[test]
fn mix_and_tint_match() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "mix") {
        let got = mix(color(&r["c1"]), color(&r["c2"]), real(&r["bias"]));
        check_color(&mut failures, format!("mix {r}"), got, &r["out"]);
    }
    for r in records(&all, "tint") {
        let got = tint(color(&r["base"]), color(&r["color"]), real(&r["amount"]));
        check_color(&mut failures, format!("tint {r}"), got, &r["out"]);
    }
    assert_none("mix and tint", failures);
}

#[test]
fn hcy_values_convert_to_the_golden_colours() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "hcy_color") {
        let hcy = Hcy {
            h: real(&r["h"]),
            c: real(&r["c"]),
            y: real(&r["y"]),
            a: real(&r["a"]),
        };
        check_color(&mut failures, format!("{r}"), hcy.to_color(), &r["out"]);
    }
    assert_none("hcy_color", failures);
}

#[test]
fn shade_roles_match() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "scheme_shade") {
        let role = match r["role"].as_str() {
            Some("Light") => ShadeRole::Light,
            Some("Midlight") => ShadeRole::Midlight,
            Some("Mid") => ShadeRole::Mid,
            Some("Dark") => ShadeRole::Dark,
            Some("Shadow") => ShadeRole::Shadow,
            other => panic!("unknown shade role {other:?}"),
        };
        let got = shade_role(
            color(&r["in"]),
            role,
            real(&r["contrast"]),
            real(&r["chroma"]),
        );
        check_color(&mut failures, format!("{r}"), got, &r["out"]);
    }
    assert_none("scheme_shade", failures);
}

#[test]
fn hsv_lighter_and_darker_match() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "qt_lighter_darker") {
        let c = color(&r["in"]);
        let factor = int(&r["factor"]);
        let got = match r["op"].as_str() {
            Some("lighter") => hsv_lighter(c, factor),
            Some("darker") => hsv_darker(c, factor),
            other => panic!("unknown operation {other:?}"),
        };
        check_color(&mut failures, format!("{r}"), got, &r["out"]);
    }
    assert_none("qt_lighter_darker", failures);
}

/// Checks a float colour against a golden `{hex, u16}`, which also carries
/// `f` exactly when the colour is held in half precision.
fn check_float(failures: &mut Vec<String>, what: String, got: FloatColor, expected: &Value) {
    match (&expected["f"], got) {
        (Value::Array(f), FloatColor::Half(channels)) => {
            // Each value is a binary16 number, so exactly an `f32`; reading
            // it as `f32` absorbs the last-digit error of the JSON decimal.
            let want: Vec<f32> = f.iter().map(|v| real(v) as f32).collect();
            if want != channels {
                let have = channels;
                failures.push(format!("{what}: half channels {have:?}, expected {want:?}"));
            }
        }
        (Value::Null, FloatColor::Rgb16(_)) => {}
        (_, got) => failures.push(format!("{what}: storage {got:?} disagrees with {expected}")),
    }
    check_color(failures, what, got.to_rgb16(), expected);
}

#[test]
fn srgb_and_linear_conversions_match() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "qt_colorspace") {
        let c = FloatColor::Rgb16(color(&r["in"]));
        check_float(
            &mut failures,
            format!("to linear {r}"),
            math::srgb_to_linear(c),
            &r["to_linear"],
        );
        check_float(
            &mut failures,
            format!("from linear {r}"),
            math::linear_to_srgb(c),
            &r["from_linear"],
        );
    }
    assert_none("qt_colorspace", failures);
}

#[test]
fn out_of_range_float_colours_are_held_in_half_precision() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "qt_extended") {
        let input: Vec<f32> = r["in"]
            .as_array()
            .expect("a channel list")
            .iter()
            .map(|v| real(v) as f32)
            .collect();
        let stored = FloatColor::from_f32([input[0], input[1], input[2]], 1.0);
        check_float(&mut failures, format!("stored {r}"), stored, &r["stored"]);
        check_color(
            &mut failures,
            format!("to rgb {r}"),
            stored.to_rgb16(),
            &r["to_rgb"],
        );
    }
    assert_none("qt_extended", failures);
}

#[test]
fn colour_text_parses_as_qt_reads_it() {
    let all = vectors();
    let mut failures = Vec::new();
    for r in records(&all, "qt_color_text") {
        let text = r["in"].as_str().expect("a string");
        // Section b.9: a configuration value is trimmed before it is read
        // as a colour, so the reader accepts the surrounding whitespace Qt's
        // own parser rejects. Every other text must agree.
        if text != text.trim() {
            continue;
        }
        let got = KdeColor::parse(text);
        match (got, r["out"]["valid"].as_bool()) {
            (None, Some(false)) => {}
            (Some(c), None) => check_color(&mut failures, format!("{text:?}"), c, &r["out"]),
            (got, _) => failures.push(format!("{text:?}: got {got:?}, expected {}", r["out"])),
        }
    }
    assert_none("qt_color_text", failures);
}

const ACCENT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/accent.json"
));

/// The OKLab tint on its own, through the accent oracle's tinting scheme:
/// every colour Plasma tinted (section g.3 step 3: not an accent key, not
/// one the selection or button accent overwrites) must be
/// `oklab_tint(scheme value, accent, TintFactor)`.
#[test]
fn oklab_tint_reproduces_the_tinted_scheme_colours() {
    let all: Value = serde_json::from_str(ACCENT_JSON).expect("accent.json parses");
    let scheme = &all["schemes"]["KdeGoldenTintLight"];
    let factor: f64 = scheme["changes"]["General"]["TintFactor"]
        .as_str()
        .expect("the scheme sets a tint factor")
        .parse()
        .expect("a real tint factor");
    let accent_keys = [
        "ForegroundActive",
        "ForegroundLink",
        "DecorationFocus",
        "DecorationHover",
    ];
    let parse =
        |v: &Value| KdeColor::parse(v.as_str().expect("a colour string")).expect("a colour");
    let mut failures = Vec::new();
    let mut checked = 0;
    let cases = all["cases"].as_array().expect("a case list");
    for case in cases.iter().filter(|c| c["scheme"] == "KdeGoldenTintLight") {
        let accent = parse(&case["accent"]);
        for group in [
            "View",
            "Window",
            "Button",
            "Tooltip",
            "Complementary",
            "Header",
        ] {
            let name = format!("Colors:{group}");
            let Some(input) = scheme["input"][&name].as_object() else {
                continue;
            };
            for (key, value) in input {
                if accent_keys.contains(&key.as_str())
                    || (group == "Button" && key == "BackgroundAlternate")
                {
                    continue;
                }
                let written = &case["kdeglobals"][&name][key];
                let got = math::oklab_tint(parse(value), accent, factor);
                checked += 1;
                if got.to_rgba8() != parse(written).to_rgba8() {
                    failures.push(format!(
                        "accent {} [{name}] {key}: {value} tinted to {got}, Plasma wrote {written}",
                        case["accent"]
                    ));
                }
            }
        }
    }
    assert!(checked > 0, "no tinted colour was checked");
    assert_none("accent.json tinted colours", failures);
}
