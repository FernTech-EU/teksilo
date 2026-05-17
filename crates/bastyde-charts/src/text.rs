//! Shared text-measurement helpers.
//!
//! `Canvas::draw_text` truncates with "…" when the supplied rect width is
//! too tight to fit the laid-out glyphs (it sets `max_width = rect.width +
//! 0.5` for the text backend). Approximating widths from
//! `chars * font_size * 0.55` consistently underestimates real-typeface
//! widths, so axis labels like `"100"` or `"1000"` were rendering as `"…"`.
//!
//! These helpers go through the live text backend when one is wired and
//! pad by 1 pixel for sub-pixel headroom.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, TextBackend};
use bastyde_tokens::TextStyle;

/// Measure the actual rendered width of `text` in logical pixels using
/// the canvas's text backend, plus a 1 pixel sub-pixel headroom. Falls
/// back to a generous `chars * 0.7 * size + 4` estimate when no backend
/// is wired (headless tests).
pub fn measure_text_width(canvas: &mut Canvas, text: &str, style: &TextStyle) -> f32 {
    measure_text_width_via(canvas.text_backend(), text, style)
}

/// Same measurement as [`measure_text_width`] but takes the backend
/// directly so it can be called from layout-time paths that have a
/// `LayoutContext` (with `text_backend: Option<&Rc<RefCell<…>>>`) but
/// no `Canvas`. Both axis labels at paint time and legend-band size
/// reservation at layout time go through this same fallback so the
/// reserved space matches what later gets rendered.
pub fn measure_text_width_via(
    backend: Option<&Rc<RefCell<dyn TextBackend>>>,
    text: &str,
    style: &TextStyle,
) -> f32 {
    if let Some(b) = backend {
        b.borrow_mut().layout_single_line(text, style, None).width + 1.0
    } else {
        text.chars().count() as f32 * style.size * 0.7 + 4.0
    }
}
