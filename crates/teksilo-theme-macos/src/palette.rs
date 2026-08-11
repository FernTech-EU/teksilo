// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The AppKit semantic-colour palette, attached to the theme as a typed
//! extension.
//!
//! Teksilo's [`ColorTokens`](teksilo_tokens::ColorTokens) carries one
//! semantic slot per *role*; AppKit carries a larger vocabulary — four
//! grades of label, two independent selection families, a separate
//! control face and control background. [`MacOsPalette`] holds the full
//! set so macOS-aware app code and the macOS widget styles in this crate
//! can read the exact token they need:
//!
//! ```ignore
//! if let Some(m) = theme.extension::<MacOsPalette>() {
//!     let hairline = m.separator;
//! }
//! ```
//!
//! ## Provenance, and why every literal here is a snapshot
//!
//! Apple attaches a standing disclaimer to every colour value it
//! publishes: *"The color values provided below are intended for reference
//! during your app design process. The actual color values will fluctuate
//! from release to release … Always use the API to apply system colors."*
//! There is no `NSColor` API to call from a cross-platform Rust renderer,
//! so this module transcribes the values and says where each came from:
//!
//! - **`[HIG]`** — published on developer.apple.com. The 13-hue system
//!   colour table (four variants each) and the whole typography ramp are
//!   the only fully-published numeric tables Apple ships.
//! - **`[measured]`** — a community capture of the private per-appearance
//!   `NSColor` enumeration. The label / separator / placeholder alphas are
//!   cross-corroborated across several such captures; the window and
//!   control backgrounds are stable across releases; the selection
//!   colours come from a single dated capture and are the weakest values
//!   in the file.
//! - **`[derived]`** — no Apple source exists and the value is computed
//!   from one that does. Each is marked at its assignment with the rule.
//!
//! Two of Apple's own values are deliberately **not** transcribed because
//! they fail WCAG on the surfaces this preset paints; both lifts are
//! called out at the assignment and covered by a test. See
//! [`SECONDARY_LABEL_ALPHA_LIGHT`] and [`MacOsPalette::system_red`].
//!
//! ## Scope: Aqua and Dark Aqua, not Liquid Glass
//!
//! This is the `NSAppearance` model from macOS 11 Big Sur through 15
//! Sequoia — flat controls, two corner radii, `NSVisualEffectView`
//! materials. macOS 26 Tahoe layers *Liquid Glass* on top, with
//! window-style-dependent "concentric" radii and a second material
//! system; Apple publishes no numeric table for it, and nothing here
//! assumes it.

use teksilo_tokens::Color;

/// An opaque `#RRGGBB` literal.
///
/// A thin alias for [`Color::from_hex`] so a reviewer can see at a glance
/// which values are transcribed literals and which are computed.
pub fn hex(s: &str) -> Color {
    Color::from_hex(s)
}

/// Black at `alpha`.
///
/// AppKit publishes most of its semantic *foreground* colours as an
/// alpha over the appearance's base rather than as an opaque literal —
/// `labelColor` is "black at 85 %", not `#262626`. Keeping them in that
/// form is what lets the same token read correctly over a window
/// background, a control face and a popover.
pub fn black_alpha(alpha: f32) -> Color {
    Color::new(0.0, 0.0, 0.0, alpha)
}

/// White at `alpha` — the Dark Aqua counterpart of [`black_alpha`].
pub fn white_alpha(alpha: f32) -> Color {
    Color::new(1.0, 1.0, 1.0, alpha)
}

/// Composite `top` over `bottom` (source-over), returning a colour with
/// `bottom`'s alpha.
///
/// Alpha-over-base is how AppKit expresses most of its palette, and
/// Teksilo paints flat colours over the parent surface, which composites
/// the same way — so alpha is **kept** wherever the wash is the point
/// (hairlines, hover washes, the scrim) and pre-composited wherever a
/// Teksilo slot must be opaque (a window background, an editor surface,
/// an accent fill whose contrast against its label has to be
/// predictable).
pub fn over(top: Color, bottom: Color) -> Color {
    bottom.mix(Color::new(top.r(), top.g(), top.b(), bottom.a()), top.a())
}

