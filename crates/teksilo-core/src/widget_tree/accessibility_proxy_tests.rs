// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A composite that hands its accessibility node to a descendant
//! ([`Widget::accessibility_proxy`]).
//!
//! The shape is a `SpinBox`'s: a focusable field inside a composite whose own
//! node is structure. An application reaches only the composite, so everything
//! it attaches there has to arrive on the field, which is the node focus lands
//! on and the only one a screen reader announces.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use accesskit::Role;
use teksilo_canvas::{Rect, SizeProposal};

use crate::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use crate::build_context::BuildContext;
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use crate::widget_builder::{AccessSubtreeMode, HandlerSet, WidgetBuilder};
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

/// A focusable editing field, named or not, the way a composite builds one.
#[derive(Debug)]
struct Field {
    name: Option<&'static str>,
}

impl Widget for Field {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(HandlerSet::new().focusable(true));
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::TextInput);
        if let Some(name) = self.name {
            builder.set_name(name);
        }
    }
}

/// The composite: chrome around a field, a structural node of its own, and,
/// when `hands_on` is set, the field as its proxy. Anchors a tooltip on the
/// chrome, as composing controls do, when given one.
#[derive(Debug)]
struct Composite {
    field_name: Option<&'static str>,
    field_override: Option<&'static str>,
    hands_on: bool,
    tooltip: Option<&'static str>,
    chrome_subtree: Option<AccessSubtreeMode>,
    field: Rc<Cell<Option<WidgetId>>>,
    chrome: Option<WidgetId>,
}

impl Composite {
    fn new(hands_on: bool) -> Self {
        Self {
            field_name: None,
            field_override: None,
            hands_on,
            tooltip: None,
            chrome_subtree: None,
            field: Rc::new(Cell::new(None)),
            chrome: None,
        }
    }

    fn field_name(mut self, name: &'static str) -> Self {
        self.field_name = Some(name);
        self
    }

    /// A label the composite attaches to its own field as an override, the
    /// way a `SpinBox` names its field.
    fn field_override(mut self, name: &'static str) -> Self {
        self.field_override = Some(name);
        self
    }

    fn tooltip(mut self, text: &'static str) -> Self {
        self.tooltip = Some(text);
        self
    }

    /// Walk the chrome between the composite and its field in `mode`.
    fn chrome_subtree(mut self, mode: AccessSubtreeMode) -> Self {
        self.chrome_subtree = Some(mode);
        self
    }

    /// The field's id, readable once the composite is built.
    fn field_slot(&self) -> Rc<Cell<Option<WidgetId>>> {
        self.field.clone()
    }
}

impl Widget for Composite {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let field = Field {
            name: self.field_name,
        };
        let field = match self.field_override {
            Some(name) => ctx.add(field.access_label_literal(name)),
            None => ctx.add(field),
        };
        let chrome = StackWidget::new().child(field);
        let chrome = match self.chrome_subtree {
            Some(mode) => ctx.add(chrome.access_subtree(mode)),
            None => ctx.add(chrome),
        };
        if let Some(text) = self.tooltip {
            let tip = ctx.add(FillWidget::new().label(text));
            ctx.attach_tooltip(chrome, tip, Duration::from_millis(500));
        }
        self.field.set(Some(field));
        self.chrome = Some(chrome);
        vec![chrome]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.chrome.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::GenericContainer);
    }

    fn accessibility_proxy(&self) -> Option<WidgetId> {
        self.field.get().filter(|_| self.hands_on)
    }
}

fn lay_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(300.0, 80.0));
}

fn node(update: &accesskit::TreeUpdate, id: WidgetId) -> Option<&accesskit::Node> {
    let nid = widget_id_to_node_id(id);
    update
        .nodes
        .iter()
        .find(|(n, _)| *n == nid)
        .map(|(_, node)| node)
}

fn field_of(slot: &Rc<Cell<Option<WidgetId>>>) -> WidgetId {
    slot.get().expect("the composite has built its field")
}

