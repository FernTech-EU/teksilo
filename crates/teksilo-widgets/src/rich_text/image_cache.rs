// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Decoded-image cache for RichTextEditor.
//!
//! Inline images in a `TextDocument` are referenced by name. The cache resolves
//! a name to *decoded* RGBA pixels once and keeps them for later paints.
//!
//! Two things about this are worth stating, because the obvious shape is wrong
//! in both:
//!
//! **Decoding belongs here, not in the renderer.** The canvas draws an image by
//! resource key, but a key only resolves once something has called
//! `Canvas::ensure_image_registered` with real pixels. Nothing does that for
//! document images except this cache's owner, so a cache that stored encoded
//! bytes would hand the paint pass something it could not use — the draw command
//! would reference a texture that was never uploaded, and the renderer would
//! silently skip it. That was the state of inline images before this cache
//! decoded anything: they laid out, took space, hit-tested, and painted nothing.
//!
//! **Display size is bounded here, not at insert time.** A document may
//! legitimately hold a 40-megapixel original — that is the writer's file and
//! nothing should quietly degrade it. But uploading it to the GPU at full
//! resolution costs ~160 MB of VRAM for something drawn into a few hundred
//! pixels of column. [`MAX_DISPLAY_EDGE`] caps what gets uploaded while leaving
//! the document's bytes untouched.

use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::RasterIcon;
use teksilo_text::text_document::{ResourceType, TextDocument};

/// Asked for an image's bytes when the document has no resource under that name.
///
/// Returns `(mime_type, bytes)`, or `None` if the host cannot supply it either.
///
/// A document's images are its own resources, so a name that arrives *without*
/// them resolves to nothing — which is what a paste into a second editor is: the
/// interchange format carries the reference, not the pixels. Rather than have
/// every host scan for newly-arrived names after every edit, the cache asks for
/// what it is missing, once, at the moment it needs it. The answer is written
/// into the document, so paste, drag-and-drop, and an undo that re-inserts a
/// deleted image are all served by the same hook without any of them knowing
/// about it.
pub type ImageResolver = Rc<dyn Fn(&str) -> Option<(String, Vec<u8>)>>;

/// Longest edge, in pixels, of any image uploaded to the GPU for on-screen use.
///
/// Sized for a full-screen image on a HiDPI display with room to spare; a text
/// column is far narrower. Originals larger than this are downscaled for
/// display only — the document keeps whatever the writer inserted.
pub const MAX_DISPLAY_EDGE: u32 = 2048;

/// One resolved entry: the decoded display-resolution pixels, plus whether the
/// canvas has been told about them yet.
struct Entry {
    icon: RasterIcon,
    /// Cleared by [`ImageCache::invalidate_registrations`] when the surface that
    /// owned the texture goes away, so the next paint re-uploads.
    registered: bool,
}

/// Name → decoded pixels, with negative caching for names that resolve to
/// nothing.
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<String, Option<Entry>>,
}

impl std::fmt::Debug for ImageCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageCache")
            .field("len", &self.entries.len())
            .finish()
    }
}

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve, decode and cache the image stored under `name`.
    ///
    /// Returns `None` if the document has no such resource, the host's
    /// `resolver` cannot supply one either, or the bytes are not a supported
    /// image; subsequent calls with the same name hit the negative cache instead
    /// of retrying a decode that already failed. A broken image must not cost a
    /// decode attempt on every frame.
    fn resolve(
        &mut self,
        document: &TextDocument,
        name: &str,
        resolver: Option<&ImageResolver>,
    ) -> Option<&mut Entry> {
        if !self.entries.contains_key(name) {
            let mut bytes = document.resource(name).ok().flatten();
            // Nothing under that name: ask the host, and keep what it gives us
            // *on the document*, so the answer is permanent rather than living
            // only in this widget's cache. A second editor on the same document,
            // a save, and an export all read the resource table.
            if bytes.is_none()
                && let Some(resolve) = resolver
                && let Some((mime, supplied)) = resolve(name)
            {
                let _ = document.add_resource(ResourceType::Image, name, &mime, &supplied);
                bytes = Some(supplied);
            }
            let decoded = bytes
                .and_then(|bytes| RasterIcon::decode(&bytes).ok())
                .map(|icon| {
                    // Cap only what the GPU sees; the document keeps the original.
                    let icon = icon.downsample_to_max(MAX_DISPLAY_EDGE).unwrap_or(icon);
                    Entry {
                        icon,
                        registered: false,
                    }
                });
            self.entries.insert(name.to_string(), decoded);
        }
        self.entries.get_mut(name).and_then(|e| e.as_mut())
    }

    /// Ensure the canvas can draw `name`, uploading the pixels the first time.
    ///
    /// Returns `false` when the image could not be resolved, so the caller can
    /// skip emitting a draw command that would reference nothing.
    pub fn ensure_registered(
        &mut self,
        canvas: &mut teksilo_canvas::Canvas,
        document: &TextDocument,
        name: &str,
        resolver: Option<&ImageResolver>,
    ) -> bool {
        let Some(entry) = self.resolve(document, name, resolver) else {
            return false;
        };
        // `registered` tracks this cache's own uploads; `has_pending_image`
        // catches a second widget in the same frame having already queued the
        // same key, which would otherwise upload identical pixels twice.
        if !entry.registered && !canvas.has_pending_image(name) {
            canvas.ensure_image_registered(
                name,
                entry.icon.width(),
                entry.icon.height(),
                std::borrow::Cow::Owned(entry.icon.pixels().to_vec()),
            );
        }
        entry.registered = true;
        true
    }

    /// Decoded display size of a cached image, if it resolved.
    pub fn size_of(&self, name: &str) -> Option<(u32, u32)> {
        self.entries
            .get(name)
            .and_then(|e| e.as_ref())
            .map(|e| (e.icon.width(), e.icon.height()))
    }

    /// Drop every cached entry — used when the document is replaced wholesale.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Forget which images have been uploaded without discarding the decoded
    /// pixels. The next paint re-registers them.
    ///
    /// Needed because textures live in the renderer, which may drop them
    /// independently of this cache (surface loss, device reset); keeping the
    /// decode but redoing the upload is much cheaper than decoding again.
    pub fn invalidate_registrations(&mut self) {
        for entry in self.entries.values_mut().flatten() {
            entry.registered = false;
        }
    }

    /// Drop entries whose names are not in `live`, returning the names dropped
    /// so the caller can release their GPU textures.
    ///
    /// Without this an editing session that inserts and deletes many images
    /// grows both this cache and the renderer's texture table without bound.
    pub fn retain_only<'a>(&mut self, live: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let live: std::collections::HashSet<&str> = live.into_iter().collect();
        let dropped: Vec<String> = self
            .entries
            .keys()
            .filter(|k| !live.contains(k.as_str()))
            .cloned()
            .collect();
        for name in &dropped {
            self.entries.remove(name);
        }
        dropped
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
