// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Terminal colour model: the themeable 16-colour ANSI palette plus the fixed
//! xterm 256-colour cube, and how a cell's [`TermColor`] resolves to a concrete
//! [`bastyde_tokens::Color`].
//!
//! Colour resolution is deliberately a *view* concern (not the engine's): the
//! engine reports each cell's colour symbolically ([`TermColor`]) and the
//! [`ColorScheme`] the app installs decides the actual pixels. This is what lets
//! the same running shell re-theme live (light/dark, custom scheme) with no
//! restart, and keeps the engine trait free of any palette state.

use bastyde_tokens::Color;

/// A cell's colour as reported by the terminal engine — resolved against a
/// [`ColorScheme`] at paint time.
///
/// The engine adapter normalises the raw VT colour (SGR default / one of the 16
/// ANSI slots / an xterm-256 index / a 24-bit truecolour value) into this small
/// closed set so the renderer and the [`ColorScheme`] never depend on the
/// engine's own colour type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TermColor {
    /// The scheme's default foreground (SGR 39 / an unset fg).
    DefaultFg,
    /// The scheme's default background (SGR 49 / an unset bg).
    DefaultBg,
    /// The scheme's cursor colour (used when a cell paints under the block
    /// cursor and the engine reports the special "cursor" colour).
    Cursor,
    /// One of the 16 themeable ANSI slots: `0..=7` normal, `8..=15` bright.
    Ansi(u8),
    /// An xterm 256-colour index in `16..=255` (the 6×6×6 cube and the 24-step
    /// grayscale ramp). Fixed by the xterm spec — never themed.
    Indexed(u8),
    /// A 24-bit truecolour value (SGR 38/48;2).
    Rgb(u8, u8, u8),
}

/// Build a [`Color`] from 8-bit sRGB components.
#[inline]
pub(crate) fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

/// A full terminal colour scheme: the 16 ANSI slots plus the default
/// foreground / background / cursor / selection colours.
///
/// Index `0..=7` are the normal ANSI colours (black, red, green, yellow, blue,
/// magenta, cyan, white); `8..=15` are their bright variants.
#[derive(Debug, Clone)]
pub struct ColorScheme {
    /// The 16 ANSI palette slots (`0..=7` normal, `8..=15` bright).
    pub palette: [Color; 16],
    /// Default text colour.
    pub foreground: Color,
    /// Default background colour.
    pub background: Color,
    /// Cursor block colour.
    pub cursor: Color,
    /// Text colour under the block cursor (contrasts with [`Self::cursor`]).
    pub cursor_text: Color,
    /// Selection highlight background.
    pub selection_background: Color,
    /// Selection text colour. `None` keeps each cell's own foreground (the
    /// common "tint the background only" behaviour).
    pub selection_foreground: Option<Color>,
    /// When `true`, a **bold** cell whose foreground is one of the normal ANSI
    /// slots (`0..=7`) is drawn using the bright slot (`8..=15`) instead — the
    /// traditional "bold is bright" terminal behaviour. Independent of the
    /// font's own weight.
    pub bold_is_bright: bool,
}

impl ColorScheme {
    /// Resolve a [`TermColor`] to a concrete colour, honouring the `bold`
    /// attribute for the "bold is bright" promotion of normal ANSI slots.
    pub fn resolve(&self, color: TermColor, bold: bool) -> Color {
        match color {
            TermColor::DefaultFg => self.foreground,
            TermColor::DefaultBg => self.background,
            TermColor::Cursor => self.cursor,
            TermColor::Ansi(idx) => {
                let idx = idx & 0x0f;
                let idx = if self.bold_is_bright && bold && idx < 8 {
                    idx + 8
                } else {
                    idx
                };
                self.palette[idx as usize]
            }
            TermColor::Indexed(idx) => xterm_256(idx, &self.palette),
            TermColor::Rgb(r, g, b) => rgb8(r, g, b),
        }
    }

