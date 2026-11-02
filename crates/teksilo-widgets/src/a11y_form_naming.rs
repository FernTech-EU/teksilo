// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A field in a `FormLayout` row is read by the row's label.
//!
//! The naming order is the one Qt and WinUI follow: a name the application
//! gave the control wins, then the label the row links it to, then the
//! control's built-in default. A checkbox, a radio button and a button keep
//! their own visible text. Every name here is resolved through
//! `accesskit_consumer`, the crate every platform adapter reads, so it is
//! the name a screen reader says, not the one a node happens to carry.

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::{Role, TreeUpdate};
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::primitives::{FixedSize, FormLayout, HStack, TextWidget, VStack};
use crate::{Checkbox, ComboBox, PasswordField, TextInput};

/// A tree speaking English, so the built-in names resolve to their text.
fn tree() -> WidgetTree {
    crate::common::locale_switch_test::speaking("en-US").1
}

fn laid_out(tree: &mut WidgetTree) -> TreeUpdate {
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    tree.sync_accessibility()
}

/// The name and value the consumer resolves for every node of `role`, in
/// tree order.
fn read(update: &TreeUpdate, role: Role) -> Vec<(Option<String>, Option<String>)> {
    let consumer = accesskit_consumer::Tree::new(update.clone(), false);
    let state = consumer.state();
    update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == role)
        .filter_map(|(id, _)| {
            state.node_by_tree_local_id(*id, teksilo_core::accesskit::TreeId::ROOT)
        })
        .map(|node| (node.label(), node.value()))
        .collect()
}

/// The one name the consumer resolves for the node of `role`.
fn name_of(update: &TreeUpdate, role: Role) -> Option<String> {
    let found = read(update, role);
    assert_eq!(
        found.len(),
        1,
        "expected one {role:?} node, found {found:?}"
    );
    found.into_iter().next().unwrap().0
}

fn row(field: impl Widget + 'static) -> FormLayout {
    FormLayout::new().line(TextWidget::new(lit!("Full name")), field)
}

fn text_input() -> TextInput {
    TextInput::new(Signal::new(String::new()))
}

#[test]
fn a_text_input_is_named_by_its_row() {
    let mut tree = tree();
    tree.add(row(text_input()));
    let update = laid_out(&mut tree);
    assert_eq!(
        name_of(&update, Role::TextInput).as_deref(),
        Some("Full name")
    );
}

#[test]
fn a_password_field_is_named_by_its_row() {
    let mut tree = tree();
    tree.add(row(PasswordField::new(Signal::new(String::new()))));
    let update = laid_out(&mut tree);
    assert_eq!(
        name_of(&update, Role::PasswordInput).as_deref(),
        Some("Full name")
    );
}

#[test]
fn a_combo_box_reads_the_row_as_its_name_and_the_choice_as_its_value() {
    let mut tree = tree();
    let combo = ComboBox::from_items(
        ["Apple".to_string(), "Pear".to_string()],
        Signal::new(Some("Pear".to_string())),
        |s: &String| lit!(s.clone()),
    );
    tree.add(row(combo));
    let update = laid_out(&mut tree);
    let found = read(&update, Role::ComboBox);
    assert_eq!(
        found,
        vec![(Some("Full name".to_string()), Some("Pear".to_string()))]
    );
}

#[test]
fn a_date_field_behind_a_wrapper_reads_the_row_not_its_default() {
    use crate::DateEdit;
    let mut tree = tree();
    tree.add(row(FixedSize::new()
        .width(220.0)
        .child(DateEdit::new(Signal::new(None)))));
    let update = laid_out(&mut tree);
    assert_eq!(
        name_of(&update, Role::TextInput).as_deref(),
        Some("Full name")
    );
}

#[test]
fn a_date_field_with_no_row_reads_its_default() {
    use crate::DateEdit;
    let mut tree = tree();
    tree.add(DateEdit::new(Signal::new(None)));
    let update = laid_out(&mut tree);
    assert_eq!(name_of(&update, Role::TextInput).as_deref(), Some("Date"));
}

#[test]
fn a_time_field_in_a_row_reads_the_row() {
    use crate::TimeEdit;
    let mut tree = tree();
    tree.add(row(TimeEdit::new(Signal::new(None))));
    let update = laid_out(&mut tree);
    assert_eq!(
        name_of(&update, Role::TextInput).as_deref(),
        Some("Full name")
    );
}

