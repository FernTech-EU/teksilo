// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a blinking caret costs a window in which nothing else happens, for the
//! three text surfaces that share [`CaretBlink`](super::editor_runtime::CaretBlink).
//!
//! The loop below does what `teksilo-app`'s event loop does with a window: it
//! sleeps until the tree's next deadline (or until something needs drawing),
//! then runs a frame: layout, render, the accessibility refresh and, with a
//! reader attached, the delivery. It runs on the wall clock because the blink
//! does: a blink is a wake at a real `Instant`, which the tree's simulated
//! clock does not move.
//!
//! What each test holds the blink to: every frame of an idle window is one
//! caret toggle woken by its own deadline, repainting and nothing else (no
//! layout pass with work in it, no accessibility walk, nothing owed to a
//! reader).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use crate::code_editor::CodeEditor;
use crate::rich_text::RichTextEditor;
use crate::text_input::TextInput;

fn proposal() -> SizeProposal {
    SizeProposal::exact(400.0, 300.0)
}

/// A layout-transparent root that counts the layout passes that do work: a
/// pass that finds nothing dirty returns before it reaches any widget, and one
/// that does work starts from the roots.
#[derive(Debug)]
struct LayoutProbe {
    pending: Option<Box<dyn Widget>>,
    child: Option<WidgetId>,
    passes: Rc<Cell<usize>>,
}

