// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Applying an accent colour to a scheme, as Plasma's colour settings do.
//!
//! [`KdeColorScheme::with_accent`] writes, for one scheme and one accent,
//! every key Plasma writes into `kdeglobals` when the user picks an accent:
//! the seven colour groups and their `[Inactive]` subgroups, `[WM]`,
//! `[KDE] contrast` and `frameContrast`, and the two `[ColorEffects:*]`
//! groups. Colours are computed with [`crate::math`] and written as KDE
//! writes them, so each key reads back to the same 8-bit value; the keys
//! the procedure copies keep the scheme's own text.
//!
//! [`ACCENT_SWATCHES`] are the ten colours Plasma's settings page offers.

use crate::color::KdeColor;
use crate::config::KConfig;
use crate::math::{accent_over, black_or_white_on, darken, lighten, luma, luma_ratio, oklab_tint};
use crate::scheme::{
    ColorRole, ColorSet, ColorState, DEFAULT_CONTRAST, DEFAULT_FRAME_CONTRAST, DEFAULT_TINT_FACTOR,
    KdeColorScheme,
};

/// The ten accent colours Plasma's colour settings page offers, in the
/// order it shows them: pink, red, orange, yellow, green, teal, blue,
/// purple, violet and gray. KDE gives them no names. The first is also the
/// page's custom colour until the user saves one, and the seventh,
/// 61,174,233, is Breeze's own accent.
pub const ACCENT_SWATCHES: [KdeColor; 10] = [
    KdeColor::rgb8(233, 58, 154),
    KdeColor::rgb8(233, 61, 88),
    KdeColor::rgb8(233, 100, 58),
    KdeColor::rgb8(232, 203, 45),
    KdeColor::rgb8(61, 212, 37),
    KdeColor::rgb8(0, 211, 184),
    KdeColor::rgb8(61, 174, 233),
    KdeColor::rgb8(184, 117, 220),
    KdeColor::rgb8(146, 110, 228),
    KdeColor::rgb8(104, 107, 111),
];

/// The contrast against its background that keeps a link the plain
/// accent, in [`luma_ratio`].
const READABLE_LINK_RATIO: f64 = 5.0;

/// The roles that take the accent itself (the link made readable first).
const ACCENT_ROLES: [ColorRole; 4] = [
    ColorRole::ForegroundActive,
    ColorRole::ForegroundLink,
    ColorRole::DecorationFocus,
    ColorRole::DecorationHover,
];

/// The keys of each `[ColorEffects:*]` group, in the order they are written.
const EFFECT_KEYS: [&str; 9] = [
    "Enable",
    "ChangeSelectionColor",
    "IntensityEffect",
    "IntensityAmount",
    "ColorEffect",
    "ColorAmount",
    "Color",
    "ContrastEffect",
    "ContrastAmount",
];

/// The accent as a link colour on `background`.
///
/// The accent itself when its [`luma_ratio`] against `background` is at
/// least 5. Otherwise it is lightened (amount 0.5, chroma factor 1) when it
/// is lighter than the background and darkened by the same amounts when it
/// is not, once, without checking the result again. Breeze's accent
/// 61,174,233 on white becomes 45,127,170.
pub fn readable_link(accent: KdeColor, background: KdeColor) -> KdeColor {
    let (accent_luma, background_luma) = (luma(accent), luma(background));
    if luma_ratio(background_luma, accent_luma) >= READABLE_LINK_RATIO {
        accent
    } else if accent_luma > background_luma {
        lighten(accent, 0.5, 1.0)
    } else {
        darken(accent, 0.5, 1.0)
    }
}