/// `secondaryLabelColor`'s published alpha in **Aqua** — black at 50 %.
///
/// Transcribed as Apple states it. It does **not** clear WCAG SC 1.4.3's
/// 4.5:1 floor on the surfaces this preset paints (3.98:1 on
/// `textBackgroundColor`), so the crate's colour projection raises it
/// until it does, and `ColorTokens::text_secondary` carries the lifted
/// value. This field stays at Apple's number because that is what a
/// consumer reading the AppKit palette is asking for; the *token* is what
/// the framework paints.
pub const SECONDARY_LABEL_ALPHA_LIGHT: f32 = 0.50;

/// `secondaryLabelColor`'s published alpha in **Dark Aqua** — white at
/// 55 %. Clears the floor on every plain surface, and not on the lightest
/// *washed* one; see [`SECONDARY_LABEL_ALPHA_LIGHT`].
pub const SECONDARY_LABEL_ALPHA_DARK: f32 = 0.55;

// ── Accent ──────────────────────────────────────────────────────────────

/// The eight accent colours offered in **System Settings › Appearance ›
/// Accent colour**.
///
/// Apple names the swatches but publishes no hex values for them; the
/// literals below are a community capture of the private `NSColor`
/// enumeration `[measured, ~2018]` and are the least-verified numbers in
/// this crate. `Blue` is the out-of-box default and is the one value
/// corroborated by the published system-colour table (it is `systemBlue`).
///
/// Apple's ninth option, *Multicolour*, is not an accent: it means "let
/// each app use its own", which a single-theme preset cannot express.
///
/// ```ignore
/// let theme = teksilo_theme_macos::light_with_accent(SystemAccent::Purple.color(ThemeAppearance::Light));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemAccent {
    /// The out-of-box default — `systemBlue`.
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Graphite,
}

impl SystemAccent {
    /// Every swatch, in the order System Settings shows them.
    pub const ALL: [SystemAccent; 8] = [
        SystemAccent::Blue,
        SystemAccent::Purple,
        SystemAccent::Pink,
        SystemAccent::Red,
        SystemAccent::Orange,
        SystemAccent::Yellow,
        SystemAccent::Green,
        SystemAccent::Graphite,
    ];

    /// This swatch's Aqua (light) value.
    pub fn light(self) -> Color {
        match self {
            // `systemBlue` — the one swatch corroborated by the published
            // system-colour table.
            SystemAccent::Blue => hex("#007AFF"),
            SystemAccent::Purple => hex("#953D96"),
            SystemAccent::Pink => hex("#F74F9E"),
            SystemAccent::Red => hex("#E0383E"),
            SystemAccent::Orange => hex("#F7821B"),
            SystemAccent::Yellow => hex("#FCB827"),
            SystemAccent::Green => hex("#62BA46"),
            SystemAccent::Graphite => hex("#989898"),
        }
    }

    /// This swatch's Dark Aqua value. Only Blue, Red and Graphite differ
    /// from their light counterparts in the capture.
    pub fn dark(self) -> Color {
        match self {
            SystemAccent::Blue => hex("#0A84FF"),
            SystemAccent::Red => hex("#F23D43"),
            SystemAccent::Graphite => hex("#8C8C8C"),
            other => other.light(),
        }
    }

    /// A stable lowercase identifier — useful for persisting a user's
    /// choice.
    pub fn as_str(self) -> &'static str {
        match self {
            SystemAccent::Blue => "blue",
            SystemAccent::Purple => "purple",
            SystemAccent::Pink => "pink",
            SystemAccent::Red => "red",
            SystemAccent::Orange => "orange",
            SystemAccent::Yellow => "yellow",
            SystemAccent::Green => "green",
            SystemAccent::Graphite => "graphite",
        }
    }
}

