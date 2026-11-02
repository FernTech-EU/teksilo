// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless widget-level integration tests for [`DockingLayout`](super::DockingLayout):
//! build a real layout in a `WidgetTree`, lay it out, and assert the geometry
//! and accessibility wiring.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Size, SizeProposal};
use teksilo_core::accessibility::widget_id_to_node_id;
use teksilo_core::accesskit;
use teksilo_core::accesskit::Role;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use super::{
    DockOpenLocation, DockPolicy, DockSide, DockWidget, DockWidgetId, DockingLayout, DockingModel,
};

#[derive(Debug)]
struct FixedLeaf(f32, f32);
impl Widget for FixedLeaf {
    fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(self.0, self.1).into()
    }
}

fn tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn subtree_has_role(tree: &WidgetTree, id: WidgetId, role: Role) -> bool {
    if tree.accessibility_node(id).role() == role {
        return true;
    }
    tree.children(id)
        .iter()
        .any(|&c| subtree_has_role(tree, c, role))
}

fn count_role(tree: &WidgetTree, id: WidgetId, role: Role) -> usize {
    let here = usize::from(tree.accessibility_node(id).role() == role);
    here + tree
        .children(id)
        .iter()
        .map(|&c| count_role(tree, c, role))
        .sum::<usize>()
}

/// Like [`count_role`], but counts only nodes that are actually laid out.
/// Overflow parks the surplus rail items dormant (zero bounds) rather than
/// removing them from the arena, so a plain `count_role` cannot see overflow at
/// all — it would return the full item count regardless.
fn count_role_laid_out(tree: &WidgetTree, id: WidgetId, role: Role) -> usize {
    let here =
        usize::from(tree.accessibility_node(id).role() == role && tree.bounds(id).height > 0.0);
    here + tree
        .children(id)
        .iter()
        .map(|&c| count_role_laid_out(tree, c, role))
        .sum::<usize>()
}

fn dock(title: &'static str) -> (DockWidgetId, DockWidget) {
    let id = DockWidgetId::fresh();
    (
        id,
        DockWidget::new(id, lit!(title), |_| FixedLeaf(120.0, 120.0)),
    )
}

/// Register a dock's metadata directly (no `DockingLayout` render needed) so
/// `open_dock` works in a standalone model test.
fn reg(m: &DockingModel, side: DockSide) -> DockWidgetId {
    use super::model::DockWidgetMeta;
    let id = DockWidgetId::fresh();
    m.register_meta(
        id,
        DockWidgetMeta {
            title: lit!("Dock"),
            icon: None,
            min_size: None,
            default: DockOpenLocation::side(side),
            header_actions: None,
            show_header: false,
        },
    );
    id
}

/// Find a node with `role` + `name` that is actually laid out (non-zero
/// bounds) — skips stale/dormant duplicates.
fn find_role_name(tree: &WidgetTree, id: WidgetId, role: Role, name: &str) -> Option<WidgetId> {
    let node = tree.accessibility_node(id);
    if node.role() == role && node.name() == Some(name) && tree.bounds(id).height > 0.0 {
        return Some(id);
    }
    tree.children(id)
        .iter()
        .find_map(|&c| find_role_name(tree, c, role, name))
}

/// First node with the given role (and non-zero height) in a subtree.
fn find_first_role(tree: &WidgetTree, id: WidgetId, role: Role) -> Option<WidgetId> {
    if tree.accessibility_node(id).role() == role && tree.bounds(id).height > 0.0 {
        return Some(id);
    }
    tree.children(id)
        .iter()
        .find_map(|&c| find_first_role(tree, c, role))
}

/// First laid-out node whose concrete widget type ends with `suffix`.
///
/// Needed for the activity bar's outer strip: its root is deliberately a
/// property-free `Role::GenericContainer` (so the AT pass prunes it — see
/// `DockActivityBar::accessibility`), which makes it indistinguishable by role
/// from every layout primitive in the tree. `Role::TabList` is NOT a substitute:
/// it now sits on `DockRailTabList`, which wraps only the items and is inset by
/// the rail's padding, so measuring it answers "how wide is the item column",
/// not "how wide is the strip".
fn find_widget_type(tree: &WidgetTree, id: WidgetId, suffix: &str) -> Option<WidgetId> {
    if tree
        .widget_type_name(id)
        .is_some_and(|n| n.ends_with(suffix))
        && tree.bounds(id).height > 0.0
    {
        return Some(id);
    }
    tree.children(id)
        .iter()
        .find_map(|&c| find_widget_type(tree, c, suffix))
}

/// First laid-out node whose AT name equals `name`, anywhere in a subtree.
fn find_named(tree: &WidgetTree, id: WidgetId, name: &str) -> Option<WidgetId> {
    if tree.accessibility_node(id).name() == Some(name) && tree.bounds(id).height > 0.0 {
        return Some(id);
    }
    tree.children(id)
        .iter()
        .find_map(|&c| find_named(tree, c, name))
}

