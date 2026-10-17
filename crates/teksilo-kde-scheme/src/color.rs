// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Colour values as KDE configuration files store them.
//!
//! A colour is held as four 16-bit channels, the precision KDE's own colour
//! arithmetic works in, so that a value read from a file and written back
//! is the same bytes. [`KdeColor::to_color`] converts to Teksilo's
//! [`Color`] at the crate boundary.
//!
//! The accepted text forms (all trimmed and escape-decoded by the
//! configuration parser first):
//!
//! | Text | Result |
//! |---|---|
//! | `r,g,b` or `r,g,b,a` | 8-bit channels, each a base-10 integer in 0..=255, optional sign and surrounding ASCII whitespace |
//! | `#rgb`, `#rrggbb`, `#aarrggbb` | 8-bit channels (a single digit `d` is `17 d`) |
//! | `#rrrgggbbb` | 12-bit channels |
//! | `#rrrrggggbbbb` | 16-bit channels, as written |
//! | an SVG 1.1 colour keyword, or `transparent` | that colour, ignoring ASCII case |
//!
//! Anything else (an empty value, `invalid`, a wrong component count, an
//! out-of-range component, another digit count after `#`, an unknown name)
//! is treated as absent, so the reader falls back to its default. KDE keeps
//! an empty or `invalid` value as an "invalid colour" that paints as black;
//! Teksilo treats it as absent instead, which is the one deliberate
//! difference.

use std::fmt;

use teksilo_tokens::Color;

/// A colour with four 16-bit channels, red, green, blue and alpha, each in
/// `0..=65535`.
///
/// An 8-bit channel `v` is stored as `257 v`, so every 8-bit colour is
/// represented exactly and [`KdeColor::to_rgba8`] gives it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KdeColor {
    /// Red, 0..=65535.
    pub r: u16,
    /// Green, 0..=65535.
    pub g: u16,
    /// Blue, 0..=65535.
    pub b: u16,
    /// Alpha, 0..=65535 (65535 is opaque).
    pub a: u16,
}

impl KdeColor {
    /// Opaque black.
    pub const BLACK: KdeColor = KdeColor::rgb8(0, 0, 0);
    /// Opaque white.
    pub const WHITE: KdeColor = KdeColor::rgb8(255, 255, 255);
    /// Fully transparent black, the `transparent` keyword.
    pub const TRANSPARENT: KdeColor = KdeColor::rgba8(0, 0, 0, 0);

    /// An opaque colour from 8-bit channels.
    pub const fn rgb8(r: u8, g: u8, b: u8) -> KdeColor {
        KdeColor::rgba8(r, g, b, 255)
    }

    /// A colour from 8-bit channels, alpha included.
    pub const fn rgba8(r: u8, g: u8, b: u8, a: u8) -> KdeColor {
        KdeColor {
            r: widen8(r),
            g: widen8(g),
            b: widen8(b),
            a: widen8(a),
        }
    }

    /// A colour from 16-bit channels.
    pub const fn rgba16(r: u16, g: u16, b: u16, a: u16) -> KdeColor {
        KdeColor { r, g, b, a }
    }

    /// The four channels reduced to 8 bits, red, green, blue, alpha.
    ///
    /// Each channel `q` becomes `floor((q + 128) / 257)`, which is
    /// `round(q / 257)` and never ties.
    pub const fn to_rgba8(self) -> [u8; 4] {
        [
            narrow16(self.r),
            narrow16(self.g),
            narrow16(self.b),
            narrow16(self.a),
        ]
    }

    /// Whether every 8-bit channel, alpha included, is zero: the
    /// `transparent` keyword, which KDE treats as "no colour" where a key
    /// may name one.
    pub const fn is_transparent_black(self) -> bool {
        let [r, g, b, a] = self.to_rgba8();
        r == 0 && g == 0 && b == 0 && a == 0
    }

    /// Converts to Teksilo's [`Color`], each channel `q` becoming
    /// `q / 65535`.
    pub fn to_color(self) -> Color {
        Color::new(
            f32::from(self.r) / 65535.0,
            f32::from(self.g) / 65535.0,
            f32::from(self.b) / 65535.0,
            f32::from(self.a) / 65535.0,
        )
    }

