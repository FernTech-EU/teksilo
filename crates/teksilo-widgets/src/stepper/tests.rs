// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit;
use teksilo_core::event::WidgetEvent;
use teksilo_core::signal::Signal;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use super::*;

// ── helpers ──────────────────────────────────────────────────────────────

fn tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn layout(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(720.0, 520.0));
}

/// Activate a widget via its advertised `Action::Click` (works for the footer
/// buttons and the clickable indicators alike).
fn access_click(tree: &mut WidgetTree, id: WidgetId) {
    tree.dispatch_event(WidgetEvent::AccessAction {
        action: accesskit::Action::Click,
        target: Some(id),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
}

fn click_label(tree: &mut WidgetTree, label: &str) {
    let id = tree
        .find_by_label(label)
        .unwrap_or_else(|| panic!("no widget labelled {label:?}"));
    access_click(tree, id);
    layout(tree);
}

fn three_steps() -> Vec<Step> {
    vec![
        Step::new(lit!("One")).content(|| crate::button::Button::new(lit!("body 1"))),
        Step::new(lit!("Two")).content(|| crate::button::Button::new(lit!("body 2"))),
        Step::new(lit!("Three")).content(|| crate::button::Button::new(lit!("body 3"))),
    ]
}

fn nodes_with_role(
    update: &accesskit::TreeUpdate,
    role: accesskit::Role,
) -> Vec<(accesskit::NodeId, &accesskit::Node)> {
    update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == role)
        .map(|(id, n)| (*id, n))
        .collect()
}

fn node_by_label<'a>(
    update: &'a accesskit::TreeUpdate,
    label: &str,
) -> Option<&'a accesskit::Node> {
    update
        .nodes
        .iter()
        .find(|(_, n)| n.label() == Some(label))
        .map(|(_, n)| n)
}

fn node_id_by_label(update: &accesskit::TreeUpdate, label: &str) -> Option<accesskit::NodeId> {
    update
        .nodes
        .iter()
        .find(|(_, n)| n.label() == Some(label))
        .map(|(id, _)| *id)
}

// ── build / embedding ──────────────────────────────────────────────────────

#[test]
fn embeds_in_layout() {
    let mut t = tree();
    let id = t.add(Stepper::new().steps(three_steps()));
    layout(&mut t);
    assert!(t.bounds(id).width > 0.0 && t.bounds(id).height > 0.0);
}

#[test]
#[should_panic(expected = "requires .content(...)")]
fn step_without_content_panics() {
    let mut t = tree();
    t.add(Stepper::new().step(Step::new(lit!("Bare"))));
    layout(&mut t);
}

// ── navigation ───────────────────────────────────────────────────────────

#[test]
fn forward_and_back_navigation() {
    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(Stepper::new().controller(ctrl.clone()).steps(three_steps()));
    layout(&mut t);

    assert_eq!(ctrl.current(), 0);
    click_label(&mut t, "Next");
    assert_eq!(ctrl.current(), 1);
    click_label(&mut t, "Next");
    assert_eq!(ctrl.current(), 2);
    click_label(&mut t, "Back");
    assert_eq!(ctrl.current(), 1, "Back returns to the last visited step");
}

#[test]
fn non_linear_jump_via_indicator_click() {
    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .non_linear(true)
            .steps(three_steps()),
    );
    layout(&mut t);

    // The indicator carries the step title as its accessible name.
    click_label(&mut t, "Three");
    assert_eq!(ctrl.current(), 2);
}

#[test]
fn visit_history_back_is_non_sequential() {
    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .non_linear(true)
            .steps(three_steps()),
    );
    layout(&mut t);

    ctrl.go_to(2); // 0 → 2 (skipping 1)
    layout(&mut t);
    assert_eq!(ctrl.current(), 2);
    ctrl.back();
    layout(&mut t);
    assert_eq!(
        ctrl.current(),
        0,
        "Back pops the visit history, not index-1"
    );
}

// ── validation gating ──────────────────────────────────────────────────────