#[test]
fn collapsing_a_split_accordion_keeps_its_header() {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let model = DockingModel::new();
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    // Two docks split in one leading tab → two vertical accordions.
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    // Let the side finish animating open so the panes have real sizes.
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // The accordion node id is stable across the collapse (no rebuild).
    let acc_a = find_role_name(&t, root, Role::Button, "Aaa").expect("accordion A header exists");
    let ab = t.bounds(acc_a);
    assert!(ab.height > 0.0, "accordion A visible before collapse");

    // Tap in A's header (top of the accordion) → collapse it → the Splitter
    // folds A's pane to the header sliver.
    let p = Point::new(ab.x + 10.0, ab.y + 6.0);
    t.dispatch_event(WidgetEvent::pointer_down(
        p,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    t.dispatch_event(WidgetEvent::pointer_up(
        p,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    t.tick_animations(Duration::from_millis(400));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // The accordion (and its header) must STILL be laid out (live, not dormant)
    // after collapse — the pane folds to a header sliver, the panel doesn't
    // vanish. (Its bounds stay at the full layout size; the Splitter clips it.)
    assert!(
        t.bounds(acc_a).height > 1.0,
        "collapsed accordion A must stay laid out (a header sliver), got {}",
        t.bounds(acc_a).height
    );
}

/// A click on a control inside a split pane works the control and leaves the
/// pane open. The pane's accordion header is the dock's drag handle, so the
/// accordion competes for every press in its pane through that drag, and the
/// release used to fire its collapsing tap beside the control's own.
#[test]
fn clicking_a_button_in_a_split_pane_does_not_collapse_it() {
    use crate::button::Button;
    use std::cell::Cell;
    use std::rc::Rc;

    let clicks = Rc::new(Cell::new(0));
    let model = DockingModel::new();
    let a = DockWidgetId::fresh();
    let (b, dwb) = dock("Bbb");
    let dwa = DockWidget::new(a, lit!("Aaa"), {
        let clicks = clicks.clone();
        move |_| {
            let clicks = clicks.clone();
            crate::scroll_area::ScrollArea::new().child(
                Button::new(lit!("Press")).on_activate_fn(move |_| clicks.set(clicks.get() + 1)),
            )
        }
    });
    let mut t = tree().with_text_backend(Rc::new(std::cell::RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let acc_a = find_role_name(&t, root, Role::Button, "Aaa").expect("accordion A header exists");
    assert!(t.accessibility_node(acc_a).is_expanded());
    let button =
        find_role_name(&t, root, Role::Button, "Press").expect("dock A's button is laid out");

    t.click(button);
    t.tick_animations(Duration::from_millis(400));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(clicks.get(), 1, "the click works the button");
    assert!(
        t.accessibility_node(acc_a).is_expanded(),
        "the click leaves dock A's accordion open"
    );
}

#[test]
fn empty_layout_center_fills() {
    let model = DockingModel::new();
    let mut t = tree();
    let root = t.add(DockingLayout::new(model).center(FixedLeaf(200.0, 200.0)));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let center = t.children(root)[0];
    let cb = t.bounds(center);
    assert!(
        (cb.width - 1000.0).abs() < 0.5,
        "center fills with no docks"
    );
}

#[test]
fn open_dock_insets_center_and_emits_landmark() {
    let model = DockingModel::new();
    let (id, dw) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let center = t.children(root)[0];
    let cb = t.bounds(center);
    assert!(
        cb.x > 100.0,
        "centre is inset by the leading dock (got x={})",
        cb.x
    );
    assert!(
        subtree_has_role(&t, root, Role::Complementary),
        "the leading side region is a Complementary landmark"
    );
}

#[test]
fn rail_presentation_emits_a_tablist_rail() {
    let model = DockingModel::new();
    let (id, dw) = dock("Explorer");
    model.set_side_rail(DockSide::Leading, 48.0);
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(
        subtree_has_role(&t, root, Role::TabList),
        "a Rail-presentation side renders a TabList activity bar"
    );
}

#[test]
fn hiding_a_side_grows_the_center() {
    let model = DockingModel::new();
    let (id, dw) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let center = t.children(root)[0];
    let inset_x = t.bounds(center).x;
    assert!(inset_x > 100.0);

    // Hide the side and let the collapse animation run to completion.
    model.set_side_visible(DockSide::Leading, false);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let grown_x = t.bounds(center).x;
    assert!(
        grown_x < inset_x - 50.0,
        "hiding the side grows the centre (was x={inset_x}, now x={grown_x})"
    );
}

#[test]
fn two_stacked_docks_build_without_panic() {
    let model = DockingModel::new();
    let (a, dwa) = dock("A");
    let (b, dwb) = dock("B");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom).stack());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    // The bottom side hosts both docks in a ToolBox (one tab).
    assert_eq!(model.tab_count(DockSide::Bottom), 1);
    // Sanity: the tree laid out with a sane root size.
    assert!(t.bounds(root).width > 0.0);
}

#[test]
fn toggling_a_rail_side_does_not_hang() {
    let model = DockingModel::new();
    let (id, dw) = dock("Explorer");
    model.set_side_rail(DockSide::Leading, 48.0);
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Hide (the demo's "Toggle Sidebar"), animate, then show again.
    model.toggle_side_visible(DockSide::Leading);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(!model.is_side_visible(DockSide::Leading));

    model.toggle_side_visible(DockSide::Leading);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(model.is_side_visible(DockSide::Leading));
}

#[test]
fn clicking_a_toggle_button_does_not_hang() {
    use crate::button::Button;
    use crate::primitives::VStack;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct Root {
        model: DockingModel,
        dock: DockWidgetId,
        btn: Rc<Cell<Option<WidgetId>>>,
        root: Option<WidgetId>,
    }
    impl Widget for Root {
        fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
            let m = self.model.clone();
            let btn = ctx.add(
                Button::new(lit!("Toggle"))
                    .on_activate_fn(move |_| m.toggle_side_visible(DockSide::Leading)),
            );
            self.btn.set(Some(btn));
            let dock = self.dock;
            let layout = ctx.add(
                DockingLayout::new(self.model.clone())
                    .center(FixedLeaf(200.0, 200.0))
                    .dock(DockWidget::new(dock, lit!("Explorer"), |_| {
                        FixedLeaf(120.0, 120.0)
                    })),
            );
            self.model
                .open_dock(dock, DockOpenLocation::side(DockSide::Leading));
            let root = ctx.add(VStack::new().child(btn).child(layout));
            self.root = Some(root);
            vec![root]
        }
        fn layout_response(&self, p: SizeProposal, c: &LayoutContext) -> LayoutResponse {
            self.root
                .and_then(|id| c.child_size(id, p))
                .unwrap_or_else(|| p.resolve(0.0, 0.0))
                .into()
        }
        fn place_children(
            &self,
            b: teksilo_canvas::Rect,
            _p: SizeProposal,
            ch: &mut [teksilo_core::widget::WidgetPlacement],
            _c: &LayoutContext,
        ) {
            for c in ch.iter_mut() {
                c.origin = b.origin();
                c.size = b.size();
            }
        }
        fn children(&self) -> Vec<WidgetId> {
            self.root.into_iter().collect()
        }
    }

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let btn = Rc::new(Cell::new(None));
    let mut t = tree();
    t.add(Root {
        model: model.clone(),
        dock: DockWidgetId::fresh(),
        btn: btn.clone(),
        root: None,
    });
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(model.is_side_visible(DockSide::Leading));

    // Click the toggle through the real event system, then drive frames.
    t.click(btn.get().unwrap());
    for _ in 0..60 {
        t.layout(SizeProposal::exact(1000.0, 800.0));
        t.tick_animations(Duration::from_millis(16));
    }
    assert!(
        !model.is_side_visible(DockSide::Leading),
        "click hid the side"
    );
}

#[test]
fn collapsing_a_side_with_a_stacked_toolbox_does_not_hang() {
    // Repro of the demo's "Toggle Sidebar": the leading side stacks two docks
    // in a ToolBox, then animates closed while the ToolBox is laid out at a
    // shrinking width.
    let model = DockingModel::new();
    let (a, dwa) = dock("Explorer");
    let (b, dwb) = dock("Search");
    model.set_side_rail(DockSide::Leading, 48.0);
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Animate the side closed in small frame steps.
    model.set_side_visible(DockSide::Leading, false);
    for _ in 0..60 {
        t.layout(SizeProposal::exact(1000.0, 800.0));
        t.tick_animations(Duration::from_millis(16));
    }
    assert!(!model.is_side_visible(DockSide::Leading));
}

#[test]
fn collapsing_content_keeps_full_layout_size_and_does_not_reflow() {
    // The content slides out (clipped) rather than reflowing: the side's
    // content child stays laid out at its full size even mid-collapse, while
    // the clip (its parent) shrinks.
    let model = DockingModel::new();
    let (id, dw) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // children: [centre region, leading region [rail, clip, handle], …]
    let leading = t.children(root)[1];
    let clip = t.children(leading)[1];
    let panel = t.children(clip)[0];
    let full_w = t.bounds(panel).width;
    assert!(full_w > 100.0, "content laid out at full width ({full_w})");

    // Collapse partway (one small tick → progress ≈ mid).
    model.set_side_visible(DockSide::Leading, false);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(40));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let clip_w = t.bounds(clip).width;
    let panel_w = t.bounds(panel).width;
    assert!(
        clip_w < full_w - 1.0,
        "the clip shrinks as the side collapses (clip={clip_w}, full={full_w})"
    );
    assert!(
        (panel_w - full_w).abs() < 1.0,
        "the content keeps its full layout (no reflow): full={full_w}, mid-collapse={panel_w}"
    );
}

#[test]
fn two_tabs_render_a_strip_with_tab_items() {
    let model = DockingModel::new();
    let (a, dwa) = dock("Terminal");
    let (b, dwb) = dock("Problems");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(model.tab_count(DockSide::Bottom), 2);
    // The in-side strip renders one Role::Tab per tab — these are the
    // whole-tab drag sources / drop targets.
    assert!(
        count_role(&t, root, Role::Tab) >= 2,
        "a 2-tab side renders its strip (got {} Tab nodes)",
        count_role(&t, root, Role::Tab)
    );
}

#[test]
fn dock_drop_recognizes_both_tab_drag_sources() {
    use std::num::NonZeroU64;

    use teksilo_core::DragPayload;

    use super::DockTabId;
    use super::drag::{DockDragData, DockTabDragData, dropped_dock_tab, dropped_dock_widget};
    use crate::tab_widget::{TabBarDragData, TabHandle, TabId};

    // 1. An activity-rail item drag carries `DockTabDragData`.
    let rail = DragPayload::typed(DockTabDragData {
        tab_id: DockTabId::from_raw(7),
        source_side: DockSide::Leading,
    });
    assert_eq!(dropped_dock_tab(&rail).map(|t| t.raw()), Some(7));
    assert!(dropped_dock_widget(&rail).is_none());

    // 2. A tab dragged from the in-strip TabWidget carries
    //    `TabBarDragData<TabHandle>` — whose `source_id` the dock builds from
    //    the same `DockTabId`. Both must resolve to the SAME dock tab (this is
    //    the bug: the strip drag used to be rejected by dock drop targets).
    let mut t = tree();
    let bar = t.add(FixedLeaf(1.0, 1.0));
    let strip = DragPayload::typed(TabBarDragData::<TabHandle> {
        source_index: 0,
        source_bar_id: bar,
        source_id: TabId::from_raw(NonZeroU64::new(7).unwrap()),
        item: None,
    });
    assert_eq!(
        dropped_dock_tab(&strip).map(|t| t.raw()),
        Some(7),
        "an in-strip tab drag must resolve to the same dock tab as a rail drag"
    );

    // 3. A single-dock (split-pane header) drag carries `DockDragData`.
    let widget = DragPayload::typed(DockDragData {
        dock_id: DockWidgetId::from_raw(3),
    });
    assert_eq!(dropped_dock_widget(&widget).map(|d| d.raw()), Some(3));
    assert!(dropped_dock_tab(&widget).is_none());
}

#[test]
fn revealed_empty_side_shows_a_drop_target() {
    let model = DockingModel::new();
    let (a, dwa) = dock("Terminal");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    // A lives on Leading; Trailing has no docks.
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(
        find_named(&t, root, "Drop a panel here").is_none(),
        "a hidden empty side shows nothing (its panel is dormant)"
    );

    // Reveal the empty Trailing side (as a toolbar button / Cmd+B would).
    model.set_side_visible(DockSide::Trailing, true);
    t.tick_animations(Duration::from_millis(400));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // The revealed-but-empty side now offers a drop target so the first dock
    // can be dragged in.
    assert!(
        find_named(&t, root, "Drop a panel here").is_some(),
        "a revealed empty side renders its 'Drop a panel here' drop target"
    );
}

#[test]
fn hiding_an_activity_drops_its_strip_tab() {
    let model = DockingModel::new();
    let (a, dwa) = dock("Terminal");
    let (b, dwb) = dock("Problems");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let before = count_role(&t, root, Role::Tab);
    assert!(before >= 2);

    // Hide one activity (the context-menu "Hide" / unchecked in the list).
    let hidden_tab = model.side_tabs(DockSide::Bottom)[1].id;
    model.set_tab_hidden(hidden_tab, true);
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(
        count_role(&t, root, Role::Tab),
        before - 1,
        "the hidden activity's strip tab is gone, the other remains"
    );
    // Still in the model (restorable via the activities checklist).
    assert_eq!(model.tab_count(DockSide::Bottom), 2);

    // Restore → the tab comes back.
    model.set_tab_hidden(hidden_tab, false);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert_eq!(count_role(&t, root, Role::Tab), before, "restored");
}

#[test]
fn changing_tab_display_mode_rebuilds_the_strip() {
    use super::DockTabDisplay;

    // An icon-less dock — the case the docking example ships, and the one that
    // looked like "the Tab size menu does nothing". The visible chrome changes
    // per mode (so the tab width changes), but the tab's *accessible name* must
    // stay "Terminal" in every mode (the displayed initial is a visual only).
    let model = DockingModel::new();
    let a_id = DockWidgetId::fresh();
    let dwa = DockWidget::new(a_id, lit!("Terminal"), |_| FixedLeaf(120.0, 120.0));
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    model.open_dock(a_id, DockOpenLocation::side(DockSide::Bottom));

    // The tab header keeps the accessible name "Terminal" in every mode.
    let tab_width = |t: &WidgetTree, root: WidgetId| -> f32 {
        let tab = find_role_name(t, root, Role::Tab, "Terminal")
            .expect("the tab header keeps its accessible name in every display mode");
        t.bounds(tab).width
    };

    // Text mode (default): the full title is shown — the tab is wide.
    model.set_side_tab_display(DockSide::Bottom, DockTabDisplay::Text);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let w_text = tab_width(&t, root);

    // Icon-only: even an icon-less dock visibly changes — it shows the title's
    // initial letter, so the tab is narrower than the full title.
    model.set_side_tab_display(DockSide::Bottom, DockTabDisplay::Icon);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let w_icon = tab_width(&t, root);
    assert!(
        w_icon < w_text,
        "icon-less Icon mode shows the compact initial — narrower ({w_icon} < {w_text})"
    );

    // Icon + Text restores the full title → wider again than the initial.
    model.set_side_tab_display(DockSide::Bottom, DockTabDisplay::IconText);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let w_icontext = tab_width(&t, root);
    assert!(
        w_icontext > w_icon,
        "icon + text restores the full title — wider than the initial ({w_icontext} > {w_icon})"
    );
}

#[test]
fn icon_only_tab_sizes_to_its_icon() {
    use super::DockTabDisplay;
    use crate::primitives::IconWidget;

    // A dock *with* an icon: icon-only mode must shrink the tab to its icon
    // instead of clamping to the editor-tab minimum, and icon+text must be
    // wider than text-only (the icon is accounted for).
    let model = DockingModel::new();
    let a_id = DockWidgetId::fresh();
    let dwa = DockWidget::new(a_id, lit!("Terminal"), |_| FixedLeaf(120.0, 120.0))
        .icon(|| IconWidget::checkmark(16.0));
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    model.open_dock(a_id, DockOpenLocation::side(DockSide::Bottom));

    let tab_width = |t: &WidgetTree, root: WidgetId| -> f32 {
        let tab = find_first_role(t, root, Role::Tab).expect("a strip tab renders");
        t.bounds(tab).width
    };

    // Text-only.
    model.set_side_tab_display(DockSide::Bottom, DockTabDisplay::Text);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let w_text = tab_width(&t, root);

    // Icon-only must be strictly narrower (sized to the icon, not the text min).
    model.set_side_tab_display(DockSide::Bottom, DockTabDisplay::Icon);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let w_icon = tab_width(&t, root);
    assert!(
        w_icon < w_text,
        "icon-only tab ({w_icon}) must be narrower than the text tab ({w_text})"
    );

    // Icon + text must be wider than BOTH icon-only and text-only — the icon's
    // width is genuinely added to the label's (the bug was icon+text == text).
    model.set_side_tab_display(DockSide::Bottom, DockTabDisplay::IconText);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let w_icontext = tab_width(&t, root);
    assert!(
        w_icontext > w_icon,
        "icon + text ({w_icontext}) must be wider than icon-only ({w_icon})"
    );
    assert!(
        w_icontext > w_text,
        "icon + text ({w_icontext}) must be wider than text-only ({w_text}) — \
         the icon's width is added"
    );
}

#[test]
fn rail_strip_width_follows_the_size_mode() {
    use super::{DockRail, DockRailItemSize};
    use crate::icon_button::IconButtonSize;

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(DockRail::new(DockSide::Leading).size(IconButtonSize::Large)),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Measure the activity-bar STRIP, not its item column. Looking the strip up
    // by `Role::TabList` would silently start measuring `DockRailTabList` (the
    // items alone, inset by the rail padding) and the assertion below would
    // still pass while no longer testing what it claims — the whole point is
    // that the strip follows the size mode, not just its items.
    let rail = find_widget_type(&t, root, "DockActivityBar").expect("a rail renders");
    let w_default = t.bounds(rail).width;
    let items_default = t
        .bounds(find_first_role(&t, root, Role::TabList).expect("a tab list renders"))
        .width;
    assert!(
        w_default > items_default,
        "the strip is wider than its item column (padding): {w_default} > {items_default}"
    );

    // Compact must make the whole strip narrower — not just its items.
    model.set_side_rail_size(DockSide::Leading, DockRailItemSize::Compact);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let rail = find_widget_type(&t, root, "DockActivityBar").expect("the rail still renders");
    let w_compact = t.bounds(rail).width;

    assert!(
        w_compact < w_default,
        "the activity bar itself shrinks in Compact ({w_compact} < {w_default})"
    );
}

#[test]
fn rail_with_divider_builds_and_lays_out() {
    use super::DockRail;

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(DockRail::new(DockSide::Leading).divider()),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // The rail still renders normally with the divider overlay attached.
    let rail = find_first_role(&t, root, Role::TabList).expect("a rail renders");
    let b = t.bounds(rail);
    assert!(b.width > 0.0 && b.height > 0.0);
}

#[test]
fn rail_slot_resizes_with_the_activity_bar_size() {
    use super::{DockRail, DockRailItemSize};
    use crate::icon_button::{IconButton, IconButtonSize};
    use crate::primitives::IconWidget;
    use std::cell::Cell;
    use std::rc::Rc;

    // The slot factory derives its size from the rail's mode signal and records
    // it each time it runs (the rail rebuilds its slots on a mode change).
    let seen: Rc<Cell<Option<IconButtonSize>>> = Rc::new(Cell::new(None));
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let a_id = DockWidgetId::fresh();
    let dwa = DockWidget::new(a_id, lit!("Explorer"), |_| FixedLeaf(120.0, 120.0));
    let mut t = tree();
    let seen_in = seen.clone();
    let rail_mode = model.rail_size_mode_signal(DockSide::Leading);
    t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(
                DockRail::new(DockSide::Leading)
                    .size(IconButtonSize::Large)
                    .bottom_slot(move || {
                        let size = if rail_mode.get() == DockRailItemSize::Compact {
                            IconButtonSize::Compact
                        } else {
                            IconButtonSize::Large
                        };
                        seen_in.set(Some(size));
                        IconButton::new(IconWidget::checkmark(16.0)).size(size)
                    }),
            ),
    );
    model.open_dock(a_id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Default mode → the rail's configured Large size reaches the slot.
    assert_eq!(
        seen.get(),
        Some(IconButtonSize::Large),
        "slot gets the rail size"
    );

    // Switching to Compact rebuilds the rail and re-hands the slot the new size.
    model.set_side_rail_size(DockSide::Leading, DockRailItemSize::Compact);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert_eq!(
        seen.get(),
        Some(IconButtonSize::Compact),
        "the slot adapts to the new activity-bar size"
    );
}

#[test]
fn labeled_rail_mode_grows_the_item_for_a_rotated_title() {
    use super::DockRailItemSize;
    use crate::primitives::IconWidget;

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0); // Rail presentation
    let a_id = DockWidgetId::fresh();
    let dwa = DockWidget::new(a_id, lit!("Explorer"), |_| FixedLeaf(120.0, 120.0))
        .icon(|| IconWidget::checkmark(18.0));
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    model.open_dock(a_id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Default (icon-only) rail item: roughly the icon square.
    let item = find_first_role(&t, root, Role::Tab).expect("a rail item renders");
    let h_default = t.bounds(item).height;

    // Icon + 90° label: the item must grow taller to seat the rotated title
    // beneath the icon.
    model.set_side_rail_size(DockSide::Leading, DockRailItemSize::Labeled);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let item = find_first_role(&t, root, Role::Tab).expect("the rail item still renders");
    let h_labeled = t.bounds(item).height;

    assert!(
        h_labeled > h_default,
        "labeled rail item ({h_labeled}) must be taller than the icon-only item ({h_default})"
    );
}

#[test]
fn right_clicking_a_rail_item_opens_a_context_menu() {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0); // Rail presentation
    let (a, dwa) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // A rail item is a Role::Tab; right-click its centre.
    let item = find_first_role(&t, root, Role::Tab).expect("a rail item renders");
    let b = t.bounds(item);
    let centre = Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);

    assert!(t.active_overlays().is_empty(), "no menu before the click");
    t.dispatch_event(WidgetEvent::pointer_down(
        centre,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    assert_eq!(
        t.active_overlays().len(),
        1,
        "right-clicking a rail item opens its context menu"
    );
}

#[test]
fn context_menu_is_only_on_tabs_not_on_pane_content() {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let model = DockingModel::new();
    let (a, dwa) = dock("Terminal");
    let (b, dwb) = dock("Problems");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let tab = find_first_role(&t, root, Role::Tab).expect("a strip tab renders");
    let tb = t.bounds(tab);

    // Right-click the pane *content* (well below the tab strip): no menu — the
    // panes / accordions / dock content must NOT carry the context menu.
    let content = Point::new(tb.x + tb.width / 2.0, tb.y + tb.height + 60.0);
    t.dispatch_event(WidgetEvent::pointer_down(
        content,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    assert!(
        t.active_overlays().is_empty(),
        "right-clicking pane content must not open a context menu"
    );

    // Right-click the tab header itself: the menu opens.
    let tab_centre = Point::new(tb.x + tb.width / 2.0, tb.y + tb.height / 2.0);
    t.dispatch_event(WidgetEvent::pointer_down(
        tab_centre,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    assert_eq!(
        t.active_overlays().len(),
        1,
        "right-clicking a tab header opens its context menu"
    );
}

#[test]
fn dragging_a_rail_item_reorders_within_the_side() {
    // An internal move (like a TabWidget reorder): drag one of the leading
    // rail's activity items past the other and the side's tab order flips.
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0); // Rail presentation
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    // Two activities (one dock each) on the leading rail.
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(
        model.side_tabs(DockSide::Leading)[0].panes[0],
        a,
        "A is first"
    );

    // Drag A's rail item down past B → A relocates to the end of the side.
    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item");
    let b_item = find_role_name(&t, root, Role::Tab, "Bbb").expect("B rail item");
    let ab = t.bounds(a_item);
    let bb = t.bounds(b_item);
    let from = Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0);
    let to = Point::new(ab.x + ab.width / 2.0, bb.y + bb.height + 10.0);
    t.drag(from, to);
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(
        model.side_tabs(DockSide::Leading)[0].panes[0],
        b,
        "dragging A's rail item past B reordered the side (B is now first)"
    );
}

/// The dock pane's five split/stack zones are the reusable `DropTarget`'s, not a
/// hand-computed set of fifths — which is what makes them inherit the per-axis
/// floor the drop target applies to a small pane (`tests/composite_touch.rs`
/// measures the floor itself).
///
/// The pin is structural on purpose: if the five-zone overlay were ever
/// hand-rolled again, the floor would silently stop applying and every geometry
/// test of it would stay green.
#[test]
fn a_dock_panes_drop_zones_come_from_the_shared_drop_target() {
    let model = DockingModel::new();
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    // A second dock in the same tab makes each one a split pane, which is the
    // shape that carries the zones.
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    fn has_drop_target(tree: &WidgetTree, id: WidgetId) -> bool {
        if tree
            .widget_type_name(id)
            .is_some_and(|n| n.contains("DropTarget"))
        {
            return true;
        }
        tree.children(id).iter().any(|&c| has_drop_target(tree, c))
    }
    assert!(
        has_drop_target(&t, root),
        "a dock pane hosts the shared DropTarget",
    );
}

/// A finger's tap on a rail item selects its activity and shows the side, on the
/// release — the same single activation path the mouse, the keyboard and an AT
/// `Click` share.
#[test]
fn a_finger_taps_a_rail_item_to_show_its_side() {
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Tap the activity that is *not* current: tapping the current one is the
    // collapse toggle, which is a different assertion.
    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item");
    let ab = t.bounds(a_item);
    let at = Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0);

    let selected_before = model.side_selected_tab_signal(DockSide::Leading).get();
    assert_ne!(selected_before, 0, "fixture: A is not the current activity");
    let f = t.new_contact();
    t.touch_down(f, at);
    assert_eq!(
        model.side_selected_tab_signal(DockSide::Leading).get(),
        selected_before,
        "the press alone selects nothing",
    );
    t.touch_up(f, at);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert_ne!(
        model.side_selected_tab_signal(DockSide::Leading).get(),
        selected_before,
        "the release selects the activity the finger lifted on",
    );
    assert!(model.side_visible_signal(DockSide::Leading).get());
}

/// And a finger that travels instead of lifting reorders the rail: the drag is
/// the rail item's, not the side's, so nothing had to be declared for a contact
/// to reach it.
#[test]
fn a_finger_reorders_the_rail_by_dragging_an_item() {
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert_eq!(model.side_tabs(DockSide::Leading)[0].panes[0], a);

    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item");
    let b_item = find_role_name(&t, root, Role::Tab, "Bbb").expect("B rail item");
    let ab = t.bounds(a_item);
    let bb = t.bounds(b_item);
    let from = Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0);
    let to = Point::new(from.x, bb.y + bb.height + 10.0);

    let f = t.new_contact();
    t.touch_down(f, from);
    for step in 1..=8 {
        let y = from.y + (to.y - from.y) * step as f32 / 8.0;
        t.touch_move(f, Point::new(from.x, y));
    }
    t.touch_up(f, to);
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(
        model.side_tabs(DockSide::Leading)[0].panes[0],
        b,
        "a finger dragging A past B reordered the side",
    );
}

#[test]
fn dropping_a_tab_on_another_sides_rail_moves_it() {
    // An external activity (like a TabWidget `accept_external_tabs`): drag the
    // leading rail's item and drop it on the *trailing* rail — the whole tab
    // relocates to the trailing side.
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    model.set_side_rail(DockSide::Trailing, 48.0);
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Trailing));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(model.dock_location(a).unwrap().side, DockSide::Leading);

    // Drag A's leading rail item onto the trailing rail (drop on its B item).
    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item");
    let b_item = find_role_name(&t, root, Role::Tab, "Bbb").expect("B rail item");
    let ab = t.bounds(a_item);
    let bb = t.bounds(b_item);
    let from = Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0);
    let to = Point::new(bb.x + bb.width / 2.0, bb.y + bb.height / 2.0);
    t.drag(from, to);
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(
        model.dock_location(a).unwrap().side,
        DockSide::Trailing,
        "dropping A on the trailing rail moved its activity to the trailing side"
    );
    assert!(
        !model.is_side_visible(DockSide::Leading),
        "the emptied leading side hides"
    );
}

#[test]
fn dragging_a_rail_item_onto_another_sides_tab_bar_moves_it() {
    // A rail activity (carrying `DockTabDragData`) dropped on another side's
    // *tab strip*. The strip's TabWidget bar is the drop target and only speaks
    // its own `TabBarDragData<TabHandle>` natively, so without the dock's
    // `on_external_drop` wiring the drag would be silently rejected.
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0); // Leading = rail
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let (c, dwc) = dock("Ccc");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb)
            .dock(dwc),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading)); // rail item
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom)); // strip
    model.open_dock(c, DockOpenLocation::side(DockSide::Bottom).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(model.dock_location(a).unwrap().side, DockSide::Leading);

    // Drag A's leading rail item onto the bottom side's tab strip (B's header).
    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item");
    let b_tab = find_role_name(&t, root, Role::Tab, "Bbb").expect("B strip tab");
    let ab = t.bounds(a_item);
    let bb = t.bounds(b_tab);
    t.drag(
        Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0),
        Point::new(bb.x + bb.width / 2.0, bb.y + bb.height / 2.0),
    );
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(
        model.dock_location(a).unwrap().side,
        DockSide::Bottom,
        "the rail activity moved onto the bottom side's tab bar"
    );
    assert_eq!(model.tab_count(DockSide::Bottom), 3);
}

#[test]
fn import_a_tab_to_the_rail_then_reorder_it_inside_the_rail() {
    // Repro: import an activity from another side onto a rail, THEN drag it to a
    // new position inside that same rail. The second (internal) move must not
    // crash (it did: an unbounded selection-sync feedback loop).
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    model.set_side_rail(DockSide::Trailing, 48.0);
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let (c, dwc) = dock("Ccc");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb)
            .dock(dwc),
    );
    // Trailing already has two activities (B, C); A starts on Leading.
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Trailing));
    model.open_dock(c, DockOpenLocation::side(DockSide::Trailing).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Import A onto the trailing rail (drop on its B item).
    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item");
    let b_item = find_role_name(&t, root, Role::Tab, "Bbb").expect("B rail item");
    let ab = t.bounds(a_item);
    let bb = t.bounds(b_item);
    t.drag(
        Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0),
        Point::new(bb.x + bb.width / 2.0, bb.y + bb.height / 2.0),
    );
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert_eq!(model.dock_location(a).unwrap().side, DockSide::Trailing);

    // Now reorder A *inside* the trailing rail: drag it to the end.
    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item on trailing");
    let c_item = find_role_name(&t, root, Role::Tab, "Ccc").expect("C rail item");
    let ab = t.bounds(a_item);
    let cb = t.bounds(c_item);
    t.drag(
        Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0),
        Point::new(cb.x + cb.width / 2.0, cb.y + cb.height + 10.0),
    );
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // No crash; A is still on the trailing side.
    assert_eq!(model.dock_location(a).unwrap().side, DockSide::Trailing);
}

