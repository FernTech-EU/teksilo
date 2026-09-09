// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer editing and the context menu, across all three faces.
//!
//! The **mouse** half is the invariant: a precise pointer reaches none of the
//! touch machinery, and Alt-click still adds a caret. The **touch** half is the
//! new behaviour — a release-time caret, a word selected by a hold, handles, and
//! the toolbar a read-only `LogView` offers. The **menu** half is a capability
//! all three faces simply did not have: a right-click bubbled past them, so
//! `Ctrl+C` was the only route to the clipboard.

use std::rc::Rc;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::overlay::SelectionHandleKind;
use teksilo_core::pointer::{PointerId, PointerPhase};
use teksilo_core::text_touch::TextAction;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;
use teksilo_tokens::PointerKind;

use super::state::SharedState;
use super::{CodeEditor, LogView, PlainTextEditor};
use crate::button::press_test_support::{finger, touch};
use crate::rich_text::touch_mount::EditorTouch;

fn viewport() -> SizeProposal {
    SizeProposal::exact(520.0, 560.0)
}

/// One of the three faces, mounted at a non-zero origin so every coordinate
/// assertion has to survive the window→local conversion the router applies.
struct Harness {
    tree: WidgetTree,
    state: SharedState,
    touch: Rc<EditorTouch>,
    editor: WidgetId,
    elsewhere: WidgetId,
}

impl Harness {
    fn mount(
        state: SharedState,
        touch: Rc<EditorTouch>,
        widget: impl teksilo_core::widget::Widget + 'static,
    ) -> Self {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        // 120 dp of vertical headroom, so a toolbar has somewhere to hang above
        // the selection instead of flipping down over the handles.
        let padded = tree.add(crate::primitives::Padding::symmetric(120.0, 20.0).child(widget));
        let other = tree.add(
            crate::primitives::FixedSize::new()
                .width(40.0)
                .height(20.0)
                .child(crate::primitives::RectWidget::new().focusable(true)),
        );
        tree.add(
            crate::primitives::VStack::new()
                .add_child(padded)
                .add_child(other),
        );
        tree.layout(viewport());
        // The engine lays out in `paint`, so a test that never renders
        // hit-tests an empty layout and every press lands at the document end.
        let _ = tree.render();
        let editor = tree
            .first_focusable_descendant(padded)
            .expect("the surface is focusable");
        let elsewhere = tree
            .first_focusable_descendant(other)
            .expect("somewhere else for the focus to go");
        let mut h = Self {
            tree,
            state,
            touch,
            editor,
            elsewhere,
        };
        h.tree.focus(h.editor);
        h.render();
        h
    }

    fn code(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let editor = CodeEditor::new(doc);
        let (state, touch) = (editor.state.clone(), editor.touch.clone());
        Self::mount(state, touch, editor)
    }

    fn plain(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let editor = PlainTextEditor::new(doc);
        let inner = editor.inner.as_ref().expect("the wrapper holds its editor");
        let (state, touch) = (inner.state.clone(), inner.touch.clone());
        Self::mount(state, touch, editor)
    }

    fn viewer(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let editor = CodeEditor::read_only(doc);
        let (state, touch) = (editor.state.clone(), editor.touch.clone());
        Self::mount(state, touch, editor)
    }

    fn log(lines: &[&str]) -> Self {
        let view = LogView::new();
        let handle = view.handle();
        let (state, touch) = (view.state.clone(), view.touch.clone());
        for line in lines {
            handle.append_line(line);
        }
        let mut h = Self::mount(state, touch, view);
        // The stream lands on the frame tick, not on the append.
        for _ in 0..3 {
            h.tree.request_frame();
            h.tree.tick_animations(std::time::Duration::from_millis(16));
            h.render();
        }
        h
    }

    fn render(&mut self) {
        self.tree.layout(viewport());
        let _ = self.tree.render();
    }

    /// The window point of the caret boundary before character `offset`.
    fn at(&self, offset: usize) -> Point {
        let st = self.state.borrow();
        let rect = super::keyboard::window_rect_at(&st, offset).expect("the surface has laid out");
        Point::new(rect.x + 0.5, rect.y + rect.height / 2.0)
    }

    fn selection(&self) -> std::ops::Range<usize> {
        let st = self.state.borrow();
        let a = st.cursor.anchor();
        let p = st.cursor.position();
        a.min(p)..a.max(p)
    }