#[test]
fn validation_gates_next() {
    let gate = Signal::new(false);
    let steps = vec![
        Step::new(lit!("One"))
            .content(|| crate::button::Button::new(lit!("body 1")))
            .complete_when(gate.clone()),
        Step::new(lit!("Two")).content(|| crate::button::Button::new(lit!("body 2"))),
    ];
    let mut t = tree();
    t.add(Stepper::new().steps(steps));
    layout(&mut t);

    let update = t.sync_accessibility();
    assert!(
        node_by_label(&update, "Next")
            .expect("Next button")
            .is_disabled(),
        "Next is disabled until the active step's gate is satisfied"
    );

    gate.set(true);
    layout(&mut t);
    let update = t.sync_accessibility();
    assert!(
        !node_by_label(&update, "Next")
            .expect("Next button")
            .is_disabled(),
        "Next enables when the gate flips true"
    );
}

#[test]
fn optional_step_skip_advances_and_marks_skipped() {
    let ctrl = StepperController::new(3);
    let steps = vec![
        Step::new(lit!("One")).content(|| crate::button::Button::new(lit!("b1"))),
        Step::new(lit!("Two"))
            .optional(true)
            .content(|| crate::button::Button::new(lit!("b2"))),
        Step::new(lit!("Three")).content(|| crate::button::Button::new(lit!("b3"))),
    ];
    let mut t = tree();
    t.add(Stepper::new().controller(ctrl.clone()).steps(steps));
    layout(&mut t);

    click_label(&mut t, "Next"); // → step 1 (optional)
    assert_eq!(ctrl.current(), 1);
    click_label(&mut t, "Skip"); // skip optional step
    assert_eq!(ctrl.current(), 2);
    assert!(ctrl.skipped(1), "optional step is recorded as skipped");
}

#[test]
fn validate_on_next_blocks_then_clears_error_on_pass() {
    let ok = Signal::new(false);
    let ok_check = ok.clone();
    let ctrl = StepperController::new(2);
    let steps = vec![
        Step::new(lit!("One"))
            .content(|| crate::button::Button::new(lit!("b1")))
            .validate_on_next(move || ok_check.get()),
        Step::new(lit!("Two")).content(|| crate::button::Button::new(lit!("b2"))),
    ];
    let mut t = tree();
    t.add(Stepper::new().controller(ctrl.clone()).steps(steps));
    layout(&mut t);

    // Validation fails → stays on step 0, marked Error.
    click_label(&mut t, "Next");
    assert_eq!(ctrl.current(), 0);
    assert_eq!(ctrl.status(0), StepStatus::Error);

    // Fix the input → Next now advances and the step is no longer Error.
    ok.set(true);
    click_label(&mut t, "Next");
    assert_eq!(ctrl.current(), 1);
    assert_eq!(ctrl.status(0), StepStatus::Complete);
}

#[test]
fn content_boxed_takes_a_body_whose_type_varies_at_runtime() {
    // The branching-body case `content` cannot express: `Box<dyn Widget>` is
    // not itself a `Widget`, so one generic factory cannot return two types.
    #[derive(Clone, Copy, PartialEq)]
    enum Purpose {
        Novel,
        Import,
    }
    let purpose = Signal::new(Purpose::Import);

    let mut t = tree();
    t.add(
        Stepper::new().step(Step::new(lit!("Details")).content_boxed({
            let purpose = purpose.clone();
            move || -> Box<dyn teksilo_core::widget::Widget> {
                match purpose.get() {
                    Purpose::Novel => Box::new(crate::button::Button::new(lit!("novel form"))),
                    Purpose::Import => Box::new(crate::text_input::TextInput::new(Signal::new(
                        String::new(),
                    ))),
                }
            }
        })),
    );
    layout(&mut t);

    assert!(
        t.find_by_label("novel form").is_none(),
        "the Import branch was selected"
    );
}

// ── finish can refuse ──────────────────────────────────────────────────────

#[test]
fn on_finish_rejection_keeps_the_step_and_marks_it_error() {
    let ok = Signal::new(false);
    let attempts = Rc::new(std::cell::Cell::new(0_u32));
    let ctrl = StepperController::new(2);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .steps(vec![
                Step::new(lit!("One")).content(|| crate::button::Button::new(lit!("b1"))),
                Step::new(lit!("Two")).content(|| crate::button::Button::new(lit!("b2"))),
            ])
            .on_finish({
                let ok = ok.clone();
                let attempts = attempts.clone();
                move |_ctx, _ctrl| {
                    attempts.set(attempts.get() + 1);
                    ok.get() // `false` → FinishOutcome::Rejected
                }
            }),
    );
    layout(&mut t);

    click_label(&mut t, "Next");
    assert_eq!(ctrl.current(), 1);

    // The commit failed: stay on the last step, mark it Error.
    click_label(&mut t, "Finish");
    assert_eq!(attempts.get(), 1);
    assert_eq!(ctrl.current(), 1, "a rejected finish does not navigate");
    assert_eq!(ctrl.status(1), StepStatus::Error);

    // Retry after fixing whatever failed.
    ok.set(true);
    click_label(&mut t, "Finish");
    assert_eq!(attempts.get(), 2);
    assert_eq!(ctrl.status(1), StepStatus::Complete);
}