#[test]
fn import_a_tab_to_a_strip_then_reorder_it_does_not_loop() {
    // The same latent selection-sync feedback loop reached the strip path too
    // (shared `DockSidePanel` effects): import an activity onto a strip side,
    // then reorder it within that side's tab strip.
    let model = DockingModel::new();
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let (c, dwc) = dock("Ccc");
    let mut t = tree();
    // Drives the model directly (the loop was in selection sync), so the tree
    // root id isn't needed.
    let _root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb)
            .dock(dwc),
    );
    // Bottom is a Strip side with two tabs; A starts on Leading.
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom));
    model.open_dock(c, DockOpenLocation::side(DockSide::Bottom).new_tab());
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Move A's whole tab onto Bottom (programmatic — same `move_tab` the strip
    // drop uses), then reorder it within Bottom. Neither must loop.
    let a_tab = model.dock_location(a).unwrap();
    let a_tab_id = model.side_tabs(a_tab.side)[a_tab.tab_idx].id;
    model.move_tab(a_tab_id, DockSide::Bottom, 0);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    model.move_tab(a_tab_id, DockSide::Bottom, 2);
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(model.dock_location(a).unwrap().side, DockSide::Bottom);
    assert_eq!(model.tab_count(DockSide::Bottom), 3);
}

#[test]
fn hamburger_restores_activities_when_all_hidden_in_strip() {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let model = DockingModel::new();
    let (a, dwa) = dock("Terminal");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Bottom)); // Strip
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Hide the only activity → no visible tabs (the dead-end the hamburger
    // exists to escape).
    let tab = model.side_tabs(DockSide::Bottom)[0].id;
    model.set_tab_hidden(tab, true);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert_eq!(model.side_visible_tab_count(DockSide::Bottom), 0);
    assert_eq!(count_role(&t, root, Role::Tab), 0, "no tab headers remain");

    // The strip still renders a trailing hamburger button; tapping it opens the
    // activities menu, from which the activity can be re-checked.
    let hb =
        find_first_role(&t, root, Role::Button).expect("a hamburger renders in the empty strip");
    let b = t.bounds(hb);
    let c = Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
    assert!(t.active_overlays().is_empty());
    t.dispatch_event(WidgetEvent::pointer_down(
        c,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    t.dispatch_event(WidgetEvent::pointer_up(
        c,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    assert_eq!(
        t.active_overlays().len(),
        1,
        "the hamburger opens the activities/restore menu"
    );
}

// ── Lock-down policy + per-side disable ────────────────────────────────────

#[test]
fn disabling_a_rail_side_builds_no_rail_and_rejects_docks() {
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    model.set_side_enabled(DockSide::Leading, false);
    let (id, dw) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dw),
    );

    // Placement to a disabled side is rejected.
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(!model.is_dock_open(id), "a disabled side rejects placement");
    assert!(
        !subtree_has_role(&t, root, Role::TabList),
        "no activity rail is built for a disabled side"
    );
    let center = t.children(root)[0];
    assert!(
        (t.bounds(center).width - 1000.0).abs() < 1.0,
        "the centre reclaims the disabled side's space (got x={})",
        t.bounds(center).width
    );

    // Re-enable → the rail returns and the dock can be placed.
    model.set_side_enabled(DockSide::Leading, true);
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(model.is_dock_open(id), "a re-enabled side accepts the dock");
    assert_eq!(model.dock_location(id).unwrap().side, DockSide::Leading);
    assert!(
        subtree_has_role(&t, root, Role::TabList),
        "the rail returns when the side is re-enabled"
    );
}

#[test]
fn locking_activity_drag_freezes_rail_reorder() {
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    model.set_policy(DockPolicy {
        allow_activity_drag: false,
        ..Default::default()
    });
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(
        model.side_tabs(DockSide::Leading)[0].panes[0],
        a,
        "A is first"
    );

    // Drag A's rail item past B — with activity drag locked the rail item is not
    // a drag source, so nothing happens.
    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item");
    let b_item = find_role_name(&t, root, Role::Tab, "Bbb").expect("B rail item");
    let ab = t.bounds(a_item);
    let bb = t.bounds(b_item);
    t.drag(
        Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0),
        Point::new(ab.x + ab.width / 2.0, bb.y + bb.height + 10.0),
    );
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert_eq!(
        model.side_tabs(DockSide::Leading)[0].panes[0],
        a,
        "locked activity drag: the rail order is unchanged"
    );
}

/// Drag dock A's split-pane accordion header onto the leading rail and return
/// the side it ends up on. With dock drag allowed it is promoted to the rail;
/// locked, the header is no drag handle so it stays put.
fn dock_drag_lands_on(locked: bool) -> DockSide {
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Trailing, 48.0);
    if locked {
        model.set_policy(DockPolicy {
            allow_dock_drag: false,
            ..Default::default()
        });
    }
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let (c, dwc) = dock("Ccc");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb)
            .dock(dwc),
    );
    // A + B stacked on the leading side → a *vertical* split: each pane is an
    // accordion whose header is a strip across the top (so the drag start point
    // below lands on the header, not the content). C lives on the trailing rail
    // so the rail is laid out at full thickness as the drop target.
    model.open_dock(c, DockOpenLocation::side(DockSide::Trailing));
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let header = find_role_name(&t, root, Role::Button, "Aaa").expect("A accordion header");
    let rail =
        find_role_name(&t, root, Role::TabList, "Right activity bar").expect("the trailing rail");
    let hb = t.bounds(header);
    let rb = t.bounds(rail);
    // `rb` is now the rail's item column (`DockRailTabList`), not the whole
    // strip — the drop handler lives on the strip, so assert the point we aim
    // at really is inside it before relying on the drop landing there.
    let strip = t.bounds(find_widget_type(&t, root, "DockActivityBar").expect("a rail strip"));
    let drop_point = Point::new(rb.x + rb.width / 2.0, rb.y + rb.height / 2.0);
    assert!(
        drop_point.x >= strip.x
            && drop_point.x <= strip.x + strip.width
            && drop_point.y >= strip.y
            && drop_point.y <= strip.y + strip.height,
        "the drop point must land inside the rail strip {strip:?}, got {drop_point:?}"
    );
    t.drag(Point::new(hb.x + hb.width / 2.0, hb.y + 6.0), drop_point);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    model.dock_location(a).unwrap().side
}

#[test]
fn locking_dock_drag_freezes_the_split_pane_header() {
    assert_eq!(
        dock_drag_lands_on(false),
        DockSide::Trailing,
        "dock drag allowed: the header drag promotes A onto the trailing rail"
    );
    assert_eq!(
        dock_drag_lands_on(true),
        DockSide::Leading,
        "dock drag locked: the header is no drag handle, A stays on leading"
    );
}

#[test]
fn locking_side_collapse_keeps_the_side_but_api_still_hides() {
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    model.set_policy(DockPolicy {
        allow_side_collapse: false,
        ..Default::default()
    });
    let (a, dwa) = dock("Aaa");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(model.is_side_visible(DockSide::Leading));

    // Clicking the active rail item normally hides the side — locked, it stays.
    let a_item = find_role_name(&t, root, Role::Tab, "Aaa").expect("A rail item");
    t.click(a_item);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(
        model.is_side_visible(DockSide::Leading),
        "locked side collapse: clicking the active rail item does not hide the side"
    );

    // The programmatic API is NOT gated (scope = user affordances only).
    model.set_side_visible(DockSide::Leading, false);
    assert!(
        !model.is_side_visible(DockSide::Leading),
        "the programmatic set_side_visible still hides a collapse-locked side"
    );
}

#[test]
fn programmatic_api_bypasses_a_fully_locked_layout() {
    let model = DockingModel::new();
    model.set_policy(DockPolicy::locked());
    let (a, dwa) = dock("Aaa");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let tab = model.side_tabs(DockSide::Bottom)[0].id;
    // Hiding an activity is locked for the USER, but the app may still do it.
    model.set_tab_hidden(tab, true);
    assert!(
        model.is_tab_hidden(tab),
        "set_tab_hidden works programmatically even under DockPolicy::locked()"
    );
}

#[test]
fn dock_context_menus_respect_the_policy() {
    use super::context_menu::{DockMenuKind, activity_context_menu};

    let model = DockingModel::new();
    let (a, dwa) = dock("Aaa");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let tab_id = model.side_tabs(DockSide::Bottom)[0].id;

    // Default policy: both "Hide" and "Move to" rows are present.
    let menu = activity_context_menu(&model, DockSide::Bottom, tab_id, DockMenuKind::Strip);
    let m1 = t.add(menu);
    t.layout(SizeProposal::exact(400.0, 600.0));
    assert!(
        find_named(&t, m1, "Move to").is_some(),
        "default: 'Move to' is present"
    );
    assert!(
        find_named(&t, m1, "Hide \"Aaa\"").is_some(),
        "default: 'Hide' is present"
    );

    // Locked: both user affordances drop out.
    model.set_policy(DockPolicy::locked());
    let menu = activity_context_menu(&model, DockSide::Bottom, tab_id, DockMenuKind::Strip);
    let m2 = t.add(menu);
    t.layout(SizeProposal::exact(400.0, 600.0));
    assert!(
        find_named(&t, m2, "Move to").is_none(),
        "locked: no 'Move to' (activity drag off)"
    );
    assert!(
        find_named(&t, m2, "Hide \"Aaa\"").is_none(),
        "locked: no 'Hide' (activity hide off)"
    );
}

#[test]
fn setting_the_policy_live_re_gates_the_widgets() {
    // The demo's "Lock Layout" toggle: `set_policy` after mount must rebuild and
    // re-gate. Prove the rail reorder works first, then locks after the flip.
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    // Default policy: dragging A past B reorders.
    let drag_a_past_b = |t: &mut WidgetTree| {
        let a_item = find_role_name(t, root, Role::Tab, "Aaa").expect("A rail item");
        let b_item = find_role_name(t, root, Role::Tab, "Bbb").expect("B rail item");
        let ab = t.bounds(a_item);
        let bb = t.bounds(b_item);
        t.drag(
            Point::new(ab.x + ab.width / 2.0, ab.y + ab.height / 2.0),
            Point::new(ab.x + ab.width / 2.0, bb.y + bb.height + 10.0),
        );
        t.layout(SizeProposal::exact(1000.0, 800.0));
    };
    drag_a_past_b(&mut t);
    assert_eq!(
        model.side_tabs(DockSide::Leading)[0].panes[0],
        b,
        "default: dragging A past B reorders the rail"
    );

    // Lock the layout LIVE — the rebuild must re-gate the rail.
    model.set_policy(DockPolicy::locked());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let before = model.side_tabs(DockSide::Leading)[0].panes[0];
    drag_a_past_b(&mut t);
    assert_eq!(
        model.side_tabs(DockSide::Leading)[0].panes[0],
        before,
        "after set_policy(locked): the live rebuild froze the rail reorder"
    );
}

#[test]
fn disabling_a_side_with_docks_hides_then_restores_them() {
    let model = DockingModel::new();
    let (a, dwa) = dock("Aaa");
    let (b, dwb) = dock("Bbb");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(
        count_role(&t, root, Role::Tab) >= 2,
        "two tabs render initially"
    );

    // Disable the bottom side: its content is gone, but the docks remain in the
    // model.
    model.set_side_enabled(DockSide::Bottom, false);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert_eq!(
        count_role(&t, root, Role::Tab),
        0,
        "a disabled side renders no tabs"
    );
    assert_eq!(
        model.tab_count(DockSide::Bottom),
        2,
        "docks stay in the model"
    );
    assert!(model.is_dock_open(a) && model.is_dock_open(b));

    // Re-enable → the docks reappear.
    model.set_side_enabled(DockSide::Bottom, true);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(
        count_role(&t, root, Role::Tab) >= 2,
        "re-enabling the side restores its tabs"
    );
}

// ─── Activity-rail accessibility + keyboard ───────────────────────────────

/// A Leading side in Rail presentation with two separate tabs (two rail
/// items). Returns the tree, the model, and the layout root.
fn rail_two_tabs() -> (WidgetTree, DockingModel, WidgetId) {
    let model = DockingModel::new();
    let (a, dwa) = dock("Explorer");
    let (b, dwb) = dock("Search");
    model.set_side_rail(DockSide::Leading, 48.0);
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    (t, model, root)
}

fn collect_role(t: &WidgetTree, id: WidgetId, role: Role, out: &mut Vec<WidgetId>) {
    if t.accessibility_node(id).role() == role {
        out.push(id);
    }
    for c in t.children(id) {
        collect_role(t, c, role, out);
    }
}

/// The rail's `Role::Tab` items, in visible (top-to-bottom) order.
fn rail_tabs(t: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    let tablist = find_first_role(t, root, Role::TabList).expect("a rail TabList");
    let mut out = Vec::new();
    collect_role(t, tablist, Role::Tab, &mut out);
    out
}

fn find_a11y_node(
    update: &accesskit::TreeUpdate,
    id: accesskit::NodeId,
) -> Option<&accesskit::Node> {
    update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == id)
        .map(|(_, n)| n)
}

#[test]
fn rail_tabs_advertise_click_and_focus_actions() {
    let (t, _model, root) = rail_two_tabs();
    let tabs = rail_tabs(&t, root);
    assert_eq!(tabs.len(), 2, "two docks → two rail tabs");
    for &tab in &tabs {
        let node = t.accessibility_node(tab);
        assert!(
            node.actions().contains(&accesskit::Action::Click),
            "a rail tab must advertise Click (else AT cannot activate it)"
        );
        assert!(
            node.actions().contains(&accesskit::Action::Focus),
            "a rail tab must advertise Focus"
        );
    }
    let selected = tabs
        .iter()
        .filter(|&&id| t.accessibility_node(id).is_selected())
        .count();
    assert_eq!(selected, 1, "exactly one rail tab reports selected");
}

#[test]
fn rail_tabs_report_position_and_size_of_set() {
    let (mut t, _model, root) = rail_two_tabs();
    let tabs = rail_tabs(&t, root);
    let update = t.sync_accessibility();
    // Asked the way an adapter asks it: the position off the tab, the size by
    // walking up to the `Role::TabList` wrapper that holds the rail's items.
    for (i, &tab) in tabs.iter().enumerate() {
        crate::a11y_set_semantics::assert_announces(
            &update,
            widget_id_to_node_id(tab),
            i + 1,
            2,
            &format!("rail tab {i}"),
        );
    }
}

#[test]
fn rail_selected_tab_reports_expanded_matching_visibility() {
    let (mut t, model, root) = rail_two_tabs();
    let selected_idx = model.side_selected_tab_signal(DockSide::Leading).get();
    let tabs = rail_tabs(&t, root);
    let sel = widget_id_to_node_id(tabs[selected_idx]);
    let other_idx = (0..tabs.len()).find(|&i| i != selected_idx).unwrap();
    let other = widget_id_to_node_id(tabs[other_idx]);

    // Side visible → the selected tab is expanded; the other tab omits the
    // state entirely ("expanded" doesn't apply to an inactive tab).
    let update = t.sync_accessibility();
    assert_eq!(
        find_a11y_node(&update, sel).unwrap().is_expanded(),
        Some(true),
        "the selected rail tab is expanded while its side is shown"
    );
    assert_eq!(
        find_a11y_node(&update, other).unwrap().is_expanded(),
        None,
        "a non-selected rail tab omits the expanded state"
    );

    // Hide the side → the (still-selected) tab reports collapsed.
    model.set_side_visible(DockSide::Leading, false);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let update = t.sync_accessibility();
    assert_eq!(
        find_a11y_node(&update, sel).unwrap().is_expanded(),
        Some(false),
        "hiding the side collapses the selected tab"
    );
}

#[test]
fn rail_tab_controls_its_side_content_region() {
    let (mut t, model, root) = rail_two_tabs();
    let tabs = rail_tabs(&t, root);
    let tab0 = widget_id_to_node_id(tabs[0]);
    let update = t.sync_accessibility();
    let node = find_a11y_node(&update, tab0).expect("rail tab node");
    let controlled = node.controls();
    assert!(
        !controlled.is_empty(),
        "a rail tab declares a controls relationship to its content region"
    );
    // The controlled node is the side's Role::Complementary content region,
    // and it is actually present in the tree (no dangling relationship).
    let emitted: std::collections::HashSet<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    assert!(controlled.iter().all(|t| emitted.contains(t)));
    let target = find_a11y_node(&update, controlled[0]).expect("controlled node present in tree");
    assert_eq!(
        target.role(),
        Role::Complementary,
        "the rail tab controls the side's content landmark"
    );
    accesskit_consumer::Tree::new(update.clone(), false);

    // Hiding the side parks its content dormant — the tab must then drop the
    // controls relation rather than dangle at a pruned node.
    model.set_side_visible(DockSide::Leading, false);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let update = t.sync_accessibility();
    let node = find_a11y_node(&update, tab0).expect("rail tab still present (reopen affordance)");
    assert!(
        node.controls().is_empty(),
        "a rail tab whose side is hidden advertises no (dangling) controls"
    );
    accesskit_consumer::Tree::new(update.clone(), false);
}

#[test]
fn enter_activates_a_focused_rail_tab() {
    let (mut t, model, root) = rail_two_tabs();
    let tabs = rail_tabs(&t, root);
    let selected_idx = model.side_selected_tab_signal(DockSide::Leading).get();
    let other = (0..tabs.len()).find(|&i| i != selected_idx).unwrap();

    // Focus a non-selected tab and activate it by keyboard.
    t.focus(tabs[other]);
    t.press_key(Key::Enter, Modifiers::NONE);

    assert_eq!(
        model.side_selected_tab_signal(DockSide::Leading).get(),
        other,
        "Enter on a focused rail tab selects it"
    );
    assert!(
        model.is_side_visible(DockSide::Leading),
        "activating a rail tab shows the side"
    );

    // Enter on the now-active tab collapses the side (the toggle).
    t.focus(tabs[other]);
    t.press_key(Key::Space, Modifiers::NONE);
    assert!(
        !model.is_side_visible(DockSide::Leading),
        "Space on the active rail tab hides the side (collapse toggle)"
    );
}

