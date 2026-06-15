// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Typed drag payload for intra-application drag and drop.
//!
//! `DragPayload` carries the data being dragged. For intra-application DnD,
//! the fast path stores a typed Rust value (via `Any`). For drops originating
//! outside the application (files / text / URLs dragged from the OS), the
//! payload carries an [`ExternalDropData`] plus MIME-typed byte
//! representations.

use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;

/// Where a drag originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOrigin {
    /// Started inside the application via `EventContext::start_drag`.
    Internal,
    /// Delivered by the OS — files / text / URLs dragged from another
    /// application or the file manager into a window.
    External,
}

/// How a drag the source widget started ended. Delivered to the source's
/// `on_drag_ended` handler so it can react (e.g. remove the item on a move).
///
/// One unified completion outcome for *every* drag a widget starts — whether
/// it dropped on an in-app target, was exported to another application via the
/// OS, or was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropOutcome {
    /// Dropped on an in-app target. `accepted` is what the target's `on_drop`
    /// returned.
    InApp {
        /// Whether the drop target accepted the payload.
        accepted: bool,
    },
    /// Exported to another application via the OS as a copy.
    OsCopy,
    /// Exported to another application via the OS as a move — the source
    /// should remove the dragged item.
    OsMove,
    /// No drop happened: Escape, dropped on nothing, or the OS rejected it.
    Cancelled,
}

/// Flattened, OS-exportable view of a [`DragPayload`], handed to the platform
/// backend when an in-app drag escalates to an OS drag at the window boundary.
///
/// Plain data with no GUI dependency so [`crate::window::WindowOps`] can name
/// it without bastyde-core depending on bastyde-platform.
#[derive(Debug, Clone, Default)]
pub struct OutboundDragData {
    /// MIME-typed byte representations to advertise to the OS.
    pub mime: HashMap<String, Vec<u8>>,
    /// Filesystem paths, if the payload represents files.
    pub files: Vec<PathBuf>,
    /// Plain text, if any.
    pub text: Option<String>,
    /// Non-file URLs, if any.
    pub uris: Vec<String>,
}

impl OutboundDragData {
    /// True when there is nothing to hand the OS.
    pub fn is_empty(&self) -> bool {
        self.mime.is_empty() && self.files.is_empty() && self.text.is_none() && self.uris.is_empty()
    }
}

/// Optional drag image for an OS drag (RGBA8, top-left origin, premultiplied
/// alpha not assumed). `None` lets the platform draw a default. `hot_x` /
/// `hot_y` are the cursor hotspot in logical pixels from the image's top-left.
#[derive(Debug, Clone)]
pub struct DragImageData {
    /// Pixel data, `width * height * 4` bytes (RGBA8, row-major, top-left).
    pub rgba: Vec<u8>,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Cursor hotspot X in logical pixels from the top-left.
    pub hot_x: f32,
    /// Cursor hotspot Y in logical pixels from the top-left.
    pub hot_y: f32,
}

/// Data delivered by an external (OS) drag-and-drop.
///
/// Platform backends populate the fields they can extract from the native
/// drag payload. `files` are real filesystem paths, `text` is plain UTF-8
/// text, `uris` are non-`file://` URLs (e.g. `https://…`). `mime` holds any
/// additional raw representations keyed by MIME type, for consumers that want
/// the bytes verbatim.
#[derive(Debug, Clone, Default)]
pub struct ExternalDropData {
    /// Dropped filesystem paths.
    pub files: Vec<PathBuf>,
    /// Dropped plain text, if any.
    pub text: Option<String>,
    /// Dropped non-file URLs (http, https, mailto, …).
    pub uris: Vec<String>,
    /// Additional raw MIME representations, keyed by MIME type.
    pub mime: HashMap<String, Vec<u8>>,
    /// Advertised data formats (platform MIME types / type identifiers),
    /// available at drag-*enter* time even before the bytes are transferred.
    /// On Wayland the actual `files` / `text` / `uris` are only filled at drop
    /// (received over a pipe), so a drop target validates on hover from these
    /// format strings (e.g. `"text/uri-list"` ⇒ a file drag). macOS / Windows
    /// read the full payload at enter, so they fill the data directly and may
    /// leave this empty.
    pub formats: Vec<String>,
}