#[test]
fn on_finish_returning_unit_still_completes() {
    // The pre-existing `()` signature keeps working through IntoFinishOutcome.
    let ran = Rc::new(std::cell::Cell::new(false));
    let ctrl = StepperController::new(1);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .step(Step::new(lit!("Only")).content(|| crate::button::Button::new(lit!("b"))))
            .on_finish({
                let ran = ran.clone();
                move |_ctx, _ctrl| ran.set(true)
            }),
    );
    layout(&mut t);

    click_label(&mut t, "Finish");
    assert!(ran.get());
    assert_eq!(ctrl.status(0), StepStatus::Complete);
}

// ── step visibility (branching flows) ──────────────────────────────────────

/// Three steps where the middle one is gated on `shown`.
fn branching(shown: &Signal<bool>) -> Vec<Step> {
    vec![
        Step::new(lit!("One")).content(|| crate::button::Button::new(lit!("b1"))),
        Step::new(lit!("Two"))
            .visible_when(shown.clone())
            .content(|| crate::button::Button::new(lit!("b2"))),
        Step::new(lit!("Three")).content(|| crate::button::Button::new(lit!("b3"))),
    ]
}

#[test]
fn hidden_step_is_skipped_forward_and_backward() {
    let shown = Signal::new(false);
    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .steps(branching(&shown)),
    );
    layout(&mut t);

    click_label(&mut t, "Next");
    assert_eq!(ctrl.current(), 2, "Next steps over the hidden middle step");
    click_label(&mut t, "Back");
    assert_eq!(ctrl.current(), 0, "Back does too");
}

#[test]
fn step_visibility_is_reactive() {
    let shown = Signal::new(false);
    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .steps(branching(&shown)),
    );
    layout(&mut t);

    shown.set(true); // the user's earlier choice now selects this branch
    layout(&mut t);
    click_label(&mut t, "Next");
    assert_eq!(ctrl.current(), 1, "the revealed step joins the flow");
}

#[test]
fn hidden_step_leaves_the_indicator_strip() {
    let shown = Signal::new(false);
    let mut t = tree();
    t.add(Stepper::new().non_linear(true).steps(branching(&shown)));
    layout(&mut t);

    let update = t.sync_accessibility();
    assert!(
        node_by_label(&update, "Two").is_none(),
        "a hidden step must not advertise a marker navigation refuses to reach"
    );
    assert!(node_by_label(&update, "Three").is_some());
}

#[test]
fn trailing_hidden_steps_turn_next_into_finish() {
    // Steps 1 and 2 hidden: step 0 is the last reachable one, so the footer
    // must offer Finish, not Next.
    let shown = Signal::new(false);
    let mut t = tree();
    t.add(Stepper::new().steps(vec![
            Step::new(lit!("One")).content(|| crate::button::Button::new(lit!("b1"))),
            Step::new(lit!("Two"))
                .visible_when(shown.clone())
                .content(|| crate::button::Button::new(lit!("b2"))),
            Step::new(lit!("Three"))
                .visible_when(shown.clone())
                .content(|| crate::button::Button::new(lit!("b3"))),
        ]));
    layout(&mut t);

    assert!(t.find_by_label("Next").is_none());
    assert!(t.find_by_label("Finish").is_some());

    shown.set(true);
    layout(&mut t);
    assert!(t.find_by_label("Next").is_some(), "Next returns with them");
}

#[test]
fn disabled_step_is_skipped_by_next() {
    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(Stepper::new().controller(ctrl.clone()).steps(three_steps()));
    layout(&mut t);

    ctrl.set_status(1, StepStatus::Disabled);
    layout(&mut t);
    click_label(&mut t, "Next");
    assert_eq!(ctrl.current(), 2, "next() walks past a Disabled step");
}

