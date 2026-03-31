use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::TextBackend;

use crate::typesetter_bridge::TypesetterBridge;

/// Shared typesetter instance for single-threaded use across the widget tree.
#[derive(Clone)]
pub struct SharedTypesetter {
    inner: Rc<RefCell<TypesetterBridge>>,
}

impl SharedTypesetter {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(TypesetterBridge::new())),
        }
    }

    /// Create with the bundled default font.
    pub fn new_with_default_font() -> Self {
        Self {
            inner: Rc::new(RefCell::new(TypesetterBridge::new_with_default_font())),
        }
    }

    /// Get a reference to use as a TextBackend.
    pub fn as_text_backend(&self) -> Rc<RefCell<dyn TextBackend>> {
        self.inner.clone()
    }

    /// Access the inner bridge for font registration etc.
    pub fn bridge(&self) -> &Rc<RefCell<TypesetterBridge>> {
        &self.inner
    }

    /// Set the display scale factor for HiDPI glyph rasterization.
    pub fn set_scale_factor(&self, scale_factor: f32) {
        self.inner.borrow_mut().set_scale_factor(scale_factor);
    }
}

impl Default for SharedTypesetter {
    fn default() -> Self {
        Self::new()
    }
}
