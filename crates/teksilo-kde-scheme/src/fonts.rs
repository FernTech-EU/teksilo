// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fonts as KDE stores them: Qt's comma-separated font description.
//!
//! The fields, in order: family, point size, pixel size, style hint,
//! weight, style, underline, strike out, fixed pitch, a field that is
//! always 0, then either a style name (the Qt 5 form) or capitalisation,
//! letter-spacing type, letter spacing, word spacing, stretch, style
//! strategy and an optional style name (the Qt 6 form).
//!
//! [`KdeFont::parse_qfont`] accepts 2 fields (family and point size), 10
//! or 11 (Qt 5) and 16 or 17 (Qt 6). Newer Qt versions append further
//! fields; only the first 17 are read. The weight is on the OpenType scale
//! (1 to 1000) in the Qt 6 form and on Qt 5's legacy 0 to 99 scale in the
//! Qt 5 form, which is mapped to the nearest named OpenType weight.

/// A font's slant (field 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    /// Upright, 0 (and any value without a meaning).
    #[default]
    Normal,
    /// Italic, 1.
    Italic,
    /// Oblique, 2.
    Oblique,
}

/// The style hint Qt stores for a monospace font.
pub const STYLE_HINT_MONOSPACE: i32 = 7;

/// The style hint Qt stores when no hint was asked for.
pub const STYLE_HINT_ANY: i32 = 5;

/// The OpenType weight of normal text.
pub const WEIGHT_NORMAL: u16 = 400;

/// A font read from a KDE configuration key.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct KdeFont {
    /// The family name.
    pub family: String,
    /// The size in points, when set (a stored value at or below zero means
    /// unset).
    pub point_size: Option<f64>,
    /// The size in pixels, when set (a stored value at or below zero means
    /// unset).
    pub pixel_size: Option<i32>,
    /// Qt's style hint ([`STYLE_HINT_ANY`], [`STYLE_HINT_MONOSPACE`], ...).
    pub style_hint: i32,
    /// The OpenType weight, 1 to 1000 (400 is normal, 700 bold).
    pub weight: u16,
    /// The slant.
    pub style: FontStyle,
    /// Underlined.
    pub underline: bool,
    /// Struck out.
    pub strike_out: bool,
    /// Fixed pitch.
    pub fixed_pitch: bool,
    /// Qt's capitalisation mode (0 when absent).
    pub capitalization: i32,
    /// Qt's letter-spacing type (0 when absent).
    pub letter_spacing_type: i32,
    /// The letter spacing (0 when absent).
    pub letter_spacing: f64,
    /// The word spacing (0 when absent).
    pub word_spacing: f64,
    /// The stretch (0, "any", when absent).
    pub stretch: i32,
    /// Qt's style strategy (1, the default, when absent).
    pub style_strategy: i32,
    /// The style name, such as `Regular` or `Bold Italic`, when stored.
    pub style_name: Option<String>,
}

/// The legacy (Qt 5) weights of the named weights, with their OpenType
/// values, lightest first.
const LEGACY_WEIGHTS: [(i32, u16); 9] = [
    (0, 100),
    (12, 200),
    (25, 300),
    (50, 400),
    (57, 500),
    (63, 600),
    (75, 700),
    (81, 800),
    (87, 900),
];

impl KdeFont {
    /// A regular, upright font of `family` at `point_size` points, every
    /// other field at its default.
    pub fn new(family: impl Into<String>, point_size: f64) -> KdeFont {
        KdeFont {
            family: family.into(),
            point_size: Some(point_size),
            pixel_size: None,
            style_hint: STYLE_HINT_ANY,
            weight: WEIGHT_NORMAL,
            style: FontStyle::Normal,
            underline: false,
            strike_out: false,
            fixed_pitch: false,
            capitalization: 0,
            letter_spacing_type: 0,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            stretch: 0,
            style_strategy: 1,
            style_name: None,
        }
    }

