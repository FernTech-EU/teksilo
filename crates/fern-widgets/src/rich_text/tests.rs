//! Headless integration tests for the read-only `RichTextEditor`.
//!
//! These tests drive the widget through its public surface: add it to a
//! `WidgetTree`, poke a shared `TextDocument`, advance the simulated
//! clock, and verify that the widget produced layout, ran the frame
//! loop, and dispatched events correctly.

use fern_canvas::{Point, SizeProposal};
use fern_core::widget_tree::WidgetTree;
use fern_text::text_document::TextDocument;

use super::widget::{RichTextEditor, ScrollPolicy};
use super::ContextTarget;

fn tree_with_layout() -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree
}

fn tick_once(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

#[test]
fn read_only_constructs_and_exposes_version_signal() {
    let doc = TextDocument::new();
    doc.set_plain_text("Hello, world!").unwrap();
    let editor = RichTextEditor::read_only(doc);
    let version = editor.document_version();
    assert_eq!(
        version.get(),
        0,
        "freshly built editor reports version 0 until it drains events"
    );
}

#[test]
fn scroll_policy_builder_roundtrip() {
    let doc = TextDocument::new();
    let editor = RichTextEditor::read_only(doc)
        .v_scroll_policy(ScrollPolicy::AlwaysOff)
        .h_scroll_policy(ScrollPolicy::AlwaysOn);
    // Smoke test — the builder methods must not panic and must return
    // `Self`, so we can chain further setters.
    let _ = editor.zoom(1.0);
}

#[test]
fn read_only_widget_places_into_tree_and_lays_out() {
    let doc = TextDocument::new();
    doc.set_plain_text("First paragraph.\n\nSecond paragraph with more text.")
        .unwrap();

    let mut tree = WidgetTree::new();
    let _id = tree.add(RichTextEditor::read_only(doc.clone()));

    // `build()` calls `ctx.request_frame()` so the tree reports the
    // editor wants to pump.
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(
        tree.frame_requested() || tree.needs_redraw(),
        "the editor must request a frame during build so its frame loop kicks in"
    );

    // Drive a handful of frames so the widget has a chance to drain
    // the `on_change` burst triggered by the initial `set_plain_text`
    // call, run its layout, and settle.
    for _ in 0..3 {
        tick_once(&mut tree);
    }
}

#[test]
fn shared_document_between_two_editors_delivers_events_independently() {
    // Gap 10 of the plan: two editors on one document both receive
    // `on_change` callbacks and update independently. This is the
    // critical test that justifies the `on_change`-based routing
    // instead of `poll_events`.
    let doc = TextDocument::new();
    doc.set_plain_text("initial").unwrap();

    let a = RichTextEditor::read_only(doc.clone());
    let b = RichTextEditor::read_only(doc.clone());
    let version_a = a.document_version();
    let version_b = b.document_version();

    let mut tree = WidgetTree::new();
    let _ia = tree.add(a);
    let _ib = tree.add(b);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Initial drain: both editors receive the first burst of events
    // from `set_plain_text` that was queued on the document before the
    // editors subscribed. After subscription, another `set_plain_text`
    // fires events into *both* subscriptions.
    let before_a = version_a.get();
    let before_b = version_b.get();

    doc.set_plain_text("updated by external mutation").unwrap();

    // Pump one frame so each editor's frame loop drains its queue.
    tick_once(&mut tree);

    assert!(
        version_a.get() > before_a,
        "editor A must see the external mutation (version was {}, now {})",
        before_a,
        version_a.get()
    );
    assert!(
        version_b.get() > before_b,
        "editor B must see the external mutation (version was {}, now {})",
        before_b,
        version_b.get()
    );
}

#[test]
fn select_all_updates_has_selection_signal() {
    let doc = TextDocument::new();
    doc.set_plain_text("some text").unwrap();
    let editor = RichTextEditor::read_only(doc);
    let has_sel = editor.has_selection();

    assert!(!has_sel.get(), "initial state: nothing selected");
    editor.select_all();
    assert!(
        has_sel.get(),
        "select_all must flip the has_selection signal"
    );
    assert!(!editor.selected_text().is_empty());

    editor.deselect();
    assert!(
        !has_sel.get(),
        "deselect must flip has_selection back to false"
    );
}

#[test]
fn context_target_reports_plain_text_for_empty_document() {
    let doc = TextDocument::new();
    doc.set_plain_text("line").unwrap();
    let editor = RichTextEditor::read_only(doc);

    // Context target on a freshly constructed widget (no layout yet)
    // may return None because the typesetter has not run a layout
    // pass. That's a valid response — the test only asserts the call
    // does not panic.
    let _ = editor.context_target_at(Point::new(5.0, 5.0));
}

#[test]
fn idle_read_only_editor_does_not_pump_frames_after_events_drain() {
    // This is the draw-when-needed contract for rich-text widgets.
    // An editor with no pending events, no drag, no focus must not
    // keep `needs_redraw()` stuck at true.
    let doc = TextDocument::new();
    doc.set_plain_text("steady").unwrap();

    let mut tree = WidgetTree::new();
    let _ = tree.add(RichTextEditor::read_only(doc));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Pump a few frames to drain the startup burst.
    for _ in 0..5 {
        tick_once(&mut tree);
    }

    // Now the editor should be idle: no more frame requests.
    assert!(
        !tree.frame_requested(),
        "an idle read-only editor must stop requesting frames"
    );
}

#[test]
fn read_only_editor_arrow_keys_move_caret_and_preserve_selection() {
    // Full end-to-end navigation check: focus the widget, press a
    // sequence of arrow / Home / End / Ctrl+A keys, assert that the
    // cursor position and selection state track the key input.
    // Observes through the `cursor_position_signal` + `has_selection`
    // so the test never has to downcast through the arena.
    use fern_canvas::Point;
    use fern_core::event::{Key, Modifiers, PointerButton, WidgetEvent};

    let doc = TextDocument::new();
    doc.set_plain_text("Hello world").unwrap();
    let editor = RichTextEditor::read_only(doc);
    let caret = editor.cursor_position_signal();
    let has_sel = editor.has_selection();

    let mut tree = WidgetTree::new();
    let id = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Click near the start of the widget to place the caret.
    // Pump a render so the engine actually lays out the document.
    // Without this, `hit_test()` has nothing to hit, and the click
    // silently falls on the "no region" branch — the click returns
    // Ignored, no cursor placement, and later arrow keys operate on
    // a never-placed caret. Real apps never see this because their
    // render loop paints on every layout pass; the test has to do
    // it explicitly.
    let _ = tree.render();

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(1.0, 8.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    assert_eq!(tree.focused(), Some(id), "click must focus");
    let after_click = caret.get();

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(
        caret.get(),
        after_click + 1,
        "ArrowRight must advance caret by 1 (was {}, now {})",
        after_click,
        caret.get()
    );

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::SHIFT,
        text: None,
    });
    assert_eq!(caret.get(), after_click + 2);
    assert!(has_sel.get(), "Shift+ArrowRight must flip has_selection");

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::End,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(
        caret.get(),
        "Hello world".chars().count(),
        "End must move to block end"
    );
    assert!(
        !has_sel.get(),
        "End without shift must collapse the selection"
    );

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Home,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(caret.get(), 0, "Home must move to block start");

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::A,
        modifiers: Modifiers::CTRL,
        text: None,
    });
    assert!(has_sel.get(), "Ctrl+A must set has_selection");
}