    fn caret(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    fn caret_count(&self) -> usize {
        1 + self.state.borrow().extra_carets.len()
    }

    fn handles(&self) -> Vec<teksilo_core::text_touch::SelectionHandleGeometry> {
        self.touch.handles()
    }

    fn toolbar_actions(&self) -> Vec<TextAction> {
        let mut out = Vec::new();
        self.touch
            .with_toolbar_for_test(|t| out = t.actions.clone());
        out
    }

    fn mouse_press(&mut self, at: Point) {
        self.tree
            .pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
    }

    fn mouse_press_with(&mut self, at: Point, modifiers: teksilo_core::event::Modifiers) {
        self.tree
            .dispatch_event(teksilo_core::event::WidgetEvent::PointerDown {
                position: at,
                button: teksilo_core::event::PointerButton::Primary,
                modifiers,
            });
    }

    fn mouse_move(&mut self, at: Point) {
        self.tree
            .dispatch_event(teksilo_core::event::WidgetEvent::PointerMove { position: at });
    }

    fn mouse_release(&mut self, at: Point) {
        self.tree
            .pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
    }

    fn finger_down(&mut self, id: PointerId, at: Point, ms: u64) {
        self.tree
            .dispatch_pointer(touch(id, PointerPhase::Down, at, ms));
    }

    fn finger_move(&mut self, id: PointerId, at: Point, ms: u64) {
        self.tree
            .dispatch_pointer(touch(id, PointerPhase::Move, at, ms));
    }

    fn finger_up(&mut self, id: PointerId, at: Point, ms: u64) {
        self.tree
            .dispatch_pointer(touch(id, PointerPhase::Up, at, ms));
    }

    fn finger_tap(&mut self, at: Point) {
        let id = finger();
        self.finger_down(id, at, 0);
        self.finger_up(id, at, 30);
    }
}

// ---------------------------------------------------------------------------
// The mouse invariant
// ---------------------------------------------------------------------------

/// A mouse press places the caret at the press, before any release.
#[test]
fn a_mouse_press_places_the_caret_where_it_landed() {
    let mut h = Harness::code("fn main() { let x = 1; }");
    h.mouse_press(h.at(12));
    assert_eq!(h.caret(), 12);
    assert!(h.selection().is_empty());
}

/// Press, move, release still selects everything the pointer crossed — the
/// drag-select session a direct pointer no longer arms.
#[test]
fn a_mouse_drag_still_selects_the_text_it_crossed() {
    let mut h = Harness::code("fn main() { let x = 1; }");
    let from = h.at(3);
    let to = h.at(9);
    h.mouse_press(from);
    h.mouse_move(to);
    h.mouse_release(to);
    assert_eq!(h.selection(), 3..9);
}

/// Alt-click still adds a caret. It is the one press-time commitment this module
/// makes that has no direct-pointer form, so it is worth pinning that the
/// precise path kept it.
#[test]
fn an_alt_click_still_adds_a_caret() {
    let mut h = Harness::code("fn main() { let x = 1; }");
    h.mouse_press(h.at(2));
    assert_eq!(h.caret_count(), 1);
    h.mouse_press_with(h.at(14), teksilo_core::event::Modifiers::ALT);
    assert_eq!(
        h.caret_count(),
        2,
        "Alt-click adds a caret rather than moving"
    );
}

/// A mouse reaches **none** of the touch machinery. This is the assertion that
/// fails if the kind guard at the top of `handle_pointer_event` is removed.
#[test]
fn a_mouse_raises_no_affordances() {
    let mut h = Harness::code("fn main() { let x = 1; }");
    let from = h.at(3);
    let to = h.at(9);
    h.mouse_press(from);
    h.mouse_move(to);
    h.mouse_release(to);
    assert_eq!(h.selection(), 3..9, "it did select");
    assert!(
        h.handles().is_empty(),
        "…and raised no handles: {:?}",
        h.handles()
    );
    assert!(h.toolbar_actions().is_empty(), "nor a selection toolbar");
}

/// A mouse hold is not a word selection.
#[test]
fn a_mouse_hold_selects_no_word() {
    let mut h = Harness::code("fn main() { let x = 1; }");
    h.tree.long_press_at(PointerKind::Mouse, h.at(4));
    assert!(
        h.selection().is_empty(),
        "a mouse hold selected {:?}",
        h.selection()
    );
    assert!(h.handles().is_empty());
}

// ---------------------------------------------------------------------------
// Touch
// ---------------------------------------------------------------------------

/// A finger's press is not yet a click — the same contact is the opening sample
/// of a pan — so the caret is placed on the release.
#[test]
fn a_finger_places_the_caret_on_the_release_not_the_press() {
    let mut h = Harness::code("fn main() { let x = 1; }");
    let start = h.caret();
    let target = h.at(12);
    let id = finger();
    h.finger_down(id, target, 0);
    assert_eq!(h.caret(), start, "the press leaves the caret where it was");
    h.finger_up(id, target, 30);
    assert_eq!(h.caret(), 12, "the release is what places it");
}

/// A contact that travelled further than a tap of its kind may was panning, so
/// it places nothing. The framework's coarse tap boundary is the pressed node's
/// whole rectangle, which a finger panning a tall editor never leaves.
#[test]
fn a_finger_that_panned_places_no_caret() {
    let mut text = String::new();
    for i in 0..60 {
        text.push_str(&format!("let line_{i} = {i};\n"));
    }
    let mut h = Harness::code(&text);
    let start = h.caret();
    let from = h.at(4);
    let id = finger();
    h.finger_down(id, from, 0);
    let away = Point::new(from.x, from.y + 90.0);
    h.finger_move(id, away, 16);
    h.finger_up(id, away, 32);
    assert_eq!(h.caret(), start, "a pan must not move the caret");
    assert!(h.handles().is_empty(), "nor raise handles");
}

/// A hold selects the word under the finger and raises its two handles.
#[test]
fn a_finger_hold_selects_a_word_and_raises_its_handles() {
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(18));
    assert_eq!(h.selection(), 16..21, "the word under the finger");
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        vec![SelectionHandleKind::Start, SelectionHandleKind::End]
    );
    assert!(h.touch.toolbar_is_wanted());
}

