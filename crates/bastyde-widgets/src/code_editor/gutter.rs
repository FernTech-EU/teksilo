// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The line-number gutter.
//!
//! A sibling of the body, not a part of it: both read the same shared state, and
//! the composing wrapper places the gutter to the body's leading edge. Keeping
//! them separate is what lets the gutter be hidden from assistive technology
//! while the text beside it stays fully readable.
//!
//! # One number per logical line
//!
//! One block is one logical line, so a line that soft-wraps to three visual rows
//! still gets **one** number, at its first row. That is not a special case — it
//! falls out of asking the engine for the block's `y`, which is the top of the
//! whole block regardless of how many rows it occupies.
//!
//! # Not in the accessibility tree
//!
//! The gutter reports `set_hidden()` and nothing else. A screen-reader user does
//! want to know which line they are on, but not by hearing thirty numbers they
//! must arrow past to reach the text. Line position belongs on the *paragraph*
//! nodes as `position_in_set` / `size_of_set` — "line 42 of 200", spoken with
//! the line — which the accessibility walk supplies.
//!
//! `accesskit` offers no `Role::Line`, and `is_line_breaking_object` is a
//! schema-only flag no platform adapter reads, so there is no role-based
//! alternative to reach for.
//!
//! # What it must not do
//!
//! Ask the document anything per frame. `TextDocument::block_count()` documents
//! itself as "O(1) — reads cached value" and then fetches every block, reads each
//! one's content from the rope, and word-counts it before returning the cached
//! number — so a gutter sizing itself from that would word-count the document on
//! every layout. The count arrives on `DocumentEvent::BlockCountChanged` instead
//! and lives in `state.line_count`.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use super::state::SharedState;

/// Padding either side of the numbers, in logical pixels before text scale.
const GUTTER_PAD_LEADING: f32 = 8.0;
const GUTTER_PAD_TRAILING: f32 = 12.0;

/// Digits needed to write `n`. `0` and `1` both need one.
fn digits(n: usize) -> usize {
    let mut d = 1;
    let mut v = n;
    while v >= 10 {
        v /= 10;
        d += 1;
    }
    d
}

pub(crate) struct CodeGutter {
    state: SharedState,
}

impl std::fmt::Debug for CodeGutter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeGutter").finish_non_exhaustive()
    }
}

impl CodeGutter {
    pub(crate) fn new(state: &SharedState) -> Self {
        Self {
            state: state.clone(),
        }
    }
}

impl Widget for CodeGutter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use bastyde_core::binding::BindingLevel;
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        let st = self.state.borrow();

        // Scrolling moves the numbers but never resizes the gutter.
        st.scroll_y
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        // The caret's line is highlighted, so moving it repaints.
        st.cursor_position
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        // A new digit (999 -> 1000) widens the gutter, which is a relayout: the
        // body's width depends on ours. Everything else here is repaint-only.
        st.line_count
            .bind_to(self_id, registry, BindingLevel::Relayout);

        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let st = self.state.borrow();
        // Width from the TOTAL line count, never the visible maximum: sizing to
        // what is on screen would make the gutter breathe as the user scrolls
        // past line 99 into 100, shifting the text sideways under the caret.
        let widest = st.line_count.get().max(1);
        drop(st);

        let style = number_style(ctx.theme);
        // Measure the widest number that will ever be drawn, rather than
        // multiplying a digit width by a count — that would assume monospaced
        // digits, and the gutter has no say in which face the app chose.
        let label = "9".repeat(digits(widest));
        let text_w = match ctx.text_backend {
            Some(backend) => {
                backend
                    .borrow_mut()
                    .layout_single_line(&label, &style, None)
                    .width
            }
            // Headless: keep layout sane. A windowed app always has a backend.
            None => label.len() as f32 * 8.0,
        };
        let w = GUTTER_PAD_LEADING + text_w + GUTTER_PAD_TRAILING;
        let h = proposal.height.unwrap_or(0.0).max(0.0);
        Size::new(w, h).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let st = self.state.borrow();
        if !st.engine.has_full_layout() {
            // Nothing is laid out yet, so no line has a y. Painting numbers now
            // would put them at made-up positions for one frame.
            return;
        }

        let scroll_y = st.scroll_y.get();
        let line_h = st.engine.default_line_height().max(1.0);
        let total = st.line_count.get();
        if total == 0 || line_h <= 0.0 {
            return;
        }

        // Which lines are on screen. Derived arithmetically because the caller
        // that gets a gutter is the code editor, whose lines do not wrap and
        // carry no margins — so every block is exactly `line_h` tall and the
        // first visible line is a division rather than a search.
        //
        // A wrapped document breaks that assumption, which is why the gutter is
        // not offered on the wrapping face.
        let first = (scroll_y / line_h).floor().max(0.0) as usize;
        let visible = (bounds.height / line_h).ceil() as usize + 1;
        let last = (first + visible).min(total);

        let caret_line = caret_line_index(&st, line_h);
        let style = number_style(ctx.theme);
        let number_color = ctx.theme.colors.editor_gutter_fg;
        let current_color = ctx.theme.colors.editor_fg;

        // `draw_text` draws from the rect's left edge — it has no alignment of
        // its own — so right-aligning means measuring each label and placing it.
        // Worth the measure: numbers read as a column against the code's left
        // edge, and left-aligning would leave 9 and 100 at different distances
        // from the line each labels.
        let right_edge = bounds.x + bounds.width - GUTTER_PAD_TRAILING;

        canvas.set_clip(bounds);
        for line in first..last {
            let y = line as f32 * line_h - scroll_y + bounds.y;
            let label = (line + 1).to_string();

            let text_w = match canvas.text_backend() {
                Some(b) => {
                    b.borrow_mut()
                        .layout_single_line(&label, &style, None)
                        .width
                }
                None => return,
            };
            let slot = Rect::new(right_edge - text_w, y, text_w, line_h);

            canvas.draw_text(
                &label,
                slot,
                &style,
                // The caret's own number is brighter — the gutter answering
                // "where am I" at a glance.
                if Some(line) == caret_line {
                    current_color
                } else {
                    number_color
                },
            );
        }
        canvas.clear_clip();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational, in full. See the module docs: the line *number* is
        // conveyed on the paragraph nodes beside it, where a reader hears it
        // with the line instead of having to walk past it.
        builder.set_hidden();
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// Which line the primary caret is on, or `None` when it cannot be resolved.
fn caret_line_index(st: &super::state::CodeEditorState, line_h: f32) -> Option<usize> {
    let caret = st
        .engine
        .caret_rect(st.cursor.position(), st.cursor_affinity);
    let y = caret[1];
    if line_h <= 0.0 {
        return None;
    }
    Some((y / line_h).floor().max(0.0) as usize)
}

/// The style the numbers are drawn in.
///
/// Deliberately the theme's body style rather than the editor's own: the numbers
/// are chrome, and pinning them to the document's face would make them grow with
/// a display magnify that has nothing to do with them. The global accessibility text
/// scale still reaches them — the theme's typography is already scaled by it, so
/// they track the UI, which is what they are.
fn number_style(theme: &bastyde_core::styles::Theme) -> bastyde_tokens::TextStyle {
    theme.typography.body.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_count_grows_with_the_number() {
        assert_eq!(digits(0), 1);
        assert_eq!(digits(1), 1);
        assert_eq!(digits(9), 1);
        assert_eq!(digits(10), 2);
        assert_eq!(digits(99), 2);
        assert_eq!(digits(100), 3);
        assert_eq!(digits(99_999), 5);
        assert_eq!(digits(100_000), 6);
    }
}