#[test]
fn what_is_attached_to_the_composite_lands_on_the_focused_field() {
    let composite = Composite::new(true)
        .field_name("inner")
        .field_override("the field's own override");
    let slot = composite.field_slot();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        composite
            .access_label_literal("Minutes")
            .access_description_literal("Past the hour"),
    );
    lay_out(&mut tree);
    let field = field_of(&slot);
    tree.focus(field);

    let update = tree.sync_accessibility();
    let focused = node(&update, field).expect("the field is in the tree");
    assert_eq!(
        focused.label(),
        Some("Minutes"),
        "the composite's label, applied after the field's own name and overrides"
    );
    assert_eq!(focused.description(), Some("Past the hour"));
    assert_eq!(update.focus, widget_id_to_node_id(field));
    assert!(
        node(&update, id).is_none(),
        "the composite's own node is structure, and collapses"
    );

    // What an adapter reads: one node says the name, and it is the one with
    // focus. Two would be announced as "Minutes, Minutes" on the way in.
    let consumer = accesskit_consumer::Tree::new(update.clone(), true);
    let state = consumer.state();
    let focus = state.focus().expect("the consumer resolves the focus");
    assert_eq!(focus.label().as_deref(), Some("Minutes"));
    let named = update
        .nodes
        .iter()
        .filter(|(nid, _)| {
            state
                .node_by_tree_local_id(*nid, accesskit::TreeId::ROOT)
                .and_then(|n| n.label())
                .is_some_and(|label| label == "Minutes")
        })
        .count();
    assert_eq!(named, 1, "the name is said once");
}

#[test]
fn without_a_proxy_the_composite_keeps_what_is_attached_to_it() {
    // The control for the test above: the same composite, not handing on.
    let composite = Composite::new(false).field_name("inner");
    let slot = composite.field_slot();
    let mut tree = WidgetTree::new();
    let id = tree.add(composite.access_label_literal("Minutes"));
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    assert_eq!(
        node(&update, id).and_then(|n| n.label()),
        Some("Minutes"),
        "the label stays where it was attached"
    );
    assert_eq!(
        node(&update, field_of(&slot)).and_then(|n| n.label()),
        Some("inner")
    );
}

#[test]
fn relations_attached_after_mount_reach_the_proxy() {
    // A `FormLayout` names its field with `labelled_by` once both ids exist,
    // and an application describes a field with `access_described_by` the
    // same way. Both only know the composite.
    let composite = Composite::new(true);
    let slot = composite.field_slot();
    let mut tree = WidgetTree::new();
    let label = tree.add(FillWidget::new().label("Minutes"));
    let hint = tree.add(FillWidget::new().label("Five at a time"));
    let id = tree.add(composite);
    lay_out(&mut tree);
    tree.push_access_labelled_by(id, label);
    tree.push_access_described_by(id, hint);

    let update = tree.sync_accessibility();
    let field = node(&update, field_of(&slot)).expect("the field is in the tree");
    assert_eq!(field.labelled_by(), &[widget_id_to_node_id(label)]);
    assert_eq!(field.described_by(), &[widget_id_to_node_id(hint)]);

    let consumer = accesskit_consumer::Tree::new(update.clone(), true);
    let name = consumer
        .state()
        .node_by_tree_local_id(
            widget_id_to_node_id(field_of(&slot)),
            accesskit::TreeId::ROOT,
        )
        .and_then(|n| n.label());
    assert_eq!(name.as_deref(), Some("Minutes"));
}

#[test]
fn a_relation_naming_the_composite_names_its_proxy() {
    let composite = Composite::new(true);
    let slot = composite.field_slot();
    let mut tree = WidgetTree::new();
    let id = tree.add(composite);
    let button = tree.add(FillWidget::new().label("Reset").access_controls(id));
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    let button = node(&update, button).expect("the button is in the tree");
    assert_eq!(
        button.controls(),
        &[widget_id_to_node_id(field_of(&slot))],
        "the control points at the node that stands for the composite"
    );
}

#[test]
fn the_tooltip_the_composite_owns_describes_its_proxy() {
    // The tooltip is anchored on the chrome, which the walk reaches before the
    // field. Left to the usual rule it would land there, on a structural node
    // no adapter shows.
    let composite = Composite::new(true).tooltip("Minutes past the hour");
    let slot = composite.field_slot();
    let mut tree = WidgetTree::new();
    tree.add(composite);
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    assert_eq!(
        node(&update, field_of(&slot)).and_then(|n| n.description()),
        Some("Minutes past the hour")
    );
    let described_elsewhere = update
        .nodes
        .iter()
        .filter(|(_, n)| n.description() == Some("Minutes past the hour"))
        .count();
    assert_eq!(described_elsewhere, 1, "and nowhere else");
}

#[test]
fn a_proxy_the_walk_never_reaches_leaves_everything_on_the_composite() {
    // The field is a live descendant, but the chrome between the two is
    // excluded or merged, so the walk stops above the field and emits no node
    // for it. Handing the composite's label down there put it on nothing.
    for mode in [AccessSubtreeMode::Exclude, AccessSubtreeMode::Merge] {
        let composite = Composite::new(true).chrome_subtree(mode);
        let mut tree = WidgetTree::new();
        let id = tree.add(composite.access_label_literal("Minutes"));
        lay_out(&mut tree);

        let update = tree.sync_accessibility();
        assert_eq!(
            node(&update, id).and_then(|n| n.label()),
            Some("Minutes"),
            "{mode:?}: the label stays where it was attached"
        );
        assert_eq!(
            tree.accessibility_node(id).name(),
            Some("Minutes"),
            "{mode:?}: and the tree's own query agrees"
        );
    }
}

