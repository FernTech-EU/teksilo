// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer editing on the rich text editor — both devices, in one file because
//! they are one behaviour with two answers.
//!
//! The **mouse** half is the invariant. `rich_text/tests.rs` already drives a
//! press, a drag and the multi-tap ladder, so those are not repeated; what is
//! here is the half nothing asserted before this package: that a precise pointer
//! reaches **none** of the touch machinery. "The mouse path is untouched" is only
//! a claim if something fails when it stops being true.
//!
//! The **touch** half is the new behaviour: a release-time caret, a word
//! selected by a hold, handles in the affordance band, a link a tap can follow,
//! and the toolbar a read-only viewer offers.

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
use super::touch_mount::EditorTouch;
use super::{EditorHandle, RichTextEditor};
use crate::button::press_test_support::{finger, touch};

/// The editor sits at (20, 120) so every coordinate assertion has to survive the
/// window→local conversion the router applies. An editor at the origin cannot
/// tell a correct conversion from a missing one.
fn origin() -> Point {
    Point::new(20.0, 120.0)
}

fn viewport() -> SizeProposal {
    SizeProposal::exact(480.0, 520.0)
}

struct Harness {
    tree: WidgetTree,
    state: SharedState,
    handle: EditorHandle,
    touch: Rc<EditorTouch>,
    editor: WidgetId,
    /// Somewhere else for the focus to go.
    elsewhere: WidgetId,
}

impl Harness {
    fn new(editor: RichTextEditor) -> Self {
        let handle = editor.handle();
        let state = editor.state.clone();
        let touch = editor.touch.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        // 120 dp of vertical headroom, so the toolbar has somewhere to hang
        // above the selection instead of flipping down over the handles.
        let padded = tree.add(crate::primitives::Padding::symmetric(120.0, 20.0).child(editor));
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
            .expect("the editor is focusable");
        let elsewhere = tree
            .first_focusable_descendant(other)
            .expect("somewhere else for the focus to go");
        let mut h = Self {
            tree,
            state,
            handle,
            touch,
            editor,
            elsewhere,
        };
        h.render();
        h
    }

    fn editable(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        Self::new(RichTextEditor::editor(doc))
    }

    fn viewer(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        Self::new(RichTextEditor::read_only(doc))
    }

    fn focused(text: &str) -> Self {
        let mut h = Self::editable(text);
        h.tree.focus(h.editor);
        h.render();
        h
    }

    fn render(&mut self) {
        self.tree.layout(viewport());
        let _ = self.tree.render();
    }

    /// The window point of the caret boundary before character `offset`.
    fn at(&self, offset: usize) -> Point {
        let rect = self
            .handle
            .offset_rect(offset)
            .expect("the editor has laid out");
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

    fn handles(&self) -> Vec<teksilo_core::text_touch::SelectionHandleGeometry> {
        self.touch.handles()
    }

    fn handle_of(
        &self,
        kind: SelectionHandleKind,
    ) -> Option<teksilo_core::text_touch::SelectionHandleGeometry> {
        self.handles().into_iter().find(|h| h.kind == kind)
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

    fn mouse_move(&mut self, at: Point) {
        self.tree
            .dispatch_event(teksilo_core::event::WidgetEvent::pointer_move(at));
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

    /// A hold of `kind` at a window point, **laid out**.
    ///
    /// The `render()` is not tidiness. The affordances the hold raises live in a
    /// `FullViewport` overlay, and until a layout runs its bounds are still zero
    /// — so a press dispatched before one is not routed through the overlay at
    /// all, and any test that then asserts the surface answered it is answering
    /// for the wrong mechanism. Every hold in this file goes through here so that
    /// cannot happen by omission.
    fn hold_at(&mut self, kind: PointerKind, at: Point) {
        self.tree.long_press_at(kind, at);
        self.render();
    }

    /// Press, then release, at the same point.
    fn finger_tap(&mut self, at: Point) {
        let id = finger();
        self.finger_down(id, at, 0);
        self.finger_up(id, at, 30);
    }
}

// ---------------------------------------------------------------------------
// The mouse invariant
// ---------------------------------------------------------------------------

/// A mouse press places the caret at the press — before any release, and before
/// any gesture is recognised. That is the click convention this package
/// deliberately does not move for a precise pointer.
#[test]
fn a_mouse_press_places_the_caret_where_it_landed() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    let target = h.at(5);
    h.mouse_press(target);
    assert_eq!(h.caret(), 5, "the caret follows the press, not the release");
    assert!(h.selection().is_empty(), "a bare press selects nothing");
}

/// Press, move, release still selects everything the pointer crossed — the
/// drag-select session a direct pointer no longer arms.
#[test]
fn a_mouse_drag_still_selects_the_text_it_crossed() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    let from = h.at(2);
    let to = h.at(9);
    h.mouse_press(from);
    h.mouse_move(to);
    h.mouse_release(to);
    assert_eq!(h.selection(), 2..9);
}

/// A mouse reaches **none** of the touch machinery: no handles, no lens, no
/// toolbar. This is what "the mouse path is untouched" means, and it is the
/// assertion that fails if the kind guard in `handle_pointer_event` is removed.
#[test]
fn a_mouse_raises_no_affordances() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    let from = h.at(2);
    let to = h.at(9);
    h.mouse_press(from);
    h.mouse_move(to);
    h.mouse_release(to);
    assert_eq!(h.selection(), 2..9, "it did select");
    assert!(
        h.handles().is_empty(),
        "…and it raised no handles: {:?}",
        h.handles()
    );
    assert!(h.toolbar_actions().is_empty(), "nor a selection toolbar");
}