impl KdeColorScheme {
    /// The scheme with `accent` applied the way Plasma applies an accent
    /// colour to it.
    ///
    /// The result is read from the configuration the application writes,
    /// which is also its [`KdeColorScheme::source`]: what KDE writes into
    /// `kdeglobals`, key for key and value for value, with three
    /// differences. `[General] ColorSchemeHash` is not written (KDE writes
    /// a digest of the scheme file's bytes); the scheme's `[General] Name`
    /// and `ColorScheme` are copied, so the result keeps its identity; and,
    /// as a documented Teksilo divergence, a colour the scheme does not set
    /// is not tinted (KDE writes the tint of an undefined colour). Nothing
    /// else of `[General]` is written, so the result has no `TintFactor`,
    /// no accent title bar and no `AccentColor`.
    ///
    /// In outline, with tinting on when the scheme has a `[General]
    /// TintFactor` key (its value, or 0.15 when it does not parse):
    ///
    /// - every colour group takes the accent for `ForegroundActive`,
    ///   `DecorationFocus` and `DecorationHover`, and for `ForegroundLink`
    ///   the accent made readable against the group's own `BackgroundNormal`
    ///   ([`readable_link`]); every other colour is tinted toward the
    ///   accent in OKLab when tinting is on, and copied as written
    ///   otherwise;
    /// - the selection background (normal and alternate) becomes the accent
    ///   at 0.7 over the View background, with black or white text on it,
    ///   and the Button alternate background the accent at 0.7 over the
    ///   Button background;
    /// - an `[Inactive]` subgroup is tinted when tinting is on and copied
    ///   otherwise, the Header one from the Window group; with an accent
    ///   title bar the Header backgrounds and text follow the accent;
    /// - `[WM]` is the scheme's own or the resolved Header colours, then the
    ///   tinted or accented window background when tinting or the accent
    ///   title bar is on;
    /// - `[KDE] contrast` and `frameContrast` are written with their
    ///   defaults (7 and 0.2) when the scheme lacks them, and each
    ///   `[ColorEffects:*]` key is copied, empty when the scheme lacks it
    ///   (an empty `Enable` reads as true, so effects the scheme left
    ///   unset are on).
    ///
    /// A fully transparent accent (all four 8-bit channels 0) is no accent:
    /// the colour groups are copied, except that `[Inactive]` subgroups are
    /// still tinted toward it when tinting is on. Where the procedure needs
    /// a scheme colour the scheme does not set (a group's
    /// `BackgroundNormal`), it uses black.
    ///
    /// **Which configuration is accented.** A scheme read with
    /// [`KdeColorScheme::from_config`] or [`KdeColorScheme::parse`] is
    /// accented from its source configuration, so copied keys keep their
    /// text and a key the file lacks stays absent; fields changed after
    /// reading are not seen. A built-in scheme ([`KdeColorScheme::breeze_light`]
    /// and the others) has no source and is accented as the configuration
    /// [`KdeColorScheme::to_kconfig_text`] writes for it. For the three
    /// stock Breeze schemes that text holds the same keys with the same
    /// values as the stock `.colors` file, as far as the accent
    /// application reads it, so accenting the constant gives exactly what
    /// accenting the file gives.
    ///
    /// **Applying it twice is not the same as applying it once.** The
    /// procedure reads the scheme it is given: on an accented scheme the
    /// Header `[Inactive]` group copies the already accented Window
    /// colours, the selection link is checked against the accented
    /// selection background, and a tinting scheme, whose result has no
    /// tint factor, is not tinted the second time. KDE reads the scheme
    /// file afresh for every accent, so to change the accent, apply the new
    /// one to the original scheme.
    pub fn with_accent(&self, accent: impl Into<KdeColor>) -> KdeColorScheme {
        KdeColorScheme::from_config(&self.accented_config(accent.into()))
    }

    /// The configuration [`KdeColorScheme::with_accent`] writes.
    pub(crate) fn accented_config(&self, accent: KdeColor) -> KConfig {
        let written;
        let source = match self.source() {
            Some(source) => source,
            None => {
                written = KConfig::parse(&self.to_kconfig_text()).0;
                &written
            }
        };
        Application::new(source, accent).run()
    }
}

/// One application of an accent to one scheme configuration.
struct Application<'a> {
    /// The scheme configuration, read on its own.
    source: &'a KConfig,
    /// The same scheme with every default filled in.
    scheme: KdeColorScheme,
    accent: KdeColor,
    /// Whether there is an accent: it is not fully transparent black.
    present: bool,
    /// The tint factor when tinting is on.
    tint: Option<f64>,
    /// The tint factor, 0.15 when the scheme has none.
    factor: f64,
    /// Whether the title bar is accent coloured.
    titlebar: bool,
    out: KConfig,
}

