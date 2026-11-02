// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What the Windows adapter raises while an input method composes and commits
//! in each editor, frame by frame, as `teksilo-app` delivers it.
//!
//! The record behind NVDA's report of a Japanese commit spoken three times in
//! the code editor and once in the rich text editor, on both Windows input
//! paths. Each frame here is what the application does for a turn of its
//! loop: the turn's input, then the frame tick and layout, then the
//! accessibility update handed to the adapter.
//!
//! - **winit's IMM path** (`TEKSILO_TSF=0`): winit sends an empty preedit
//!   right before every commit (`winit-0.30.13`,
//!   `platform_impl/windows/event_loop.rs`, `WM_IME_COMPOSITION` with
//!   `GCS_RESULTSTR`), in the same window message, so the two arrive in one
//!   turn; the record is also taken with a frame between them, the order a
//!   platform that sends them in two turns would give.
//! - **The text store** (the default, `TextStoreMode::Store`): the
//!   composition is written through the field's `TextInputEditor`, one edit
//!   session per grant, and ended as a commit; no empty preedit, no
//!   `ImeCommit`.
//!
//! What it shows, among the event kinds [`Uia`] models: on both paths, in
//! the order the platform gives, the commit raises one event, the editor's
//! selection collapsing from the composition range to the caret, and no text
//! change; and the code editor and a rich text plain paragraph raise the same
//! events at every step. Only a heading differs (its text changes land on its
//! `Role::Label` container), and the rich text control of the report was
//! typed on a heading. Nothing singles out the code editor's node, so no
//! per-line container is added. Only a frame between winit's empty preedit and
//! its commit gives extra text changes (the ignored diagnostic below). Left
//! to the Windows record: whether such a frame happens there, what NVDA says
//! through its own input-composition support rather than UIA, and the
//! re-announcement on Space, where the candidate window takes focus outside
//! the application.

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::widget_builder::WidgetBuilder as _;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::heard_test::{Uia, UiaRecorder};

fn viewport() -> SizeProposal {
    SizeProposal::exact(520.0, 400.0)
}

/// One turn of the application's loop with no event: the frame tick, the
/// layout and the paint.
fn frame(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(viewport());
    let _ = tree.render();
}

/// A tree holding `editor` below somewhere else for the focus to be, the
/// editor focused and its caret placed by `keys`.
fn mount(editor: impl teksilo_core::widget::Widget + 'static, keys: &[Key]) -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let other = tree.add(
        crate::primitives::FixedSize::new()
            .width(40.0)
            .height(20.0)
            .child(crate::primitives::RectWidget::new().focusable(true)),
    );
    let padded = tree.add(
        crate::primitives::FixedSize::new()
            .height(240.0)
            .child(crate::primitives::Padding::symmetric(20.0, 20.0).child(editor)),
    );
    tree.add(crate::primitives::VStack::new().child(other).child(padded));
    frame(&mut tree);
    let editor = tree
        .first_focusable_descendant(padded)
        .expect("the editor is focusable");
    tree.focus(editor);
    frame(&mut tree);
    tree.press_key(Key::Home, Modifiers::COMMAND);
    frame(&mut tree);
    for key in keys {
        tree.press_key(*key, Modifiers::NONE);
        frame(&mut tree);
    }
    frame(&mut tree);
    tree
}

/// The record of composing にほんご, converting to 日本語 and committing it,
/// one entry per frame. With `frame_between`, a frame runs between the
/// commit's empty preedit and its text.
fn record(mut tree: WidgetTree, frame_between: bool) -> Vec<(&'static str, Vec<Uia>)> {
    let mut adapter = UiaRecorder::attach(&mut tree);
    let mut out = Vec::new();
    let mut step = |tree: &mut WidgetTree, name: &'static str, events: &[WidgetEvent]| {
        for event in events {
            tree.dispatch_event(event.clone());
        }
        frame(tree);
        out.push((name, adapter.frame(tree)));
    };
    step(
        &mut tree,
        "preedit kana",
        &[WidgetEvent::ime_composition("にほんご", None)],
    );
    step(
        &mut tree,
        "preedit kanji",
        &[WidgetEvent::ime_composition("日本語", None)],
    );
    if frame_between {
        step(
            &mut tree,
            "empty preedit",
            &[WidgetEvent::ime_composition("", None)],
        );
        step(&mut tree, "commit", &[WidgetEvent::ime_commit("日本語")]);
    } else {
        step(
            &mut tree,
            "empty preedit + commit",
            &[
                WidgetEvent::ime_composition("", None),
                WidgetEvent::ime_commit("日本語"),
            ],
        );
    }
    step(&mut tree, "next frame", &[]);
    out
}

