// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::TextBackend;

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

    /// Replay a [`FontRegistrar`](crate::FontRegistrar)'s faces into this
    /// typesetter's font service. Call before any text is shaped (e.g. at
    /// app startup) so a theme's font family resolves instead of silently
    /// falling back to the bundled default.
    pub fn apply_font_registrar(&self, registrar: &dyn crate::font_registrar::FontRegistrar) {
        let mut bridge = self.inner.borrow_mut();
        registrar.register_on_service(bridge.service_mut());
    }

    /// Set the display scale factor for HiDPI glyph rasterization.
    pub fn set_scale_factor(&self, scale_factor: f32) {
        self.inner.borrow_mut().set_scale_factor(scale_factor);
    }

    /// Set the language, as a BCP 47 tag (the UI locale), that Chinese
    /// characters with no kana or hangul around them, in a block that names
    /// no language, are drawn for: Japanese, Korean, Simplified or
    /// Traditional Chinese each have their own forms of them, and glyph
    /// fallback picks a font for one. `teksilo-app` keeps it on the active
    /// locale. A change marks the text laid out so far for layout again.
    pub fn set_default_language(&self, language: Option<&str>) {
        self.inner.borrow_mut().set_default_language(language);
    }

    /// Set how glyphs are fitted to the pixel grid: natural (exact spacing,
    /// fractional positions) or crisp (whole pixels, hinted on both axes,
    /// advances in whole pixels). A change marks the text laid out so far
    /// for layout again; the widget trees must be laid out again too
    /// (`WidgetTree::text_metrics_changed`). `teksilo-app` keeps it on what
    /// the application asks for, or on what the desktop asks for.
    pub fn set_glyph_rendering(&self, rendering: text_typeset::GlyphRendering) {
        self.inner.borrow_mut().set_glyph_rendering(rendering);
    }

    /// How glyphs are fitted to the pixel grid.
    pub fn glyph_rendering(&self) -> text_typeset::GlyphRendering {
        self.inner.borrow().service().glyph_rendering()
    }
}

impl Default for SharedTypesetter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_registrar::FontRegistrar;
    use std::cell::Cell;
    use text_typeset::{FontFaceId, TextFontService};

    struct FlagRegistrar(Rc<Cell<bool>>);
    impl FontRegistrar for FlagRegistrar {
        fn register_on_service(&self, _service: &mut TextFontService) -> Option<FontFaceId> {
            self.0.set(true);
            None
        }
    }

    #[test]
    fn apply_font_registrar_replays_into_service() {
        let ts = SharedTypesetter::new();
        let flag = Rc::new(Cell::new(false));
        ts.apply_font_registrar(&FlagRegistrar(flag.clone()));
        assert!(flag.get(), "registrar should be replayed into the service");
    }

    #[test]
    fn embedded_inter_registrar_loads_a_face() {
        // The bundled Inter registrar returns a real face id when applied.
        let ts = SharedTypesetter::new();
        let face = {
            let mut bridge = ts.bridge().borrow_mut();
            crate::font_registrar::EmbeddedInterRegistrar::new()
                .register_on_service(bridge.service_mut())
        };
        assert!(face.is_some(), "EmbeddedInterRegistrar should load Inter");
    }
}
