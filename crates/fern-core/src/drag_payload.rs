//! Typed drag payload for intra-application drag and drop.
//!
//! `DragPayload` carries the data being dragged. For intra-application DnD,
//! the fast path stores a typed Rust value (via `Any`). For future cross-
//! application DnD, MIME-typed byte representations can be added.

use std::any::Any;
use std::collections::HashMap;

/// A drag payload carrying data from a drag source to a drop target.
///
/// For intra-application transfers, use `DragPayload::typed(data)` to store
/// a typed Rust value. Drop targets extract it via `get_typed::<T>()`.
///
/// For future cross-application transfers, MIME-typed byte data can be added
/// via `with_mime()`.
pub struct DragPayload {
    /// Typed intra-app payload (fast path, no serialization).
    typed: Option<Box<dyn Any>>,
    /// MIME-typed byte data (for cross-app DnD, future).
    mime_data: HashMap<String, Vec<u8>>,
}

impl DragPayload {
    /// Create a payload from a typed Rust value.
    pub fn typed<T: 'static>(data: T) -> Self {
        Self {
            typed: Some(Box::new(data)),
            mime_data: HashMap::new(),
        }
    }

    /// Create an empty payload (for MIME-only transfers).
    pub fn empty() -> Self {
        Self {
            typed: None,
            mime_data: HashMap::new(),
        }
    }

    /// Add a MIME-typed byte representation.
    pub fn with_mime(mut self, mime_type: &str, data: Vec<u8>) -> Self {
        self.mime_data.insert(mime_type.to_string(), data);
        self
    }

    /// Extract the typed payload by type. Returns `None` if the type doesn't match
    /// or no typed payload was set.
    pub fn get_typed<T: 'static>(&self) -> Option<&T> {
        self.typed.as_ref().and_then(|v| v.downcast_ref::<T>())
    }

    /// Take the typed payload, consuming it from the DragPayload.
    pub fn take_typed<T: 'static>(&mut self) -> Option<T> {
        let boxed = self.typed.take()?;
        match boxed.downcast::<T>() {
            Ok(value) => Some(*value),
            Err(boxed) => {
                // Put it back if the type didn't match
                self.typed = Some(boxed);
                None
            }
        }
    }

    /// Whether this payload has a typed value of the given type.
    pub fn has_typed<T: 'static>(&self) -> bool {
        self.typed
            .as_ref()
            .is_some_and(|v| v.downcast_ref::<T>().is_some())
    }

    /// Whether this payload has data for the given MIME type.
    pub fn has_mime(&self, mime_type: &str) -> bool {
        self.mime_data.contains_key(mime_type)
    }

    /// Get MIME-typed byte data.
    pub fn get_mime(&self, mime_type: &str) -> Option<&[u8]> {
        self.mime_data.get(mime_type).map(|v| v.as_slice())
    }

    /// List all MIME types in this payload.
    pub fn mime_types(&self) -> Vec<&str> {
        self.mime_data.keys().map(|s| s.as_str()).collect()
    }
}

impl std::fmt::Debug for DragPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragPayload")
            .field("has_typed", &self.typed.is_some())
            .field("mime_types", &self.mime_types())
            .finish()
    }
}

/// Trait for typed drag data with an associated MIME type.
///
/// Implementing this trait allows a type to be used as both an intra-application
/// typed payload and (in the future) a cross-application MIME-serialized payload.
pub trait DragData: Any + std::fmt::Debug + 'static {
    /// The canonical MIME type for this data type.
    fn mime_type(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct ChapterDrag {
        chapter_id: u32,
        title: String,
    }

    impl DragData for ChapterDrag {
        fn mime_type(&self) -> &'static str {
            "application/x-skribisto-chapter"
        }
    }

    #[test]
    fn typed_roundtrip() {
        let payload = DragPayload::typed(ChapterDrag {
            chapter_id: 42,
            title: "Introduction".into(),
        });

        assert!(payload.has_typed::<ChapterDrag>());
        assert!(!payload.has_typed::<String>());

        let extracted = payload.get_typed::<ChapterDrag>().unwrap();
        assert_eq!(extracted.chapter_id, 42);
        assert_eq!(extracted.title, "Introduction");
    }

    #[test]
    fn take_typed() {
        let mut payload = DragPayload::typed(42_u32);
        assert!(payload.has_typed::<u32>());
        let val = payload.take_typed::<u32>().unwrap();
        assert_eq!(val, 42);
        assert!(!payload.has_typed::<u32>());
    }

    #[test]
    fn take_typed_wrong_type_preserves() {
        let mut payload = DragPayload::typed(42_u32);
        assert!(payload.take_typed::<String>().is_none());
        assert!(payload.has_typed::<u32>()); // still there
    }

    #[test]
    fn mime_data() {
        let payload = DragPayload::empty()
            .with_mime("text/plain", b"hello".to_vec())
            .with_mime("text/html", b"<b>hello</b>".to_vec());

        assert!(payload.has_mime("text/plain"));
        assert!(payload.has_mime("text/html"));
        assert!(!payload.has_mime("image/png"));

        assert_eq!(payload.get_mime("text/plain"), Some(b"hello".as_slice()));
        assert_eq!(payload.mime_types().len(), 2);
    }

    #[test]
    fn typed_with_mime() {
        let payload = DragPayload::typed(ChapterDrag {
            chapter_id: 1,
            title: "Ch1".into(),
        })
        .with_mime("text/plain", b"Ch1".to_vec());

        assert!(payload.has_typed::<ChapterDrag>());
        assert!(payload.has_mime("text/plain"));
    }

    #[test]
    fn debug_format() {
        let payload = DragPayload::typed(42_u32);
        let s = format!("{:?}", payload);
        assert!(s.contains("DragPayload"));
        assert!(s.contains("has_typed: true"));
    }
}