/// The hold's own position is converted out of the surface's space into the
/// window space the controller works in: a `TapEvent` arrives widget-local, and
/// a long press has no sample to read a window position from. The point chosen
/// is inside the *second* identifier, so a hold that lost the origin selects
/// something else entirely rather than merely being off by a character.
#[test]
fn a_hold_selects_the_word_under_the_finger_and_not_one_beside_it() {
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(17));
    assert_eq!(h.selection(), 16..21);
}

/// A hold collapses a multi-caret set: a touch selection is one range, and the
/// rest of the editor — including what it reports to assistive technology —
/// assumes a single caret when a selection exists.
#[test]
fn a_hold_collapses_a_multi_caret_set() {
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
    let mut h = Harness::code("let value = 1;\nlet other = 2;\n");
    // Ctrl+Alt+Down adds a caret below, which is the keyboard route — and does
    // not spend a pointer press, so the hold that follows is the first one.
    h.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::COMMAND | Modifiers::ALT,
        text: None,
    });
    assert_eq!(h.caret_count(), 2, "fixture precondition: two carets");
    // "value" is 4..9 on the first line.
    h.tree.long_press_at(PointerKind::Touch, h.at(6));
    assert_eq!(h.caret_count(), 1, "the hold's range is one selection");
    assert_eq!(h.selection(), 4..9);
}

/// Dragging the end handle grows the selection to where the finger let go.
#[test]
fn dragging_the_end_handle_extends_the_selection() {
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(18));
    assert_eq!(h.selection(), 16..21);
    h.render();
    let end = h
        .handles()
        .into_iter()
        .find(|g| g.kind == SelectionHandleKind::End)
        .expect("an end handle is up");
    let target = Point::new(h.at(24).x, end.anchor.y);
    let contact = h.tree.new_contact();
    h.tree.touch_down(contact, end.anchor);
    h.tree.touch_move(contact, target);
    h.tree.touch_up(contact, target);
    assert_eq!(h.selection(), 16..24);
}

/// An editable surface with a selection offers all three clipboard commands.
#[test]
fn a_hold_on_a_code_editor_offers_cut_copy_and_paste() {
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(18));
    let actions = h.toolbar_actions();
    assert!(actions.contains(&TextAction::Cut), "{actions:?}");
    assert!(actions.contains(&TextAction::Copy), "{actions:?}");
    assert!(actions.contains(&TextAction::Paste), "{actions:?}");
}

/// A read-only code viewer offers Copy and nothing that changes the text.
#[test]
fn a_hold_on_a_read_only_viewer_offers_copy_only() {
    let mut h = Harness::viewer("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(18));
    assert_eq!(h.selection(), 16..21, "a viewer still selects");
    assert_eq!(h.toolbar_actions(), vec![TextAction::Copy]);
}

/// The plain-text face inherits the whole adoption — it is the same state with
/// wrapping on and the code affordances off.
#[test]
fn the_plain_text_face_gets_the_same_hold() {
    let mut h = Harness::plain("some notes about a thing");
    // Offsets: "some"=0..4, "notes"=5..10, "about"=11..16.
    h.tree.long_press_at(PointerKind::Touch, h.at(12));
    assert_eq!(h.selection(), 11..16, "the word under the finger");
    assert!(!h.handles().is_empty());
}

/// …and so does the **log view**, where it matters most: it is read-only, and
/// before this its only route to the clipboard was a chord, on a device that has
/// no `Ctrl`.
#[test]
fn a_hold_on_a_log_view_offers_copy() {
    let mut h = Harness::log(&["error while loading configuration", "second line"]);
    h.tree.long_press_at(PointerKind::Touch, h.at(6));
    assert!(
        !h.selection().is_empty(),
        "the hold selected nothing in the log"
    );
    assert_eq!(h.toolbar_actions(), vec![TextAction::Copy]);
}

/// A finger's tap raises the caret handle and no toolbar.
#[test]
fn a_finger_tap_raises_a_caret_handle_and_no_toolbar() {
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.finger_tap(h.at(12));
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(kinds, vec![SelectionHandleKind::Caret]);
    assert!(!h.touch.toolbar_is_wanted());
}

// ---------------------------------------------------------------------------
// Retirement
// ---------------------------------------------------------------------------

/// A cursor's press anywhere in the surface retires raised touch chrome. Nothing
/// else on a hybrid machine would: the affordance band is exempt from
/// outside-press dismissal.
#[test]
fn a_mouse_press_retires_the_touch_chrome() {
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(18));
    assert!(!h.handles().is_empty(), "the hold raised handles");
    h.mouse_press(h.at(2));
    assert!(
        h.handles().is_empty(),
        "a cursor's press left {:?} standing",
        h.handles()
    );
    assert!(!h.touch.toolbar_is_wanted());
}

