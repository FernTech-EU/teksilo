// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless integration tests for TextInput.

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use super::TextInput;

fn setup(
    initial: &str,
) -> (
    WidgetTree,
    Signal<String>,
    teksilo_core::widget_id::WidgetId,
) {
    let text = Signal::new(initial.to_string());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(text.clone()).placeholder(lit!("Type here...")));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
    (tree, text, id)
}

fn tick(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(300.0, 40.0));
}

#[test]
fn constructs_and_lays_out() {
    let (tree, text, id) = setup("");
    assert_eq!(text.get(), "");
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0, "widget should have non-zero width");
    assert!(bounds.height > 0.0, "widget should have non-zero height");
}

#[test]
fn initial_text_propagates() {
    let (_tree, text, _id) = setup("Hello");
    assert_eq!(text.get(), "Hello");
}

// ── char_filter ────────────────────────────────────────────────────

/// Focus the inner focusable descendant of the outer TextInput.
/// `TextInput` is a composite — its own root is `GenericContainer`
/// and not focusable. The inner `TextInputField` is what the
/// framework actually focuses, so tests must descend to it.
fn focus_field(tree: &mut WidgetTree, outer: teksilo_core::widget_id::WidgetId) {
    let field = tree
        .first_focusable_descendant(outer)
        .expect("TextInput should expose a focusable inner field");
    tree.focus(field);
}

#[test]
fn char_filter_rejects_disallowed_keystrokes() {
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(text.clone()).char_filter(|c| c.is_ascii_digit()));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
    focus_field(&mut tree, id);

    // Type "4a2" — the 'a' must be dropped, "42" must remain.
    tree.type_text(id, "4a2");
    tick(&mut tree);
    tick(&mut tree); // second tick: flush pending_chars → text_signal
    assert_eq!(text.get(), "42", "char_filter must reject non-digits");
}

#[test]
fn char_filter_admits_allowed_keystrokes() {
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TextInput::new(text.clone()).char_filter(|c| c.is_ascii_digit() || c == '.' || c == '-'),
    );
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    tree.type_text(id, "-3.14");
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(text.get(), "-3.14");
}

#[test]
fn char_filter_composes_with_max_length() {
    // max_length is enforced per keystroke against the committed
    // document length (see `keyboard.rs`), so this test ticks after
    // each character to match real-world typing cadence.
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TextInput::new(text.clone())
            .char_filter(|c| c.is_ascii_digit())
            .max_length(3),
    );
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
    focus_field(&mut tree, id);
    for ch in "1a2b3c4d5".chars() {
        tree.type_text(id, &ch.to_string());
        tick(&mut tree);
        tick(&mut tree);
    }
    // Filter strips letters → "12345". max_length caps at 3 → "123".
    assert_eq!(text.get(), "123");
}

// ── on_blur ────────────────────────────────────────────────────────

#[test]
fn on_blur_fires_when_focus_is_lost() {
    use std::cell::Cell;
    use std::rc::Rc;
    let text = Signal::new("hello".to_string());
    let fired = Rc::new(Cell::new(0u32));
    let fired_c = fired.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(text).on_blur_fn(move |_| fired_c.set(fired_c.get() + 1)));
    // A focusable sibling button we can park focus on to force blur.
    let sink = tree.add(crate::button::Button::new(lit!("sink")).on_activate_fn(|_| {}));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);

    focus_field(&mut tree, id);
    tick(&mut tree);
    assert_eq!(fired.get(), 0, "on_blur must not fire on focus gain");

    // Blur by moving focus to a sibling widget.
    tree.focus(tree.first_focusable_descendant(sink).unwrap());
    tick(&mut tree);
    assert_eq!(
        fired.get(),
        1,
        "on_blur must fire exactly once on focus loss"
    );
}

