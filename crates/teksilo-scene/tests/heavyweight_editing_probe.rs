// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Does a real editable widget embedded in a `Scene` behave like one? **Yes** —
//! these four pass. They are here because nothing in the crate covered it.
//!
//! `docs/teksilo-scene.md` promises the heavyweight tier keeps "focus,
//! animation, DnD, AT" intact, and the z-order section specifically promises
//! that restacking reorders `node.children` "*without* recreating the widgets,
//! so a dragged card keeps its focus, text-edit cursor and in-flight animations
//! across the restack."
//!
//! Nothing tested that. `grep -rn 'TextInput' crates/teksilo-scene/` was empty,
//! no example embeds an editor in a scene, and the focus tests under
//! `view/tests.rs` all exercise the *lightweight* `focus_order` callback rather
//! than a heavyweight child receiving a keystroke. That promise is the single
//! load-bearing assumption under any "note container on a canvas" (OneNote /
//! Milanote / Muse) use case, so it is worth pinning:
//!
//!   1. an embedded `TextInput` is a tab stop;
//!   2. typing into it reaches its bound signal;
//!   3. a z-restack preserves both its content and the focus on it;
//!   4. CONTROL — the same keystrokes into a bare `TextInput`, so a harness
//!      limitation can never be misread as a scene defect.
//!
//! Two traps worth knowing if you extend these. The **first** tab stop under
//! the tree is the `SceneView` itself, not an editor — targeting it and
//! concluding "the scene swallows keystrokes" is wrong, so these walk every
//! stop. And `TextInput` is a composite whose editable node is an inner
//! `TextInputField`: `tree.focus(the_composite)` does not make it typeable, and
//! `WidgetTree::focus` does not check focusability, so only
//! `tab_stops_within` answers the reachability question honestly.
//!
//! Run with: cargo test -p teksilo-scene --test heavyweight_editing_probe

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{SceneModel, SceneView};
use teksilo_widgets::TextInput;

const VIEWPORT: SizeProposal = SizeProposal {
    width: Some(800.0),
    height: Some(600.0),
};

/// A scene with two text inputs as heavyweight items, laid out and ready.
fn scene_with_two_editors() -> (WidgetTree, SceneModel, Signal<String>, Signal<String>) {
    let a_text = Signal::new(String::new());
    let b_text = Signal::new(String::new());

    let model = SceneModel::new();
    let a = model.add_widget_item(0u32, Rect::new(0.0, 0.0, 200.0, 40.0));
    let b = model.add_widget_item(1u32, Rect::new(0.0, 100.0, 200.0, 40.0));
    let _ = (a, b);

    let (ta, tb) = (a_text.clone(), b_text.clone());
    let view = SceneView::with_model(model.clone()).delegate_typed::<u32>(move |which, _id| {
        let sig = if *which == 0 { ta.clone() } else { tb.clone() };
        Box::new(TextInput::new(sig))
    });

    let mut tree = WidgetTree::new().with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let _root = tree.add(view);
    tree.layout(VIEWPORT);
    (tree, model, a_text, b_text)
}

/// 1. Keyboard reachability. `WidgetTree::focus(id)` does NOT check that a node
///    is focusable, so the only honest question is what `tab_stops_within`
///    reports for the whole tree.
#[test]
fn an_embedded_text_input_is_a_tab_stop() {
    let (tree, _model, _a, _b) = scene_with_two_editors();
    let root = *tree.roots().first().expect("a root");
    let stops = tree.tab_stops_within(root);
    assert!(
        stops.len() >= 2,
        "expected the two embedded TextInputs to be keyboard-reachable, \
         found {} tab stop(s) in the whole tree",
        stops.len()
    );
}

/// 2. Typing reaches the model.
#[test]
fn typing_into_an_embedded_text_input_reaches_the_model() {
    let (mut tree, _model, a_text, b_text) = scene_with_two_editors();
    let root = *tree.roots().first().expect("a root");
    let stops = tree.tab_stops_within(root);

    // Try EVERY tab stop, so a wrong guess about which one is the editable
    // node can't be mistaken for "the scene swallows keystrokes".
    for stop in &stops {
        tree.focus(*stop);
        tree.type_text(*stop, "hello");
        tree.layout(VIEWPORT);
    }

    assert!(
        a_text.get().contains("hello") || b_text.get().contains("hello"),
        "typing into an embedded editor should reach its bound signal; \
         tried all {} tab stop(s), both signals are still {:?} / {:?}",
        stops.len(),
        a_text.get(),
        b_text.get()
    );
}

/// 3. The documented restack promise: `set_z` reorders `node.children` without
///    recreating widgets, "so a dragged card keeps its focus [and] text-edit
///    cursor across the restack."
#[test]
fn a_z_restack_preserves_focus_and_content_of_an_embedded_editor() {
    let (mut tree, model, a_text, b_text) = scene_with_two_editors();
    let root = *tree.roots().first().expect("a root");

    // Find the tab stop that is actually an editable field (the first stop is
    // the SceneView itself), by typing into each and seeing which one lands.
    let mut target = None;
    for stop in tree.tab_stops_within(root) {
        tree.focus(stop);
        tree.type_text(stop, "draft");
        tree.layout(VIEWPORT);
        if a_text.get().contains("draft") || b_text.get().contains("draft") {
            target = Some(stop);
            break;
        }
    }
    let target = target.expect("precondition: some tab stop must be an editable field");
    let typed_into_a = a_text.get().contains("draft");
    let before = if typed_into_a {
        a_text.get()
    } else {
        b_text.get()
    };

    // Bring the *other* item to the front — the drag-to-front primitive.
    let ids = model.ids();
    model.bring_to_front(ids[1]);
    tree.layout(VIEWPORT);

    let after = if typed_into_a {
        a_text.get()
    } else {
        b_text.get()
    };
    assert_eq!(
        after, before,
        "a z-restack must not lose the embedded editor's content"
    );
    assert_eq!(
        tree.focused(),
        Some(target),
        "a z-restack must not move focus off the embedded editor"
    );
}

/// CONTROL: the same keystrokes into a `TextInput` that is NOT in a scene.
/// If this fails too, the harness (not the scene) is the limitation and tests
/// 2 and 3 above prove nothing. If it passes, the scene is the difference.
#[test]
fn control_typing_into_a_bare_text_input_reaches_the_model() {
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let input = tree.add(TextInput::new(text.clone()));
    tree.layout(VIEWPORT);

    // Symmetric with the scene probes: focus the real tab stop (TextInput is a
    // composite; the editable node is an inner TextInputField), not the outer
    // composite node.
    let target = *tree
        .tab_stops_within(input)
        .first()
        .expect("a bare TextInput must expose a tab stop");
    tree.focus(target);
    tree.type_text(target, "hello");
    tree.layout(VIEWPORT);

    assert_eq!(
        text.get(),
        "hello",
        "CONTROL: a bare TextInput must accept typed text, or the probe above proves nothing"
    );
}
