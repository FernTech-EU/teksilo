//! System clipboard abstraction.
//!
//! Widgets do not talk to `arboard` directly — they hold a `ClipboardHandle`
//! handed to them by the host application, which lets tests swap in a pure
//! in-memory backend.

use std::cell::RefCell;
use std::rc::Rc;

/// Backend-agnostic clipboard interface. Implementors read and write system
/// clipboard text. Errors are returned as strings so backends do not need a
/// shared error type.
pub trait ClipboardBackend {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
    fn has_text(&mut self) -> bool {
        self.get_text().map(|s| !s.is_empty()).unwrap_or(false)
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
        Ok(())
    }

    fn has_text(&mut self) -> bool {
        self.text.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
    }
}

#[cfg(feature = "clipboard")]
pub use arboard_backend::ArboardClipboard;

#[cfg(feature = "clipboard")]
mod arboard_backend {
    use super::ClipboardBackend;
    use arboard::Clipboard;

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
            self.inner.set_text(text.to_string()).map_err(|e| e.to_string())
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
}
