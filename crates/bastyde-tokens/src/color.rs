// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use serde::{Deserialize, Serialize};

/// A color represented as four f32 components (red, green, blue, alpha) in the range 0.0..=1.0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Color {
    pub const WHITE: Color = Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const BLACK: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const RED: Color = Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const GREEN: Color = Color {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    };
    pub const BLUE: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Create a color from HSL (Hue 0-360, Saturation 0-1, Lightness 0-1).
    ///
    /// Hue wraps (`rem_euclid(360)`) and S/L are clamped, mirroring
    /// [`Color::from_hsva`]. Without the wrap, `h == 360.0` gave `h_prime == 6`
    /// which fell through to the catch-all arm and produced black instead of
    /// red (360° ≡ 0°).
    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        let h = h.rem_euclid(360.0);
        let s = s.clamp(0.0, 1.0);
        let l = l.clamp(0.0, 1.0);
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let h_prime = h / 60.0;
        let x = c * (1.0 - (h_prime.rem_euclid(2.0) - 1.0).abs());
        // Catch-all is the 300°–360° (magenta) sextant, not black — matches
        // `from_hsva`, and the hue wrap above keeps `h_prime` in `0.0..6.0`.
        let (r1, g1, b1) = match h_prime as i32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = l - c / 2.0;
        Self::from_rgb(r1 + m, g1 + m, b1 + m)
    }

    /// Parse a hex color string like "#2E7D32" or "#2E7D32FF".
    pub fn from_hex(hex: &str) -> Self {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        let len = hex.len();
        let parse_byte = |s: &str| -> f32 { u8::from_str_radix(s, 16).unwrap_or(0) as f32 / 255.0 };
        match len {
            6 => Self {
                r: parse_byte(&hex[0..2]),
                g: parse_byte(&hex[2..4]),
                b: parse_byte(&hex[4..6]),
                a: 1.0,
            },
            8 => Self {
                r: parse_byte(&hex[0..2]),
                g: parse_byte(&hex[2..4]),
                b: parse_byte(&hex[4..6]),
                a: parse_byte(&hex[6..8]),
            },
            _ => Self::BLACK,
        }
    }

    pub fn r(&self) -> f32 {
        self.r
    }

    pub fn g(&self) -> f32 {
        self.g
    }

    pub fn b(&self) -> f32 {
        self.b
    }

    pub fn a(&self) -> f32 {
        self.a
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Mix two colors linearly. `t=0.0` returns `self`, `t=1.0` returns `other`.
    /// Alpha is also interpolated.
    pub fn mix(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        Color {
            r: self.r * inv + other.r * t,
            g: self.g * inv + other.g * t,
            b: self.b * inv + other.b * t,
            a: self.a * inv + other.a * t,
        }
    }

    /// Darken the color by mixing with black. `amount=0.0` is unchanged, `1.0` is black.
    pub fn darken(self, amount: f32) -> Color {
        self.mix(Color::new(0.0, 0.0, 0.0, self.a), amount)
    }

    /// Lighten the color by mixing with white. `amount=0.0` is unchanged, `1.0` is white.
    pub fn lighten(self, amount: f32) -> Color {
        self.mix(Color::new(1.0, 1.0, 1.0, self.a), amount)
    }

    /// Desaturate toward the perceptual-luminance gray of the same brightness.
    /// `amount=0.0` is unchanged, `1.0` is fully gray (alpha preserved).
    /// Because it mixes toward a gray of equal luminance, a bright colour
    /// becomes a bright gray and a dark colour a dark gray — so it reads
    /// correctly in both light and dark themes. Used for the inactive-window
    /// accent projection ([`crate::ColorTokens::for_inactive_window`]).
    pub fn desaturated(self, amount: f32) -> Color {
        let luma = 0.299 * self.r + 0.587 * self.g + 0.114 * self.b;
        self.mix(Color::new(luma, luma, luma, self.a), amount)
    }

    /// Format as `#RRGGBB` (uppercase). With `include_alpha = true`, returns
    /// `#RRGGBBAA`. Channels are quantized to 8-bit by rounding `f32 * 255.0`.
    /// Inverse of [`Color::from_hex`] modulo quantization.
    pub fn to_hex_upper(&self, include_alpha: bool) -> String {
        let r = (self.r * 255.0).round().clamp(0.0, 255.0) as u8;
        let g = (self.g * 255.0).round().clamp(0.0, 255.0) as u8;
        let b = (self.b * 255.0).round().clamp(0.0, 255.0) as u8;
        if include_alpha {
            let a = (self.a * 255.0).round().clamp(0.0, 255.0) as u8;
            format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, a)
        } else {
            format!("#{:02X}{:02X}{:02X}", r, g, b)
        }
    }

    /// Lowercase variant of [`Color::to_hex_upper`].
    pub fn to_hex_lower(&self, include_alpha: bool) -> String {
        self.to_hex_upper(include_alpha).to_lowercase()
    }

    /// Convert sRGB to HSV. Returns `(hue 0..360, saturation 0..1, value 0..1)`.
    /// Hue is undefined when saturation is `0` (gray); returns `0.0` by convention
    /// so round-trips on grays are stable.
    pub fn to_hsv(&self) -> (f32, f32, f32) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let delta = max - min;

        let h = if delta < 1e-6 {
            0.0
        } else if (max - self.r).abs() < 1e-6 {
            60.0 * (((self.g - self.b) / delta).rem_euclid(6.0))
        } else if (max - self.g).abs() < 1e-6 {
            60.0 * ((self.b - self.r) / delta + 2.0)
        } else {
            60.0 * ((self.r - self.g) / delta + 4.0)
        };
        let h = if h < 0.0 { h + 360.0 } else { h };

        let s = if max < 1e-6 { 0.0 } else { delta / max };
        let v = max;

        (h, s, v)
    }

    /// Convert sRGB-with-alpha to HSV-with-alpha.
    /// `(hue 0..360, saturation 0..1, value 0..1, alpha 0..1)`.
    pub fn to_hsva(&self) -> (f32, f32, f32, f32) {
        let (h, s, v) = self.to_hsv();
        (h, s, v, self.a)
    }

    /// Build a Color from HSV. Hue is wrapped modulo 360 (handles negative
    /// values too); saturation and value are clamped to `0..=1`. Alpha is `1.0`.
    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        Self::from_hsva(h, s, v, 1.0)
    }

    /// Build a Color from HSV-with-alpha. Same wrapping/clamping rules as
    /// [`Color::from_hsv`]; alpha is also clamped to `0..=1`.
    pub fn from_hsva(h: f32, s: f32, v: f32, a: f32) -> Self {
        let h = h.rem_euclid(360.0);
        let s = s.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        let a = a.clamp(0.0, 1.0);

        let c = v * s;
        let h_prime = h / 60.0;
        let x = c * (1.0 - (h_prime.rem_euclid(2.0) - 1.0).abs());
        let (r1, g1, b1) = match h_prime as i32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        Self::from_rgba(r1 + m, g1 + m, b1 + m, a)
    }

    /// Compute the relative luminance (WCAG 2.x formula).
    /// Returns a value in 0.0..=1.0 where 0 is black and 1 is white.
    pub fn relative_luminance(self) -> f32 {
        fn linearize(c: f32) -> f32 {
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linearize(self.r) + 0.7152 * linearize(self.g) + 0.0722 * linearize(self.b)
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desaturated_moves_toward_equal_luminance_gray() {
        let c = Color::from_hex("#0FB5CC"); // teal accent
        // Fully desaturated → r == g == b (a gray), alpha preserved.
        let g = c.desaturated(1.0);
        assert!((g.r() - g.g()).abs() < 1e-6 && (g.g() - g.b()).abs() < 1e-6);
        let luma = 0.299 * c.r() + 0.587 * c.g() + 0.114 * c.b();
        assert!((g.r() - luma).abs() < 1e-6);
        // amount 0 → unchanged; alpha always preserved.
        assert_eq!(c.desaturated(0.0).to_array(), c.to_array());
        assert_eq!(c.desaturated(0.5).a(), c.a());
        // Partial desaturation moves toward gray (reduces channel spread).
        let spread = |x: Color| (x.r() - x.b()).abs();
        assert!(spread(c.desaturated(0.7)) < spread(c));
    }

    #[test]
    fn color_from_hex_6_digits() {
        let c = Color::from_hex("#2E7D32");
        let expected_r = 0x2E as f32 / 255.0;
        let expected_g = 0x7D as f32 / 255.0;
        let expected_b = 0x32 as f32 / 255.0;
        assert!((c.r() - expected_r).abs() < f32::EPSILON);
        assert!((c.g() - expected_g).abs() < f32::EPSILON);
        assert!((c.b() - expected_b).abs() < f32::EPSILON);
        assert!((c.a() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn color_from_hsl() {
        let red = Color::from_hsl(0.0, 1.0, 0.5);
        assert!((red.r() - 1.0).abs() < 0.01);
        assert!(red.g() < 0.01);
        assert!(red.b() < 0.01);

        let green = Color::from_hsl(120.0, 1.0, 0.5);
        assert!(green.r() < 0.01);
        assert!((green.g() - 1.0).abs() < 0.01);
    }

    #[test]
    fn color_from_hsl_wraps_hue_360_to_red() {
        // Regression: h == 360.0 used to fall through to the catch-all arm and
        // produce black. 360° ≡ 0° must be red, matching from_hsl(0.0, ...).
        let wrapped = Color::from_hsl(360.0, 1.0, 0.5);
        assert!((wrapped.r() - 1.0).abs() < 0.01, "r={}", wrapped.r());
        assert!(wrapped.g() < 0.01, "g={}", wrapped.g());
        assert!(wrapped.b() < 0.01, "b={}", wrapped.b());
        // Negative / over-range hues also wrap rather than blacking out.
        let neg = Color::from_hsl(-360.0, 1.0, 0.5);
        assert!((neg.r() - 1.0).abs() < 0.01);
    }

    #[test]
    fn color_from_hex_8_digits() {
        let c = Color::from_hex("#FF000080");
        assert!((c.r() - 1.0).abs() < f32::EPSILON);
        assert!((c.a() - 128.0 / 255.0).abs() < f32::EPSILON);
    }

    #[test]
    fn color_from_hex_without_hash() {
        let c = Color::from_hex("FF0000");
        assert!((c.r() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn color_to_array() {
        let c = Color::from_rgba(1.0, 0.0, 0.0, 1.0);
        assert_eq!(c.to_array(), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn color_constants() {
        assert_eq!(Color::WHITE.to_array(), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(Color::BLACK.to_array(), [0.0, 0.0, 0.0, 1.0]);
        assert!((Color::TRANSPARENT.a() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn color_with_alpha() {
        let c = Color::RED.with_alpha(0.5);
        assert!((c.r() - 1.0).abs() < f32::EPSILON);
        assert!((c.a() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn color_equality() {
        assert_eq!(Color::RED, Color::from_rgba(1.0, 0.0, 0.0, 1.0));
        assert_ne!(Color::RED, Color::BLUE);
    }

    #[test]
    fn color_serde_roundtrip() {
        let c = Color::from_hex("#2E7D32");
        let json = serde_json::to_string(&c).unwrap();
        let deserialized: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(c, deserialized);
    }

    #[test]
    fn mix_endpoints() {
        let a = Color::RED;
        let b = Color::BLUE;
        // t=0 returns self
        let m0 = a.mix(b, 0.0);
        assert!((m0.r() - 1.0).abs() < f32::EPSILON);
        assert!(m0.b().abs() < f32::EPSILON);
        // t=1 returns other
        let m1 = a.mix(b, 1.0);
        assert!(m1.r().abs() < f32::EPSILON);
        assert!((m1.b() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn mix_midpoint() {
        let a = Color::BLACK;
        let b = Color::WHITE;
        let mid = a.mix(b, 0.5);
        assert!((mid.r() - 0.5).abs() < f32::EPSILON);
        assert!((mid.g() - 0.5).abs() < f32::EPSILON);
        assert!((mid.b() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn mix_clamps_t() {
        let a = Color::RED;
        let b = Color::BLUE;
        // t > 1 should clamp to 1
        let m = a.mix(b, 2.0);
        assert!(m.r().abs() < f32::EPSILON);
        assert!((m.b() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn darken_zero_unchanged() {
        let c = Color::from_hex("#3584e4");
        let d = c.darken(0.0);
        assert!((d.r() - c.r()).abs() < f32::EPSILON);
        assert!((d.g() - c.g()).abs() < f32::EPSILON);
        assert!((d.b() - c.b()).abs() < f32::EPSILON);
    }

    #[test]
    fn darken_one_is_black() {
        let c = Color::from_hex("#3584e4");
        let d = c.darken(1.0);
        assert!(d.r().abs() < f32::EPSILON);
        assert!(d.g().abs() < f32::EPSILON);
        assert!(d.b().abs() < f32::EPSILON);
    }

    #[test]
    fn lighten_one_is_white() {
        let c = Color::from_hex("#3584e4");
        let l = c.lighten(1.0);
        assert!((l.r() - 1.0).abs() < f32::EPSILON);
        assert!((l.g() - 1.0).abs() < f32::EPSILON);
        assert!((l.b() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn relative_luminance_black_is_zero() {
        assert!(Color::BLACK.relative_luminance().abs() < 0.001);
    }

    #[test]
    fn relative_luminance_white_is_one() {
        assert!((Color::WHITE.relative_luminance() - 1.0).abs() < 0.001);
    }

    #[test]
    fn relative_luminance_ordering() {
        // White > light gray > dark gray > black
        let light = Color::from_rgb(0.8, 0.8, 0.8);
        let dark = Color::from_rgb(0.2, 0.2, 0.2);
        assert!(Color::WHITE.relative_luminance() > light.relative_luminance());
        assert!(light.relative_luminance() > dark.relative_luminance());
        assert!(dark.relative_luminance() > Color::BLACK.relative_luminance());
    }

    #[test]
    fn hsv_roundtrip_primary_colors() {
        for color in &[Color::RED, Color::GREEN, Color::BLUE] {
            let (h, s, v) = color.to_hsv();
            let back = Color::from_hsv(h, s, v);
            assert!(
                (back.r() - color.r()).abs() < 0.01,
                "r mismatch for {color:?}"
            );
            assert!(
                (back.g() - color.g()).abs() < 0.01,
                "g mismatch for {color:?}"
            );
            assert!(
                (back.b() - color.b()).abs() < 0.01,
                "b mismatch for {color:?}"
            );
        }
    }

    #[test]
    fn hsv_roundtrip_secondary_colors() {
        let yellow = Color::from_rgb(1.0, 1.0, 0.0);
        let cyan = Color::from_rgb(0.0, 1.0, 1.0);
        let magenta = Color::from_rgb(1.0, 0.0, 1.0);
        for color in &[yellow, cyan, magenta] {
            let (h, s, v) = color.to_hsv();
            let back = Color::from_hsv(h, s, v);
            assert!((back.r() - color.r()).abs() < 0.01);
            assert!((back.g() - color.g()).abs() < 0.01);
            assert!((back.b() - color.b()).abs() < 0.01);
        }
    }

    #[test]
    fn hsv_gray_zero_saturation() {
        let gray = Color::from_rgb(0.5, 0.5, 0.5);
        let (h, s, v) = gray.to_hsv();
        assert_eq!(h, 0.0);
        assert!(s.abs() < 1e-6);
        assert!((v - 0.5).abs() < 1e-6);
    }

    #[test]
    fn hsv_white_full_value_zero_saturation() {
        let (_h, s, v) = Color::WHITE.to_hsv();
        assert!(s.abs() < 1e-6);
        assert!((v - 1.0).abs() < 1e-6);
    }

    #[test]
    fn hsv_black_zero_value() {
        let (_h, _s, v) = Color::BLACK.to_hsv();
        assert!(v.abs() < 1e-6);
    }

    #[test]
    fn from_hsv_wraps_positive_hue() {
        let a = Color::from_hsv(370.0, 1.0, 1.0);
        let b = Color::from_hsv(10.0, 1.0, 1.0);
        assert!((a.r() - b.r()).abs() < 0.01);
        assert!((a.g() - b.g()).abs() < 0.01);
        assert!((a.b() - b.b()).abs() < 0.01);
    }

    #[test]
    fn from_hsv_wraps_negative_hue() {
        let a = Color::from_hsv(-10.0, 1.0, 1.0);
        let b = Color::from_hsv(350.0, 1.0, 1.0);
        assert!((a.r() - b.r()).abs() < 0.01);
        assert!((a.g() - b.g()).abs() < 0.01);
        assert!((a.b() - b.b()).abs() < 0.01);
    }

    #[test]
    fn from_hsv_clamps_saturation_value() {
        // Out-of-range S/V should clamp, not panic, and produce a valid color
        let c = Color::from_hsv(0.0, 1.5, 1.5);
        assert!((c.r() - 1.0).abs() < 0.01);
        assert!(c.g().abs() < 0.01);
        assert!(c.b().abs() < 0.01);
    }

    #[test]
    fn hsva_alpha_roundtrip() {
        let c = Color::from_rgba(1.0, 0.0, 0.0, 0.5);
        let (h, s, v, a) = c.to_hsva();
        let back = Color::from_hsva(h, s, v, a);
        assert!((back.a() - 0.5).abs() < 1e-6);
        assert!((back.r() - 1.0).abs() < 0.01);
    }

    #[test]
    fn to_hex_upper_no_alpha() {
        assert_eq!(Color::RED.to_hex_upper(false), "#FF0000");
        assert_eq!(Color::WHITE.to_hex_upper(false), "#FFFFFF");
        assert_eq!(Color::BLACK.to_hex_upper(false), "#000000");
    }

    #[test]
    fn to_hex_upper_with_alpha() {
        let c = Color::from_rgba(1.0, 0.0, 0.0, 0.5);
        // 0.5 * 255 = 127.5 → rounds to 128 = 0x80
        assert_eq!(c.to_hex_upper(true), "#FF000080");
    }

    #[test]
    fn to_hex_lower_matches_uppercase() {
        let c = Color::from_hex("#3584E4");
        assert_eq!(c.to_hex_lower(false), "#3584e4");
    }

    #[test]
    fn to_hex_roundtrips_through_from_hex() {
        let c = Color::from_hex("#3584E4");
        assert_eq!(Color::from_hex(&c.to_hex_upper(false)), c);
    }
}