/// Focus leaving retires them too.
#[test]
fn losing_focus_retires_the_touch_chrome() {
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(18));
    assert!(!h.handles().is_empty());
    h.tree.focus(h.elsewhere);
    h.render();
    assert!(h.handles().is_empty(), "blur left {:?}", h.handles());
}

/// A keystroke moves the caret without going through the controller.
#[test]
fn a_keystroke_refreshes_the_handles_it_did_not_place() {
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(18));
    assert_eq!(h.selection(), 16..21);
    h.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert!(h.selection().is_empty());
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(kinds, vec![SelectionHandleKind::Caret]);
}

// ---------------------------------------------------------------------------
// The IME area
// ---------------------------------------------------------------------------

/// A caret placed by a **pointer** reports the OS IME candidate area, exactly as
/// a caret moved by a key already did.
#[test]
fn a_pointer_caret_placement_reports_the_ime_area() {
    for finger_not_mouse in [false, true] {
        let mut h = Harness::code("fn main() { let value = 1; }");
        h.state.borrow_mut().last_ime_area = None;
        let target = h.at(12);
        if finger_not_mouse {
            h.finger_tap(target);
        } else {
            h.mouse_press(target);
        }
        let reported = h.state.borrow().last_ime_area;
        assert!(
            reported.is_some(),
            "finger={finger_not_mouse}: a pointer caret placement reported no IME area"
        );
        let expected = {
            let st = h.state.borrow();
            super::keyboard::window_rect_at(&st, 12).expect("caret 12 has geometry")
        };
        let got = reported.unwrap();
        assert!(
            (got.x - expected.x).abs() < 1.0 && (got.y - expected.y).abs() < 1.0,
            "finger={finger_not_mouse}: reported {got:?}, caret 12 is at {expected:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The context menu these three faces never had
// ---------------------------------------------------------------------------

mod menu {
    use super::super::policy::{CODE_EDITOR_PRESET, CODE_READ_ONLY_PRESET};
    use super::*;
    use crate::common::editor_runtime::{CommandFilter, PolicyBundle};

    /// An editable code editor offers the four commands its keyboard already
    /// had. One list, read by both the right-click menu and the touch toolbar,
    /// so neither can drift from the chords.
    #[test]
    fn a_code_editor_offers_cut_copy_paste_and_select_all() {
        assert_eq!(
            super::super::context_menu::offered(&CODE_EDITOR_PRESET),
            vec![
                TextAction::Cut,
                TextAction::Copy,
                TextAction::Paste,
                TextAction::SelectAll
            ]
        );
    }

    /// A read-only viewer — and a log view, which is the same policy — offers
    /// Copy and Select All only: its own `Ctrl+X` and `Ctrl+V` are refused by the
    /// very filter this asks.
    #[test]
    fn a_read_only_surface_offers_copy_and_select_all_only() {
        assert_eq!(
            super::super::context_menu::offered(&CODE_READ_ONLY_PRESET),
            vec![TextAction::Copy, TextAction::SelectAll]
        );
    }

    /// A host that vetoes a command through the filter removes it from both
    /// surfaces, not just from the chord. `ForwardOnly` is the shipped filter
    /// that refuses anything regressive.
    #[test]
    fn a_vetoed_command_is_offered_by_neither_surface() {
        let policy = PolicyBundle {
            command_filter: CommandFilter::ForwardOnly,
            ..CODE_EDITOR_PRESET
        };
        let offered = super::super::context_menu::offered(&policy);
        assert!(
            !offered.contains(&TextAction::Cut),
            "a forward-only surface offered Cut: {offered:?}"
        );
        assert!(offered.contains(&TextAction::Copy), "{offered:?}");
        assert!(offered.contains(&TextAction::Paste), "{offered:?}");
    }

    /// A right-click **opens** a menu — the capability all three faces lacked.
    /// Driven end to end through the framework's own right-click path, because
    /// that is what a factory being installed on the node actually means.
    #[test]
    fn a_right_click_opens_a_menu() {
        let mut h = Harness::code("fn main() { let value = 1; }");
        let before = h.tree.overlay_manager().active_ids().len();
        h.tree
            .pointer_down_button(h.at(4), teksilo_core::event::PointerButton::Secondary);
        assert!(
            h.tree.overlay_manager().active_ids().len() > before,
            "a right-click opened no menu"
        );
    }

    /// …including on a log view, whose only route to the clipboard used to be a
    /// chord.
    #[test]
    fn a_right_click_opens_a_menu_on_a_log_view() {
        let mut h = Harness::log(&["error while loading configuration"]);
        let before = h.tree.overlay_manager().active_ids().len();
        h.tree
            .pointer_down_button(h.at(4), teksilo_core::event::PointerButton::Secondary);
        assert!(
            h.tree.overlay_manager().active_ids().len() > before,
            "a right-click on a log opened no menu"
        );
    }

    /// `default_context_menu(false)` installs nothing, so a right-click bubbles
    /// past the surface for an application that renders its own.
    #[test]
    fn the_built_in_menu_can_be_turned_off() {
        let doc = TextDocument::new();
        doc.set_plain_text("fn main() { let value = 1; }").unwrap();
        let editor = CodeEditor::new(doc).default_context_menu(false);
        let (state, touch) = (editor.state.clone(), editor.touch.clone());
        let mut h = Harness::mount(state, touch, editor);
        let before = h.tree.overlay_manager().active_ids().len();
        h.tree
            .pointer_down_button(h.at(4), teksilo_core::event::PointerButton::Secondary);
        assert_eq!(
            h.tree.overlay_manager().active_ids().len(),
            before,
            "a suppressed menu opened anyway"
        );
    }

    /// A right-click **outside** the selection moves the caret to it, so Paste
    /// lands where the user pointed; one *inside* keeps the selection, so Cut and
    /// Copy act on it. Both halves, because either alone holds for the wrong
    /// reason.
    #[test]
    fn a_right_click_repositions_the_caret_only_when_it_lands_outside_the_selection() {
        let mut h = Harness::code("fn main() { let value = 1; }");
        select(&h, 16, 21);
        h.tree
            .pointer_down_button(h.at(18), teksilo_core::event::PointerButton::Secondary);
        assert_eq!(
            h.selection(),
            16..21,
            "a right-click inside the selection must keep it"
        );

        let mut h = Harness::code("fn main() { let value = 1; }");
        select(&h, 16, 21);
        h.tree
            .pointer_down_button(h.at(3), teksilo_core::event::PointerButton::Secondary);
        assert_eq!(h.caret(), 3, "and one outside it must move the caret");
        assert!(h.selection().is_empty());
    }

    /// A read-only surface is exempt from the repositioning: there is no caret to
    /// move, and moving the selection would destroy the one the reader was about
    /// to copy.
    #[test]
    fn a_right_click_in_a_viewer_leaves_its_selection_alone() {
        let mut h = Harness::viewer("fn main() { let value = 1; }");
        select(&h, 16, 21);
        h.tree
            .pointer_down_button(h.at(3), teksilo_core::event::PointerButton::Secondary);
        assert_eq!(h.selection(), 16..21);
    }

    fn select(h: &Harness, from: usize, to: usize) {
        let st = h.state.borrow();
        st.cursor
            .set_position(from, teksilo_text::text_document::MoveMode::MoveAnchor);
        st.cursor
            .set_position(to, teksilo_text::text_document::MoveMode::KeepAnchor);
    }
}

// ---------------------------------------------------------------------------
// The caret re-reveal after a viewport shrink
// ---------------------------------------------------------------------------

mod shrink {
    use super::*;

    /// A viewport whose **height is driven from outside**, so a test can shrink
    /// it. A surface inside a `VStack` is given its wanted height, which does not
    /// follow the window's, and the tree places a *root* at the whole viewport —
    /// so the sized box has to sit inside something.
    struct ShrinkHarness {
        tree: WidgetTree,
        state: SharedState,
        height: teksilo_core::signal::Signal<f32>,
    }

    impl ShrinkHarness {
        fn new(widget: impl teksilo_core::widget::Widget + 'static, state: SharedState) -> Self {
            let height = teksilo_core::signal::Signal::new(300.0_f32);
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
            let sized = tree.add(
                crate::primitives::FixedSize::new()
                    .width(400.0)
                    .height(height.clone())
                    .child(widget),
            );
            tree.add(crate::primitives::VStack::new().add_child(sized));
            tree.layout(SizeProposal::exact(480.0, 600.0));
            let _ = tree.render();
            let editor = tree
                .first_focusable_descendant(sized)
                .expect("the surface is focusable");
            tree.focus(editor);
            let mut h = Self {
                tree,
                state,
                height,
            };
            h.render();
            h
        }

        fn code(lines: usize) -> Self {
            let mut text = String::new();
            for i in 0..lines {
                text.push_str(&format!("let line_{i} = {i};\n"));
            }
            let doc = TextDocument::new();
            doc.set_plain_text(&text).unwrap();
            let editor = CodeEditor::new(doc);
            let state = editor.state.clone();
            let mut h = Self::new(editor, state);
            // Ctrl+End: the caret goes to the document's end and the editor
            // reveals it, putting a real scroll offset in place to be corrected.
            h.tree
                .dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
                    key: teksilo_core::event::Key::End,
                    modifiers: teksilo_core::event::Modifiers::COMMAND,
                    text: None,
                });
            h.render();
            h
        }

        fn render(&mut self) {
            self.tree.layout(SizeProposal::exact(480.0, 600.0));
            let _ = self.tree.render();
        }

        fn scroll(&self) -> f32 {
            self.state.borrow().scroll_y.get()
        }

        fn set_height(&mut self, h: f32) {
            self.height.set(h);
            self.render();
        }
    }

    /// A viewport that got **smaller** can leave the caret outside it — the
    /// on-screen keyboard rising under a focused editor is the case that makes
    /// this a correctness matter. `sync_viewport` records the shrink; the body's
    /// paint consumes it after the relayout the shrink forced.
    #[test]
    fn a_viewport_shrink_re_reveals_the_caret() {
        let mut h = ShrinkHarness::code(60);
        let tall = h.scroll();
        assert!(tall > 0.0, "fixture precondition: the caret is scrolled to");
        h.set_height(120.0);
        let short = h.scroll();
        assert!(
            short > tall,
            "a shrink must scroll further down to keep the caret in view: \
             {tall} -> {short}"
        );
        assert!(
            !h.state.borrow().pending_caret_reveal,
            "and the request must be consumed, not left to fire every frame"
        );
    }

    /// Growing the viewport does not drag the view back to the caret.
    #[test]
    fn a_viewport_growth_leaves_the_scroll_alone() {
        let mut h = ShrinkHarness::code(60);
        h.state.borrow().scroll_y.set(0.0);
        h.set_height(500.0);
        assert_eq!(h.scroll(), 0.0);
    }

    /// A **log view** never records a shrink. It has no caret, and pulling its
    /// scroll offset anywhere would fight its own follow-tail rule — which is
    /// derived from the scroll position, so a correction would silently turn
    /// following back on.
    #[test]
    fn a_log_view_records_no_caret_reveal() {
        let view = LogView::new();
        let handle = view.handle();
        let state = view.state.clone();
        for i in 0..200 {
            handle.append_line(&format!("line {i}"));
        }
        let mut h = ShrinkHarness::new(view, state);
        for _ in 0..4 {
            h.tree.request_frame();
            h.tree.tick_animations(std::time::Duration::from_millis(16));
            h.render();
        }
        h.set_height(120.0);
        assert!(
            !h.state.borrow().pending_caret_reveal,
            "a log view asked for a caret reveal it has no caret for"
        );
    }
}

// ---------------------------------------------------------------------------
// A pre-existing caret-reveal defect the shrink path uncovered
// ---------------------------------------------------------------------------

/// `Ctrl+End` scrolls to the caret it just moved.
///
/// A keyboard defect, not a touch one, and it is what made the shrink reveal
/// look broken: `ensure_caret_visible` asked the engine about its **cached**
/// cursor, which is refreshed at paint, so inside a key handler it still held
/// the caret from before the keystroke. For a motion that leaves the viewport in
/// one step the engine therefore answered "already visible" and scrolled
/// nothing, leaving a document-end jump showing the top of the file. Measured
/// before the fix: 0 dp of scroll for a 1033 dp document in a 300 dp viewport.
#[test]
fn ctrl_end_scrolls_to_the_document_end() {
    let mut text = String::new();
    for i in 0..60 {
        text.push_str(&format!("let line_{i} = {i};\n"));
    }
    let mut h = Harness::code(&text);
    assert_eq!(h.state.borrow().scroll_y.get(), 0.0, "starts at the top");
    h.tree
        .dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
            key: teksilo_core::event::Key::End,
            modifiers: teksilo_core::event::Modifiers::COMMAND,
            text: None,
        });
    let (scroll, viewport, content) = {
        let st = h.state.borrow();
        (
            st.scroll_y.get(),
            st.viewport_height,
            st.engine.content_height(),
        )
    };
    assert!(
        scroll > 0.0,
        "the caret went to the end and the view stayed at the top"
    );
    // Within one line of the bottom. The engine adds a reveal margin of its own
    // (measured: 10 dp), so pinning the exact offset would pin its constant
    // rather than the behaviour.
    let line = h.state.borrow().engine.default_line_height().max(1.0);
    assert!(
        (scroll - (content - viewport)).abs() <= line,
        "the end of a {content} dp document in a {viewport} dp viewport is \
         {} dp down; the view stopped at {scroll}, more than one {line} dp line away",
        content - viewport
    );
}

