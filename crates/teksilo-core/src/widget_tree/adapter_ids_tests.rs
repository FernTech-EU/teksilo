// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A node that leaves the tree a reader sees and comes back, as a platform
//! adapter receives it.
//!
//! `accesskit_atspi_common` announces a node that leaves the filtered tree as
//! defunct (`adapter.rs:91-106`, `remove_node`), and it leaves in three ways
//! (`adapter.rs:280-349`): it is gone from the update, it stops passing
//! `common_filter` (hidden, under a hidden ancestor, scrolled out of its
//! clipping parent, a `GenericContainer` focus has left), or the window loses
//! focus while it is a focused node nothing else keeps in. Nothing unsays the
//! state when the same id is added again (`add_node`, `adapter.rs:49-80`):
//! libatspi keeps it for the path, since it rejects every cache signal
//! `accesskit_unix` 0.23 sends, and Orca 46.1 drops every event from the
//! object ("Ignoring defunct object", `event_manager.py:796-798`).
//! `tools/reader/` measured it on a reopened tab page, menu, combo list,
//! calendar and a row scrolled back into view.
//!
//! So the tree must never hand an adapter, for a node, an id the adapter has
//! already removed. Every test replays what the tree hands the adapter through
//! `accesskit_consumer` with the adapter's own add and remove rules, and holds
//! the tree to two things a reader needs: no id comes back once removed, and
//! focus landing on a returning control comes from a live object.

use std::collections::HashSet;

use accesskit::{NodeId, Role, TreeUpdate};
use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeChangeHandler, common_filter};
use teksilo_canvas::SizeProposal;

use crate::accessibility::AccessNodeBuilder;
use crate::signal::Signal;
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget::{LayoutContext, LayoutResponse, Widget};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

/// What the tree hands a platform adapter on a frame, the window having focus
/// or not, as `teksilo-app` delivers it.
fn handed_to_adapter(tree: &mut WidgetTree, window_focused: bool) -> TreeUpdate {
    let _ = tree.sync_accessibility();
    tree.deliver_accessibility(window_focused)
}

/// A platform adapter's view of the tree, kept by `accesskit_atspi_common`'s
/// rules.
struct Adapter {
    replay: Tree,
    /// Every id the adapter has removed, which AT-SPI announced as defunct
    /// and libatspi keeps as defunct.
    defunct: HashSet<NodeId>,
    window_focused: bool,
}

/// What one update did, as a reader is told it.
#[derive(Debug, Default)]
struct Frame {
    /// Ids the adapter added although it had removed them before.
    revived: Vec<(NodeId, String)>,
    /// Focus moved to this node: its name, its id, and whether libatspi holds
    /// the id defunct, in which case Orca drops the event.
    focus: Option<(String, NodeId, bool)>,
}

struct Rules<'a> {
    defunct: &'a mut HashSet<NodeId>,
    frame: Frame,
}

fn included(node: &NodeRef) -> bool {
    common_filter(node) == FilterResult::Include
}

fn name(node: &NodeRef) -> String {
    node.label().unwrap_or_default()
}

impl Rules<'_> {
    /// `add_node`.
    fn add(&mut self, node: &NodeRef) {
        let id = node.locate().0;
        if self.defunct.contains(&id) {
            self.frame.revived.push((id, name(node)));
        }
    }

    /// `add_subtree`.
    fn add_subtree(&mut self, node: &NodeRef) {
        self.add(node);
        for child in node.filtered_children(&common_filter) {
            self.add_subtree(&child);
        }
    }

    /// `remove_node`, which emits `StateChanged(Defunct, true)`.
    fn remove(&mut self, node: &NodeRef) {
        self.defunct.insert(node.locate().0);
    }

    /// `remove_subtree`.
    fn remove_subtree(&mut self, node: &NodeRef) {
        for child in node.filtered_children(&common_filter) {
            self.remove_subtree(&child);
        }
        self.remove(node);
    }
}