/// The shades AppKit derives from `controlAccentColor`.
///
/// Unlike Windows, macOS publishes no accent *ramp* — it derives what it
/// needs on the fly. Three derived values matter to a widget toolkit:
///
/// - **`fill`** — what an accent-filled control actually paints. AppKit's
///   `selectedContentBackgroundColor` is a *darkened* accent, not the
///   accent itself, which is what lets `alternateSelectedControlTextColor`
///   (white) clear contrast on it. The measured value for the stock blue
///   is `#0063E1`; white on the raw `#007AFF` is only 4.02:1, below WCAG
///   1.4.3, so painting the raw accent would ship an inaccessible default
///   button. See
///   [`MacOsAccentRamp::system_blue_light`].
/// - **`hover` / `pressed`** — Aqua darkens under the pointer, Dark Aqua
///   brightens. macOS push buttons famously do **not** react to hover at
///   all; the 5 % step here is the smallest that still confirms the
///   pointer is over a live control, which a cross-platform toolkit needs
///   more than the last 5 % of fidelity. The press step is a full 15 %,
///   as on macOS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacOsAccentRamp {
    /// `controlAccentColor` — the identity colour. Focus rings, tints and
    /// the `accent_subtle_bg` wash use this, unmodified.
    pub base: Color,
    /// `selectedContentBackgroundColor` — the fill of an accent-filled
    /// control or an emphasised selection.
    pub fill: Color,
    /// `fill` under the pointer.
    pub hover: Color,
    /// `fill` while pressed.
    pub pressed: Color,
}

impl MacOsAccentRamp {
    /// The measured ramp for the stock blue accent in **Aqua**.
    pub fn system_blue_light() -> Self {
        Self {
            base: SystemAccent::Blue.light(),
            // `selectedContentBackgroundColor` [measured].
            fill: hex("#0063E1"),
            hover: hex("#0063E1").darken(0.05),
            pressed: hex("#0063E1").darken(0.15),
        }
    }

    /// The measured ramp for the stock blue accent in **Dark Aqua**.
    pub fn system_blue_dark() -> Self {
        Self {
            base: SystemAccent::Blue.dark(),
            // `selectedContentBackgroundColor` [measured].
            fill: hex("#0058D0"),
            // Dark Aqua brightens under interaction rather than darkening.
            hover: hex("#0058D0").lighten(0.05),
            pressed: hex("#0058D0").lighten(0.15),
        }
    }

    /// Derive the ramp for an arbitrary accent in **Aqua**.
    ///
    /// `fill` is `base` darkened 15 %, which reproduces the measured
    /// `selectedContentBackgroundColor` for the stock blue to within
    /// ~3 % per channel (see the test) and, more importantly, reproduces
    /// its *purpose*: a fill dark enough for white text.
    ///
    /// A light accent (Yellow, Graphite) cannot reach 4.5:1 against white
    /// by 15 % alone, so the fill is darkened further until it does —
    /// which is what keeps `light_with_accent(SystemAccent::Yellow…)`
    /// legible instead of merely authentic-looking.
    pub fn light_from_base(base: Color) -> Self {
        let fill = darken_until_white_text_is_legible(base.darken(0.15));
        Self {
            base,
            fill,
            hover: fill.darken(0.05),
            pressed: fill.darken(0.15),
        }
    }

    /// Derive the ramp for an arbitrary accent in **Dark Aqua**.
    pub fn dark_from_base(base: Color) -> Self {
        let fill = darken_until_white_text_is_legible(base.darken(0.18));
        Self {
            base,
            fill,
            hover: fill.lighten(0.05),
            pressed: fill.lighten(0.15),
        }
    }
}

