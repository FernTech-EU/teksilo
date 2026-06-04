pub mod shared_typesetter;
pub mod typesetter_bridge;

#[cfg(feature = "system-emoji")]
mod system_emoji;

pub mod font_registrar;
pub mod rich_text_engine;

pub use shared_typesetter::SharedTypesetter;
pub use typesetter_bridge::TypesetterBridge;

pub use font_registrar::{EmbeddedInterRegistrar, FontFaceSpec, FontRegistrar, VecFontRegistrar};
pub use rich_text_engine::{RichTextEngine, WrapMode};

pub use text_typeset::{
    CharacterGeometry, CursorAffinity, CursorDisplay, DecorationKind as TypesetterDecorationKind,
    DecorationRect, FontFaceId, GlyphQuad as TypesetterGlyphQuad, HitRegion, HitTestResult,
    ImageQuad, RenderFrame,
};

pub use text_document;