#[test]
fn access_action_click_activates_a_rail_tab() {
    let (mut t, model, root) = rail_two_tabs();
    let tabs = rail_tabs(&t, root);
    let selected_idx = model.side_selected_tab_signal(DockSide::Leading).get();
    let other = (0..tabs.len()).find(|&i| i != selected_idx).unwrap();

    t.dispatch_event(WidgetEvent::AccessAction {
        action: accesskit::Action::Click,
        target: Some(tabs[other]),
        target_node: widget_id_to_node_id(tabs[other]),
        data: None,
    });

    assert_eq!(
        model.side_selected_tab_signal(DockSide::Leading).get(),
        other,
        "an AT Click selects the targeted rail tab"
    );
    assert!(model.is_side_visible(DockSide::Leading));
}

#[test]
fn arrow_key_moves_rail_focus_without_changing_selection() {
    let (mut t, model, root) = rail_two_tabs();
    let tabs = rail_tabs(&t, root);
    let before = model.side_selected_tab_signal(DockSide::Leading).get();

    t.focus(tabs[0]);
    t.press_key(Key::ArrowDown, Modifiers::NONE);

    assert_eq!(
        t.focused(),
        Some(tabs[1]),
        "ArrowDown moves keyboard focus to the next rail tab"
    );
    assert_eq!(
        model.side_selected_tab_signal(DockSide::Leading).get(),
        before,
        "arrow navigation is manual-activation: selection does not change"
    );
}

#[test]
fn only_the_selected_rail_tab_is_a_tab_stop() {
    let (mut t, model, root) = rail_two_tabs();
    let tabs = rail_tabs(&t, root);
    let selected_idx = model.side_selected_tab_signal(DockSide::Leading).get();

    // Cycle through the Tab order and record which rail tabs are reachable.
    let mut reached = std::collections::HashSet::new();
    for _ in 0..16 {
        t.press_key(Key::Tab, Modifiers::NONE);
        if let Some(f) = t.focused() {
            reached.insert(f);
        }
    }

    assert!(
        reached.contains(&tabs[selected_idx]),
        "the selected rail tab participates in the Tab cycle"
    );
    for (i, &tab) in tabs.iter().enumerate() {
        if i != selected_idx {
            assert!(
                !reached.contains(&tab),
                "a non-selected rail tab is skipped by Tab (roving tab-index)"
            );
        }
    }
}

#[test]
fn keyboard_input_makes_focus_visible_pointer_input_clears_it() {
    let (mut t, _model, root) = rail_two_tabs();
    let tabs = rail_tabs(&t, root);

    t.focus(tabs[0]);
    t.press_key(Key::ArrowDown, Modifiers::NONE);
    assert!(
        t.focus_visible_signal().get(),
        "keyboard input turns :focus-visible on (drives the rail focus ring)"
    );

    t.pointer_down_button(
        Point::new(5.0, 5.0),
        teksilo_core::event::PointerButton::Primary,
    );
    assert!(
        !t.focus_visible_signal().get(),
        "pointer input turns :focus-visible off"
    );
}

// ─── dock header / options button (obs 1) ──────────────────────────────────

/// Whether any laid-out node in the subtree carries the given AT name.
fn has_named(tree: &WidgetTree, id: WidgetId, name: &str) -> bool {
    let n = tree.accessibility_node(id);
    if n.name() == Some(name) && tree.bounds(id).height > 0.0 {
        return true;
    }
    tree.children(id).iter().any(|&c| has_named(tree, c, name))
}

#[test]
fn bare_dock_shows_options_header_only_when_opted_in() {
    // show_header(true): a sole-pane dock gets a header carrying the `⋮`
    // options button (access label "More actions: <title>").
    let model = DockingModel::new();
    let (id, dw) = dock("Explorer");
    let dw = dw.show_header(true);
    let layout = DockingLayout::new(model.clone())
        .center(FixedLeaf(400.0, 300.0))
        .dock(dw);
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    let mut t = tree();
    let root = t.add(layout);
    t.layout(SizeProposal::exact(900.0, 600.0));
    let options = find_named(&t, root, "More actions: Explorer")
        .expect("show_header(true) ⇒ a bare dock has the ⋮ options button");

    // The census counts this button as the keyboard route to the placements a
    // drag offers, so its reachability is asserted rather than read off the
    // fact that an `IconButton` is focusable by default: it sits inside a dock
    // header — an accordion header wrapped in a `DeadZone` in the split-pane
    // form — and a `tab_stop` suppression anywhere above it would leave the
    // menu pointer-only.
    //
    // The stop is the inner `IconButton`, not this node: the name is carried by
    // the enclosing `PopoverWidget`, which is correctly not a stop of its own.
    assert!(
        !t.tab_stops_within(options).is_empty(),
        "the ⋮ options button offers no Tab stop, so its menu — the only \
         non-drag route to those placements — is reachable by pointer alone"
    );
}

#[test]
fn bare_dock_has_no_options_header_by_default() {
    let model = DockingModel::new();
    let (id, dw) = dock("Explorer"); // show_header defaults off
    let layout = DockingLayout::new(model.clone())
        .center(FixedLeaf(400.0, 300.0))
        .dock(dw);
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    let mut t = tree();
    let root = t.add(layout);
    t.layout(SizeProposal::exact(900.0, 600.0));
    assert!(
        !has_named(&t, root, "More actions: Explorer"),
        "a bare dock is headerless unless show_header(true)"
    );
}

#[test]
fn split_pane_docks_each_get_an_options_button() {
    // Two stacked docks → a split (Accordion per pane), each accordion header
    // carrying its own `⋮` options button.
    let model = DockingModel::new();
    let (id_a, a) = dock("Explorer");
    let (id_b, b) = dock("Search");
    let layout = DockingLayout::new(model.clone())
        .center(FixedLeaf(400.0, 300.0))
        .dock(a)
        .dock(b);
    model.open_dock(id_a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(id_b, DockOpenLocation::side(DockSide::Leading).stack());
    let mut t = tree();
    let root = t.add(layout);
    t.layout(SizeProposal::exact(900.0, 600.0));
    assert!(has_named(&t, root, "More actions: Explorer"));
    assert!(has_named(&t, root, "More actions: Search"));
}

#[test]
fn split_pane_trailing_cluster_stacks_with_the_header_axis() {
    // The dock-header action cluster — the app's `header_actions` (hosted in a
    // `Toolbar`) AND the ⋮ options button — must follow the accordion header's
    // axis: a horizontal row on leading / trailing sides (vertical header), and a
    // *vertical* column on top / bottom sides (rotated vertical header strip).
    // The framework owns the arrangement (the `Toolbar`'s orientation), so two
    // app actions must lay out along the same axis as the ⋮ — they must not stay
    // horizontal inside the narrow vertical strip.
    use crate::primitives::IconWidget;
    use crate::toolbar::{ToolbarAction, ToolbarItem};

    // Returns the centers of (action #1, action #2, ⋮) for a two-pane split dock
    // opened on `side`, the first pane carrying two findable header actions.
    fn centers(side: DockSide) -> [(f32, f32); 3] {
        let model = DockingModel::new();
        let id_a = DockWidgetId::fresh();
        let id_b = DockWidgetId::fresh();
        let a = DockWidget::new(id_a, lit!("Explorer"), |_| FixedLeaf(120.0, 120.0))
            .header_actions(|_| {
                // Icon-only toolbar actions: the label becomes the AT name.
                vec![
                    ToolbarItem::action(ToolbarAction::new(lit!("ActX"), || {
                        IconWidget::chevron_up(12.0)
                    })),
                    ToolbarItem::action(ToolbarAction::new(lit!("ActY"), || {
                        IconWidget::chevron_up(12.0)
                    })),
                ]
            });
        let b = DockWidget::new(id_b, lit!("Search"), |_| FixedLeaf(120.0, 120.0));
        let layout = DockingLayout::new(model.clone())
            .center(FixedLeaf(400.0, 300.0))
            .dock(a)
            .dock(b);
        model.open_dock(id_a, DockOpenLocation::side(side));
        model.open_dock(id_b, DockOpenLocation::side(side).stack());
        let mut t = tree();
        let root = t.add(layout);
        t.layout(SizeProposal::exact(1000.0, 700.0));
        let center = |name: &str| {
            let id = find_named(&t, root, name).unwrap_or_else(|| panic!("{name} laid out"));
            let b = t.bounds(id);
            (b.x + b.width / 2.0, b.y + b.height / 2.0)
        };
        [
            center("ActX"),
            center("ActY"),
            center("More actions: Explorer"),
        ]
    }

    // The three controls must be (near-)collinear along the header axis: leading
    // side → horizontal row (varies in x, ~constant y); top side → vertical
    // column (varies in y, ~constant x). Comparing the *spread* across all three
    // catches the reported bug where the two app actions stayed horizontal.
    let spread = |c: [(f32, f32); 3]| {
        let xs = [c[0].0, c[1].0, c[2].0];
        let ys = [c[0].1, c[1].1, c[2].1];
        let span = |v: [f32; 3]| {
            v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min)
        };
        (span(xs), span(ys))
    };

    let (lx, ly) = spread(centers(DockSide::Leading));
    assert!(
        lx > ly,
        "leading-side cluster is a horizontal row (x-span {lx} > y-span {ly})"
    );

    let (tx, ty) = spread(centers(DockSide::Top));
    assert!(
        ty > tx,
        "top-side cluster (both actions + ⋮) is a vertical column (y-span {ty} > x-span {tx})"
    );
}

// ── Pane drop targets (migrated to the reusable DropTarget) ─────────────────

/// A whole-tab drag dropped anywhere on a pane relocates the tab to the pane's
/// side. The pane's DropTarget accepts only a single dock, so the tab bubbles
/// one level up to `DockPanePane`'s own tab handler (a local bubble) — no per-
/// zone overlay shows, and the tab moves. This is the new mechanism introduced
/// by the migration off the hand-rolled five-zone overlay.
#[test]
fn tab_dropped_on_a_pane_relocates_to_its_side() {
    use super::DockTabId;
    use super::drag::DockTabDragData;
    use super::panel::DockPanePane;
    use crate::primitives::{Expand, HStack};
    use teksilo_core::DragPayload;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::gesture::DragPhase;
    use teksilo_core::widget_builder::HandlerSet;

    let model = DockingModel::new();
    let c = reg(&model, DockSide::Trailing);
    model.open_dock(c, DockOpenLocation::side(DockSide::Trailing));
    assert_eq!(
        model.tab_count(DockSide::Trailing),
        1,
        "C opens on Trailing"
    );
    let tab_id = model.side_tabs(DockSide::Trailing)[0].id;

    #[derive(Debug)]
    struct TabSource(DockTabId);
    impl Widget for TabSource {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let self_id = ctx.self_id();
            let tab = self.0;
            let hs = HandlerSet::new().on_drag(move |phase, ctx| {
                if let DragPhase::Started { .. } = phase {
                    ctx.start_drag(
                        self_id,
                        DragPayload::typed(DockTabDragData {
                            tab_id: tab,
                            source_side: DockSide::Trailing,
                        }),
                    );
                }
            });
            ctx.apply_self_handlers(hs);
            Vec::new()
        }
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            Size::new(100.0, 100.0).into()
        }
    }

    let mut t = tree();
    let inner = t.add(FixedLeaf(100.0, 100.0));
    let pane = t.add(DockPanePane::new(
        DockSide::Leading,
        0,
        0,
        model.clone(),
        inner,
    ));
    let src = t.add(TabSource(tab_id));
    let es = t.add(Expand::new().flex(1.0).child(src));
    let ep = t.add(Expand::new().flex(1.0).child(pane));
    t.add(HStack::new().child(es).child(ep));
    t.layout(SizeProposal::exact(400.0, 200.0));

    let from = t.bounds(src).center();
    let to = t.bounds(pane).center();
    t.drag(from, to);

    assert_eq!(
        model.tab_count(DockSide::Leading),
        1,
        "tab relocated to Leading"
    );
    assert_eq!(model.tab_count(DockSide::Trailing), 0, "tab left Trailing");
}

/// A single-dock drag dropped on a pane's **centre** stacks it into that pane's
/// tab (`Center` → `stack_into_tab`) — validating the DropTarget's per-zone
/// routing reaches the docking model. The dropped dock joins the existing tab
/// (still one tab on the side) rather than creating a new one.
#[test]
fn dock_dropped_on_a_pane_centre_stacks_into_its_tab() {
    use super::drag::DockDragData;
    use super::panel::DockPanePane;
    use crate::primitives::{Expand, HStack};
    use teksilo_core::DragPayload;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::gesture::DragPhase;
    use teksilo_core::widget_builder::HandlerSet;

    let model = DockingModel::new();
    let a = reg(&model, DockSide::Leading);
    let b = reg(&model, DockSide::Leading);
    let d = reg(&model, DockSide::Bottom);
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
    model.open_dock(d, DockOpenLocation::side(DockSide::Bottom));
    assert_eq!(
        model.tab_count(DockSide::Leading),
        1,
        "A+B share one Leading tab"
    );
    assert_eq!(model.tab_count(DockSide::Bottom), 1, "D opens on Bottom");

    #[derive(Debug)]
    struct DockSource(DockWidgetId);
    impl Widget for DockSource {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let self_id = ctx.self_id();
            let dock_id = self.0;
            let hs = HandlerSet::new().on_drag(move |phase, ctx| {
                if let DragPhase::Started { .. } = phase {
                    ctx.start_drag(self_id, DragPayload::typed(DockDragData { dock_id }));
                }
            });
            ctx.apply_self_handlers(hs);
            Vec::new()
        }
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            Size::new(100.0, 100.0).into()
        }
    }

    let mut t = tree();
    let inner = t.add(FixedLeaf(100.0, 100.0));
    let pane = t.add(DockPanePane::new(
        DockSide::Leading,
        0,
        0,
        model.clone(),
        inner,
    ));
    let src = t.add(DockSource(d));
    let es = t.add(Expand::new().flex(1.0).child(src));
    let ep = t.add(Expand::new().flex(1.0).child(pane));
    t.add(HStack::new().child(es).child(ep));
    t.layout(SizeProposal::exact(400.0, 200.0));

    let from = t.bounds(src).center();
    let to = t.bounds(pane).center();
    t.drag(from, to);

    assert_eq!(model.tab_count(DockSide::Bottom), 0, "D left Bottom");
    assert_eq!(
        model.tab_count(DockSide::Leading),
        1,
        "D stacked into Leading's existing tab (a new pane, not a new tab)"
    );
}

/// Regression for the migration's AT-noise: a dock pane's `DropTarget` chrome
/// (the decorative reject-border `RectWidget` and the hint-less
/// `DropRegionOverlay`) must be **hidden** from the accessibility tree — the
/// hand-rolled overlay it replaced was `set_hidden()`. Also confirms the
/// per-zone highlight actually paints at the proportional edge strip.
#[test]
fn pane_droptarget_chrome_is_at_hidden_and_highlights() {
    use super::drag::DockDragData;
    use super::panel::DockPanePane;
    use crate::primitives::{Expand, HStack};
    use teksilo_core::DragPayload;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::event::PointerButton;
    use teksilo_core::gesture::DragPhase;
    use teksilo_core::widget_builder::HandlerSet;

    #[derive(Debug)]
    struct DockSource(DockWidgetId);
    impl Widget for DockSource {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let self_id = ctx.self_id();
            let dock_id = self.0;
            let hs = HandlerSet::new().on_drag(move |phase, ctx| {
                if let DragPhase::Started { .. } = phase {
                    ctx.start_drag(self_id, DragPayload::typed(DockDragData { dock_id }));
                }
            });
            ctx.apply_self_handlers(hs);
            Vec::new()
        }
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            Size::new(100.0, 100.0).into()
        }
    }

    let model = DockingModel::new();
    let mut t = tree();
    let inner = t.add(FixedLeaf(100.0, 100.0));
    let pane = t.add(DockPanePane::new(
        DockSide::Leading,
        0,
        0,
        model.clone(),
        inner,
    ));
    let src = t.add(DockSource(DockWidgetId::fresh()));
    let es = t.add(Expand::new().flex(1.0).child(src));
    let ep = t.add(Expand::new().flex(1.0).child(pane));
    t.add(HStack::new().child(es).child(ep));
    t.layout(SizeProposal::exact(400.0, 300.0));
    t.sync_accessibility();

    // The decorative reject-border + zone overlay must be AT-hidden (the old
    // hand-rolled overlay was `set_hidden()`), so a screen reader doesn't meet
    // empty containers per pane.
    fn count_hidden(t: &WidgetTree, id: WidgetId) -> usize {
        let here = usize::from(t.accessibility_node(id).is_hidden());
        here + t
            .children(id)
            .iter()
            .map(|&c| count_hidden(t, c))
            .sum::<usize>()
    }
    assert!(
        count_hidden(&t, pane) >= 2,
        "the pane's decorative reject-border and zone overlay must be AT-hidden"
    );

    // A dock-widget hover over the leading edge paints a translucent highlight
    // at exactly the proportional (20%) leading strip of the pane.
    let pb = t.bounds(pane);
    let sc = t.bounds(src).center();
    let edge = Point::new(pb.x + pb.width * 0.1, pb.y + pb.height * 0.5);
    t.pointer_down_button(sc, PointerButton::Primary);
    t.pointer_move(Point::new(sc.x + 30.0, sc.y)); // arm the drag
    t.pointer_move(edge); // hover the leading zone
    let frame = t.render();
    let strip_w = pb.width * 0.2;
    let painted = frame.decorations.iter().any(|d| {
        (d.rect[0] - pb.x).abs() < 1.0
            && (d.rect[2] - strip_w).abs() < 1.0
            && d.color[3] > 0.05
            && d.color[3] < 0.95
    });
    t.pointer_up_button(edge, PointerButton::Primary);
    assert!(
        painted,
        "a dock-widget hover must paint a translucent highlight at the leading 20% strip"
    );
}

