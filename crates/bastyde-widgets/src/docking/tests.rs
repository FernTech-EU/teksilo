// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless widget-level integration tests for [`DockingLayout`](super::DockingLayout):
//! build a real layout in a `WidgetTree`, lay it out, and assert the geometry
//! and accessibility wiring.

use std::time::Duration;

use bastyde_canvas::{Point, Size, SizeProposal};
use bastyde_core::accessibility::widget_id_to_node_id;
use bastyde_core::accesskit;
use bastyde_core::accesskit::Role;
use bastyde_core::event::{Key, Modifiers, WidgetEvent};
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_i18n::lit;

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
    WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
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

fn dock(title: &'static str) -> (DockWidgetId, DockWidget) {
    let id = DockWidgetId::fresh();
    (
        id,
        DockWidget::new(id, lit!(title), |_| FixedLeaf(120.0, 120.0)),
    )
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
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

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
    t.dispatch_event(WidgetEvent::PointerDown {
        position: p,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    t.dispatch_event(WidgetEvent::PointerUp {
        position: p,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
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
        fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
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
            let root = ctx.add(VStack::new().add_child(btn).add_child(layout));
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
            b: bastyde_canvas::Rect,
            _p: SizeProposal,
            ch: &mut [bastyde_core::widget::WidgetPlacement],
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

    // children: [center, leading_clip, leading_rail, leading_handle, …]
    let clip = t.children(root)[1];
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

    use bastyde_core::DragPayload;

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

    // The rail (Role::TabList) is the activity-bar strip; its width is the rail
    // thickness.
    let rail = find_first_role(&t, root, Role::TabList).expect("a rail renders");
    let w_default = t.bounds(rail).width;

    // Compact must make the whole strip narrower — not just its items.
    model.set_side_rail_size(DockSide::Leading, DockRailItemSize::Compact);
    t.layout(SizeProposal::exact(1000.0, 800.0));
    let rail = find_first_role(&t, root, Role::TabList).expect("the rail still renders");
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
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

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
    t.dispatch_event(WidgetEvent::PointerDown {
        position: centre,
        button: PointerButton::Secondary,
        modifiers: Modifiers::NONE,
    });
    assert_eq!(
        t.active_overlays().len(),
        1,
        "right-clicking a rail item opens its context menu"
    );
}

#[test]
fn context_menu_is_only_on_tabs_not_on_pane_content() {
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

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
    t.dispatch_event(WidgetEvent::PointerDown {
        position: content,
        button: PointerButton::Secondary,
        modifiers: Modifiers::NONE,
    });
    assert!(
        t.active_overlays().is_empty(),
        "right-clicking pane content must not open a context menu"
    );

    // Right-click the tab header itself: the menu opens.
    let tab_centre = Point::new(tb.x + tb.width / 2.0, tb.y + tb.height / 2.0);
    t.dispatch_event(WidgetEvent::PointerDown {
        position: tab_centre,
        button: PointerButton::Secondary,
        modifiers: Modifiers::NONE,
    });
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
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

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
    t.dispatch_event(WidgetEvent::PointerDown {
        position: c,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    t.dispatch_event(WidgetEvent::PointerUp {
        position: c,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
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
    let rail = find_role_name(&t, root, Role::TabList, "Trailing activity bar")
        .expect("the trailing rail");
    let hb = t.bounds(header);
    let rb = t.bounds(rail);
    t.drag(
        Point::new(hb.x + hb.width / 2.0, hb.y + 6.0),
        Point::new(rb.x + rb.width / 2.0, rb.y + rb.height / 2.0),
    );
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
    for (i, &tab) in tabs.iter().enumerate() {
        let node =
            find_a11y_node(&update, widget_id_to_node_id(tab)).expect("rail tab in a11y tree");
        assert_eq!(node.size_of_set(), Some(2), "rail tab {i} size_of_set");
        assert_eq!(
            node.position_in_set(),
            Some(i + 1),
            "rail tab {i} reports 1-based position_in_set"
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
        bastyde_core::event::PointerButton::Primary,
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
    assert!(
        has_named(&t, root, "More actions: Explorer"),
        "show_header(true) ⇒ a bare dock has the ⋮ options button"
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
    use crate::toolbar::ToolbarAction;

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
                    ToolbarAction::new(lit!("ActX"), || IconWidget::chevron_up(12.0)),
                    ToolbarAction::new(lit!("ActY"), || IconWidget::chevron_up(12.0)),
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
