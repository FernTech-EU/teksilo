// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-widget catalog icons for the navigator palette and the
//! `teksilo-designer` outline tree.
//!
//! Each function returns a fresh `Box<dyn Widget>` (an [`IconWidget`]
//! built from a simple vector path on a 16×16 grid). Curated widgets'
//! `WidgetCatalog::icon()` impls return their glyph; widgets without a
//! dedicated glyph fall back to [`generic_widget_icon`] in the consumer
//! (the designer). Glyphs are intentionally minimal, fill-rule-agnostic
//! (built from separate solid sub-paths, never nested outlines), so they
//! read at small sizes regardless of the rasteriser's winding rule.

use teksilo_canvas::{Path, Point};
use teksilo_core::widget::Widget;
use teksilo_tokens::TextRole;

use crate::primitives::IconWidget;

/// The logical glyph box (matches the icon's intrinsic size).
const SZ: f32 = 16.0;

/// Append a solid axis-aligned rectangle as one closed sub-path.
fn rect(p: &mut Path, x: f32, y: f32, w: f32, h: f32) {
    p.move_to(Point::new(x, y));
    p.line_to(Point::new(x + w, y));
    p.line_to(Point::new(x + w, y + h));
    p.line_to(Point::new(x, y + h));
    p.close();
}

/// Append a hollow frame as four thin solid bars (fill-rule-agnostic).
fn frame(p: &mut Path, x: f32, y: f32, w: f32, h: f32, t: f32) {
    rect(p, x, y, w, t); // top
    rect(p, x, y + h - t, w, t); // bottom
    rect(p, x, y, t, h); // left
    rect(p, x + w - t, y, t, h); // right
}

/// Append a downward triangle (a chevron proxy) centred at `(cx, cy)`.
fn down_tri(p: &mut Path, cx: f32, cy: f32, s: f32) {
    p.move_to(Point::new(cx - s, cy - s * 0.5));
    p.line_to(Point::new(cx + s, cy - s * 0.5));
    p.line_to(Point::new(cx, cy + s * 0.5));
    p.close();
}

/// Build an icon widget from a path-building closure.
fn glyph(build: impl FnOnce(&mut Path)) -> Box<dyn Widget> {
    let mut p = Path::new();
    build(&mut p);
    Box::new(IconWidget::from_path(p, SZ).color(TextRole::Secondary))
}

/// Fallback icon for any widget without a dedicated glyph (a plain
/// rounded-square proxy — a solid square).
pub fn generic_widget_icon() -> Box<dyn Widget> {
    glyph(|p| rect(p, 3.0, 3.0, 10.0, 10.0))
}

// ── Layout primitives ──────────────────────────────────────────────

/// Three stacked horizontal bars.
pub fn vstack() -> Box<dyn Widget> {
    glyph(|p| {
        rect(p, 3.0, 3.0, 10.0, 2.5);
        rect(p, 3.0, 6.75, 10.0, 2.5);
        rect(p, 3.0, 10.5, 10.0, 2.5);
    })
}

/// Three side-by-side vertical bars.
pub fn hstack() -> Box<dyn Widget> {
    glyph(|p| {
        rect(p, 3.0, 3.0, 2.5, 10.0);
        rect(p, 6.75, 3.0, 2.5, 10.0);
        rect(p, 10.5, 3.0, 2.5, 10.0);
    })
}

/// Two offset overlapping squares.
pub fn zstack() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 3.0, 3.0, 8.0, 8.0, 1.5);
        frame(p, 5.0, 5.0, 8.0, 8.0, 1.5);
    })
}

/// A 2×2 grid of squares.
pub fn grid() -> Box<dyn Widget> {
    glyph(|p| {
        rect(p, 3.0, 3.0, 4.5, 4.5);
        rect(p, 8.5, 3.0, 4.5, 4.5);
        rect(p, 3.0, 8.5, 4.5, 4.5);
        rect(p, 8.5, 8.5, 4.5, 4.5);
    })
}

/// A frame with inset corner ticks (outer border = padding).
pub fn padding() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 2.0, 2.0, 12.0, 12.0, 1.5);
        frame(p, 5.5, 5.5, 5.0, 5.0, 1.0);
    })
}

/// A horizontal bar flanked by end caps (a double-arrow proxy).
pub fn expand() -> Box<dyn Widget> {
    glyph(|p| {
        rect(p, 4.0, 7.0, 8.0, 2.0);
        rect(p, 2.0, 4.0, 2.0, 8.0);
        rect(p, 12.0, 4.0, 2.0, 8.0);
    })
}

/// A small square centred in the glyph box.
pub fn center() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 2.0, 2.0, 12.0, 12.0, 1.0);
        rect(p, 6.0, 6.0, 4.0, 4.0);
    })
}

/// A dashed horizontal line (two short bars with a gap).
pub fn spacer() -> Box<dyn Widget> {
    glyph(|p| {
        rect(p, 2.0, 7.0, 4.5, 2.0);
        rect(p, 9.5, 7.0, 4.5, 2.0);
    })
}

// ── Controls ───────────────────────────────────────────────────────

/// A filled pill.
pub fn button() -> Box<dyn Widget> {
    glyph(|p| rect(p, 2.0, 5.5, 12.0, 5.0))
}

/// Three text lines of decreasing width.
pub fn text_widget() -> Box<dyn Widget> {
    glyph(|p| {
        rect(p, 3.0, 4.0, 10.0, 1.6);
        rect(p, 3.0, 7.2, 10.0, 1.6);
        rect(p, 3.0, 10.4, 6.0, 1.6);
    })
}

/// A box with an inner tick (checkbox).
pub fn checkbox() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 3.0, 3.0, 10.0, 10.0, 1.5);
        rect(p, 6.0, 6.0, 4.0, 4.0);
    })
}

/// A box with a caret (text field).
pub fn text_input() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 2.0, 5.0, 12.0, 6.0, 1.2);
        rect(p, 4.0, 6.5, 1.4, 3.0);
    })
}

/// A pill with a knob at the trailing end (toggle).
pub fn toggle() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 2.0, 5.5, 12.0, 5.0, 1.2);
        rect(p, 9.5, 6.5, 3.0, 3.0);
    })
}

/// A box with a downward chevron (combo box).
pub fn combo_box() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 2.0, 5.0, 12.0, 6.0, 1.2);
        down_tri(p, 11.0, 8.0, 2.0);
    })
}

/// A horizontal track with a centred knob (slider).
pub fn slider() -> Box<dyn Widget> {
    glyph(|p| {
        rect(p, 2.0, 7.2, 12.0, 1.6);
        rect(p, 6.5, 5.0, 3.0, 6.0);
    })
}

// ── Containers ─────────────────────────────────────────────────────

/// A frame with a header bar (card).
pub fn card() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 2.0, 2.0, 12.0, 12.0, 1.2);
        rect(p, 2.0, 2.0, 12.0, 3.0);
    })
}

/// A plain frame (panel).
pub fn panel() -> Box<dyn Widget> {
    glyph(|p| frame(p, 2.0, 2.0, 12.0, 12.0, 1.4))
}

/// A frame with a trailing scrollbar (scroll area).
pub fn scroll_area() -> Box<dyn Widget> {
    glyph(|p| {
        frame(p, 2.0, 2.0, 9.5, 12.0, 1.2);
        rect(p, 12.5, 3.0, 1.6, 6.0);
    })
}