#[test]
fn on_blur_and_on_submit_coexist() {
    use std::cell::Cell;
    use std::rc::Rc;
    let text = Signal::new(String::new());
    let blurred = Rc::new(Cell::new(false));
    let submitted = Rc::new(Cell::new(false));
    let b = blurred.clone();
    let s = submitted.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TextInput::new(text)
            .on_submit_fn(move |_| s.set(true))
            .on_blur_fn(move |_| b.set(true)),
    );
    let sink = tree.add(crate::button::Button::new(lit!("sink")).on_activate_fn(|_| {}));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);

    focus_field(&mut tree, id);
    tree.press_key(Key::Enter, Modifiers::NONE);
    tick(&mut tree);
    assert!(submitted.get(), "Enter must fire on_submit");
    assert!(!blurred.get(), "Enter alone must not fire on_blur");

    tree.focus(tree.first_focusable_descendant(sink).unwrap());
    tick(&mut tree);
    assert!(blurred.get(), "Focus loss must fire on_blur");
}

// ── suffix ─────────────────────────────────────────────────────────

#[test]
fn suffix_layout_stays_single_line() {
    let text = Signal::new("123".to_string());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(text).suffix(" kg"));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
    // Height must still match the theme's field height (no wrap
    // added by the suffix).
    let bounds = tree.bounds(id);
    assert!(
        bounds.height > 0.0 && bounds.height < 80.0,
        "single-line field with suffix should stay under 80 px tall (got {})",
        bounds.height
    );
}

// ── IME (input-method) composition ──────────────────────────────────

use teksilo_core::event::WidgetEvent;
use teksilo_core::ime::ImeContext;

#[test]
fn ime_composition_then_commit_inserts_finalised_text() {
    let (mut tree, text, id) = setup("");
    focus_field(&mut tree, id);

    // Two intermediate compositions, then a commit.
    tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "ni".to_string(),
        cursor: Some(2..2),
    });
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(text.get(), "ni", "preedit shows tentatively in the value");

    tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "nihao".to_string(),
        cursor: Some(5..5),
    });
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(text.get(), "nihao");

    // winit clears the preedit just before the commit.
    tree.dispatch_event(WidgetEvent::ImeComposition {
        text: String::new(),
        cursor: None,
    });
    tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "你好".to_string(),
    });
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(
        text.get(),
        "你好",
        "commit replaces the preedit with the final text"
    );
}

#[test]
fn ime_composition_cancelled_leaves_field_clean() {
    let (mut tree, text, id) = setup("");
    focus_field(&mut tree, id);
    tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "ni".to_string(),
        cursor: Some(2..2),
    });
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(text.get(), "ni");

    // Cancel: empty composition, no commit.
    tree.dispatch_event(WidgetEvent::ImeComposition {
        text: String::new(),
        cursor: None,
    });
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(text.get(), "", "cancelled preedit is removed entirely");
}

#[test]
fn empty_ime_composition_flood_is_inert() {
    // Some Linux IME backends (ibus / fcitx via winit) flood empty
    // `Ime::Preedit("")` events while a field is focused. With no active preedit
    // each is a genuine no-op: it must not alter the document, and (per the
    // widget-level short-circuit) must not churn an undo block or re-report the
    // IME area every event. The field must remain fully usable afterwards.
    let (mut tree, text, id) = setup("ab");
    focus_field(&mut tree, id);

    for _ in 0..20 {
        tree.dispatch_event(WidgetEvent::ImeComposition {
            text: String::new(),
            cursor: None,
        });
        tick(&mut tree);
    }
    assert_eq!(
        text.get(),
        "ab",
        "empty IME flood must not alter the document"
    );

    // Still functional: a real composition + commit lands text.
    tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "c".to_string(),
        cursor: Some(1..1),
    });
    tick(&mut tree);
    tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "c".to_string(),
    });
    tick(&mut tree);
    assert!(
        text.get().contains('c'),
        "field must still accept IME input after the flood; got {:?}",
        text.get()
    );
}

#[test]
fn ime_preedit_removed_on_focus_loss() {
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(text.clone()));
    let sink = tree.add(crate::button::Button::new(lit!("sink")).on_activate_fn(|_| {}));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
    focus_field(&mut tree, id);

    tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "ni".to_string(),
        cursor: Some(2..2),
    });
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(text.get(), "ni");

    // Move focus away — the tentative composition must be abandoned.
    tree.focus(tree.first_focusable_descendant(sink).unwrap());
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(text.get(), "", "blur abandons the in-progress composition");
}