// ---------------------------------------------------------------------------
// The toolbar acts
// ---------------------------------------------------------------------------

/// Every node in `root`'s subtree, `root` first.
fn subtree(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        out.extend(tree.children(out[i]));
        i += 1;
    }
    out
}

/// Tapping the toolbar's **Cut** row cuts.
///
/// The row is not a mock: the toolbar and the right-click menu are built from the
/// same `row_for`, so this is the command a finger reaches. Cut is the one of the
/// four whose effect is visible without a platform clipboard — it removes the
/// text whether or not a handle is there to receive it, which is what the chord
/// already did.
#[test]
fn tapping_the_toolbars_cut_row_cuts_the_selection() {
    let mut h = Harness::code("fn main() { let value = 1; }");
    h.tree.long_press_at(PointerKind::Touch, h.at(18));
    assert_eq!(h.selection(), 16..21, "the hold selected the word");
    h.render();

    let toolbar = h
        .touch
        .toolbar_content()
        .expect("the toolbar content was built");
    assert!(
        h.tree.is_active(toolbar),
        "the hold must have raised the toolbar for its row to be reachable"
    );
    // The first active laid-out leaf: `offered` puts Cut first, and `item_when`
    // parks the rows a selection does not offer (Select All) dormant rather than
    // removing them.
    let row = subtree(&h.tree, toolbar)
        .into_iter()
        .filter(|&id| id != toolbar && h.tree.is_active(id))
        .find(|&id| {
            let b = h.tree.bounds(id);
            b.width > 1.0 && b.height > 1.0 && h.tree.children(id).is_empty()
        })
        .expect("the toolbar has at least one laid-out leaf");
    let bounds = h.tree.bounds(row);
    let centre = Point::new(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    );
    let id = finger();
    h.finger_down(id, centre, 0);
    h.finger_up(id, centre, 30);

    assert_eq!(
        h.state
            .borrow()
            .document
            .to_plain_text()
            .unwrap_or_default(),
        "fn main() { let  = 1; }",
        "the toolbar's Cut row did not cut"
    );
}