/// A mouse hold is not a word selection. The gesture arena installs a
/// long-press recognizer on the *presence* of the handler, with no pointer-kind
/// condition, so the controller's own guard is the only thing between a resting
/// mouse button and a selected word.
#[test]
fn a_mouse_hold_selects_no_word() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    let target = h.at(8);
    h.hold_at(PointerKind::Mouse, target);
    assert!(
        h.selection().is_empty(),
        "a mouse hold selected {:?}",
        h.selection()
    );
    assert!(h.handles().is_empty(), "and raised handles");
}

// ---------------------------------------------------------------------------
// Touch: the caret
// ---------------------------------------------------------------------------

/// A finger's press is not yet a click — the same contact is the opening sample
/// of a pan — so the caret is placed on the release.
#[test]
fn a_finger_places_the_caret_on_the_release_not_the_press() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    let start = h.caret();
    let target = h.at(5);
    let id = finger();
    h.finger_down(id, target, 0);
    assert_eq!(
        h.caret(),
        start,
        "the press must leave the caret where it was"
    );
    h.finger_up(id, target, 30);
    assert_eq!(h.caret(), 5, "the release is what places it");
}

/// …and a contact that travelled further than a tap of its kind may was panning
/// *this* surface, so it places nothing.
///
/// This is the half `release_completes_the_press` cannot answer on its own. For a
/// coarse pointer the framework's tap boundary is the pressed node's whole
/// rectangle — right for a control, wrong for a text surface, whose target is a
/// character — and the editor is both the press's owner and the pan's claimant,
/// so nothing is ever "claimed elsewhere". Without the surface's own radius
/// check, a finger could pan a tall editor from the top of the text to the
/// bottom and land a caret wherever it stopped.
#[test]
fn a_finger_that_panned_places_no_caret() {
    let mut lines = String::new();
    for i in 0..60 {
        lines.push_str(&format!("line {i} of a document tall enough to scroll\n"));
    }
    let mut h = Harness::focused(&lines);
    let start = h.caret();
    let from = h.at(5);
    let id = finger();
    h.finger_down(id, from, 0);
    // Well past the touch profile's tap slop, and vertical, so it reads as a pan
    // — and deliberately still *inside* the editor's own rectangle, which is
    // what makes the framework's boundary say the press never left.
    let away = Point::new(from.x, from.y + 90.0);
    h.finger_move(id, away, 16);
    h.finger_up(id, away, 32);
    assert_eq!(h.caret(), start, "a pan must not move the caret");
    assert!(h.handles().is_empty(), "nor raise handles");
}

/// A finger's tap raises the caret handle — the "paste here" affordance — and no
/// toolbar: a menu opening on every tap in a text surface is not what any
/// platform does.
#[test]
fn a_finger_tap_raises_a_caret_handle_and_no_toolbar() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.finger_tap(h.at(5));
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(kinds, vec![SelectionHandleKind::Caret]);
    assert!(
        !h.touch.toolbar_is_wanted(),
        "a tap asks for a caret, not a menu"
    );
}

// ---------------------------------------------------------------------------
// Touch: the hold
// ---------------------------------------------------------------------------

/// A hold selects the word under the finger and raises the two handles that
/// adjust it.
#[test]
fn a_finger_hold_selects_a_word_and_raises_its_handles() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    assert_eq!(h.selection(), 6..11, "the word under the finger");
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        vec![SelectionHandleKind::Start, SelectionHandleKind::End],
        "a range gets two handles and no caret handle"
    );
    assert!(
        h.touch.toolbar_is_wanted(),
        "a hold is a deliberate selection, so it asks for the toolbar"
    );
}

/// The hold's own position is converted out of the editor's space into the window
/// space the controller works in.
///
/// A `TapEvent`'s position arrives widget-**local**, like every other pointer
/// coordinate the router hands a handler, while `TextHitSource` is stated
/// entirely in window coordinates — and a long press has no sample to read a
/// window position from, because it is recognised by a timer. The point chosen
/// here is a few characters into the *second* word, so a hold that lost the
/// editor's origin on the way in selects the first one instead of merely being
/// off by a character.
#[test]
fn a_hold_selects_the_word_under_the_finger_and_not_one_beside_it() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(7));
    assert_eq!(h.selection(), 6..11, "the second word, not the first");
}