#[test]
fn ime_commit_respects_char_filter() {
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(text.clone()).char_filter(|c| c.is_ascii_digit()));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
    focus_field(&mut tree, id);

    tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "4a2".to_string(),
    });
    tick(&mut tree);
    tick(&mut tree);
    assert_eq!(
        text.get(),
        "42",
        "commit goes through the char_filter like typed input"
    );
}

#[test]
fn focused_text_field_is_an_ime_text_surface() {
    let (mut tree, _text, id) = setup("");
    focus_field(&mut tree, id);
    assert_eq!(
        tree.ime_context_for_focused(),
        Some(ImeContext::text()),
        "a focused plain text field declares a Normal IME surface"
    );
}

#[test]
fn focused_button_is_not_an_ime_surface() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let btn = tree.add(crate::button::Button::new(lit!("ok")).on_activate_fn(|_| {}));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
    tree.focus(tree.first_focusable_descendant(btn).unwrap());
    assert_eq!(
        tree.ime_context_for_focused(),
        None,
        "a non-text widget must not enable the OS IME"
    );
}

#[test]
fn replace_selected_text_inserts_at_caret() {
    // The AT-SPI (Linux) / UIA (Windows) braille-keyboard & dictation
    // insertion path. We advertise `Action::ReplaceSelectedText`, so it
    // must insert at the caret, replacing any active selection — NOT
    // wipe the whole field like `SetValue`.
    use teksilo_core::accesskit::{Action, ActionData};
    use teksilo_core::event::WidgetEvent;

    let (mut tree, text, id) = setup("hi");
    let field = tree
        .first_focusable_descendant(id)
        .expect("inner field is focusable");
    tree.focus(field);

    // Collapse the caret to the end (focus selects-all on keyboard focus).
    tree.dispatch_event(WidgetEvent::AccessAction {
        action: Action::SetTextSelection,
        target: Some(field),
        target_node: teksilo_core::accessibility::widget_id_to_node_id(field),
        data: Some(ActionData::SetTextSelection(
            teksilo_core::accesskit::TextSelection {
                anchor: teksilo_core::accesskit::TextPosition {
                    node: teksilo_core::accessibility::widget_id_to_node_id(field),
                    character_index: 2,
                },
                focus: teksilo_core::accesskit::TextPosition {
                    node: teksilo_core::accessibility::widget_id_to_node_id(field),
                    character_index: 2,
                },
            },
        )),
    });
    tick(&mut tree);

    tree.dispatch_event(WidgetEvent::AccessAction {
        action: Action::ReplaceSelectedText,
        target: Some(field),
        target_node: teksilo_core::accessibility::widget_id_to_node_id(field),
        data: Some(ActionData::Value("!".into())),
    });
    tick(&mut tree);

    assert_eq!(
        text.get(),
        "hi!",
        "ReplaceSelectedText inserts at the caret, it does not replace the document"
    );
}

#[test]
fn focused_field_emits_a_text_run_child_for_voiceover_echo() {
    // Regression: VoiceOver echoes typed characters only when the macOS
    // adapter fires `AXSelectedTextChanged`, which accesskit_consumer
    // gates on `supports_text_ranges()` — and that requires a child
    // `Role::TextRun`, NOT `character_lengths` hosted on the input node
    // itself. A childless input reads its value once on focus but stays
    // silent while typing. Lock in the TextRun child + its value /
    // character_lengths, and that the selection targets that child.
    use teksilo_core::accesskit::Role;

    let (mut tree, text, id) = setup("");
    focus_field(&mut tree, id);
    // café — the final char is 2 UTF-8 bytes, so character_lengths must
    // be per-char byte counts [1,1,1,2] and the caret a *character*
    // index (4), not a byte offset.
    text.set("café".to_string());
    tick(&mut tree);
    tick(&mut tree);

    let update = tree.sync_accessibility();

    let (input_id, input) = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::TextInput)
        .expect("a Role::TextInput node is present");

    // The input node must have a TextRun child (the thing that makes
    // `supports_text_ranges()` true).
    let (run_id, run) = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::TextRun)
        .expect("focused field must emit a Role::TextRun child");
    assert!(
        input.children().contains(run_id),
        "the TextRun must be a direct child of the input node"
    );
    assert_eq!(run.value(), Some("café"), "TextRun carries the value");
    assert_eq!(
        run.character_lengths(),
        &[1u8, 1, 1, 2],
        "character_lengths are per-char UTF-8 byte counts"
    );

    // The caret/selection must reference the TextRun child, with a
    // character index (4), so AT range queries resolve.
    let sel = input
        .text_selection()
        .expect("focused field exposes a text selection");
    assert_eq!(
        sel.focus.node, *run_id,
        "selection targets the TextRun child, not the input node"
    );
    assert_eq!(
        sel.focus.character_index, 4,
        "caret is a character index past 'café' (not byte offset 5)"
    );
    let _ = input_id;
}