// ---------------------------------------------------------------------------
// Edge auto-scroll while a handle is dragged
// ---------------------------------------------------------------------------

/// Dragging a handle into the bottom edge band scrolls the surface, so a
/// selection can grow past the visible text.
///
/// Without it the drag stops at the last shaped line: the offset the controller
/// asks for is not on screen, so there is nothing for `offset_at` to answer with.
/// The band is the shared one every dragging view uses, and it widens for a
/// coarse pointer.
#[test]
fn dragging_a_handle_into_the_edge_band_scrolls_the_surface() {
    let mut text = String::new();
    for i in 0..60 {
        text.push_str(&format!("let line_{i} = {i};\n"));
    }
    let mut h = Harness::code(&text);
    // A word on the first line, so the End handle starts far from the bottom.
    h.tree.long_press_at(PointerKind::Touch, h.at(6));
    assert!(!h.selection().is_empty(), "the hold selected a word");
    h.render();
    let before = h.state.borrow().scroll_y.get();
    assert_eq!(before, 0.0, "fixture precondition: at the top");

    let end = h
        .handles()
        .into_iter()
        .find(|g| g.kind == SelectionHandleKind::End)
        .expect("an end handle is up");
    let viewport = {
        let st = h.state.borrow();
        teksilo_canvas::Rect::new(
            st.viewport_origin.x,
            st.viewport_origin.y,
            st.viewport_width,
            st.viewport_height,
        )
    };
    // Well inside the coarse band at the bottom edge.
    let into_band = Point::new(
        end.anchor.x,
        viewport.y + viewport.height - crate::common::drag_autoscroll::EDGE_BAND_COARSE / 4.0,
    );
    let contact = h.tree.new_contact();
    h.tree.touch_down(contact, end.anchor);
    h.tree.touch_move(contact, into_band);

    let during = h.state.borrow().scroll_y.get();
    assert!(
        during > before,
        "a handle dragged into the bottom band did not scroll: {before} -> {during}"
    );
    h.tree.touch_up(contact, into_band);
    assert!(
        h.selection().end > 9,
        "and the selection grew past the first line's word: {:?}",
        h.selection()
    );
}

