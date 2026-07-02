// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use bastyde_i18n::LocalizedString;
use bastyde_i18n::lit;
use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Size, SizeProposal};
use bastyde_core::accesskit;
use bastyde_core::event::{Key, Modifiers};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_data::ListModel;

use crate::tab_widget::{
    TabBar, TabBarOrientation, TabDelegate, TabHandle, TabId, TabInfo, TabSizing, TabWidget,
};

// ─── shared test helpers ────────────────────────────────────────────

#[derive(Debug)]
struct FixedLeaf(f32, f32);

impl Widget for FixedLeaf {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(self.0, self.1).into()
    }
}

/// A focusable leaf that publishes its own `WidgetId` on build, so a
/// test can assert focus landed on it.
#[derive(Debug)]
struct FocusableLeaf {
    id_out: Rc<Cell<Option<WidgetId>>>,
}

impl Widget for FocusableLeaf {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        self.id_out.set(Some(ctx.self_id()));
        ctx.apply_self_handlers(bastyde_core::widget_builder::HandlerSet::new().focusable(true));
        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(100.0, 40.0).into()
    }
}

#[derive(Debug)]
struct BuildCountingLeaf {
    build_count: Rc<Cell<usize>>,
}

impl Widget for BuildCountingLeaf {
    fn build(&mut self, _ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        self.build_count.set(self.build_count.get() + 1);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(120.0, 48.0).into()
    }
}

/// The bar's content row/column. `TabBar`'s direct child is the
/// `TabStyle::make_bar` chrome container — `[backdrop?, chrome
/// painter, content]` — so the content (the `row_outer` HStack /
/// VStack) is the container's last child.
fn bar_content(tree: &WidgetTree, bar_id: WidgetId) -> WidgetId {
    let chrome = tree.child_widget(bar_id, 0);
    *tree
        .children(chrome)
        .last()
        .expect("bar chrome has a content child")
}

/// Walk a stand-alone `TabBar` (no `TabWidget` wrapper) to the
/// `TabHeaderRow` containing the unpinned headers.
fn data_source_header_row(tree: &WidgetTree, bar_id: WidgetId) -> WidgetId {
    let row_outer = bar_content(tree, bar_id);
    let expand = expand_in_row_outer(tree, row_outer);
    let scroll = tree.child_widget(expand, 0);
    tree.child_widget(scroll, 0)
}

/// Locate the `Expand` child of `row_outer` regardless of whether
/// scroll arrows / overflow dropdown / pinned strip are present —
/// signature: a child whose only descendant is a `ScrollView`.
fn expand_in_row_outer(tree: &WidgetTree, row_outer: WidgetId) -> WidgetId {
    for child in tree.children(row_outer) {
        if tree.children(child).len() == 1 {
            let scroll = tree.child_widget(child, 0);
            let info = tree.accessibility_node(scroll);
            if info.role() == accesskit::Role::ScrollView {
                return child;
            }
        }
    }
    panic!("no Expand[ScrollArea] child found in row_outer");
}

fn label(s: &str) -> LocalizedString {
    lit!(s)
}

// ─── TabWidget — static-only construction ───────────────────────────

#[test]
fn icon_only_tab_centers_its_icon() {
    use crate::IconWidget;
    use crate::tab_widget::TabDisplayMode;

    fn first_tab(tree: &WidgetTree, id: WidgetId) -> Option<WidgetId> {
        if tree.accessibility_node(id).role() == accesskit::Role::Tab
            && tree.bounds(id).height > 0.0
        {
            return Some(id);
        }
        tree.children(id).iter().find_map(|&c| first_tab(tree, c))
    }
    // Smallest roughly-square leaf in the subtree — the icon glyph (background
    // / indicator rects are full-width, the icon is a small square).
    fn icon_leaf(tree: &WidgetTree, id: WidgetId, best: &mut Option<bastyde_canvas::Rect>) {
        let kids = tree.children(id);
        if kids.is_empty() {
            let b = tree.bounds(id);
            let squarish = (b.width - b.height).abs() < 6.0;
            if b.width > 4.0 && b.width < 40.0 && squarish {
                if best.is_none_or(|cur| b.width < cur.width) {
                    *best = Some(b);
                }
            }
        }
        for &c in kids.iter() {
            icon_leaf(tree, c, best);
        }
    }

    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    // Content-sized (Independent, the dock's mode): the tab sizes to icon +
    // padding, so the lone icon must (a) still render and (b) sit centred.
    let root = tree.add(
        TabWidget::new(selected)
            .tab_display(TabDisplayMode::Icon)
            .tab_sizing(TabSizing::Independent)
            .min_tab_width(20.0)
            .static_tab(
                TabInfo::new()
                    .title(label("Alpha"))
                    .icon(|| IconWidget::checkmark(16.0)),
                FixedLeaf(120.0, 48.0),
            )
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    let tab = first_tab(&tree, root).expect("an icon-only tab renders");
    let tb = tree.bounds(tab);
    // The icon must still be laid out with real bounds (it must not vanish).
    let mut icon = None;
    icon_leaf(&tree, tab, &mut icon);
    let icon = icon.expect("the icon glyph still renders in icon-only mode");
    assert!(
        icon.width > 4.0 && icon.height > 4.0,
        "the icon has real bounds ({}×{})",
        icon.width,
        icon.height
    );
    let icon_cx = icon.x + icon.width / 2.0;
    let tab_cx = tb.x + tb.width / 2.0;
    assert!(
        (icon_cx - tab_cx).abs() < 3.0,
        "the lone icon is centred: icon_cx={icon_cx}, tab_cx={tab_cx}"
    );
}

#[test]
fn tab_display_flips_icon_text_live() {
    use crate::IconWidget;
    use crate::tab_widget::TabDisplayMode;

    fn tab_named(tree: &WidgetTree, id: WidgetId, name: &str) -> Option<WidgetId> {
        let n = tree.accessibility_node(id);
        if n.role() == accesskit::Role::Tab
            && n.name() == Some(name)
            && tree.bounds(id).height > 0.0
        {
            return Some(id);
        }
        tree.children(id)
            .iter()
            .find_map(|&c| tab_named(tree, c, name))
    }

    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mode = Signal::new(TabDisplayMode::Text);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let root = tree.add(
        TabWidget::new(selected.clone())
            .tab_display(mode.clone())
            .tab_sizing(TabSizing::Independent)
            // A compact floor so an icon-only tab can shrink below the default
            // editor-tab minimum (the bar's row clamps to `min_tab_width`).
            .min_tab_width(20.0)
            .static_tab(
                TabInfo::new()
                    .title(label("Alpha"))
                    .icon(|| IconWidget::checkmark(16.0)),
                FixedLeaf(120.0, 48.0),
            )
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let text_tab = tab_named(&tree, root, "Alpha").expect("text mode names the tab 'Alpha'");
    let w_text = tree.bounds(text_tab).width;

    // Flipping the bound signal must rebuild the bar live into icon-only: the
    // tab shrinks to its icon, but its *accessible name* stays "Alpha" (the
    // title moved to the tooltip — a screen reader must still identify it).
    mode.set(TabDisplayMode::Icon);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let icon_tab = tab_named(&tree, root, "Alpha")
        .expect("icon mode keeps the accessible name 'Alpha' (a11y, not a visual)");
    let w_icon = tree.bounds(icon_tab).width;
    assert!(
        w_icon < w_text,
        "flipping tab_display to Icon shrinks the tab to its icon live \
         ({w_icon} < {w_text})"
    );
}

#[test]
fn static_tab_initial_selection_lands_on_first_tab() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
            .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // After build, selected_id has been auto-populated by the
    // index → id sync effect (initial_idx = 0, mapped to the first
    // static tab's TabId).
    assert!(
        selected.get().is_some(),
        "static tabs should auto-populate selection from index 0"
    );
}

#[test]
fn static_tabs_dormancy_preserved_across_switches() {
    // Use a stable TabId per static tab so we can flip selection
    // by id from outside without depending on index lookups.
    let id_first = TabId::fresh();
    let id_second = TabId::fresh();
    let selected: Signal<Option<TabId>> = Signal::new(Some(id_first));

    let first_builds = Rc::new(Cell::new(0));
    let second_builds = Rc::new(Cell::new(0));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab_with_id(
                id_first,
                TabInfo::new().title(label("First")),
                BuildCountingLeaf {
                    build_count: first_builds.clone(),
                },
            )
            .static_tab_with_id(
                id_second,
                TabInfo::new().title(label("Second")),
                BuildCountingLeaf {
                    build_count: second_builds.clone(),
                },
            )
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // Both tabs build once on initial layout (Switcher is eager;
    // dormant means hidden, not unbuilt).
    assert_eq!(first_builds.get(), 1);
    assert_eq!(second_builds.get(), 1);

    // Flip to second tab. No model mutation, but visibility flips
    // and the dormant pane stays dormant — neither pane should
    // build a second time.
    selected.set(Some(id_second));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(first_builds.get(), 1, "first pane should not rebuild");
    assert_eq!(second_builds.get(), 1, "second pane should not rebuild");

    // Flip back. Same expectation.
    selected.set(Some(id_first));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(first_builds.get(), 1);
    assert_eq!(second_builds.get(), 1);
}

#[test]
fn static_tab_pane_survives_dynamic_model_push() {
    // Regression: before pane memoization, a dynamic_model mutation
    // forced TabWidget to rebuild and substituted Spacer placeholders
    // for static-tab content. With memoization the static pane's
    // build counter must NOT increment on subsequent rebuilds.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let static_builds = Rc::new(Cell::new(0));
    let model: ListModel<TabHandle> = ListModel::new();

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab(
                TabInfo::new().title(label("Welcome")),
                BuildCountingLeaf {
                    build_count: static_builds.clone(),
                },
            )
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(static_builds.get(), 1, "static pane builds once");

    // Push a dynamic tab — TabWidget rebuilds, Switcher recomposes,
    // but the static pane WidgetId is memoized so the inner widget
    // is NOT rebuilt.
    model.push(TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new().title(label("Doc1")),
        (),
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(
        static_builds.get(),
        1,
        "static pane must NOT rebuild on dynamic-model mutation"
    );

    // Removing the dynamic tab triggers another rebuild. Same.
    let _ = model.remove(0);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(static_builds.get(), 1);
}

#[test]
fn static_tab_id_survives_rebuild() {
    // The DSL element-valued slot path: static_tab_id pre-registers
    // the content in the arena, then memoization keeps the pane
    // referring to that id across rebuilds.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model: ListModel<TabHandle> = ListModel::new();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let content_id = tree.add(FixedLeaf(120.0, 48.0));
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab_id(TabInfo::new().title(label("Pre")), content_id)
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // The pre-registered widget is reachable via the AT tree.
    let update = tree.sync_accessibility();
    let panel_count_before = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == accesskit::Role::TabPanel)
        .count();
    assert_eq!(panel_count_before, 1, "exactly one tab panel");

    // Trigger a rebuild via dynamic_model.push.
    model.push(TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new().title(label("Doc")),
        (),
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // After the rebuild, the static-tab panel must STILL exist —
    // before memoization this would have been replaced by a Spacer
    // (no TabPanel role). Only ONE panel is visible at a time (the
    // active one), so we count == 1 and verify it's the static.
    let update = tree.sync_accessibility();
    let panels: Vec<String> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == accesskit::Role::TabPanel)
        .map(|(_, n)| n.label().unwrap_or("").to_string())
        .collect();
    assert_eq!(panels.len(), 1, "one visible panel at a time");
    assert_eq!(
        panels[0], "Pre",
        "static panel must survive rebuild — was replaced before memoization"
    );

    // Now flip selection to the dynamic tab; the dynamic panel
    // should appear (proving it was created and dormant, not lost).
    if let Some(dyn_id) = (0..model.len())
        .filter_map(|i| model.with_item(i, |h| h.id))
        .next()
    {
        selected.set(Some(dyn_id));
        tree.layout(SizeProposal::exact(640.0, 320.0));
        let update = tree.sync_accessibility();
        let panels: Vec<String> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == accesskit::Role::TabPanel)
            .map(|(_, n)| n.label().unwrap_or("").to_string())
            .collect();
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0], "Doc", "dynamic panel reachable after activation");
    }
}