impl Widget for LayoutProbe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending.take() {
            self.child = Some(ctx.add_boxed(pending));
        }
        self.child.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.passes.set(self.passes.get() + 1);
        self.child
            .and_then(|id| ctx.child_layout_response(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn cacheable_layout(&self) -> bool {
        false
    }
}

/// A window holding `surface`, focused, with a reader attached, after the
/// frames its focus asked for have run.
struct Window {
    tree: WidgetTree,
    layout_passes: Rc<Cell<usize>>,
}

impl Window {
    fn focused(surface: impl Widget + 'static) -> Self {
        let layout_passes = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let root = tree.add(LayoutProbe {
            pending: Some(Box::new(surface)),
            child: None,
            passes: layout_passes.clone(),
        });
        tree.set_at_client_attached(true);
        tree.layout(proposal());
        let _ = tree.render();
        for event in [
            WidgetEvent::pointer_down(
                Point::new(20.0, 15.0),
                PointerButton::Primary,
                Modifiers::NONE,
            ),
            WidgetEvent::pointer_up(
                Point::new(20.0, 15.0),
                PointerButton::Primary,
                Modifiers::NONE,
            ),
        ] {
            tree.dispatch_event(event);
        }
        assert!(
            tree.focused()
                .is_some_and(|f| f == root || tree.is_descendant_of(f, root)),
            "the click must focus the text surface"
        );
        let mut window = Self {
            tree,
            layout_passes,
        };
        // The frames the focus change asked for (the caret, the reader's
        // update), on their own pace, before anything is counted.
        let settle_until = Instant::now() + Duration::from_millis(150);
        while Instant::now() < settle_until {
            window.frame();
            std::thread::sleep(Duration::from_millis(5));
        }
        window
    }

    /// One frame, as the event loop runs it. Returns whether it repainted
    /// anything and whether a reader was owed an update.
    fn frame(&mut self) -> (bool, bool) {
        self.tree.layout(proposal());
        let repainted = self.tree.needs_render();
        let _ = self.tree.render();
        self.tree.refresh_accessibility();
        let owed = self.tree.accessibility_delivery_owed(true);
        if owed {
            let _ = self.tree.deliver_accessibility(true);
        }
        (repainted, owed)
    }

    /// Let the window idle for `span`, waking only when the tree asks to.
    fn idle(&mut self, span: Duration, caret: Option<&Signal<bool>>) -> Idle {
        let walks = self.tree.a11y_walk_generation();
        let at_version = self.tree.at_version().get();
        let passes = self.layout_passes.get();
        let mut idle = Idle::default();
        let mut shown = caret.map(Signal::get);
        let end = Instant::now() + span;
        loop {
            let now = Instant::now();
            if now >= end {
                break;
            }
            let deadline = self.tree.next_timer_deadline_source();
            if !self.tree.needs_render() && !deadline.is_some_and(|(at, _)| at <= now) {
                let wake = deadline.map_or(end, |(at, _)| at.min(end));
                std::thread::sleep(wake.saturating_duration_since(now));
                continue;
            }
            idle.frames += 1;
            if deadline.is_none_or(|(at, source)| at > now || source != "wake_at") {
                idle.not_woken_by_the_blink += 1;
            }
            let (repainted, owed) = self.frame();
            idle.repaints += usize::from(repainted);
            idle.owed += usize::from(owed);
            if let (Some(caret), Some(before)) = (caret, shown.as_mut()) {
                let now_shown = caret.get();
                if now_shown != *before {
                    idle.toggles += 1;
                    *before = now_shown;
                }
            }
        }
        idle.layout_passes = self.layout_passes.get() - passes;
        idle.walks = self.tree.a11y_walk_generation() - walks;
        idle.at_bumps = self.tree.at_version().get() - at_version;
        idle
    }
}

#[derive(Debug, Default)]
struct Idle {
    frames: usize,
    not_woken_by_the_blink: usize,
    repaints: usize,
    toggles: usize,
    layout_passes: usize,
    walks: u64,
    at_bumps: u64,
    owed: usize,
}

/// 1.2 s holds two or three toggles of a 500 ms blink, whatever its phase.
const SPAN: Duration = Duration::from_millis(1200);

fn assert_blink_floor(name: &str, idle: &Idle) {
    eprintln!("{name}: {idle:?}");
    assert!(
        (1..=3).contains(&idle.frames),
        "{name}: an idle window draws one frame per caret toggle, two a second; \
         {idle:?}"
    );
    assert_eq!(
        idle.not_woken_by_the_blink, 0,
        "{name}: every idle frame must be the blink's own wake; {idle:?}"
    );
    assert_eq!(
        idle.repaints, idle.frames,
        "{name}: each blink frame repaints the caret; {idle:?}"
    );
    assert_eq!(
        idle.layout_passes, 0,
        "{name}: a blink must not lay anything out; {idle:?}"
    );
    assert_eq!(
        (idle.walks, idle.at_bumps, idle.owed),
        (0, 0, 0),
        "{name}: a blink must not touch the accessibility tree; {idle:?}"
    );
}

fn rich_text() -> (RichTextEditor, Signal<bool>) {
    let doc = TextDocument::new();
    doc.set_plain_text("A first paragraph.\nA second one.")
        .unwrap();
    let editor = RichTextEditor::editor(doc);
    let caret = editor.state_handle().borrow().caret_visible.clone();
    (editor, caret)
}

fn code() -> (CodeEditor, Signal<bool>) {
    let doc = TextDocument::new();
    doc.set_plain_text("fn main() {}\n").unwrap();
    let editor = CodeEditor::new(doc);
    let caret = editor
        .handle()
        .state_handle()
        .borrow()
        .caret_visible
        .clone();
    (editor, caret)
}

#[test]
fn a_blinking_caret_in_a_rich_text_editor_only_repaints() {
    let (editor, caret) = rich_text();
    let mut window = Window::focused(editor);
    let idle = window.idle(SPAN, Some(&caret));
    assert_blink_floor("rich text", &idle);
    assert_eq!(idle.toggles, idle.frames, "one toggle a frame; {idle:?}");
}

#[test]
fn a_blinking_caret_in_a_code_editor_only_repaints() {
    let (editor, caret) = code();
    let mut window = Window::focused(editor);
    let idle = window.idle(SPAN, Some(&caret));
    assert_blink_floor("code", &idle);
    assert_eq!(idle.toggles, idle.frames, "one toggle a frame; {idle:?}");
}

#[test]
fn a_blinking_caret_in_a_text_input_only_repaints() {
    let mut window = Window::focused(TextInput::new(Signal::new("hello".to_string())));
    let idle = window.idle(SPAN, None);
    assert_blink_floor("text input", &idle);
}
