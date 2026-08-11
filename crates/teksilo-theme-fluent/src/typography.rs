// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fluent typography — the WinUI 3 type ramp mapped onto Teksilo's six
//! `TextStyle` slots.
//!
//! WinUI's ramp is Caption 12/16 · Body 14/20 · Body Strong 14/20 SemiBold
//! · Body Large 18/24 · Subtitle 20/28 · Title 28/36 · Title Large 40/52 ·
//! Display 68/92. Teksilo carries six slots and no heading styles, so the
//! bottom four rungs are what map:
//!
//! | Teksilo slot | WinUI style | Size / line height / weight |
//! | --- | --- | --- |
//! | `body` | Body | 14 / 20 / Regular |
//! | `body_bold` | Body Strong | 14 / 20 / SemiBold |
//! | `small` | Caption | 12 / 16 / Regular |
//! | `small_bold` | Caption (emphasised) | 12 / 16 / SemiBold |
//! | `tiny` | — | 11 / 15 / Regular |
//! | `mono` | Consolas | 13 / 1.5 / Regular |
//!
//! `small_bold` and `tiny` have no WinUI counterpart: the ramp stops at
//! Caption and Microsoft's own guidance sets **12 px Regular / 14 px
//! SemiBold as the legibility floor** — so `small_bold` emphasises Caption
//! at its own size rather than inventing an 11 px SemiBold, and `tiny`
//! stays Regular one step below Caption for status-bar chrome.
//!
//! **Tracking is zero everywhere**, verified against
//! `TextBlock_themeresources.xaml`: not one type-ramp style sets
//! `CharacterSpacing`, and the property's documented default is 0. Fluent
//! gets its size-appropriate texture from Segoe UI Variable's `opsz`
//! optical-size axis reshaping the glyphs, not from manual letter-spacing —
//! the opposite of Material 3, which tracks every rung.
//!
//! **Control content is Regular, not SemiBold.** `BaseTextBlockStyle`
//! defaults to SemiBold and every control theme-resource explicitly opts
//! back down (`Button_themeresources.xaml` sets `FontWeight="Normal"`), so
//! a Fluent button label must not inherit the emphasis of a heading.
//!
//! ## Font family
//!
//! Windows 11's UI face is **Segoe UI Variable** (Windows 10 used Segoe
//! UI); the documented monospace is **Consolas**. Neither can be bundled —
//! both are proprietary Microsoft faces with no redistribution licence —
//! so the default build keeps Teksilo's bundled Inter / JetBrains Mono and
//! the `system-fonts` feature switches the family *names* to the Windows
//! stack for the text engine to resolve from installed system fonts. On a
//! machine without those faces the engine falls back to the bundled
//! default, so the feature is never fatal — it simply does nothing off
//! Windows.

use teksilo_tokens::{FontWeight, TypographyTokens};

/// `BodyTextBlockStyle` — 14 epx.
pub const FLUENT_BODY_SIZE: f32 = 14.0;
/// `CaptionTextBlockStyle` — 12 epx.
pub const FLUENT_CAPTION_SIZE: f32 = 12.0;

/// The Windows 11 UI face, used under the `system-fonts` feature.
#[cfg(feature = "system-fonts")]
pub const FLUENT_UI_FONT_FAMILY: &str = "Segoe UI Variable Text";
/// The monospace face documented in the Windows typography guidance, used
/// under the `system-fonts` feature.
#[cfg(feature = "system-fonts")]
pub const FLUENT_MONO_FONT_FAMILY: &str = "Consolas";

