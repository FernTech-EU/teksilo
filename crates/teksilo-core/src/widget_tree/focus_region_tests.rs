// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! F6 and Shift+F6 move focus between the focus regions of a window
//! (`focus_region_impl`), through the real key path: the shortcut registry,
//! the intent funnel and the tree's own cycling.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;

use crate::event::{EventResponse, Key, Modifiers, WidgetEvent};
use crate::overlay::{DismissBehavior, OverlayPlacement, OverlayRequest};
use crate::shortcut::{KeyStroke, NEXT_REGION, PREVIOUS_REGION, Shortcut};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_builder::WidgetBuilder;
use crate::window::NoopWindowOps;
use crate::{Action, BuildContext, Intent, LayoutContext, LayoutResponse, Widget, WidgetId};

use super::WidgetTree;

fn size() -> SizeProposal {
    SizeProposal::exact(400.0, 300.0)
}

fn f6(tree: &mut WidgetTree) {
    tree.press_key(Key::F6, Modifiers::NONE);
}

fn shift_f6(tree: &mut WidgetTree) {
    tree.press_key(Key::F6, Modifiers::SHIFT);
}

/// Two focusable fields in a region named `name`, added to `tree` as a root.
fn region(tree: &mut WidgetTree, name: &str) -> (WidgetId, [WidgetId; 2]) {
    let a = tree.add(FillWidget::new().focusable());
    let b = tree.add(FillWidget::new().focusable());
    let region = tree.add(
        StackWidget::new()
            .child(a)
            .child(b)
            .focus_region(name.to_owned()),
    );
    (region, [a, b])
}

/// A window with regions A, B and C, two fields each.
struct Window {
    tree: WidgetTree,
    regions: Vec<(WidgetId, [WidgetId; 2])>,
}

impl Window {
    fn new() -> Self {
        let mut tree = WidgetTree::new();
        let regions = ["A", "B", "C"]
            .into_iter()
            .map(|name| region(&mut tree, name))
            .collect();
        tree.layout(size());
        Self { tree, regions }
    }

    fn field(&self, region: usize, field: usize) -> WidgetId {
        self.regions[region].1[field]
    }
}

/// What has been announced so far, after the syncs the announcer needs to let
/// a message out once focus has landed.
fn announced(tree: &mut WidgetTree) -> Vec<String> {
    for _ in 0..3 {
        tree.request_accessibility_update();
        let _ = tree.sync_accessibility();
    }
    tree.announcements_since(0)
        .into_iter()
        .map(|announcement| announcement.text)
        .collect()
}

#[test]
fn f6_cycles_regions_and_announces() {
    let mut w = Window::new();
    w.tree.focus(w.field(0, 0));
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(1, 0)));
    assert!(announced(&mut w.tree).contains(&"B".to_owned()));
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(2, 0)));
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(0, 0)), "wraps");
    shift_f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(2, 0)), "and back");
}

#[test]
fn region_remembers_its_last_focus() {
    let mut w = Window::new();
    w.tree.focus(w.field(1, 1));
    f6(&mut w.tree);
    shift_f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(1, 1)));
}

#[test]
fn remembered_focus_that_left_is_not_used() {
    let mut w = Window::new();
    let gone = w.field(1, 1);
    w.tree.focus(gone);
    f6(&mut w.tree);
    w.tree.set_dormant(gone);
    w.tree.layout(size());
    shift_f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(1, 0)));
}

#[test]
fn dormant_and_focusless_regions_are_skipped() {
    let mut tree = WidgetTree::new();
    let (_, [a, _]) = region(&mut tree, "A");
    let (hidden, _) = region(&mut tree, "Hidden");
    let label = tree.add(FillWidget::new());
    tree.add(
        StackWidget::new()
            .child(label)
            .focus_region("Empty".to_owned()),
    );
    let (_, [c, _]) = region(&mut tree, "C");
    tree.set_dormant(hidden);
    tree.layout(size());
    tree.focus(a);
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(c));
}