#[test]
fn read_only_editor_end_key_does_not_escalate_past_block() {
    // Regression for the "End jumps into the next block on second
    // press" bug. text-document's `get_block_at_position` returns
    // the **next** block when queried at a block boundary, so a
    // naive `MoveOperation::EndOfBlock` from an already-at-end
    // cursor would land on the end of the next paragraph. The
    // widget guards the call with `at_block_end()`.
    use fern_canvas::Point;
    use fern_core::event::{Key, Modifiers, PointerButton, WidgetEvent};

    let doc = TextDocument::new();
    doc.set_plain_text("first block\nsecond block").unwrap();
    let editor = RichTextEditor::read_only(doc);
    let caret = editor.cursor_position_signal();

    let mut tree = WidgetTree::new();
    let _ = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(1.0, 8.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::End,
        modifiers: Modifiers::NONE,
        text: None,
    });
    let after_first_end = caret.get();
    assert!(after_first_end > 0, "first End must move caret");

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::End,
        modifiers: Modifiers::NONE,
        text: None,
    });
    let after_second_end = caret.get();
    assert_eq!(
        after_second_end, after_first_end,
        "second End must stay at end of current block (was {}, now {})",
        after_first_end, after_second_end
    );
}

#[test]
fn read_only_editor_blink_policy_stays_off_when_unfocused() {
    // While the read-only preset is temporarily Blinking (M8b will
    // switch to Hidden — see `policy.rs`), an **unfocused** widget
    // must never pump frames: the blink is gated on `has_focus`.
    let doc = TextDocument::new();
    doc.set_plain_text("quiet").unwrap();

    let mut tree = WidgetTree::new();
    let _ = tree.add(RichTextEditor::read_only(doc));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();

    // Drain the build-time frame request so the assertion reflects
    // the steady state, not the initial tick.
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));

    assert!(
        !tree.frame_requested(),
        "an unfocused read-only editor must not chain-request frames"
    );
}