/// …and the handles land on the caret they mark, in window coordinates. A host
/// that forwarded the router's widget-local positions unconverted would put
/// every handle one viewport origin away from its text.
#[test]
fn a_raised_handle_sits_at_the_caret_it_marks_in_window_space() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    let start = h
        .handle_of(SelectionHandleKind::Start)
        .expect("a start handle is up");
    let expected = h.handle.offset_rect(6).expect("caret 6 has geometry");
    assert!(
        (start.caret.x - expected.x).abs() < 0.5,
        "start handle caret at {:?}, offset 6 is at {expected:?}",
        start.caret
    );
    assert!(
        start.caret.x > origin().x,
        "a handle at {:?} lost the editor's origin (which is {:?})",
        start.caret,
        origin()
    );
}

/// The release that follows a hold must not place a caret over the word the hold
/// just selected. The hold fires from the gesture timer, *before* the finger
/// lifts, so without the spent-press record every hold would end as a tap.
#[test]
fn the_release_after_a_hold_does_not_collapse_its_word() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    assert_eq!(h.selection(), 6..11, "the hold selected the word");
}

// ---------------------------------------------------------------------------
// Touch: dragging a handle
// ---------------------------------------------------------------------------

/// Dragging the end handle grows the selection to where the finger let go.
#[test]
fn dragging_the_end_handle_extends_the_selection() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    assert_eq!(h.selection(), 6..11);
    h.render();
    let end = h
        .handle_of(SelectionHandleKind::End)
        .expect("an end handle is up");
    let target = Point::new(h.at(15).x, end.anchor.y);
    let contact = h.tree.new_contact();
    h.tree.touch_down(contact, end.anchor);
    h.tree.touch_move(contact, target);
    h.tree.touch_up(contact, target);
    assert_eq!(
        h.selection(),
        6..15,
        "the range follows the handle to where it was dropped"
    );
    assert!(
        h.touch.toolbar_is_wanted(),
        "letting go of a chosen range is when its commands belong on screen"
    );
}

// ---------------------------------------------------------------------------
// Touch: the toolbar's contents
// ---------------------------------------------------------------------------

/// An editable surface with a selection offers all four commands.
#[test]
fn a_hold_on_an_editable_editor_offers_cut_copy_and_paste() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    let actions = h.toolbar_actions();
    assert!(actions.contains(&TextAction::Cut), "{actions:?}");
    assert!(actions.contains(&TextAction::Copy), "{actions:?}");
    assert!(actions.contains(&TextAction::Paste), "{actions:?}");
    assert!(
        !actions.contains(&TextAction::SelectAll),
        "Select All is for a surface with nothing selected: {actions:?}"
    );
}

/// A **viewer** offers Copy and nothing that would change the text. The whole
/// derivation is `TextHitSource::is_editable` plus `allows_copy`, so this is what
/// pins that the read-only policy reaches the controller at all.
#[test]
fn a_hold_on_a_viewer_offers_copy_only() {
    let doc = TextDocument::new();
    doc.set_plain_text("hello world and a longer line of prose")
        .unwrap();
    let mut h = Harness::new(RichTextEditor::read_only(doc));
    h.tree.focus(h.editor);
    h.render();
    h.hold_at(PointerKind::Touch, h.at(8));
    assert_eq!(h.selection(), 6..11, "a viewer still selects");
    assert_eq!(h.toolbar_actions(), vec![TextAction::Copy]);
}

// ---------------------------------------------------------------------------
// Touch: links
// ---------------------------------------------------------------------------

