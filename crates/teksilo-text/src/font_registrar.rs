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

/// A single font face to register with a typesetter. The bytes are held in a
/// shared container so the registrar can outlive any borrow of the source data
/// (a file on disk, a memory-mapped archive) without copying it.
///
/// [`SharedFontData`](text_typeset::SharedFontData) is
/// `Arc<dyn AsRef<[u8]> + Sync + Send>`, so a face
/// compiled into the binary costs nothing to register: `Arc::new(bytes)` over a
/// `&'static [u8]` shares the rodata rather than duplicating it onto the heap.
/// An owned buffer still works, because `Arc<Vec<u8>>` coerces to the same type.
/// Share a face compiled into the binary rather than copying it.
///
/// An `include_bytes!`-ed face is already resident in the binary's rodata, so
/// wrapping the static slice registers it as-is. `register_font` would put a
/// second copy of every face on the heap for the life of the process.
pub(crate) fn shared_static(bytes: &'static [u8]) -> text_typeset::SharedFontData {
    std::sync::Arc::new(bytes)
}

#[derive(Clone)]
pub struct FontFaceSpec {
    /// Raw TTF/OTF/WOFF data.
    pub data: text_typeset::SharedFontData,
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
            .field("data_len", &self.data.as_ref().as_ref().len())
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
/// teksilo-text already bundles) as the default face and JetBrains
/// Mono as the monospace face, returning Inter's face id as the
/// default. Used when a `RichTextEditor` is built without an explicit
/// `font_registrar()`. Mirrors [`TypesetterBridge::new_with_default_font`]
/// so a standalone engine resolves the default theme's "Inter" and
/// "JetBrains Mono" typography families the same way the shared
/// app-wide service does.
///
/// [`TypesetterBridge::new_with_default_font`]: crate::TypesetterBridge::new_with_default_font
pub struct EmbeddedInterRegistrar {
    data: &'static [u8],
    data_italic: &'static [u8],
    mono_data: &'static [u8],
    mono_data_italic: &'static [u8],
    default_size_px: f32,
}

impl EmbeddedInterRegistrar {
    pub const fn new() -> Self {
        Self {
            data: include_bytes!("../fonts/InterVariable.ttf"),
            data_italic: include_bytes!("../fonts/InterVariable-Italic.ttf"),
            mono_data: include_bytes!("../fonts/JetBrainsMono.ttf"),
            mono_data_italic: include_bytes!("../fonts/JetBrainsMono-Italic.ttf"),
            default_size_px: 14.0,
        }
    }

    pub const fn with_size(size_px: f32) -> Self {
        Self {
            data: include_bytes!("../fonts/InterVariable.ttf"),
            data_italic: include_bytes!("../fonts/InterVariable-Italic.ttf"),
            mono_data: include_bytes!("../fonts/JetBrainsMono.ttf"),
            mono_data_italic: include_bytes!("../fonts/JetBrainsMono-Italic.ttf"),
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
        // Every face here is `include_bytes!`-ed, so it is already resident in
        // the binary's rodata. Sharing the static slice registers it without a
        // second copy on the heap; `register_font` would duplicate all four.
        let face = service.register_font_shared(shared_static(self.data));
        service.set_default_font(face, self.default_size_px);
        let _ = service.register_font_shared(shared_static(self.data_italic));
        // `InterVariable.ttf` registers under family "Inter Variable"; the
        // default theme requests "Inter". Alias so the request resolves to
        // the bundled face (see `TypesetterBridge::register_default_font`).
        service.set_generic_family("Inter", "Inter Variable");
        // Register the monospace faces (upright + italic) for the theme's
        // `mono` typography token.
        let _ = service.register_font_shared(shared_static(self.mono_data));
        let _ = service.register_font_shared(shared_static(self.mono_data_italic));
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
            // `register_font_shared`, never `register_font`: the latter copies
            // the whole face onto the heap, and every registrar here already
            // holds its bytes in a shareable container. On the app's bundled
            // writing serifs that copy was 6.4 MB, resident for the session.
            let face = service.register_font_shared(spec.data.clone());
            if spec.is_default && default.is_none() {
                service.set_default_font(face, spec.default_size_px);
                default = Some(face);
            }
        }
        default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face compiled into the binary must be *shared*, never copied.
    ///
    /// Both halves of this were regressions: `FontFaceSpec` held an
    /// `Arc<Vec<u8>>`, so describing a static face required copying it out of
    /// rodata, and `FontRegistry::register_font` then copied it again. On the
    /// downstream writing app, with eight bundled serifs, that was 12.8 MB
    /// resident for the life of the process.
    ///
    /// Comparing the pointer is what makes this a real guard: an
    /// `Arc<Vec<u8>>` built from the same bytes compares equal by value and
    /// would pass a content check while allocating exactly what this forbids.
    #[test]
    fn a_static_face_is_shared_rather_than_copied() {
        static FACE: &[u8] = b"not really a font, but the bytes are the point";

        let spec = FontFaceSpec {
            data: std::sync::Arc::new(FACE),
            is_default: false,
            default_size_px: 14.0,
        };

        let held: &[u8] = spec.data.as_ref().as_ref();
        assert_eq!(
            held.as_ptr(),
            FACE.as_ptr(),
            "the spec must point at the original bytes, not a copy of them"
        );
        assert_eq!(held.len(), FACE.len());
    }

    /// The owned case still has to work: a face read from disk or out of an
    /// archive has no static lifetime, and `Arc<Vec<u8>>` must keep coercing.
    #[test]
    fn an_owned_face_still_describes_itself() {
        let bytes = vec![1u8, 2, 3, 4];
        let spec = FontFaceSpec {
            data: std::sync::Arc::new(bytes),
            is_default: true,
            default_size_px: 18.0,
        };
        assert_eq!(spec.data.as_ref().as_ref(), &[1u8, 2, 3, 4]);
        assert!(spec.is_default);
    }
}