#[test]
fn empty_focused_field_still_emits_a_text_run() {
    // The run must exist even when empty: the change-diff's *old* node
    // (empty field) has to `supports_text_ranges()` too, or the very
    // first keystroke won't fire `AXSelectedTextChanged`.
    use teksilo_core::accesskit::Role;

    let (mut tree, _text, id) = setup("");
    focus_field(&mut tree, id);
    tick(&mut tree);

    let update = tree.sync_accessibility();
    assert!(
        update.nodes.iter().any(|(_, n)| n.role() == Role::TextRun),
        "an empty focused field must still emit a TextRun child"
    );
}

#[test]
fn composing_field_exposes_the_composition_as_an_at_selection() {
    let (mut tree, _text, id) = setup("");
    focus_field(&mut tree, id);
    tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "nihao".to_string(),
        cursor: Some(5..5),
    });
    tick(&mut tree);
    tick(&mut tree);

    let update = tree.sync_accessibility();
    let (_, node) = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == teksilo_core::accesskit::Role::TextInput)
        .expect("a text-input node is present");
    assert_eq!(
        node.value(),
        Some("nihao"),
        "composing text is in the value"
    );
    let sel = node
        .text_selection()
        .expect("composing field exposes a text selection");
    assert_ne!(
        sel.anchor.character_index, sel.focus.character_index,
        "the composition is exposed as a non-empty selection so AT tracks it"
    );
}

#[test]
fn input_purpose_sets_specialised_at_role() {
    // WCAG 1.3.5 (audit G10): a field declared with an email purpose exposes
    // Role::EmailInput to assistive tech instead of a generic TextInput.
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(TextInput::new(text).input_purpose(crate::primitives::InputPurpose::Email));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    let update = tree.sync_accessibility();
    assert!(
        update
            .nodes
            .iter()
            .any(|(_, n)| n.role() == teksilo_core::accesskit::Role::EmailInput),
        "input_purpose(Email) must emit a Role::EmailInput node"
    );
}

// ── placeholder layout ─────────────────────────────────────────────

/// Find the placeholder `TextWidget`. Structurally it is the only
/// `TextWidget` whose parent is the align-mode `Expand` overlay in the
/// field's `ZStack` (the validation strip's `TextWidget` is parented by
/// `ValidationStrip`).
fn placeholder_text_widget(
    tree: &WidgetTree,
    root: teksilo_core::widget_id::WidgetId,
) -> teksilo_core::widget_id::WidgetId {
    fn walk(
        tree: &WidgetTree,
        id: teksilo_core::widget_id::WidgetId,
        found: &mut Vec<teksilo_core::widget_id::WidgetId>,
    ) {
        let is_text = tree
            .widget_type_name(id)
            .is_some_and(|n| n.ends_with("::TextWidget"));
        let parent_is_expand = tree
            .parent(id)
            .and_then(|p| tree.widget_type_name(p))
            .is_some_and(|n| n.ends_with("::Expand"));
        if is_text && parent_is_expand {
            found.push(id);
        }
        for c in tree.children(id) {
            walk(tree, c, found);
        }
    }
    let mut found = Vec::new();
    walk(tree, root, &mut found);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one Expand-parented TextWidget (the placeholder)"
    );
    found[0]
}

