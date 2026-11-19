// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! How the application asks glyphs to be fitted to the pixel grid (see
//! [`TeksiloAppBuilder::glyph_rendering`](crate::TeksiloAppBuilder::glyph_rendering)).

use teksilo_platform::font_hinting::DesktopFontHinting;
use teksilo_text::GlyphRendering;

/// How text is fitted to the pixel grid, in every window.
///
/// Natural rendering keeps the font's spacing exactly and draws each glyph
/// at its position to a quarter of a pixel, hinted vertically only, as
/// browsers and macOS do. Crisp rendering puts glyphs on whole pixels,
/// fitted to the grid on both axes, and lays text out with those whole-pixel
/// widths, as Windows' classic rendering and FreeType's full hinting do:
/// sharper and darker stems at 100 % scale, lines 1 to 5 % wider. Text
/// drawn under a zoom stays natural.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GlyphRenderingPreference {
    /// What the desktop asks of every application: crisp where the user
    /// chose medium or full hinting (KDE Plasma's font settings; GNOME's
    /// font hinting under manual font rendering), natural elsewhere and on
    /// Windows and macOS. A change on the desktop is followed while the
    /// application runs.
    #[default]
    FollowDesktop,
    /// Natural, whatever the desktop says.
    Natural,
    /// Crisp, whatever the desktop says.
    Crisp,
}

impl GlyphRenderingPreference {
    /// The rendering this preference gives on a desktop asking for
    /// `desktop` (`None`: it asks for nothing).
    pub fn resolve(self, desktop: Option<DesktopFontHinting>) -> GlyphRendering {
        match self {
            Self::Natural => GlyphRendering::Natural,
            Self::Crisp => GlyphRendering::Crisp,
            Self::FollowDesktop => {
                if desktop.is_some_and(DesktopFontHinting::fits_both_axes) {
                    GlyphRendering::Crisp
                } else {
                    GlyphRendering::Natural
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn following_the_desktop_is_crisp_for_medium_and_full_hinting_only() {
        let follow = GlyphRenderingPreference::FollowDesktop;
        assert_eq!(
            follow.resolve(Some(DesktopFontHinting::Full)),
            GlyphRendering::Crisp
        );
        assert_eq!(
            follow.resolve(Some(DesktopFontHinting::Medium)),
            GlyphRendering::Crisp
        );
        assert_eq!(
            follow.resolve(Some(DesktopFontHinting::Slight)),
            GlyphRendering::Natural
        );
        assert_eq!(follow.resolve(None), GlyphRendering::Natural);
    }

    #[test]
    fn a_fixed_choice_ignores_the_desktop() {
        assert_eq!(
            GlyphRenderingPreference::Natural.resolve(Some(DesktopFontHinting::Full)),
            GlyphRendering::Natural
        );
        assert_eq!(
            GlyphRenderingPreference::Crisp.resolve(None),
            GlyphRendering::Crisp
        );
    }
}