#[test]
fn a_label_behind_a_wrapper_names_a_spin_box() {
    use crate::SpinBox;
    let mut tree = tree();
    tree.add(
        FormLayout::new().line(
            FixedSize::new()
                .width(120.0)
                .child(TextWidget::new(lit!("Size"))),
            SpinBox::new(Signal::new(12_i32), 1, 72),
        ),
    );
    let update = laid_out(&mut tree);
    assert_eq!(name_of(&update, Role::SpinButton).as_deref(), Some("Size"));
}

#[test]
fn a_field_beside_a_hint_in_a_stack_is_named_by_the_row() {
    let mut tree = tree();
    tree.add(row(VStack::new()
        .child(text_input())
        .child(TextWidget::new(lit!("As on your passport")))));
    let update = laid_out(&mut tree);
    assert_eq!(
        name_of(&update, Role::TextInput).as_deref(),
        Some("Full name")
    );
}

#[test]
fn a_focusable_leading_slot_does_not_take_the_row_label() {
    use crate::Button;
    let mut tree = tree();
    tree.add(row(text_input().leading_slot(Button::new(lit!("Pick")))));
    let update = laid_out(&mut tree);
    assert_eq!(
        name_of(&update, Role::TextInput).as_deref(),
        Some("Full name")
    );
    assert_eq!(name_of(&update, Role::Button).as_deref(), Some("Pick"));
}

#[test]
fn a_cell_with_two_controls_names_neither() {
    use crate::Button;
    let mut tree = tree();
    tree.add(row(HStack::new()
        .child(text_input())
        .child(Button::new(lit!("Browse")))));
    let update = laid_out(&mut tree);
    assert_eq!(name_of(&update, Role::TextInput), None);
    assert_eq!(name_of(&update, Role::Button).as_deref(), Some("Browse"));
}

#[test]
fn a_field_rebuilt_inside_a_row_keeps_the_row_label() {
    use crate::DateEdit;
    let mut tree = tree();
    let input = tree.add(text_input());
    let date = tree.add(DateEdit::new(Signal::new(None)));
    tree.add(
        FormLayout::new()
            .line(TextWidget::new(lit!("Full name")), input)
            .line(TextWidget::new(lit!("Birthday")), date),
    );
    laid_out(&mut tree);
    for _ in 0..2 {
        tree.arena_mark_needs_rebuild_for_testing(input);
        tree.arena_mark_needs_rebuild_for_testing(date);
        laid_out(&mut tree);
    }
    let update = laid_out(&mut tree);
    let names: Vec<Option<String>> = read(&update, Role::TextInput)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        names,
        vec![Some("Full name".to_string()), Some("Birthday".to_string())]
    );
}

#[test]
fn a_checkbox_keeps_its_own_text_over_the_row() {
    let mut tree = tree();
    tree.add(row(
        Checkbox::new(Signal::new(false)).label(lit!("Email me"))
    ));
    let update = laid_out(&mut tree);
    assert_eq!(
        name_of(&update, Role::CheckBox).as_deref(),
        Some("Email me")
    );
}

#[test]
fn an_application_name_wins_over_the_row() {
    use crate::DateEdit;
    let cases: Vec<(Box<dyn Widget>, Role)> = vec![
        (Box::new(text_input().label(lit!("Given"))), Role::TextInput),
        (
            Box::new(text_input().access_label(lit!("Given"))),
            Role::TextInput,
        ),
        (
            Box::new(DateEdit::new(Signal::new(None)).label(lit!("Given"))),
            Role::TextInput,
        ),
        (
            Box::new(
                ComboBox::from_items(["A".to_string()], Signal::new(None), |s: &String| {
                    lit!(s.clone())
                })
                .label(lit!("Given")),
            ),
            Role::ComboBox,
        ),
    ];
    for (field, role) in cases {
        let mut tree = tree();
        let field = tree.add_boxed(field);
        tree.add(row_with(field));
        let update = laid_out(&mut tree);
        assert_eq!(name_of(&update, role).as_deref(), Some("Given"), "{role:?}");
    }
}

fn row_with(field: WidgetId) -> FormLayout {
    FormLayout::new().line(TextWidget::new(lit!("Full name")), field)
}

#[test]
fn a_language_switcher_reads_its_default_alone_and_the_row_in_a_form() {
    use crate::common::locale_switch_test::speaking;
    use crate::language_switcher::LanguageSwitcher;

    let (_mgr, mut alone) = speaking("fr-FR");
    alone.add(LanguageSwitcher::new());
    let update = laid_out(&mut alone);
    assert_eq!(name_of(&update, Role::ComboBox).as_deref(), Some("Langue"));

    let (_mgr, mut in_row) = speaking("fr-FR");
    in_row.add(row(LanguageSwitcher::new()));
    let update = laid_out(&mut in_row);
    assert_eq!(
        name_of(&update, Role::ComboBox).as_deref(),
        Some("Full name")
    );
}