/// Regression: a placeholder wider than the field must be capped at the
/// text column's bounds so paint-time trailing-ellipsis truncation kicks
/// in (`draw_text` truncates at `bounds.width`), instead of the
/// placeholder painting its full untruncated line past the field frame.
#[test]
fn placeholder_is_capped_at_field_width_for_ellipsis() {
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(text).placeholder(lit!("A rather long placeholder sentence")));
    // 34 chars × 8 px (mock backend) = 272 px natural — far wider than
    // the 120 px field.
    tree.layout(SizeProposal::exact(120.0, 40.0));

    let ph = placeholder_text_widget(&tree, id);
    let pb = tree.bounds(ph);
    let ob = tree.bounds(id);
    assert!(
        pb.x >= ob.x - 0.01 && pb.x + pb.width <= ob.x + ob.width + 0.01,
        "placeholder must stay inside the field ({pb:?} vs {ob:?})"
    );
    assert!(
        pb.width < 272.0 - 0.01,
        "placeholder width must be capped below its natural line width, got {}",
        pb.width
    );
}

/// Regression: a short placeholder is pinned to the leading edge (where
/// the typed text starts), vertically centered — not centered
/// horizontally in the column.
#[test]
fn placeholder_is_leading_aligned() {
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(text).placeholder(lit!("Short hint")));
    tree.layout(SizeProposal::exact(400.0, 40.0));

    let ph = placeholder_text_widget(&tree, id);
    let pb = tree.bounds(ph);
    let column = tree.bounds(tree.parent(ph).expect("placeholder has a parent"));
    assert!(
        (pb.x - column.x).abs() < 0.01,
        "placeholder must sit at the column's leading edge (x={}, column.x={})",
        pb.x,
        column.x
    );
    assert!(
        pb.width < column.width / 2.0,
        "precondition: placeholder is much narrower than the column"
    );
    let ph_mid = pb.y + pb.height / 2.0;
    let col_mid = column.y + column.height / 2.0;
    assert!(
        (ph_mid - col_mid).abs() < 0.5,
        "placeholder stays on the column's vertical midline"
    );
}

/// The set of actions the field **advertises** to assistive technology.
///
/// An AT client acts on what a node advertises, not on what the widget can in
/// fact do: `accesskit_consumer` filters by `supports_action`, VoiceOver's
/// rotor and Narrator's scan build their verb lists from it, and an action
/// that is serviced but never advertised is an action no screen reader will
/// ever invoke. So the advertisement is a contract in its own right, and it
/// was not covered — deleting any of the `builder.add_action(...)` lines in
/// `primitives/text_input_field/widget_impl.rs` left the whole suite green,
/// because every existing test drives the *handler* directly.
///
/// Asserted once here on the primitive's node rather than four times over the
/// widgets built on it: `TextInput`, `PasswordField`, `SpinBox` and
/// `SearchField` all embed the same `TextInputField` and emit this node from
/// the same `accessibility()`. The two conditional pairs get their own case
/// below, since it is those conditions the set turns on.
#[test]
fn the_field_advertises_the_actions_an_assistive_client_may_invoke() {
    use teksilo_core::accesskit::Action;

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(TextInput::new(Signal::new("hello".to_string())));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    let field = tree
        .find_by_role(teksilo_core::accesskit::Role::TextInput)
        .expect("the composite builds a Role::TextInput field");
    let actions = tree.accessibility_node(field).actions().to_vec();

    for action in [
        Action::Focus,
        Action::SetValue,
        Action::ReplaceSelectedText,
        Action::SetTextSelection,
    ] {
        assert!(
            actions.contains(&action),
            "an editable field must advertise {action:?}; it advertises {actions:?}",
        );
    }

    // `ScrollIntoView` is deliberately absent, and this is the assertion that
    // keeps the omission a decision rather than a drift. `RichTextEditor` and
    // `CodeEditor` do advertise it, being scrollable surfaces — though only
    // `RichTextEditor` services it; `CodeEditor` and `LogView` advertise it and
    // answer `Ignored`, an open defect recorded in `docs/a11y/a11y_issues.md`
    // — whereas `handle_access_action` in `primitives/text_input_field.rs`
    // answers `Ignored` to it, and advertising an action nothing performs is
    // worse for a client than not advertising it: the verb appears in the
    // rotor and does nothing when chosen.
    assert!(
        !actions.contains(&Action::ScrollIntoView),
        "a field that does not service ScrollIntoView must not advertise it",
    );
}

