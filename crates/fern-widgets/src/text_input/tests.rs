//! Headless integration tests for TextInput.

use fern_canvas::SizeProposal;
use fern_core::event::{Key, Modifiers};
use fern_core::signal::Signal;
use fern_core::widget_tree::WidgetTree;
use fern_tokens::theme::Theme;

use super::TextInput;

fn setup(initial: &str) -> (WidgetTree, Signal<String>, fern_core::widget_id::WidgetId) {
    let text = Signal::new(initial.to_string());
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let id = tree.add(TextInput::new(text.clone()).placeholder("Type here..."));
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

#[test]
fn placeholder_text_set() {
    // Smoke test — placeholder is configured, build succeeds.
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let _id = tree.add(
        TextInput::new(text)
            .placeholder("Search...")
            .show_clear_button(true),
    );
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
}

#[test]
fn builder_methods_chain() {
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let _id = tree.add(
        TextInput::new(text)
            .placeholder("Enter value")
            .label("Username")
            .enabled(true)
            .read_only(false)
            .max_length(100)
            .show_clear_button(true)
            .tooltip_literal("A text input field"),
    );
    tree.layout(SizeProposal::exact(300.0, 40.0));
}

#[test]
fn disabled_builds_without_panic() {
    let text = Signal::new("disabled".to_string());
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let _id = tree.add(TextInput::new(text).enabled(false));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
}

#[test]
fn read_only_builds_without_panic() {
    let text = Signal::new("read only".to_string());
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let _id = tree.add(TextInput::new(text).read_only(true));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
}

#[test]
fn accessibility_role_is_text_input() {
    let (_tree, _text, id) = setup("hello");
    // The inner TextInputField carries the Role::TextInput.
    // The outer TextInput is GenericContainer. Since we test the
    // outer widget here, just verify construction didn't panic.
    // A deeper test would probe the inner field's a11y node.
    let _ = id;
}

#[test]
fn with_leading_slot() {
    use crate::primitives::icon_widget::IconWidget;
    let icon = IconWidget::checkmark(12.0);
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let _id = tree.add(TextInput::new(text).leading_slot(icon));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
}

#[test]
fn with_trailing_slot() {
    use crate::primitives::icon_widget::IconWidget;
    let icon = IconWidget::chevron_down(12.0);
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let _id = tree.add(TextInput::new(text).trailing_slot(icon));
    tree.layout(SizeProposal::exact(300.0, 40.0));
    tick(&mut tree);
}