/// The migration changed `DockPanePane::layout_response` to forward the content's
/// full `LayoutResponse` (incl. `flex`) via the DropTarget, instead of just its
/// `Size`. This proves that change is **inert**: the enclosing Splitter sizes
/// panes from its own model (`stretch`), never from a pane's flex — so pane 0's
/// width is identical whether its content is flexible or rigid, and the slack
/// goes to pane 1 via `stretch`, not to pane 0 via `flex`.
#[test]
fn splitter_pane_size_is_invariant_to_content_flex() {
    use super::panel::DockPanePane;
    use crate::splitter::{PaneDescriptor, Splitter, SplitterModel};
    use teksilo_core::widget::LayoutResponse as LR;
    use teksilo_tokens::Orientation;

    #[derive(Debug)]
    struct FlexLeaf;
    impl Widget for FlexLeaf {
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            LR::flexible(Size::new(50.0, 50.0), 1.0)
        }
    }

    // Pane 0 has stretch 0 (must NOT grow); pane 1 stretch 1 (absorbs slack).
    // Container (600) exceeds the summed sizes (400), so there IS slack to place.
    let make = |flex_content: bool| -> (f32, f32) {
        let model = SplitterModel::from_panes(
            vec![
                PaneDescriptor::new().size(200.0).min_size(0.0).stretch(0.0),
                PaneDescriptor::new().size(200.0).min_size(0.0).stretch(1.0),
            ],
            Orientation::Horizontal,
        );
        let dm = DockingModel::new();
        let mut t = tree();
        let inner0: WidgetId = if flex_content {
            t.add(FlexLeaf)
        } else {
            t.add(FixedLeaf(50.0, 50.0))
        };
        let inner1 = t.add(FixedLeaf(50.0, 50.0));
        let p0 = t.add(DockPanePane::new(
            DockSide::Leading,
            0,
            0,
            dm.clone(),
            inner0,
        ));
        let p1 = t.add(DockPanePane::new(
            DockSide::Leading,
            0,
            1,
            dm.clone(),
            inner1,
        ));
        t.add(Splitter::new(model).pane(p0).pane(p1));
        t.layout(SizeProposal::exact(600.0, 200.0));
        (t.bounds(p0).width, t.bounds(p1).width)
    };

    let (w0_flex, w1_flex) = make(true);
    let (w0_rigid, w1_rigid) = make(false);

    // Flex-invariant: pane 0's width is the same regardless of its content's flex.
    assert!(
        (w0_flex - w0_rigid).abs() < 0.01,
        "pane 0 width must be invariant to content flex (flex={w0_flex}, rigid={w0_rigid})"
    );
    assert!(
        (w1_flex - w1_rigid).abs() < 0.01,
        "pane 1 width must be invariant to content flex (flex={w1_flex}, rigid={w1_rigid})"
    );
    // And the slack went to pane 1 (stretch), not pane 0 (flex): pane 0 stays ~200.
    assert!(
        (w0_flex - 200.0).abs() < 20.0 && w1_flex > w0_flex + 100.0,
        "slack must follow the model's stretch (pane 1), not the content's flex (pane 0): w0={w0_flex}, w1={w1_flex}"
    );
}

// ───────────────────────────────────────────────────────────────────────
// Dockless rail actions (`DockAction`) + Strip bar slots.
// ───────────────────────────────────────────────────────────────────────

/// A rail action that records its activations, so a test can assert the
/// closure really ran (and that a disabled action's did not).
fn counting_action(
    name: &'static str,
    placement: super::DockActionPlacement,
) -> (super::DockAction, std::rc::Rc<std::cell::Cell<usize>>) {
    use super::{DockAction, DockActionId};
    let hits = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let h = hits.clone();
    let action = DockAction::new(
        DockActionId::named(name),
        lit!(name),
        || crate::primitives::IconWidget::dash(16.0),
        move |_ctx| h.set(h.get() + 1),
    )
    .placement(placement);
    (action, hits)
}

#[test]
fn dock_action_id_named_is_stable_and_distinct() {
    use super::DockActionId;
    assert_eq!(
        DockActionId::named("app.settings"),
        DockActionId::named("app.settings"),
        "the same name must hash to the same id across calls (and runs)"
    );
    assert_ne!(
        DockActionId::named("app.settings"),
        DockActionId::named("app.about"),
        "different names must not collide"
    );
    // Usable in a `const` item — the reason it is a `const fn`.
    const SETTINGS: DockActionId = DockActionId::named("app.settings");
    assert_eq!(SETTINGS, DockActionId::named("app.settings"));
}

/// Build a Rail-presentation leading side carrying one action per placement.
fn rail_with_actions() -> (WidgetTree, WidgetId, DockingModel) {
    use super::{DockActionPlacement as P, DockRail};

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(
                DockRail::new(DockSide::Leading)
                    .action(counting_action("start", P::Start).0)
                    .action(counting_action("end", P::End).0)
                    .action(counting_action("pinned", P::Pinned).0),
            ),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    (t, root, model)
}

#[test]
fn rail_actions_render_in_all_three_placements() {
    let (t, root, _m) = rail_with_actions();

    let start = t.bounds(find_named(&t, root, "start").expect("Start action renders"));
    let end = t.bounds(find_named(&t, root, "end").expect("End action renders"));
    let pinned = t.bounds(find_named(&t, root, "pinned").expect("Pinned action renders"));
    let items = t.bounds(find_first_role(&t, root, Role::TabList).expect("the tab list"));

    assert!(
        start.y + start.height <= items.y + 0.01,
        "Start sits above the activity items ({start:?} vs {items:?})"
    );
    assert!(
        end.y >= items.y + items.height - 0.01,
        "End sits below the activity items ({end:?} vs {items:?})"
    );
    assert!(
        pinned.y > end.y,
        "Pinned sits past the spacer, below End ({pinned:?} vs {end:?})"
    );

    // Pinned is anchored to the rail's far edge, not merely "after End": with
    // one activity item there is a lot of slack between the two.
    let strip = t.bounds(find_widget_type(&t, root, "DockActivityBar").expect("the rail strip"));
    assert!(
        pinned.y + pinned.height >= strip.y + strip.height - 16.0,
        "Pinned hugs the bottom of the strip ({pinned:?} vs {strip:?})"
    );
    assert!(
        pinned.y - (end.y + end.height) > 100.0,
        "the spacer really does separate End from Pinned"
    );
}

#[test]
fn rail_action_toolbar_is_a_sibling_of_the_tab_list_not_a_child() {
    let (t, root, _m) = rail_with_actions();

    let tab_list = find_first_role(&t, root, Role::TabList).expect("the tab list");
    assert!(
        !subtree_has_role(&t, tab_list, Role::Toolbar),
        "ARIA forbids a non-tab child of a tablist: the action toolbars must not \
         be inside the tab list"
    );
    assert!(
        !subtree_has_role(&t, tab_list, Role::Button),
        "no plain buttons inside the tablist either"
    );
    // Every rail item under the tab list is a Tab, and all three action groups
    // exist as toolbars elsewhere in the rail.
    assert_eq!(
        count_role(&t, root, Role::Toolbar),
        3,
        "one Role::Toolbar per non-empty placement"
    );
    assert!(count_role(&t, tab_list, Role::Tab) >= 1);
}

#[test]
fn rail_slots_and_overflow_are_not_tab_list_children() {
    use super::DockRail;
    use crate::icon_button::IconButton;

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(
                DockRail::new(DockSide::Leading)
                    .top_slot(|| IconButton::menu().tooltip(lit!("Logo")))
                    .bottom_slot(|| IconButton::menu().tooltip(lit!("Account"))),
            ),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let tab_list = find_first_role(&t, root, Role::TabList).expect("the tab list");
    // Pre-existing ARIA violation, now fixed: the slots used to be ordinary
    // children of the `Role::TabList`-tagged rail root.
    assert!(
        find_named(&t, tab_list, "Logo").is_none(),
        "top_slot must not be a tablist descendant"
    );
    assert!(
        find_named(&t, tab_list, "Account").is_none(),
        "bottom_slot must not be a tablist descendant"
    );
    assert!(
        find_named(&t, root, "Logo").is_some() && find_named(&t, root, "Account").is_some(),
        "…but both must still render (restricting the AT tree would delete them)"
    );
}

#[test]
fn rail_action_activates_on_click_and_stays_inert_when_disabled() {
    use super::{DockAction, DockActionId, DockActionPlacement as P, DockRail};
    use teksilo_core::signal::Signal;

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Explorer");

    let (live, live_hits) = counting_action("live", P::Pinned);
    let off_hits = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let h = off_hits.clone();
    let off = DockAction::new(
        DockActionId::named("off"),
        lit!("off"),
        || crate::primitives::IconWidget::dash(16.0),
        move |_ctx| h.set(h.get() + 1),
    )
    .placement(P::Pinned)
    .enabled(Signal::new(false));

    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(DockRail::new(DockSide::Leading).action(live).action(off)),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    t.click(find_named(&t, root, "live").expect("the enabled action"));
    assert_eq!(live_hits.get(), 1, "an enabled action runs its closure");

    t.click(find_named(&t, root, "off").expect("the disabled action"));
    assert_eq!(off_hits.get(), 0, "a disabled action is inert");

    // …but it stays FOCUSABLE and in the AT tree. The group has exactly one
    // Tab stop (chosen by its roving index); dropping a disabled item out of
    // the focus order could strand that stop and make the whole toolbar
    // keyboard-unreachable — and `enabled` is a live Prop, so that can happen
    // at any time. ARIA's toolbar pattern wants disabled controls discoverable
    // anyway.
    let off_id = find_named(&t, root, "off").expect("the disabled action");
    let node = t.accessibility_node(off_id);
    assert_eq!(node.role(), Role::Button, "still a button in the AT tree");
    // (`AccessibilityInfo::is_disabled` mirrors the *arena* enabled flag, not
    // the widget's own `set_disabled()`, so it stays false here by design —
    // the action is deliberately not `enabled_when`'d out of the arena. The
    // AccessKit node still carries the disabled state; what this test pins is
    // the actionable half, which is what a keyboard/AT user actually hits.)
    assert!(
        !node.actions().contains(&accesskit::Action::Click),
        "a disabled action offers no Click to assistive tech"
    );
    assert!(
        node.actions().contains(&accesskit::Action::Focus),
        "but it remains focusable, so a keyboard user can still discover it"
    );
}

#[test]
fn rail_action_toggled_is_reflect_only() {
    use super::{DockAction, DockActionId, DockActionPlacement as P, DockRail};
    use teksilo_core::signal::Signal;

    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Explorer");
    let state = Signal::new(false);
    let s = state.clone();
    let action = DockAction::new(
        DockActionId::named("toggle"),
        lit!("toggle"),
        || crate::primitives::IconWidget::dash(16.0),
        move |_ctx| { /* deliberately does NOT write `s` */ },
    )
    .placement(P::Pinned)
    .toggled(s.clone());

    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(DockRail::new(DockSide::Leading).action(action)),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let id = find_named(&t, root, "toggle").expect("the toggled action");
    t.click(id);
    assert!(
        !state.get(),
        "the rail must not write the toggled signal — unlike IconButton::toggle, \
         a DockAction's bistate is reflect-only so a derived signal is safe"
    );

    // …and it does reflect an external write.
    state.set(true);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(t.accessibility_node(id).is_toggled());
}

#[test]
fn actions_are_absent_in_strip_presentation() {
    use super::{DockActionPlacement as P, DockRail};

    // Rail-only, by decision: a Strip side renders no actions. Pinned here so
    // the no-op is deliberate and cannot silently reverse.
    let model = DockingModel::new();
    let (a, dwa) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(DockRail::new(DockSide::Leading).action(counting_action("act", P::Pinned).0)),
    );
    // No `set_side_rail` ⇒ Strip presentation.
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert!(
        find_named(&t, root, "act").is_none(),
        "a Strip-presentation side renders no rail actions (mirror them with \
         trailing_slot if the side may flip presentation)"
    );
}

#[test]
fn strip_trailing_slot_composes_with_the_hidden_activities_hamburger() {
    use super::DockRail;
    use crate::icon_button::IconButton;

    // Both the app's trailing slot AND the framework's restore hamburger want
    // `TabWidget::bar_trailing_slot`, which is last-write-wins — so without
    // explicit composition one of them is silently dropped.
    let model = DockingModel::new();
    let (a, dwa) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(
                DockRail::new(DockSide::Leading)
                    .trailing_slot(|| IconButton::menu().tooltip(lit!("App slot"))),
            ),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.set_side_visible(DockSide::Leading, true);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(
        find_named(&t, root, "App slot").is_some(),
        "the app's trailing slot renders on a Strip side"
    );

    // Hide every activity ⇒ the hamburger appears. Both must now be present.
    let tab_id = model.side_tabs(DockSide::Leading)[0].id;
    model.set_tab_hidden(tab_id, true);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(
        find_named(&t, root, "Hidden activities").is_some(),
        "the restore hamburger must survive an app-declared trailing slot"
    );
    assert!(
        find_named(&t, root, "App slot").is_some(),
        "…and the app's slot must survive the hamburger"
    );
}

#[test]
fn strip_slots_render_on_a_side_with_no_docks() {
    use super::DockRail;
    use crate::icon_button::IconButton;

    // A configured-but-empty side is reachable, not misuse. The zero-tabs
    // branch returns before the TabWidget is built, so the slots need their own
    // bar there (the bug Qt's `setCornerWidget` still has).
    let model = DockingModel::new();
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .rail(
                DockRail::new(DockSide::Leading)
                    .leading_slot(|| IconButton::menu().tooltip(lit!("Lead")))
                    .trailing_slot(|| IconButton::menu().tooltip(lit!("Trail"))),
            ),
    );
    model.set_side_visible(DockSide::Leading, true);
    model.set_side_size(DockSide::Leading, 240.0);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    assert!(
        find_named(&t, root, "Lead").is_some(),
        "leading_slot renders even with zero docks"
    );
    assert!(
        find_named(&t, root, "Trail").is_some(),
        "trailing_slot renders even with zero docks"
    );
}

#[test]
#[should_panic(expected = "duplicate DockActionId")]
fn duplicate_action_ids_are_caught_in_debug() {
    use super::{DockActionPlacement as P, DockRail};
    // The id is an action's stable address for AT and the automation bridge;
    // two buttons sharing one is silent (both render) but makes "click the
    // settings action" ambiguous.
    let _ = DockRail::new(DockSide::Leading)
        .action(counting_action("dup", P::Pinned).0)
        .action(counting_action("dup", P::End).0);
}

#[test]
fn rail_composition_order_places_each_cluster_correctly() {
    use super::{DockActionPlacement as P, DockRail};
    use crate::icon_button::IconButton;

    // Assert the rail column's child ORDER directly rather than via painted
    // positions: the ordering that is easiest to get backwards is the End
    // group versus the overflow trigger (both sit between the tab list and the
    // spacer), and the trigger is only *painted* once the rail actually
    // overflows — which depends on a signal written during layout.
    let model = DockingModel::new();
    model.set_side_rail(DockSide::Leading, 48.0);
    let (a, dwa) = dock("Explorer");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dwa)
            .rail(
                DockRail::new(DockSide::Leading)
                    .top_slot(|| IconButton::menu().tooltip(lit!("Logo")))
                    .bottom_slot(|| IconButton::menu().tooltip(lit!("Account")))
                    .overflow_icon(|| crate::primitives::IconWidget::dash(16.0))
                    .action(counting_action("start", P::Start).0)
                    .action(counting_action("end", P::End).0)
                    .action(counting_action("pinned", P::Pinned).0),
            ),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));

    let tab_list = find_first_role(&t, root, Role::TabList).expect("the tab list");
    let column = t.parent(tab_list).expect("the rail's item column");
    let kids = t.children(column);
    let type_at = |i: usize| t.widget_type_name(kids[i]).unwrap_or("");
    let index_of = |needle: &str| {
        (0..kids.len())
            .find(|&i| type_at(i).contains(needle))
            .unwrap_or_else(|| panic!("no `{needle}` among the rail column's children"))
    };

    let i_tab_list = index_of("DockRailTabList");
    // The trigger is a `PopoverWidget<IconButton>`, not the `PopoverIconButton`
    // builder's own name.
    let i_trigger = index_of("PopoverWidget");
    let i_spacer = index_of("Spacer");
    let groups: Vec<usize> = (0..kids.len())
        .filter(|&i| type_at(i).contains("DockRailActionGroup"))
        .collect();
    assert_eq!(groups.len(), 3, "one group per non-empty placement");
    let (i_start, i_end, i_pinned) = (groups[0], groups[1], groups[2]);

    assert!(i_start < i_tab_list, "Start precedes the activity items");
    assert!(
        i_tab_list < i_trigger,
        "the overflow trigger follows the activity items"
    );
    assert!(
        i_trigger < i_end,
        "the End group follows the overflow trigger — both sit between the tab \
         list and the spacer, and this is the ordering most easily inverted"
    );
    assert!(
        i_end < i_spacer,
        "End is before the spacer (it flows with the tabs)"
    );
    assert!(
        i_spacer < i_pinned,
        "Pinned is past the spacer — anchored to the rail's far edge"
    );
    assert_eq!(
        i_pinned,
        kids.len() - 2,
        "only bottom_slot may follow the Pinned cluster"
    );
}

// ── Resize-handle keyboard ─────────────────────────────────────────

/// Build a layout with one dock on `side`, shown and sized, and hand back the
/// tree plus the focusable resize handle.
fn side_with_handle(side: DockSide, size: f32) -> (WidgetTree, DockingModel, WidgetId) {
    let model = DockingModel::new();
    let (id, dw) = dock("Panel");
    let mut t = tree();
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(side));
    model.set_side_size(side, size);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    // Let the side finish animating open so the handle has real bounds.
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    // One dock means no inner pane splitter, so the only `Role::Splitter` in
    // the tree is the side's own resize handle.
    let handle = find_first_role(&t, root, Role::Splitter).expect("a resize handle");
    (t, model, handle)
}

#[test]
fn a_trailing_side_grows_toward_the_leading_edge() {
    // A trailing side's handle sits on its *leading* edge, so growing the side
    // moves the handle left. The pointer path has always inverted per side
    // (`side_main`); the keyboard mapped `ArrowRight` to "bigger" for every
    // horizontal-axis side, so the handle moved the way the arrow did not
    // point.
    let (mut t, model, handle) = side_with_handle(DockSide::Trailing, 240.0);
    t.focus(handle);

    let before = model.side_size(DockSide::Trailing);
    t.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert!(
        model.side_size(DockSide::Trailing) > before,
        "ArrowLeft moves a trailing handle left, which makes the side wider"
    );

    let mid = model.side_size(DockSide::Trailing);
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert!(
        model.side_size(DockSide::Trailing) < mid,
        "and back the other way"
    );
}

