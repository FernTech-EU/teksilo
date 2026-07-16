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

/// The bytes of the bundled Noto Sans Arabic variable font — the RTL face used for Arabic,
/// Persian, Urdu and related scripts.
#[cfg(feature = "fonts-arabic")]
pub fn noto_sans_arabic_bytes() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansArabic-VariableFont_wdth,wght.ttf")
}

/// The bytes of the bundled Noto Sans Hebrew variable font.
#[cfg(feature = "fonts-hebrew")]
pub fn noto_sans_hebrew_bytes() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansHebrew-VariableFont_wdth,wght.ttf")
}