impl ExternalDropData {
    /// Build from a `text/uri-list` payload (RFC 2483): one URI per line,
    /// `#`-prefixed comment lines ignored, CRLF line endings, percent-encoded.
    /// `file://` URIs become [`Self::files`]; everything else becomes
    /// [`Self::uris`]. The raw list is also retained under the
    /// `text/uri-list` MIME key.
    pub fn from_uri_list(list: &str) -> Self {
        let mut files = Vec::new();
        let mut uris = Vec::new();
        for line in list.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("file://") {
                files.push(uri_path_to_pathbuf(rest));
            } else {
                uris.push(percent_decode(line));
            }
        }
        let mut mime = HashMap::new();
        mime.insert("text/uri-list".to_string(), list.as_bytes().to_vec());
        Self {
            files,
            text: None,
            uris,
            mime,
            formats: vec!["text/uri-list".to_string()],
        }
    }

    /// True when there is nothing usable in this payload.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.text.is_none() && self.uris.is_empty()
    }
}

/// Decode a `file://` URI tail (everything after `file://`) into a `PathBuf`.
///
/// Handles the optional `//host` authority (UNC on Windows, dropped on Unix
/// for the local host), strips it, percent-decodes the path, and on Windows
/// turns a leading `/C:/…` into `C:\…`.
fn uri_path_to_pathbuf(after_scheme: &str) -> PathBuf {
    // `after_scheme` is what followed `file://`. A leading authority segment
    // ends at the next `/`. The common local form is `file:///path` →
    // authority empty → `after_scheme` starts with `/`.
    let (authority, path) = match after_scheme.find('/') {
        Some(idx) => (&after_scheme[..idx], &after_scheme[idx..]),
        None => (after_scheme, ""),
    };
    let decoded = percent_decode(path);

    #[cfg(windows)]
    {
        // UNC share: file://server/share → \\server\share
        if !authority.is_empty() {
            let mut s = String::from(r"\\");
            s.push_str(authority);
            s.push_str(&decoded.replace('/', r"\"));
            return PathBuf::from(s);
        }
        // Drive path: /C:/Users → C:\Users
        let trimmed = decoded.strip_prefix('/').unwrap_or(&decoded);
        return PathBuf::from(trimmed.replace('/', r"\"));
    }
    #[cfg(not(windows))]
    {
        let _ = authority; // non-local authorities are rare; keep the path
        PathBuf::from(decoded)
    }
}

/// Percent-decode a URI component (`%20` → space, etc.). Invalid escapes are
/// left verbatim. Operates on UTF-8 bytes so multi-byte sequences decode
/// correctly.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A drag payload carrying data from a drag source to a drop target.
///
/// For intra-application transfers, use `DragPayload::typed(data)` to store
/// a typed Rust value. Drop targets extract it via `get_typed::<T>()`.
///
/// For drops from the OS, use `DragPayload::external(data)`; targets read
/// `files()` / `text()` / `uris()` (and `origin()` / `is_external()` to
/// distinguish the source).
pub struct DragPayload {
    /// Typed intra-app payload (fast path, no serialization).
    typed: Option<Box<dyn Any>>,
    /// MIME-typed byte data (cross-app DnD, populated for external drags).
    mime_data: HashMap<String, Vec<u8>>,
    /// Where this drag came from.
    origin: DragOrigin,
    /// Structured external-drop data (only for `DragOrigin::External`).
    external: Option<ExternalDropData>,
}

impl DragPayload {
    /// Create a payload from a typed Rust value.
    pub fn typed<T: 'static>(data: T) -> Self {
        Self {
            typed: Some(Box::new(data)),
            mime_data: HashMap::new(),
            origin: DragOrigin::Internal,
            external: None,
        }
    }

    /// Create an empty payload (for MIME-only transfers).
    pub fn empty() -> Self {
        Self {
            typed: None,
            mime_data: HashMap::new(),
            origin: DragOrigin::Internal,
            external: None,
        }
    }

    /// Create a payload from OS-delivered external drop data.
    ///
    /// Synthesizes canonical MIME entries (`text/plain` from `text`,
    /// `text/uri-list` from `files` + `uris` when not already present) so the
    /// generic `get_mime` API and the typed `files()` / `text()` / `uris()`
    /// accessors stay consistent.
    pub fn external(data: ExternalDropData) -> Self {
        let mut mime_data = data.mime.clone();
        if let Some(text) = &data.text {
            mime_data
                .entry("text/plain".to_string())
                .or_insert_with(|| text.clone().into_bytes());
        }
        if (!data.files.is_empty() || !data.uris.is_empty())
            && !mime_data.contains_key("text/uri-list")
        {
            let mut list = String::new();
            for f in &data.files {
                list.push_str("file://");
                list.push_str(&f.to_string_lossy());
                list.push_str("\r\n");
            }
            for u in &data.uris {
                list.push_str(u);
                list.push_str("\r\n");
            }
            mime_data.insert("text/uri-list".to_string(), list.into_bytes());
        }
        Self {
            typed: None,
            mime_data,
            origin: DragOrigin::External,
            external: Some(data),
        }
    }

    /// Where this drag originated.
    pub fn origin(&self) -> DragOrigin {
        self.origin
    }

    /// Whether this payload came from outside the application (an OS drop).
    pub fn is_external(&self) -> bool {
        self.origin == DragOrigin::External
    }

    /// Dropped filesystem paths (empty for internal drags or non-file drops).
    pub fn files(&self) -> &[PathBuf] {
        self.external.as_ref().map_or(&[], |e| &e.files)
    }

    /// Dropped plain text, if any.
    pub fn text(&self) -> Option<&str> {
        self.external.as_ref().and_then(|e| e.text.as_deref())
    }

    /// Dropped non-file URLs (empty for internal drags or file-only drops).
    pub fn uris(&self) -> &[String] {
        self.external.as_ref().map_or(&[], |e| &e.uris)
    }

    /// Advertised data formats (platform MIME types / type identifiers) for an
    /// external drag. Available at drag-enter even before the bytes transfer —
    /// the basis for hover-time accept/reject when `files()` / `text()` /
    /// `uris()` aren't populated yet (Wayland). See
    /// [`ExternalDropData::formats`].
    pub fn formats(&self) -> &[String] {
        self.external.as_ref().map_or(&[], |e| &e.formats)
    }

    /// Add a MIME-typed byte representation.
    pub fn with_mime(mut self, mime_type: &str, data: Vec<u8>) -> Self {
        self.mime_data.insert(mime_type.to_string(), data);
        self
    }

    /// Populate the structured external view (`files` / `text` / `uris`) from
    /// this payload's MIME data, so a drag that round-tripped through the OS
    /// and re-entered the app satisfies file/text drop targets (e.g. a
    /// `DropZone`) **as well as** typed in-app targets. The typed value is
    /// preserved, so `get_typed::<T>()` still works.
    ///
    /// **Origin is deliberately left `Internal`.** A round-tripped drag still
    /// *originated from this app*, so `is_external()` stays `false` — an in-app
    /// reorder target that rejects external drags via `!is_external()` must
    /// still accept its own drag coming back. `is_external()` means "came from
    /// another application", not "carries file/text data". Consumers that want
    /// content should test `files()` / `text()` / `uris()` (or `has_typed`),
    /// which is what `DropZone` does. So a payload here can legitimately have
    /// `origin == Internal` *and* a populated external view.
    ///
    /// No-op if an external view is already present or no recognizable MIME is
    /// carried.
    pub fn enrich_external_from_mime(&mut self) {
        if self.external.is_some() {
            return;
        }
        let mut ext = ExternalDropData::default();
        if let Some(bytes) = self.mime_data.get("text/uri-list") {
            let parsed = ExternalDropData::from_uri_list(&String::from_utf8_lossy(bytes));
            ext.files = parsed.files;
            ext.uris = parsed.uris;
        }
        if let Some(bytes) = self
            .mime_data
            .get("text/plain")
            .or_else(|| self.mime_data.get("text/plain;charset=utf-8"))
        {
            ext.text = Some(String::from_utf8_lossy(bytes).into_owned());
        }
        ext.formats = self.mime_data.keys().cloned().collect();
        if !ext.is_empty() {
            self.external = Some(ext);
        }
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

    /// Whether this payload carries anything an OS drag could export. A drag
    /// becomes OS-exportable simply by populating `mime_data` (via
    /// [`Self::with_mime`]) or by carrying external file / text / URI data.
    pub fn is_os_exportable(&self) -> bool {
        !self.mime_data.is_empty()
            || self
                .external
                .as_ref()
                .is_some_and(|e| !e.files.is_empty() || e.text.is_some() || !e.uris.is_empty())
    }

    /// Extract the flattened, OS-exportable view used when an in-app drag
    /// escalates to an OS drag at the window boundary.
    ///
    /// Internal drags populate only `mime_data` (via [`Self::with_mime`]); the
    /// `files` / `text` / `uris` fields come from [`ExternalDropData`] and are
    /// empty for them. So when those structured fields are absent, derive them
    /// from the canonical `text/uri-list` / `text/plain` MIME entries — the
    /// platform backends (NSURL items, etc.) need the structured form.
    pub fn to_outbound(&self) -> OutboundDragData {
        let ext = self.external.as_ref();
        let mut files = ext.map(|e| e.files.clone()).unwrap_or_default();
        let mut uris = ext.map(|e| e.uris.clone()).unwrap_or_default();
        let mut text = ext.and_then(|e| e.text.clone());

        if files.is_empty()
            && uris.is_empty()
            && let Some(bytes) = self.mime_data.get("text/uri-list")
        {
            let parsed = ExternalDropData::from_uri_list(&String::from_utf8_lossy(bytes));
            files = parsed.files;
            uris = parsed.uris;
        }
        if text.is_none()
            && let Some(bytes) = self
                .mime_data
                .get("text/plain")
                .or_else(|| self.mime_data.get("text/plain;charset=utf-8"))
        {
            text = Some(String::from_utf8_lossy(bytes).into_owned());
        }

        OutboundDragData {
            mime: self.mime_data.clone(),
            files,
            text,
            uris,
        }
    }
}

impl std::fmt::Debug for DragPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragPayload")
            .field("origin", &self.origin)
            .field("has_typed", &self.typed.is_some())
            .field("mime_types", &self.mime_types())
            .field("files", &self.files())
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

    #[test]
    fn external_payload_origin_and_accessors() {
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png")],
            text: Some("hello".into()),
            uris: vec!["https://example.com".into()],
            mime: HashMap::new(),
            formats: Vec::new(),
        };
        let payload = DragPayload::external(data);

        assert!(payload.is_external());
        assert_eq!(payload.origin(), DragOrigin::External);
        assert!(!payload.has_typed::<u32>());
        assert_eq!(payload.files(), &[PathBuf::from("/tmp/a.png")]);
        assert_eq!(payload.text(), Some("hello"));
        assert_eq!(payload.uris(), &["https://example.com".to_string()]);
        // Canonical MIME entries are synthesized.
        assert!(payload.has_mime("text/plain"));
        assert!(payload.has_mime("text/uri-list"));
    }

    #[test]
    fn internal_payload_has_no_external_data() {
        let payload = DragPayload::typed(7_u32);
        assert!(!payload.is_external());
        assert_eq!(payload.origin(), DragOrigin::Internal);
        assert!(payload.files().is_empty());
        assert_eq!(payload.text(), None);
        assert!(payload.uris().is_empty());
    }

    #[test]
    fn uri_list_parses_files_and_urls() {
        let list = "#comment\r\nfile:///tmp/My%20File.txt\r\nhttps://example.com/a%2Bb\r\n";
        let data = ExternalDropData::from_uri_list(list);
        assert_eq!(data.files, vec![PathBuf::from("/tmp/My File.txt")]);
        assert_eq!(data.uris, vec!["https://example.com/a+b".to_string()]);
        assert!(data.mime.contains_key("text/uri-list"));
    }

    #[test]
    fn percent_decode_handles_utf8_and_invalid() {
        // "café" encoded; plus an invalid trailing % left verbatim.
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
    }

    #[cfg(not(windows))]
    #[test]
    fn file_uri_to_pathbuf_unix() {
        assert_eq!(uri_path_to_pathbuf("/tmp/a%20b"), PathBuf::from("/tmp/a b"));
    }
}
