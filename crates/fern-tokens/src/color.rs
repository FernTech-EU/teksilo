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
    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let h_prime = h / 60.0;
        let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
        let (r1, g1, b1) = match h_prime as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            5 => (c, 0.0, x),
            _ => (0.0, 0.0, 0.0),
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
}