/// A contains A1 (first in A's subtree), then a field of its own; B follows.
/// Each step lands in a different region, and cycling ends.
#[test]
fn nested_region_is_its_own_stop_and_cycling_terminates() {
    let mut tree = WidgetTree::new();
    let inner_field = tree.add(FillWidget::new().focusable());
    let inner = tree.add(
        StackWidget::new()
            .child(inner_field)
            .focus_region("A1".to_owned()),
    );
    let outer_field = tree.add(FillWidget::new().focusable());
    tree.add(
        StackWidget::new()
            .child(inner)
            .child(outer_field)
            .focus_region("A".to_owned()),
    );
    let (_, [b, _]) = region(&mut tree, "B");
    tree.layout(size());
    tree.focus(b);
    shift_f6(&mut tree);
    assert_eq!(
        tree.focused(),
        Some(inner_field),
        "A1, after A in tree order"
    );
    shift_f6(&mut tree);
    assert_eq!(tree.focused(), Some(outer_field), "A's own field, not A1's");
    shift_f6(&mut tree);
    assert_eq!(tree.focused(), Some(b));
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(outer_field));
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(inner_field));
}

#[test]
fn with_focus_in_no_region_next_goes_to_the_first_and_previous_to_the_last() {
    let mut w = Window::new();
    let outside = w.tree.add(FillWidget::new().focusable());
    w.tree.layout(size());
    w.tree.focus(outside);
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(0, 0)));
    w.tree.focus(outside);
    shift_f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(2, 0)));
    w.tree.clear_focus();
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(0, 0)), "nothing focused");
}

#[test]
fn single_region_keeps_focus_and_reannounces() {
    let mut tree = WidgetTree::new();
    let (_, [_, b]) = region(&mut tree, "Only");
    tree.layout(size());
    tree.focus(b);
    let before = announced(&mut tree).len();
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(b));
    let heard = announced(&mut tree);
    assert_eq!(heard.len(), before + 1);
    assert_eq!(heard.last().map(String::as_str), Some("Only"));
}

/// A field recording the F6 keys it receives.
fn key_recorder(keys: Rc<RefCell<Vec<Modifiers>>>) -> impl Widget + 'static {
    FillWidget::new().focusable().on_key(move |event, _| {
        if let WidgetEvent::KeyDown {
            key: Key::F6,
            modifiers,
            ..
        } = event
        {
            keys.borrow_mut().push(*modifiers);
        }
        EventResponse::Handled
    })
}

#[test]
fn a_modal_cycles_only_its_own_regions() {
    let mut w = Window::new();
    let mut inside = Vec::new();
    let mut fields = Vec::new();
    for name in ["M1", "M2"] {
        let field = w.tree.add(FillWidget::new().focusable());
        inside.push(
            w.tree.add(
                StackWidget::new()
                    .child(field)
                    .focus_region(name.to_owned()),
            ),
        );
        fields.push(field);
    }
    let content = w
        .tree
        .add(StackWidget::new().child(inside[0]).child(inside[1]));
    let anchor = w.field(0, 0);
    w.tree.show_overlay(OverlayRequest::new(
        content,
        anchor,
        OverlayPlacement::Centered,
        DismissBehavior::Manual,
    ));
    w.tree.layout(size());
    w.tree.focus(fields[0]);
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(fields[1]));
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(fields[0]), "never the page's");
}

#[test]
fn a_modal_without_regions_lets_f6_through() {
    let mut w = Window::new();
    let keys = Rc::new(RefCell::new(Vec::new()));
    let field = w.tree.add(key_recorder(keys.clone()));
    let content = w.tree.add(StackWidget::new().child(field));
    let anchor = w.field(0, 0);
    w.tree.show_overlay(OverlayRequest::new(
        content,
        anchor,
        OverlayPlacement::Centered,
        DismissBehavior::Manual,
    ));
    w.tree.layout(size());
    w.tree.focus(field);
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(field));
    assert_eq!(keys.borrow().len(), 1, "the field had F6");
}

