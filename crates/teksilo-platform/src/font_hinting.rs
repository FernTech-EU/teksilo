// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The font hinting the desktop asks applications for.
//!
//! On Linux the user picks a hinting style for every application, and where
//! it is kept depends on the desktop. GNOME keeps
//! `org.gnome.desktop.interface font-hinting`, which GTK follows only while
//! `font-rendering` is `manual` (GNOME 47 and later; under `automatic`, the
//! default, GTK chooses by itself); GNOME 50's Settings offers no control for
//! either, GNOME Tweaks does. Everywhere else the answer is fontconfig's, as
//! Qt, GTK 3 and Chromium read it: KDE Plasma's font settings write the
//! user's `fonts.conf` (`kdeglobals [General] XftHintStyle` lags one Apply
//! behind it, and the portal mirrors that stale value, so neither is read).
//! Windows and macOS have no such setting, so [`query_desktop_font_hinting`]
//! answers `None` there.
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

/// fontconfig's answer, as `fc-match -f "%{hinting}|%{hintstyle}"` prints
/// it (`True|1`): its hint style, [`DesktopFontHinting::None`] when hinting
/// is off, `None` when the output is not that.
pub fn fontconfig_font_hinting(output: &str) -> Option<DesktopFontHinting> {
    let (hinting, style) = output.trim().split_once('|')?;
    if hinting.trim().eq_ignore_ascii_case("false") {
        return Some(DesktopFontHinting::None);
    }
    match style.trim() {
        "0" => Some(DesktopFontHinting::None),
        "1" => Some(DesktopFontHinting::Slight),
        "2" => Some(DesktopFontHinting::Medium),
        "3" => Some(DesktopFontHinting::Full),
        _ => None,
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
/// On Linux this makes blocking D-Bus calls (the settings portal, bounded at
/// half a second each) on GNOME and runs `fc-match` elsewhere: call it at
/// startup and when the desktop reports a change, not every frame.
pub fn query_desktop_font_hinting() -> Option<DesktopFontHinting> {
    platform::query()
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{DesktopFontHinting, fontconfig_font_hinting, gnome_font_hinting};
    use crate::linux_helpers::{Desktop, detect_desktop};
    use crate::linux_portal::read_portal_string;

    pub(super) fn query() -> Option<DesktopFontHinting> {
        match detect_desktop() {
            Desktop::Gnome => gnome_font_hinting(
                read_portal_string("org.gnome.desktop.interface", "font-rendering").as_deref(),
                read_portal_string("org.gnome.desktop.interface", "font-hinting").as_deref(),
            ),
            _ => fontconfig(),
        }
    }

    /// fontconfig's hint style for the default sans-serif face, which it
    /// reads from the user's `fonts.conf` and the system's on each run.
    fn fontconfig() -> Option<DesktopFontHinting> {
        let output = std::process::Command::new("fc-match")
            .args(["-f", "%{hinting}|%{hintstyle}", "sans-serif"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        fontconfig_font_hinting(&String::from_utf8_lossy(&output.stdout))
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
    fn fontconfig_output_reads_as_its_hint_style() {
        assert_eq!(
            fontconfig_font_hinting("True|1"),
            Some(DesktopFontHinting::Slight)
        );
        assert_eq!(
            fontconfig_font_hinting("True|3\n"),
            Some(DesktopFontHinting::Full)
        );
        assert_eq!(
            fontconfig_font_hinting("True|2"),
            Some(DesktopFontHinting::Medium)
        );
        assert_eq!(
            fontconfig_font_hinting("False|3"),
            Some(DesktopFontHinting::None),
            "hinting off wins over the style"
        );
        assert_eq!(fontconfig_font_hinting("True|"), None);
        assert_eq!(fontconfig_font_hinting(""), None);
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
