// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! CSS color parsing for the SVG icon pipeline.
//!
//! Everything an SVG `fill` / `stroke` / `stop-color` value can legally be:
//! hex (`#f00`, `#ff0000`, and the 4/8-digit forms with alpha), the functional
//! notations (`rgb()` / `rgba()` / `hsl()` / `hsla()`, in both the legacy
//! comma-separated and the modern space-separated syntax), the 148 CSS named
//! colors, and `transparent`.
//!
//! Deliberately **fallible** (`Option<Color>`), unlike [`Color::from_hex`],
//! which answers black for anything it can't read. The SVG cascade needs to
//! tell "this element declared a color I understood" apart from "this element
//! declared `none` / `currentColor` / `url(#grad)` / gibberish", because those
//! four mean four different things and only one of them is a color — see
//! [`SvgPaint`](super::SvgPaint), whose parser this backs.

use bastyde_tokens::Color;

/// Parse a CSS color. `None` if the value isn't a color the parser knows —
/// including the keywords (`none`, `currentColor`, `inherit`) and paint-server
/// references (`url(#id)`), which the caller must handle before reaching here.
pub(crate) fn parse_color(value: &str) -> Option<Color> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if let Some(hex) = v.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(rest) = strip_fn(v, "rgba").or_else(|| strip_fn(v, "rgb")) {
        return parse_rgb_fn(rest);
    }
    if let Some(rest) = strip_fn(v, "hsla").or_else(|| strip_fn(v, "hsl")) {
        return parse_hsl_fn(rest);
    }
    if v.eq_ignore_ascii_case("transparent") {
        return Some(Color::TRANSPARENT);
    }
    parse_named(v)
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` — the shorthand forms double each
/// nibble (`#f80` == `#ff8800`), per CSS.
fn parse_hex(hex: &str) -> Option<Color> {
    let h = hex.trim();
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let nib = |i: usize| -> Option<f32> {
        let c = h.as_bytes().get(i)?;
        let d = (*c as char).to_digit(16)? as f32;
        // A single nibble expands to a byte by repetition: 0xF -> 0xFF.
        Some((d * 16.0 + d) / 255.0)
    };
    let byte = |i: usize| -> Option<f32> {
        let s = h.get(i..i + 2)?;
        Some(u8::from_str_radix(s, 16).ok()? as f32 / 255.0)
    };
    match h.len() {
        3 => Some(Color::new(nib(0)?, nib(1)?, nib(2)?, 1.0)),
        4 => Some(Color::new(nib(0)?, nib(1)?, nib(2)?, nib(3)?)),
        6 => Some(Color::new(byte(0)?, byte(2)?, byte(4)?, 1.0)),
        8 => Some(Color::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

/// Strip a `name(...)` wrapper, returning the argument text.
fn strip_fn<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let v = value.trim();
    if v.len() <= name.len() || !v[..name.len()].eq_ignore_ascii_case(name) {
        return None;
    }
    let rest = v[name.len()..].trim_start();
    rest.strip_prefix('(')?.strip_suffix(')')
}

/// Split functional-notation arguments. Handles both the legacy
/// `rgb(255, 0, 0)` and the modern `rgb(255 0 0 / 50%)` syntaxes: commas and
/// whitespace both separate, and a `/` introduces the alpha in either.
fn split_args(args: &str) -> (Vec<&str>, Option<&str>) {
    let (main, alpha) = match args.split_once('/') {
        Some((m, a)) => (m, Some(a.trim())),
        None => (args, None),
    };
    let parts: Vec<&str> = main
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    // Legacy `rgba(r, g, b, a)` puts alpha in the 4th slot instead.
    if alpha.is_none() && parts.len() == 4 {
        return (parts[..3].to_vec(), Some(parts[3]));
    }
    (parts, alpha)
}

/// A channel value: `0..255`, or a percentage of 255.
fn parse_channel(s: &str) -> Option<f32> {
    let t = s.trim();
    let v = match t.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
        None => t.parse::<f32>().ok()? / 255.0,
    };
    Some(v.clamp(0.0, 1.0))
}

/// An alpha value: `0..1`, or a percentage.
pub(crate) fn parse_alpha(s: &str) -> Option<f32> {
    let t = s.trim();
    let v = match t.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
        None => t.parse::<f32>().ok()?,
    };
    Some(v.clamp(0.0, 1.0))
}