/// A finger's tap follows a link with **no modifier**, in the editable face too.
/// There is no Ctrl to hold on a touch screen, so the precise pointer's
/// "Ctrl(⌘)+click follows, a plain click edits" split has no direct form.
#[test]
fn a_finger_tap_follows_a_link_in_an_editable_editor() {
    let followed = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let doc = TextDocument::new();
    doc.set_html(r#"<p><a href="https://example.invalid/">teksilo link text</a></p>"#)
        .expect("a link");
    let sink = followed.clone();
    let mut h = Harness::new(
        RichTextEditor::editor(doc).on_link_activated(move |href, _ctx| {
            sink.borrow_mut().push(href.to_string());
        }),
    );
    h.tree.focus(h.editor);
    h.render();
    h.finger_tap(h.at(3));
    assert_eq!(
        followed.borrow().as_slice(),
        ["https://example.invalid/"],
        "a tap on a link must follow it"
    );
}

/// …and a **mouse**'s plain click on the same link still does not, because the
/// writer clicking their own link is far more often trying to edit its text.
/// Both halves are asserted: either alone would hold for the wrong reason.
#[test]
fn a_plain_mouse_click_on_a_link_in_an_editable_editor_places_a_caret_instead() {
    let followed = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let doc = TextDocument::new();
    doc.set_html(r#"<p><a href="https://example.invalid/">teksilo link text</a></p>"#)
        .expect("a link");
    let sink = followed.clone();
    let mut h = Harness::new(
        RichTextEditor::editor(doc).on_link_activated(move |href, _ctx| {
            sink.borrow_mut().push(href.to_string());
        }),
    );
    h.tree.focus(h.editor);
    h.render();
    let target = h.at(3);
    h.mouse_press(target);
    assert!(
        followed.borrow().is_empty(),
        "a plain mouse click must not leave the document"
    );
    assert_eq!(h.caret(), 3, "it places a caret in the link's text");
}

// ---------------------------------------------------------------------------
// The affordance overlay, laid out
// ---------------------------------------------------------------------------

/// A tap that lands on **no handle** reaches the editor and places a caret,
/// while the affordance overlay above it is at its real, viewport-sized bounds.
///
/// The overlay is `FullViewport` and declares `event_pass_through`, so the
/// router's fall-through is the only thing that lets the editor underneath keep
/// taking presses. That is invisible to a test that never lays out after the
/// hold: an overlay whose bounds are still zero is not chosen by the hit test at
/// all, so the press reaches the editor for a reason the fall-through had no part
/// in. Hence [`Harness::hold_at`]'s layout, and hence the check below that the
/// overlay really does cover the target.
#[test]
fn a_tap_beyond_the_affordances_reaches_the_editor_itself() {
    let mut h = Harness::focused(FALLTHROUGH_TEXT);
    h.hold_at(PointerKind::Touch, h.at(8));
    let target = h.at(FALLTHROUGH_TARGET);
    let handles = h.handles();
    assert!(!handles.is_empty(), "the hold raised no handles");
    for g in &handles {
        assert!(
            !g.hit.contains(target),
            "the fixture's target is on the {:?} handle's own hit square: {:?}",
            g.kind,
            g.hit
        );
    }
    let layer = h
        .touch
        .layer_content()
        .expect("the affordance host was built");
    let bounds = h
        .tree
        .overlay_manager()
        .bounds_for_content(layer)
        .expect("the overlay is up");
    assert!(
        bounds.contains(target),
        "the overlay must be laid out over the target, or the fall-through is \
         never consulted: {bounds:?} against {target:?}"
    );

    let id = finger();
    h.finger_down(id, target, 0);
    h.finger_up(id, target, 30);
    assert_eq!(
        h.caret(),
        FALLTHROUGH_TARGET,
        "the editor never saw the tap"
    );
}

/// Four lines, so a tap can be placed clear of the handles' own squares without
/// leaving the editor.
const FALLTHROUGH_TEXT: &str =
    "hello world and prose\nsecond line here\nthird line of text\nfourth line here";

/// An offset on the last line — two lines below the handles the hold on line one
/// raises.
const FALLTHROUGH_TARGET: usize = 60;

// ---------------------------------------------------------------------------
// Retirement
// ---------------------------------------------------------------------------

/// A cursor's press anywhere in the editor retires raised touch chrome. Nothing
/// else on a hybrid machine would: the affordance band is exempt from
/// outside-press dismissal, because every caret-moving tap is outside a handle.
#[test]
fn a_mouse_press_retires_the_touch_chrome() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    assert!(!h.handles().is_empty(), "the hold raised handles");
    h.mouse_press(h.at(2));
    assert!(
        h.handles().is_empty(),
        "a cursor's press left {:?} standing",
        h.handles()
    );
    assert!(!h.touch.toolbar_is_wanted(), "and the toolbar with them");
}

/// Focus leaving retires them too — the host's job, because the effects that
/// watch focus have no `EventContext` to dismiss an overlay from.
#[test]
fn losing_focus_retires_the_touch_chrome() {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    assert!(!h.handles().is_empty(), "the hold raised handles");
    h.tree.focus(h.elsewhere);
    h.render();
    assert!(
        h.handles().is_empty(),
        "blur left {:?} standing",
        h.handles()
    );
}

/// A keystroke moves the caret without going through the controller, so raised
/// handles would be left marking where the text used to be.
#[test]
fn a_keystroke_refreshes_the_handles_it_did_not_place() {
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    assert_eq!(h.selection(), 6..11);
    h.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert!(
        h.selection().is_empty(),
        "the arrow collapsed the selection"
    );
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        vec![SelectionHandleKind::Caret],
        "so the two range handles must have become one caret handle"
    );
}