    /// Converts from Teksilo's [`Color`], each channel `c` becoming
    /// `round(65535 c)` after clamping to `0..=1`. A NaN channel becomes 0.
    pub fn from_color(color: Color) -> KdeColor {
        KdeColor {
            r: quantise(color.r()),
            g: quantise(color.g()),
            b: quantise(color.b()),
            a: quantise(color.a()),
        }
    }

    /// Parses a colour value as stored in a KDE configuration file.
    ///
    /// Returns `None` for every value the module documentation lists as
    /// absent.
    pub fn parse(text: &str) -> Option<KdeColor> {
        let text = text.trim_matches(is_ascii_space);
        if text.is_empty() || text == "invalid" {
            return None;
        }
        if let Some(digits) = text.strip_prefix('#') {
            return parse_hex(digits);
        }
        if text.contains(',') {
            return parse_components(text);
        }
        named_color(text)
    }

    /// The text KDE writes for this colour: `r,g,b` when the 8-bit alpha
    /// is 255, `r,g,b,a` otherwise, each component the 8-bit channel in
    /// decimal.
    pub fn to_kconfig_text(self) -> String {
        let [r, g, b, a] = self.to_rgba8();
        if a == 255 {
            format!("{r},{g},{b}")
        } else {
            format!("{r},{g},{b},{a}")
        }
    }
}

impl From<KdeColor> for Color {
    fn from(color: KdeColor) -> Color {
        color.to_color()
    }
}

impl From<Color> for KdeColor {
    /// The same as [`KdeColor::from_color`].
    fn from(color: Color) -> KdeColor {
        KdeColor::from_color(color)
    }
}

impl fmt::Display for KdeColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_kconfig_text())
    }
}

const fn widen8(v: u8) -> u16 {
    // 257 * 255 = 65535, so the product never overflows.
    (v as u16) * 257
}

const fn narrow16(q: u16) -> u8 {
    // (65535 + 128) / 257 = 255, so the quotient always fits.
    ((q as u32 + 128) / 257) as u8
}

