// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Font registration abstraction for rich-text widgets.
//!
//! Every rich-text widget renders against a shared
//! [`TextFontService`], and the host application needs a
//! repeatable way to describe which fonts the service should have
//! loaded. A `FontRegistrar` lets the host describe a fixed set of
//! fonts once and replay them into any service that needs
//! populating.
//!
//! [`TextFontService`]: text_typeset::TextFontService

use text_typeset::{FontFaceId, TextFontService};

/// A single font face to register with a typesetter. Owned byte
/// buffer so the registrar can outlive any borrow of the source
/// data (e.g. from a file on disk or a memory-mapped archive).
#[derive(Clone)]
pub struct FontFaceSpec {
    /// Raw TTF/OTF/WOFF data.
    pub data: std::sync::Arc<Vec<u8>>,
    /// Whether this face should be the default used for
    /// unattributed text. Exactly one face per registrar should
    /// set this to `true`.
    pub is_default: bool,
    /// Default size in logical pixels, used only when
    /// `is_default == true`.
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

/// Registers a pre-described set of fonts into a
/// [`TextFontService`]. Implementors are typically application-owned
/// (a single registrar per app) and cheap to clone or share across
/// widgets.
pub trait FontRegistrar {
    /// Register every font face into `service`. Called once per
    /// widget (or once per app) at the point where the shared
    /// service is built. Implementations may cache their
    /// `FontFaceId` results, but different `TextFontService`
    /// instances do not share face IDs so the registration must be
    /// replayed per service.
    fn register_on_service(&self, service: &mut TextFontService) -> Option<FontFaceId>;
}

/// The default registrar embeds InterVariable (the same font
/// bastyde-text already bundles) and returns its face id as the
/// default. Used when a `RichTextEditor` is built without an
/// explicit `font_registrar()`.
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
    fn register_on_service(&self, service: &mut TextFontService) -> Option<FontFaceId> {
        let face = service.register_font(self.data);
        service.set_default_font(face, self.default_size_px);
        Some(face)
    }
}

/// Registrar composed of an arbitrary vector of `FontFaceSpec`.
/// The first entry flagged `is_default` becomes the default face;
/// additional fonts feed text-typeset's fallback loop for scripts
/// Inter does not cover.
pub struct VecFontRegistrar {
    faces: Vec<FontFaceSpec>,
}

impl VecFontRegistrar {
    pub fn new(faces: Vec<FontFaceSpec>) -> Self {
        Self { faces }
    }
}

impl FontRegistrar for VecFontRegistrar {
    fn register_on_service(&self, service: &mut TextFontService) -> Option<FontFaceId> {
        let mut default = None;
        for spec in &self.faces {
            let face = service.register_font(&spec.data);
            if spec.is_default && default.is_none() {
                service.set_default_font(face, spec.default_size_px);
                default = Some(face);
            }
        }
        default
    }
}