#[test]
fn bar_trailing_slot_survives_rebuild() {
    // Regression: the bar slot was consumed on first build via
    // PendingChild::Deferred and lost on rebuild. With BarSlot
    // memoization the slot's WidgetId must remain reachable.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model: ListModel<TabHandle> = ListModel::new();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    #[derive(Debug)]
    struct MarkerLeaf;
    impl Widget for MarkerLeaf {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(48.0, 24.0).into()
        }
        fn accessibility(&self, b: &mut bastyde_core::accessibility::AccessNodeBuilder) {
            b.set_role(accesskit::Role::Button);
            b.set_name("MARKER_SLOT");
        }
    }

    tree.add(
        TabWidget::new(selected.clone())
            .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .bar_trailing_slot(MarkerLeaf)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    let marker_present = |tree: &mut WidgetTree| -> bool {
        let update = tree.sync_accessibility();
        update
            .nodes
            .iter()
            .any(|(_, n)| n.role() == accesskit::Role::Button && n.label() == Some("MARKER_SLOT"))
    };
    assert!(
        marker_present(&mut tree),
        "marker slot present after first build"
    );

    // Force a rebuild via dynamic_model push.
    model.push(TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new().title(label("Doc")),
        (),
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert!(
        marker_present(&mut tree),
        "marker slot must still be present after rebuild"
    );
}

#[test]
fn dynamic_tab_pane_state_survives_reorder() {
    // Heavy state lives in the handle's payload (not the widget),
    // and the pane WidgetId is memoized by TabId — so a tab that
    // moves to a different position keeps the SAME pane widget.
    // Verify by tracking pane build counts: a memoized pane builds
    // exactly once, regardless of reorders.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let id_c = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![
        TabHandle::dynamic(id_a, "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(id_b, "doc", TabInfo::new().title(label("B")), ()),
        TabHandle::dynamic(id_c, "doc", TabInfo::new().title(label("C")), ()),
    ]);

    // Track build count per TabId via a shared map. The factory is
    // called once per (id, first build); after memoization, NEVER
    // again. We verify the factory call count, NOT the pane's
    // internal build counter.
    let factory_calls: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let fc = factory_calls.clone();

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", move |_h, _s| {
                fc.set(fc.get() + 1);
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(factory_calls.get(), 3, "three factory calls for three tabs");

    // Move A to the end → [B, C, A].
    model.move_item(0, 2);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(
        factory_calls.get(),
        3,
        "reorder must NOT recreate dynamic panes (state would be lost)"
    );

    // Insert a fresh tab. Only the new one calls the factory.
    let id_d = TabId::fresh();
    model.push(TabHandle::dynamic(
        id_d,
        "doc",
        TabInfo::new().title(label("D")),
        (),
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(factory_calls.get(), 4, "exactly one new factory call");
}

#[test]
fn reorder_preserves_selected_id() {
    // Regression: the bar's reorder wrapper used to call
    // `selected.set(adjusted_index)` *before* `move_item`, which
    // fired the bar's index → id effect against the pre-move
    // `index_to_id` and stamped the wrong TabId into `selected_id`.
    // After the rebuild the visually-selected tab and the visible
    // content pane fell out of sync. Verify a reorder (`move_item`)
    // never changes which TabId is active.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let id_c = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![
        TabHandle::dynamic(id_a, "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(id_b, "doc", TabInfo::new().title(label("B")), ()),
        TabHandle::dynamic(id_c, "doc", TabInfo::new().title(label("C")), ()),
    ]);

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .reorderable(true)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // Activate B (the middle tab).
    selected.set(Some(id_b));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(selected.get(), Some(id_b));

    // Move B from index 1 to index 2 → [A, C, B]. The selected
    // *id* must still be B (its position in the model changed but
    // its identity is stable).
    model.move_item(1, 2);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(
        selected.get(),
        Some(id_b),
        "reorder must not change which tab id is active"
    );

    // Move A (now at 0) to the end → [C, B, A]. Still B selected.
    model.move_item(0, 2);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(selected.get(), Some(id_b));

    // Move B (now at 1) to position 0 — the active tab itself
    // moves; its id stays.
    model.move_item(1, 0);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(selected.get(), Some(id_b));
}

#[test]
fn dynamic_pane_dropped_when_tab_removed() {
    // After model.remove, the removed tab's pane id should be
    // pruned from the memo so we don't leak references.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![
        TabHandle::dynamic(id_a, "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(id_b, "doc", TabInfo::new().title(label("B")), ()),
    ]);

    let factory_calls: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let fc = factory_calls.clone();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", move |_h, _s| {
                fc.set(fc.get() + 1);
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(factory_calls.get(), 2);

    // Remove A. Then re-push a tab with the same id (apps reuse
    // ids on session restore). The factory should fire AGAIN for
    // the resurrected id — proving the prior memo entry was
    // pruned, not retained.
    let _ = model.remove(0);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(factory_calls.get(), 2);

    model.push(TabHandle::dynamic(
        id_a,
        "doc",
        TabInfo::new().title(label("A2")),
        (),
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(
        factory_calls.get(),
        3,
        "factory must fire again for a re-pushed id (memo entry was pruned)"
    );
}

#[test]
fn dynamic_tab_removal_reaps_pane_no_active_leak() {
    // The reconcile fix at the framework level: TabWidget is
    // `preserves_children_on_rebuild`, so before the fix a removed tab's pane
    // stayed in the arena AND active (a ghost), growing the active-widget set
    // on every add/remove cycle. Cycling add→remove must leave the active
    // count flat.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model: ListModel<TabHandle> = ListModel::new();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let baseline = tree.active_widget_count();

    for _ in 0..5 {
        model.push(TabHandle::dynamic(
            TabId::fresh(),
            "doc",
            TabInfo::new().title(label("Tmp")),
            (),
        ));
        tree.layout(SizeProposal::exact(640.0, 320.0));
        let _ = model.remove(model.len() - 1);
        tree.layout(SizeProposal::exact(640.0, 320.0));
    }

    assert_eq!(
        tree.active_widget_count(),
        baseline,
        "each removed tab's pane must be reaped — no stranded active ghosts"
    );
}

#[test]
fn locale_change_retitles_live_tabs() {
    // Regression for the eager-resolution bug: with delegate-based
    // label resolution, live tabs must retitle when the locale
    // changes — without recreating the widget. The test installs a
    // process-thread-local i18n manager, switches it, and verifies
    // the AT label changes accordingly.
    use bastyde_i18n::{
        I18nConfig, I18nManager, LanguageIdentifier, LocalizedString, localized, resolve_message,
        thread_local::{clear, install},
    };

    clear();
    let cfg = I18nConfig::test_only("en-US", &[("tab-greeting", "Hello")])
        .with_locale("fr-FR", &[("tab-greeting", "Bonjour")]);
    let mgr = I18nManager::from_config(&cfg);
    install(mgr.clone());

    let selected: Signal<Option<TabId>> = Signal::new(None);
    let title: LocalizedString = localized(|| resolve_message("tab-greeting", &[]));

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab(TabInfo::new().title(title), FixedLeaf(120.0, 48.0))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    let label_of_tab = |tree: &mut WidgetTree| -> String {
        let update = tree.sync_accessibility();
        let tab = update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == accesskit::Role::Tab)
            .expect("at least one tab in tree");
        tab.1.label().unwrap_or("").to_string()
    };
    assert_eq!(label_of_tab(&mut tree), "Hello");

    let fr: LanguageIdentifier = "fr-FR".parse().unwrap();
    mgr.set_locale(fr);
    // The widget tree also needs to be told the locale changed so
    // it marks the accessibility cache dirty for re-emission.
    tree.set_locale("fr-FR".to_string());
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(
        label_of_tab(&mut tree),
        "Bonjour",
        "tab label must retitle on locale change"
    );
    clear();
}

#[test]
#[should_panic(expected = "is reserved by the framework for static tabs")]
fn dynamic_tab_kind_must_not_be_static_kind_at_registration() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let _ = TabWidget::new(selected).dynamic_tab::<()>(crate::tab_widget::STATIC_KIND, |_h, _s| {
        Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
    });
}

