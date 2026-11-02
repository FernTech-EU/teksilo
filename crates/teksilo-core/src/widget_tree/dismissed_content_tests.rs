// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Content the framework built for one overlay alone
//! (`OverlayRequest::destroy_content_on_dismiss`) is destroyed after the
//! overlay is dismissed, by every removal path, at the next layout pass.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::SizeProposal;

use super::WidgetTree;
use crate::event::{Key, Modifiers};
use crate::overlay::{DismissBehavior, OverlayId, OverlayPlacement, OverlayRequest};
use crate::test_widgets::StackWidget;
use crate::widget::{LayoutContext, LayoutResponse, Widget};
use crate::widget_id::WidgetId;

/// A leaf that records its own `Drop`.
#[derive(Debug)]
struct DropProbe(Rc<Cell<bool>>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

impl Widget for DropProbe {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(10.0, 10.0).into()
    }
}

fn lay_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

/// A tree with an anchor, and flagged overlay content holding a probe.
fn setup() -> (WidgetTree, WidgetId, WidgetId, Rc<Cell<bool>>) {
    let mut tree = WidgetTree::new();
    let anchor = tree.add(StackWidget::new());
    lay_out(&mut tree);
    let dropped = Rc::new(Cell::new(false));
    let content = tree.add(StackWidget::new());
    tree.add_child(content, DropProbe(dropped.clone()));
    (tree, anchor, content, dropped)
}

fn request(content: WidgetId, anchor: WidgetId, destroy: bool) -> OverlayRequest {
    let mut request = OverlayRequest::new(
        content,
        anchor,
        OverlayPlacement::Centered,
        DismissBehavior::EscapeKey,
    );
    request.destroy_content_on_dismiss = destroy;
    request
}

fn show(tree: &mut WidgetTree, content: WidgetId, anchor: WidgetId, destroy: bool) -> OverlayId {
    tree.activate(content);
    let id = tree.show_overlay(request(content, anchor, destroy));
    lay_out(tree);
    id
}

#[test]
fn flagged_content_is_destroyed_at_the_layout_after_escape() {
    let (mut tree, anchor, content, dropped) = setup();
    show(&mut tree, content, anchor, true);
    tree.press_key(Key::Escape, Modifiers::NONE);
    assert!(!dropped.get(), "not inside the dispatch that dismissed it");
    lay_out(&mut tree);
    assert!(dropped.get());
    assert!(tree.arena.get(content).is_none());
}

#[test]
fn unflagged_content_is_kept_and_can_be_shown_again() {
    let (mut tree, anchor, content, dropped) = setup();
    show(&mut tree, content, anchor, false);
    tree.press_key(Key::Escape, Modifiers::NONE);
    lay_out(&mut tree);
    assert!(!dropped.get());
    show(&mut tree, content, anchor, false);
    assert!(tree.is_active(content), "presented again");
}

/// `on_dismiss` can still read the content, and runs before the `Drop`.
#[test]
fn on_dismiss_runs_before_the_content_goes() {
    let (mut tree, anchor, content, dropped) = setup();
    let seen = Rc::new(Cell::new(None));
    let mut req = request(content, anchor, true);
    let (seen_in, dropped_in) = (seen.clone(), dropped.clone());
    req.on_dismiss = Some(Rc::new(
        move |_reason, ctx: &mut crate::widget::EventContext| {
            let _ = ctx;
            seen_in.set(Some(dropped_in.get()));
        },
    ));
    tree.show_overlay(req);
    lay_out(&mut tree);
    tree.press_key(Key::Escape, Modifiers::NONE);
    lay_out(&mut tree);
    assert_eq!(
        seen.get(),
        Some(false),
        "on_dismiss ran with the content alive"
    );
    assert!(dropped.get());
}

#[test]
fn a_faded_overlay_is_destroyed_once_its_fade_ends() {
    let (mut tree, anchor, content, dropped) = setup();
    tree.show_overlay(request(content, anchor, true).with_fade(Duration::from_millis(100)));
    lay_out(&mut tree);
    tree.press_key(Key::Escape, Modifiers::NONE);
    lay_out(&mut tree);
    assert!(!dropped.get(), "still fading out");
    tree.advance_time(Duration::from_millis(200));
    lay_out(&mut tree);
    assert!(dropped.get());
}

/// A fade in progress, then a second, programmatic dismissal: destroyed
/// once, no panic.
#[test]
fn dismissed_twice_it_is_destroyed_once() {
    let (mut tree, anchor, content, dropped) = setup();
    let id =
        tree.show_overlay(request(content, anchor, true).with_fade(Duration::from_millis(100)));
    lay_out(&mut tree);
    let mut noop = crate::window::NoopWindowOps;
    tree.dismiss_overlay_with_ops(id, &mut noop);
    tree.overlay_manager
        .dismiss_immediate(id, crate::overlay::DismissReason::Programmatic);
    // The tree parks what the manager removes; done by hand here.
    tree.set_dormant(content);
    tree.dismiss_overlay_with_ops(id, &mut noop);
    tree.advance_time(Duration::from_millis(200));
    lay_out(&mut tree);
    lay_out(&mut tree);
    assert!(dropped.get());
}