impl<'a> Application<'a> {
    fn new(source: &'a KConfig, accent: KdeColor) -> Application<'a> {
        let scheme = KdeColorScheme::from_config(source);
        Application {
            source,
            present: !accent.is_transparent_black(),
            tint: scheme.tint_factor,
            factor: scheme.tint_factor.unwrap_or(DEFAULT_TINT_FACTOR),
            titlebar: scheme.titlebar_is_accent_colored,
            scheme,
            accent,
            out: KConfig::default(),
        }
    }

    fn run(mut self) -> KConfig {
        // ColorSet::ALL is the order the groups are processed in: View,
        // Window, Button, Selection, Tooltip, Complementary, Header.
        for set in ColorSet::ALL {
            self.color_group(set);
        }
        self.window_manager();
        self.kde();
        self.effects();
        for key in ["Name", "ColorScheme"] {
            self.copy(&["General"], &["General"], key);
        }
        self.out
    }

    /// A colour of the scheme as written, black when the scheme lacks it
    /// or it is not a colour.
    fn scheme_color(&self, group: &[&str], key: &str) -> KdeColor {
        self.source
            .read_color(group, key)
            .unwrap_or(KdeColor::BLACK)
    }

    fn write(&mut self, group: &[&str], key: &str, color: KdeColor) {
        self.out.set(group, key, color.to_kconfig_text());
    }

    /// Copies `key` as text from the scheme's `from` group to the output's
    /// `to` group, when the scheme has it.
    fn copy(&mut self, from: &[&str], to: &[&str], key: &str) {
        if let Some(value) = self.source.get(from, key) {
            self.out.set(to, key, value);
        }
    }

    /// Writes the tint of the scheme's `key` in `group`, when the scheme
    /// has that colour.
    fn tint_key(&mut self, group: &[&str], key: &str, factor: f64) {
        if let Some(color) = self.source.read_color(group, key) {
            self.write(group, key, oklab_tint(color, self.accent, factor));
        }
    }

    fn color_group(&mut self, set: ColorSet) {
        let name = set.group_name();
        let group: &[&str] = &[name];
        let inactive: &[&str] = &[name, "Inactive"];
        if set == ColorSet::Header && !self.source.group_exists(group) {
            return;
        }

        for role in ColorRole::ALL {
            let key = role.key();
            if self.present && ACCENT_ROLES.contains(&role) {
                let color = if role == ColorRole::ForegroundLink {
                    readable_link(self.accent, self.scheme_color(group, "BackgroundNormal"))
                } else {
                    self.accent
                };
                self.write(group, key, color);
            } else if let (true, Some(factor)) = (self.present, self.tint) {
                self.tint_key(group, key, factor);
            } else {
                self.copy(group, group, key);
            }
        }

        if self.present && set == ColorSet::Selection {
            let view = self.scheme_color(&[ColorSet::View.group_name()], "BackgroundNormal");
            let fill = accent_over(self.accent, view);
            let text = black_or_white_on(fill, true);
            self.write(group, "BackgroundNormal", fill);
            self.write(group, "BackgroundAlternate", fill);
            self.write(group, "ForegroundNormal", text);
            self.write(group, "ForegroundInactive", text);
        }
        if self.present && set == ColorSet::Button {
            let button = self.scheme_color(group, "BackgroundNormal");
            self.write(
                group,
                "BackgroundAlternate",
                accent_over(self.accent, button),
            );
        }

        if self.source.group_exists(inactive) {
            for role in ColorRole::ALL {
                let key = role.key();
                if let Some(factor) = self.tint {
                    self.tint_key(inactive, key, factor);
                } else if set == ColorSet::Header {
                    self.copy(&[ColorSet::Window.group_name()], inactive, key);
                } else {
                    self.copy(inactive, inactive, key);
                }
            }
        }

        if self.present && set == ColorSet::Header && self.titlebar {
            let window = self.window_background();
            let accented = accent_over(self.accent, window);
            let tinted = oklab_tint(window, self.accent, self.factor);
            self.write(group, "BackgroundNormal", accented);
            self.write(group, "ForegroundNormal", black_or_white_on(accented, true));
            self.write(inactive, "BackgroundNormal", tinted);
            self.write(
                inactive,
                "ForegroundNormal",
                black_or_white_on(tinted, false),
            );
        }
    }

    fn window_background(&self) -> KdeColor {
        self.scheme_color(&[ColorSet::Window.group_name()], "BackgroundNormal")
    }

    /// `[WM]`: the scheme's own colours, else the Header colours it
    /// resolves to before the accent, then the title-bar override.
    fn window_manager(&mut self) {
        let group: &[&str] = &["WM"];
        let active = self
            .scheme
            .state_colors(ColorState::Active, ColorSet::Header);
        let inactive = self
            .scheme
            .state_colors(ColorState::Inactive, ColorSet::Header);
        for (key, default) in [
            ("activeBackground", active.background_normal),
            ("activeForeground", active.foreground_normal),
            ("inactiveBackground", inactive.background_normal),
            ("inactiveForeground", inactive.foreground_normal),
            ("activeBlend", active.background_normal),
            ("inactiveBlend", inactive.background_normal),
        ] {
            let color = self.source.read_color(group, key).unwrap_or(default);
            self.write(group, key, color);
        }

        if !self.present {
            return;
        }
        let window = self.window_background();
        if let Some(factor) = self.tint {
            let tinted = oklab_tint(window, self.accent, factor);
            if !self.titlebar {
                self.write(group, "activeBackground", tinted);
                self.write(group, "activeForeground", black_or_white_on(tinted, true));
            }
            self.write(group, "inactiveBackground", tinted);
            self.write(
                group,
                "inactiveForeground",
                black_or_white_on(tinted, false),
            );
        }
        if self.titlebar {
            let accented = accent_over(self.accent, window);
            self.write(group, "activeBackground", accented);
            self.write(group, "activeForeground", black_or_white_on(accented, true));
        }
    }

    /// `[KDE]`: `frameContrast` as the scheme writes it (not clamped) or
    /// 0.2, and `contrast` or 7, both written as numbers.
    fn kde(&mut self) {
        let group: &[&str] = &["KDE"];
        let frame = self
            .source
            .read_real(group, "frameContrast", DEFAULT_FRAME_CONTRAST);
        let contrast = self
            .source
            .read_int(group, "contrast", i64::from(DEFAULT_CONTRAST));
        self.out.set(group, "frameContrast", frame.to_string());
        self.out.set(group, "contrast", contrast.to_string());
    }

    /// The effect groups, each key copied as text, empty when the scheme
    /// lacks it.
    fn effects(&mut self) {
        for name in ["ColorEffects:Inactive", "ColorEffects:Disabled"] {
            let group: &[&str] = &[name];
            for key in EFFECT_KEYS {
                let value = self.source.get(group, key).unwrap_or_default();
                self.out.set(group, key, value);
            }
        }
    }
}