impl TreeChangeHandler for Rules<'_> {
    fn node_added(&mut self, node: &NodeRef) {
        if included(node) {
            self.add(node);
        }
    }

    fn node_updated(&mut self, old: &NodeRef, new: &NodeRef) {
        let (was, is) = (common_filter(old), common_filter(new));
        if was == is {
            return;
        }
        if is == FilterResult::Include {
            if was == FilterResult::ExcludeSubtree {
                self.add_subtree(new);
            } else {
                self.add(new);
            }
        } else if was == FilterResult::Include {
            if is == FilterResult::ExcludeSubtree {
                self.remove_subtree(old);
            } else {
                self.remove(old);
            }
        }
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef>, new: Option<&NodeRef>) {
        if let Some(node) = new {
            let id = node.locate().0;
            self.frame.focus = Some((name(node), id, self.defunct.contains(&id)));
        }
    }

    fn node_removed(&mut self, node: &NodeRef) {
        if included(node) {
            self.remove(node);
        }
    }
}

impl Adapter {
    fn attach(tree: &mut WidgetTree) -> Self {
        tree.layout(SizeProposal::exact(400.0, 300.0));
        Self {
            replay: Tree::new(handed_to_adapter(tree, true), true),
            defunct: HashSet::new(),
            window_focused: true,
        }
    }

    /// Run one frame, as `teksilo-app` does: lay out, sync, deliver.
    fn frame(&mut self, tree: &mut WidgetTree) -> Frame {
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = handed_to_adapter(tree, self.window_focused);
        let mut rules = Rules {
            defunct: &mut self.defunct,
            frame: Frame::default(),
        };
        self.replay.update_and_process_changes(update, &mut rules);
        rules.frame
    }

    /// The window gains or loses focus. The adapter hears it from winit at
    /// once, between two frames (`accesskit_winit` `process_event`).
    fn window_focus(&mut self, focused: bool) -> Frame {
        self.window_focused = focused;
        let mut rules = Rules {
            defunct: &mut self.defunct,
            frame: Frame::default(),
        };
        self.replay
            .update_host_focus_state_and_process_changes(focused, &mut rules);
        rules.frame
    }

    /// The id the adapter knows the node named `text` by, if it is in the
    /// tree a reader walks.
    fn id_of(&self, text: &str) -> Option<NodeId> {
        fn walk(node: NodeRef<'_>, text: &str) -> Option<NodeId> {
            if node.label().as_deref() == Some(text) {
                return Some(node.locate().0);
            }
            node.filtered_children(&common_filter)
                .find_map(|child| walk(child, text))
        }
        walk(self.replay.state().root(), text)
    }
}

/// Fail with what a reader lost when the frame brought back an id the adapter
/// had removed, or put focus on one.
#[track_caller]
fn assert_heard(frame: &Frame, what: &str, expected_focus: &str) {
    assert!(
        frame.revived.is_empty(),
        "{what}: the adapter was handed {:?} again after it had removed them; libatspi holds \
         each for defunct and Orca drops its events",
        frame.revived
    );
    match &frame.focus {
        Some((name, id, defunct)) => {
            assert_eq!(name, expected_focus, "{what}: focus went elsewhere");
            assert!(
                !defunct,
                "{what}: focus landed on {name:?} under #{}, an id the adapter had already \
                 removed, so Orca drops it ('Ignoring defunct object') and says nothing",
                id.0
            );
        }
        None => panic!("{what}: focus never reached {expected_focus:?}"),
    }
}

fn button(label: &str) -> impl Widget + 'static {
    FillWidget::new()
        .access_role(Role::Button)
        .access_label(label.to_string())
        .focusable(true)
}

/// The catalog's text page, reduced: a field on a page a tab switch hides and
/// shows, and a control outside it that keeps focus meanwhile. Three rounds, as
/// `catalog-b-text-return` ran them; Orca heard the first and none after.
#[test]
fn a_page_shown_again_comes_back_under_ids_the_adapter_never_removed() {
    let mut tree = WidgetTree::new();
    let username = tree.add(
        FillWidget::new()
            .access_role(Role::TextInput)
            .access_label("Username")
            .focusable(true),
    );
    let page = tree.add(StackWidget::new().child(username));
    let tab = tree.add(button("Scene"));
    tree.focus(username);
    let mut adapter = Adapter::attach(&mut tree);

    for round in 1..=3 {
        tree.focus(tab);
        tree.set_dormant(page);
        let _ = adapter.frame(&mut tree);
        assert_eq!(
            adapter.id_of("Username"),
            None,
            "the hidden page left the tree"
        );

        tree.activate(page);
        let back = adapter.frame(&mut tree);
        assert!(
            back.revived.is_empty(),
            "round {round}: the page came back under ids the adapter had removed: {:?}",
            back.revived
        );

        tree.focus(username);
        let focus = adapter.frame(&mut tree);
        assert_heard(&focus, &format!("round {round}"), "Username");
    }
}