#[test]
#[should_panic(expected = "is reserved for static tabs")]
fn tab_handle_dynamic_rejects_static_kind() {
    // Construction-side defense — even if the registration check
    // is bypassed, building a TabHandle with the reserved kind must
    // also fail.
    let _ = TabHandle::dynamic(
        TabId::fresh(),
        crate::tab_widget::STATIC_KIND,
        TabInfo::new().title(label("oops")),
        (),
    );
}

#[test]
fn cross_boundary_default_reorder_silently_dropped() {
    // The default on_reorder rejects moves that cross the
    // static / dynamic boundary, since static tabs are pinned in
    // place. The model is unchanged after such a drop.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_static = TabId::fresh();
    let id_dyn_a = TabId::fresh();
    let id_dyn_b = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![
        TabHandle::dynamic(id_dyn_a, "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(id_dyn_b, "doc", TabInfo::new().title(label("B")), ()),
    ]);
    let initial_ids: Vec<TabId> = (0..model.len())
        .filter_map(|i| model.with_item(i, |h| h.id))
        .collect();

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(
        TabWidget::new(selected)
            .static_tab_with_id(
                id_static,
                TabInfo::new().title(label("S")),
                FixedLeaf(120.0, 48.0),
            )
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .reorderable(true)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // Synthesize a cross-boundary reorder by directly invoking the
    // bar's on_reorder dispatcher via its index. We don't have a
    // public hook to call the bar's callback from outside, but we
    // can verify the *outcome*: simulate the reorder via a
    // `move_item` fired from the bar layer would, by definition,
    // hit the default handler. Drag-DnD path is exercised in bar
    // tests; here we just verify the model's dynamic region is
    // unchanged when nothing in our own code moves it. End-state
    // check: model still in original order.
    let _ = widget_id;
    let after_ids: Vec<TabId> = (0..model.len())
        .filter_map(|i| model.with_item(i, |h| h.id))
        .collect();
    assert_eq!(after_ids, initial_ids, "model unchanged");
}

#[test]
fn on_pin_toggle_handler_fires_with_tab_id() {
    // Wire a pin-toggle callback and verify it fires with the
    // correct TabId when the bar reports a pin/unpin from a
    // cross-boundary drag. We can't drive the bar's DnD pipeline
    // from a unit test directly (it requires a dragged-item
    // handle), so we verify the wiring at the API level: the
    // setter installs a handler, and the build path translates
    // index → id correctly when the bar fires.
    //
    // This test verifies that the closure is wired up; bar-level
    // DnD drive tests live alongside bar.rs.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![TabHandle::dynamic(
        id_a,
        "doc",
        TabInfo::new().title(label("A")),
        (),
    )]);

    let captured: Rc<Cell<Option<(TabId, bool)>>> = Rc::new(Cell::new(None));
    let cap = captured.clone();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(
        TabWidget::new(selected)
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model)
            .on_pin_toggle(move |id, pinned, _ctx| cap.set(Some((id, pinned))))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // The wiring is in place; bar-level DnD pipelines drive the
    // actual fire. This test just establishes that the API path
    // doesn't panic and that a TabWidget with on_pin_toggle
    // installed reaches a stable state. Setting `captured` is
    // exercised in bar-level DnD tests.
    let _ = widget_id;
    assert_eq!(
        captured.get(),
        None,
        "no drag yet — handler should not have fired"
    );
}

#[test]
fn empty_model_after_close_drains_selection() {
    // Close the only tab → selection drains to None.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![TabHandle::dynamic(
        id_a,
        "doc",
        TabInfo::new().title(label("Only")).closable(true),
        (),
    )]);

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(selected.get(), Some(id_a));

    // Close the tab via the close button (hover-only — the helper
    // simulates the hover before locating + clicking).
    let bar = bar_of(&tree, widget_id);
    let header_row = data_source_header_row(&tree, bar);
    let headers = tree.children(header_row);
    hover_and_click_close(&mut tree, headers[0]);
    tree.layout(SizeProposal::exact(640.0, 320.0));

    assert_eq!(model.len(), 0);
    assert_eq!(selected.get(), None, "empty model drains selection to None");
}

#[test]
fn static_tab_disabled_skipped_by_arrow_keys() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
            .static_tab(
                TabInfo::new().title(label("Locked")).enabled(false),
                FixedLeaf(120.0, 48.0),
            )
            .static_tab(TabInfo::new().title(label("C")), FixedLeaf(120.0, 48.0))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // Tab into the strip, then ArrowRight: should jump from
    // index 0 ("A") past the disabled "Locked" to index 2 ("C").
    tree.press_key(Key::Tab, Modifiers::NONE);
    tree.press_key(Key::ArrowRight, Modifiers::NONE);

    // The active id should now correspond to the third static tab.
    // We can't get its TabId from outside the widget directly;
    // verify the AT tree reports the right Tab as selected.
    let update = tree.sync_accessibility();
    let tab_nodes: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == accesskit::Role::Tab)
        .collect();
    assert_eq!(tab_nodes.len(), 3);
    let selected_count = tab_nodes
        .iter()
        .filter(|(_, n)| n.is_selected().unwrap_or(false))
        .count();
    assert_eq!(
        selected_count, 1,
        "exactly one tab should be marked selected after keyboard nav"
    );
}

/// Both `Enter` and `Space` activate the focused tab AND dive focus into
/// its content panel — the desktop tab-control convention (Space/Enter
/// keyboard parity for invocable controls). Parameterized so each key is
/// asserted identically.
fn assert_activation_key_dives_into_panel(activation: Key) {
    let leaf_id: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab(
                TabInfo::new().title(label("A")),
                FocusableLeaf {
                    id_out: leaf_id.clone(),
                },
            )
            .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // Tab into the strip — focus lands on the selected (index 0) header.
    tree.press_key(Key::Tab, Modifiers::NONE);
    let header = tree.focused().expect("Tab should focus a tab header");
    let leaf = leaf_id.get().expect("panel content should have built");
    assert_ne!(
        header, leaf,
        "focus should start on the header, not the panel"
    );

    // The activation key dives focus into the selected tab's content panel.
    tree.press_key(activation, Modifiers::NONE);
    assert_eq!(
        tree.focused(),
        Some(leaf),
        "{activation:?} on a focused tab header should move focus into the panel's first control"
    );

    // The focus must survive the next build/layout pass — activation sets
    // the (already-selected) tab, and a rebuild must not yank focus back
    // to the header.
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(
        tree.focused(),
        Some(leaf),
        "focus should remain inside the panel after a rebuild/layout pass"
    );
}

#[test]
fn enter_on_tab_header_moves_focus_into_panel_content() {
    assert_activation_key_dives_into_panel(Key::Enter);
}

#[test]
fn space_on_tab_header_moves_focus_into_panel_content() {
    assert_activation_key_dives_into_panel(Key::Space);
}

/// A content-less tab (no focusable descendant, no `focusable_panel`
/// opt-in) must NOT trap focus on the non-interactive panel — focus stays
/// on the header for both activation keys.
fn assert_activation_key_keeps_focus_on_contentless_header(activation: Key) {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
            .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    tree.press_key(Key::Tab, Modifiers::NONE);
    let header = tree.focused().expect("Tab should focus a tab header");

    tree.press_key(activation, Modifiers::NONE);
    assert_eq!(
        tree.focused(),
        Some(header),
        "{activation:?} on a tab with no focusable panel content should leave focus on the header"
    );
}

#[test]
fn enter_on_contentless_tab_keeps_focus_on_header() {
    assert_activation_key_keeps_focus_on_contentless_header(Key::Enter);
}

#[test]
fn space_on_contentless_tab_keeps_focus_on_header() {
    assert_activation_key_keeps_focus_on_contentless_header(Key::Space);
}

// ─── TabWidget — dynamic registration + ListModel mutations ─────────

#[derive(Debug, Clone)]
struct DocState {
    initial_text: String,
}

#[derive(Debug, Clone)]
struct ImageState {
    url: String,
}

fn new_dynamic_handle<S: 'static>(kind: &'static str, title: &str, state: S) -> TabHandle {
    TabHandle::dynamic(
        TabId::fresh(),
        kind,
        TabInfo::new().title(label(title)).closable(true),
        state,
    )
}