#[test]
fn with_no_regions_f6_reaches_on_key() {
    let mut tree = WidgetTree::new();
    let keys = Rc::new(RefCell::new(Vec::new()));
    let field = tree.add(key_recorder(keys.clone()));
    tree.layout(size());
    tree.focus(field);
    f6(&mut tree);
    shift_f6(&mut tree);
    assert_eq!(*keys.borrow(), [Modifiers::NONE, Modifiers::SHIFT]);
    assert_eq!(tree.focused(), Some(field));
}

#[test]
fn f6_no_longer_reaches_on_key_once_regions_exist() {
    let mut w = Window::new();
    let keys = Rc::new(RefCell::new(Vec::new()));
    let field = w.tree.add(key_recorder(keys.clone()));
    let first = w.regions[0].0;
    w.tree.add_child(first, StackWidget::new().child(field));
    w.tree.layout(size());
    w.tree.focus(field);
    f6(&mut w.tree);
    assert!(keys.borrow().is_empty());
    assert_eq!(w.tree.focused(), Some(w.field(1, 0)));
}

/// An application's own global shortcut on F6, recording "app".
#[derive(Debug)]
struct AppF6 {
    calls: Rc<RefCell<Vec<&'static str>>>,
    category: &'static str,
}

impl Widget for AppF6 {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let calls = self.calls.clone();
        ctx.register_action_global(Action::new("app.refresh").on_invoke(move |_, _| {
            calls.borrow_mut().push("app");
        }));
        ctx.register_shortcut_global(
            Shortcut::new("app.refresh")
                .category(self.category)
                .primary(KeyStroke::new(Key::F6, Modifiers::NONE))
                .build(),
        );
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// An application's shortcut on F6 keeps working whatever its category: with
/// no region, and with regions too, where the app's binding wins.
#[test]
fn an_app_shortcut_on_f6_wins_whatever_its_category() {
    for category in ["view", "View", "a"] {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();
        tree.add(AppF6 {
            calls: calls.clone(),
            category,
        });
        let field = tree.add(FillWidget::new().focusable());
        tree.layout(size());
        tree.focus(field);
        f6(&mut tree);
        assert_eq!(*calls.borrow(), ["app"], "no region, category {category}");

        let (_, [a, _]) = region(&mut tree, "A");
        region(&mut tree, "B");
        tree.layout(size());
        tree.focus(a);
        f6(&mut tree);
        assert_eq!(
            *calls.borrow(),
            ["app", "app"],
            "regions, category {category}"
        );
        assert_eq!(tree.focused(), Some(a));
    }
}

/// The page's own handler for the command runs instead of the cycling.
#[derive(Debug)]
struct OwnNextRegion {
    calls: Rc<RefCell<Vec<&'static str>>>,
}

impl Widget for OwnNextRegion {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let calls = self.calls.clone();
        ctx.register_action_global(Action::new(NEXT_REGION).on_invoke(move |_, _| {
            calls.borrow_mut().push("own");
        }));
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[test]
fn a_window_action_overrides_the_built_in_cycling() {
    let mut w = Window::new();
    let calls = Rc::new(RefCell::new(Vec::new()));
    w.tree.add(OwnNextRegion {
        calls: calls.clone(),
    });
    w.tree.layout(size());
    w.tree.focus(w.field(0, 0));
    f6(&mut w.tree);
    assert_eq!(*calls.borrow(), ["own"]);
    assert_eq!(w.tree.focused(), Some(w.field(0, 0)));
}

/// In a window with no region, an action taking the command still gets F6:
/// the key steps aside only when nothing would take it.
#[test]
fn with_no_regions_an_action_for_the_command_still_gets_f6() {
    let mut tree = WidgetTree::new();
    let field = tree.add(FillWidget::new().focusable());
    let calls = Rc::new(RefCell::new(Vec::new()));
    tree.add(OwnNextRegion {
        calls: calls.clone(),
    });
    tree.layout(size());
    tree.focus(field);
    f6(&mut tree);
    assert_eq!(*calls.borrow(), ["own"]);
}

#[test]
fn f6_stays_in_a_keyboard_capture_surface() {
    let mut w = Window::new();
    let keys = Rc::new(RefCell::new(Vec::new()));
    let recorder = keys.clone();
    let terminal =
        w.tree.add(
            FillWidget::new()
                .focusable()
                .keyboard_capture(true)
                .on_key(move |event, _| {
                    if let WidgetEvent::KeyDown { key: Key::F6, .. } = event {
                        recorder.borrow_mut().push(());
                    }
                    EventResponse::Handled
                }),
        );
    let first = w.regions[0].0;
    w.tree.add_child(first, StackWidget::new().child(terminal));
    w.tree.layout(size());
    w.tree.focus(terminal);
    f6(&mut w.tree);
    assert_eq!(keys.borrow().len(), 1);
    assert_eq!(w.tree.focused(), Some(terminal));
    w.tree.press_key(Key::Tab, Modifiers::CTRL);
    assert_eq!(w.tree.focused(), Some(w.field(1, 0)), "Ctrl+Tab leaves it");
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(2, 0)), "then F6 cycles");
}

