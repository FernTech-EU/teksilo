// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A click at the point a screen reader derives from a character's reported
//! box lands on that character, on that character's line.
//!
//! NVDA routes a braille cell onto the cell its cursor is already on by
//! "activating" the caret: it expands the caret to the character after it,
//! asks UIA for that character's bounding rectangles, truncates the first
//! one's left and top to integers (`RectLTWH.fromFloatCollection` is
//! `int()` of each double) and clicks there. A line top that falls inside
//! a device pixel reported a top whose truncation is the pixel row the line
//! above ends in, so the click moved the caret one line up.

use accesskit_consumer::Tree;
use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::Signal;
use teksilo_core::accesskit::{Role, TreeUpdate};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::{EditorHandle, RichTextEditor};
use crate::primitives::Padding;

const TEXT: &str = "A long paragraph that absolutely positively must wrap across \
     multiple visual lines, so that a reader has a line in the middle to \
     route on and a soft wrap to stand at the end of.";

const WIDTH: f32 = 300.0;

/// Where the window's client area sits on the screen, in device pixels.
/// Always whole pixels: `ClientToScreen` answers in integers.
const CLIENT: (f64, f64) = (113.0, 207.0);

struct Setup {
    tree: WidgetTree,
    handle: EditorHandle,
    scale: f32,
    /// How far down the window the editor sits.
    top: Signal<f32>,
}

impl Setup {
    /// An editor placed `top` logical pixels down the window, at `scale`.
    fn new(scale: f32, top: f32) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(TEXT).unwrap();
        let editor = RichTextEditor::editor(doc);
        let handle = editor.handle();
        let mut tree = WidgetTree::new();
        tree.set_device_scale_factor(scale);
        let top_signal = Signal::new(top);
        // The bottom gives back what the top takes, so moving the editor
        // never resizes it.
        let bottom = top_signal.map(|top| 20.0 - top);
        tree.add(Padding::new(top_signal.clone(), 0.0, bottom, 0.0).child(editor));
        let mut setup = Self {
            tree,
            handle,
            scale,
            top: top_signal,
        };
        setup.settle();
        // Focus the editor, well inside its first line, then leave enough
        // time that the reader's click is not read as a second click.
        setup.click(Point::new(5.0, top + 5.0));
        setup.tree.advance_time(std::time::Duration::from_secs(2));
        setup
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(SizeProposal::exact(WIDTH, 400.0));
        }
        let _ = self.tree.render();
    }

    fn update(&mut self) -> TreeUpdate {
        self.settle();
        self.tree.sync_accessibility()
    }

    fn click(&mut self, at: Point) {
        self.tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        self.tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        self.settle();
    }

    fn caret(&self) -> Rect {
        self.handle
            .caret_window_rect()
            .expect("a focused editor has a caret")
    }

    fn nvda_activation_point(&mut self) -> Point {
        let update = self.update();
        nvda_activation_point(update, self.scale)
    }
}

/// Where NVDA clicks to activate the caret of the text input in `update`, as a
/// window point in logical pixels: the top-left of the first box of the
/// character after the caret, on the screen, each coordinate truncated by
/// `int()` as `RectLTWH.fromFloatCollection` does.
pub(crate) fn nvda_activation_point(update: TreeUpdate, scale: f32) -> Point {
    let tree = Tree::new(update, true);
    let state = tree.state();
    let mut stack = vec![state.root()];
    let editor = loop {
        let node = stack.pop().expect("a text input in the tree");
        if node.role() == Role::MultilineTextInput {
            break node;
        }
        stack.extend(node.children());
    };
    let focus = editor
        .text_selection_focus()
        .expect("a focused editor reports its caret");
    // `ExpandToEnclosingUnit(TextUnit_Character)` on the collapsed caret.
    let mut range = focus.to_degenerate_range();
    range.set_end(focus.forward_to_character_end());
    let first = range.bounding_boxes()[0];
    let x = (first.x0 + CLIENT.0).trunc();
    let y = (first.y0 + CLIENT.1).trunc();
    // The click reaches the window in device pixels from the client origin,
    // and the tree in logical pixels.
    let scale = f64::from(scale);
    Point::new(
        ((x - CLIENT.0) / scale) as f32,
        ((y - CLIENT.1) / scale) as f32,
    )
}