    /// Parses Qt's font description. Returns `None` for a field count
    /// other than 2, 10, 11, 16, 17 or more, or when a numeric field does
    /// not parse.
    pub fn parse_qfont(text: &str) -> Option<KdeFont> {
        let fields: Vec<&str> = text.split(',').collect();
        let count = fields.len();
        let fields = &fields[..count.min(17)];
        let qt6 = match fields.len() {
            2 => None,
            10 | 11 => Some(false),
            16 | 17 => Some(true),
            _ => return None,
        };

        let mut font = KdeFont::new(fields[0], 0.0);
        font.point_size = Some(parse_f64(fields[1])?).filter(|&p| p > 0.0);
        let Some(qt6) = qt6 else {
            return Some(font);
        };

        font.pixel_size = Some(parse_i32(fields[2])?).filter(|&p| p > 0);
        font.style_hint = parse_i32(fields[3])?;
        let weight = parse_i32(fields[4])?;
        font.weight = if qt6 {
            weight.clamp(1, 1000) as u16
        } else {
            legacy_weight(weight)
        };
        font.style = match parse_i32(fields[5])? {
            1 => FontStyle::Italic,
            2 => FontStyle::Oblique,
            _ => FontStyle::Normal,
        };
        font.underline = parse_i32(fields[6])? != 0;
        font.strike_out = parse_i32(fields[7])? != 0;
        font.fixed_pitch = parse_i32(fields[8])? != 0;
        parse_i32(fields[9])?;

        let style_name_field = if qt6 {
            font.capitalization = parse_i32(fields[10])?;
            font.letter_spacing_type = parse_i32(fields[11])?;
            font.letter_spacing = parse_f64(fields[12])?;
            font.word_spacing = parse_f64(fields[13])?;
            font.stretch = parse_i32(fields[14])?;
            font.style_strategy = parse_i32(fields[15])?;
            fields.get(16)
        } else {
            fields.get(10)
        };
        font.style_name = style_name_field
            .filter(|name| !name.is_empty())
            .map(|name| (*name).to_owned());
        Some(font)
    }

    /// The size in logical pixels: the pixel size when one is set,
    /// otherwise the point size at 96 logical pixels per 72 points. `None`
    /// when neither is set.
    pub fn size_px(&self) -> Option<f64> {
        match (self.pixel_size, self.point_size) {
            (Some(px), _) => Some(f64::from(px)),
            (None, Some(pt)) => Some(pt * 96.0 / 72.0),
            (None, None) => None,
        }
    }
}

/// Maps a Qt 5 legacy weight to the OpenType weight of the nearest named
/// weight, a tie going to the lighter one. Any `i32` is accepted: the
/// distances are taken in `i64`, so a weight a user wrote near `i32::MIN`
/// or `i32::MAX` maps to the lightest or the heaviest weight instead of
/// overflowing.
pub fn legacy_weight(weight: i32) -> u16 {
    let distance = |named: i32| (i64::from(weight) - i64::from(named)).abs();
    let mut best = LEGACY_WEIGHTS[0];
    for candidate in LEGACY_WEIGHTS {
        if distance(candidate.0) < distance(best.0) {
            best = candidate;
        }
    }
    best.1
}

fn parse_i32(field: &str) -> Option<i32> {
    field.trim().parse().ok()
}

fn parse_f64(field: &str) -> Option<f64> {
    field.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_weights_go_to_the_nearest_named_weight_and_ties_to_the_lighter() {
        assert_eq!(legacy_weight(50), 400);
        assert_eq!(legacy_weight(60), 500);
        assert_eq!(legacy_weight(70), 700);
        assert_eq!(legacy_weight(0), 100);
        assert_eq!(legacy_weight(99), 900);
        assert_eq!(legacy_weight(-5), 100);
        // 6 is halfway between 0 and 12.
        assert_eq!(legacy_weight(6), 100);
        assert_eq!(legacy_weight(7), 200);
    }

    #[test]
    fn legacy_weights_at_the_ends_of_the_integer_range_do_not_overflow() {
        assert_eq!(legacy_weight(i32::MIN), 100);
        assert_eq!(legacy_weight(i32::MIN + 1), 100);
        assert_eq!(legacy_weight(i32::MAX), 900);
        assert_eq!(legacy_weight(i32::MAX - 1), 900);
    }
}