#[test]
fn a_proxy_outside_the_composite_is_ignored() {
    /// Names a proxy that is not its descendant.
    #[derive(Debug)]
    struct Stray(WidgetId);
    impl Widget for Stray {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(Role::Group);
        }
        fn accessibility_proxy(&self) -> Option<WidgetId> {
            Some(self.0)
        }
    }

    let mut tree = WidgetTree::new();
    let elsewhere = tree.add(Field {
        name: Some("elsewhere"),
    });
    let stray = tree.add(Stray(elsewhere).access_label_literal("Stray"));
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    assert_eq!(node(&update, stray).and_then(|n| n.label()), Some("Stray"));
    assert_eq!(
        node(&update, elsewhere).and_then(|n| n.label()),
        Some("elsewhere")
    );
}

#[test]
fn nested_composites_hand_down_to_the_innermost_proxy() {
    /// A composite whose proxy is another composite.
    #[derive(Debug)]
    struct Outer {
        inner: Option<WidgetId>,
    }
    impl Widget for Outer {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let composite = Composite::new(true);
            let inner = ctx.add(composite.access_description_literal("inner's"));
            self.inner = Some(inner);
            vec![inner]
        }
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            for child in children.iter_mut() {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }
        fn children(&self) -> Vec<WidgetId> {
            self.inner.into_iter().collect()
        }
        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(Role::GenericContainer);
        }
        fn accessibility_proxy(&self) -> Option<WidgetId> {
            self.inner
        }
    }

    let mut tree = WidgetTree::new();
    let outer = tree.add(
        Outer { inner: None }
            .access_label_literal("outer's")
            .access_description_literal("outer's"),
    );
    lay_out(&mut tree);
    let field = tree
        .first_focusable_descendant(outer)
        .expect("the innermost field is focusable");

    let update = tree.sync_accessibility();
    let node = node(&update, field).expect("the field is in the tree");
    assert_eq!(node.label(), Some("outer's"));
    assert_eq!(
        node.description(),
        Some("outer's"),
        "the outer composite applies last, so its description wins"
    );
    let info = tree.accessibility_node(field);
    assert_eq!(
        info.name(),
        Some("outer's"),
        "and the tree's own query agrees with the walk"
    );
}

#[test]
fn the_trees_own_queries_agree_with_the_walk() {
    let composite = Composite::new(true).field_name("inner");
    let slot = composite.field_slot();
    let mut tree = WidgetTree::new();
    let id = tree.add(composite.access_label_literal("Minutes"));
    lay_out(&mut tree);
    let field = field_of(&slot);

    assert_eq!(tree.accessibility_node(field).name(), Some("Minutes"));
    assert_eq!(tree.accessibility_node(id).name(), None);
    assert_eq!(tree.accessibility_node(id).role(), Role::GenericContainer);
    assert_eq!(tree.find_by_label("Minutes"), Some(field));
}

#[test]
fn a_disabled_override_on_the_composite_governs_the_proxy() {
    // `access_disabled(false)` is the one override that has to clear the
    // arena's own gate as well, and the gate is applied to the node being
    // emitted, which is the field's.
    let composite = Composite::new(true);
    let slot = composite.field_slot();
    let mut tree = WidgetTree::new();
    let id = tree.add(composite.access_disabled(false));
    tree.enabled_when(id, crate::signal::Signal::new(false));
    lay_out(&mut tree);
    let field = field_of(&slot);

    let update = tree.sync_accessibility();
    assert!(
        !node(&update, field)
            .expect("the field is in the tree")
            .is_disabled(),
        "the composite said not disabled, and that is what the field reports"
    );
    assert!(!tree.accessibility_node(field).is_disabled());
}

// ── Focus ────────────────────────────────────────────────────────────────
//
// The other shape a proxy takes is an editor's (`RichTextEditor`,
// `CodeEditor`, `LogView`): the composite is the widget that takes focus and
// the keys, and the node that holds the text is a body inside it that the
// keyboard never lands on. A screen reader reads the node an update names as
// focus when it arrives, and `accesskit_atspi_common` sends a caret or
// selection event only for that node (`adapter.rs:215-218`). Focus on the
// composite has to be published on the body, or the reader lands on a
// structural node with no name and no text and hears no caret move after it.

/// A focusable composite whose own node is structure, around a text body that
/// takes no focus of its own: the editors' shape.
#[derive(Debug)]
struct TextSurface {
    hands_on: bool,
    body: Rc<Cell<Option<WidgetId>>>,
}