#[test]
fn rebinding_next_region_moves_the_chord() {
    let mut w = Window::new();
    w.tree
        .shortcut_registry_mut()
        .rebind_primary(NEXT_REGION, Some(KeyStroke::new(Key::F7, Modifiers::NONE)));
    w.tree.focus(w.field(0, 0));
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(0, 0)), "F6 no longer cycles");
    w.tree.press_key(Key::F7, Modifiers::NONE);
    assert_eq!(w.tree.focused(), Some(w.field(1, 0)));
}

#[test]
fn the_command_sent_as_an_intent_cycles_and_is_available_only_with_regions() {
    let mut tree = WidgetTree::new();
    let field = tree.add(FillWidget::new().focusable());
    tree.layout(size());
    tree.focus(field);
    assert!(!tree.can_handle(NEXT_REGION));
    assert!(!tree.handled_intents_from(field).contains(&NEXT_REGION));

    let mut w = Window::new();
    w.tree.focus(w.field(0, 0));
    assert!(w.tree.can_handle(NEXT_REGION));
    assert!(w.tree.can_handle(PREVIOUS_REGION));
    assert!(
        w.tree
            .handled_intents_from(w.field(0, 0))
            .contains(&NEXT_REGION)
    );
    w.tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.send_intent_from_focus(Intent::new(NEXT_REGION));
    });
    assert_eq!(w.tree.focused(), Some(w.field(1, 0)));
}

/// From a popover attached inside region B, F6 goes to the region after B.
#[test]
fn from_a_popover_the_region_is_the_one_its_anchor_is_in() {
    let mut w = Window::new();
    let field = w.tree.add(FillWidget::new().focusable());
    let content = w.tree.add(StackWidget::new().child(field));
    let anchor = w.field(1, 0);
    w.tree.show_overlay(OverlayRequest::new(
        content,
        anchor,
        OverlayPlacement::Below,
        DismissBehavior::Manual,
    ));
    w.tree.layout(size());
    w.tree.focus(field);
    f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(2, 0)));
    assert!(
        w.tree.overlay_manager.find_by_content(content).is_none(),
        "the popover followed focus out"
    );
}

