//! Font registration abstraction for rich-text widgets.
//!
//! Each `RichTextEditor` owns its own `Typesetter`, but every Typesetter
//! needs the same fonts registered. A `FontRegistrar` lets the host
//! application describe a fixed set of fonts once and replay them into
//! each engine as it is constructed.

use text_typeset::{FontFaceId, Typesetter};

/// A single font face to register with a typesetter. Owned byte buffer so
/// the registrar can outlive any borrow of the source data (e.g. from a
/// file on disk or a memory-mapped archive).
#[derive(Clone)]
pub struct FontFaceSpec {
    /// Raw TTF/OTF/WOFF data.
    pub data: std::sync::Arc<Vec<u8>>,
    /// Whether this face should be the default used for unattributed text.
    /// Exactly one face per registrar should set this to `true`.
    pub is_default: bool,
    /// Default size in logical pixels, used only when `is_default == true`.
    pub default_size_px: f32,
}

impl std::fmt::Debug for FontFaceSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFaceSpec")
            .field("data_len", &self.data.len())
            .field("is_default", &self.is_default)
            .field("default_size_px", &self.default_size_px)
            .finish()
    }
}

/// Registers a pre-described set of fonts into a typesetter. Implementors
/// are typically application-owned (a single registrar per app) and cheap
/// to clone or share across widgets.
pub trait FontRegistrar {
    /// Register every font face into `typesetter`. Called once per
    /// widget at construction time. Implementations may choose to cache
    /// `FontFaceId`s if they register the same fonts repeatedly, but
    /// different `Typesetter` instances do not share face IDs so the
    /// registration must be replayed per engine.
    fn register(&self, typesetter: &mut Typesetter) -> Option<FontFaceId>;
}

/// The default registrar embeds InterVariable (the same font fern-text
/// already bundles) and returns its face id as the default. Used when a
/// `RichTextEditor` is built without an explicit `font_registrar()`.
pub struct EmbeddedInterRegistrar {
    data: &'static [u8],
    default_size_px: f32,
}

impl EmbeddedInterRegistrar {
    pub const fn new() -> Self {
        Self {
            data: include_bytes!("../fonts/InterVariable.ttf"),
            default_size_px: 14.0,
        }
    }

    pub const fn with_size(size_px: f32) -> Self {
        Self {
            data: include_bytes!("../fonts/InterVariable.ttf"),
            default_size_px: size_px,
        }
    }
}

impl Default for EmbeddedInterRegistrar {
    fn default() -> Self {
        Self::new()
    }
}

impl FontRegistrar for EmbeddedInterRegistrar {
    fn register(&self, typesetter: &mut Typesetter) -> Option<FontFaceId> {
        let face = typesetter.register_font(self.data);
        typesetter.set_default_font(face, self.default_size_px);
        Some(face)
    }
}

/// Registrar composed of an arbitrary vector of `FontFaceSpec`. The first
/// entry flagged `is_default` becomes the default face; additional fonts
/// feed text-typeset's fallback loop for scripts Inter does not cover.
pub struct VecFontRegistrar {
    faces: Vec<FontFaceSpec>,
}

impl VecFontRegistrar {
    pub fn new(faces: Vec<FontFaceSpec>) -> Self {
        Self { faces }
    }
}

impl FontRegistrar for VecFontRegistrar {
    fn register(&self, typesetter: &mut Typesetter) -> Option<FontFaceId> {
        let mut default = None;
        for spec in &self.faces {
            let face = typesetter.register_font(&spec.data);
            if spec.is_default && default.is_none() {
                typesetter.set_default_font(face, spec.default_size_px);
                default = Some(face);
            }
        }
        default
    }
}