impl TextSurface {
    fn new(hands_on: bool) -> Self {
        Self {
            hands_on,
            body: Rc::new(Cell::new(None)),
        }
    }

    fn body_slot(&self) -> Rc<Cell<Option<WidgetId>>> {
        self.body.clone()
    }
}

impl Widget for TextSurface {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(HandlerSet::new().focusable(true));
        let body = ctx.add(TextBody);
        self.body.set(Some(body));
        vec![body]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body.get().into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::GenericContainer);
    }

    fn accessibility_proxy(&self) -> Option<WidgetId> {
        self.body.get().filter(|_| self.hands_on)
    }
}

/// The text role, and the focus action a text surface offers, on a widget
/// that is not focusable.
#[derive(Debug)]
struct TextBody;

impl Widget for TextBody {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::MultilineTextInput);
        builder.add_action(accesskit::Action::Focus);
    }
}

#[test]
fn focus_on_a_composite_is_published_on_the_node_that_stands_for_it() {
    use accesskit_consumer::{FilterResult, common_filter};

    let surface = TextSurface::new(true);
    let slot = surface.body_slot();
    let mut tree = WidgetTree::new();
    let id = tree.add(surface.access_label_literal("Notes"));
    lay_out(&mut tree);
    tree.focus(id);
    let body = field_of(&slot);

    let update = tree.sync_accessibility();
    let consumer = accesskit_consumer::Tree::new(update.clone(), true);
    let state = consumer.state();
    let focus = state.focus().expect("the consumer resolves the focus");
    assert_eq!(
        (focus.role(), focus.label().as_deref()),
        (Role::MultilineTextInput, Some("Notes")),
        "a reader must land on the node that holds the text, named by the composite, \
         not on the composite's structural node"
    );
    assert_eq!(update.focus, widget_id_to_node_id(body));
    assert_eq!(common_filter(&focus), FilterResult::Include);
    let composite = state
        .node_by_tree_local_id(widget_id_to_node_id(id), accesskit::TreeId::ROOT)
        .expect("the composite's node is kept: it offers focus");
    assert_ne!(
        common_filter(&composite),
        FilterResult::Include,
        "no adapter shows the composite's node once it is not the focus"
    );
}

#[test]
fn a_composite_that_keeps_its_node_keeps_the_focus() {
    // The control for the test above: the same composite, not handing on.
    let surface = TextSurface::new(false);
    let mut tree = WidgetTree::new();
    let id = tree.add(surface);
    lay_out(&mut tree);
    tree.focus(id);

    assert_eq!(tree.sync_accessibility().focus, widget_id_to_node_id(id));
}

#[test]
fn a_focus_request_on_the_proxy_focuses_the_composite_it_stands_for() {
    // A screen reader asks for focus on the node it can see, which is the
    // body. The body takes no keys and runs none of the composite's focus
    // handlers (caret, IME, focus ring), so the keyboard belongs on the
    // composite; the published focus is the body either way.
    let surface = TextSurface::new(true);
    let slot = surface.body_slot();
    let mut tree = WidgetTree::new();
    let id = tree.add(surface);
    lay_out(&mut tree);
    let body = field_of(&slot);

    let mut ops = crate::window::NoopWindowOps;
    for _ in 0..2 {
        // Twice: asking again while the composite holds focus must leave it
        // there, not move the keyboard onto the body.
        assert!(tree.dispatch_access_action(
            widget_id_to_node_id(body),
            accesskit::Action::Focus,
            None,
            &mut ops,
        ));
        assert_eq!(
            tree.focused(),
            Some(id),
            "the keyboard lands on the composite that takes the keys"
        );
    }
    assert_eq!(tree.sync_accessibility().focus, widget_id_to_node_id(body));
}

#[test]
fn the_context_menu_the_composite_owns_is_offered_on_its_proxy() {
    // The editors own their right-click menu on the wrapper. Once focus is
    // published on the body, the body is where a reader looks for it.
    let surface = TextSurface::new(true);
    let slot = surface.body_slot();
    let mut tree = WidgetTree::new();
    let id = tree.add(surface.context_menu(|_, _| None));
    lay_out(&mut tree);
    tree.focus(id);
    let body = field_of(&slot);

    let update = tree.sync_accessibility();
    assert!(
        node(&update, body)
            .expect("the body is in the tree")
            .supports_action(accesskit::Action::ShowContextMenu),
        "the node focus is published on offers the composite's menu"
    );
    assert!(
        tree.accessibility_node(body)
            .actions()
            .contains(&accesskit::Action::ShowContextMenu),
        "and the tree's own query agrees with the walk"
    );
}