/// Regions are visited in the order Tab reaches them, not in the arena's
/// child order: three regions whose traversal scopes' `tab_index` reorders
/// them (as a docking layout orders its sides).
#[test]
fn regions_follow_the_tab_order_not_the_tree_order() {
    let mut tree = WidgetTree::new();
    let page = tree.add(StackWidget::new());
    let mut fields = Vec::new();
    // Tree order X, Y, Z; Tab order Y, X, Z.
    for (name, index) in [("X", 2), ("Y", 1), ("Z", 3)] {
        let region = tree.add_child(
            page,
            StackWidget::new()
                .focus_region(name.to_owned())
                .tab_index(index),
        );
        tree.set_traversal_scope(region, crate::focus::TraversalScopePolicy::Continue);
        fields.push(tree.add_child(region, FillWidget::new().focusable()));
    }
    let [x, y, z] = [fields[0], fields[1], fields[2]];
    tree.layout(size());
    tree.press_key(Key::Tab, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(y), "premise: Tab goes to Y first");
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(x), "then X, as Tab");
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(z));
    shift_f6(&mut tree);
    assert_eq!(tree.focused(), Some(x));
}

/// A remembered focus that has become disabled is not landed on.
#[test]
fn a_remembered_focus_now_disabled_is_not_used() {
    let mut w = Window::new();
    let remembered = w.field(1, 1);
    let enabled = crate::signal::Signal::new(true);
    w.tree.enabled_when(remembered, enabled.clone());
    w.tree.focus(remembered);
    f6(&mut w.tree);
    enabled.set(false);
    w.tree.layout(size());
    shift_f6(&mut w.tree);
    assert_eq!(w.tree.focused(), Some(w.field(1, 0)));
}

/// The region's name is read when it is announced, so a name bound to a
/// signal (a translated one, in an app) is announced as it is now.
#[test]
fn the_announced_name_is_the_current_one() {
    let mut tree = WidgetTree::new();
    let name = crate::signal::Signal::new("Explorer".to_owned());
    let a = tree.add(FillWidget::new().focusable());
    tree.add(StackWidget::new().child(a).focus_region(name.clone()));
    let (_, [b, _]) = region(&mut tree, "Editor");
    tree.layout(size());
    tree.focus(b);
    name.set("Explorateur".to_owned());
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(a));
    assert!(announced(&mut tree).contains(&"Explorateur".to_owned()));
}

/// A widget that marks a child built in `build` a region, through `ctx.add`.
#[derive(Debug)]
struct BuildsARegion {
    field: Rc<RefCell<Option<WidgetId>>>,
}

impl Widget for BuildsARegion {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let field = ctx.add(FillWidget::new().focusable());
        *self.field.borrow_mut() = Some(field);
        let region = ctx.add(
            StackWidget::new()
                .child(field)
                .focus_region("Built".to_owned()),
        );
        vec![region]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 100.0).into()
    }
}

/// A widget that makes itself a region, through `apply_self_handlers`.
#[derive(Debug)]
struct SelfRegion {
    field: Rc<RefCell<Option<WidgetId>>>,
}

impl Widget for SelfRegion {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(
            crate::widget_builder::HandlerSet::new().focus_region("Self".to_owned()),
        );
        let field = ctx.add(FillWidget::new().focusable());
        *self.field.borrow_mut() = Some(field);
        vec![field]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 100.0).into()
    }
}