#[test]
fn relations_given_to_a_text_input_land_on_its_field() {
    let mut tree = tree();
    let title = tree.add(TextWidget::new(lit!("Title")));
    let hint = tree.add(TextWidget::new(lit!("Shown on the cover")));
    let input = tree.add(
        text_input()
            .access_labelled_by(title)
            .access_described_by(hint),
    );
    let update = laid_out(&mut tree);
    let field = update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::TextInput)
        .map(|(_, node)| node.clone())
        .expect("a TextInput field");
    let nid = teksilo_core::accessibility::widget_id_to_node_id;
    assert_eq!(field.labelled_by(), &[nid(title)]);
    assert!(field.described_by().contains(&nid(hint)));
    assert_eq!(name_of(&update, Role::TextInput).as_deref(), Some("Title"));
    let _ = input;
}

#[test]
fn the_other_built_in_names_give_way_to_the_row_too() {
    use crate::text_scale_control::TextScaleControl;
    use crate::{DateRangeEdit, DateTimeEdit, ThemeSwitcher};
    let cases: Vec<(Box<dyn Widget>, Role)> = vec![
        (
            Box::new(DateTimeEdit::new(Signal::new(None))),
            Role::DateTimeInput,
        ),
        (
            Box::new(DateRangeEdit::new(Signal::new(None))),
            Role::DateInput,
        ),
        (
            Box::new(TextScaleControl::new(Signal::new(1.0))),
            Role::SpinButton,
        ),
        (Box::new(ThemeSwitcher::new()), Role::ComboBox),
    ];
    for (field, role) in cases {
        let mut tree = tree();
        let field = tree.add_boxed(field);
        tree.add(row_with(field));
        let update = laid_out(&mut tree);
        assert_eq!(
            name_of(&update, role).as_deref(),
            Some("Full name"),
            "{role:?}"
        );
    }
}

#[test]
fn text_scale_control_alone_reads_its_translated_name() {
    use crate::common::locale_switch_test::speaking;
    use crate::text_scale_control::TextScaleControl;
    let (_mgr, mut tree) = speaking("fr-FR");
    tree.add(TextScaleControl::new(Signal::new(1.0)));
    let update = laid_out(&mut tree);
    assert_eq!(
        name_of(&update, Role::SpinButton).as_deref(),
        Some("Taille du texte")
    );
    teksilo_i18n::thread_local::clear();
}

/// Two fields linked to each other: neither has a name to give the other
/// (each one's own default would yield), so both keep their default rather
/// than both reading nothing.
/// Builds two date fields and links each to the other, as a container does
/// with `BuildContext::access_labelled_by`.
#[derive(Debug)]
struct LinkedPair {
    root: Option<WidgetId>,
}

impl Widget for LinkedPair {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let a = ctx.add(crate::DateEdit::new(Signal::new(None)));
        let b = ctx.add(crate::DateEdit::new(Signal::new(None)));
        ctx.access_labelled_by(a, b);
        ctx.access_labelled_by(b, a);
        let root = ctx.add(VStack::new().child(a).child(b));
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

/// Two fields linked to each other: neither has a name to give the other
/// (each one's own default would yield), so both keep their default rather
/// than both reading nothing.
#[test]
fn two_defaults_linked_to_each_other_keep_them() {
    let mut tree = tree();
    tree.add(LinkedPair { root: None });
    let update = laid_out(&mut tree);
    let names: Vec<Option<String>> = read(&update, Role::TextInput)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        names,
        vec![Some("Date".to_string()), Some("Date".to_string())]
    );
}

/// A label left out of the accessibility tree names nothing, so the field
/// keeps its default.
#[test]
fn an_excluded_label_leaves_the_default() {
    use crate::DateEdit;
    let mut tree = tree();
    let label = tree.add(TextWidget::new(lit!("Birthday")));
    let hidden = tree.add(VStack::new().child(label).access_exclude_subtree());
    let field = tree.add(DateEdit::new(Signal::new(None)).access_labelled_by(label));
    tree.add(VStack::new().child(hidden).child(field));
    let update = laid_out(&mut tree);
    assert_eq!(name_of(&update, Role::TextInput).as_deref(), Some("Date"));
}

#[test]
fn a_toggle_in_a_row_keeps_its_own_text() {
    let mut tree = tree();
    tree.add(row(
        crate::Toggle::new(Signal::new(false)).label(lit!("Dark mode"))
    ));
    let update = laid_out(&mut tree);
    assert_eq!(name_of(&update, Role::Switch).as_deref(), Some("Dark mode"));
}