#[test]
fn a_bottom_side_grows_upward() {
    let (mut t, model, handle) = side_with_handle(DockSide::Bottom, 200.0);
    t.focus(handle);

    let before = model.side_size(DockSide::Bottom);
    t.press_key(Key::ArrowUp, Modifiers::NONE);
    assert!(
        model.side_size(DockSide::Bottom) > before,
        "ArrowUp moves a bottom handle up, which makes the side taller"
    );
}

#[test]
fn a_leading_side_still_grows_with_the_trailing_arrow() {
    // The regression guard for the side that was already right.
    let (mut t, model, handle) = side_with_handle(DockSide::Leading, 240.0);
    t.focus(handle);

    let before = model.side_size(DockSide::Leading);
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert!(model.side_size(DockSide::Leading) > before);
}

#[test]
fn home_hides_the_side() {
    // A dock side's low extreme *is* hidden — the meaning preserved through
    // the move onto the shared chord table.
    //
    // The `End` half is deliberately not asserted here: hiding the side takes
    // its resize handle with it, so once `Home` has fired there is no focused
    // handle left to receive `End`. Revealing a hidden side is the activity
    // rail's job, or `set_side_visible` from outside — the same asymmetry the
    // pre-existing code had, and one this change does not alter.
    let (mut t, model, handle) = side_with_handle(DockSide::Leading, 240.0);
    t.focus(handle);

    t.press_key(Key::Home, Modifiers::NONE);
    assert!(!model.is_side_visible(DockSide::Leading));
}

#[test]
fn a_modified_enter_does_not_hide_a_dock_side() {
    // `Enter` was matched before any modifier test, so `Ctrl+Enter` hid the
    // side and reported the key handled — the one row of
    // `docs/range-keyboard.md` ("`Ctrl` / `Alt` / `Super` + any → falls
    // through") the handle did not honour, while every other key on it did.
    let (mut t, model, handle) = side_with_handle(DockSide::Leading, 240.0);
    t.focus(handle);

    for mods in [Modifiers::CTRL, Modifiers::ALT, Modifiers::SUPER] {
        t.press_key(Key::Enter, mods);
        assert!(
            model.is_side_visible(DockSide::Leading),
            "Enter with {mods:?} must fall through to the application"
        );
    }

    // Bare `Enter` still toggles, which is what makes the guard a refusal
    // rather than a removal.
    t.press_key(Key::Enter, Modifiers::NONE);
    assert!(!model.is_side_visible(DockSide::Leading));
}

#[test]
fn an_accelerator_chord_does_not_resize_a_dock_side() {
    // Behaviour change: modifiers used to be ignored outright.
    let (mut t, model, handle) = side_with_handle(DockSide::Leading, 240.0);
    t.focus(handle);

    let before = model.side_size(DockSide::Leading);
    for (key, mods) in [
        (Key::ArrowRight, Modifiers::CTRL),
        (Key::Home, Modifiers::ALT),
        (Key::End, Modifiers::SUPER),
    ] {
        t.press_key(key, mods);
        assert_eq!(
            model.side_size(DockSide::Leading),
            before,
            "{key:?} with {mods:?} must fall through"
        );
        assert!(model.is_side_visible(DockSide::Leading));
    }
}

// ── Accumulator plurals ─────────────────────────────────────────────────────
//
// `rails` and `docks` are folds over `rail` and `dock`, so N singular calls and
// one plural call over the same values must leave the same layout behind.
// Neither accumulator reaches the arena at build time (rails only render in Rail
// presentation, dock content only when a dock is opened), so these read the
// layout's own state rather than the tree.

#[test]
fn rails_plural_matches_the_singular_chain() {
    use super::activity_bar::DockRail;

    let sides = [DockSide::Leading, DockSide::Trailing, DockSide::Bottom];
    let singular = DockingLayout::new(DockingModel::new())
        .rail(DockRail::new(sides[0]))
        .rail(DockRail::new(sides[1]))
        .rail(DockRail::new(sides[2]));
    let plural = DockingLayout::new(DockingModel::new()).rails(sides.map(DockRail::new));

    let configured = |l: &DockingLayout| {
        let mut v: Vec<String> = l.rails.keys().map(|s| format!("{s:?}")).collect();
        v.sort();
        v
    };
    assert_eq!(configured(&singular).len(), 3, "the fixture lost a rail");
    assert_eq!(configured(&singular), configured(&plural));
}

#[test]
fn docks_plural_matches_the_singular_chain() {
    let a = DockWidgetId::fresh();
    let b = DockWidgetId::fresh();
    let make = |id| DockWidget::new(id, lit!("Panel"), |_| FixedLeaf(120.0, 120.0));

    let singular = DockingLayout::new(DockingModel::new())
        .dock(make(a))
        .dock(make(b));
    let plural = DockingLayout::new(DockingModel::new()).docks([make(a), make(b)]);

    // Registered means the content factory answers for that id.
    let registered = |l: &DockingLayout| {
        [a, b]
            .map(|id| l.registry.borrow().build(id).is_some())
            .to_vec()
    };
    assert_eq!(registered(&singular), vec![true, true]);
    assert_eq!(registered(&singular), registered(&plural));
}

/// N22: a centre holding a `Splitter` too small for its panes' minimums
/// keeps every pane inside the centre, so nothing in it lands on (or paints
/// over) the trailing side, and a press in the trailing side reaches the
/// side's own control.
/// The centre's clipping frame. It sits inside the centre's Tab region,
/// the layout's first child.
fn centre_frame(t: &WidgetTree, root: WidgetId) -> WidgetId {
    t.children(t.children(root)[0])[0]
}

#[test]
fn a_too_small_splitter_in_the_centre_stays_off_the_trailing_side() {
    use crate::button::Button;
    use crate::splitter::{PaneDescriptor, Splitter, SplitterModel};
    use std::rc::Rc;
    use teksilo_tokens::Orientation;

    let split = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new()
                .size(500.0)
                .min_size(500.0)
                .stretch(1.0),
            PaneDescriptor::new()
                .size(500.0)
                .min_size(500.0)
                .stretch(1.0),
        ],
        Orientation::Horizontal,
    );
    let model = DockingModel::new();
    let (lead, dwl) = dock("Lead");
    let trail = DockWidgetId::fresh();
    let dwt = DockWidget::new(trail, lit!("Trail"), |_| Button::new(lit!("Inside")));
    let mut t = tree().with_text_backend(Rc::new(std::cell::RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(
                Splitter::new(split)
                    .pane(FixedLeaf(100.0, 100.0))
                    .pane(FixedLeaf(100.0, 100.0)),
            )
            .dock(dwl)
            .dock(dwt),
    );
    model.open_dock(lead, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(trail, DockOpenLocation::side(DockSide::Trailing));
    let window = SizeProposal::exact(1000.0, 800.0);
    t.layout(window);
    t.tick_animations(Duration::from_millis(600));
    t.layout(window);

    let center = centre_frame(&t, root);
    let cb = t.bounds(center);
    assert!(
        cb.width < 1000.0,
        "the centre is narrower than the splitter's floors"
    );
    let button = find_role_name(&t, root, Role::Button, "Inside").expect("trailing button");
    let bb = t.bounds(button);
    // The centre's clip node, grown past the centre by the focus-ring
    // margin, holds the content at the centre rectangle.
    let clip = t.children(center)[0];
    let mut stack = t.children(clip);
    while let Some(id) = stack.pop() {
        let b = t.bounds(id);
        let overlaps = b.x < bb.x + bb.width
            && bb.x < b.x + b.width
            && b.y < bb.y + bb.height
            && bb.y < b.y + b.height
            && b.width > 0.0
            && b.height > 0.0;
        assert!(
            !overlaps,
            "centre descendant {id:?} at {b:?} overlaps the trailing control at {bb:?}"
        );
        assert!(
            b.x + b.width <= cb.x + cb.width + 0.5,
            "centre descendant {id:?} at {b:?} ends past the centre at {cb:?}"
        );
        stack.extend(t.children(id));
    }
    assert!(
        t.widget_clips_children(clip),
        "the centre clips its content"
    );
    let content = t.children(clip)[0];
    let inner = t.bounds(content);
    assert!(
        (inner.x - cb.x).abs() < 0.01
            && (inner.y - cb.y).abs() < 0.01
            && (inner.width - cb.width).abs() < 0.01
            && (inner.height - cb.height).abs() < 0.01,
        "the content fills the centre exactly: {inner:?} vs {cb:?}"
    );

    // The hit pin: a press over the trailing control reaches it.
    let hit = t.hit_test(bb.center()).expect("a hit");
    let mut at = Some(hit);
    while at.is_some_and(|id| id != button) {
        at = t.parent(at.unwrap());
    }
    assert_eq!(at, Some(button), "the press reaches the trailing control");
}

/// The centre's clip reaches past its edge by the theme's focus-ring margin,
/// so the ring a control flush with the centre's edge paints outside itself
/// (the IntUI, Fluent and macOS presets all do) is not cut. The second theme
/// carries Fluent's ring geometry (1 dp off the control, 2 dp wide).
#[test]
fn the_centre_clip_leaves_room_for_a_focus_ring() {
    let intui = teksilo_core::presets::intui::light();
    let mut fluent_like = teksilo_core::presets::intui::light();
    fluent_like.shape.focus_ring_offset = 1.0;
    fluent_like.shape.focus_ring_width = 2.0;
    for theme in [intui, fluent_like] {
        let margin = super::centre::focus_ring_margin(&theme.shape);
        assert!(
            margin >= theme.shape.focus_ring_envelope() + theme.shape.focus_ring_width,
            "the margin covers the ring and a halo one ring width outside it"
        );
        let model = DockingModel::new();
        let (id, dw) = dock("Explorer");
        let mut t = WidgetTree::new().with_theme(theme);
        let root = t.add(
            DockingLayout::new(model.clone())
                .center(FixedLeaf(200.0, 200.0))
                .dock(dw),
        );
        model.open_dock(id, DockOpenLocation::side(DockSide::Trailing));
        t.layout(SizeProposal::exact(1000.0, 800.0));
        t.tick_animations(Duration::from_millis(600));
        t.layout(SizeProposal::exact(1000.0, 800.0));

        let center = centre_frame(&t, root);
        let cb = t.bounds(center);
        let clip = t.children(center)[0];
        let expected = teksilo_canvas::Rect::new(
            cb.x - margin,
            cb.y - margin,
            cb.width + 2.0 * margin,
            cb.height + 2.0 * margin,
        );
        assert_eq!(t.bounds(clip), expected);
        let frame = t.render();
        assert!(
            frame.draw_order.iter().any(|cmd| matches!(
                cmd,
                teksilo_canvas::DrawCommand::SetClip(r) if *r == expected
            )),
            "the centre's clip is {expected:?}, past its bounds {cb:?}"
        );
        assert!(
            !frame.draw_order.iter().any(|cmd| matches!(
                cmd,
                teksilo_canvas::DrawCommand::SetClip(r) if *r == cb
            )),
            "nothing clips the centre at its own bounds"
        );
        // A press in the band past the centre's edge reaches nothing in it.
        let band = teksilo_canvas::Point::new(cb.x + cb.width + margin * 0.5, cb.y + 10.0);
        let content = t.children(clip)[0];
        let mut at = t.hit_test(band);
        while let Some(id) = at {
            assert!(id != content && id != clip, "the band is not the centre's");
            at = t.parent(id);
        }
    }
}

/// The clip's margin follows a theme set at runtime: the next layout grows
/// the clip by the new theme's focus-ring margin.
#[test]
fn the_centre_clip_follows_a_theme_change() {
    let model = DockingModel::new();
    let mut t = tree();
    let root = t.add(DockingLayout::new(model).center(FixedLeaf(200.0, 200.0)));
    let window = SizeProposal::exact(1000.0, 800.0);
    t.layout(window);
    let center = centre_frame(&t, root);
    let clip = t.children(center)[0];
    let grown_by = |t: &WidgetTree| t.bounds(center).x - t.bounds(clip).x;

    let before = teksilo_core::presets::intui::light();
    let first = super::centre::focus_ring_margin(&before.shape);
    assert!((grown_by(&t) - first).abs() < 0.01);

    let mut wider = before.clone();
    wider.shape.focus_ring_width += 4.0;
    let second = super::centre::focus_ring_margin(&wider.shape);
    assert!(second > first);
    t.set_theme(wider);
    t.layout(window);
    assert!(
        (grown_by(&t) - second).abs() < 0.01,
        "the clip grows by the new margin {second}, got {}",
        grown_by(&t)
    );
    let cb = t.bounds(center);
    let frame = t.render();
    assert!(frame.draw_order.iter().any(|cmd| matches!(
        cmd,
        teksilo_canvas::DrawCommand::SetClip(r)
            if (r.x - (cb.x - second)).abs() < 0.01 && (r.width - (cb.width + 2.0 * second)).abs() < 0.01
    )));
}

// ── focus_dock ────────────────────────────────────────────────────────

/// A focusable field named `name`, as a dock's content would hold.
#[derive(Debug)]
struct FocusField(&'static str);
impl Widget for FocusField {
    fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(100.0, 24.0).into()
    }
    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        builder.set_role(Role::TextInput);
        builder.set_name(self.0);
    }
}

/// A dock whose content is two focusable fields, `<title> 1` and `<title> 2`.
fn field_dock(
    title: &'static str,
    first: &'static str,
    second: &'static str,
) -> (DockWidgetId, DockWidget) {
    use teksilo_core::widget_builder::WidgetBuilder;
    let id = DockWidgetId::fresh();
    (
        id,
        DockWidget::new(id, lit!(title), move |_| {
            crate::primitives::VStack::new()
                .child(FocusField(first).focusable(true))
                .child(FocusField(second).focusable(true))
        }),
    )
}

/// Lay out, run the post-mount actions (where a focus request is applied,
/// as the app loop runs them each iteration), and hear the result.
fn settle(t: &mut WidgetTree) {
    for _ in 0..2 {
        t.layout(SizeProposal::exact(1000.0, 800.0));
        t.tick_animations(Duration::from_millis(600));
        t.layout(SizeProposal::exact(1000.0, 800.0));
        t.run_mount_actions(&mut teksilo_core::window::NoopWindowOps);
        t.layout(SizeProposal::exact(1000.0, 800.0));
        let _ = t.sync_accessibility();
    }
}

fn heard_mark(t: &WidgetTree) -> u64 {
    t.announcements_since(0).last().map_or(0, |a| a.seq)
}

fn heard_since(t: &WidgetTree, mark: u64) -> Vec<String> {
    t.announcements_since(mark)
        .into_iter()
        .map(|a| a.text)
        .collect()
}

fn focused_name(t: &WidgetTree) -> Option<String> {
    let id = t.focused()?;
    t.accessibility_node(id).name().map(str::to_owned)
}

/// A centre with a focusable field, the place a user works in.
fn centre_field() -> impl Widget {
    use teksilo_core::widget_builder::WidgetBuilder;
    FocusField("Editor").focusable(true)
}

#[test]
fn focus_dock_reveals_a_hidden_side_focuses_its_first_field_and_says_its_title() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.set_side_visible(DockSide::Leading, false);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    let mark = heard_mark(&t);

    model.focus_dock(a);
    settle(&mut t);

    assert!(model.is_side_visible(DockSide::Leading));
    assert_eq!(focused_name(&t).as_deref(), Some("Explorer 1"));
    assert_eq!(heard_since(&t, mark), vec!["Explorer".to_string()]);
}

#[test]
fn focus_dock_on_a_shown_dock_moves_focus_without_rebuilding_anything() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    let version = model.version().get();
    let mark = heard_mark(&t);

    model.focus_dock(a);
    settle(&mut t);

    assert_eq!(model.version().get(), version, "nothing was rebuilt");
    assert_eq!(focused_name(&t).as_deref(), Some("Explorer 1"));
    assert_eq!(heard_since(&t, mark), vec!["Explorer".to_string()]);
}

#[test]
fn reveal_dock_on_a_shown_dock_still_moves_nothing_and_says_nothing() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    let version = model.version().get();
    let mark = heard_mark(&t);

    model.reveal_dock(a);
    settle(&mut t);

    assert_eq!(model.version().get(), version);
    assert_eq!(t.focused(), Some(editor));
    assert!(heard_since(&t, mark).is_empty());
}

#[test]
fn focus_dock_returns_to_the_control_the_dock_last_had() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let second = t.find_by_label("Explorer 2").expect("the second field");
    t.focus(second);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    let mark = heard_mark(&t);

    model.focus_dock(a);
    settle(&mut t);
    assert_eq!(t.focused(), Some(second));
    assert_eq!(heard_since(&t, mark), vec!["Explorer".to_string()]);

    // Already inside: focus stays, and the title is still said.
    let mark = heard_mark(&t);
    model.focus_dock(a);
    settle(&mut t);
    assert_eq!(t.focused(), Some(second));
    assert_eq!(heard_since(&t, mark), vec!["Explorer".to_string()]);
}

#[test]
fn focus_dock_on_a_dock_with_nothing_focusable_lands_on_its_tab_and_says_nothing_more() {
    let model = DockingModel::new();
    let (a, dwa) = dock("Outline");
    let (b, dwb) = dock("Search");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    let mark = heard_mark(&t);

    model.focus_dock(a);
    settle(&mut t);

    assert_eq!(
        focused_name(&t).as_deref(),
        Some("Outline"),
        "focus is on the dock's tab, named by its title"
    );
    assert!(
        heard_since(&t, mark).is_empty(),
        "the tab already says the title"
    );
}

#[test]
fn focus_dock_on_a_second_pane_focuses_that_pane() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let (b, dwb) = field_dock("Outline", "Outline 1", "Outline 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
    settle(&mut t);

    model.focus_dock(b);
    settle(&mut t);
    assert_eq!(focused_name(&t).as_deref(), Some("Outline 1"));
}