/// A viewport that clips its one child, a button, and places it `offset`
/// below its top edge, bound at `Relayout`: a scroll, which re-places the
/// cached tree rather than walking it.
#[derive(Debug)]
struct Viewport {
    offset: Signal<f32>,
    child: Option<WidgetId>,
}

impl Widget for Viewport {
    fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
        self.offset.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            crate::binding::BindingLevel::Relayout,
        );
        let child = ctx.add(button("Bar chart"));
        self.child = Some(child);
        vec![child]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        teksilo_canvas::Size::new(200.0, 100.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y + self.offset.get());
            child.size = teksilo_canvas::Size::new(80.0, 20.0);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::ScrollView);
        builder.set_name("Charts");
        builder.inner_mut().set_clips_children();
    }
}

/// `catalog-b-charts-marks`: Tab down past the bar chart until it scrolls out
/// of view, Shift+Tab back to it. Orca dropped the focus as defunct in 3 of 3
/// runs.
#[test]
fn a_control_scrolled_back_into_view_is_a_live_object() {
    let offset = Signal::new(10.0_f32);
    let mut tree = WidgetTree::new();
    let viewport = tree.add(Viewport {
        offset: offset.clone(),
        child: None,
    });
    let other = tree.add(button("Donut"));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let bar = tree.arena.children(viewport)[0];
    tree.focus(bar);
    let mut adapter = Adapter::attach(&mut tree);

    tree.focus(other);
    let _ = adapter.frame(&mut tree);
    offset.set(400.0);
    let _ = adapter.frame(&mut tree);
    assert_eq!(
        adapter.id_of("Bar chart"),
        None,
        "scrolled out, the chart left the tree"
    );

    offset.set(10.0);
    let back = adapter.frame(&mut tree);
    assert!(
        back.revived.is_empty(),
        "the chart scrolled back under an id the adapter had removed: {:?}",
        back.revived
    );
    tree.focus(bar);
    let focus = adapter.frame(&mut tree);
    assert_heard(&focus, "Shift+Tab back to the chart", "Bar chart");
}

/// A subtree hidden and shown again, which the adapter removes and adds as a
/// whole (`remove_subtree`, `add_subtree`) although no node of it left the
/// update.
#[test]
fn a_subtree_hidden_and_shown_again_comes_back_live() {
    let hidden = Signal::new(false);
    let mut tree = WidgetTree::new();
    let save = tree.add(button("Save"));
    let _panel = tree.add(StackWidget::new().child(save).access_hidden(hidden.clone()));
    let other = tree.add(button("Close"));
    tree.focus(save);
    let mut adapter = Adapter::attach(&mut tree);

    tree.focus(other);
    let _ = adapter.frame(&mut tree);
    hidden.set(true);
    let _ = adapter.frame(&mut tree);
    assert_eq!(
        adapter.id_of("Save"),
        None,
        "the hidden panel left the tree"
    );

    hidden.set(false);
    let back = adapter.frame(&mut tree);
    assert!(
        back.revived.is_empty(),
        "the panel came back under ids the adapter had removed: {:?}",
        back.revived
    );
    tree.focus(save);
    let focus = adapter.frame(&mut tree);
    assert_heard(&focus, "focus back on Save", "Save");
}

/// A focusable node with no role of its own: the shape of the rich-text
/// editor's wrapper, which is in the filtered tree only while it holds focus
/// (`common_filter` excludes a `GenericContainer` that is not focused).
#[derive(Debug)]
struct Wrapper;

impl Widget for Wrapper {
    fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(crate::widget_builder::HandlerSet::new().focusable(true));
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(100.0, 40.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::GenericContainer);
    }
}