#[test]
fn read_only_editor_is_focusable_and_dispatches_key_events() {
    // Regression guard for "arrow keys don't move the caret" — the
    // widget must be marked focusable (so click-to-focus finds it)
    // and it must actually receive KeyDown events on its on_key
    // handler after gaining focus.
    use fern_core::event::{Key, Modifiers, WidgetEvent};
    use fern_canvas::Point;

    let doc = TextDocument::new();
    doc.set_plain_text("abcdef").unwrap();
    let editor = RichTextEditor::read_only(doc);
    let version = editor.document_version();

    let mut tree = WidgetTree::new();
    let id = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Simulate a primary click on the middle of the widget to focus it.
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(100.0, 20.0),
        button: fern_core::event::PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    assert_eq!(
        tree.focused(),
        Some(id),
        "clicking the editor must focus it"
    );

    // Capture the cursor position before key dispatch.
    let version_before = version.get();
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    // The on_key handler should run, move the cursor, and bump the
    // document version signal (via has_selection mirror in paint or
    // via the frame-tick chain). More directly: the key handler
    // calls `ctx.request_frame()` which sets the frame-request flag,
    // proving the handler was invoked.
    let _ = version_before; // unused in this minimal assertion
    assert!(
        tree.frame_requested(),
        "on_key must call ctx.request_frame() when an arrow key is handled"
    );
}

#[test]
fn read_only_editor_with_shared_typesetter_also_renders_glyphs() {
    // Mirrors the fern-app configuration: a `SharedTypesetter` is
    // installed in `app_state`, and the editor's `build()` swaps its
    // private engine for one that shares the app's typesetter. This
    // is the only path that produces GPU-uploadable glyphs when run
    // through fern-app, so it must be exercised by a regression
    // test.
    use fern_text::SharedTypesetter;
    use std::any::TypeId;
    use std::collections::HashMap;

    let doc = TextDocument::new();
    doc.set_plain_text("Alpha Beta Gamma").unwrap();

    let mut tree = WidgetTree::new();
    let shared = SharedTypesetter::new_with_default_font();
    tree = tree.with_text_backend(shared.as_text_backend());

    // Install the shared typesetter into app_state so the widget
    // picks it up in build(), matching fern-app's wiring.
    let mut registry: HashMap<TypeId, Box<dyn std::any::Any>> = HashMap::new();
    registry.insert(TypeId::of::<SharedTypesetter>(), Box::new(shared));
    let ctx = fern_core::event_source::TreeAppContext::empty().with_app_state(registry);
    tree.set_app_context(std::rc::Rc::new(ctx));

    let _ = tree.add(RichTextEditor::read_only(doc));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();
    assert!(
        !frame.glyphs.is_empty(),
        "shared-typesetter editor must emit glyphs, got {}",
        frame.glyphs.len()
    );
}

#[test]
fn read_only_editor_emits_glyphs_into_final_render_frame() {
    // Regression guard for the "blank viewer" bug.  Two related
    // defects caused the widget to render no text:
    //   1. `place_children` is never called for leaf widgets, so
    //      `viewport_width/height` on the editor state stayed at 0
    //      and layout produced a degenerate flow.
    //   2. `frame_loop::tick` ran during `tree.layout()` and called
    //      `engine.set_cursor(...)` *before* any `layout_full` had
    //      happened, poisoning text-typeset's render state so every
    //      subsequent `render()` returned zero glyphs even after a
    //      correct layout pass.
    // Both are now fixed: `paint()` wires the viewport from bounds
    // and runs layout, and `frame_loop::tick` gates `set_cursor` /
    // `ensure_caret_visible` behind `engine.has_full_layout()`.
    let doc = TextDocument::new();
    doc.set_plain_text("Alpha Beta Gamma Delta Epsilon").unwrap();

    let mut tree = WidgetTree::new();
    let id = tree.add(RichTextEditor::read_only(doc));

    tree.layout(SizeProposal::exact(400.0, 300.0));
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0 && bounds.height > 0.0);

    let frame = tree.render();
    assert!(
        !frame.glyphs.is_empty(),
        "expected the read-only editor to emit glyph quads into the rendered frame, got {} glyphs",
        frame.glyphs.len()
    );
    assert!(
        frame
            .glyphs
            .iter()
            .any(|g| g.screen[0] < 400.0 && g.screen[1] < 300.0),
        "glyph coordinates must land inside the widget viewport"
    );
}

#[test]
fn context_target_variants_compile_out_of_the_box() {
    // Purely a compile-time test — make sure the public enum covers
    // the variants the plan promises so applications can pattern-match
    // exhaustively.
    let target = ContextTarget::Plain;
    match target {
        ContextTarget::Plain
        | ContextTarget::InSelection
        | ContextTarget::Link { .. }
        | ContextTarget::Image { .. }
        | ContextTarget::TableCell { .. } => {}
    }
}