/// Darken `fill` in 3 % steps until white text on it clears WCAG SC
/// 1.4.3's 4.5:1 floor, giving up after 24 steps (a hue that cannot get
/// there is already black).
///
/// Needed because a *user-chosen* accent is arbitrary: Apple's own Yellow
/// swatch `#FCB827` is far too light to carry white text, and macOS
/// sidesteps that by flipping the on-accent label to black for light
/// accents. Teksilo resolves `TextRole::OnAccent` from a single token, so
/// the fill has to move instead of the label. The alternative — leaving
/// the fill alone — ships a control whose label is unreadable.
fn darken_until_white_text_is_legible(fill: Color) -> Color {
    const FLOOR: f32 = 4.5;
    let mut out = fill;
    for _ in 0..24 {
        if out.contrast_ratio(Color::WHITE) >= FLOOR {
            return out;
        }
        out = out.darken(0.03);
    }
    out
}

// ── Palette ─────────────────────────────────────────────────────────────

/// The face of a macOS bezelled control — push button, popup button,
/// stepper, the knob of a switch or slider.
///
/// The single most recognisable thing about a macOS control is that it
/// looks like a *physical object*: a very slightly graded face, a
/// hairline that is not the same weight all the way round, and a shadow
/// that separates it from the surface it sits on. Big Sur flattened the
/// Aqua gloss but kept all three.
///
/// Apple publishes none of these values. The gradient is a two-stop
/// vertical ramp measured off a screenshot; the shadow is the softest
/// offset/blur pair that still separates the control at 1× and 2×.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacOsBezel {
    /// Top of the face gradient.
    pub face_top: Color,
    /// Bottom of the face gradient.
    pub face_bottom: Color,
    /// The hairline around the whole outline.
    pub stroke: Color,
    /// A catch-light along the top inside edge. Transparent in Aqua (a
    /// near-white face has nothing to catch); a real highlight in Dark
    /// Aqua, where it is what keeps a grey button from reading as a hole.
    pub inner_light: Color,
    /// The control's drop shadow.
    pub shadow: Color,
}

/// The AppKit semantic palette for one appearance.
///
/// Installed on the theme via `theme.extensions.insert(...)` and read back
/// with `theme.extension::<MacOsPalette>()`. Field names mirror the
/// `NSColor` class properties with the `Color` suffix dropped and
/// `snake_case` applied (`unemphasizedSelectedContentBackgroundColor` →
/// `unemphasized_selected_content_background`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacOsPalette {
    // ── Accent ──
    /// The ramp this palette was built from.
    pub accent_ramp: MacOsAccentRamp,

    // ── Backgrounds ──
    /// `windowBackgroundColor` — the chrome behind a window's content.
    pub window_background: Color,
    /// `underPageBackgroundColor` — the surface a document page sits on.
    /// `[derived]`: Apple publishes no value; one step below the window.
    pub under_page_background: Color,
    /// `controlBackgroundColor` — the surface a control group sits on.
    pub control_background: Color,
    /// `textBackgroundColor` — the surface of an editable text area.
    pub text_background: Color,
    /// The face of a menu, popover or other floating panel. `[derived]`
    /// from the opaque fallback of the `.menu` / `.popover`
    /// `NSVisualEffectView` materials — see the crate docs on vibrancy.
    pub panel_background: Color,

    // ── Labels ──
    /// `labelColor` — primary text.
    pub label: Color,
    /// `secondaryLabelColor`, at Apple's published alpha. The
    /// *projection* lifts it to clear WCAG — see
    /// [`SECONDARY_LABEL_ALPHA_LIGHT`].
    pub secondary_label: Color,
    /// `tertiaryLabelColor`.
    pub tertiary_label: Color,
    /// `quaternaryLabelColor`.
    pub quaternary_label: Color,
    /// `disabledControlTextColor`.
    pub disabled_control_text: Color,
    /// `placeholderTextColor`.
    pub placeholder_text: Color,
    /// `alternateSelectedControlTextColor` / `selectedMenuItemTextColor` —
    /// the label of an accent-filled row or control. White in both
    /// appearances.
    pub selected_control_text: Color,

    // ── Selection ──
    /// `unemphasizedSelectedContentBackgroundColor` — the neutral capsule
    /// a selected row wears when its view does not hold focus.
    pub unemphasized_selected_content_background: Color,

    // ── Separators ──
    /// `separatorColor` / `gridColor` — the universal hairline.
    pub separator: Color,

    // ── Control chrome ──
    /// The bezel of a push button / popup button / knob.
    pub bezel: MacOsBezel,
    /// The recessed track of a switch, slider or scroll bar while off.
    pub control_track: Color,
    /// `controlColor` for a disabled control.
    pub disabled_control_face: Color,

    // ── Links ──
    /// `linkColor`. **Does not track the accent** — AppKit keeps it a
    /// fixed blue whatever the user picks, so a purple-accented Mac still
    /// has blue links.
    pub link: Color,

    // ── System colours (the *Accessible* variant) ──
    /// `systemRed`, Accessible variant.
    pub system_red: Color,
    /// `systemOrange`, Accessible variant.
    pub system_orange: Color,
    /// `systemYellow`, Accessible variant.
    pub system_yellow: Color,
    /// `systemGreen`, Accessible variant.
    pub system_green: Color,
    /// `systemBlue`, Accessible variant.
    pub system_blue: Color,
    /// `systemPurple`, Accessible variant. AppKit has no visited-link
    /// colour; this is the macOS hue closest to the one every browser and
    /// document viewer uses for one.
    pub system_purple: Color,

    // ── Misc ──
    /// `findHighlightColor` — the find-bar match wash. Pure yellow in
    /// both appearances, which is why the dark theme pairs it with black
    /// text.
    pub find_highlight: Color,
    /// The dimming behind a sheet or modal.
    pub sheet_scrim: Color,
}

