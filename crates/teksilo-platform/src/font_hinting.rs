// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The font hinting the desktop asks applications for.
//!
//! On Linux the user picks a hinting style for every application: KDE
//! Plasma's font settings write `kdeglobals [General] XftHintStyle`, and
//! GNOME keeps `org.gnome.desktop.interface font-hinting`, which applies
//! only while `font-rendering` is `manual` (GNOME 47 and later; under
//! `automatic`, the default, GTK chooses by itself). Windows and macOS have
//! no such setting, so [`query_desktop_font_hinting`] answers `None` there.
//!
//! What a style means for drawing is the caller's to decide: `teksilo-app`
//! draws text crisp (fitted to the pixel grid on both axes) for
//! [`DesktopFontHinting::Medium`] and [`DesktopFontHinting::Full`], which
//! FreeType and cairo render that way too, and natural otherwise.

/// A desktop's font hinting style, in fontconfig's four steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DesktopFontHinting {
    /// No hinting.
    None,
    /// Vertical hinting only: the default of current GNOME and Plasma.
    Slight,
    /// Hinting on both axes, moderate.
    Medium,
    /// Hinting on both axes.
    Full,
}

impl DesktopFontHinting {
    /// Read a style as KDE (`hintfull`) or GNOME (`full`) stores it,
    /// ignoring case and quotes; `None` for anything else.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().trim_matches('\'').trim_matches('"');
        let value = value.to_ascii_lowercase();
        match value.strip_prefix("hint").unwrap_or(&value) {
            "none" => Some(Self::None),
            "slight" => Some(Self::Slight),
            "medium" => Some(Self::Medium),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    /// Whether this style fits glyphs to the pixel grid horizontally as well
    /// as vertically.
    pub fn fits_both_axes(self) -> bool {
        matches!(self, Self::Medium | Self::Full)
    }
}

/// GNOME's two keys, as read: `font-rendering` (absent before GNOME 47) and
/// `font-hinting`. The hinting applies unless rendering is `automatic`.
pub fn gnome_font_hinting(
    font_rendering: Option<&str>,
    font_hinting: Option<&str>,
) -> Option<DesktopFontHinting> {
    let automatic = font_rendering
        .map(|r| {
            r.trim()
                .trim_matches('\'')
                .eq_ignore_ascii_case("automatic")
        })
        .unwrap_or(false);
    if automatic {
        return None;
    }
    font_hinting.and_then(DesktopFontHinting::parse)
}

/// The hinting style the desktop asks for, `None` when it states none (no
/// setting, an automatic choice, a desktop without one, Windows, macOS).
///
/// On Linux this reads files and makes blocking D-Bus calls (the settings
/// portal, bounded at half a second each): call it at startup and when the
/// desktop reports a change, not every frame.
pub fn query_desktop_font_hinting() -> Option<DesktopFontHinting> {
    platform::query()
}

#[cfg(target_os = "linux")]
mod platform {
    use teksilo_kde_scheme::KdeConfigLocations;

    use super::{DesktopFontHinting, gnome_font_hinting};
    use crate::linux_helpers::{Desktop, detect_desktop};
    use crate::linux_portal::read_portal_string;
    use crate::os_theme::kde;

    pub(super) fn query() -> Option<DesktopFontHinting> {
        match detect_desktop() {
            Desktop::Kde => {
                let locations = KdeConfigLocations::from_env(kde::process_env);
                kde::kde_font_hinting(&locations, &kde::process_env, kde::read_file)
                    .as_deref()
                    .and_then(DesktopFontHinting::parse)
            }
            _ => gnome_font_hinting(
                read_portal_string("org.gnome.desktop.interface", "font-rendering").as_deref(),
                read_portal_string("org.gnome.desktop.interface", "font-hinting").as_deref(),
            ),
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    pub(super) fn query() -> Option<super::DesktopFontHinting> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_and_gnome_spellings_read_the_same() {
        assert_eq!(
            DesktopFontHinting::parse("hintfull"),
            Some(DesktopFontHinting::Full)
        );
        assert_eq!(
            DesktopFontHinting::parse("'full'"),
            Some(DesktopFontHinting::Full)
        );
        assert_eq!(
            DesktopFontHinting::parse("hintslight"),
            Some(DesktopFontHinting::Slight)
        );
        assert_eq!(
            DesktopFontHinting::parse("Medium"),
            Some(DesktopFontHinting::Medium)
        );
        assert_eq!(
            DesktopFontHinting::parse("hintnone"),
            Some(DesktopFontHinting::None)
        );
        assert_eq!(DesktopFontHinting::parse("hintdefault"), None);
        assert_eq!(DesktopFontHinting::parse(""), None);
    }

    #[test]
    fn medium_and_full_fit_both_axes() {
        assert!(DesktopFontHinting::Full.fits_both_axes());
        assert!(DesktopFontHinting::Medium.fits_both_axes());
        assert!(!DesktopFontHinting::Slight.fits_both_axes());
        assert!(!DesktopFontHinting::None.fits_both_axes());
    }

    #[test]
    fn gnome_hinting_applies_only_under_manual_rendering() {
        assert_eq!(gnome_font_hinting(Some("automatic"), Some("full")), None);
        assert_eq!(
            gnome_font_hinting(Some("manual"), Some("full")),
            Some(DesktopFontHinting::Full)
        );
        // Before GNOME 47 there is no font-rendering key: hinting applies.
        assert_eq!(
            gnome_font_hinting(None, Some("slight")),
            Some(DesktopFontHinting::Slight)
        );
        assert_eq!(gnome_font_hinting(Some("manual"), None), None);
    }
}
