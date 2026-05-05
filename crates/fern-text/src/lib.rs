pub mod shared_typesetter;
pub mod typesetter_bridge;

#[cfg(feature = "system-emoji")]
mod system_emoji;

#[cfg(feature = "rich-text")]
pub mod font_registrar;
#[cfg(feature = "rich-text")]
pub mod rich_text_engine;

pub use shared_typesetter::SharedTypesetter;
pub use typesetter_bridge::TypesetterBridge;

#[cfg(feature = "rich-text")]
pub use font_registrar::{EmbeddedInterRegistrar, FontFaceSpec, FontRegistrar, VecFontRegistrar};
#[cfg(feature = "rich-text")]
pub use rich_text_engine::{RichTextEngine, WrapMode};

#[cfg(feature = "rich-text")]
pub use text_typeset::{
    CharacterGeometry, CursorDisplay, DecorationKind as TypesetterDecorationKind, DecorationRect,
    FontFaceId, GlyphQuad as TypesetterGlyphQuad, HitRegion, HitTestResult, ImageQuad, RenderFrame,
};

#[cfg(feature = "rich-text")]
pub use text_document;