impl MacOsPalette {
    /// **Aqua** — the light appearance, on the stock blue accent.
    pub fn light() -> Self {
        Self::light_with_accent(MacOsAccentRamp::system_blue_light())
    }

    /// **Dark Aqua** — the dark appearance, on the stock blue accent.
    pub fn dark() -> Self {
        Self::dark_with_accent(MacOsAccentRamp::system_blue_dark())
    }

    /// Aqua with a caller-supplied accent ramp substituted for the
    /// `controlAccentColor` bindings — the substitution macOS performs
    /// when the user picks an accent colour.
    pub fn light_with_accent(ramp: MacOsAccentRamp) -> Self {
        Self {
            accent_ramp: ramp,

            window_background: hex("#ECECEC"),
            // [derived] one step below the window, as the name implies.
            under_page_background: hex("#E1E1E1"),
            control_background: hex("#FFFFFF"),
            text_background: hex("#FFFFFF"),
            // [derived] the opaque fallback of the `.menu` material.
            panel_background: hex("#F6F6F6"),

            label: black_alpha(0.85),
            secondary_label: black_alpha(SECONDARY_LABEL_ALPHA_LIGHT),
            tertiary_label: black_alpha(0.25),
            quaternary_label: black_alpha(0.10),
            disabled_control_text: black_alpha(0.25),
            placeholder_text: black_alpha(0.25),
            selected_control_text: Color::WHITE,

            unemphasized_selected_content_background: hex("#DCDCDC"),

            separator: black_alpha(0.10),

            bezel: MacOsBezel {
                face_top: hex("#FFFFFF"),
                face_bottom: hex("#F4F4F4"),
                stroke: black_alpha(0.14),
                // A near-white face has no catch-light to give.
                inner_light: Color::TRANSPARENT,
                shadow: black_alpha(0.12),
            },
            // `controlAltFill`-equivalent: the off track of a switch.
            control_track: black_alpha(0.16),
            disabled_control_face: hex("#F5F5F5"),

            link: hex("#0068DA"),

            // The *Accessible* variant of each hue — see the type doc.
            system_red: hex("#D70015"),
            system_orange: hex("#C93400"),
            system_yellow: hex("#A05A00"),
            system_green: hex("#007D1B"),
            system_blue: hex("#0040DD"),
            system_purple: hex("#AD44AB"),

            find_highlight: hex("#FFFF00"),
            sheet_scrim: black_alpha(0.28),
        }
    }

