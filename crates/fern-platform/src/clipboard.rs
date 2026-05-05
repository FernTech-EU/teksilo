//! System clipboard abstraction.
//!
//! Widgets do not talk to `arboard` directly — they hold a `ClipboardHandle`
//! handed to them by the host application, which lets tests swap in a pure
//! in-memory backend.
//!
//! The trait carries two payload kinds:
//!
//!  * **Plain text** (`get_text` / `set_text`) — the universal baseline.
//!  * **HTML** (`get_html` / `set_html`) — rich content that round-trips
//!    between applications on Linux (`text/html`), macOS (`public.html`),
//!    and Windows (`CF_HTML`). `set_html` takes a plain-text alternative
//!    because real platform clipboards demand both payloads in the same
//!    transaction — writing HTML alone means pasting into a plain-text
//!    surface (Notepad, terminal) yields nothing.
//!
//! Backends without native HTML support inherit default trait bodies that
//! gracefully degrade: `get_html` reports unsupported, `set_html` falls
//! back to writing the plain-text payload. Callers who query `has_html`
//! before building rich-paste menu state avoid speculatively probing an
//! X11 selection owner when no HTML payload exists.
//!
//! Extension point for RTF / other typed payloads: add `get_rtf` /
//! `set_rtf` with the same default-body convention. The named-method
//! approach is preferred over a generic `get(mime)` for IDE
//! discoverability and for keeping the ergonomic "write HTML + plain in
//! one call" contract visible in the signature.

use std::cell::RefCell;
use std::rc::Rc;

/// Backend-agnostic clipboard interface. Implementors read and write system
/// clipboard payloads. Errors are returned as strings so backends do not need a
/// shared error type.
pub trait ClipboardBackend {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
    fn has_text(&mut self) -> bool {
        self.get_text().map(|s| !s.is_empty()).unwrap_or(false)
    }

    /// Read an HTML payload from the system clipboard. Backends without
    /// HTML support return `Err("unsupported".into())`; callers typically
    /// check `has_html` first and fall back to `get_text`.
    fn get_html(&mut self) -> Result<String, String> {
        Err("unsupported".into())
    }

    /// Write an HTML payload and a plain-text alternative onto the
    /// clipboard in one transaction. Real platform clipboards demand
    /// both so apps that only understand plain text still see the
    /// copied content.
    ///
    /// Default body: drop the HTML payload and call `set_text` with
    /// the plain alternative. Backends that support HTML natively
    /// override this method to write both payloads to the OS clipboard.
    fn set_html(&mut self, _html: &str, plain_fallback: &str) -> Result<(), String> {
        self.set_text(plain_fallback)
    }

    /// Whether the clipboard currently carries an HTML payload. Default
    /// body returns `false`; HTML-capable backends override to perform
    /// a real probe. The probe may be expensive (X11 selection-owner
    /// round-trip), so callers should invoke `has_html` only when
    /// building menu state, not per-frame.
    fn has_html(&mut self) -> bool {
        false
    }
}

/// Shared handle passed to widgets. Interior mutability so multiple widgets
/// in the same window can call into the same backend without the host having
/// to own a mutable reference per frame.
#[derive(Clone)]
pub struct ClipboardHandle {
    inner: Rc<RefCell<dyn ClipboardBackend>>,
}

impl ClipboardHandle {
    pub fn new<B: ClipboardBackend + 'static>(backend: B) -> Self {
        Self {
            inner: Rc::new(RefCell::new(backend)),
        }
    }

    pub fn get_text(&self) -> Result<String, String> {
        self.inner.borrow_mut().get_text()
    }

    pub fn set_text(&self, text: &str) -> Result<(), String> {
        self.inner.borrow_mut().set_text(text)
    }

    pub fn has_text(&self) -> bool {
        self.inner.borrow_mut().has_text()
    }

    /// Read an HTML payload from the system clipboard, or `Err` when
    /// the backend lacks HTML support or the clipboard has none.
    pub fn get_html(&self) -> Result<String, String> {
        self.inner.borrow_mut().get_html()
    }

    /// Write HTML and a plain-text alternative in one transaction. See
    /// [`ClipboardBackend::set_html`] for the platform-behaviour rationale.
    pub fn set_html(&self, html: &str, plain_fallback: &str) -> Result<(), String> {
        self.inner.borrow_mut().set_html(html, plain_fallback)
    }

    /// Whether the clipboard currently carries an HTML payload. Callers
    /// should invoke this only when building menu state (e.g. right-click
    /// context menu), not per-frame: the probe can round-trip to the
    /// selection owner on X11.
    pub fn has_html(&self) -> bool {
        self.inner.borrow_mut().has_html()
    }
}

impl std::fmt::Debug for ClipboardHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardHandle").finish_non_exhaustive()
    }
}

/// In-memory clipboard used by headless tests and by apps that opt out of the
/// real system clipboard. Not shared across processes.
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    text: Option<String>,
    html: Option<String>,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClipboardBackend for MemoryClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        Ok(self.text.clone().unwrap_or_default())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.text = Some(text.to_string());
        // Setting plain text invalidates any stored HTML — the HTML
        // payload was associated with the *previous* plain content,
        // and returning it now would be semantically wrong.
        self.html = None;
        Ok(())
    }

    fn has_text(&mut self) -> bool {
        self.text.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
    }

    fn get_html(&mut self) -> Result<String, String> {
        match self.html.as_deref() {
            Some(h) if !h.is_empty() => Ok(h.to_string()),
            _ => Err("no html payload".into()),
        }
    }

    fn set_html(&mut self, html: &str, plain_fallback: &str) -> Result<(), String> {
        self.html = Some(html.to_string());
        self.text = Some(plain_fallback.to_string());
        Ok(())
    }

    fn has_html(&mut self) -> bool {
        self.html.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
    }
}