fn parse_rgb_fn(args: &str) -> Option<Color> {
    let (parts, alpha) = split_args(args);
    if parts.len() != 3 {
        return None;
    }
    let a = match alpha {
        Some(s) => parse_alpha(s)?,
        None => 1.0,
    };
    Some(Color::new(
        parse_channel(parts[0])?,
        parse_channel(parts[1])?,
        parse_channel(parts[2])?,
        a,
    ))
}

fn parse_hsl_fn(args: &str) -> Option<Color> {
    let (parts, alpha) = split_args(args);
    if parts.len() != 3 {
        return None;
    }
    // Hue is an angle: bare number = degrees; `deg` / `turn` / `rad` allowed.
    let h_raw = parts[0].trim();
    let hue = if let Some(t) = h_raw.strip_suffix("turn") {
        t.trim().parse::<f32>().ok()? * 360.0
    } else if let Some(r) = h_raw.strip_suffix("rad") {
        r.trim().parse::<f32>().ok()?.to_degrees()
    } else {
        h_raw.trim_end_matches("deg").trim().parse::<f32>().ok()?
    };
    let pct = |s: &str| -> Option<f32> {
        Some(
            s.trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f32>()
                .ok()?
                .clamp(0.0, 100.0)
                / 100.0,
        )
    };
    let a = match alpha {
        Some(s) => parse_alpha(s)?,
        None => 1.0,
    };
    let c = Color::from_hsl(hue, pct(parts[1])?, pct(parts[2])?);
    Some(c.with_alpha(a))
}

/// Look up a CSS named color (case-insensitive).
fn parse_named(name: &str) -> Option<Color> {
    let lower = name.to_ascii_lowercase();
    let idx = NAMED_COLORS
        .binary_search_by(|(n, _)| (*n).cmp(lower.as_str()))
        .ok()?;
    let rgb = NAMED_COLORS[idx].1;
    Some(Color::new(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
        1.0,
    ))
}