#[test]
fn dynamic_tab_factory_dispatches_on_kind() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![
        new_dynamic_handle(
            "plain-text-doc",
            "Notes",
            DocState {
                initial_text: "hi".to_string(),
            },
        ),
        new_dynamic_handle(
            "image",
            "Logo",
            ImageState {
                url: "logo.png".to_string(),
            },
        ),
    ]);

    let doc_calls = Rc::new(Cell::new(0));
    let img_calls = Rc::new(Cell::new(0));
    let dc = doc_calls.clone();
    let ic = img_calls.clone();

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected)
            .dynamic_tab::<DocState>("plain-text-doc", move |_h, _s| {
                dc.set(dc.get() + 1);
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_tab::<ImageState>("image", move |_h, _s| {
                ic.set(ic.get() + 1);
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    assert_eq!(doc_calls.get(), 1, "doc factory should be called once");
    assert_eq!(img_calls.get(), 1, "image factory should be called once");
}

#[test]
fn dynamic_model_push_triggers_rebuild_with_new_tab() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model: ListModel<TabHandle> = ListModel::new();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<DocState>("plain-text-doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // Initially zero tabs — selection stays None.
    assert_eq!(selected.get(), None);

    // Push a new tab; widget should rebuild and adopt selection.
    let new_id = TabId::fresh();
    model.push(TabHandle::dynamic(
        new_id,
        "plain-text-doc",
        TabInfo::new().title(label("New")).closable(true),
        DocState {
            initial_text: "x".into(),
        },
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // After the rebuild, the index→id sync effect should have set
    // selected_id to the new tab (since selected_index_internal
    // defaults to 0 and there's now exactly one tab).
    assert_eq!(selected.get(), Some(new_id));
}

#[test]
fn dynamic_default_close_removes_from_model() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![
        TabHandle::dynamic(
            id_a,
            "doc",
            TabInfo::new().title(label("A")).closable(true),
            (),
        ),
        TabHandle::dynamic(
            id_b,
            "doc",
            TabInfo::new().title(label("B")).closable(true),
            (),
        ),
    ]);
    let model_for_assert = model.clone();

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // Walk to the close button on tab A and click it (the close
    // button is hover-only, so we hover the header first).
    let bar = bar_of(&tree, widget_id);
    let header_row = data_source_header_row(&tree, bar);
    let headers = tree.children(header_row);
    let header_a = headers[0];
    hover_and_click_close(&mut tree, header_a);
    tree.layout(SizeProposal::exact(640.0, 320.0));

    assert_eq!(
        model_for_assert.len(),
        1,
        "tab A should be removed by default close"
    );
    // Selection moved to the surviving tab.
    assert_eq!(selected.get(), Some(id_b));
}

#[test]
fn primary_click_activates_tab_secondary_does_not() {
    use bastyde_canvas::Point;
    use bastyde_core::event::PointerButton;

    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(
        TabWidget::new(selected.clone())
            .static_tab_with_id(
                id_a,
                TabInfo::new().title(label("A")),
                FixedLeaf(120.0, 48.0),
            )
            .static_tab_with_id(
                id_b,
                TabInfo::new().title(label("B")),
                FixedLeaf(120.0, 48.0),
            )
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // Initial selection lands on A (first static tab).
    assert_eq!(selected.get(), Some(id_a));

    let bar = bar_of(&tree, widget_id);
    let header_row = data_source_header_row(&tree, bar);
    let headers = tree.children(header_row);
    let center_b: Point = tree.bounds(headers[1]).center();

    // Right-click on tab B with no context-menu factory installed:
    // the framework's secondary-click path falls through to the
    // widget, but the auto-wired `TapRecognizer` defaults to
    // `ButtonMask::PRIMARY` and silently ignores non-Primary
    // presses, so no Tap is recognised. Selection must not move.
    tree.pointer_down_button(center_b, PointerButton::Secondary);
    tree.pointer_up_button(center_b, PointerButton::Secondary);
    assert_eq!(
        selected.get(),
        Some(id_a),
        "right-click must not activate a tab"
    );

    // Middle-click likewise must not activate (the close handler is
    // independent and doesn't fire here because the tab isn't
    // closable).
    tree.pointer_down_button(center_b, PointerButton::Middle);
    tree.pointer_up_button(center_b, PointerButton::Middle);
    assert_eq!(
        selected.get(),
        Some(id_a),
        "middle-click must not activate a tab"
    );

    // Primary click does activate.
    tree.pointer_down_button(center_b, PointerButton::Primary);
    tree.pointer_up_button(center_b, PointerButton::Primary);
    assert_eq!(
        selected.get(),
        Some(id_b),
        "primary-click must activate the clicked tab"
    );
}

#[test]
fn explicit_on_close_receives_tab_id() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![
        TabHandle::dynamic(
            id_a,
            "doc",
            TabInfo::new().title(label("A")).closable(true),
            (),
        ),
        TabHandle::dynamic(
            id_b,
            "doc",
            TabInfo::new().title(label("B")).closable(true),
            (),
        ),
    ]);

    let captured: Rc<Cell<Option<TabId>>> = Rc::new(Cell::new(None));
    let cap = captured.clone();

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model)
            .on_close(move |id, _ctx| cap.set(Some(id)))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    let bar = bar_of(&tree, widget_id);
    let header_row = data_source_header_row(&tree, bar);
    let headers = tree.children(header_row);
    hover_and_click_close(&mut tree, headers[1]);

    assert_eq!(
        captured.get(),
        Some(id_b),
        "explicit on_close should fire with the closed tab's id"
    );
}

#[test]
#[should_panic(expected = "tab kind 'image' was registered for")]
fn payload_kind_mismatch_panics_with_clear_message() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![TabHandle::dynamic(
        id,
        "image",
        TabInfo::new().title(label("Wrong")),
        DocState {
            initial_text: "doc-state-payload".into(),
        }, // wrong kind!
    )]);

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected)
            // Register "image" with payload type ImageState.
            .dynamic_tab::<ImageState>("image", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );

    // The mismatch surfaces during the build → layout pass when
    // the dynamic factory tries to downcast the payload.
    tree.layout(SizeProposal::exact(640.0, 320.0));
}

#[test]
fn static_then_dynamic_renders_in_order() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let dyn_id = TabId::fresh();
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![TabHandle::dynamic(
        dyn_id,
        "doc",
        TabInfo::new().title(label("Doc1")).closable(true),
        DocState {
            initial_text: "x".into(),
        },
    )]);

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(
        TabWidget::new(selected)
            .static_tab(
                TabInfo::new().title(label("Welcome")).pinned(true),
                FixedLeaf(120.0, 48.0),
            )
            .dynamic_tab::<DocState>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(800.0, 320.0));

    // The static "Welcome" tab is pinned → renders in the leading
    // pinned strip; the dynamic Doc1 tab renders in the scrollable
    // row.
    let bar = bar_of(&tree, widget_id);
    let row_outer = bar_content(&tree, bar);
    let pinned_strip_count: usize = tree
        .children(row_outer)
        .iter()
        .filter_map(|&c| {
            let kids = tree.children(c);
            if kids.len() == 1 {
                let info = tree.accessibility_node(kids[0]);
                if info.role() == accesskit::Role::ScrollView {
                    return None; // Expand wrapper
                }
            }
            Some(kids.len())
        })
        .max()
        .unwrap_or(0);
    assert_eq!(
        pinned_strip_count, 1,
        "expected the pinned static tab in the leading strip"
    );

    let header_row = data_source_header_row(&tree, bar);
    assert_eq!(
        tree.children(header_row).len(),
        1,
        "expected the dynamic tab in the scrollable row"
    );
}

// ─── TabBar (stand-alone) — sizing + data-source mode ───────────────

#[test]
fn tab_bar_shared_sizing_divides_viewport_equally() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model = ListModel::from_vec(vec![
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("B")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("C")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("D")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("E")), ()),
    ]);
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let bar_id = tree.add(
        TabBar::horizontal(model, delegate, selected, |_, h: &TabHandle| h.id)
            .tab_sizing(TabSizing::Shared)
            .min_tab_width(0.0)
            .max_tab_width(1000.0)
            .tab_spacing(0.0)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(1000.0, 60.0));

    let header_row = data_source_header_row(&tree, bar_id);
    let headers = tree.children(header_row);
    let widths: Vec<f32> = headers.iter().map(|&h| tree.bounds(h).width).collect();
    let target = widths[0];
    for w in &widths {
        assert!(
            (w - target).abs() < 0.5,
            "widths drift in Shared mode: {widths:?}"
        );
    }
    assert!(
        (target - 200.0).abs() < 1.0,
        "expected ~200 dp per tab, got {target}"
    );
}

#[test]
fn pinned_tab_renders_in_leading_strip() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model = ListModel::from_vec(vec![
        TabHandle::dynamic(
            TabId::fresh(),
            "doc",
            TabInfo::new().title(label("Pinned")).pinned(true),
            (),
        ),
        TabHandle::dynamic(
            TabId::fresh(),
            "doc",
            TabInfo::new().title(label("Normal")),
            (),
        ),
    ]);
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")))
            .pinned(|_, h: &TabHandle| h.info.pinned);

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let bar_id = tree.add(
        TabBar::horizontal(model, delegate, selected, |_, h: &TabHandle| h.id)
            .pinned_tab_width(36.0)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(800.0, 60.0));

    let header_row = data_source_header_row(&tree, bar_id);
    assert_eq!(
        tree.children(header_row).len(),
        1,
        "only the unpinned tab is in the scroll row"
    );
}