#[cfg(feature = "clipboard")]
pub use arboard_backend::ArboardClipboard;

#[cfg(feature = "clipboard")]
mod arboard_backend {
    use super::ClipboardBackend;
    use arboard::{Clipboard, Error};

    /// Real system clipboard backed by `arboard`. One live instance per
    /// window is the expected usage; the host application constructs it
    /// during startup and hands a `ClipboardHandle::new(ArboardClipboard::new()?)`
    /// to widgets.
    pub struct ArboardClipboard {
        inner: Clipboard,
    }

    impl ArboardClipboard {
        pub fn new() -> Result<Self, String> {
            Clipboard::new()
                .map(|inner| Self { inner })
                .map_err(|e| e.to_string())
        }
    }

    impl ClipboardBackend for ArboardClipboard {
        fn get_text(&mut self) -> Result<String, String> {
            self.inner.get_text().map_err(|e| e.to_string())
        }

        fn set_text(&mut self, text: &str) -> Result<(), String> {
            self.inner
                .set_text(text.to_string())
                .map_err(|e| e.to_string())
        }

        fn get_html(&mut self) -> Result<String, String> {
            self.inner.get().html().map_err(|e| e.to_string())
        }

        fn set_html(&mut self, html: &str, plain_fallback: &str) -> Result<(), String> {
            // `arboard::Clipboard::set_html(html, alt_text)` writes both
            // HTML and the plain-text alternative in a single transaction
            // — matching the Linux `text/html` + `UTF8_STRING` pair, macOS
            // `NSPasteboardTypeHTML` + `NSPasteboardTypeString`, and
            // Windows `CF_HTML` + `CF_UNICODETEXT`.
            self.inner
                .set_html(html.to_string(), Some(plain_fallback.to_string()))
                .map_err(|e| e.to_string())
        }

        fn has_html(&mut self) -> bool {
            match self.inner.get().html() {
                Ok(s) => !s.is_empty(),
                // `ContentNotAvailable` just means the clipboard does
                // not currently carry an HTML payload — expected, not an
                // error. Any other error (backend disconnect, IPC) also
                // resolves to `false`: the menu treats "don't know" as
                // "nothing to paste" and we avoid leaking a flaky X11
                // round-trip into UI state.
                Err(Error::ContentNotAvailable) => false,
                Err(_) => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_backend_roundtrip() {
        let handle = ClipboardHandle::new(MemoryClipboard::new());
        assert!(!handle.has_text());
        handle.set_text("hello").unwrap();
        assert!(handle.has_text());
        assert_eq!(handle.get_text().unwrap(), "hello");
        handle.set_text("").unwrap();
        assert!(!handle.has_text());
    }

    #[test]
    fn handle_is_cloneable_and_shares_state() {
        let a = ClipboardHandle::new(MemoryClipboard::new());
        let b = a.clone();
        a.set_text("shared").unwrap();
        assert_eq!(b.get_text().unwrap(), "shared");
    }

    #[test]
    fn memory_backend_html_roundtrip() {
        let handle = ClipboardHandle::new(MemoryClipboard::new());
        assert!(!handle.has_html(), "empty clipboard has no html");

        handle.set_html("<p>a</p>", "a").unwrap();
        assert!(handle.has_html(), "set_html must flip has_html");
        assert_eq!(handle.get_html().unwrap(), "<p>a</p>");
        assert_eq!(
            handle.get_text().unwrap(),
            "a",
            "set_html must also install the plain-text alternative"
        );
    }

    #[test]
    fn memory_backend_set_text_invalidates_html() {
        // Self-round-trip detection in the rich-text editor compares
        // the stored plain text against what the system clipboard
        // currently reports. If we left the old HTML behind after a
        // plain-text overwrite, paste would reinsert a rich fragment
        // whose plain form no longer matches the clipboard.
        let handle = ClipboardHandle::new(MemoryClipboard::new());
        handle.set_html("<b>old</b>", "old").unwrap();
        assert!(handle.has_html());
        handle.set_text("new").unwrap();
        assert!(
            !handle.has_html(),
            "plain-text overwrite must invalidate stale html"
        );
        assert!(handle.get_html().is_err());
    }

    #[test]
    fn handle_html_shared_state() {
        let a = ClipboardHandle::new(MemoryClipboard::new());
        let b = a.clone();
        a.set_html("<p>shared</p>", "shared").unwrap();
        assert_eq!(b.get_html().unwrap(), "<p>shared</p>");
        assert_eq!(b.get_text().unwrap(), "shared");
    }

    #[test]
    fn default_set_html_falls_back_to_plain_text() {
        // A hand-rolled backend that does not override set_html / get_html
        // must still round-trip plain text correctly via the default trait
        // body — writes become `set_text(plain_fallback)` so pasting into
        // a plain-text surface continues to work.
        struct PlainOnly {
            text: Option<String>,
        }
        impl ClipboardBackend for PlainOnly {
            fn get_text(&mut self) -> Result<String, String> {
                Ok(self.text.clone().unwrap_or_default())
            }
            fn set_text(&mut self, text: &str) -> Result<(), String> {
                self.text = Some(text.to_string());
                Ok(())
            }
        }

        let handle = ClipboardHandle::new(PlainOnly { text: None });
        assert!(!handle.has_html());
        assert!(handle.get_html().is_err());
        handle.set_html("<p>ignored</p>", "fallback").unwrap();
        assert_eq!(handle.get_text().unwrap(), "fallback");
        assert!(!handle.has_html(), "plain-only backend never reports html");
    }
}