#[test]
fn focus_dock_expands_a_collapsed_pane_and_shows_a_hidden_activity() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let (b, dwb) = field_dock("Outline", "Outline 1", "Outline 2");
    let (c, dwc) = field_dock("Search", "Search 1", "Search 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa)
            .dock(dwb)
            .dock(dwc),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
    model.open_dock(c, DockOpenLocation::side(DockSide::Bottom));
    settle(&mut t);

    // Collapse the Outline pane.
    let tab = model.activity_of(b).expect("Outline's tab");
    let (_, view) = model.tab_view_by_id(tab).expect("its view");
    view.splitter.set_collapsed(1, true);
    settle(&mut t);
    model.focus_dock(b);
    settle(&mut t);
    assert!(!view.splitter.is_collapsed(1), "the pane was expanded");
    assert_eq!(focused_name(&t).as_deref(), Some("Outline 1"));

    // Hide the Search activity.
    let search = model.activity_of(c).expect("Search's tab");
    model.set_tab_hidden(search, true);
    settle(&mut t);
    model.focus_dock(c);
    settle(&mut t);
    assert!(!model.is_tab_hidden(search), "the activity is shown again");
    assert_eq!(focused_name(&t).as_deref(), Some("Search 1"));
}

#[test]
fn focus_dock_does_nothing_for_an_unknown_dock_or_a_disabled_side() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.set_side_enabled(DockSide::Leading, false);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    let mark = heard_mark(&t);

    model.focus_dock(a);
    model.focus_dock(DockWidgetId::fresh());
    settle(&mut t);

    assert_eq!(t.focused(), Some(editor));
    assert!(heard_since(&t, mark).is_empty());
}

#[test]
fn of_two_focus_requests_in_one_turn_the_later_wins() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let (b, dwb) = field_dock("Problems", "Problems 1", "Problems 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa)
            .dock(dwb),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom));
    settle(&mut t);
    let mark = heard_mark(&t);

    model.focus_dock(a);
    model.focus_dock(b);
    settle(&mut t);

    assert_eq!(focused_name(&t).as_deref(), Some("Problems 1"));
    assert_eq!(heard_since(&t, mark), vec!["Problems".to_string()]);
}

/// Focus inside a side that the reveal rebuilds used to fall to a rail item
/// (the rebuild's focus restore picked the same index among what survived).
#[test]
fn focus_dock_from_inside_a_side_the_reveal_rebuilds_lands_in_the_dock() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let (b, dwb) = field_dock("Problems", "Problems 1", "Problems 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa)
            .dock(dwb),
    );
    model.set_side_rail(DockSide::Leading, 40.0);
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Bottom));
    model.set_side_visible(DockSide::Bottom, false);
    settle(&mut t);
    let explorer = t
        .find_by_label("Explorer 2")
        .expect("a field in the leading side");
    t.focus(explorer);
    settle(&mut t);

    model.focus_dock(b);
    settle(&mut t);
    assert_eq!(focused_name(&t).as_deref(), Some("Problems 1"));
}

#[test]
fn a_focus_request_after_a_key_press_shows_the_focus_ring() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    t.press_key(Key::F6, Modifiers::NONE);
    model.focus_dock(a);
    settle(&mut t);
    assert_eq!(focused_name(&t).as_deref(), Some("Explorer 1"));
    assert!(t.focus_visible_signal().get(), "a chord shows the ring");
}

// ── Tab order: top, leading, centre, trailing, bottom ─────────────────

/// A dock whose content is one button labelled `label`.
fn button_dock(title: &'static str, label: &'static str) -> (DockWidgetId, DockWidget) {
    let id = DockWidgetId::fresh();
    (
        id,
        DockWidget::new(id, lit!(title), move |_| crate::Button::new(lit!(label))),
    )
}

/// A window `[Title, DockingLayout, Status]` with one button dock on each
/// side, a button in the centre and a rail on the leading side.
fn five_regions(
    trailing_content: Option<Box<dyn Fn(DockWidgetId) -> Box<dyn Widget>>>,
) -> (WidgetTree, DockingModel, WidgetId, WidgetId) {
    use crate::primitives::{FixedSize, VStack};
    let model = DockingModel::new();
    let (top, top_w) = button_dock("TopDock", "Top button");
    let (lead, lead_w) = button_dock("LeadDock", "Leading button");
    let (trail, trail_w) = match trailing_content {
        Some(f) => {
            let id = DockWidgetId::fresh();
            (id, DockWidget::new(id, lit!("TrailDock"), f))
        }
        None => button_dock("TrailDock", "Trailing button"),
    };
    let (bottom, bottom_w) = button_dock("BottomDock", "Bottom button");
    let mut t = tree();
    let dock = DockingLayout::new(model.clone())
        .center(crate::Button::new(lit!("Centre button")))
        .dock(top_w)
        .dock(lead_w)
        .dock(trail_w)
        .dock(bottom_w);
    let dock_id = t.add(dock);
    let root = t.add(
        VStack::new()
            .child(crate::Button::new(lit!("Title")))
            .child(FixedSize::new().width(1000.0).height(700.0).child(dock_id))
            .child(crate::Button::new(lit!("Status"))),
    );
    model.set_side_rail(DockSide::Leading, 40.0);
    model.open_dock(top, DockOpenLocation::side(DockSide::Top));
    model.open_dock(lead, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(trail, DockOpenLocation::side(DockSide::Trailing));
    model.open_dock(bottom, DockOpenLocation::side(DockSide::Bottom));
    settle_tab(&mut t);
    (t, model, root, dock_id)
}

fn settle_tab(t: &mut WidgetTree) {
    for _ in 0..2 {
        t.layout(SizeProposal::exact(1000.0, 800.0));
        t.tick_animations(Duration::from_millis(600));
        t.layout(SizeProposal::exact(1000.0, 800.0));
    }
}

/// What Tab (or Shift+Tab) reaches from `start`, by name or role, until it
/// comes back to `start`.
fn tab_walk(t: &mut WidgetTree, start: WidgetId, back: bool) -> Vec<String> {
    t.focus(start);
    settle_tab(t);
    let mut out = Vec::new();
    for _ in 0..64 {
        let f = t.focused().expect("focus stays somewhere");
        let node = t.accessibility_node(f);
        out.push(
            node.name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{:?}", node.role())),
        );
        let mods = if back {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        t.press_key(Key::Tab, mods);
        settle_tab(t);
        if t.focused() == Some(start) {
            break;
        }
    }
    out
}

fn position(walk: &[String], name: &str) -> usize {
    walk.iter()
        .position(|n| n == name)
        .unwrap_or_else(|| panic!("{name} is not reached by Tab: {walk:?}"))
}

#[test]
fn tab_reads_the_dock_top_leading_centre_trailing_bottom_within_the_window() {
    let (mut t, _model, _root, _dock) = five_regions(None);
    let title = t.find_by_label("Title").expect("the title button");
    let walk = tab_walk(&mut t, title, false);
    let order: Vec<usize> = [
        "Title",
        "Top button",
        "Leading button",
        "Centre button",
        "Trailing button",
        "Bottom button",
        "Status",
    ]
    .iter()
    .map(|n| position(&walk, n))
    .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "reading order, the dock between the window's own controls: {walk:?}"
    );
    // Inside the leading side: its rail (a tab), its content, its handle.
    let lead = position(&walk, "Leading button");
    let centre = position(&walk, "Centre button");
    let top = position(&walk, "Top button");
    assert_eq!(
        walk[lead - 1],
        "LeadDock",
        "the rail item comes before the panel: {walk:?}"
    );
    assert!(lead - 1 > top);
    assert!(
        walk[lead + 1..centre].iter().any(|n| n == "Splitter"),
        "the handle comes after the panel: {walk:?}"
    );

    // Shift+Tab walks it back.
    let back = tab_walk(&mut t, title, true);
    let rev: Vec<usize> = [
        "Title",
        "Status",
        "Bottom button",
        "Trailing button",
        "Centre button",
        "Leading button",
        "Top button",
    ]
    .iter()
    .map(|n| position(&back, n))
    .collect();
    assert!(rev.windows(2).all(|w| w[0] < w[1]), "{back:?}");
}

#[test]
fn right_to_left_reads_the_same_logical_order_from_the_right() {
    let (mut t, _model, _root, _dock) = five_regions(None);
    t.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    settle_tab(&mut t);
    let leading = t.find_by_label("Leading button").expect("leading");
    let trailing = t.find_by_label("Trailing button").expect("trailing");
    assert!(
        t.bounds(leading).x > t.bounds(trailing).x,
        "the leading side is on the right"
    );
    let title = t.find_by_label("Title").expect("the title button");
    let walk = tab_walk(&mut t, title, false);
    assert!(position(&walk, "Leading button") < position(&walk, "Centre button"));
    assert!(position(&walk, "Centre button") < position(&walk, "Trailing button"));
}

#[test]
fn a_tab_index_inside_a_dock_orders_only_within_its_region() {
    use teksilo_core::widget_builder::WidgetBuilder;
    let (mut t, _model, _root, _dock) = five_regions(Some(Box::new(|_| {
        Box::new(crate::Button::new(lit!("Trailing button")).tab_index(1)) as Box<dyn Widget>
    })));
    let title = t.find_by_label("Title").expect("the title button");
    let walk = tab_walk(&mut t, title, false);
    assert!(position(&walk, "Title") < position(&walk, "Centre button"));
    assert!(
        position(&walk, "Centre button") < position(&walk, "Trailing button"),
        "{walk:?}"
    );
}

#[test]
fn a_rebuild_keeps_the_centre_and_the_order() {
    let (mut t, model, _root, _dock) = five_regions(None);
    let centre = t.find_by_label("Centre button").expect("centre");
    let version = model.version().get();
    // A structural change: the bottom activity moves to the top side.
    let tab = model
        .tab_id_at(DockSide::Bottom, 0)
        .expect("the bottom tab");
    model.move_tab(tab, DockSide::Top, 1);
    settle_tab(&mut t);
    assert_ne!(model.version().get(), version, "the layout was rebuilt");
    assert_eq!(
        t.find_by_label("Centre button"),
        Some(centre),
        "the centre survives"
    );
    let title = t.find_by_label("Title").expect("the title button");
    let walk = tab_walk(&mut t, title, false);
    assert!(position(&walk, "Leading button") < position(&walk, "Centre button"));
    assert!(position(&walk, "Centre button") < position(&walk, "Trailing button"));
}

#[test]
fn a_collapsed_side_keeps_only_its_rail_and_a_hidden_band_nothing() {
    let (mut t, model, _root, dock) = five_regions(None);
    model.set_side_visible(DockSide::Leading, false);
    model.set_side_visible(DockSide::Bottom, false);
    settle_tab(&mut t);
    let stops: Vec<String> = t
        .tab_stops_within(dock)
        .into_iter()
        .map(|f| {
            let n = t.accessibility_node(f);
            n.name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{:?}", n.role()))
        })
        .collect();
    assert!(
        stops.iter().any(|n| n == "LeadDock"),
        "the rail stays: {stops:?}"
    );
    assert!(!stops.iter().any(|n| n == "Leading button"), "{stops:?}");
    assert!(!stops.iter().any(|n| n == "Bottom button"), "{stops:?}");
    let regions = t.children(dock);
    let bottom_region = regions[1 + 3];
    assert_eq!(
        t.bounds(bottom_region).width * t.bounds(bottom_region).height,
        0.0
    );
}

#[test]
fn the_landmarks_are_in_reading_order_and_the_regions_are_no_nodes() {
    let (mut t, _model, _root, dock) = five_regions(None);
    let update = t.sync_accessibility();
    let nid = |w| widget_id_to_node_id(w);
    // Depth-first reading order through the update.
    let index: std::collections::HashMap<_, _> =
        update.nodes.iter().map(|(id, n)| (*id, n)).collect();
    let mut order = Vec::new();
    let mut stack = vec![update.tree.as_ref().unwrap().root];
    while let Some(id) = stack.pop() {
        let Some(n) = index.get(&id) else { continue };
        if n.role() == Role::Complementary {
            order.push(id);
        }
        stack.extend(n.children().iter().rev().copied());
    }
    let side_of = |landmark: accesskit::NodeId| {
        let names = [
            "Top button",
            "Leading button",
            "Trailing button",
            "Bottom button",
        ];
        names
            .iter()
            .position(|name| {
                let b = t.find_by_label(name).unwrap();
                let mut cur = Some(b);
                while let Some(c) = cur {
                    if nid(c) == landmark {
                        return true;
                    }
                    cur = t.parent(c);
                }
                false
            })
            .unwrap()
    };
    let sides: Vec<usize> = order.iter().map(|&l| side_of(l)).collect();
    assert_eq!(sides, vec![0, 1, 2, 3], "top, leading, trailing, bottom");
    for region in t.children(dock) {
        assert!(!index.contains_key(&nid(region)), "a region is no AT node");
        assert!(!t.tab_stops_within(dock).contains(&region));
    }
    let first = t.first_focusable_descendant(dock).expect("a control");
    let top_region = t.children(dock)[1 + 2];
    let mut cur = Some(first);
    let mut in_top = false;
    while let Some(c) = cur {
        if c == top_region {
            in_top = true;
        }
        cur = t.parent(c);
    }
    assert!(in_top, "the dock's first control is now in the top panel");
}

// ── Side names: where the side is, translated, or the application's ──────

fn named_sides(t: &mut WidgetTree, model: DockingModel, layout: DockingLayout) -> WidgetId {
    let (a, dwa) = dock("Explorer");
    let (b, dwb) = dock("Outline");
    let dwa = dwa.show_header(true);
    let root = t.add(layout.center(FixedLeaf(200.0, 200.0)).dock(dwa).dock(dwb));
    model.set_side_rail(DockSide::Leading, 40.0);
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.open_dock(b, DockOpenLocation::side(DockSide::Trailing));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    root
}

#[test]
fn the_sides_are_named_in_french_for_where_they_are() {
    use crate::common::locale_switch_test::speaking;
    let (_mgr, mut t) = speaking("fr-FR");
    let model = DockingModel::new();
    let root = named_sides(&mut t, model.clone(), DockingLayout::new(model.clone()));
    assert!(find_role_name(&t, root, Role::Complementary, "Panneau de gauche").is_some());
    assert!(find_role_name(&t, root, Role::Complementary, "Panneau de droite").is_some());
    assert!(find_role_name(&t, root, Role::TabList, "Barre d'activité de gauche").is_some());
    let options = find_named(&t, root, "Plus d'actions : Explorer").expect("the options button");
    assert_eq!(t.accessibility_node(options).role(), Role::Button);
    assert!(
        t.tab_stops_within(root).contains(&options),
        "the named node is the focusable one"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn right_to_left_names_the_leading_side_right_and_draws_it_there() {
    let model = DockingModel::new();
    let mut t = tree();
    let root = named_sides(&mut t, model.clone(), DockingLayout::new(model.clone()));
    let _ = t.sync_accessibility();
    assert!(find_role_name(&t, root, Role::Complementary, "Left panel").is_some());

    // A direction switch alone, with no locale change, renames the sides.
    t.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let update = t.sync_accessibility();
    let landmark = |name: &str| {
        update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == Role::Complementary && n.label() == Some(name))
            .and_then(|(_, n)| n.bounds())
            .unwrap_or_else(|| panic!("no landmark named {name}"))
    };
    let right = landmark("Right panel");
    let left = landmark("Left panel");
    assert!(
        right.x0 > left.x0,
        "the side named right is drawn on the right"
    );
    let _ = root;
}

#[test]
fn a_locale_switch_renames_the_sides() {
    use crate::common::locale_switch_test::speaking;
    let (mgr, mut t) = speaking("en-US");
    let model = DockingModel::new();
    // A dock title that is itself translated, as an application's `tr!` is.
    let word = teksilo_core::signal::Signal::new("Explorer".to_string());
    let title = {
        let word = word.clone();
        teksilo_i18n::localized(move || word.get())
    };
    let id = DockWidgetId::fresh();
    let dw = DockWidget::new(id, title, |_| FixedLeaf(120.0, 120.0)).show_header(true);
    let root = t.add(
        DockingLayout::new(model.clone())
            .center(FixedLeaf(200.0, 200.0))
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    t.tick_animations(Duration::from_millis(600));
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(find_role_name(&t, root, Role::Complementary, "Left panel").is_some());
    assert!(find_named(&t, root, "More actions: Explorer").is_some());
    word.set("Explorateur".to_string());
    mgr.set_locale("fr-FR".parse().unwrap());
    t.set_locale("fr-FR".to_string());
    t.layout(SizeProposal::exact(1000.0, 800.0));
    assert!(find_role_name(&t, root, Role::Complementary, "Panneau de gauche").is_some());
    assert!(
        find_named(&t, root, "Plus d'actions : Explorateur").is_some(),
        "a name composed with the dock title follows the switch, title included"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn an_application_names_a_side() {
    let model = DockingModel::new();
    let mut t = tree();
    let root = named_sides(
        &mut t,
        model.clone(),
        DockingLayout::new(model.clone()).side_label(DockSide::Leading, lit!("Binder")),
    );
    assert!(find_role_name(&t, root, Role::Complementary, "Binder").is_some());
    assert!(find_role_name(&t, root, Role::TabList, "Binder activity bar").is_some());
    assert!(find_role_name(&t, root, Role::Complementary, "Right panel").is_some());
    assert_eq!(
        super::a11y::move_target_label(&model, DockSide::Leading),
        "Binder",
        "the Move to menus offer the side by the application's name"
    );
    assert_eq!(
        super::a11y::move_target_label(&model, DockSide::Bottom),
        "Bottom"
    );
}

// ── focus_dock: the reviewed edges ───────────────────────────────────

#[test]
fn focus_dock_on_a_hidden_side_leaves_focus_within_true_above_the_layout() {
    use teksilo_core::widget_builder::WidgetBuilder;
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let within = teksilo_core::signal::Signal::new(false);
    let mut t = tree();
    let layout = t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    t.add(
        crate::primitives::VStack::new()
            .child(layout)
            .focus_within(within.clone()),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.set_side_visible(DockSide::Leading, false);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    assert!(within.get());

    model.focus_dock(a);
    settle(&mut t);
    assert_eq!(focused_name(&t).as_deref(), Some("Explorer 1"));
    assert!(
        within.get(),
        "focus is inside the layout, so focus_within holds above it"
    );
}

#[test]
fn a_bare_dock_with_a_header_and_no_control_lands_on_the_header_and_says_the_title() {
    let model = DockingModel::new();
    let (a, dwa) = dock("Outline");
    let dwa = dwa.show_header(true);
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    let mark = heard_mark(&t);
    model.focus_dock(a);
    settle(&mut t);
    assert_ne!(t.focused(), Some(editor), "focus moved into the header");
    assert_eq!(
        heard_since(&t, mark),
        vec!["Outline".to_string()],
        "the header's control is not named by the title, so it is said"
    );
}

#[test]
fn with_a_rail_and_nothing_to_focus_focus_lands_on_the_rail() {
    let model = DockingModel::new();
    let (a, dwa) = dock("Outline");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.set_side_rail(DockSide::Leading, 40.0);
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    let mark = heard_mark(&t);
    model.focus_dock(a);
    settle(&mut t);
    let f = t.focused().expect("focus");
    assert_eq!(t.accessibility_node(f).role(), Role::Tab, "the rail item");
    assert!(
        heard_since(&t, mark).is_empty(),
        "the rail item says the title"
    );
}

#[test]
fn a_remembered_control_that_is_now_disabled_is_skipped() {
    let model = DockingModel::new();
    let enabled = teksilo_core::signal::Signal::new(true);
    let id = DockWidgetId::fresh();
    let dw = DockWidget::new(id, lit!("Explorer"), {
        let enabled = enabled.clone();
        move |_| {
            crate::primitives::VStack::new()
                .child(crate::Button::new(lit!("First")))
                .child(crate::Button::new(lit!("Second")).enabled(enabled.clone()))
        }
    });
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let second = t.find_by_label("Second").expect("second");
    t.focus(second);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    enabled.set(false);
    settle(&mut t);
    model.focus_dock(id);
    settle(&mut t);
    assert_eq!(focused_name(&t).as_deref(), Some("First"));
}

#[test]
fn a_focusable_content_root_is_remembered_too() {
    use teksilo_core::widget_builder::WidgetBuilder;
    let model = DockingModel::new();
    let id = DockWidgetId::fresh();
    // A content root that takes focus itself (as a list view does), with a
    // control inside it.
    let dw = DockWidget::new(id, lit!("Explorer"), |_| {
        crate::primitives::VStack::new()
            .child(crate::Button::new(lit!("Inner")))
            .focusable(true)
            .access_label(lit!("Root"))
    });
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let inner = t.find_by_label("Inner").expect("the inner control");
    let root = t.find_by_label("Root").expect("the content root");
    t.focus(inner);
    settle(&mut t);
    t.focus(root);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    model.focus_dock(id);
    settle(&mut t);
    assert_eq!(
        t.focused(),
        Some(root),
        "the root had focus last, not the inner control"
    );
}

#[test]
fn focus_dock_on_a_hidden_side_returns_to_the_control_the_dock_last_had() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let second = t.find_by_label("Explorer 2").expect("second");
    t.focus(second);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    // Hidden, then revealed by `focus_dock`: the reveal rebuilds the side,
    // and the field focus had is a new widget.
    model.set_side_visible(DockSide::Leading, false);
    settle(&mut t);
    let mark = heard_mark(&t);
    model.focus_dock(a);
    settle(&mut t);
    assert_ne!(t.focused(), Some(second), "the side was rebuilt");
    assert_eq!(focused_name(&t).as_deref(), Some("Explorer 2"));
    assert_eq!(heard_since(&t, mark), vec!["Explorer".to_string()]);
}

#[test]
fn after_a_structural_move_the_control_at_the_same_place_is_focused() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let second = t.find_by_label("Explorer 2").expect("second");
    t.focus(second);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    // A structural move destroys the remembered control.
    let tab = model.activity_of(a).expect("its tab");
    model.move_tab(tab, DockSide::Trailing, 0);
    settle(&mut t);
    model.focus_dock(a);
    settle(&mut t);
    assert_eq!(focused_name(&t).as_deref(), Some("Explorer 2"));
}

#[test]
fn a_rebuilt_dock_with_fewer_controls_lands_on_its_first() {
    use teksilo_core::widget_builder::WidgetBuilder;
    let model = DockingModel::new();
    let count = Rc::new(Cell::new(3usize));
    let id = DockWidgetId::fresh();
    let names = ["Field 1", "Field 2", "Field 3"];
    let dw = DockWidget::new(id, lit!("Explorer"), {
        let count = count.clone();
        move |_| {
            names[..count.get()]
                .iter()
                .fold(crate::primitives::VStack::new(), |stack, &name| {
                    stack.child(FocusField(name).focusable(true))
                })
        }
    });
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dw),
    );
    model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
    settle(&mut t);
    let third = t.find_by_label("Field 3").expect("third");
    t.focus(third);
    settle(&mut t);
    let editor = t.find_by_label("Editor").expect("the centre field");
    t.focus(editor);
    settle(&mut t);
    count.set(1);
    model.set_side_visible(DockSide::Leading, false);
    settle(&mut t);
    model.focus_dock(id);
    settle(&mut t);
    assert_eq!(focused_name(&t).as_deref(), Some("Field 1"));
}

#[test]
fn a_request_made_before_the_layout_is_mounted_does_not_fire() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let layout = DockingLayout::new(model.clone())
        .center(centre_field())
        .dock(dwa);
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.focus_dock(a);
    let mut t = tree();
    t.add(layout);
    settle(&mut t);
    assert_ne!(focused_name(&t).as_deref(), Some("Explorer 1"));
}

#[test]
fn a_dock_that_cannot_open_makes_no_request() {
    let model = DockingModel::new();
    let (b, dwb) = field_dock("Problems", "Problems 1", "Problems 2");
    let dwb = dwb.default_location(DockOpenLocation::side(DockSide::Bottom));
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwb),
    );
    model.set_side_enabled(DockSide::Bottom, false);
    settle(&mut t);
    model.focus_dock(b);
    assert!(!model.is_dock_open(b));
    assert!(
        model.pending_focus_request().is_none(),
        "no request waits to fire at some later rebuild"
    );
}