    /// Dark Aqua with a caller-supplied accent ramp substituted.
    pub fn dark_with_accent(ramp: MacOsAccentRamp) -> Self {
        Self {
            accent_ramp: ramp,

            window_background: hex("#323232"),
            under_page_background: hex("#282828"),
            control_background: hex("#1E1E1E"),
            text_background: hex("#1E1E1E"),
            // [derived] the opaque fallback of the `.popover` / `.menu`
            // materials. Deliberately *lighter* than the window rather
            // than darker: Teksilo's surface ladder is monotonic, and a
            // floating panel that recedes reads as a hole. Real Dark Aqua
            // menus are vibrant and can land either side of the window
            // depending on what is behind them — an effect no flat-fill
            // renderer reproduces (see the crate docs on vibrancy).
            panel_background: hex("#3A3A3A"),

            label: white_alpha(0.85),
            secondary_label: white_alpha(SECONDARY_LABEL_ALPHA_DARK),
            tertiary_label: white_alpha(0.25),
            quaternary_label: white_alpha(0.10),
            disabled_control_text: white_alpha(0.25),
            placeholder_text: white_alpha(0.50),
            selected_control_text: Color::WHITE,

            unemphasized_selected_content_background: hex("#464646"),

            separator: white_alpha(0.10),

            bezel: MacOsBezel {
                // `controlColor` in Dark Aqua is white at ~25 % over the
                // window background, i.e. ≈ `#656565`; the two stops
                // bracket it.
                face_top: hex("#6B6B6B"),
                face_bottom: hex("#5C5C5C"),
                stroke: black_alpha(0.28),
                // What keeps a grey button from reading as a hole.
                inner_light: white_alpha(0.09),
                shadow: black_alpha(0.36),
            },
            control_track: white_alpha(0.14),
            disabled_control_face: hex("#3A3A3A"),

            link: hex("#419CFF"),

            system_red: hex("#FF6961"),
            system_orange: hex("#FFB340"),
            system_yellow: hex("#FFD426"),
            system_green: hex("#31DE4B"),
            system_blue: hex("#409CFF"),
            system_purple: hex("#DA8FFF"),

            find_highlight: hex("#FFFF00"),
            sheet_scrim: black_alpha(0.45),
        }
    }

    /// `labelColor` composited onto the window background — the opaque
    /// colour primary text actually reaches the screen as.
    pub fn label_on_window(&self) -> Color {
        over(self.label, self.window_background)
    }

    /// The opaque surface a bezelled control's face averages to. Used
    /// where a flat value is needed (a `RectWidget` background, a token
    /// slot) rather than the two-stop gradient the chrome paints.
    pub fn bezel_face_solid(&self) -> Color {
        self.bezel.face_top.mix(self.bezel.face_bottom, 0.5)
    }