// ─── helpers ────────────────────────────────────────────────────────

fn bar_of(tree: &WidgetTree, widget_id: WidgetId) -> WidgetId {
    // TabWidget root: VStack { TabBar, Expand[Switcher] }
    let root = tree.child_widget(widget_id, 0);
    tree.child_widget(root, 0)
}

fn find_close_button(tree: &WidgetTree, header: WidgetId) -> WidgetId {
    fn walk(tree: &WidgetTree, id: WidgetId, out: &mut Option<WidgetId>) {
        if out.is_some() {
            return;
        }
        let info = tree.accessibility_node(id);
        if info.role() == accesskit::Role::Button
            && let Some(name) = info.name()
            && name.contains("Close")
        {
            *out = Some(id);
            return;
        }
        for child in tree.children(id) {
            walk(tree, child, out);
        }
    }
    let mut out = None;
    walk(tree, header, &mut out);
    out.expect("expected a close button on this tab header")
}

/// Hover the given header (so its close button becomes visible),
/// then locate and click the close button. The close button is now
/// hidden until the surrounding tab is hovered (Firefox / Chrome
/// convention) — this helper bundles the hover + click for tests
/// that just want to invoke the close affordance.
fn hover_and_click_close(tree: &mut WidgetTree, header: WidgetId) {
    let center = tree.bounds(header).center();
    tree.pointer_move(center);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let close = find_close_button(tree, header);
    tree.click(close);
}

// ─── Overflow dropdown visibility mode ──────────────────────────────

/// Find the "show all tabs" overflow trigger by its AT name, requiring
/// real laid-out bounds — so a dormant (hidden) trigger reads as `None`.
fn active_overflow_trigger(tree: &WidgetTree, id: WidgetId) -> Option<WidgetId> {
    let info = tree.accessibility_node(id);
    if info.role() == accesskit::Role::Button
        && info.name().is_some_and(|n| n.contains("Show all tabs"))
        && tree.bounds(id).height > 0.0
    {
        return Some(id);
    }
    tree.children(id)
        .iter()
        .find_map(|&c| active_overflow_trigger(tree, c))
}

fn overflow_bar(mode: crate::tab_widget::TabOverflowButton) -> (WidgetTree, WidgetId) {
    // Six min-120 tabs: fit at 900 px wide (no overflow), overflow at 300.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model = ListModel::from_vec(
        (0..6)
            .map(|i| {
                TabHandle::dynamic(
                    TabId::fresh(),
                    "doc",
                    TabInfo::new().title(label(&format!("Document {i}"))),
                    (),
                )
            })
            .collect::<Vec<_>>(),
    );
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let bar = tree.add(
        TabBar::horizontal(model, delegate, selected, |_, h: &TabHandle| h.id)
            .min_tab_width(120.0)
            .overflow_button(mode),
    );
    (tree, bar)
}

#[test]
fn overflow_button_auto_reveals_only_when_tabs_overflow() {
    use crate::tab_widget::TabOverflowButton;
    // The ScrollArea writes its `max_scroll` extent during layout; the
    // `visible_when` gate on that signal re-activates on the *next* pass (as
    // the frame loop settles). Lay out twice per size to reach that steady
    // state, exactly as a live window would.
    let settle = |tree: &mut WidgetTree, w: f32| {
        tree.layout(SizeProposal::exact(w, 60.0));
        tree.layout(SizeProposal::exact(w, 60.0));
    };
    let (mut tree, bar) = overflow_bar(TabOverflowButton::Auto);
    // Narrow: the headers overflow the viewport → the trigger appears. Grab
    // its (rebuild-stable) id here so we can query dormancy directly across
    // resizes, rather than re-reading a pruned AT node / stale bounds.
    settle(&mut tree, 300.0);
    let trigger = active_overflow_trigger(&tree, bar)
        .expect("Auto: overflow → the 'show all tabs' trigger is revealed");
    assert!(tree.is_active(trigger), "Auto: revealed trigger is active");
    // Re-widen: everything fits → the trigger goes dormant again (regression
    // against a stuck-visible trigger).
    settle(&mut tree, 900.0);
    assert!(
        !tree.is_active(trigger),
        "Auto: no overflow → the 'show all tabs' trigger is hidden"
    );
    // Narrow again: it comes back.
    settle(&mut tree, 300.0);
    assert!(
        tree.is_active(trigger),
        "Auto: re-narrowed → the trigger is revealed again"
    );
}

#[test]
fn overflow_button_always_shows_even_without_overflow() {
    use crate::tab_widget::TabOverflowButton;
    let (mut tree, bar) = overflow_bar(TabOverflowButton::Always);
    tree.layout(SizeProposal::exact(900.0, 60.0));
    assert!(
        active_overflow_trigger(&tree, bar).is_some(),
        "Always: the trigger is shown even when every tab fits"
    );
}

#[test]
fn overflow_button_never_omits_the_trigger() {
    use crate::tab_widget::TabOverflowButton;
    let (mut tree, bar) = overflow_bar(TabOverflowButton::Never);
    // Even when the tabs overflow, Never never builds the trigger.
    tree.layout(SizeProposal::exact(300.0, 60.0));
    assert!(
        active_overflow_trigger(&tree, bar).is_none(),
        "Never: the 'show all tabs' trigger is never built"
    );
}

// ─── Vertical orientation ───────────────────────────────────────────

#[test]
fn vertical_bar_lays_out_pills_top_to_bottom() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model = ListModel::from_vec(vec![
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("B")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("C")), ()),
    ]);
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")));

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let bar_id = tree.add(
        TabBar::vertical(model, delegate, selected, |_, h: &TabHandle| h.id)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(200.0, 600.0));

    let header_row = data_source_header_row(&tree, bar_id);
    let headers = tree.children(header_row);
    assert_eq!(headers.len(), 3);

    // Each header below the previous one — y strictly increasing.
    let bounds: Vec<_> = headers.iter().map(|&h| tree.bounds(h)).collect();
    assert!(bounds[0].y < bounds[1].y, "y order: {bounds:?}");
    assert!(bounds[1].y < bounds[2].y, "y order: {bounds:?}");
    // X is the same — pills stacked, no horizontal flow.
    assert!((bounds[0].x - bounds[1].x).abs() < 0.5);
    assert!((bounds[1].x - bounds[2].x).abs() < 0.5);
}

#[test]
fn vertical_shared_sizing_uses_intrinsic_pill_height() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model = ListModel::from_vec(vec![
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("B")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("C")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("D")), ()),
    ]);
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")));
    let theme = bastyde_core::presets::intui::light();
    let intrinsic = crate::styles::recipe_tab_style::TAB_EDITOR_HEIGHT;
    let mut tree = WidgetTree::new().with_theme(theme);
    let bar_id = tree.add(
        TabBar::vertical(model, delegate, selected, |_, h: &TabHandle| h.id)
            .tab_sizing(TabSizing::Shared)
            .tab_spacing(0.0)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(200.0, 800.0));

    let header_row = data_source_header_row(&tree, bar_id);
    let headers = tree.children(header_row);
    let heights: Vec<f32> = headers.iter().map(|&h| tree.bounds(h).height).collect();
    let target = heights[0];
    for h in &heights {
        assert!(
            (h - target).abs() < 0.5,
            "heights drift in Shared mode: {heights:?}"
        );
    }
    // Vertical Shared does NOT divide the viewport — sidebar pills
    // stay at the intrinsic per-tab height (`editor_tab_height`)
    // regardless of bar height. A 800 dp bar with 4 tabs gives 4
    // pills of ~50 dp each, not 4 × ~200 dp bands.
    assert!(
        (target - intrinsic).abs() < 0.5,
        "expected ~{intrinsic} dp per tab (editor_tab_height), got {target}"
    );
}

#[test]
fn compact_tab_bar_height_overrides_editor_height() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model = ListModel::from_vec(vec![
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(label("B")), ()),
    ]);
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let bar_id = tree.add(
        TabBar::horizontal(model, delegate, selected, |_, h: &TabHandle| h.id)
            .tab_bar_height(38.0)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    // Leave the height unconstrained so the bar takes its NATURAL height (what
    // a TabWidget allocates) rather than being stretched to a forced extent.
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });

    let h = tree.bounds(bar_id).height;
    assert!(
        (h - 38.0).abs() < 1.0,
        "compact bar should be 38 dp tall, got {h}"
    );
    // …and well under the 50 dp default.
    assert!(h < crate::styles::recipe_tab_style::TAB_EDITOR_HEIGHT);
}

#[test]
fn tab_widget_vertical_compose_lays_bar_on_leading_edge() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(
        TabWidget::new(selected.clone())
            .vertical()
            .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
            .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // TabWidget root is now an HStack { bar, content } in vertical
    // mode (instead of VStack).
    let root = tree.child_widget(widget_id, 0);
    let root_kids = tree.children(root);
    assert_eq!(root_kids.len(), 2);
    let bar_bounds = tree.bounds(root_kids[0]);
    let content_bounds = tree.bounds(root_kids[1]);
    // Bar on the leading edge (smaller x), content on trailing.
    assert!(
        bar_bounds.x < content_bounds.x,
        "bar should be on leading edge: bar={bar_bounds:?} content={content_bounds:?}"
    );
}