/// `text-editor-tab-in`: Ctrl+Tab out of the editor, Ctrl+Shift+Tab back.
/// The wrapper left the tree as focus left it, and came back under its old id:
/// "Ignoring defunct object: [section]" in 5 of 5 returns.
#[test]
fn a_container_that_is_in_the_tree_only_while_focused_comes_back_live() {
    let mut tree = WidgetTree::new();
    let editor = tree.add(Wrapper);
    let divider = tree.add(button("Splitter divider"));
    tree.focus(editor);
    let mut adapter = Adapter::attach(&mut tree);

    for round in 1..=2 {
        tree.focus(divider);
        let _ = adapter.frame(&mut tree);
        tree.focus(editor);
        let back = adapter.frame(&mut tree);
        assert_heard(&back, &format!("round {round}, back into the editor"), "");
    }
}

/// The window loses focus while the editor's wrapper holds it, and gets it
/// back: the adapter removes the wrapper as the window deactivates and adds it
/// as it activates, both at once, from winit's event (`accesskit_winit`
/// `process_event`), with a frame in between.
#[test]
fn a_focused_container_comes_back_live_when_the_window_is_refocused() {
    let mut tree = WidgetTree::new();
    let editor = tree.add(Wrapper);
    tree.add(button("Splitter divider"));
    tree.focus(editor);
    let mut adapter = Adapter::attach(&mut tree);
    let _ = adapter.frame(&mut tree);

    let _ = adapter.window_focus(false);
    let _ = adapter.frame(&mut tree);
    let back = adapter.window_focus(true);
    assert!(
        back.revived.is_empty(),
        "the editor came back under an id the adapter had removed as the window lost focus: {:?}",
        back.revived
    );
}

/// The focused control leaves the reader's tree while the window is in the
/// background (scrolled out of its viewport, or hidden), and the user comes
/// back to the window before anything else redraws it. A focused node passes
/// `common_filter` whatever it is once its window has focus, so the adapter
/// adds it back at once, from winit's event, with no update in between: the
/// update that took it out must already have given it its new id.
#[test]
fn a_focused_control_that_left_while_the_window_was_away_comes_back_live() {
    let offset = Signal::new(10.0_f32);
    let mut tree = WidgetTree::new();
    let viewport = tree.add(Viewport {
        offset: offset.clone(),
        child: None,
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let bar = tree.arena.children(viewport)[0];
    tree.focus(bar);
    let mut adapter = Adapter::attach(&mut tree);

    let _ = adapter.window_focus(false);
    let _ = adapter.frame(&mut tree);
    offset.set(400.0);
    let _ = adapter.frame(&mut tree);
    let back = adapter.window_focus(true);
    assert_heard(
        &back,
        "back to the window, the chart scrolled out",
        "Bar chart",
    );

    let hidden = Signal::new(false);
    let mut tree = WidgetTree::new();
    let save = tree.add(button("Save"));
    tree.add(StackWidget::new().child(save).access_hidden(hidden.clone()));
    tree.focus(save);
    let mut adapter = Adapter::attach(&mut tree);

    let _ = adapter.window_focus(false);
    let _ = adapter.frame(&mut tree);
    hidden.set(true);
    let _ = adapter.frame(&mut tree);
    let back = adapter.window_focus(true);
    assert_heard(&back, "back to the window, the panel hidden", "Save");
}

/// A screen reader acts on a control that came back by the id it was handed,
/// and the action reaches the widget: `teksilo-app` routes every request the
/// adapter sends through `resolve_adapter_action`.
#[test]
fn an_action_on_a_control_that_came_back_reaches_it() {
    let mut tree = WidgetTree::new();
    let username = tree.add(
        FillWidget::new()
            .access_role(Role::TextInput)
            .access_label("Username")
            .focusable(true),
    );
    let page = tree.add(StackWidget::new().child(username));
    tree.add(button("Scene"));
    let mut adapter = Adapter::attach(&mut tree);
    tree.set_dormant(page);
    let _ = adapter.frame(&mut tree);
    tree.activate(page);
    let _ = adapter.frame(&mut tree);

    let handed = adapter.id_of("Username").expect("the page is back");
    let own = crate::accessibility::widget_id_to_node_id(username);
    assert_ne!(handed, own, "the field came back under a new id");
    let request = tree
        .resolve_adapter_action(accesskit::ActionRequest {
            action: accesskit::Action::Focus,
            target_tree: accesskit::TreeId::ROOT,
            target_node: handed,
            data: None,
        })
        .expect("the id names the field");
    assert_eq!(request.target_node, own);
}