    /// The accent wash behind a subtly-tinted surface (a badge, an info
    /// background). `[derived]`: AppKit has no such token — the nearest,
    /// `selectedContentBackgroundColor`, is a fully-saturated fill. A
    /// low-alpha `controlAccentColor` over the window is the smallest
    /// honest substitute.
    pub fn accent_subtle(&self, alpha: f32) -> Color {
        over(
            self.accent_ramp.base.with_alpha(alpha),
            self.window_background,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_helpers_carry_their_channel() {
        let b = black_alpha(0.85);
        assert_eq!(b.r(), 0.0);
        assert!((b.a() - 0.85).abs() < 1e-6);
        let w = white_alpha(0.55);
        assert_eq!(w.r(), 1.0);
        assert!((w.a() - 0.55).abs() < 1e-6);
    }

    #[test]
    fn over_composites_source_over_and_returns_opaque() {
        // 50 % black over white is mid-grey and fully opaque.
        let c = over(black_alpha(0.5), Color::WHITE);
        assert!((c.r() - 0.5).abs() < 1e-6);
        assert!((c.a() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn blue_is_the_default_accent_in_both_appearances() {
        assert_eq!(
            MacOsAccentRamp::system_blue_light().base,
            SystemAccent::Blue.light()
        );
        assert_eq!(
            MacOsAccentRamp::system_blue_dark().base,
            SystemAccent::Blue.dark()
        );
    }

    #[test]
    fn only_three_swatches_differ_between_appearances() {
        // The capture shows Blue, Red and Graphite shifting; the rest are
        // shared. A swatch quietly gaining a dark value would be a
        // transcription slip.
        let differing: Vec<_> = SystemAccent::ALL
            .iter()
            .filter(|a| a.light() != a.dark())
            .copied()
            .collect();
        assert_eq!(
            differing,
            vec![
                SystemAccent::Blue,
                SystemAccent::Red,
                SystemAccent::Graphite
            ]
        );
    }

    #[test]
    fn swatch_identifiers_are_unique() {
        let mut ids: Vec<_> = SystemAccent::ALL.iter().map(|a| a.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    /// The whole reason `fill` exists rather than painting `base`.
    #[test]
    fn white_on_the_raw_accent_would_fail_but_the_fill_passes() {
        let raw = SystemAccent::Blue.light();
        assert!(
            raw.contrast_ratio(Color::WHITE) < 4.5,
            "the premise of the fill shade has changed"
        );
        for ramp in [
            MacOsAccentRamp::system_blue_light(),
            MacOsAccentRamp::system_blue_dark(),
        ] {
            assert!(
                ramp.fill.contrast_ratio(Color::WHITE) >= 4.5,
                "an accent-filled control's white label must clear 4.5:1"
            );
        }
    }

    #[test]
    fn the_derived_fill_reproduces_the_measured_selection_colour() {
        // `light_from_base` must land on the measured
        // `selectedContentBackgroundColor` for the stock accent, or the
        // derivation is not modelling the same thing the measurement did.
        let derived = MacOsAccentRamp::light_from_base(SystemAccent::Blue.light()).fill;
        let measured = MacOsAccentRamp::system_blue_light().fill;
        for (d, m) in [
            (derived.r(), measured.r()),
            (derived.g(), measured.g()),
            (derived.b(), measured.b()),
        ] {
            assert!(
                (d - m).abs() < 0.05,
                "derived fill {derived:?} strays from the measured {measured:?}"
            );
        }
    }

    #[test]
    fn every_swatch_yields_a_legible_fill_in_both_appearances() {
        // Yellow and Graphite are the ones that need the extra darkening;
        // if the loop stops running they will ship unreadable labels.
        for accent in SystemAccent::ALL {
            for fill in [
                MacOsAccentRamp::light_from_base(accent.light()).fill,
                MacOsAccentRamp::dark_from_base(accent.dark()).fill,
            ] {
                assert!(
                    fill.contrast_ratio(Color::WHITE) >= 4.5,
                    "{accent:?}: white on {fill:?} is only {:.2}:1",
                    fill.contrast_ratio(Color::WHITE)
                );
            }
        }
    }

    #[test]
    fn a_light_swatch_is_darkened_further_than_the_flat_fifteen_percent() {
        // Yellow cannot carry white text at 15 %; the loop must bite.
        let flat = SystemAccent::Yellow.light().darken(0.15);
        let ramp = MacOsAccentRamp::light_from_base(SystemAccent::Yellow.light());
        assert!(flat.contrast_ratio(Color::WHITE) < 4.5);
        assert!(ramp.fill.relative_luminance() < flat.relative_luminance());
    }

    #[test]
    fn interaction_steps_move_the_right_way_per_appearance() {
        // Aqua darkens under the pointer, Dark Aqua brightens.
        let l = MacOsAccentRamp::system_blue_light();
        assert!(l.hover.relative_luminance() < l.fill.relative_luminance());
        assert!(l.pressed.relative_luminance() < l.hover.relative_luminance());

        let d = MacOsAccentRamp::system_blue_dark();
        assert!(d.hover.relative_luminance() > d.fill.relative_luminance());
        assert!(d.pressed.relative_luminance() > d.hover.relative_luminance());
    }

    #[test]
    fn a_custom_accent_is_substituted_throughout_the_palette() {
        let ramp = MacOsAccentRamp::light_from_base(SystemAccent::Purple.light());
        let p = MacOsPalette::light_with_accent(ramp);
        assert_eq!(p.accent_ramp.base, SystemAccent::Purple.light());
        assert_ne!(p.accent_ramp.fill, MacOsPalette::light().accent_ramp.fill);
        // …but the neutral half of the palette does not move.
        assert_eq!(p.window_background, MacOsPalette::light().window_background);
        assert_eq!(p.separator, MacOsPalette::light().separator);
        // …and neither does `linkColor`, which AppKit does not tie to the
        // accent.
        assert_eq!(p.link, MacOsPalette::light().link);
    }

    #[test]
    fn the_label_ladder_is_monotonic_in_both_appearances() {
        for p in [MacOsPalette::light(), MacOsPalette::dark()] {
            assert!(p.label.a() > p.secondary_label.a());
            assert!(p.secondary_label.a() > p.tertiary_label.a());
            assert!(p.tertiary_label.a() > p.quaternary_label.a());
        }
    }

    #[test]
    fn the_label_grades_carry_apples_published_alphas() {
        // This struct transcribes; the *projection* is where anything
        // gets lifted to clear WCAG. Keeping Apple's number here is what
        // makes the palette useful to a consumer reading the AppKit
        // vocabulary rather than the framework's tokens.
        let l = MacOsPalette::light();
        assert!((l.secondary_label.a() - SECONDARY_LABEL_ALPHA_LIGHT).abs() < 1e-6);
        assert!((l.label.a() - 0.85).abs() < 1e-6);
        let d = MacOsPalette::dark();
        assert!((d.secondary_label.a() - SECONDARY_LABEL_ALPHA_DARK).abs() < 1e-6);
    }

    #[test]
    fn apples_published_secondary_label_does_not_clear_the_wcag_floor() {
        // Pins the premise of the projection's lift: if AppKit's own value
        // started passing, the lift should be reverted rather than kept.
        let p = MacOsPalette::light();
        let apple = over(p.secondary_label, p.control_background);
        assert!(apple.contrast_ratio(p.control_background) < 4.5);
    }

    #[test]
    fn the_bezel_gradient_runs_top_to_bottom_in_both_appearances() {
        // A macOS control face is always lighter at the top; inverting it
        // reads as a pressed control that is not pressed.
        for p in [MacOsPalette::light(), MacOsPalette::dark()] {
            assert!(
                p.bezel.face_top.relative_luminance() > p.bezel.face_bottom.relative_luminance()
            );
            assert!((p.bezel_face_solid().a() - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn only_dark_aqua_carries_a_catch_light() {
        assert_eq!(MacOsPalette::light().bezel.inner_light.a(), 0.0);
        assert!(MacOsPalette::dark().bezel.inner_light.a() > 0.0);
    }

    #[test]
    fn appearances_are_actually_different() {
        let l = MacOsPalette::light();
        let d = MacOsPalette::dark();
        assert!(l.window_background.relative_luminance() > 0.5);
        assert!(d.window_background.relative_luminance() < 0.5);
        assert_ne!(l.label, d.label);
        assert_ne!(l.link, d.link);
    }
}