/// A handle dragged in the **middle** of the viewport scrolls nothing. The band
/// is what makes the scroll deliberate; without this the whole viewport would be
/// one edge.
#[test]
fn dragging_a_handle_clear_of_the_bands_scrolls_nothing() {
    let mut text = String::new();
    for i in 0..60 {
        text.push_str(&format!("let line_{i} = {i};\n"));
    }
    let mut h = Harness::code(&text);
    h.tree.long_press_at(PointerKind::Touch, h.at(6));
    h.render();
    let before = h.state.borrow().scroll_y.get();
    let end = h
        .handles()
        .into_iter()
        .find(|g| g.kind == SelectionHandleKind::End)
        .expect("an end handle is up");
    let viewport = {
        let st = h.state.borrow();
        teksilo_canvas::Rect::new(
            st.viewport_origin.x,
            st.viewport_origin.y,
            st.viewport_width,
            st.viewport_height,
        )
    };
    let middle = Point::new(end.anchor.x, viewport.y + viewport.height / 2.0);
    assert!(
        middle.y - viewport.y > crate::common::drag_autoscroll::EDGE_BAND_COARSE
            && viewport.y + viewport.height - middle.y
                > crate::common::drag_autoscroll::EDGE_BAND_COARSE,
        "fixture precondition: the midpoint is clear of both bands"
    );
    let contact = h.tree.new_contact();
    h.tree.touch_down(contact, end.anchor);
    h.tree.touch_move(contact, middle);
    assert_eq!(
        h.state.borrow().scroll_y.get(),
        before,
        "a handle in the middle of the viewport scrolled the surface"
    );
}