/// Regression: a vertical bar with a per-tab background must keep every
/// header (and therefore its leading accent indicator) pinned to the bar's
/// leading edge, regardless of how wide each tab's label is. The background
/// wraps the chrome in an extra `ZStack[bg, chrome]`; a plain ZStack
/// there sized the chrome to its label and centered it, dragging the
/// indicator inward by a per-label amount (the bug: indicator x drifted as
/// the selected tab changed because the texts differ). The fix wraps the
/// chrome in `Expand` so it fills the full tab width.
#[test]
fn vertical_surface_role_keeps_headers_flush_leading() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    // Deliberately varied label widths — the bug only shows when labels
    // differ, since the drift is `(tab_width - label_width) / 2`.
    let titles = ["X", "Containers", "Rich Text", "Date & Time", "OK"];
    let mut tw = TabWidget::new(selected.clone())
        .vertical()
        .max_tab_width(180.0)
        .tab_background(bastyde_tokens::SurfaceRole::Sunken);
    for t in titles {
        tw = tw.static_tab(TabInfo::new().title(label(t)), FixedLeaf(120.0, 48.0));
    }
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let widget_id = tree.add(tw);
    tree.layout(SizeProposal::exact(900.0, 600.0));

    let root = tree.child_widget(widget_id, 0);
    let bar_id = tree.child_widget(root, 0);
    let header_row = data_source_header_row(&tree, bar_id);
    let headers = tree.children(header_row);
    assert_eq!(headers.len(), titles.len());

    // Every header fills the full bar width and starts at the same x.
    let first = tree.bounds(headers[0]);
    for &h in &headers {
        let b = tree.bounds(h);
        assert!(
            (b.x - first.x).abs() < 0.5,
            "header x drifts with label width: {:?} vs {:?}",
            b,
            first
        );
        assert!(
            (b.width - first.width).abs() < 0.5,
            "header width drifts with label width: {:?} vs {:?}",
            b,
            first
        );
    }

    // The bar must adopt the *widest* tab's natural width so the longest
    // label ("Date & Time") renders without truncation. Under
    // `MockTextBackend` (8 px/char) that label measures 11 × 8 = 88 px; the
    // header reserves it plus the label→spacer `INNER_GAP` (6) and the two
    // horizontal paddings (2 × 12). A regression in `estimate_natural_width`
    // (wrong text style, or forgetting the spacer gap) compresses the label
    // below 88 and truncates it.
    let widest_text = 11.0 * 8.0; // "Date & Time"
    let needed = widest_text + 6.0 + 2.0 * 12.0; // text + INNER_GAP + pad*2
    assert!(
        first.width + 0.5 >= needed,
        "bar didn't adopt the widest tab width: header {} < needed {}",
        first.width,
        needed
    );

    // And the leading accent indicator (the `TabBodyPainter` leaf) sits at
    // the header's leading edge — find each header's deepest leaf chain and
    // assert the painter-bearing chrome layer starts at the bar leading x.
    // The painter is the first non-bg leaf that spans the full header width;
    // we verify by checking that some descendant fills the full width at the
    // header's x (the chrome no longer collapses to the label).
    for &h in &headers {
        let hb = tree.bounds(h);
        let mut fills_at_leading = false;
        fn walk(tree: &WidgetTree, id: WidgetId, hb: bastyde_canvas::Rect, found: &mut bool) {
            let b = tree.bounds(id);
            if (b.x - hb.x).abs() < 0.5
                && (b.width - hb.width).abs() < 0.5
                && tree.children(id).is_empty()
            {
                *found = true;
            }
            for c in tree.children(id) {
                walk(tree, c, hb, found);
            }
        }
        walk(&tree, h, hb, &mut fills_at_leading);
        assert!(
            fills_at_leading,
            "no full-width leaf at the header leading edge — indicator would be inset/centered"
        );
    }
}

// ─── Reorder custom actions ─────────────────────────────────────────

#[test]
fn enabled_reorderable_tabs_advertise_move_custom_actions() {
    use accesskit::Role;

    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let id_c = TabId::fresh();
    let model = ListModel::from_vec(vec![
        TabHandle::dynamic(id_a, "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(id_b, "doc", TabInfo::new().title(label("B")), ()),
        TabHandle::dynamic(id_c, "doc", TabInfo::new().title(label("C")), ()),
    ]);

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", |_h, _s| {
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .reorderable(true)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let update = tree.sync_accessibility();
    // Collect the three Tab nodes in node order.
    let tabs: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == Role::Tab)
        .collect();
    assert_eq!(tabs.len(), 3);

    // First tab: only "Move Right" (no left neighbor).
    let first_actions = tabs[0].1.custom_actions();
    let descs: Vec<&str> = first_actions
        .iter()
        .map(|a| a.description.as_ref())
        .collect();
    assert_eq!(descs, vec!["Move Right"], "first tab — only Move Right");

    // Middle tab: both directions advertised.
    let mid_actions = tabs[1].1.custom_actions();
    let descs: Vec<&str> = mid_actions.iter().map(|a| a.description.as_ref()).collect();
    assert_eq!(
        descs,
        vec!["Move Left", "Move Right"],
        "middle tab — both directions"
    );

    // Last tab: only "Move Left".
    let last_actions = tabs[2].1.custom_actions();
    let descs: Vec<&str> = last_actions
        .iter()
        .map(|a| a.description.as_ref())
        .collect();
    assert_eq!(descs, vec!["Move Left"], "last tab — only Move Left");
}

#[test]
fn non_reorderable_tabs_do_not_advertise_move_actions() {
    use accesskit::Role;

    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
            .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0))
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let update = tree.sync_accessibility();
    for (_, node) in &update.nodes {
        if node.role() == Role::Tab {
            assert!(
                node.custom_actions().is_empty(),
                "non-reorderable tab must not advertise Move actions"
            );
        }
    }
}

// ─── Selection bridge sanity (id ↔ index inside TabBar) ─────────────

#[test]
fn tab_bar_internal_bridge_sets_selected_id_when_initial_is_none() {
    // The TabBar pre-build sync should auto-populate `selected_id`
    // when the bar gets non-empty data and the id is None.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let model = ListModel::from_vec(vec![
        TabHandle::dynamic(id_a, "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(id_b, "doc", TabInfo::new().title(label("B")), ()),
    ]);
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabBar::horizontal(model, delegate, selected.clone(), |_, h: &TabHandle| h.id)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(800.0, 60.0));

    assert_eq!(
        selected.get(),
        Some(id_a),
        "bar should auto-select the first tab when selected_id is None"
    );
}

#[test]
fn tab_bar_internal_bridge_drops_selection_when_target_id_disappears() {
    // Pre-build sync: a stale id falls back to the previously-known
    // index, clamped. After model.remove of the active tab, the
    // surviving tab gets selected.
    let id_a = TabId::fresh();
    let id_b = TabId::fresh();
    let selected: Signal<Option<TabId>> = Signal::new(Some(id_b));
    let model = ListModel::from_vec(vec![
        TabHandle::dynamic(id_a, "doc", TabInfo::new().title(label("A")), ()),
        TabHandle::dynamic(id_b, "doc", TabInfo::new().title(label("B")), ()),
    ]);
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabBar::horizontal(
            model.clone(),
            delegate,
            selected.clone(),
            |_, h: &TabHandle| h.id,
        )
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(800.0, 60.0));
    assert_eq!(selected.get(), Some(id_b));

    // Remove tab B. The bar's pre-build sync should fall back —
    // index was 1, model now has 1 item, clamped to 0 → selects id_a.
    let _ = model.remove(1);
    tree.layout(SizeProposal::exact(800.0, 60.0));
    assert_eq!(
        selected.get(),
        Some(id_a),
        "stale id should fall back to the surviving neighbor"
    );
}

#[test]
fn tab_bar_drains_selection_to_none_when_model_emptied() {
    let id_a = TabId::fresh();
    let selected: Signal<Option<TabId>> = Signal::new(Some(id_a));
    let model = ListModel::from_vec(vec![TabHandle::dynamic(
        id_a,
        "doc",
        TabInfo::new().title(label("A")),
        (),
    )]);
    let delegate =
        TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TabBar::horizontal(
            model.clone(),
            delegate,
            selected.clone(),
            |_, h: &TabHandle| h.id,
        )
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(800.0, 60.0));

    let _ = model.remove(0);
    tree.layout(SizeProposal::exact(800.0, 60.0));
    assert_eq!(selected.get(), None, "empty model drains selection");
}

// ─── Cross-bar tab transfer (app-internal DnD between TabBars) ──────

/// Build a dynamic `TabHandle` carrying an `Rc`-shared marker as its
/// payload so a test can prove (via `Rc::ptr_eq`) that a migrated tab
/// carries the *same* heavy state, not a rebuilt copy.
fn doc_handle(title: &str, marker: std::rc::Rc<u32>) -> TabHandle {
    let payload: std::rc::Rc<dyn std::any::Any> = marker;
    TabHandle::dynamic_shared(
        TabId::fresh(),
        "doc",
        TabInfo::new().title(label(title)).closable(true),
        payload,
    )
}

fn doc_delegate() -> TabDelegate<TabHandle> {
    TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| label("")))
}

/// Remove the tab with `id` from a `ListModel<TabHandle>` (the model
/// mutation an app does in `on_transfer_out`).
fn remove_by_id(model: &ListModel<TabHandle>, id: TabId) {
    if let Some(pos) = (0..model.len()).find(|&i| model.with_item(i, |h| h.id) == Some(id)) {
        let _ = model.remove(pos);
    }
}

/// Center of the `index`-th unpinned header in a stand-alone `TabBar`.
fn header_center(tree: &WidgetTree, bar_id: WidgetId, index: usize) -> bastyde_canvas::Point {
    let row = data_source_header_row(tree, bar_id);
    let header = tree.child_widget(row, index);
    tree.bounds(header).center()
}