#[test]
fn go_to_refuses_an_unreachable_step() {
    let shown = Signal::new(false);
    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .non_linear(true)
            .steps(branching(&shown)),
    );
    layout(&mut t);

    ctrl.go_to(1);
    assert_eq!(ctrl.current(), 0, "cannot jump to a hidden step");
    ctrl.set_status(2, StepStatus::Disabled);
    ctrl.go_to(2);
    assert_eq!(ctrl.current(), 0, "nor to a disabled one");
}

// ── controller ─────────────────────────────────────────────────────────────

#[test]
fn reset_restores_the_declared_statuses() {
    let ctrl = StepperController::new(3);
    let steps = vec![
        Step::new(lit!("One")).content(|| crate::button::Button::new(lit!("b1"))),
        Step::new(lit!("Two"))
            .status(StepStatus::Disabled)
            .content(|| crate::button::Button::new(lit!("b2"))),
        Step::new(lit!("Three"))
            .optional(true)
            .content(|| crate::button::Button::new(lit!("b3"))),
    ];
    let mut t = tree();
    t.add(Stepper::new().controller(ctrl.clone()).steps(steps));
    layout(&mut t);

    click_label(&mut t, "Next"); // 0 → 2, stepping over the disabled step
    assert_eq!(ctrl.current(), 2);

    ctrl.reset();
    assert_eq!(ctrl.current(), 0);
    assert_eq!(
        ctrl.status(1),
        StepStatus::Disabled,
        "reset restores the declared status instead of blanket Upcoming"
    );
    assert_eq!(ctrl.status(2), StepStatus::Optional);
}

#[test]
fn controller_reset_and_go_to() {
    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .non_linear(true)
            .steps(three_steps()),
    );
    layout(&mut t);

    ctrl.go_to(2);
    assert_eq!(ctrl.current(), 2);
    ctrl.reset();
    assert_eq!(ctrl.current(), 0);
    assert_eq!(ctrl.status(1), StepStatus::Upcoming);
    assert!(!ctrl.skipped(1));
}

// ── data flow ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Plan {
    Free,
    Pro,
}

#[test]
fn finish_reads_form_values_and_skipped_introspection() {
    // The app owns its state as signals; steps write them, on_finish reads.
    let name = Signal::new(String::new());
    let plan = Signal::new(Plan::Free);
    let captured: Rc<RefCell<Option<(String, Plan, bool)>>> = Rc::new(RefCell::new(None));

    // The step bodies would bind these signals to real inputs; the test
    // drives the signals directly below to simulate user entry.
    let steps = {
        let name_w = name.clone();
        let plan_w = plan.clone();
        vec![
            Step::new(lit!("Name")).content(move || {
                let _ = &name_w;
                crate::button::Button::new(lit!("name field"))
            }),
            Step::new(lit!("Plan")).content(move || {
                let _ = &plan_w;
                crate::button::Button::new(lit!("pick plan"))
            }),
            Step::new(lit!("Extras"))
                .optional(true)
                .content(|| crate::button::Button::new(lit!("extras"))),
        ]
    };

    let ctrl = StepperController::new(3);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .steps(steps)
            .on_finish({
                let name = name.clone();
                let plan = plan.clone();
                let captured = captured.clone();
                move |_ctx, ctrl| {
                    *captured.borrow_mut() = Some((name.get(), plan.get(), ctrl.skipped(2)));
                }
            }),
    );
    layout(&mut t);

    // Simulate user input + a choice.
    name.set("Ada".to_string());
    plan.set(Plan::Pro);

    click_label(&mut t, "Next"); // 0 → 1
    click_label(&mut t, "Next"); // 1 → 2 (last)
    click_label(&mut t, "Finish");

    let got = captured.borrow().clone().expect("finish ran");
    assert_eq!(got.0, "Ada");
    assert_eq!(got.1, Plan::Pro);
    assert!(!got.2, "step 2 was reached via Next, not skipped");
}

// ── Enter advances ─────────────────────────────────────────────────────────

fn press_enter(t: &mut WidgetTree) {
    use teksilo_core::event::{Key, Modifiers};
    t.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        text: None,
    });
    t.dispatch_event(WidgetEvent::KeyUp {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
    });
    layout(t);
}

/// A step body standing in for a form field: focusable, and it does not claim
/// Enter for itself — so Enter bubbles to the stepper.
fn form_steps() -> Vec<Step> {
    use teksilo_core::widget_builder::WidgetBuilder;
    vec![
        Step::new(lit!("One"))
            .content(|| crate::primitives::TextWidget::new(lit!("field one")).focusable(true)),
        Step::new(lit!("Two"))
            .content(|| crate::primitives::TextWidget::new(lit!("field two")).focusable(true)),
    ]
}