/// An assistive client's edit moves the selection without going through the
/// controller either, and the handles have to follow it.
///
/// Driven through the widget's **own** `on_access_action_request` handler, which
/// is the door an AccessKit adapter reaches: calling `EditorHandle` directly
/// would bypass the refresh this test is about.
#[test]
fn an_at_edit_refreshes_the_handles() {
    use teksilo_core::accesskit::{Action, ActionData};
    use teksilo_core::event::WidgetEvent;
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.hold_at(PointerKind::Touch, h.at(8));
    assert_eq!(h.selection(), 6..11, "the hold's word");
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        vec![SelectionHandleKind::Start, SelectionHandleKind::End],
        "two handles for a range"
    );

    h.tree.dispatch_event(WidgetEvent::AccessAction {
        action: Action::ReplaceSelectedText,
        target: Some(h.editor),
        target_node: teksilo_core::accessibility::widget_id_to_node_id(h.editor),
        data: Some(ActionData::Value("X".into())),
    });

    assert!(
        h.selection().is_empty(),
        "the insertion collapsed the selection"
    );
    let kinds: Vec<_> = h.handles().iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        vec![SelectionHandleKind::Caret],
        "so the two range handles must have become one caret handle"
    );
}

// ---------------------------------------------------------------------------
// The IME area
// ---------------------------------------------------------------------------