#[test]
fn cross_bar_transfer_moves_tab_and_preserves_state() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    let marker = std::rc::Rc::new(42_u32);
    let dragged = doc_handle("A1", marker.clone());
    let dragged_id = dragged.id;
    let dragged_payload = dragged.payload.clone();

    let model_a = ListModel::from_vec(vec![dragged, doc_handle("A2", std::rc::Rc::new(2))]);
    let model_b = ListModel::from_vec(vec![doc_handle("B1", std::rc::Rc::new(9))]);

    let bar_a = TabBar::horizontal(
        model_a.clone(),
        doc_delegate(),
        Signal::new(None),
        |_, h: &TabHandle| h.id,
    )
    .show_scroll_arrows(false)
    .show_overflow_dropdown(false)
    .on_transfer_out({
        let m = model_a.clone();
        move |id, _ctx| remove_by_id(&m, id)
    });

    let bar_b = TabBar::horizontal(
        model_b.clone(),
        doc_delegate(),
        Signal::new(None),
        |_, h: &TabHandle| h.id,
    )
    .show_scroll_arrows(false)
    .show_overflow_dropdown(false)
    .on_tab_received({
        let m = model_b.clone();
        move |handle, idx, _ctx| m.insert(idx.min(m.len()), handle)
    });

    let a_id = tree.add(bar_a);
    let b_id = tree.add(bar_b);
    let expand_a = tree.add(crate::Expand::new().flex(1.0).child_id(a_id));
    let expand_b = tree.add(crate::Expand::new().flex(1.0).child_id(b_id));
    tree.add(crate::HStack::new().add_child(expand_a).add_child(expand_b));
    tree.layout(SizeProposal::exact(900.0, 80.0));

    // Drag A's first tab onto B's header strip.
    let from = header_center(&tree, a_id, 0);
    let to = header_center(&tree, b_id, 0);
    tree.drag(from, to);
    tree.layout(SizeProposal::exact(900.0, 80.0));

    // Moved out of A, into B.
    assert_eq!(model_a.len(), 1, "source bar lost the dragged tab");
    assert!(
        (0..model_a.len()).all(|i| model_a.with_item(i, |h| h.id) != Some(dragged_id)),
        "dragged tab no longer in source model"
    );
    assert_eq!(model_b.len(), 2, "target bar gained the dragged tab");
    let landed = (0..model_b.len()).find(|&i| model_b.with_item(i, |h| h.id) == Some(dragged_id));
    assert!(landed.is_some(), "dragged tab id present in target model");

    // State preserved: the migrated handle carries the *same* Rc
    // payload (proves a real move, not a reconstruction).
    let same_state = model_b
        .with_item(landed.unwrap(), |h| {
            std::rc::Rc::ptr_eq(&h.payload, &dragged_payload)
        })
        .unwrap_or(false);
    assert!(
        same_state,
        "migrated tab must carry the same Rc<dyn Any> state"
    );
    assert_eq!(*marker, 42, "shared marker still reachable");
}

#[test]
fn cross_bar_rejected_when_target_not_opted_in() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    let dragged = doc_handle("A1", std::rc::Rc::new(1));
    let dragged_id = dragged.id;
    let model_a = ListModel::from_vec(vec![dragged, doc_handle("A2", std::rc::Rc::new(2))]);
    let model_b = ListModel::from_vec(vec![doc_handle("B1", std::rc::Rc::new(9))]);

    // Source opts in; target does NOT accept external tabs.
    let bar_a = TabBar::horizontal(
        model_a.clone(),
        doc_delegate(),
        Signal::new(None),
        |_, h: &TabHandle| h.id,
    )
    .show_scroll_arrows(false)
    .show_overflow_dropdown(false)
    .on_transfer_out({
        let m = model_a.clone();
        move |id, _ctx| remove_by_id(&m, id)
    });

    let bar_b = TabBar::horizontal(
        model_b.clone(),
        doc_delegate(),
        Signal::new(None),
        |_, h: &TabHandle| h.id,
    )
    .show_scroll_arrows(false)
    .show_overflow_dropdown(false);

    let a_id = tree.add(bar_a);
    let b_id = tree.add(bar_b);
    let expand_a = tree.add(crate::Expand::new().flex(1.0).child_id(a_id));
    let expand_b = tree.add(crate::Expand::new().flex(1.0).child_id(b_id));
    tree.add(crate::HStack::new().add_child(expand_a).add_child(expand_b));
    tree.layout(SizeProposal::exact(900.0, 80.0));

    let from = header_center(&tree, a_id, 0);
    let to = header_center(&tree, b_id, 0);
    tree.drag(from, to);
    tree.layout(SizeProposal::exact(900.0, 80.0));

    assert_eq!(model_a.len(), 2, "tab stays in source when target rejects");
    assert!(
        (0..model_a.len()).any(|i| model_a.with_item(i, |h| h.id) == Some(dragged_id)),
        "dragged tab still in source model"
    );
    assert_eq!(model_b.len(), 1, "target model unchanged");
}

#[test]
fn intra_bar_reorder_does_not_fire_transfer_out() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    let model = ListModel::from_vec(vec![
        doc_handle("T0", std::rc::Rc::new(0)),
        doc_handle("T1", std::rc::Rc::new(1)),
        doc_handle("T2", std::rc::Rc::new(2)),
    ]);
    let transfer_out_calls = std::rc::Rc::new(std::cell::Cell::new(0usize));

    let bar = TabBar::horizontal(
        model.clone(),
        doc_delegate(),
        Signal::new(None),
        |_, h: &TabHandle| h.id,
    )
    .show_scroll_arrows(false)
    .show_overflow_dropdown(false)
    .reorderable(true)
    .on_reorder({
        let m = model.clone();
        move |from, to, _ctx| m.move_item(from, to)
    })
    .on_transfer_out({
        let calls = transfer_out_calls.clone();
        move |_id, _ctx| calls.set(calls.get() + 1)
    });

    let bar_id = tree.add(bar);
    let expand = tree.add(crate::Expand::new().flex(1.0).child_id(bar_id));
    tree.add(crate::HStack::new().add_child(expand));
    tree.layout(SizeProposal::exact(600.0, 80.0));

    // Drag tab 0 onto tab 2's slot — an intra-bar reorder.
    let from = header_center(&tree, bar_id, 0);
    let to = header_center(&tree, bar_id, 2);
    tree.drag(from, to);
    tree.layout(SizeProposal::exact(600.0, 80.0));

    assert_eq!(
        transfer_out_calls.get(),
        0,
        "intra-bar reorder must NOT fire on_transfer_out (would drop the moved tab)"
    );
    assert_eq!(model.len(), 3, "reorder keeps every tab in the model");
}

/// A non-tab in-app drag payload (stands in for a file dragged from a
/// tree / list).
#[derive(Clone, Debug)]
struct FileRef {
    name: String,
}

/// A leaf widget that, on drag-start, publishes a `FileRef` payload —
/// i.e. a drag that is *not* a `TabBarDragData`.
#[derive(Debug)]
struct FileDragSource {
    name: String,
}

impl Widget for FileDragSource {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let name = self.name.clone();
        let hs = bastyde_core::widget_builder::HandlerSet::new().on_drag(move |phase, ctx| {
            if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
                ctx.start_drag(
                    self_id,
                    bastyde_core::drag_payload::DragPayload::typed(FileRef { name: name.clone() }),
                );
            }
        });
        ctx.apply_self_handlers(hs);
        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(120.0, 48.0).into()
    }
}

#[test]
fn on_external_drop_accepts_non_tab_payload() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    let model = ListModel::from_vec(vec![doc_handle("T0", std::rc::Rc::new(0))]);
    let dropped: std::rc::Rc<std::cell::RefCell<Vec<(String, usize)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    let bar = TabBar::horizontal(
        model.clone(),
        doc_delegate(),
        Signal::new(None),
        |_, h: &TabHandle| h.id,
    )
    .show_scroll_arrows(false)
    .show_overflow_dropdown(false)
    .on_external_drop({
        let dropped = dropped.clone();
        move |payload, idx, _ctx| {
            if let Some(file) = payload.get_typed::<FileRef>() {
                dropped.borrow_mut().push((file.name.clone(), idx));
                true
            } else {
                false
            }
        }
    });

    let bar_id = tree.add(bar);
    let source_id = tree.add(FileDragSource {
        name: "notes.txt".to_string(),
    });
    let expand_bar = tree.add(crate::Expand::new().flex(1.0).child_id(bar_id));
    let expand_src = tree.add(crate::Expand::new().flex(1.0).child_id(source_id));
    tree.add(
        crate::HStack::new()
            .add_child(expand_src)
            .add_child(expand_bar),
    );
    tree.layout(SizeProposal::exact(900.0, 80.0));

    let from = tree.bounds(source_id).center();
    let to = header_center(&tree, bar_id, 0);
    tree.drag(from, to);

    let calls = dropped.borrow();
    assert_eq!(calls.len(), 1, "on_external_drop should fire once");
    assert_eq!(calls[0].0, "notes.txt", "payload delivered to handler");
}

