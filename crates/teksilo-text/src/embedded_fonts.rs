// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Raw bytes of the bundled fallback fonts, for callers that need to **embed** a font somewhere
//! else (rather than register it for on-screen shaping). The motivating consumer is Skribisto's
//! PDF exporter, which hands these blobs to Typst so an exported PDF renders RTL scripts in the
//! same Noto faces the editor shapes with — one source of bytes, no screen/export drift.
//!
//! Each accessor is gated by the same feature that decides whether [`TypesetterBridge`] registers
//! that face (`fonts-arabic`, `fonts-hebrew`), so the bytes exist iff the app actually bundles
//! them.
//!
//! [`TypesetterBridge`]: crate::TypesetterBridge
//!
//! Every bundled face is also held here as one `static`, and every other site in the crate
//! names that `static` instead of writing its own `include_bytes!`. Each `include_bytes!` is a
//! separate allocation, and only link-time optimisation merges identical ones, so a build
//! without LTO (a debug build, or an application with a default release profile) carried each
//! default face once per site. There were three sites, so 4.8 MB of Inter and JetBrains Mono
//! was duplicate.

/// The bytes of the bundled Noto Sans Arabic variable font — the RTL face used for Arabic,
/// Persian, Urdu and related scripts.
#[cfg(feature = "fonts-arabic")]
pub fn noto_sans_arabic_bytes() -> &'static [u8] {
    NOTO_SANS_ARABIC
}

/// The bytes of the bundled Noto Sans Hebrew variable font.
#[cfg(feature = "fonts-hebrew")]
pub fn noto_sans_hebrew_bytes() -> &'static [u8] {
    NOTO_SANS_HEBREW
}

// The default UI and monospace families, always embedded.
pub(crate) static INTER: &[u8] = include_bytes!("../fonts/InterVariable.ttf");
pub(crate) static INTER_ITALIC: &[u8] = include_bytes!("../fonts/InterVariable-Italic.ttf");
pub(crate) static JETBRAINS_MONO: &[u8] = include_bytes!("../fonts/JetBrainsMono.ttf");
pub(crate) static JETBRAINS_MONO_ITALIC: &[u8] =
    include_bytes!("../fonts/JetBrainsMono-Italic.ttf");

// Script fallbacks. Arabic and Hebrew follow their feature, as the public accessors above do
// (both files are committed); the others follow the cfg `build.rs` sets only when the file
// is present, so a missing face stays a build warning.
#[cfg(feature = "fonts-arabic")]
pub(crate) static NOTO_SANS_ARABIC: &[u8] =
    include_bytes!("../fonts/NotoSansArabic-VariableFont_wdth,wght.ttf");
#[cfg(feature = "fonts-hebrew")]
pub(crate) static NOTO_SANS_HEBREW: &[u8] =
    include_bytes!("../fonts/NotoSansHebrew-VariableFont_wdth,wght.ttf");
#[cfg(font_thai)]
pub(crate) static NOTO_SANS_THAI: &[u8] =
    include_bytes!("../fonts/NotoSansThai-VariableFont_wdth,wght.ttf");
#[cfg(font_devanagari)]
pub(crate) static NOTO_SANS_DEVANAGARI: &[u8] =
    include_bytes!("../fonts/NotoSansDevanagari-VariableFont_wdth,wght.ttf");
#[cfg(font_cjk_sc)]
pub(crate) static NOTO_SANS_SC: &[u8] = include_bytes!("../fonts/NotoSansSC-VariableFont_wght.ttf");
#[cfg(font_cjk_jp)]
pub(crate) static NOTO_SANS_JP: &[u8] = include_bytes!("../fonts/NotoSansJP-VariableFont_wght.ttf");
#[cfg(font_cjk_kr)]
pub(crate) static NOTO_SANS_KR: &[u8] = include_bytes!("../fonts/NotoSansKR-VariableFont_wght.ttf");