/// The two conditional pairs, each at the condition that removes it.
///
/// A read-only field cannot be written, so `SetValue` and
/// `ReplaceSelectedText` come off — advertising them would offer a client a
/// dictation target that silently drops what it inserts. A *protected* field
/// (a masked `PasswordField`) additionally hides its caret model from AT, so
/// `SetTextSelection` comes off with it: there are no character positions
/// published for a client to select between.
#[test]
fn a_read_only_or_protected_field_withdraws_the_actions_it_cannot_service() {
    use teksilo_core::accesskit::Action;

    let read_only = {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(TextInput::new(Signal::new("hello".to_string())).read_only(true));
        tree.layout(SizeProposal::exact(300.0, 40.0));
        let field = tree
            .find_by_role(teksilo_core::accesskit::Role::TextInput)
            .expect("a read-only TextInput is still a Role::TextInput");
        tree.accessibility_node(field).actions().to_vec()
    };
    assert!(
        read_only.contains(&Action::Focus),
        "a read-only field is still focusable; it advertises {read_only:?}",
    );
    assert!(
        !read_only.contains(&Action::SetValue) && !read_only.contains(&Action::ReplaceSelectedText),
        "a read-only field must not advertise a write; it advertises {read_only:?}",
    );
    assert!(
        read_only.contains(&Action::SetTextSelection),
        "but its caret model is still published, so selecting stays on offer",
    );

    let protected = {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(crate::PasswordField::new(Signal::new(
            "hunter2".to_string(),
        )));
        tree.layout(SizeProposal::exact(300.0, 40.0));
        let field = tree
            .find_by_role(teksilo_core::accesskit::Role::PasswordInput)
            .expect("a PasswordField builds a Role::PasswordInput field");
        tree.accessibility_node(field).actions().to_vec()
    };
    assert!(
        !protected.contains(&Action::SetTextSelection),
        "a masked field publishes no character positions, so it must not offer \
         a selection action; it advertises {protected:?}",
    );
}

/// `TextInput::label` must name the node a screen reader can actually see.
///
/// The composite's outer node is a `Role::GenericContainer`, and
/// `accesskit_consumer::common_filter` drops that role from the filtered tree
/// unconditionally — so a name written there is announced by nothing on any
/// platform. The name belongs on the inner `TextInputField`, which carries
/// `Role::TextInput` and holds focus.
#[test]
fn label_names_the_text_input_node_not_the_filtered_container() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(TextInput::new(Signal::new(String::new())).label(lit!("Search")));
    tree.layout(SizeProposal::exact(300.0, 40.0));

    let field = tree
        .find_by_role(teksilo_core::accesskit::Role::TextInput)
        .expect("the composite builds a Role::TextInput field");
    assert_eq!(
        tree.accessibility_node(field).name().map(str::to_owned),
        Some("Search".to_string()),
        "the labelled name must sit on the field, which survives the AT filter"
    );

    // And the outer container stays anonymous — naming a filtered role is
    // what made the label invisible in the first place.
    let outer = tree.accessibility_node(id);
    assert_eq!(
        outer.role(),
        teksilo_core::accesskit::Role::GenericContainer
    );
    assert_eq!(outer.name(), None);
}