#[test]
fn focus_dock_is_refused_while_a_modal_is_open() {
    use teksilo_core::overlay::{DismissBehavior, OverlayPlacement, OverlayRequest};
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let mut t = tree();
    let layout = t.add(
        DockingLayout::new(model.clone())
            .center(centre_field())
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.set_side_visible(DockSide::Leading, false);
    settle(&mut t);
    let ok = t.add(crate::Button::new(lit!("Dialog OK")));
    let modal = t.add(crate::primitives::VStack::new().child(ok));
    t.show_overlay(OverlayRequest::new(
        modal,
        layout,
        OverlayPlacement::Centered,
        DismissBehavior::Manual,
    ));
    t.focus(ok);
    settle(&mut t);
    let mark = heard_mark(&t);
    model.focus_dock(a);
    settle(&mut t);
    assert_eq!(t.focused(), Some(ok), "focus stays in the modal");
    assert!(heard_since(&t, mark).is_empty());
    // Only the focus move is refused: the dock is revealed behind the modal.
    assert!(model.is_side_visible(DockSide::Leading));
    assert!(t.find_by_label("Explorer 1").is_some());
}

#[test]
fn focus_dock_from_a_button_handler_works() {
    let model = DockingModel::new();
    let (a, dwa) = field_dock("Explorer", "Explorer 1", "Explorer 2");
    let m = model.clone();
    let mut t = tree();
    t.add(
        DockingLayout::new(model.clone())
            .center(
                crate::Button::new(lit!("Show explorer")).on_activate_fn(move |_| m.focus_dock(a)),
            )
            .dock(dwa),
    );
    model.open_dock(a, DockOpenLocation::side(DockSide::Leading));
    model.set_side_visible(DockSide::Leading, false);
    settle(&mut t);
    let button = t.find_by_label("Show explorer").expect("the button");
    t.click(button);
    settle(&mut t);
    assert_eq!(focused_name(&t).as_deref(), Some("Explorer 1"));
}

// ── Tab order inside each side: the order the side is drawn in ───────────

/// The parts of each side in the order Tab reaches them, from a full walk.
fn side_part_orders(t: &mut WidgetTree, dock: WidgetId) -> Vec<(DockSide, Vec<&'static str>)> {
    let title = t.find_by_label("Title").expect("the title button");
    t.focus(title);
    settle_tab(t);
    let mut walk = Vec::new();
    for _ in 0..64 {
        let f = t.focused().expect("focus");
        walk.push(f);
        t.press_key(Key::Tab, Modifiers::NONE);
        settle_tab(t);
        if t.focused() == Some(title) {
            break;
        }
    }
    let regions = t.children(dock);
    let sides = [
        DockSide::Leading,
        DockSide::Trailing,
        DockSide::Top,
        DockSide::Bottom,
    ];
    sides
        .iter()
        .enumerate()
        .map(|(i, &side)| {
            let parts = t.children(regions[1 + i]);
            let names = ["rail", "content", "handle"];
            let mut seen: Vec<&'static str> = Vec::new();
            for &f in &walk {
                if let Some(k) = parts.iter().position(|&p| t.is_descendant_of(f, p)) {
                    if seen.last() != Some(&names[k]) {
                        seen.push(names[k]);
                    }
                }
            }
            (side, seen)
        })
        .collect()
}

fn rails_everywhere() -> (WidgetTree, DockingModel, WidgetId) {
    let (mut t, model, _root, dock) = five_regions(None);
    for side in [
        DockSide::Leading,
        DockSide::Trailing,
        DockSide::Top,
        DockSide::Bottom,
    ] {
        model.set_side_rail(side, 40.0);
    }
    settle_tab(&mut t);
    (t, model, dock)
}

fn expected_side_orders() -> Vec<(DockSide, Vec<&'static str>)> {
    vec![
        (DockSide::Leading, vec!["rail", "content", "handle"]),
        (DockSide::Trailing, vec!["handle", "content", "rail"]),
        (DockSide::Top, vec!["rail", "content", "handle"]),
        (DockSide::Bottom, vec!["handle", "rail", "content"]),
    ]
}

#[test]
fn inside_each_side_tab_follows_the_drawing() {
    let (mut t, _model, dock) = rails_everywhere();
    assert_eq!(side_part_orders(&mut t, dock), expected_side_orders());
}

#[test]
fn inside_each_side_tab_follows_the_drawing_right_to_left() {
    let (mut t, _model, dock) = rails_everywhere();
    t.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    settle_tab(&mut t);
    assert_eq!(side_part_orders(&mut t, dock), expected_side_orders());
}

#[test]
fn a_disabled_side_takes_no_tab_stop_and_no_space() {
    let (mut t, model, dock) = rails_everywhere();
    model.set_side_enabled(DockSide::Trailing, false);
    settle_tab(&mut t);
    let regions = t.children(dock);
    let trailing = regions[2];
    let b = t.bounds(trailing);
    assert_eq!(
        b.width * b.height,
        0.0,
        "the disabled side's region has no area"
    );
    assert!(
        !t.tab_stops_within(dock)
            .iter()
            .any(|&s| t.is_descendant_of(s, trailing)),
        "nothing in the disabled side is a Tab stop"
    );
    assert!(
        subtree_has_role(&t, dock, Role::TabList),
        "the other sides' rails are still tab lists"
    );
}

// ── F6 regions ────────────────────────────────────────────────────────

/// Which of the dock's five regions holds focus, by the name F6 should say
/// there: the dock's children are `[centre, leading, trailing, top, bottom,
/// courier]`.
fn focused_region_name(t: &WidgetTree, dock: WidgetId) -> Option<&'static str> {
    let focused = t.focused()?;
    let names = [
        "Main area",
        "Left panel",
        "Right panel",
        "Top panel",
        "Bottom panel",
    ];
    t.children(dock)
        .into_iter()
        .zip(names)
        .find(|&(region, _)| t.is_descendant_of(focused, region))
        .map(|(_, name)| name)
}

#[test]
fn f6_visits_each_dock_region_and_says_its_name() {
    let (mut t, _model, _root, dock) = five_regions(None);
    let centre = t.find_by_label("Centre button").expect("the centre button");
    t.focus(centre);
    settle(&mut t);

    let mut visited = Vec::new();
    for _ in 0..5 {
        let mark = heard_mark(&t);
        t.press_key(Key::F6, Modifiers::NONE);
        settle(&mut t);
        let region = focused_region_name(&t, dock).expect("focus is in a region");
        assert_eq!(
            heard_since(&t, mark),
            vec![region.to_string()],
            "F6 says the name of the region it moved to"
        );
        visited.push(region);
    }
    assert_eq!(
        visited.last(),
        Some(&"Main area"),
        "five F6 presses come back to the centre"
    );
    visited.sort_unstable();
    assert_eq!(
        visited,
        vec![
            "Bottom panel",
            "Left panel",
            "Main area",
            "Right panel",
            "Top panel"
        ],
        "each region once"
    );
}

#[test]
fn a_side_region_is_named_for_where_it_is_on_screen_in_right_to_left() {
    use teksilo_core::environment::LayoutDirection;
    let (mut t, _model, _root, dock) = five_regions(None);
    t.set_layout_direction(LayoutDirection::RightToLeft);
    settle(&mut t);
    let centre = t.find_by_label("Centre button").expect("the centre button");
    t.focus(centre);
    settle(&mut t);
    let leading = t.children(dock)[1];
    let mut heard = Vec::new();
    for _ in 0..5 {
        let mark = heard_mark(&t);
        t.press_key(Key::F6, Modifiers::NONE);
        settle(&mut t);
        if t.focused().is_some_and(|f| t.is_descendant_of(f, leading)) {
            heard = heard_since(&t, mark);
        }
    }
    assert_eq!(heard, vec!["Right panel".to_string()]);
}

#[test]
fn f6_into_a_side_lands_on_its_panel_not_its_resize_handle() {
    // The trailing and bottom sides draw their resize handle on their inner
    // edge, so it is their first Tab stop. F6 goes past it into the panel;
    // Tab still stops on it.
    let (mut t, _model, _root, dock) = five_regions(None);
    let centre = t.find_by_label("Centre button").expect("the centre button");
    t.focus(centre);
    settle(&mut t);
    t.press_key(Key::Tab, Modifiers::NONE);
    settle(&mut t);
    assert_eq!(
        t.accessibility_node(t.focused().expect("focus")).role(),
        teksilo_core::accesskit::Role::Splitter,
        "Tab from the centre reaches the trailing side's handle first"
    );

    t.focus(centre);
    settle(&mut t);
    let mut landed = Vec::new();
    for _ in 0..5 {
        t.press_key(Key::F6, Modifiers::NONE);
        settle(&mut t);
        let region = focused_region_name(&t, dock).expect("focus is in a region");
        landed.push((region, focused_name(&t)));
    }
    for side in ["Right panel", "Bottom panel"] {
        let (_, name) = landed
            .iter()
            .find(|(region, _)| *region == side)
            .expect("F6 visits the side");
        // The panel's first control: its dock's tab.
        let tab = if side == "Right panel" {
            "TrailDock"
        } else {
            "BottomDock"
        };
        assert_eq!(name.as_deref(), Some(tab), "{side}: {landed:?}");
    }

    // A handle reached by Tab is not where F6 comes back to.
    t.focus(centre);
    settle(&mut t);
    t.press_key(Key::Tab, Modifiers::NONE);
    settle(&mut t);
    for _ in 0..5 {
        t.press_key(Key::F6, Modifiers::NONE);
        settle(&mut t);
        if focused_region_name(&t, dock) == Some("Right panel") {
            break;
        }
    }
    assert_eq!(focused_name(&t).as_deref(), Some("TrailDock"));
}

/// F6 from the centre, round the dock, until focus is in `side`; what it
/// landed on there.
fn f6_into(t: &mut WidgetTree, dock: WidgetId, side: &str) -> Option<String> {
    for _ in 0..5 {
        t.press_key(Key::F6, Modifiers::NONE);
        settle(t);
        if focused_region_name(t, dock) == Some(side) {
            return focused_name(t);
        }
    }
    None
}

#[test]
fn f6_into_a_side_with_a_rail_lands_on_its_panel_and_on_the_rail_only_when_collapsed() {
    // The leading side has an activity rail, drawn before its panel and so
    // its first Tab stop. F6 goes past it into the panel; Tab still meets
    // it first.
    let (mut t, model, _root, dock) = five_regions(None);
    let centre = t.find_by_label("Centre button").expect("the centre button");
    let leading = t.children(dock)[1];
    let first_stop = t
        .tab_stops_within(leading)
        .first()
        .copied()
        .expect("the side has Tab stops");
    assert_eq!(
        t.accessibility_node(first_stop).name(),
        Some("LeadDock"),
        "Tab meets the rail's item first"
    );
    t.focus(centre);
    settle(&mut t);
    assert_eq!(
        f6_into(&mut t, dock, "Left panel").as_deref(),
        Some("Leading button"),
        "the panel's first control, not the rail"
    );

    // Collapsed, the side shows its rail and no panel: F6 lands on the rail.
    model.set_side_visible(DockSide::Leading, false);
    settle(&mut t);
    t.focus(centre);
    settle(&mut t);
    assert_eq!(
        f6_into(&mut t, dock, "Left panel").as_deref(),
        Some("LeadDock"),
        "the rail, when the panel shows nothing focusable"
    );
}

#[test]
fn a_rail_item_the_user_focused_is_where_f6_comes_back() {
    let (mut t, _model, _root, dock) = five_regions(None);
    let leading = t.children(dock)[1];
    let rail_item = t.tab_stops_within(leading)[0];
    t.focus(rail_item);
    settle(&mut t);
    assert_eq!(
        f6_into(&mut t, dock, "Left panel").as_deref(),
        Some("LeadDock"),
        "the remembered focus comes before the panel"
    );
}

#[test]
fn a_window_holding_a_dock_opens_with_the_first_focus_it_had() {
    // A side's F6 landing is not a focus hint: a window or a modal holding
    // the dock finds no hint in it, as before the landing existed, so a
    // window opens with no focus and a modal on its first control.
    let (t, _model, root, dock) = five_regions(None);
    assert_eq!(t.widget_initial_focus_hint(root), None);
    assert_eq!(t.widget_initial_focus_hint(dock), None);
    let first = t.first_focusable_descendant(dock).expect("a first control");
    assert_eq!(
        t.accessibility_node(first).name(),
        Some("TopDock"),
        "a modal holding the dock starts on the top panel's tab, its first Tab stop"
    );
}