    /// A balanced dark scheme (the default). Palette is a lightly-tuned,
    /// widely-legible 16-colour set close to the common "one dark" family.
    pub fn dark() -> Self {
        Self {
            palette: [
                rgb8(0x1e, 0x22, 0x2a), // 0 black
                rgb8(0xe0, 0x6c, 0x75), // 1 red
                rgb8(0x98, 0xc3, 0x79), // 2 green
                rgb8(0xe5, 0xc0, 0x7b), // 3 yellow
                rgb8(0x61, 0xaf, 0xef), // 4 blue
                rgb8(0xc6, 0x78, 0xdd), // 5 magenta
                rgb8(0x56, 0xb6, 0xc2), // 6 cyan
                rgb8(0xab, 0xb2, 0xbf), // 7 white
                rgb8(0x5c, 0x63, 0x70), // 8 bright black
                rgb8(0xef, 0x83, 0x8c), // 9 bright red
                rgb8(0xa7, 0xd3, 0x86), // 10 bright green
                rgb8(0xf0, 0xd0, 0x8b), // 11 bright yellow
                rgb8(0x74, 0xbe, 0xff), // 12 bright blue
                rgb8(0xd6, 0x8c, 0xed), // 13 bright magenta
                rgb8(0x66, 0xc6, 0xd2), // 14 bright cyan
                rgb8(0xd8, 0xde, 0xe9), // 15 bright white
            ],
            foreground: rgb8(0xab, 0xb2, 0xbf),
            background: rgb8(0x1e, 0x22, 0x2a),
            cursor: rgb8(0x61, 0xaf, 0xef),
            cursor_text: rgb8(0x1e, 0x22, 0x2a),
            selection_background: rgb8(0x3e, 0x44, 0x51),
            selection_foreground: None,
            bold_is_bright: false,
        }
    }

    /// A balanced light scheme.
    pub fn light() -> Self {
        Self {
            palette: [
                rgb8(0x3b, 0x40, 0x48), // 0 black
                rgb8(0xd7, 0x3a, 0x49), // 1 red
                rgb8(0x40, 0x8a, 0x3e), // 2 green
                rgb8(0xb9, 0x86, 0x00), // 3 yellow
                rgb8(0x21, 0x6e, 0xdb), // 4 blue
                rgb8(0xa6, 0x26, 0xa4), // 5 magenta
                rgb8(0x1a, 0x82, 0x92), // 6 cyan
                rgb8(0x50, 0x56, 0x61), // 7 white
                rgb8(0x69, 0x6c, 0x77), // 8 bright black
                rgb8(0xe4, 0x56, 0x49), // 9 bright red
                rgb8(0x4c, 0xa2, 0x4b), // 10 bright green
                rgb8(0xd0, 0x9c, 0x00), // 11 bright yellow
                rgb8(0x40, 0x86, 0xf4), // 12 bright blue
                rgb8(0xbf, 0x3d, 0xbf), // 13 bright magenta
                rgb8(0x25, 0x9a, 0xac), // 14 bright cyan
                rgb8(0x2f, 0x34, 0x3d), // 15 bright white
            ],
            foreground: rgb8(0x38, 0x3a, 0x42),
            background: rgb8(0xfa, 0xfa, 0xfa),
            cursor: rgb8(0x21, 0x6e, 0xdb),
            cursor_text: rgb8(0xfa, 0xfa, 0xfa),
            selection_background: rgb8(0xd4, 0xdd, 0xea),
            selection_foreground: None,
            bold_is_bright: false,
        }
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::dark()
    }
}

/// Resolve an xterm 256-colour index to RGB. Indices `0..=15` map back to the
/// supplied 16-slot palette; `16..=231` are the 6×6×6 colour cube; `232..=255`
/// are the 24-step grayscale ramp.
fn xterm_256(idx: u8, palette: &[Color; 16]) -> Color {
    match idx {
        0..=15 => palette[idx as usize],
        16..=231 => {
            let i = idx - 16;
            let r = i / 36;
            let g = (i / 6) % 6;
            let b = i % 6;
            let level = |v: u8| -> u8 { if v == 0 { 0 } else { 55 + 40 * v } };
            rgb8(level(r), level(g), level(b))
        }
        232..=255 => {
            let gray = 8 + 10 * (idx - 232);
            rgb8(gray, gray, gray)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_cube_endpoints() {
        let p = ColorScheme::dark().palette;
        // 16 is the cube's black corner.
        assert_eq!(xterm_256(16, &p), rgb8(0, 0, 0));
        // 231 is the cube's white corner (all components at level 5 = 255).
        assert_eq!(xterm_256(231, &p), rgb8(255, 255, 255));
        // 196 is pure red (r=5,g=0,b=0).
        assert_eq!(xterm_256(196, &p), rgb8(255, 0, 0));
    }

    #[test]
    fn grayscale_ramp() {
        let p = ColorScheme::dark().palette;
        assert_eq!(xterm_256(232, &p), rgb8(8, 8, 8));
        assert_eq!(xterm_256(255, &p), rgb8(238, 238, 238));
    }

    #[test]
    fn bold_is_bright_promotes_normal_slots() {
        let mut scheme = ColorScheme::dark();
        scheme.bold_is_bright = true;
        // Slot 1 (red) bold → slot 9 (bright red).
        assert_eq!(scheme.resolve(TermColor::Ansi(1), true), scheme.palette[9]);
        // Not bold → stays slot 1.
        assert_eq!(scheme.resolve(TermColor::Ansi(1), false), scheme.palette[1]);
        // Bright slots are never promoted further.
        assert_eq!(scheme.resolve(TermColor::Ansi(9), true), scheme.palette[9]);
    }
}