/// The handle names the node that actually takes focus.
///
/// A host with several fields has to say *which* one focus starts on, and the
/// id `ctx.add` hands back is the composite's — a `Role::GenericContainer`
/// that is not focusable and is dropped from the filtered accessibility tree.
/// Focusing it would put focus nowhere.
#[test]
fn the_field_handle_names_the_node_that_takes_focus() {
    let input = TextInput::new(Signal::new(String::new())).label(lit!("Title"));
    let handle = input.field_id();
    assert_eq!(handle.get(), None, "empty until build runs");

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let outer = tree.add(input);
    tree.layout(SizeProposal::exact(300.0, 40.0));

    let field = handle.get().expect("build fills the handle");
    assert_ne!(field, outer, "the field is not the composite");
    assert_eq!(
        tree.accessibility_node(field).role(),
        teksilo_core::accesskit::Role::TextInput,
        "the handle names the field, which is the node that survives the filter"
    );

    tree.focus(field);
    tree.request_frame();
    assert_eq!(
        tree.focused(),
        Some(field),
        "and focus lands on it, which the composite's own id could not do"
    );
}

/// Deferred modal content directs focus into the field without the handle.
///
/// The modal pipeline asks the content tree for an `initial_focus_hint`
/// before falling back to the first focusable descendant. A composite that
/// answered nothing was skipped, so a dialog whose content is one text field
/// opened with focus nowhere in particular.
#[test]
fn the_composite_hints_at_its_own_field() {
    let input = TextInput::new(Signal::new(String::new())).label(lit!("Title"));
    let handle = input.field_id();

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let outer = tree.add(input);
    tree.layout(SizeProposal::exact(300.0, 40.0));

    let field = handle.get().expect("build fills the handle");
    assert_eq!(
        tree.widget_initial_focus_hint(outer),
        Some(field),
        "the hint has to be the field, not the container that cannot hold focus"
    );
}

// ---------------------------------------------------------------------------
// Touch: the clear affordance
// ---------------------------------------------------------------------------

/// The clear affordance is 16 dp of paint inside a field that takes presses of
/// its own for the caret, so neither the miss-only slop pass (defeated by a
/// bubble owner at distance zero) nor a bigger `MinSize` (pinned by the slot
/// that reserves the row's space) can reach it. It earns its 24 dp from the
/// pointer side: a finger 3 dp beside it still clears the field.
#[test]
fn a_finger_just_outside_the_clear_button_still_clears() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::pointer::PointerPhase;

    let text = Signal::new("hello".to_string());
    let row_taps = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, field) = field_in_a_tappable_row(text.clone(), row_taps.clone());

    let clear = clear_slot(&tree, field).expect("the clear slot is in the row");
    assert!(
        clear.width < 24.0,
        "the slot is meant to stay at 16 dp, got {clear:?}"
    );
    // 3 dp above the affordance: inside the field's frame, outside the 16 dp
    // slot, inside the 24 dp target the outset earns it.
    let at = Point::new(clear.center().x, clear.y - 3.0);
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 30));
    assert_eq!(
        text.get(),
        "",
        "the clear affordance's outset did not take the press"
    );
    assert_eq!(row_taps.get(), 0, "and the row must not have taken it too");
}

/// …and a mouse there does not: an exact hot-spot 3 dp outside the affordance
/// is a click in the text field, exactly as it was before the touch programme.
#[test]
fn a_mouse_just_outside_the_clear_button_does_not_clear() {
    use teksilo_canvas::Point;
    use teksilo_core::event::PointerButton;

    let text = Signal::new("hello".to_string());
    let row_taps = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, field) = field_in_a_tappable_row(text.clone(), row_taps.clone());

    let clear = clear_slot(&tree, field).expect("the clear slot is in the row");
    let at = Point::new(clear.center().x, clear.y - 3.0);
    tree.pointer_down_button(at, PointerButton::Primary);
    tree.pointer_up_button(at, PointerButton::Primary);
    assert_eq!(text.get(), "hello");
    assert_eq!(row_taps.get(), 1, "the press belongs to the row");
}