/// The region flag reaches the node by every way a handler set does:
/// `ctx.add`, `apply_self_handlers`, `tree.add`, `tree.add_child`, and a
/// double wrap merged by `merge_under`.
#[test]
fn the_region_flag_survives_every_insertion_path() {
    let mut tree = WidgetTree::new();
    let built = Rc::new(RefCell::new(None));
    tree.add(BuildsARegion {
        field: built.clone(),
    });
    let own = Rc::new(RefCell::new(None));
    tree.add(SelfRegion { field: own.clone() });
    let (_, [added, _]) = region(&mut tree, "Added");
    let parent = tree.add(StackWidget::new());
    let child_field = tree.add(FillWidget::new().focusable());
    tree.add_child(
        parent,
        StackWidget::new()
            .child(child_field)
            .focus_region("Child".to_owned()),
    );
    let wrapped_field = tree.add(FillWidget::new().focusable());
    // The trait's method on the wrapper wraps it again, and the two handler
    // sets meet in `merge_under`.
    let once = StackWidget::new()
        .child(wrapped_field)
        .focus_region("Wrapped".to_owned());
    tree.add(WidgetBuilder::focusable(once, false));
    tree.layout(size());
    let built = built.borrow().expect("built");
    let own = own.borrow().expect("built");
    let order = [built, own, added, child_field, wrapped_field];
    tree.focus(order[0]);
    for (step, expected) in order.iter().cycle().skip(1).take(5).enumerate() {
        f6(&mut tree);
        assert_eq!(tree.focused(), Some(*expected), "step {step}");
    }
}

/// A region whose first Tab stop is a divider (a side panel's resize handle
/// drawn on its inner edge), then two controls; and a second region.
fn divider_first() -> (WidgetTree, [WidgetId; 3], WidgetId) {
    let mut tree = WidgetTree::new();
    let divider = tree.add(
        FillWidget::new()
            .focusable()
            .access_role(accesskit::Role::Splitter),
    );
    let a = tree.add(FillWidget::new().focusable());
    let b = tree.add(FillWidget::new().focusable());
    tree.add(
        StackWidget::new()
            .child(divider)
            .child(a)
            .child(b)
            .focus_region("Side".to_owned()),
    );
    let (_, [other, _]) = region(&mut tree, "Other");
    tree.layout(size());
    (tree, [divider, a, b], other)
}

#[test]
fn f6_lands_on_a_regions_content_before_its_divider() {
    let (mut tree, [divider, a, b], other) = divider_first();
    assert_eq!(
        tree.tab_stops_within(tree.arena.parent(divider).unwrap())
            .first(),
        Some(&divider),
        "Tab still reaches the divider first"
    );
    tree.focus(other);
    shift_f6(&mut tree);
    assert_eq!(
        tree.focused(),
        Some(a),
        "the first control, not the divider"
    );

    // A divider reached by Tab is not where F6 comes back to.
    tree.focus(divider);
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(other));
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(a));

    // The last control focused there is.
    tree.focus(b);
    f6(&mut tree);
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(b));
}

#[test]
fn a_region_with_only_a_divider_lands_on_it() {
    let mut tree = WidgetTree::new();
    let divider = tree.add(
        FillWidget::new()
            .focusable()
            .access_role(accesskit::Role::Splitter),
    );
    tree.add(
        StackWidget::new()
            .child(divider)
            .focus_region("Handle".to_owned()),
    );
    let (_, [other, _]) = region(&mut tree, "Other");
    tree.layout(size());
    tree.focus(other);
    f6(&mut tree);
    assert_eq!(tree.focused(), Some(divider));
}

/// A region that names one of its children as its F6 landing
/// (`focus_region_landing`), as a dock side names its panel.
#[derive(Debug)]
struct HintedRegion {
    children: Vec<WidgetId>,
    hint: WidgetId,
}

impl Widget for HintedRegion {
    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 100.0).into()
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _: SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        _: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn focus_region_landing(&self) -> Option<WidgetId> {
        Some(self.hint)
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }
}

/// A side: a rail control first in Tab order, then a panel of two fields
/// (the hint), then a divider; and a second region.
struct Side {
    tree: WidgetTree,
    region: WidgetId,
    rail: WidgetId,
    panel: WidgetId,
    fields: [WidgetId; 2],
    divider: WidgetId,
    other: WidgetId,
}

