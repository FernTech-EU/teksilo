// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub mod embedded_fonts;
pub mod shared_typesetter;
pub mod typesetter_bridge;

#[cfg(feature = "system-emoji")]
mod system_emoji;

pub mod font_registrar;
pub mod rich_text_engine;
pub mod typography_defaults;

#[cfg(feature = "fonts-arabic")]
pub use embedded_fonts::noto_sans_arabic_bytes;
#[cfg(feature = "fonts-hebrew")]
pub use embedded_fonts::noto_sans_hebrew_bytes;
pub use shared_typesetter::SharedTypesetter;
pub use typesetter_bridge::TypesetterBridge;

pub use font_registrar::{EmbeddedInterRegistrar, FontFaceSpec, FontRegistrar, VecFontRegistrar};
pub use rich_text_engine::{RichTextEngine, WrapMode};
pub use typography_defaults::EditorTypographyDefaults;

pub use text_typeset::{
    BlockVisualInfo, CharacterGeometry, CursorAffinity, CursorDisplay,
    DecorationKind as TypesetterDecorationKind, DecorationRect, FontFaceId, FontFamilyInfo,
    GlyphQuad as TypesetterGlyphQuad, HitRegion, HitTestResult, ImageQuad, RelayoutError,
    RenderFrame, TextFontService, WritingSystem, WritingSystemIndexBuilder, WritingSystemSet,
};

pub use text_document;