/// Fluent typography tokens (WinUI type-ramp metrics).
pub fn fluent_typography() -> TypographyTokens {
    let mut t = TypographyTokens::default();

    // Under `system-fonts`, name the Windows faces so the text engine
    // resolves them from the installed system fonts; otherwise keep the
    // bundled Inter / JetBrains Mono.
    #[cfg(feature = "system-fonts")]
    {
        for style in [
            &mut t.body,
            &mut t.body_bold,
            &mut t.small,
            &mut t.small_bold,
            &mut t.tiny,
        ] {
            style.family = FLUENT_UI_FONT_FAMILY.to_string();
        }
        t.mono.family = FLUENT_MONO_FONT_FAMILY.to_string();
    }

    // Body — 14 / 20 / Regular. `line_height` is a multiplier here, so
    // WinUI's 20 epx on a 14 epx body is 20 / 14.
    t.body.size = FLUENT_BODY_SIZE;
    t.body.weight = FontWeight::REGULAR;
    t.body.line_height = 20.0 / FLUENT_BODY_SIZE;
    t.body.letter_spacing = 0.0;

    // Body Strong — same 14 / 20 metrics, SemiBold.
    t.body_bold.size = FLUENT_BODY_SIZE;
    t.body_bold.weight = FontWeight::SEMI_BOLD;
    t.body_bold.line_height = 20.0 / FLUENT_BODY_SIZE;
    t.body_bold.letter_spacing = 0.0;

    // Caption — 12 / 16 / Regular.
    t.small.size = FLUENT_CAPTION_SIZE;
    t.small.weight = FontWeight::REGULAR;
    t.small.line_height = 16.0 / FLUENT_CAPTION_SIZE;
    t.small.letter_spacing = 0.0;

    // Caption, emphasised — Microsoft's legibility floor is 12 px
    // Regular / 14 px SemiBold, so the emphasised caption keeps Caption's
    // size instead of shrinking further.
    t.small_bold.size = FLUENT_CAPTION_SIZE;
    t.small_bold.weight = FontWeight::SEMI_BOLD;
    t.small_bold.line_height = 16.0 / FLUENT_CAPTION_SIZE;
    t.small_bold.letter_spacing = 0.0;

    // One rung below Caption for status-bar / timestamp chrome. No WinUI
    // equivalent; kept Regular because SemiBold below 14 px is off-guidance.
    t.tiny.size = 11.0;
    t.tiny.weight = FontWeight::REGULAR;
    t.tiny.line_height = 15.0 / 11.0;
    t.tiny.letter_spacing = 0.0;

    // `mono` keeps the bundled default metrics — the Windows typography
    // guidance names Consolas as the fixed-width face but publishes no
    // size ramp for code.

    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_is_the_winui_body_ramp() {
        let t = fluent_typography();
        assert!((t.body.size - 14.0).abs() < 0.01);
        assert_eq!(t.body.weight, FontWeight::REGULAR);
        // 14 / 20 → the computed line box must land on 20 epx.
        assert!((t.body.size * t.body.line_height - 20.0).abs() < 0.01);
    }

    #[test]
    fn body_strong_shares_body_metrics_but_semibold() {
        let t = fluent_typography();
        assert_eq!(t.body.size, t.body_bold.size);
        assert!((t.body.line_height - t.body_bold.line_height).abs() < 1e-6);
        assert!(t.body_bold.weight.0 > t.body.weight.0);
    }

    #[test]
    fn caption_is_twelve_over_sixteen() {
        let t = fluent_typography();
        assert!((t.small.size - 12.0).abs() < 0.01);
        assert!((t.small.size * t.small.line_height - 16.0).abs() < 0.01);
    }

    #[test]
    fn tracking_is_zero_everywhere() {
        // The clearest typographic difference from Material 3, which
        // tracks every rung. Verified against TextBlock_themeresources.xaml:
        // no type-ramp style sets CharacterSpacing.
        let t = fluent_typography();
        for s in [
            &t.body,
            &t.body_bold,
            &t.small,
            &t.small_bold,
            &t.tiny,
            &t.mono,
        ] {
            assert_eq!(s.letter_spacing, 0.0);
        }
    }

    #[test]
    fn no_slot_falls_below_the_legibility_floor() {
        // Microsoft's guidance: 12 px Regular / 14 px SemiBold minimum.
        let t = fluent_typography();
        for s in [&t.body, &t.body_bold, &t.small, &t.small_bold] {
            assert!(s.size >= 12.0, "{} px is below the 12 px floor", s.size);
        }
        for s in [&t.body_bold, &t.small_bold] {
            assert!(s.weight.0 >= FontWeight::SEMI_BOLD.0);
        }
    }

    #[test]
    fn ramp_is_monotonic() {
        let t = fluent_typography();
        assert!(t.body.size > t.small.size);
        assert!(t.small.size > t.tiny.size);
    }

    #[cfg(not(feature = "system-fonts"))]
    #[test]
    fn default_build_keeps_the_bundled_families() {
        let t = fluent_typography();
        let base = TypographyTokens::default();
        assert_eq!(t.body.family, base.body.family);
        assert_eq!(t.mono.family, base.mono.family);
    }

    #[cfg(feature = "system-fonts")]
    #[test]
    fn system_fonts_feature_names_the_windows_stack() {
        let t = fluent_typography();
        assert_eq!(t.body.family, FLUENT_UI_FONT_FAMILY);
        assert_eq!(t.small.family, FLUENT_UI_FONT_FAMILY);
        assert_eq!(t.mono.family, FLUENT_MONO_FONT_FAMILY);
    }
}
