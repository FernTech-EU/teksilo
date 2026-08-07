// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 typography tokens.
//!
//! Teksilo's six `TextStyle` slots are mapped onto the M3 type scale. The
//! font *family* is left as Teksilo's bundled Inter (text) / JetBrains
//! Mono (`mono`) — M3's reference Roboto is not bundled and the text
//! engine would silently fall back to Inter for it anyway, and Inter is
//! metrically Roboto-compatible. Only the M3 metrics (size, weight,
//! line-height, and the non-zero letter-spacing M3 specifies) are
//! applied.

use teksilo_tokens::{FontWeight, TypographyTokens};

/// Material 3 typography tokens (Inter family, M3 type-scale metrics).
pub fn m3_typography() -> TypographyTokens {
    let mut t = TypographyTokens::default();

    // Under `bundled-fonts`, switch the (non-mono) text family to the
    // embedded Roboto; otherwise keep the bundled Inter (Roboto-metric-
    // compatible). See `crate::font_registrar`.
    #[cfg(feature = "bundled-fonts")]
    {
        for style in [
            &mut t.body,
            &mut t.body_bold,
            &mut t.small,
            &mut t.small_bold,
            &mut t.tiny,
        ] {
            style.family = "Roboto".to_string();
        }
    }

    // Body Medium — 14 sp / 400 / 0.25 tracking.
    t.body.size = 14.0;
    t.body.weight = FontWeight::REGULAR;
    t.body.line_height = 1.43;
    t.body.letter_spacing = 0.25;

    // Title Small / Label Large — 14 sp / 500 / 0.1 tracking.
    t.body_bold.size = 14.0;
    t.body_bold.weight = FontWeight::MEDIUM;
    t.body_bold.line_height = 1.43;
    t.body_bold.letter_spacing = 0.1;

    // Body Small — 12 sp / 400 / 0.4 tracking.
    t.small.size = 12.0;
    t.small.weight = FontWeight::REGULAR;
    t.small.line_height = 1.33;
    t.small.letter_spacing = 0.4;

    // Label Medium — 12 sp / 500 / 0.5 tracking.
    t.small_bold.size = 12.0;
    t.small_bold.weight = FontWeight::MEDIUM;
    t.small_bold.line_height = 1.33;
    t.small_bold.letter_spacing = 0.5;

    // Label Small — 11 sp / 500 / 0.5 tracking.
    t.tiny.size = 11.0;
    t.tiny.weight = FontWeight::MEDIUM;
    t.tiny.line_height = 1.45;
    t.tiny.letter_spacing = 0.5;

    // `mono` has no M3 equivalent — keep the bundled default.

    t
}