/// The CSS named colors, **sorted** for `binary_search` (the table is asserted
/// sorted by a test, so an added name in the wrong place fails loudly instead
/// of becoming an unfindable entry).
#[rustfmt::skip]
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xF0F8FF), ("antiquewhite", 0xFAEBD7), ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4), ("azure", 0xF0FFFF), ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4), ("black", 0x000000), ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF), ("blueviolet", 0x8A2BE2), ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887), ("cadetblue", 0x5F9EA0), ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E), ("coral", 0xFF7F50), ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC), ("crimson", 0xDC143C), ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B), ("darkcyan", 0x008B8B), ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9), ("darkgreen", 0x006400), ("darkgrey", 0xA9A9A9),
    ("darkkhaki", 0xBDB76B), ("darkmagenta", 0x8B008B), ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00), ("darkorchid", 0x9932CC), ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A), ("darkseagreen", 0x8FBC8F), ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F), ("darkslategrey", 0x2F4F4F), ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3), ("deeppink", 0xFF1493), ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969), ("dimgrey", 0x696969), ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222), ("floralwhite", 0xFFFAF0), ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF), ("gainsboro", 0xDCDCDC), ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700), ("goldenrod", 0xDAA520), ("gray", 0x808080),
    ("green", 0x008000), ("greenyellow", 0xADFF2F), ("grey", 0x808080),
    ("honeydew", 0xF0FFF0), ("hotpink", 0xFF69B4), ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082), ("ivory", 0xFFFFF0), ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA), ("lavenderblush", 0xFFF0F5), ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD), ("lightblue", 0xADD8E6), ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF), ("lightgoldenrodyellow", 0xFAFAD2), ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90), ("lightgrey", 0xD3D3D3), ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A), ("lightseagreen", 0x20B2AA), ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899), ("lightslategrey", 0x778899), ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0), ("lime", 0x00FF00), ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6), ("magenta", 0xFF00FF), ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA), ("mediumblue", 0x0000CD), ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB), ("mediumseagreen", 0x3CB371), ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A), ("mediumturquoise", 0x48D1CC), ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970), ("mintcream", 0xF5FFFA), ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5), ("navajowhite", 0xFFDEAD), ("navy", 0x000080),
    ("oldlace", 0xFDF5E6), ("olive", 0x808000), ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500), ("orangered", 0xFF4500), ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA), ("palegreen", 0x98FB98), ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093), ("papayawhip", 0xFFEFD5), ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F), ("pink", 0xFFC0CB), ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
    ("red", 0xFF0000), ("rosybrown", 0xBC8F8F), ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513), ("salmon", 0xFA8072), ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57), ("seashell", 0xFFF5EE), ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0), ("skyblue", 0x87CEEB), ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F), ("steelblue", 0x4682B4), ("tan", 0xD2B48C),
    ("teal", 0x008080), ("thistle", 0xD8BFD8), ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0), ("violet", 0xEE82EE), ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF), ("whitesmoke", 0xF5F5F5), ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(c: Color, r: f32, g: f32, b: f32, a: f32) {
        let d = 1.0 / 255.0 + 1e-4;
        assert!(
            (c.r() - r).abs() < d
                && (c.g() - g).abs() < d
                && (c.b() - b).abs() < d
                && (c.a() - a).abs() < d,
            "expected ({r}, {g}, {b}, {a}), got ({}, {}, {}, {})",
            c.r(),
            c.g(),
            c.b(),
            c.a()
        );
    }

    #[test]
    fn hex_forms() {
        approx(parse_color("#f00").unwrap(), 1.0, 0.0, 0.0, 1.0);
        approx(parse_color("#FF0000").unwrap(), 1.0, 0.0, 0.0, 1.0);
        // Shorthand doubles each nibble: #f80 == #ff8800, NOT #f08000.
        approx(
            parse_color("#f80").unwrap(),
            1.0,
            0x88 as f32 / 255.0,
            0.0,
            1.0,
        );
        approx(parse_color("#00ff0080").unwrap(), 0.0, 1.0, 0.0, 0.502);
        approx(parse_color("#0f08").unwrap(), 0.0, 1.0, 0.0, 0.533);
        assert!(parse_color("#12345").is_none());
        assert!(parse_color("#gg0000").is_none());
    }

    #[test]
    fn rgb_and_rgba_functions() {
        approx(parse_color("rgb(255, 0, 0)").unwrap(), 1.0, 0.0, 0.0, 1.0);
        approx(
            parse_color("rgba(0,255,0,0.5)").unwrap(),
            0.0,
            1.0,
            0.0,
            0.5,
        );
        approx(
            parse_color("rgb(100%, 0%, 0%)").unwrap(),
            1.0,
            0.0,
            0.0,
            1.0,
        );
        // Modern space-separated syntax with a slash alpha.
        approx(
            parse_color("rgb(0 0 255 / 25%)").unwrap(),
            0.0,
            0.0,
            1.0,
            0.25,
        );
        assert!(parse_color("rgb(1, 2)").is_none());
    }

    #[test]
    fn hsl_functions() {
        approx(
            parse_color("hsl(0, 100%, 50%)").unwrap(),
            1.0,
            0.0,
            0.0,
            1.0,
        );
        approx(
            parse_color("hsl(120deg 100% 50%)").unwrap(),
            0.0,
            1.0,
            0.0,
            1.0,
        );
        approx(
            parse_color("hsla(240, 100%, 50%, 0.5)").unwrap(),
            0.0,
            0.0,
            1.0,
            0.5,
        );
    }

    #[test]
    fn named_colors_and_transparent() {
        approx(parse_color("red").unwrap(), 1.0, 0.0, 0.0, 1.0);
        approx(parse_color("REBECCAPURPLE").unwrap(), 0.4, 0.2, 0.6, 1.0);
        // Both spellings of grey resolve, and to the same value.
        assert_eq!(parse_color("gray"), parse_color("grey"));
        approx(parse_color("transparent").unwrap(), 0.0, 0.0, 0.0, 0.0);
        assert!(parse_color("chartreuseish").is_none());
    }

    /// The lookup is a binary search, so an out-of-order entry would be
    /// silently unfindable rather than wrong — pin the invariant.
    #[test]
    fn named_table_is_sorted() {
        assert!(
            NAMED_COLORS.windows(2).all(|w| w[0].0 < w[1].0),
            "NAMED_COLORS must stay sorted for binary_search"
        );
    }

    /// The keywords are NOT colors: the cascade has to see through them to
    /// `none` / the widget tint / a paint server.
    #[test]
    fn keywords_are_not_colors() {
        assert!(parse_color("none").is_none());
        assert!(parse_color("currentColor").is_none());
        assert!(parse_color("inherit").is_none());
        assert!(parse_color("url(#grad)").is_none());
        assert!(parse_color("").is_none());
    }
}
