//! Headless widget-level integration tests for [`DockingLayout`](super::DockingLayout):
//! build a real layout in a `WidgetTree`, lay it out, and assert the geometry
//! and accessibility wiring.

use std::time::Duration;

use bastyde_canvas::{Point, Size, SizeProposal};
use bastyde_core::accesskit::Role;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_i18n::lit;

use super::{DockOpenLocation, DockSide, DockWidget, DockWidgetId, DockingLayout, DockingModel};

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
    assert!((cb.width - 1000.0).abs() < 0.5, "center fills with no docks");
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
                    .dock(DockWidget::new(dock, lit!("Explorer"), |_| FixedLeaf(120.0, 120.0))),
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
    assert!(!model.is_side_visible(DockSide::Leading), "click hid the side");
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
    assert_eq!(seen.get(), Some(IconButtonSize::Large), "slot gets the rail size");

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

    assert_eq!(model.side_tabs(DockSide::Leading)[0].panes[0], a, "A is first");

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
    let hb = find_first_role(&t, root, Role::Button)
        .expect("a hamburger renders in the empty strip");
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
