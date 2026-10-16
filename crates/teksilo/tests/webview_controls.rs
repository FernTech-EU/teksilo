// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The default presenter of a page's dialogs and lists, driven headlessly.

#![cfg(all(feature = "web-view-headless", feature = "widgets", feature = "i18n"))]

use std::cell::RefCell;
use std::rc::Rc;

use teksilo::webview_install::WidgetsControlPresenter;
use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::{ModalContent, NoopWindowOps};
use teksilo_webview::{
    WebViewControl, WebViewControlAnswer, WebViewControlPresenter, WebViewSelectOption,
};

/// Present `control` and open the modal it asks for, as `teksilo-app` does.
fn present(control: WebViewControl) -> (WidgetTree, Rc<RefCell<Vec<WebViewControlAnswer>>>) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(teksilo_widgets::primitives::RectWidget::new());
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let answers = Rc::new(RefCell::new(Vec::new()));
    let sink = answers.clone();
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        WidgetsControlPresenter.present(
            control,
            ctx,
            Box::new(move |answer| sink.borrow_mut().push(answer)),
        );
    });
    let request = tree
        .drain_pending_modal_requests()
        .pop()
        .expect("a modal was asked for")
        .request;
    let content = match request.content {
        ModalContent::Deferred(build) => build(&mut tree),
        ModalContent::ExistingWidget(id) => id,
    };
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let focus = request
        .focus_target
        .or_else(|| tree.widget_initial_focus_hint(content))
        .or_else(|| tree.first_focusable_descendant(content));
    if let Some(id) = focus {
        tree.focus(id);
    }
    (tree, answers)
}

#[test]
fn an_alert_is_acknowledged_once() {
    let (mut tree, answers) = present(WebViewControl::Alert {
        message: "Saved".into(),
    });
    tree.press_key(Key::Enter, Modifiers::NONE);
    tree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(*answers.borrow(), vec![WebViewControlAnswer::Confirmed]);
}

#[test]
fn a_confirm_left_with_escape_is_dismissed() {
    let (mut tree, answers) = present(WebViewControl::Confirm {
        message: "Leave?".into(),
    });
    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(*answers.borrow(), vec![WebViewControlAnswer::Dismissed]);
}

#[test]
fn a_list_answers_with_the_option_chosen() {
    let option = |id, label: &str| WebViewSelectOption {
        id,
        label: label.into(),
        disabled: false,
        group: None,
    };
    let (mut tree, answers) = present(WebViewControl::Select {
        options: vec![option(7, "Seven"), option(9, "Nine")],
        selected: Some(7),
        area: Rect::new(0.0, 0.0, 10.0, 10.0),
    });
    // The list opens with nothing highlighted; the first arrow highlights
    // the first option, Enter takes it.
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    tree.press_key(Key::Enter, Modifiers::NONE);
    let answers = answers.borrow();
    assert_eq!(answers.len(), 1, "answered once: {answers:?}");
    assert!(
        answers[0] == WebViewControlAnswer::Selected(7),
        "the first option, by its id: {answers:?}"
    );
}