fn hinted_side() -> Side {
    let mut tree = WidgetTree::new();
    let rail = tree.add(FillWidget::new().focusable());
    let a = tree.add(FillWidget::new().focusable());
    let b = tree.add(FillWidget::new().focusable());
    let panel = tree.add(StackWidget::new().child(a).child(b));
    let divider = tree.add(
        FillWidget::new()
            .focusable()
            .access_role(accesskit::Role::Splitter),
    );
    let side = tree.add(
        HintedRegion {
            children: vec![rail, panel, divider],
            hint: panel,
        }
        .focus_region("Side".to_owned()),
    );
    let (_, [other, _]) = region(&mut tree, "Other");
    tree.layout(size());
    Side {
        tree,
        region: side,
        rail,
        panel,
        fields: [a, b],
        divider,
        other,
    }
}

#[test]
fn f6_lands_in_the_hinted_subtree_before_an_earlier_tab_stop() {
    let mut s = hinted_side();
    assert_eq!(
        s.tree.tab_stops_within(s.region).first(),
        Some(&s.rail),
        "Tab still meets the rail first"
    );
    s.tree.focus(s.other);
    f6(&mut s.tree);
    assert_eq!(
        s.tree.focused(),
        Some(s.fields[0]),
        "the panel, not the rail"
    );
    assert!(announced(&mut s.tree).contains(&"Side".to_owned()));
}

#[test]
fn a_remembered_focus_wins_over_the_hint() {
    let mut s = hinted_side();
    // The rail, focused by the user, is where F6 comes back to.
    s.tree.focus(s.rail);
    f6(&mut s.tree);
    assert_eq!(s.tree.focused(), Some(s.other));
    f6(&mut s.tree);
    assert_eq!(s.tree.focused(), Some(s.rail));
    // So is the panel's second field.
    s.tree.focus(s.fields[1]);
    f6(&mut s.tree);
    f6(&mut s.tree);
    assert_eq!(s.tree.focused(), Some(s.fields[1]));
}

#[test]
fn a_divider_focused_on_purpose_is_not_where_f6_comes_back() {
    let mut s = hinted_side();
    s.tree.focus(s.divider);
    f6(&mut s.tree);
    assert_eq!(s.tree.focused(), Some(s.other));
    f6(&mut s.tree);
    assert_eq!(
        s.tree.focused(),
        Some(s.fields[0]),
        "back in the panel, as if nothing were remembered"
    );
}

#[test]
fn a_hinted_subtree_with_no_tab_stop_lands_on_the_first_control() {
    let mut s = hinted_side();
    // The panel collapsed: its fields are out of the tree's focus.
    s.tree.set_dormant(s.panel);
    s.tree.layout(size());
    s.tree.focus(s.other);
    f6(&mut s.tree);
    assert_eq!(s.tree.focused(), Some(s.rail));
}

#[test]
fn a_regions_landing_is_never_a_windows_first_focus() {
    let mut s = hinted_side();
    let root = s.tree.add(StackWidget::new().child(s.region));
    s.tree.layout(size());
    assert_eq!(s.tree.widget_initial_focus_hint(root), None);
    assert_eq!(s.tree.widget_initial_focus_hint(s.region), None);
}

/// A container that names a child as its `initial_focus_hint`.
#[derive(Debug)]
struct Hinted {
    child: WidgetId,
}

impl Widget for Hinted {
    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 100.0).into()
    }

    fn initial_focus_hint(&self) -> Option<WidgetId> {
        Some(self.child)
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

#[test]
fn a_hinted_widget_marked_as_a_region_keeps_its_windows_first_focus() {
    // Marking a widget as a region changes nothing about the first focus its
    // `initial_focus_hint` names, at the walk's root or below it.
    let mut tree = WidgetTree::new();
    let field = tree.add(FillWidget::new().focusable());
    let panel = tree.add(Hinted { child: field }.focus_region("Panel".to_owned()));
    let root = tree.add(StackWidget::new().child(panel));
    tree.layout(size());
    assert_eq!(tree.widget_initial_focus_hint(root), Some(field));
    assert_eq!(tree.widget_initial_focus_hint(panel), Some(field));
}