#[test]
fn enter_in_a_step_form_activates_next() {
    let ctrl = StepperController::new(2);
    let mut t = tree();
    t.add(Stepper::new().controller(ctrl.clone()).steps(form_steps()));
    layout(&mut t);

    let field = t.find_by_label("field one").expect("focusable field");
    t.focus(field);
    press_enter(&mut t);
    assert_eq!(ctrl.current(), 1, "Enter on a step form means Next");
}

#[test]
fn enter_on_the_last_step_finishes() {
    let finished = Rc::new(std::cell::Cell::new(false));
    let ctrl = StepperController::new(1);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .step(Step::new(lit!("Only")).content(|| {
                use teksilo_core::widget_builder::WidgetBuilder;
                crate::primitives::TextWidget::new(lit!("field")).focusable(true)
            }))
            .on_finish({
                let finished = finished.clone();
                move |_ctx, _ctrl| finished.set(true)
            }),
    );
    layout(&mut t);

    let field = t.find_by_label("field").unwrap();
    t.focus(field);
    press_enter(&mut t);
    assert!(finished.get(), "Enter on the last step means Finish");
}

#[test]
fn enter_respects_the_completion_gate() {
    use teksilo_core::widget_builder::WidgetBuilder;
    let gate = Signal::new(false);
    let ctrl = StepperController::new(2);
    let mut t = tree();
    t.add(Stepper::new().controller(ctrl.clone()).steps(vec![
                Step::new(lit!("One"))
                    .complete_when(gate.clone())
                    .content(|| {
                        crate::primitives::TextWidget::new(lit!("field one")).focusable(true)
                    }),
                Step::new(lit!("Two")).content(|| crate::button::Button::new(lit!("b2"))),
            ]));
    layout(&mut t);

    let field = t.find_by_label("field one").unwrap();
    t.focus(field);
    press_enter(&mut t);
    assert_eq!(ctrl.current(), 0, "Enter is gated exactly like the button");

    gate.set(true);
    layout(&mut t);
    t.focus(field);
    press_enter(&mut t);
    assert_eq!(ctrl.current(), 1);
}

#[test]
fn enter_is_left_to_a_focused_control_that_claims_it() {
    // A Button consumes Enter on the way up, so the stepper never sees it —
    // this is why the handler sits on the bubble pass, not the preview pass.
    let ctrl = StepperController::new(2);
    let mut t = tree();
    t.add(Stepper::new().controller(ctrl.clone()).steps(three_steps()));
    layout(&mut t);

    let body = t.find_by_label("body 1").expect("step body button");
    t.focus(body);
    press_enter(&mut t);
    assert_eq!(
        ctrl.current(),
        0,
        "the focused button handled Enter; the stepper must not also advance"
    );
}

#[test]
fn enter_advances_can_be_turned_off() {
    let ctrl = StepperController::new(2);
    let mut t = tree();
    t.add(
        Stepper::new()
            .controller(ctrl.clone())
            .enter_advances(false)
            .steps(form_steps()),
    );
    layout(&mut t);

    let field = t.find_by_label("field one").unwrap();
    t.focus(field);
    press_enter(&mut t);
    assert_eq!(ctrl.current(), 0);
}

// ── accessibility ──────────────────────────────────────────────────────────

#[test]
fn a11y_strip_is_tablist_when_non_linear_else_list() {
    let mut t = tree();
    t.add(Stepper::new().non_linear(true).steps(three_steps()));
    layout(&mut t);
    let update = t.sync_accessibility();
    assert_eq!(nodes_with_role(&update, accesskit::Role::TabList).len(), 1);
    assert_eq!(nodes_with_role(&update, accesskit::Role::Tab).len(), 3);

    let mut t2 = tree();
    t2.add(Stepper::new().steps(three_steps())); // linear
    layout(&mut t2);
    let update2 = t2.sync_accessibility();
    assert!(nodes_with_role(&update2, accesskit::Role::TabList).is_empty());
    assert_eq!(nodes_with_role(&update2, accesskit::Role::List).len(), 1);
    assert_eq!(
        nodes_with_role(&update2, accesskit::Role::ListItem).len(),
        3
    );
}