/// Shown again by a new overlay before the drain: kept.
#[test]
fn content_shown_again_before_the_drain_is_kept() {
    let (mut tree, anchor, content, dropped) = setup();
    let id = show(&mut tree, content, anchor, true);
    let mut noop = crate::window::NoopWindowOps;
    tree.dismiss_overlay_with_ops(id, &mut noop);
    // Not activated yet: only the overlay on the stack says it is in use.
    tree.show_overlay(request(content, anchor, false));
    lay_out(&mut tree);
    assert!(!dropped.get());
    assert!(tree.arena.get(content).is_some());
}

/// Each way the manager takes entries off the stack queues flagged content.
#[test]
fn every_removal_path_queues_flagged_content() {
    type Remove = fn(&mut WidgetTree, OverlayId, WidgetId);
    let paths: [(&str, Remove); 4] = [
        ("dismiss", |tree, id, _| {
            tree.overlay_manager.dismiss(id);
        }),
        ("dismiss_all", |tree, _, _| {
            tree.overlay_manager.dismiss_all();
        }),
        ("dismiss_except", |tree, _, _| {
            tree.overlay_manager
                .dismiss_except(&std::collections::HashSet::new());
        }),
        ("dismiss_descendants_of", |tree, _, parent_content| {
            let parent = tree
                .overlay_manager
                .find_by_content(parent_content)
                .expect("the parent overlay");
            tree.overlay_manager.dismiss_descendants_of(parent, None);
        }),
    ];
    for (name, remove) in paths {
        let (mut tree, anchor, content, dropped) = setup();
        let parent_content = tree.add(StackWidget::new());
        let parent = show(&mut tree, parent_content, anchor, false);
        let mut req = request(content, anchor, true);
        req.parent_overlay = Some(parent);
        let id = tree.show_overlay(req);
        lay_out(&mut tree);
        remove(&mut tree, id, parent_content);
        // The tree parks what the manager removes; done by hand here.
        tree.set_dormant(content);
        lay_out(&mut tree);
        assert!(dropped.get(), "{name} queues the content");
    }
}

// ── Context menus: the factory builds one per opening ──────────────────

fn right_click(tree: &mut WidgetTree, id: WidgetId) {
    use crate::event::{PointerButton, WidgetEvent};
    let at = tree.bounds(id).center();
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
}

/// A field whose context menu is a fresh probe each time, the count of
/// probes dropped so far beside it.
fn field_with_menu(tree: &mut WidgetTree) -> (WidgetId, Rc<Cell<usize>>) {
    use crate::widget_builder::WidgetBuilder;
    let drops = Rc::new(Cell::new(0));
    let counter = drops.clone();
    let field = tree.add(
        crate::test_widgets::FillWidget::new()
            .focusable()
            .context_menu(move |_at, _ctx| {
                let flag = Rc::new(Cell::new(false));
                let counter = counter.clone();
                // Count the drop through a second probe's flag.
                #[derive(Debug)]
                struct Counting(Rc<Cell<usize>>, DropProbe);
                impl Drop for Counting {
                    fn drop(&mut self) {
                        self.0.set(self.0.get() + 1);
                    }
                }
                impl Widget for Counting {
                    fn layout_response(
                        &self,
                        proposal: SizeProposal,
                        ctx: &LayoutContext,
                    ) -> LayoutResponse {
                        self.1.layout_response(proposal, ctx)
                    }
                }
                Some(Box::new(Counting(counter, DropProbe(flag))))
            }),
    );
    lay_out(tree);
    (field, drops)
}

#[test]
fn a_context_menu_is_destroyed_after_it_closes() {
    let mut tree = WidgetTree::new();
    let (field, drops) = field_with_menu(&mut tree);
    let before = tree.widget_count();
    for close in ["escape", "outside click"] {
        right_click(&mut tree, field);
        lay_out(&mut tree);
        assert_eq!(tree.widget_count(), before + 1, "{close}: the menu is up");
        match close {
            "escape" => tree.press_key(Key::Escape, Modifiers::NONE),
            _ => {
                use crate::event::{PointerButton, WidgetEvent};
                let far = teksilo_canvas::Point::new(399.0, 299.0);
                tree.dispatch_event(WidgetEvent::pointer_down(
                    far,
                    PointerButton::Primary,
                    Modifiers::NONE,
                ));
                tree.dispatch_event(WidgetEvent::pointer_up(
                    far,
                    PointerButton::Primary,
                    Modifiers::NONE,
                ));
            }
        }
        lay_out(&mut tree);
        assert_eq!(tree.widget_count(), before, "{close}: the menu is gone");
    }
    assert_eq!(drops.get(), 2);
}