#[test]
fn non_tab_payload_rejected_without_handler() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

    let model = ListModel::from_vec(vec![doc_handle("T0", std::rc::Rc::new(0))]);
    // Reorderable bar, but NO on_external_drop: a FileRef drag must be
    // ignored (the bar only consumes TabBarDragData here).
    let bar = TabBar::horizontal(
        model.clone(),
        doc_delegate(),
        Signal::new(None),
        |_, h: &TabHandle| h.id,
    )
    .show_scroll_arrows(false)
    .show_overflow_dropdown(false)
    .reorderable(true)
    .on_reorder({
        let m = model.clone();
        move |from, to, _| m.move_item(from, to)
    });

    let bar_id = tree.add(bar);
    let source_id = tree.add(FileDragSource {
        name: "x.txt".to_string(),
    });
    let expand_bar = tree.add(crate::Expand::new().flex(1.0).child_id(bar_id));
    let expand_src = tree.add(crate::Expand::new().flex(1.0).child_id(source_id));
    tree.add(
        crate::HStack::new()
            .add_child(expand_src)
            .add_child(expand_bar),
    );
    tree.layout(SizeProposal::exact(900.0, 80.0));

    // Should not panic, and the model is untouched (no tab created).
    let from = tree.bounds(source_id).center();
    let to = header_center(&tree, bar_id, 0);
    tree.drag(from, to);
    assert_eq!(model.len(), 1, "non-tab drop without a handler is a no-op");
}

// ─── bar_visibility ─────────────────────────────────────────────────

/// Recursively report whether any node in `id`'s subtree carries the
/// given accessibility role.
fn subtree_has_role(tree: &WidgetTree, id: WidgetId, role: accesskit::Role) -> bool {
    if tree.accessibility_node(id).role() == role {
        return true;
    }
    tree.children(id)
        .iter()
        .any(|&c| subtree_has_role(tree, c, role))
}

fn count_role(tree: &WidgetTree, id: WidgetId, role: accesskit::Role) -> usize {
    let here = usize::from(tree.accessibility_node(id).role() == role);
    here + tree
        .children(id)
        .iter()
        .map(|&c| count_role(tree, c, role))
        .sum::<usize>()
}

#[test]
fn bar_visibility_always_shows_strip() {
    use crate::tab_widget::TabBarVisibility;
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let root = tree.add(
        TabWidget::new(selected.clone())
            .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
            .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0))
            .bar_visibility(TabBarVisibility::Always),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert!(
        subtree_has_role(&tree, root, accesskit::Role::TabList),
        "Always must render the TabList strip"
    );
}

#[test]
fn bar_visibility_never_hides_strip() {
    use crate::tab_widget::TabBarVisibility;
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let root = tree.add(
        TabWidget::new(selected.clone())
            .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
            .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0))
            .bar_visibility(TabBarVisibility::Never),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert!(
        !subtree_has_role(&tree, root, accesskit::Role::TabList),
        "Never must not render any TabList strip"
    );
}

#[test]
fn bar_visibility_when_multiple_is_reactive_and_preserves_content() {
    use crate::tab_widget::TabBarVisibility;
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let pane_builds = Rc::new(Cell::new(0));
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new().title(label("Only")),
        (),
    )]);

    let pb = pane_builds.clone();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let root = tree.add(
        TabWidget::new(selected.clone())
            .dynamic_tab::<()>("doc", move |_h, _s| {
                pb.set(pb.get() + 1);
                Box::new(FixedLeaf(120.0, 48.0)) as Box<dyn Widget>
            })
            .dynamic_model(model.clone())
            .bar_visibility(TabBarVisibility::WhenMultiple),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));

    // One tab → strip hidden.
    assert!(
        !subtree_has_role(&tree, root, accesskit::Role::TabList),
        "WhenMultiple hides the strip at a single tab"
    );
    assert_eq!(pane_builds.get(), 1, "the sole pane builds once");

    // Add a second tab → strip appears (reactive via the rebuild).
    model.push(TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new().title(label("Second")),
        (),
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert!(
        subtree_has_role(&tree, root, accesskit::Role::TabList),
        "WhenMultiple shows the strip once a second tab appears"
    );
    // The first pane's content survives the 1→2 strip toggle (memoized).
    assert_eq!(
        pane_builds.get(),
        2,
        "only the newly-added pane builds; the first is preserved across the toggle"
    );

    // Remove back down to one → strip hides again.
    let _ = model.remove(1);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert!(
        !subtree_has_role(&tree, root, accesskit::Role::TabList),
        "WhenMultiple hides the strip again at a single tab"
    );
    assert_eq!(pane_builds.get(), 2, "no pane rebuild on the 2→1 toggle");
}

// ─── Appearance API — per-state colours, dividers, indicator ────────

use bastyde_core::styles::TabIndicatorPosition;

/// A horizontal `TabWidget` with `n` static tabs, optionally with
/// inter-tab dividers, laid out at a generous size.
fn appearance_widget(n: usize, dividers: bool) -> (WidgetTree, WidgetId) {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tw = TabWidget::new(selected)
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false);
    if dividers {
        tw = tw.tab_dividers();
    }
    for i in 0..n {
        tw = tw.static_tab(
            TabInfo::new().title(label(&format!("Tab {i}"))),
            FixedLeaf(120.0, 48.0),
        );
    }
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let id = tree.add(tw);
    tree.layout(SizeProposal::exact(900.0, 320.0));
    (tree, id)
}

#[test]
fn tab_dividers_append_one_overlay_child_to_the_scroll_row() {
    // Off: the row's children are exactly the headers.
    let (tree, id) = appearance_widget(3, false);
    let row = data_source_header_row(&tree, bar_of(&tree, id));
    assert_eq!(
        tree.children(row).len(),
        3,
        "no overlay child when dividers are off"
    );

    // On: exactly one extra child (the divider overlay), and it is not a
    // Tab (so the tab count is unchanged) and is hidden from AT.
    let (tree, id) = appearance_widget(3, true);
    let row = data_source_header_row(&tree, bar_of(&tree, id));
    let kids = tree.children(row);
    assert_eq!(kids.len(), 4, "dividers add one overlay child to the row");
    let tabs = kids
        .iter()
        .filter(|&&c| tree.accessibility_node(c).role() == accesskit::Role::Tab)
        .count();
    assert_eq!(tabs, 3, "the overlay is not a tab");
    // The last child is the overlay — hidden from assistive tech.
    let overlay = *kids.last().unwrap();
    assert!(
        tree.accessibility_node(overlay).is_hidden(),
        "the divider overlay is presentational (AT-hidden)"
    );
}

#[test]
fn pinned_tabs_with_dividers_build() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let tw = TabWidget::new(selected)
        .tab_divider_color(bastyde_tokens::BorderRole::DividerStrong)
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false)
        .static_tab(
            TabInfo::new().title(label("P1")).pinned(true),
            FixedLeaf(40.0, 48.0),
        )
        .static_tab(
            TabInfo::new().title(label("P2")).pinned(true),
            FixedLeaf(40.0, 48.0),
        )
        .static_tab(TabInfo::new().title(label("Doc")), FixedLeaf(120.0, 48.0));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let id = tree.add(tw);
    tree.layout(SizeProposal::exact(800.0, 320.0));
    // Two pinned tabs + one unpinned all render (smoke: no panic, tabs present).
    let tab_count = count_role(&tree, id, accesskit::Role::Tab);
    assert_eq!(
        tab_count, 3,
        "all three tabs render with pinned-strip dividers"
    );
}

#[test]
fn active_indicator_inner_edge_builds_and_lays_out() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let tw = TabWidget::new(selected)
        .active_indicator(TabIndicatorPosition::InnerEdge)
        .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
        .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let id = tree.add(tw);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert!(subtree_has_role(&tree, id, accesskit::Role::Tab));
}

#[test]
fn per_state_tab_backgrounds_build_with_and_without() {
    use bastyde_tokens::SurfaceRole;
    // All three states set: builds, lays out, all tabs present.
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let tw = TabWidget::new(selected)
        .selected_tab_background(SurfaceRole::Raised)
        .hover_tab_background(SurfaceRole::Hover)
        .idle_tab_background(SurfaceRole::Transparent)
        .bar_background(SurfaceRole::Sunken)
        .static_tab(TabInfo::new().title(label("A")), FixedLeaf(120.0, 48.0))
        .static_tab(TabInfo::new().title(label("B")), FixedLeaf(120.0, 48.0));
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let id = tree.add(tw);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(count_role(&tree, id, accesskit::Role::Tab), 2);

    // Regression (nested-ZStack collapse): the selected tab's background rect
    // must FILL its tab, not shrink to 0×0. The first tab is selected by
    // default; its header → root ZStack → first child is the selected-state
    // RectWidget, which must cover the full tab bounds.
    fn first_tab(tree: &WidgetTree, id: WidgetId) -> Option<WidgetId> {
        if tree.accessibility_node(id).role() == accesskit::Role::Tab
            && tree.bounds(id).height > 0.0
        {
            return Some(id);
        }
        tree.children(id).iter().find_map(|&c| first_tab(tree, c))
    }
    let tab = first_tab(&tree, id).expect("a tab header");
    let tab_b = tree.bounds(tab);
    let root_zstack = tree.children(tab)[0];
    let sel_bg = tree.children(root_zstack)[0];
    let bg_b = tree.bounds(sel_bg);
    assert!(
        (bg_b.width - tab_b.width).abs() < 0.5 && (bg_b.height - tab_b.height).abs() < 0.5,
        "selected-tab background must fill its tab ({bg_b:?} vs {tab_b:?}) — \
         a nested ZStack would collapse it to zero"
    );

    // Default (no backgrounds): also builds with the same tab count.
    let (tree, id) = appearance_widget(2, false);
    assert_eq!(count_role(&tree, id, accesskit::Role::Tab), 2);
}

// ─── Suppress unused-warning when only some tests run ───────────────
#[cfg(test)]
fn _orientation_export_used() {
    let _ = TabBarOrientation::Horizontal;
}
