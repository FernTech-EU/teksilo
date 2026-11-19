// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! How text is drawn on screen, as the application or the user asks: how
//! glyphs are fitted to the pixel grid
//! ([`TeksiloAppBuilder::glyph_rendering`](crate::TeksiloAppBuilder::glyph_rendering))
//! and how dark their edges are
//! ([`TeksiloAppBuilder::text_contrast`](crate::TeksiloAppBuilder::text_contrast)),
//! with the settings keys that persist both.

use serde::{Deserialize, Serialize};
use teksilo_core::signal::Signal;
use teksilo_platform::font_hinting::DesktopFontHinting;
use teksilo_render::TextContrast;
use teksilo_settings::SettingsKey;
use teksilo_text::GlyphRendering;

/// The user's text contrast, persisted: read live by `teksilo-app` when the
/// application installs a settings bundle and passes no
/// [`text_contrast`](crate::TeksiloAppBuilder::text_contrast) of its own.
/// Bind a control to `ctx.settings().signal_for(&TEXT_CONTRAST_KEY)`: a
/// change applies to every window at the end of the turn.
pub const TEXT_CONTRAST_KEY: SettingsKey<TextContrastLevel> =
    SettingsKey::new("accessibility.text_contrast", || {
        TextContrastLevel::Standard
    });

/// The user's glyph rendering, persisted: read live by `teksilo-app` when
/// the application installs a settings bundle and passes no
/// [`glyph_rendering`](crate::TeksiloAppBuilder::glyph_rendering) of its
/// own. Bind a control to `ctx.settings().signal_for(&GLYPH_RENDERING_KEY)`.
pub const GLYPH_RENDERING_KEY: SettingsKey<GlyphRenderingPreference> =
    SettingsKey::new("appearance.glyph_rendering", || {
        GlyphRenderingPreference::FollowDesktop
    });

/// A named text contrast, as a preference stores it (see
/// [`TEXT_CONTRAST_KEY`]). Stored as `"standard"`, `"stronger"`,
/// `"strongest"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TextContrastLevel {
    /// [`TextContrast::DEFAULT`]: glyph edges as dark as a browser draws them.
    #[default]
    Standard,
    /// [`TextContrast::STRONGER`].
    Stronger,
    /// [`TextContrast::STRONGEST`].
    Strongest,
}

impl TextContrastLevel {
    /// Every level, weakest first: the options a picker offers.
    pub fn all() -> &'static [Self] {
        &[Self::Standard, Self::Stronger, Self::Strongest]
    }

    /// The contrast this level names.
    pub fn contrast(self) -> TextContrast {
        match self {
            Self::Standard => TextContrast::DEFAULT,
            Self::Stronger => TextContrast::STRONGER,
            Self::Strongest => TextContrast::STRONGEST,
        }
    }
}

/// The glyph rendering in effect, after the desktop's choice is resolved,
/// registered in app state by `teksilo-app`'s `run`:
/// `ctx.app_state::<EffectiveGlyphRendering>()`. Read it to show what
/// [`GlyphRenderingPreference::FollowDesktop`] resolved to, or bind its
/// [`signal`](Self::signal) to follow a change.
#[derive(Clone, Debug)]
pub struct EffectiveGlyphRendering(Signal<GlyphRendering>);

impl EffectiveGlyphRendering {
    pub(crate) fn new() -> Self {
        Self(Signal::new(GlyphRendering::Natural))
    }

    /// The rendering in effect now.
    pub fn get(&self) -> GlyphRendering {
        self.0.get()
    }

    /// The rendering in effect, as a signal the window manager sets when it
    /// changes. Writing it changes nothing on screen.
    pub fn signal(&self) -> Signal<GlyphRendering> {
        self.0.clone()
    }

    pub(crate) fn set(&self, rendering: GlyphRendering) {
        if self.0.get() != rendering {
            self.0.set(rendering);
        }
    }
}

/// How text is fitted to the pixel grid, in every window.
///
/// Natural rendering keeps the font's spacing exactly and draws each glyph
/// at its position to a quarter of a pixel, hinted vertically only, as
/// browsers and macOS do. Crisp rendering puts glyphs on whole pixels,
/// fitted to the grid on both axes, and lays text out with those whole-pixel
/// widths, as Windows' classic rendering and FreeType's full hinting do:
/// sharper and darker stems at 100 % scale, lines 1 to 5 % wider. Text
/// drawn under a zoom stays natural.
///
/// Stored (see [`GLYPH_RENDERING_KEY`]) as `"follow-desktop"`, `"natural"`,
/// `"crisp"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum GlyphRenderingPreference {
    /// What the desktop asks of every application: crisp where the user
    /// chose medium or full hinting (KDE Plasma's font settings; GNOME's
    /// font hinting, set with GNOME Tweaks), natural elsewhere and on
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
    /// Every preference, the desktop's first: the options a picker offers.
    pub fn all() -> &'static [Self] {
        &[Self::FollowDesktop, Self::Natural, Self::Crisp]
    }

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
    fn levels_name_the_presets() {
        assert_eq!(
            TextContrastLevel::default().contrast(),
            TextContrast::DEFAULT
        );
        assert_eq!(
            TextContrastLevel::Stronger.contrast(),
            TextContrast::STRONGER
        );
        assert_eq!(
            TextContrastLevel::Strongest.contrast(),
            TextContrast::STRONGEST
        );
    }

    #[test]
    fn both_keys_store_kebab_case_names_and_read_them_back() {
        use teksilo_settings::SettingsStore;
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("settings.toml");
        {
            let store =
                SettingsStore::open_with_delay(path.clone(), std::time::Duration::ZERO).unwrap();
            assert_eq!(
                store.signal_for(&TEXT_CONTRAST_KEY).get(),
                TextContrastLevel::Standard
            );
            assert_eq!(
                store.signal_for(&GLYPH_RENDERING_KEY).get(),
                GlyphRenderingPreference::FollowDesktop
            );
            store
                .signal_for(&TEXT_CONTRAST_KEY)
                .set(TextContrastLevel::Strongest);
            store
                .signal_for(&GLYPH_RENDERING_KEY)
                .set(GlyphRenderingPreference::Crisp);
            store.flush_now().unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("text_contrast = \"strongest\""), "{text}");
        assert!(text.contains("glyph_rendering = \"crisp\""), "{text}");
        let store = SettingsStore::open_with_delay(path, std::time::Duration::ZERO).unwrap();
        assert_eq!(
            store.signal_for(&TEXT_CONTRAST_KEY).get(),
            TextContrastLevel::Strongest
        );
        assert_eq!(
            store.signal_for(&GLYPH_RENDERING_KEY).get(),
            GlyphRenderingPreference::Crisp
        );
    }

    #[test]
    fn all_lists_every_variant_once() {
        // A variant added to either enum without being listed here is caught
        // by the matches below, which name every variant.
        for level in TextContrastLevel::all() {
            match level {
                TextContrastLevel::Standard
                | TextContrastLevel::Stronger
                | TextContrastLevel::Strongest => {}
            }
        }
        assert_eq!(TextContrastLevel::all().len(), 3);
        assert_eq!(TextContrastLevel::all()[0], TextContrastLevel::default());
        for preference in GlyphRenderingPreference::all() {
            match preference {
                GlyphRenderingPreference::FollowDesktop
                | GlyphRenderingPreference::Natural
                | GlyphRenderingPreference::Crisp => {}
            }
        }
        assert_eq!(GlyphRenderingPreference::all().len(), 3);
        assert_eq!(
            GlyphRenderingPreference::all()[0],
            GlyphRenderingPreference::default()
        );
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