#[test]
fn a11y_active_indicator_selected_and_aria_current() {
    let mut t = tree();
    t.add(Stepper::new().non_linear(true).steps(three_steps()));
    layout(&mut t);
    let update = t.sync_accessibility();
    let active = node_by_label(&update, "One").expect("first indicator");
    assert_eq!(active.is_selected(), Some(true));
    assert_eq!(active.aria_current(), Some(accesskit::AriaCurrent::Step));

    let inactive = node_by_label(&update, "Two").expect("second indicator");
    assert_eq!(inactive.aria_current(), None);
}

#[test]
fn a11y_indicators_have_posinset_and_setsize() {
    let mut t = tree();
    t.add(Stepper::new().non_linear(true).steps(three_steps()));
    layout(&mut t);
    let update = t.sync_accessibility();
    // Asked the way an adapter asks it: the position off the indicator, the
    // size by walking up to the indicator strip. In a wizard the total is the
    // whole point — it is what tells the user how much is left.
    for (label, pos) in [("One", 1), ("Two", 2), ("Three", 3)] {
        let id = node_id_by_label(&update, label).unwrap();
        crate::a11y_set_semantics::assert_announces(&update, id, pos, 3, &format!("step {label}"));
    }
}

#[test]
fn a11y_content_pane_labelled_by_indicator_no_dangling() {
    let mut t = tree();
    t.add(Stepper::new().non_linear(true).steps(three_steps()));
    layout(&mut t);
    let update = t.sync_accessibility();

    let panels = nodes_with_role(&update, accesskit::Role::TabPanel);
    assert_eq!(panels.len(), 1, "one active content panel");
    let panel = panels[0].1;
    assert!(
        !panel.labelled_by().is_empty(),
        "active panel is labelled-by its indicator"
    );

    // No relationship points outside the emitted tree.
    let emitted: std::collections::HashSet<accesskit::NodeId> =
        update.nodes.iter().map(|(id, _)| *id).collect();
    for (pid, node) in &update.nodes {
        for &target in node.controls() {
            assert!(
                emitted.contains(&target),
                "node {pid:?} controls absent {target:?}"
            );
        }
        for &target in node.labelled_by() {
            assert!(
                emitted.contains(&target),
                "node {pid:?} labelled_by absent {target:?}"
            );
        }
    }
}

// ── wizard modal launcher ────────────────────────────────────────────────────

#[test]
fn wizard_queues_modal() {
    let mut t = tree();
    t.add(
        Wizard::new(lit!("Open wizard"))
            .step(Step::new(lit!("One")).content(|| crate::button::Button::new(lit!("b1")))),
    );
    layout(&mut t);

    let trigger = t.find_by_label("Open wizard").unwrap();
    access_click(&mut t, trigger);
    assert_eq!(t.drain_pending_modal_requests().len(), 1);
}

// ── locale reactivity ────────────────────────────────────────────────────────

#[test]
fn a11y_strip_name_is_locale_reactive() {
    use teksilo_i18n::{
        I18nConfig, I18nManager, LanguageIdentifier,
        thread_local::{clear, install},
    };
    clear();
    let cfg = I18nConfig::test_only("en-US", &[])
        .with_locale("fr-FR", &[])
        .framework_locales(crate::framework_locales());
    let mgr = I18nManager::from_config(&cfg);
    install(mgr.clone());

    let mut t = tree();
    t.add(Stepper::new().non_linear(true).steps(three_steps()));
    layout(&mut t);

    let strip_name = |t: &mut WidgetTree| -> String {
        let update = t.sync_accessibility();
        nodes_with_role(&update, accesskit::Role::TabList)[0]
            .1
            .label()
            .unwrap_or("")
            .to_string()
    };
    assert_eq!(strip_name(&mut t), "Steps");

    let fr: LanguageIdentifier = "fr-FR".parse().unwrap();
    mgr.set_locale(fr);
    t.set_locale("fr-FR".to_string());
    layout(&mut t);
    assert_eq!(strip_name(&mut t), "Étapes");
    clear();
}

// ── tooltip ──────────────────────────────────────────────────────────────────

#[test]
fn tooltip_appears_on_hover() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(Stepper::new().steps(three_steps()).tooltip(lit!("Tip")));
    tree.layout(SizeProposal::exact(720.0, 520.0));
    tree.pointer_move(tree.bounds(id).center());
    tree.advance_time(std::time::Duration::from_secs(1));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "tooltip should appear on hover"
    );
    assert!(tree.find_by_label("Tip").is_some());
}