fn code_editor() -> WidgetTree {
    let doc = TextDocument::new();
    doc.set_plain_text("first line\nsecond line").unwrap();
    mount(crate::code_editor::CodeEditor::new(doc), &[Key::End])
}

fn rich_paragraph() -> WidgetTree {
    let doc = TextDocument::new();
    doc.set_plain_text("first line\nsecond line").unwrap();
    mount(crate::rich_text::RichTextEditor::editor(doc), &[Key::End])
}

fn rich_heading() -> WidgetTree {
    let doc = TextDocument::new();
    doc.set_markdown("# first line\n\nsecond line")
        .unwrap()
        .wait()
        .unwrap();
    mount(crate::rich_text::RichTextEditor::editor(doc), &[Key::End])
}

fn print(name: &str, record: &[(&'static str, Vec<Uia>)]) {
    eprintln!("== {name}");
    for (step, events) in record {
        eprintln!("  {step}: {events:?}");
    }
}

/// The events of the step named `step`.
fn at<'a>(record: &'a [(&'static str, Vec<Uia>)], step: &str) -> &'a [Uia] {
    &record
        .iter()
        .find(|(name, _)| *name == step)
        .unwrap_or_else(|| panic!("no step {step:?} in {record:?}"))
        .1
}

fn text_changes(events: &[Uia]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Uia::TextChanged(_)))
        .count()
}

/// In the order winit gives (the empty preedit and the commit in one turn),
/// the commit changes no text the adapter sees: the composition's text is
/// already the committed text. What it raises is the editor's selection
/// collapsing from the composition range to the caret, which NVDA reads as
/// the composition "unselected". So in this order a commit is not told three
/// times by the code editor; printed with `--nocapture`.
#[test]
fn a_commit_in_one_turn_raises_only_the_selection_collapsing() {
    for (name, tree) in [
        ("code editor", code_editor()),
        ("rich text paragraph", rich_paragraph()),
        ("rich text heading", rich_heading()),
    ] {
        let record = record(tree, false);
        print(name, &record);
        assert_eq!(
            at(&record, "empty preedit + commit"),
            [Uia::TextSelectionChanged(Role::MultilineTextInput)],
            "{name}"
        );
        assert_eq!(at(&record, "next frame"), [], "{name}");
    }
}

/// The code editor and a rich text editor's plain paragraph raise the same
/// events at every step, in both orders: the runs of both hang off the editor
/// node, so their text changes are raised on it. A heading in the rich text
/// editor raises them on its text container instead (a `Role::Label` under
/// the `Role::Heading`), which is the only difference the record shows
/// between the editors, and the first line of the `rich-text-editor` example
/// is a heading.
#[test]
fn the_code_editor_and_a_rich_paragraph_raise_the_same_events() {
    for frame_between in [false, true] {
        let code = record(code_editor(), frame_between);
        let rich = record(rich_paragraph(), frame_between);
        print(
            &format!("code editor, frame between: {frame_between}"),
            &code,
        );
        print(
            &format!("rich text paragraph, frame between: {frame_between}"),
            &rich,
        );
        assert_eq!(code, rich, "frame between: {frame_between}");
        let heading = record(rich_heading(), frame_between);
        print(
            &format!("rich text heading, frame between: {frame_between}"),
            &heading,
        );
        assert!(
            heading
                .iter()
                .flat_map(|(_, events)| events)
                .all(|e| *e != Uia::TextChanged(Role::MultilineTextInput)),
            "a heading's text changes are raised on its text container: {heading:?}"
        );
    }
}

/// If a frame ran between the commit's empty preedit and its text, the frame
/// would cancel the composition (`settle_cancelled_composition`) and the
/// commit insert its text again: two text changes on the editor, the line
/// without the composition and then with it, each with a selection change,
/// for a commit that changes nothing. That is three things to say where the
/// one-turn order gives one, as NVDA said in the code editor. winit's Windows
/// backend sends the two in one window message, so whether a frame falls
/// between them there is what the Windows record has to show.
#[test]
#[ignore = "diagnostic: asserts the remove-and-reinsert a frame between the empty preedit and \
            the commit causes, the behaviour a fix would remove; run with --ignored to see it"]
fn a_frame_between_the_empty_preedit_and_the_commit_takes_the_text_out_and_back() {
    let record = record(code_editor(), true);
    print("code editor, frame between: true", &record);
    assert_eq!(text_changes(at(&record, "empty preedit")), 1);
    assert_eq!(text_changes(at(&record, "commit")), 1);
}

// --- The text store (TSF) path ------------------------------------------------

/// One grant of the Windows text store: an edit session (a batch) of writes
/// through the focused field's `TextInputEditor`, then the frame that turn
/// renders and the update the adapter is handed.
fn grant(
    tree: &mut WidgetTree,
    adapter: &mut UiaRecorder,
    write: impl FnOnce(&dyn teksilo_core::text_input_document::TextInputEditor),
) -> Vec<Uia> {
    tree.text_surfaces()
        .with_focused_input_editor(|e| {
            e.begin_input_batch();
            write(e);
            e.end_input_batch();
        })
        .expect("the editor takes the input system's writes");
    frame(tree);
    adapter.frame(tree)
}

/// The same composition as the Windows text store writes it, its default path
/// (`TextStoreMode::Store`, winit's IME off): the composition marked at the
/// caret and given its kana, the kana replaced by the kanji as the input method
/// converts, then the composition ended as a commit. No empty preedit and no
/// `ImeCommit` on this path, so no frame can fall between them. One entry per
/// grant.
fn record_store(mut tree: WidgetTree) -> Vec<(&'static str, Vec<Uia>)> {
    use teksilo_core::ImeOrigin;
    use teksilo_core::text_input_document::WriteOptions;
    let caret = tree
        .text_surfaces()
        .with_focused_input_document(|d| d.caret())
        .flatten()
        .expect("the editor has a caret");
    let mut adapter = UiaRecorder::attach(&mut tree);
    let mut out = Vec::new();
    out.push((
        "mark + kana",
        grant(&mut tree, &mut adapter, |e| {
            e.mark_composition(caret..caret, &[]).unwrap();
            e.replace_text(caret..caret, "にほんご", WriteOptions::default())
                .unwrap();
        }),
    ));
    out.push((
        "convert to kanji",
        grant(&mut tree, &mut adapter, |e| {
            e.replace_text(caret..caret + 4, "日本語", WriteOptions::default())
                .unwrap();
        }),
    ));
    out.push((
        "end composition",
        grant(&mut tree, &mut adapter, |e| {
            e.end_composition(ImeOrigin::Composition).unwrap();
        }),
    ));
    frame(&mut tree);
    out.push(("next frame", adapter.frame(&mut tree)));
    out
}

/// On the store path the code editor and a rich text editor's plain
/// paragraph raise the same events at every grant, and ending the
/// composition raises, among the modelled kinds, at most the selection
/// collapsing and no text change: the store writes the committed text in
/// place of the composition's, which is already that text. Printed with
/// `--nocapture`.
#[test]
fn on_the_store_path_the_code_editor_and_a_rich_paragraph_raise_the_same_events() {
    let code = record_store(code_editor());
    let rich = record_store(rich_paragraph());
    print("code editor, text store", &code);
    print("rich text paragraph, text store", &rich);
    print(
        "rich text heading, text store",
        &record_store(rich_heading()),
    );
    assert_eq!(code, rich);
    assert_eq!(text_changes(at(&code, "end composition")), 0, "{code:?}");
    assert_eq!(at(&code, "next frame"), [], "{code:?}");
}

/// A heading's text container is a `Role::Label`, whose UIA Name is its text
/// (its label comes from its value) and which has no Value pattern: the
/// recorder reports a Name change there, as the adapter raises it.
#[test]
fn a_headings_text_container_changes_its_name_not_a_value() {
    let record = record(rich_heading(), false);
    let events: Vec<&Uia> = record.iter().flat_map(|(_, events)| events).collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Uia::Name(Role::Label, Some(name)) if name.contains("日本語"))),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Uia::Value(Role::Label, _))),
        "{events:?}"
    );
}