/// An empty field's slot still reserves its room, so the row does not jump when
/// the affordance appears — and it withdraws its outset, so an invisible
/// affordance never swallows a press meant for the field.
#[test]
fn an_empty_fields_clear_slot_reserves_room_but_claims_nothing() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::pointer::PointerPhase;

    let text = Signal::new(String::new());
    let row_taps = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, field) = field_in_a_tappable_row(text.clone(), row_taps.clone());
    let clear = clear_slot(&tree, field).expect("the slot is still laid out");
    assert!(clear.width > 0.0, "an empty field still reserves the slot");

    let at = Point::new(clear.center().x, clear.y - 3.0);
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 30));
    assert_eq!(text.get(), "", "nothing to clear, and nothing crashed");
    // …and the withdrawn outset is what lets the press through: a widened slot
    // that then refuses it would be a hole punched in the row behind it.
    assert_eq!(
        row_taps.get(),
        1,
        "the inactive slot kept the outset and swallowed the row's press",
    );
}

/// The clear handler's own emptiness guard, pressed where it can be reached.
///
/// The slot keeps its 16 dp of room while the field is empty — that is what
/// stops the row jumping when the affordance appears — and the node that
/// reserves that room is the same node that carries `on_tap`. Only the glyph
/// inside it goes dormant. So the handler *is* reachable on an empty field, by
/// the obvious gesture of pressing where the clear button will be, and without
/// the guard it answers by writing `String::new()` into the bound text signal.
///
/// The value does not change, so nothing on screen moves; what moves is the
/// signal's observers. Every consumer of a `TextInput`'s text — a validation
/// pass, a live search, a dirty flag on a settings page, the field's own
/// external→internal effect — is told the text changed because a finger landed
/// on a control that was showing nothing.
///
/// The neighbouring test presses 3 dp *above* the slot, which is the outset
/// question, not this one: it never reaches the handler at all.
#[test]
fn pressing_an_empty_fields_clear_slot_notifies_nobody() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_core::pointer::PointerPhase;

    let text = Signal::new(String::new());
    let notifications = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let counter = notifications.clone();
    let _observer = text.observe(move |_| counter.set(counter.get() + 1));

    let row_taps = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let (mut tree, field) = field_in_a_tappable_row(text.clone(), row_taps.clone());
    let clear = clear_slot(&tree, field).expect("the slot is still laid out");

    // Dead centre of the slot — not beside it. Nothing here is a near miss.
    let at = clear.center();
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 30));

    assert_eq!(text.get(), "", "there was nothing to clear");
    assert_eq!(
        notifications.get(),
        0,
        "pressing the empty field's clear slot wrote to the text signal",
    );
}

/// A field with a clear affordance, inside a settings row that takes taps of
/// its own — the shape the census names as the reason the miss-only slop pass
/// cannot serve the affordance.
///
/// The row is what makes these two tests discriminate. The slop pass only wins
/// when the exact hit's bubble path carries no eligible handler, or when its
/// candidate is strictly closer than that path's owner; the row owns the press
/// at distance zero, and a near-miss does not beat zero. So the only mechanism
/// left that can carry a press beside the affordance onto it is the slot's own
/// `HitTarget::hit_outset` — with a bare field, the slop pass served the press
/// either way and the test could not tell the two apart.
fn field_in_a_tappable_row(
    text: Signal<String>,
    row_taps: std::rc::Rc<std::cell::Cell<u32>>,
) -> (WidgetTree, teksilo_core::widget_id::WidgetId) {
    use teksilo_core::widget_builder::WidgetBuilder;

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let field = tree.add(TextInput::new(text).show_clear_button(true));
    let row = tree.add(
        crate::primitives::HStack::new()
            .add_child(field)
            .on_tap(move |_e, _c| row_taps.set(row_taps.get() + 1)),
    );
    let _ = row;
    tree.layout(SizeProposal::exact(240.0, 40.0));
    (tree, field)
}

/// The `HitTarget` slot that carries the clear affordance, by type.
fn clear_slot(
    tree: &WidgetTree,
    field: teksilo_core::widget_id::WidgetId,
) -> Option<teksilo_canvas::Rect> {
    let mut stack = vec![field];
    while let Some(id) = stack.pop() {
        if tree
            .widget_type_name(id)
            .is_some_and(|n| n.contains("HitTarget"))
        {
            return Some(tree.bounds(id));
        }
        stack.extend(tree.children(id).iter().copied());
    }
    None
}
