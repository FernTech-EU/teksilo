// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The grid painter: turns a [`GridSnapshot`] into `Canvas` draws (cell
//! backgrounds, glyphs, decorations, cursor, selection) against a
//! [`ColorScheme`]. Pure given its inputs; the widget supplies the measured
//! cell metrics and the reactive flags (focus / blink).

use bastyde_canvas::{Canvas, Point, Rect, StrokeStyle};
use bastyde_tokens::{Color, FontWeight, TextStyle};

use crate::color_scheme::ColorScheme;
use crate::engine::{Cell, GridSnapshot, TermCursorShape};

/// The fixed size of one terminal cell, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

/// Everything the painter needs for one frame.
pub struct RenderParams<'a> {
    pub snapshot: &'a GridSnapshot,
    pub scheme: &'a ColorScheme,
    pub metrics: CellMetrics,
    /// Top-left of the grid's content area (inside any padding).
    pub origin: Point,
    /// The monospace text style at the resolved size (weight is overridden
    /// per-cell for bold).
    pub base_font: &'a TextStyle,
    /// Whether the terminal currently has keyboard focus (drives cursor style
    /// and whether the block cursor is filled vs. hollow).
    pub focused: bool,
    /// Whether the cursor is in its "on" blink phase and should be drawn.
    pub cursor_on: bool,
    /// The resolved cursor shape (the app's configured preference, or the
    /// engine-reported shape). `Hidden` suppresses the cursor.
    pub cursor_shape: TermCursorShape,
}

/// Paint the whole grid.
pub fn paint_grid(canvas: &mut Canvas, p: &RenderParams<'_>) {
    let snap = p.snapshot;
    let cw = p.metrics.width;
    let ch = p.metrics.height;
    let cols = snap.columns;
    let rows = snap.screen_lines;
    if cw <= 0.0 || ch <= 0.0 || cols == 0 || rows == 0 {
        return;
    }

    // A regular-weight and a bold-weight variant of the base style, built once.
    let regular = p.base_font.clone();
    let bold = TextStyle {
        weight: FontWeight::BOLD,
        ..p.base_font.clone()
    };

    // Backdrop for the whole content area (covers inter-glyph gaps and any
    // fractional pixel at the trailing edge).
    canvas.fill_rect(
        Rect::new(p.origin.x, p.origin.y, cw * cols as f32, ch * rows as f32),
        p.scheme.background,
    );

    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = snap.cell(row, col) else {
                continue;
            };
            // The trailing half of a wide glyph is covered by its leading cell.
            if cell.attrs.wide_spacer {
                continue;
            }

            let selected = snap.is_selected(row, col);
            let (fg, bg) = resolved_colors(cell, p.scheme, selected);

            let span = if cell.attrs.wide { 2 } else { 1 };
            let x = p.origin.x + col as f32 * cw;
            let y = p.origin.y + row as f32 * ch;
            let w = cw * span as f32;

            // Background fill only when it differs from the backdrop.
            if bg != p.scheme.background {
                canvas.fill_rect(Rect::new(x, y, w, ch), bg);
            }

            // Glyph (skip blanks and concealed cells).
            if cell.ch != ' ' && cell.ch != '\0' && !cell.attrs.hidden {
                let style = if cell.attrs.bold { &bold } else { &regular };
                canvas.draw_text(&cell.text(), Rect::new(x, y, w, ch), style, fg);
            }

            // Line decorations.
            let thickness = (ch * 0.06).clamp(1.0, 3.0);
            if cell.attrs.underline {
                let uy = y + ch - thickness;
                line(canvas, x, uy, w, fg, thickness);
            }
            if cell.attrs.double_underline {
                let uy = y + ch - thickness * 2.5;
                line(canvas, x, uy, w, fg, thickness);
                line(canvas, x, y + ch - thickness, w, fg, thickness);
            }
            if cell.attrs.strikeout {
                line(canvas, x, y + ch * 0.5, w, fg, thickness);
            }
        }
    }

    // Cursor, drawn on top of the cell it occupies.
    if snap.cursor.visible && p.cursor_on {
        paint_cursor(canvas, p, cw, ch, &regular);
    }
}

/// Resolve a cell's final foreground/background, applying inverse video, dim,
/// concealment and selection highlighting.
fn resolved_colors(cell: &Cell, scheme: &ColorScheme, selected: bool) -> (Color, Color) {
    let mut fg = scheme.resolve(cell.fg, cell.attrs.bold);
    let mut bg = scheme.resolve(cell.bg, false);

    if cell.attrs.inverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.attrs.dim {
        fg = dim(fg);
    }
    if cell.attrs.hidden {
        fg = bg;
    }
    if selected {
        bg = scheme.selection_background;
        if let Some(sf) = scheme.selection_foreground {
            fg = sf;
        }
    }
    (fg, bg)
}

/// Reduce a colour's intensity for the SGR "dim/faint" attribute.
fn dim(c: Color) -> Color {
    Color::new(c.r() * 0.66, c.g() * 0.66, c.b() * 0.66, c.a())
}

fn line(canvas: &mut Canvas, x: f32, y: f32, w: f32, color: Color, thickness: f32) {
    canvas.draw_line(
        Point::new(x, y),
        Point::new(x + w, y),
        color,
        StrokeStyle::solid(thickness),
    );
}

fn paint_cursor(canvas: &mut Canvas, p: &RenderParams<'_>, cw: f32, ch: f32, font: &TextStyle) {
    let c = p.snapshot.cursor;
    let x = p.origin.x + c.column as f32 * cw;
    let y = p.origin.y + c.line as f32 * ch;
    let cursor_color = p.scheme.cursor;

    // A block cursor is hollow when the terminal is not focused.
    let shape = match (p.cursor_shape, p.focused) {
        (TermCursorShape::Block, false) => TermCursorShape::HollowBlock,
        (shape, _) => shape,
    };

    match shape {
        TermCursorShape::Block => {
            canvas.fill_rect(Rect::new(x, y, cw, ch), cursor_color);
            // Redraw the glyph under the cursor in the contrasting colour.
            if let Some(cell) = p.snapshot.cell(c.line, c.column)
                && cell.ch != ' '
                && cell.ch != '\0'
            {
                canvas.draw_text(
                    &cell.text(),
                    Rect::new(x, y, cw, ch),
                    font,
                    p.scheme.cursor_text,
                );
            }
        }
        TermCursorShape::HollowBlock => {
            let t = 1.0;
            canvas.fill_rect(Rect::new(x, y, cw, t), cursor_color);
            canvas.fill_rect(Rect::new(x, y + ch - t, cw, t), cursor_color);
            canvas.fill_rect(Rect::new(x, y, t, ch), cursor_color);
            canvas.fill_rect(Rect::new(x + cw - t, y, t, ch), cursor_color);
        }
        TermCursorShape::Beam => {
            canvas.fill_rect(Rect::new(x, y, 2.0, ch), cursor_color);
        }
        TermCursorShape::Underline => {
            canvas.fill_rect(Rect::new(x, y + ch - 2.0, cw, 2.0), cursor_color);
        }
        TermCursorShape::Hidden => {}
    }
}