#[test]
fn fifty_context_menus_do_not_accumulate() {
    let mut tree = WidgetTree::new();
    let (field, drops) = field_with_menu(&mut tree);
    let before = tree.widget_count();
    for _ in 0..50 {
        right_click(&mut tree, field);
        lay_out(&mut tree);
        tree.press_key(Key::Escape, Modifiers::NONE);
        lay_out(&mut tree);
    }
    assert_eq!(tree.widget_count(), before);
    assert_eq!(drops.get(), 50);
}

/// A `Drop` that sets a signal (an app guard released) runs at the drain,
/// after this pass's bindings were flushed: the drain asks for another
/// frame, so the change is drawn without waiting for input.
#[test]
fn a_drop_after_the_drain_asks_for_a_frame() {
    #[derive(Debug)]
    struct Releases(crate::signal::Signal<u32>);
    impl Drop for Releases {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    impl Widget for Releases {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
    }
    let (mut tree, anchor, _content, _dropped) = setup();
    let released = crate::signal::Signal::new(0u32);
    let content = tree.add(StackWidget::new());
    tree.add_child(content, Releases(released.clone()));
    show(&mut tree, content, anchor, true);
    tree.press_key(Key::Escape, Modifiers::NONE);
    // Let the dispatch's own frame requests be consumed first.
    tree.tick_animations(Duration::from_millis(16));
    lay_out(&mut tree);
    assert_eq!(released.get(), 1, "the guard was released at the drain");
    assert!(
        tree.frame_requested(),
        "and a frame is asked for to draw it"
    );
}

/// A context menu whose row closes it and sends a command, sometimes with a
/// submenu opened from it first: the command reaches the window's action,
/// and the menu (submenu included) is destroyed every time.
#[test]
fn a_context_menu_closed_by_its_row_is_destroyed_with_its_submenu() {
    use crate::event::{EventResponse, WidgetEvent};
    use crate::widget_builder::{HandlerSet, WidgetBuilder};

    #[derive(Debug)]
    struct Menu {
        drops: Rc<Cell<usize>>,
        sub: Rc<Cell<Option<WidgetId>>>,
    }
    impl Drop for Menu {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    impl Widget for Menu {
        fn build(&mut self, ctx: &mut crate::BuildContext) -> Vec<WidgetId> {
            // Pre-built overlay content, owned through `add_detached` as a
            // menu's submenu is.
            let sub = ctx.add_detached(StackWidget::new());
            self.sub.set(Some(sub));
            let me = ctx.self_id();
            ctx.apply_self_handlers(HandlerSet::new().focusable(true).on_key(move |event, ctx| {
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::ArrowRight,
                        ..
                    } => {
                        ctx.show_overlay(OverlayRequest::new(
                            sub,
                            me,
                            OverlayPlacement::Below,
                            DismissBehavior::EscapeKey,
                        ));
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::Enter, ..
                    } => {
                        ctx.dismiss_all_overlays();
                        ctx.send_intent(crate::Intent::new("test.cut"));
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }));
            vec![]
        }
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
    }

    #[derive(Debug)]
    struct Root(Rc<Cell<usize>>);
    impl Widget for Root {
        fn build(&mut self, ctx: &mut crate::BuildContext) -> Vec<WidgetId> {
            let cuts = self.0.clone();
            ctx.register_action_global(
                crate::Action::new("test.cut").on_invoke(move |_, _| cuts.set(cuts.get() + 1)),
            );
            vec![]
        }
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(400.0, 300.0).into()
        }
    }

    let cuts = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let mut tree = WidgetTree::new();
    tree.add(Root(cuts.clone()));
    let factory_drops = drops.clone();
    let field = tree.add(
        crate::test_widgets::FillWidget::new()
            .focusable()
            .context_menu(move |_at, _ctx| {
                Some(Box::new(Menu {
                    drops: factory_drops.clone(),
                    sub: Rc::new(Cell::new(None)),
                }))
            }),
    );
    lay_out(&mut tree);
    let before = tree.widget_count();
    for cycle in 0..6 {
        right_click(&mut tree, field);
        lay_out(&mut tree);
        if cycle % 2 == 1 {
            tree.press_key(Key::ArrowRight, Modifiers::NONE);
            lay_out(&mut tree);
        }
        tree.press_key(Key::Enter, Modifiers::NONE);
        lay_out(&mut tree);
        assert_eq!(tree.widget_count(), before, "cycle {cycle}: nothing left");
    }
    assert_eq!(cuts.get(), 6, "every row's command ran");
    assert_eq!(drops.get(), 6, "every menu was dropped");
}