/// A caret placed by a **pointer** reports the OS IME candidate area, exactly as
/// a caret moved by a key already did. Without it the candidate list stays beside
/// wherever the caret was last typed to, which on a touch device is where the
/// on-screen keyboard will draw its suggestions.
#[test]
fn a_pointer_caret_placement_reports_the_ime_area() {
    for finger_not_mouse in [false, true] {
        let mut h = Harness::focused("hello world and a longer line of prose");
        h.state.borrow_mut().last_ime_area = None;
        let target = h.at(9);
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
        let expected = h.handle.offset_rect(9).expect("caret 9 has geometry");
        let got = reported.unwrap();
        assert!(
            (got.x - expected.x).abs() < 1.0 && (got.y - expected.y).abs() < 1.0,
            "finger={finger_not_mouse}: reported {got:?}, caret 9 is at {expected:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The caret re-reveal after a viewport shrink
// ---------------------------------------------------------------------------

/// A viewport whose **height is driven from outside**, so a test can shrink it.
///
/// The plain harness cannot: an editor inside a `VStack` is given its wanted
/// height, which does not follow the window's, so laying the tree out smaller
/// leaves the body exactly as tall as it was (measured: 100 dp either way).
struct ShrinkHarness {
    tree: WidgetTree,
    state: SharedState,
    height: teksilo_core::signal::Signal<f32>,
}

impl ShrinkHarness {
    fn new(lines: usize) -> Self {
        let mut text = String::new();
        for i in 0..lines {
            text.push_str(&format!("line {i} of a document tall enough to scroll\n"));
        }
        let doc = TextDocument::new();
        doc.set_plain_text(&text).unwrap();
        let editor = RichTextEditor::editor(doc);
        let state = editor.state.clone();
        let height = teksilo_core::signal::Signal::new(300.0_f32);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let sized = tree.add(
            crate::primitives::FixedSize::new()
                .width(400.0)
                .height(height.clone())
                .child(editor),
        );
        // Inside a `VStack`, not as the root: the tree places a root at the
        // whole viewport, so a root `FixedSize` hands its child the window's
        // height rather than its own (measured: 592 dp for a 300 dp box).
        tree.add(crate::primitives::VStack::new().add_child(sized));
        tree.layout(SizeProposal::exact(480.0, 600.0));
        let _ = tree.render();
        let editor_id = tree
            .first_focusable_descendant(sized)
            .expect("the editor is focusable");
        tree.focus(editor_id);
        let mut h = Self {
            tree,
            state,
            height,
        };
        h.render();
        // Ctrl+End: the caret goes to the document's end and the editor reveals
        // it, which is what puts a real scroll offset in place to be corrected.
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
/// on-screen keyboard rising under a focused editor is the case that makes this
/// a correctness matter. `sync_viewport` records the shrink; the body's paint
/// consumes it after the relayout the shrink forced.
#[test]
fn a_viewport_shrink_re_reveals_the_caret() {
    let mut h = ShrinkHarness::new(60);
    let tall = h.scroll();
    assert!(
        tall > 0.0,
        "fixture precondition: the caret at the end is scrolled to"
    );
    h.set_height(120.0);
    let short = h.scroll();
    assert!(
        short > tall,
        "a shrink must scroll further down to keep the caret in view: \
         {tall} -> {short}"
    );
    assert!(
        !h.state.borrow().pending_caret_reveal,
        "and the request must be consumed, not left to fire on every frame"
    );
}

/// Growing the viewport does **not** drag the view back to the caret. Growth
/// reveals more text and never pushes the caret out, and a reader scrolled away
/// from the caret would otherwise be yanked back every time a window edge moved.
#[test]
fn a_viewport_growth_leaves_the_scroll_alone() {
    let mut h = ShrinkHarness::new(60);
    // Scroll the reader away from the caret, then grow.
    h.state.borrow().scroll_y.set(0.0);
    h.set_height(500.0);
    assert_eq!(
        h.scroll(),
        0.0,
        "growing the viewport must not chase the caret"
    );
}

// ---------------------------------------------------------------------------
// The image resize grip, through the press path
// ---------------------------------------------------------------------------

/// A 2×2 opaque-red PNG.
///
/// Real pixels are a precondition, not decoration: `paint_images` skips an image
/// whose bytes do not decode *before* it can record it as the selected one, so a
/// fixture without them raises no grips and the probes below would miss for a
/// reason that has nothing to do with aiming.
fn red_png() -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[255, 0, 0, 255].repeat(4)).unwrap();
    }
    buf
}

/// The reach of the grip's own painted square — what a press had before the
/// widening, and what the widened band has to be measured against.
///
/// The module's slop constant is private, so this restates it, exactly as
/// `the_grip_widening_is_not_a_touch_only_courtesy` does.
fn painted_grip_reach() -> f32 {
    super::paint::RESIZE_HANDLE_SIZE / 2.0 + 5.0
}

/// A mounted, focused editor holding one **selected** picture, plus that
/// picture's rect in the space the press path measures in and the reach in force
/// at this tree's density.
///
/// The rect comes from the paint pass rather than from the fixture's own
/// arithmetic, because the paint pass is what tells the press path where the
/// grips are: reading it back is what makes these tests answer for the wiring
/// rather than for a rectangle the test invented.
fn with_selected_image() -> (Harness, [f32; 4], f32) {
    // Small enough that the band outside the bottom-right grip is still inside
    // the editor's node — a probe outside the node never reaches the press path
    // at all — and large enough that the quarter-side clamp is not what the
    // probes below are measuring.
    const EDGE: u32 = 64;
    let doc = TextDocument::new();
    // Prose as well as the picture, so the editor is not sized by the picture
    // alone.
    let mut prose = String::new();
    for i in 0..20 {
        prose.push_str(&format!("line {i} of prose beside and below the picture\n"));
    }
    doc.set_plain_text(&prose).unwrap();
    doc.add_resource(
        teksilo_text::text_document::ResourceType::Image,
        "a.png",
        "image/png",
        &red_png(),
    )
    .unwrap();
    let editor = RichTextEditor::editor(doc);
    // Both before mounting, so the picture and the selection are already in
    // place for the **first** layout and paint: `paint_images` records the
    // selected picture only on a full render, and it is the recorded rect the
    // press path reads.
    editor.insert_image("a.png", "a red square", EDGE, EDGE);
    editor.handle().select_range(0, 1);
    let mut h = Harness::new(editor);
    h.tree.focus(h.editor);
    h.render();
    let (rect, reach) = {
        let st = h.state.borrow();
        let rect = st
            .selected_image
            .borrow()
            .clone()
            .expect("the paint pass recorded the selected picture")
            .rect;
        (
            rect,
            super::mouse::grip_reach_for_test(&st.input_tokens, rect),
        )
    };
    assert!(
        reach > painted_grip_reach(),
        "the fixture has no band only the widened reach covers: {reach} vs {}",
        painted_grip_reach()
    );
    (h, rect, reach)
}

/// The engine-space point `(dx, dy)` from the picture's bottom-right corner, as
/// a **window** point the tree can be handed.
///
/// `to_engine_local` reconstructs the window point from the wrapper-local one
/// the router hands it and then subtracts the body's origin, so the inverse is
/// exactly `local + viewport_origin`. The editor is mounted inside padding, so a
/// missing conversion here does not cancel out.
fn window_off_corner(h: &Harness, rect: [f32; 4], dx: f32, dy: f32) -> Point {
    let body = h.state.borrow().viewport_origin;
    Point::new(
        rect[0] + rect[2] + body.x + dx,
        rect[1] + rect[3] + body.y + dy,
    )
}

fn resizing_corner(h: &Harness) -> Option<(f32, f32)> {
    match &h.state.borrow().drag_state {
        super::state::DragState::ResizingImage { corner, .. } => Some(*corner),
        _ => None,
    }
}

/// A press **outside** the grip's painted square, in the band only the
/// density-projected reach covers, starts the resize.
///
/// The mounted counterpart of
/// `a_near_miss_on_a_grip_is_picked_up_by_the_widened_reach`, and the one that
/// answers for the press path: the widening is only a feature if
/// `grabbed_handle` consults it, and a pure-function test cannot see which reach
/// that call site uses.
#[test]
fn a_press_beside_a_grip_starts_the_resize() {
    let (mut h, rect, reach) = with_selected_image();
    let inside = reach - 1.0;
    // Precondition: the painted square's own reach misses this point, so
    // whatever answers it is the widened one.
    assert_eq!(
        super::mouse::handle_near_for_test(
            rect,
            Point::new(rect[0] + rect[2] + inside, rect[1] + rect[3] + inside),
            painted_grip_reach(),
        ),
        None,
        "fixture precondition: the painted square's reach must miss {inside} dp out"
    );
    h.mouse_press(window_off_corner(&h, rect, inside, inside));
    assert_eq!(
        resizing_corner(&h),
        Some((1.0, 1.0)),
        "a press {inside} dp off the bottom-right corner did not grab it: {:?}",
        h.state.borrow().drag_state
    );
}

/// …and a press past the widened reach is ordinary text again. Without this the
/// test above would pass for a grip that had swallowed the whole picture.
#[test]
fn a_press_past_the_widened_reach_is_not_a_resize() {
    let (mut h, rect, reach) = with_selected_image();
    let outside = reach + 1.0;
    h.mouse_press(window_off_corner(&h, rect, outside, outside));
    assert_eq!(
        resizing_corner(&h),
        None,
        "a press {outside} dp off the corner grabbed a grip whose reach is {reach}"
    );
}

// ---------------------------------------------------------------------------
// The text-drag threshold, through the press path
// ---------------------------------------------------------------------------

/// Select a word, press inside it, then move `travel` along x. Returns the drag
/// state the surface was left in.
///
/// A press inside a selection arms `PendingTextDrag` and the *move* decides:
/// under the threshold the press is still a click and the state stands, past it
/// the passage is handed to the drag system and the surface goes back to `Idle`.
/// So the state after one move is the threshold, observed through the mounted
/// editor rather than through the pure function.
fn drag_state_after_moving(travel: f32) -> super::state::DragState {
    let mut h = Harness::focused("hello world and a longer line of prose");
    h.handle.select_range(6, 11);
    let from = h.at(8);
    h.mouse_press(from);
    assert!(
        matches!(
            h.state.borrow().drag_state,
            super::state::DragState::PendingTextDrag { .. }
        ),
        "fixture precondition: the press inside the selection must arm a drag"
    );
    h.mouse_move(Point::new(from.x + travel, from.y));
    h.state.borrow().drag_state.clone()
}

/// A mouse's text drag starts at **its own** `drag_slop` and not before, so the
/// probes straddle the profile's figure rather than a constant of this test's.
/// The threshold used to be a local `4.0` invented in the module, which is
/// shorter than the mouse's slop — so a press that wobbled picked a passage up.
#[test]
fn a_mouse_needs_its_own_drag_slop_to_pick_a_passage_up() {
    // Read from a mounted surface, so it is the ladder the press path itself
    // consults rather than one this test assumed.
    let slop = {
        let h = Harness::focused("hello world");
        let tokens = h.state.borrow().input_tokens;
        tokens.profile(PointerKind::Mouse).drag_slop
    };
    assert!(
        matches!(
            drag_state_after_moving(slop - 0.5),
            super::state::DragState::PendingTextDrag { .. }
        ),
        "{} dp of travel started a drag the mouse's {slop} dp slop forbids",
        slop - 0.5
    );
    assert!(
        matches!(
            drag_state_after_moving(slop + 0.5),
            super::state::DragState::Idle
        ),
        "{} dp of travel did not hand the passage to the drag system",
        slop + 0.5
    );
}

// ---------------------------------------------------------------------------
// The pure geometry
// ---------------------------------------------------------------------------

mod geometry {
    use super::*;
    use teksilo_tokens::{InputTokens, TargetDensity};

    /// A 200×100 picture at (50, 20).
    const IMG: [f32; 4] = [50.0, 20.0, 200.0, 100.0];

    fn tokens(density: TargetDensity) -> InputTokens {
        InputTokens::for_density(density)
    }

    /// The grip's hit square reaches the density's target size — 24 dp at
    /// `Compact`, which is the WCAG 2.2 floor, for a 9 dp painted square.
    #[test]
    fn the_grip_reaches_the_densitys_target_size() {
        for (density, want) in [
            (TargetDensity::Compact, 24.0_f32),
            (TargetDensity::Comfortable, 32.0),
            (TargetDensity::Touch, 44.0),
        ] {
            let reach = super::super::mouse::grip_reach_for_test(&tokens(density), IMG);
            assert!(
                (reach * 2.0 - want).abs() < 0.01,
                "{density:?}: a {reach} dp half-extent is a {} dp target, wanted {want}",
                reach * 2.0
            );
        }
    }

    /// …and it is the **same** for every pointer kind, because a conformance
    /// floor that only one device gets is not a floor. The function takes no
    /// kind at all, which is the strongest form of that: there is no parameter
    /// to get wrong.
    #[test]
    fn the_grip_widening_is_not_a_touch_only_courtesy() {
        // A mouse's own reach — the painted square plus the module's slop —
        // is strictly smaller than what the density asks for, so the top-up is
        // doing real work for a precise pointer too.
        let base = super::super::paint::RESIZE_HANDLE_SIZE / 2.0 + 5.0;
        let reach = super::super::mouse::grip_reach_for_test(&tokens(TargetDensity::Compact), IMG);
        assert!(
            reach > base,
            "the top-up did nothing for the mouse: {reach} vs {base}"
        );
    }

    /// On a thumbnail the top-up is clamped to a quarter of the shorter side, so
    /// the four grips can never grow into each other or over the middle of the
    /// picture. `Touch`'s 44 dp projection on a 40×40 picture would otherwise
    /// leave nothing to click but grips.
    #[test]
    fn the_grips_never_swallow_a_small_picture() {
        let small = [0.0, 0.0, 40.0, 40.0];
        let reach = super::super::mouse::grip_reach_for_test(&tokens(TargetDensity::Touch), small);
        let centre = Point::new(20.0, 20.0);
        assert!(
            super::super::mouse::handle_near_for_test(small, centre, reach).is_none(),
            "a {reach} dp reach put a grip in the middle of a 40x40 picture"
        );
    }

    /// A press that [`handle_at`]'s own reach misses is picked up by the
    /// density-projected one — through the function the press path actually
    /// calls, so the widening is wired and not merely computable.
    ///
    /// The probe sits 11 dp diagonally out from the corner: past the mouse's
    /// 9.5 dp half-extent, inside `Compact`'s 12 dp.
    #[test]
    fn a_near_miss_on_a_grip_is_picked_up_by_the_widened_reach() {
        let t = tokens(TargetDensity::Compact);
        let corner = Point::new(IMG[0] + IMG[2] + 11.0, IMG[1] + IMG[3] + 11.0);
        assert_eq!(
            super::super::mouse::handle_near_for_test(IMG, corner, 9.5),
            None,
            "fixture precondition: the base reach misses this point"
        );
        assert_eq!(
            super::super::mouse::corner_at_for_test(IMG, corner, &t),
            Some((1.0, 1.0)),
            "the widened reach did not pick it up"
        );
    }

    /// Two overlapping grips settle by **distance**, not by declaration order:
    /// a widened reach makes adjacent grips overlap on a small picture, and
    /// sibling order is not an aiming rule.
    #[test]
    fn overlapping_grips_settle_by_distance() {
        let small = [0.0, 0.0, 40.0, 40.0];
        // A reach that certainly overlaps: half the picture.
        let reach = 20.0;
        // Nearer the bottom-right than any other corner.
        let near_br = Point::new(30.0, 30.0);
        assert_eq!(
            super::super::mouse::handle_near_for_test(small, near_br, reach),
            Some((1.0, 1.0))
        );
        let near_tl = Point::new(10.0, 10.0);
        assert_eq!(
            super::super::mouse::handle_near_for_test(small, near_tl, reach),
            Some((0.0, 0.0))
        );
    }

    /// The one rule both pointer paths consult to decide whether a press on a
    /// link follows it. A finger always follows; a cursor needs either a
    /// read-only surface or Ctrl(⌘).
    #[test]
    fn a_link_is_followed_by_a_finger_and_by_a_cursor_that_asked() {
        use super::super::mouse::link_follows_for_test as follows;
        let pen = PointerKind::Pen(teksilo_tokens::PenKind::Pen);
        // Direct pointers, editable surface, no modifier: follow.
        assert!(follows(PointerKind::Touch, false, false));
        assert!(follows(pen, false, false));
        // A cursor in an editable surface with no modifier: place a caret.
        assert!(!follows(PointerKind::Mouse, false, false));
        // …and follows once it asks, or once the surface is read-only.
        assert!(follows(PointerKind::Mouse, false, true));
        assert!(follows(PointerKind::Mouse, true, false));
    }

    /// The text-drag threshold is the pointer's own `drag_slop`, not one figure
    /// for every device. 4 dp of travel — the constant this used to be — is
    /// inside the jitter of a finger holding still.
    #[test]
    fn the_text_drag_threshold_follows_the_pointer() {
        let t = tokens(TargetDensity::Compact);
        let mouse = super::super::mouse::text_drag_threshold_for_test(PointerKind::Mouse, &t);
        let touch = super::super::mouse::text_drag_threshold_for_test(PointerKind::Touch, &t);
        let pen = super::super::mouse::text_drag_threshold_for_test(
            PointerKind::Pen(teksilo_tokens::PenKind::Pen),
            &t,
        );
        assert_eq!((mouse, touch, pen), (5.0, 18.0, 2.0));
        assert!(
            touch > mouse && mouse > pen,
            "the ordering is the point: {mouse} / {touch} / {pen}"
        );
    }
}