/// A tap on a **read-only** surface raises no caret handle: there is no caret to
/// place, so a 44 dp target over the text would offer nothing. The rule is
/// `TextHitSource::is_editable`'s, in the controller — this is what pins that the
/// policy reaches it.
#[test]
fn a_tap_on_a_read_only_surface_raises_no_caret_handle() {
    let mut h = Harness::viewer("fn main() { let value = 1; }");
    h.finger_tap(h.at(12));
    assert!(h.handles().is_empty(), "a viewer raised {:?}", h.handles());
    // …but the tap still lands: with no selection, this surface's Copy takes the
    // caret's whole line, so the tap is what aims it.
    assert_eq!(h.caret(), 12);
}

/// An assistive client's `SetTextSelection` moves the selection, and the raised
/// handles follow it.
///
/// End to end through the node an AccessKit adapter actually addresses: a
/// `Role::TextRun` child, resolved back to a document offset through the
/// per-run synthetic map. Both halves are asserted — the selection landing on
/// the named run, and the handles the hold left behind being recomputed for it,
/// because either alone would hold for the wrong reason.
#[test]
fn an_at_set_text_selection_moves_the_selection_and_the_handles() {
    use teksilo_core::accesskit::{Action, ActionData, TextPosition, TextSelection};
    use teksilo_core::event::WidgetEvent;

    let mut h = Harness::code("alpha\nbravo\ncharlie");
    // A hold on the first line, so the handles start there.
    h.tree.long_press_at(PointerKind::Touch, h.at(2));
    assert_eq!(h.selection(), 0..5, "the hold selected the first word");
    let before = h
        .handles()
        .into_iter()
        .find(|g| g.kind == SelectionHandleKind::End)
        .expect("an end handle is up");

    // The a11y walk is what publishes the run map, and it runs on demand rather
    // than on every render — so take a snapshot to drive it.
    let _ = h.tree.accessibility_tree_snapshot();
    let (node, start) = {
        let st = h.state.borrow();
        let map = st.synthetic_to_element.borrow();
        let (node, run) = map
            .iter()
            .find(|(_, r)| r.text == "charlie")
            .expect("the 'charlie' run is mapped");
        (*node, run.absolute_start)
    };
    assert_eq!(start, 12, "'charlie' begins at document offset 12");

    h.tree.dispatch_event(WidgetEvent::AccessAction {
        action: Action::SetTextSelection,
        target: Some(h.editor),
        target_node: teksilo_core::accessibility::widget_id_to_node_id(h.editor),
        data: Some(ActionData::SetTextSelection(TextSelection {
            anchor: TextPosition {
                node,
                character_index: 0,
            },
            focus: TextPosition {
                node,
                character_index: 7,
            },
        })),
    });

    assert_eq!(
        h.selection(),
        12..19,
        "the AT selection must land on the run it named"
    );
    let after = h
        .handles()
        .into_iter()
        .find(|g| g.kind == SelectionHandleKind::End)
        .expect("an end handle is still up");
    assert!(
        (after.caret.y - before.caret.y).abs() > 1.0,
        "the end handle stayed on the first line at {:?} while the selection \
         moved to the third",
        after.caret
    );
}
