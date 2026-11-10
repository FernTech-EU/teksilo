// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

#[cfg(all(feature = "clipboard", unix, not(target_os = "macos")))]
mod wayland;

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

    #[cfg(feature = "clipboard")]
    fn from_box(backend: Box<dyn ClipboardBackend>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(BoxedBackend(backend))),
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

/// A boxed backend, so a backend chosen at run time fits [`ClipboardHandle`].
#[cfg(feature = "clipboard")]
struct BoxedBackend(Box<dyn ClipboardBackend>);

#[cfg(feature = "clipboard")]
impl ClipboardBackend for BoxedBackend {
    fn get_text(&mut self) -> Result<String, String> {
        self.0.get_text()
    }
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.0.set_text(text)
    }
    fn has_text(&mut self) -> bool {
        self.0.has_text()
    }
    fn get_html(&mut self) -> Result<String, String> {
        self.0.get_html()
    }
    fn set_html(&mut self, html: &str, plain_fallback: &str) -> Result<(), String> {
        self.0.set_html(html, plain_fallback)
    }
    fn has_html(&mut self) -> bool {
        self.0.has_html()
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

/// The system clipboard for the display winit connected to, or an in-process
/// [`MemoryClipboard`] when none can be reached.
///
/// * **Wayland**: the core clipboard protocol (`wl_data_device`) on winit's own
///   connection, which every compositor implements, GNOME's included, and
///   which a sandbox with no X server can reach. The `ext`/`wlr` data-control
///   protocols `arboard` uses are not preferred even where they exist: they
///   are meant for clipboard managers, Mutter offers neither, and one code path
///   on every compositor is one path tested. `arboard` is tried only if the
///   core protocol cannot be set up.
/// * **X11, macOS, Windows**: [`ArboardClipboard`].
///
/// On Wayland a copy needs one of the application's windows to have keyboard
/// focus, as the core protocol requires: with none focused (every window
/// closed, or a copy made after the user switched to another application),
/// `set_text` and `set_html` return an error and the clipboard keeps what it
/// held. Pastes likewise see the current clipboard only while a window is
/// focused. On KDE and wlroots compositors, where `arboard` used data-control,
/// that is new.
///
/// When no system clipboard can be reached the reason is written to stderr
/// and copy and paste keep working inside this process only, so the next
/// "my clipboard is its own" report can be told apart in a log.
#[cfg(feature = "clipboard")]
pub fn system_clipboard(display: &winit::event_loop::OwnedDisplayHandle) -> ClipboardHandle {
    let wayland = {
        use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        display
            .display_handle()
            .is_ok_and(|handle| matches!(handle.as_raw(), RawDisplayHandle::Wayland(_)))
    };
    let opened = choose_backend(
        wayland,
        || open_wayland(display),
        || ArboardClipboard::new().map(|b| Box::new(b) as Box<dyn ClipboardBackend>),
    );
    for warning in &opened.warnings {
        eprintln!("teksilo-platform: {warning}");
    }
    match opened.backend {
        Some(backend) => ClipboardHandle::from_box(backend),
        None => ClipboardHandle::new(MemoryClipboard::new()),
    }
}

#[cfg(all(feature = "clipboard", unix, not(target_os = "macos")))]
fn open_wayland(
    display: &winit::event_loop::OwnedDisplayHandle,
) -> Result<Box<dyn ClipboardBackend>, String> {
    wayland::WaylandClipboard::new(display).map(|b| Box::new(b) as Box<dyn ClipboardBackend>)
}

#[cfg(all(feature = "clipboard", not(all(unix, not(target_os = "macos")))))]
fn open_wayland(
    _display: &winit::event_loop::OwnedDisplayHandle,
) -> Result<Box<dyn ClipboardBackend>, String> {
    Err("Wayland is not supported on this platform".into())
}

/// The backend [`choose_backend`] settled on, and what to say about the way
/// there.
#[cfg(feature = "clipboard")]
struct Chosen<B> {
    /// `None`: no system clipboard; the caller falls back to memory.
    backend: Option<B>,
    warnings: Vec<String>,
}

/// The fallback decision, apart from the backends themselves so it is tested
/// without a display: the core Wayland protocol on a Wayland display, then
/// `arboard`; `arboard` alone elsewhere; and every failure said out loud.
#[cfg(feature = "clipboard")]
fn choose_backend<B>(
    wayland: bool,
    open_wayland: impl FnOnce() -> Result<B, String>,
    open_arboard: impl FnOnce() -> Result<B, String>,
) -> Chosen<B> {
    let mut warnings = Vec::new();
    if wayland {
        match open_wayland() {
            Ok(backend) => {
                return Chosen {
                    backend: Some(backend),
                    warnings,
                };
            }
            Err(err) => warnings.push(format!(
                "the Wayland clipboard (wl_data_device) is unavailable: {err}; trying arboard"
            )),
        }
    }
    match open_arboard() {
        Ok(backend) => Chosen {
            backend: Some(backend),
            warnings,
        },
        Err(err) => {
            warnings.push(format!(
                "the system clipboard is unavailable ({err}); copy and paste work only inside this application"
            ));
            Chosen {
                backend: None,
                warnings,
            }
        }
    }
}

#[cfg(feature = "clipboard")]
mod arboard_backend {
    use super::ClipboardBackend;
    use arboard::{Clipboard, Error};

    /// Real system clipboard backed by `arboard`: the X11, macOS and Windows
    /// clipboard. [`system_clipboard`](super::system_clipboard) picks it for
    /// those, and the core Wayland protocol on a Wayland display.
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

    #[cfg(feature = "clipboard")]
    #[test]
    fn wayland_uses_the_core_protocol_and_says_nothing() {
        let chosen = choose_backend(true, || Ok("wayland"), || Ok("arboard"));
        assert_eq!(chosen.backend, Some("wayland"));
        assert!(chosen.warnings.is_empty());
    }

    #[cfg(feature = "clipboard")]
    #[test]
    fn x11_never_tries_the_wayland_protocol() {
        let chosen = choose_backend(
            false,
            || -> Result<&str, String> { panic!("Wayland opened on an X11 display") },
            || Ok("arboard"),
        );
        assert_eq!(chosen.backend, Some("arboard"));
        assert!(chosen.warnings.is_empty());
    }

    #[cfg(feature = "clipboard")]
    #[test]
    fn a_failed_wayland_clipboard_falls_back_to_arboard_and_says_so() {
        let chosen = choose_backend(true, || Err("no seat".into()), || Ok("arboard"));
        assert_eq!(chosen.backend, Some("arboard"));
        assert_eq!(chosen.warnings.len(), 1);
        assert!(
            chosen.warnings[0].contains("no seat"),
            "{:?}",
            chosen.warnings
        );
    }

    #[cfg(feature = "clipboard")]
    #[test]
    fn no_system_clipboard_at_all_is_reported_never_silent() {
        // The Flatpak case behind this backend: Wayland with no core-protocol
        // clipboard and no X server for arboard.
        let chosen: Chosen<&str> = choose_backend(
            true,
            || Err("no data device manager".into()),
            || Err("X11 server connection timed out".into()),
        );
        assert!(chosen.backend.is_none());
        assert_eq!(chosen.warnings.len(), 2);
        assert!(chosen.warnings[1].contains("only inside this application"));
        assert!(chosen.warnings[1].contains("X11 server connection timed out"));
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