fn quantise(c: f32) -> u16 {
    if c.is_nan() {
        return 0;
    }
    (c.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn is_ascii_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

fn parse_hex(digits: &str) -> Option<KdeColor> {
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let field =
        |start: usize, len: usize| u16::from_str_radix(&digits[start..start + len], 16).ok();
    match digits.len() {
        3 => {
            let r = field(0, 1)? * 17;
            let g = field(1, 1)? * 17;
            let b = field(2, 1)? * 17;
            Some(KdeColor::rgba16(r * 257, g * 257, b * 257, u16::MAX))
        }
        6 => Some(KdeColor::rgba16(
            field(0, 2)? * 257,
            field(2, 2)? * 257,
            field(4, 2)? * 257,
            u16::MAX,
        )),
        8 => Some(KdeColor::rgba16(
            field(2, 2)? * 257,
            field(4, 2)? * 257,
            field(6, 2)? * 257,
            field(0, 2)? * 257,
        )),
        9 => {
            let wide = |v: u16| 16 * v + v / 256;
            Some(KdeColor::rgba16(
                wide(field(0, 3)?),
                wide(field(3, 3)?),
                wide(field(6, 3)?),
                u16::MAX,
            ))
        }
        12 => Some(KdeColor::rgba16(
            field(0, 4)?,
            field(4, 4)?,
            field(8, 4)?,
            u16::MAX,
        )),
        _ => None,
    }
}

fn parse_components(text: &str) -> Option<KdeColor> {
    let mut channels = [255_u8; 4];
    let mut count = 0;
    for part in text.split(',') {
        if count == 4 {
            return None;
        }
        let part = part.trim_matches(is_ascii_space);
        let value: i64 = part.parse().ok()?;
        channels[count] = u8::try_from(value).ok()?;
        count += 1;
    }
    if count < 3 {
        return None;
    }
    let [r, g, b, a] = channels;
    Some(KdeColor::rgba8(r, g, b, a))
}

fn named_color(text: &str) -> Option<KdeColor> {
    let lower = text.to_ascii_lowercase();
    if lower == "transparent" {
        return Some(KdeColor::TRANSPARENT);
    }
    SVG_COLOR_KEYWORDS
        .binary_search_by(|(name, _)| name.cmp(&lower.as_str()))
        .ok()
        .map(|i| {
            let [r, g, b] = SVG_COLOR_KEYWORDS[i].1;
            KdeColor::rgb8(r, g, b)
        })
}

/// The SVG 1.1 colour keywords, sorted by name for binary search.
pub(crate) const SVG_COLOR_KEYWORDS: [(&str, [u8; 3]); 147] = [
    ("aliceblue", [240, 248, 255]),
    ("antiquewhite", [250, 235, 215]),
    ("aqua", [0, 255, 255]),
    ("aquamarine", [127, 255, 212]),
    ("azure", [240, 255, 255]),
    ("beige", [245, 245, 220]),
    ("bisque", [255, 228, 196]),
    ("black", [0, 0, 0]),
    ("blanchedalmond", [255, 235, 205]),
    ("blue", [0, 0, 255]),
    ("blueviolet", [138, 43, 226]),
    ("brown", [165, 42, 42]),
    ("burlywood", [222, 184, 135]),
    ("cadetblue", [95, 158, 160]),
    ("chartreuse", [127, 255, 0]),
    ("chocolate", [210, 105, 30]),
    ("coral", [255, 127, 80]),
    ("cornflowerblue", [100, 149, 237]),
    ("cornsilk", [255, 248, 220]),
    ("crimson", [220, 20, 60]),
    ("cyan", [0, 255, 255]),
    ("darkblue", [0, 0, 139]),
    ("darkcyan", [0, 139, 139]),
    ("darkgoldenrod", [184, 134, 11]),
    ("darkgray", [169, 169, 169]),
    ("darkgreen", [0, 100, 0]),
    ("darkgrey", [169, 169, 169]),
    ("darkkhaki", [189, 183, 107]),
    ("darkmagenta", [139, 0, 139]),
    ("darkolivegreen", [85, 107, 47]),
    ("darkorange", [255, 140, 0]),
    ("darkorchid", [153, 50, 204]),
    ("darkred", [139, 0, 0]),
    ("darksalmon", [233, 150, 122]),
    ("darkseagreen", [143, 188, 143]),
    ("darkslateblue", [72, 61, 139]),
    ("darkslategray", [47, 79, 79]),
    ("darkslategrey", [47, 79, 79]),
    ("darkturquoise", [0, 206, 209]),
    ("darkviolet", [148, 0, 211]),
    ("deeppink", [255, 20, 147]),
    ("deepskyblue", [0, 191, 255]),
    ("dimgray", [105, 105, 105]),
    ("dimgrey", [105, 105, 105]),
    ("dodgerblue", [30, 144, 255]),
    ("firebrick", [178, 34, 34]),
    ("floralwhite", [255, 250, 240]),
    ("forestgreen", [34, 139, 34]),
    ("fuchsia", [255, 0, 255]),
    ("gainsboro", [220, 220, 220]),
    ("ghostwhite", [248, 248, 255]),
    ("gold", [255, 215, 0]),
    ("goldenrod", [218, 165, 32]),
    ("gray", [128, 128, 128]),
    ("green", [0, 128, 0]),
    ("greenyellow", [173, 255, 47]),
    ("grey", [128, 128, 128]),
    ("honeydew", [240, 255, 240]),
    ("hotpink", [255, 105, 180]),
    ("indianred", [205, 92, 92]),
    ("indigo", [75, 0, 130]),
    ("ivory", [255, 255, 240]),
    ("khaki", [240, 230, 140]),
    ("lavender", [230, 230, 250]),
    ("lavenderblush", [255, 240, 245]),
    ("lawngreen", [124, 252, 0]),
    ("lemonchiffon", [255, 250, 205]),
    ("lightblue", [173, 216, 230]),
    ("lightcoral", [240, 128, 128]),
    ("lightcyan", [224, 255, 255]),
    ("lightgoldenrodyellow", [250, 250, 210]),
    ("lightgray", [211, 211, 211]),
    ("lightgreen", [144, 238, 144]),
    ("lightgrey", [211, 211, 211]),
    ("lightpink", [255, 182, 193]),
    ("lightsalmon", [255, 160, 122]),
    ("lightseagreen", [32, 178, 170]),
    ("lightskyblue", [135, 206, 250]),
    ("lightslategray", [119, 136, 153]),
    ("lightslategrey", [119, 136, 153]),
    ("lightsteelblue", [176, 196, 222]),
    ("lightyellow", [255, 255, 224]),
    ("lime", [0, 255, 0]),
    ("limegreen", [50, 205, 50]),
    ("linen", [250, 240, 230]),
    ("magenta", [255, 0, 255]),
    ("maroon", [128, 0, 0]),
    ("mediumaquamarine", [102, 205, 170]),
    ("mediumblue", [0, 0, 205]),
    ("mediumorchid", [186, 85, 211]),
    ("mediumpurple", [147, 112, 219]),
    ("mediumseagreen", [60, 179, 113]),
    ("mediumslateblue", [123, 104, 238]),
    ("mediumspringgreen", [0, 250, 154]),
    ("mediumturquoise", [72, 209, 204]),
    ("mediumvioletred", [199, 21, 133]),
    ("midnightblue", [25, 25, 112]),
    ("mintcream", [245, 255, 250]),
    ("mistyrose", [255, 228, 225]),
    ("moccasin", [255, 228, 181]),
    ("navajowhite", [255, 222, 173]),
    ("navy", [0, 0, 128]),
    ("oldlace", [253, 245, 230]),
    ("olive", [128, 128, 0]),
    ("olivedrab", [107, 142, 35]),
    ("orange", [255, 165, 0]),
    ("orangered", [255, 69, 0]),
    ("orchid", [218, 112, 214]),
    ("palegoldenrod", [238, 232, 170]),
    ("palegreen", [152, 251, 152]),
    ("paleturquoise", [175, 238, 238]),
    ("palevioletred", [219, 112, 147]),
    ("papayawhip", [255, 239, 213]),
    ("peachpuff", [255, 218, 185]),
    ("peru", [205, 133, 63]),
    ("pink", [255, 192, 203]),
    ("plum", [221, 160, 221]),
    ("powderblue", [176, 224, 230]),
    ("purple", [128, 0, 128]),
    ("red", [255, 0, 0]),
    ("rosybrown", [188, 143, 143]),
    ("royalblue", [65, 105, 225]),
    ("saddlebrown", [139, 69, 19]),
    ("salmon", [250, 128, 114]),
    ("sandybrown", [244, 164, 96]),
    ("seagreen", [46, 139, 87]),
    ("seashell", [255, 245, 238]),
    ("sienna", [160, 82, 45]),
    ("silver", [192, 192, 192]),
    ("skyblue", [135, 206, 235]),
    ("slateblue", [106, 90, 205]),
    ("slategray", [112, 128, 144]),
    ("slategrey", [112, 128, 144]),
    ("snow", [255, 250, 250]),
    ("springgreen", [0, 255, 127]),
    ("steelblue", [70, 130, 180]),
    ("tan", [210, 180, 140]),
    ("teal", [0, 128, 128]),
    ("thistle", [216, 191, 216]),
    ("tomato", [255, 99, 71]),
    ("turquoise", [64, 224, 208]),
    ("violet", [238, 130, 238]),
    ("wheat", [245, 222, 179]),
    ("white", [255, 255, 255]),
    ("whitesmoke", [245, 245, 245]),
    ("yellow", [255, 255, 0]),
    ("yellowgreen", [154, 205, 50]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyword_table_is_sorted_and_unique_for_binary_search() {
        for pair in SVG_COLOR_KEYWORDS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }

    #[test]
    fn every_eight_bit_value_round_trips_through_sixteen_bits() {
        for v in 0..=255_u8 {
            assert_eq!(narrow16(widen8(v)), v);
        }
    }

    #[test]
    fn narrowing_rounds_to_the_nearest_eight_bit_value() {
        assert_eq!(narrow16(7067), 27);
        assert_eq!(narrow16(128), 0);
        assert_eq!(narrow16(129), 1);
        assert_eq!(narrow16(u16::MAX), 255);
    }
}
