//! Image resource cache for RichTextEditor.
//!
//! Inline images in a `TextDocument` are referenced by name. The cache
//! resolves a name to owned bytes on first use and keeps them for later
//! paints. Decode is deferred to the host renderer — bastyde-canvas already
//! knows how to draw an image by its resource key, so the cache only has
//! to hand bytes to the render pipeline once.

use std::collections::HashMap;

use bastyde_text::text_document::TextDocument;

#[derive(Debug, Default)]
pub struct ImageCache {
    entries: HashMap<String, Option<Vec<u8>>>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up (and lazily load) image bytes by resource name. Returns
    /// `None` if the document has no resource under that name; subsequent
    /// calls with the same name hit the negative cache instead of retrying.
    pub fn get_or_load(&mut self, document: &TextDocument, name: &str) -> Option<&[u8]> {
        if !self.entries.contains_key(name) {
            let loaded = document.resource(name).ok().flatten();
            self.entries.insert(name.to_string(), loaded);
        }
        self.entries.get(name).and_then(|opt| opt.as_deref())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
