// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The code editor answers a click at the point a screen reader derives from a
//! character's reported box with that character, as the rich text editor does
//! (see `rich_text::route_click_tests`): its runs go through the same emitter
//! and the same snap onto whole device pixels.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{MoveMode, TextDocument};

use super::CodeEditor;
use super::state::SharedState;
use crate::primitives::Padding;
use crate::rich_text::route_click_tests::{SCALES, TOPS, nvda_activation_point};

const TEXT: &str = "fn main() {\n    let answer = 42;\n    println!(\"{answer}\");\n    let more = answer * 2;\n}\n";

fn settle(tree: &mut WidgetTree) {
    for _ in 0..3 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(420.0, 300.0));
    }
    let _ = tree.render();
}

fn click(tree: &mut WidgetTree, at: Point) {
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    settle(tree);
}

fn caret_y(state: &SharedState, offset: usize) -> f32 {
    let st = state.borrow();
    super::keyboard::window_rect_at(&st, offset)
        .expect("the editor has laid out")
        .y
}

/// Routing onto the cursor's own cell leaves the caret where it was.
#[test]
fn activating_the_caret_leaves_it_where_it_is() {
    let mut failures = Vec::new();
    for scale in SCALES {
        for top in TOPS {
            // On the second, third and fourth lines: a click above the first
            // line still lands on it, so it cannot show the fault.
            for offset in [16, 30, 45, 60] {
                let doc = TextDocument::new();
                doc.set_plain_text(TEXT).unwrap();
                let editor = CodeEditor::new(doc);
                let state = editor.state.clone();
                let mut tree = WidgetTree::new();
                tree.set_device_scale_factor(scale);
                tree.add(Padding::new(top, 0.0, 0.0, 0.0).child(editor));
                settle(&mut tree);
                click(&mut tree, Point::new(60.0, top + 8.0));
                tree.advance_time(std::time::Duration::from_secs(2));
                state
                    .borrow()
                    .cursor
                    .set_position(offset, MoveMode::MoveAnchor);
                super::sync_cursor_signals(&state);
                settle(&mut tree);
                let before = caret_y(&state, offset);
                let at = nvda_activation_point(tree.sync_accessibility(), scale);
                click(&mut tree, at);
                let now = state.borrow().cursor.position();
                if now != offset {
                    failures.push(format!(
                        "scale {scale}, top {top}, offset {offset} (y {before}): click at {at:?} \
                         put the caret at {now} (y {})",
                        caret_y(&state, now)
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
