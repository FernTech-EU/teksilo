// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a focus gain does to the field's text, by where the focus came from.
//!
//! Only Tab selects it all. The field used to infer "keyboard" from "the
//! pointer is not over me", so every focus that was not a click selected the
//! text: a dialog opening, a modal or popover closing and handing focus back,
//! an application's `request_focus`. The next key then replaced what the
//! reader had come back to.

use teksilo_canvas::SizeProposal;
use teksilo_core::FocusOrigin;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::overlay::{DismissBehavior, OverlayPlacement, OverlayRequest};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

use super::TextInputField;

fn frame(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

/// A tree holding one field with "hello" in it, and the field's id.
fn field_tree() -> (WidgetTree, Signal<String>, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let text = Signal::new("hello".to_string());
    let field = tree.add(TextInputField::new(text.clone()));
    frame(&mut tree);
    (tree, text, field)
}

fn type_str(tree: &mut WidgetTree, text: &str) {
    tree.type_text(WidgetId::default(), text);
    frame(tree);
}

fn press(tree: &mut WidgetTree, key: Key) {
    tree.press_key(key, Modifiers::NONE);
    frame(tree);
}

#[test]
fn tabbing_into_the_field_selects_its_text() {
    let (mut tree, text, field) = field_tree();
    press(&mut tree, Key::Tab);
    assert_eq!(tree.focused(), Some(field));
    type_str(&mut tree, "z");
    assert_eq!(
        text.get(),
        "z",
        "typing over a tabbed-into field replaces it"
    );
}

#[test]
fn a_programmatic_focus_leaves_the_text_alone() {
    let (mut tree, text, field) = field_tree();
    tree.focus(field);
    frame(&mut tree);
    type_str(&mut tree, "z");
    assert!(
        text.get().contains("hello"),
        "a focus the application gave must not let the next key wipe the field: {:?}",
        text.get()
    );
}

#[test]
fn an_assistive_focus_leaves_the_text_alone() {
    let (mut tree, text, field) = field_tree();
    tree.focus_with_origin(field, FocusOrigin::Accessibility);
    frame(&mut tree);
    type_str(&mut tree, "z");
    assert!(text.get().contains("hello"), "{:?}", text.get());
}

#[test]
fn a_closing_modal_hands_the_field_back_as_it_was() {
    // The caret at the start, then a modal over the field, closed by Escape.
    // The overlay's focus restore is a programmatic focus.
    let (mut tree, text, field) = field_tree();
    tree.focus_with_origin(field, FocusOrigin::Keyboard);
    frame(&mut tree);
    press(&mut tree, Key::Home);

    let button = tree.add(crate::primitives::RectWidget::new().focusable(true));
    tree.set_dormant(button);
    tree.activate(button);
    tree.show_overlay(OverlayRequest::new(
        button,
        field,
        OverlayPlacement::Centered,
        DismissBehavior::EscapeKey,
    ));
    tree.overlay_manager_mut().set_top_focus_restore(field);
    tree.focus(button);
    frame(&mut tree);
    press(&mut tree, Key::Escape);
    assert_eq!(tree.focused(), Some(field), "Escape hands focus back");

    type_str(&mut tree, "z");
    assert_eq!(
        text.get(),
        "zhello",
        "typed where the caret was, not over a selection the return made"
    );
}

/// A form of two fields that rebuilds whole when `epoch` bumps.
#[derive(Debug)]
struct Form {
    epoch: Signal<u32>,
    first: Signal<String>,
    second: Signal<String>,
    child: Option<WidgetId>,
}

impl Widget for Form {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.epoch
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        let id = ctx.add(
            crate::primitives::VStack::new()
                .child(TextInputField::new(self.first.clone()))
                .child(TextInputField::new(self.second.clone())),
        );
        self.child = Some(id);
        vec![id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_default()
            .into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

#[test]
fn a_rebuild_keeps_typing_in_the_same_field_at_its_end() {
    // Focus in the second field, then the form rebuilds. Focus used to come
    // back to the first field, so the next keys went there.
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let (epoch, first, second) = (
        Signal::new(0),
        Signal::new("alpha".to_string()),
        Signal::new("beta".to_string()),
    );
    let form = tree.add(Form {
        epoch: epoch.clone(),
        first: first.clone(),
        second: second.clone(),
        child: None,
    });
    frame(&mut tree);
    let fields = tree.tab_stops_within(form);
    tree.focus(fields[1]);
    frame(&mut tree);
    press(&mut tree, Key::Home);

    epoch.set(1);
    frame(&mut tree);
    frame(&mut tree);
    type_str(&mut tree, "Y");

    assert_eq!(first.get(), "alpha", "the first field is untouched");
    assert_eq!(
        second.get(),
        "betaY",
        "the same field, the caret at its end: its predecessor's caret is gone"
    );
}