/// The scales Windows offers, the fractional ones included, and placements
/// that put the text's line tops inside a device pixel.
pub(crate) const SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];
pub(crate) const TOPS: [f32; 4] = [0.0, 0.3, 7.6, 12.45];

/// Routing onto the cursor's own cell in the middle of a line leaves the caret
/// where it was: same offset, same line.
#[test]
fn activating_the_caret_mid_line_leaves_it_where_it_is() {
    let mut failures = Vec::new();
    for scale in SCALES {
        for top in TOPS {
            // Every line but the first: a click above the first line still
            // lands on it, so it cannot show the fault.
            for offset in [70, 75, 100, 130] {
                let mut s = Setup::new(scale, top);
                s.handle.select_range(offset, offset);
                s.settle();
                let before = s.caret();
                let at = s.nvda_activation_point();
                s.click(at);
                let after = s.caret();
                if s.handle.cursor_position() != offset || (after.y - before.y).abs() > 0.01 {
                    failures.push(format!(
                        "scale {scale}, top {top}, offset {offset}: click at {at:?} put the \
                         caret at {} on y {} (was y {})",
                        s.handle.cursor_position(),
                        after.y,
                        before.y
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// At the end of a wrapped line (End), the character after the caret is the
/// first of the next line, so the activation clicks there. The caret keeps
/// its offset and goes to that line's start, the other side of the same wrap:
/// never to the start of the line it was on, which is where a click on the
/// top row of that character used to land.
#[test]
fn activating_the_caret_at_a_wrap_keeps_its_offset() {
    let mut failures = Vec::new();
    for scale in SCALES {
        for top in TOPS {
            let mut s = Setup::new(scale, top);
            // The second line's start, then End: the caret at its wrap.
            s.handle.select_range(70, 70);
            s.settle();
            let line_start = s.caret();
            s.tree.dispatch_event(WidgetEvent::KeyDown {
                key: Key::Home,
                modifiers: Modifiers::NONE,
                text: None,
            });
            s.tree.dispatch_event(WidgetEvent::KeyDown {
                key: Key::End,
                modifiers: Modifiers::NONE,
                text: None,
            });
            s.settle();
            let wrap = s.handle.cursor_position();
            let at_end = s.caret();
            assert!(
                (at_end.y - line_start.y).abs() < 0.01,
                "End stays on the line"
            );
            let at = s.nvda_activation_point();
            s.click(at);
            let after = s.caret();
            if s.handle.cursor_position() != wrap || after.y <= at_end.y {
                failures.push(format!(
                    "scale {scale}, top {top}: click at {at:?} put the caret at {} on y {} \
                     (it was at {wrap} on y {})",
                    s.handle.cursor_position(),
                    after.y,
                    at_end.y
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// After the editor moves without changing (a scroll of the page around it),
/// its runs are re-placed rather than walked again, and the re-placed tops are
/// on whole device pixels too.
#[test]
fn activating_the_caret_after_the_editor_moved_leaves_it_where_it_is() {
    let mut failures = Vec::new();
    for scale in SCALES {
        let mut s = Setup::new(scale, 0.0);
        let offset = 100;
        s.handle.select_range(offset, offset);
        let _ = s.update();
        let walks = s.tree.a11y_walk_generation();
        s.top.set(7.6);
        let at = s.nvda_activation_point();
        assert_eq!(
            s.tree.a11y_walk_generation(),
            walks,
            "a move alone re-places the runs without a walk"
        );
        let before = s.caret();
        s.click(at);
        let after = s.caret();
        if s.handle.cursor_position() != offset || (after.y - before.y).abs() > 0.01 {
            failures.push(format!(
                "scale {scale}: click at {at:?} put the caret at {} on y {} (was y {})",
                s.handle.cursor_position(),
                after.y,
                before.y
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
