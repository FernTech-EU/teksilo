use super::*;
use fern_core::widget_tree::WidgetTree;
use fern_data::ListModel;
use fern_tokens::Theme;

// ─── Helpers ──────────────────────────────────────────────────────

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(Theme::light_default())
}

fn fruits() -> Vec<&'static str> {
    vec!["Apple", "Banana", "Cherry"]
}

// ─── Basic layout & role ──────────────────────────────────────────

#[test]
fn combo_box_builds_and_lays_out() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(
        ComboBox::new(fruits(), selected.clone()).placeholder("Select..."),
    );
    tree.layout(SizeProposal::exact(300.0, 50.0));
    let bounds = tree.bounds(cb);
    assert!(bounds.width >= 120.0);
    assert!(bounds.height >= 36.0);
}

#[test]
fn combo_box_accessibility_role() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(vec!["A", "B"], selected.clone()));
    tree.layout(SizeProposal::exact(200.0, 50.0));
    let info = tree.accessibility_node(cb);
    assert_eq!(info.role(), fern_core::accesskit::Role::ComboBox);
    assert!(!info.is_expanded());
}

#[test]
fn accessibility_exposes_label_via_set_name() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(
        ComboBox::new(vec!["Apple", "Banana"], selected.clone()).label("Fruit"),
    );
    tree.layout(SizeProposal::exact(200.0, 50.0));
    let info = tree.accessibility_node(cb);
    assert_eq!(info.name(), Some("Fruit"));
}

#[test]
fn accessibility_expanded_flips_on_open_close() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.focus(cb);

    assert!(!tree.accessibility_node(cb).is_expanded());

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert!(tree.accessibility_node(cb).is_expanded());

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert!(!tree.accessibility_node(cb).is_expanded());
}

#[test]
fn accessibility_expanded_resets_on_framework_dismiss() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.focus(cb);

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert!(tree.accessibility_node(cb).is_expanded());

    let overlay_id = tree
        .active_overlays()
        .first()
        .copied()
        .expect("dropdown overlay should be active");
    tree.dismiss_overlay(overlay_id);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert!(
        !tree.accessibility_node(cb).is_expanded(),
        "framework overlay dismiss must reset is_expanded() to false"
    );
}

// ─── Keyboard ─────────────────────────────────────────────────────

#[test]
fn arrow_keys_cycle_selection() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 50.0));
    tree.focus(cb);

    tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Banana"));

    tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Cherry"));

    tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Apple"));
}

#[test]
fn selected_updates_label() {
    let mut tree = light_tree();
    let selected = Signal::new(Some("Banana".to_string()));
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 50.0));
    assert!(tree.bounds(cb).width > 0.0);
}

#[test]
fn click_opens_overlay() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 200.0));

    assert!(tree.active_overlays().is_empty());

    tree.click(cb);
    tree.layout(SizeProposal::exact(300.0, 200.0));

    assert_eq!(tree.active_overlays().len(), 1);
}

#[test]
fn type_ahead_jumps_to_matching_item() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(
        vec!["Apple", "Banana", "Cherry", "Blueberry"],
        selected.clone(),
    ));
    tree.layout(SizeProposal::exact(300.0, 50.0));
    tree.focus(cb);

    tree.press_key(Key::B, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Banana"));

    tree.press_key(Key::L, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Blueberry"));
}

#[test]
fn type_ahead_with_character_key() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(
        vec!["100px", "200px", "300px"],
        selected.clone(),
    ));
    tree.layout(SizeProposal::exact(300.0, 50.0));
    tree.focus(cb);

    tree.press_key(Key::Character('2'), fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("200px"));
}

#[test]
fn type_ahead_case_insensitive() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 50.0));
    tree.focus(cb);

    tree.press_key(Key::C, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Cherry"));
}

#[test]
fn type_ahead_no_match_keeps_selection() {
    let mut tree = light_tree();
    let selected = Signal::new(Some("Banana".to_string()));
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 50.0));
    tree.focus(cb);

    tree.press_key(Key::Z, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Banana"));
}

#[test]
fn enter_toggles_dropdown_open_close() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.focus(cb);

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert_eq!(tree.active_overlays().len(), 1);

    tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Banana"));

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    assert!(tree.active_overlays().is_empty());
    assert_eq!(selected.get().as_deref(), Some("Banana"));
}

#[test]
fn escape_closes_dropdown() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.focus(cb);

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert_eq!(tree.active_overlays().len(), 1);

    tree.press_key(Key::Escape, fern_core::event::Modifiers::NONE);
    assert!(tree.active_overlays().is_empty());
}

#[test]
fn arrow_down_opens_dropdown_when_closed() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.focus(cb);

    tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert_eq!(tree.active_overlays().len(), 1);
    assert_eq!(selected.get().as_deref(), Some("Banana"));
}

#[test]
fn type_ahead_highlights_in_open_dropdown() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 300.0));
    tree.focus(cb);

    tree.click(cb);
    tree.layout(SizeProposal::exact(300.0, 300.0));
    assert_eq!(tree.active_overlays().len(), 1);
    let frame_before = tree.render();

    tree.press_key(Key::B, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Banana"));

    tree.layout(SizeProposal::exact(300.0, 300.0));
    let frame_after = tree.render();

    assert_ne!(frame_before.shapes, frame_after.shapes);
}

#[test]
fn below_preferred_opens_above_when_no_space() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 60.0));

    tree.click(cb);
    tree.layout(SizeProposal::exact(300.0, 60.0));

    assert_eq!(tree.active_overlays().len(), 1);

    let content_ids = tree.overlay_manager().active_content_ids();
    let overlay_bounds = tree.bounds(content_ids[0]);
    let cb_bounds = tree.bounds(cb);

    assert!(
        overlay_bounds.y + overlay_bounds.height <= cb_bounds.y + 5.0,
        "overlay should be positioned above when no space below"
    );
}

// ─── Model-backed & typed selection (new) ─────────────────────────

#[derive(Clone, Debug, PartialEq)]
struct Fruit {
    name: &'static str,
    emoji: &'static str,
}

fn fruit_list() -> Vec<Fruit> {
    vec![
        Fruit { name: "Apple", emoji: "🍎" },
        Fruit { name: "Banana", emoji: "🍌" },
        Fruit { name: "Cherry", emoji: "🍒" },
    ]
}

#[test]
fn typed_combo_box_renders_with_item_label() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<Fruit>);
    let cb = tree.add(
        ComboBox::from_items(fruit_list(), selected.clone(), |f: &Fruit| {
            f.name.to_string()
        })
        .placeholder("Pick a fruit"),
    );
    tree.layout(SizeProposal::exact(300.0, 50.0));
    assert!(tree.bounds(cb).width >= 120.0);
}

#[test]
fn model_backed_combo_reflects_insertions_via_clicks() {
    // Strong form of the insertion test: insert a new item, then
    // click it in the open dropdown and verify `selected` reflects
    // the click. Exercises the full observer → panel rebuild →
    // click path, not just the signal plumbing.
    let mut tree = light_tree();
    let model = ListModel::from_vec(vec!["Apple".to_string(), "Cherry".to_string()]);
    let selected = Signal::new(None::<String>);
    let cb = tree.add(
        ComboBox::from_model(model.clone(), selected.clone(), |s: &String| s.clone()),
    );
    tree.layout(SizeProposal::exact(300.0, 400.0));

    // Insert Banana between Apple and Cherry — model is now [Apple, Banana, Cherry].
    model.insert(1, "Banana".to_string());
    tree.layout(SizeProposal::exact(300.0, 400.0));

    // Open the dropdown.
    tree.click(cb);
    tree.layout(SizeProposal::exact(300.0, 400.0));
    assert_eq!(tree.active_overlays().len(), 1);

    // Find the newly-inserted "Banana" row via its accessibility name
    // and click it. If the panel did not rebuild on insertion, this
    // lookup would fail.
    let banana = tree
        .find_by_label("Banana")
        .expect("Banana row should be present in the dropdown");
    tree.click(banana);
    tree.layout(SizeProposal::exact(300.0, 400.0));

    assert_eq!(selected.get().as_deref(), Some("Banana"));
    // Clicking an item dismisses the overlay.
    assert!(tree.active_overlays().is_empty());
}

#[test]
fn model_backed_combo_resets_selection_on_remove() {
    let mut tree = light_tree();
    let model = ListModel::from_vec(vec![
        "Apple".to_string(),
        "Banana".to_string(),
        "Cherry".to_string(),
    ]);
    let selected = Signal::new(Some("Banana".to_string()));
    tree.add(
        ComboBox::from_model(model.clone(), selected.clone(), |s: &String| s.clone()),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));

    // Remove the selected item.
    model.remove(1);
    tree.layout(SizeProposal::exact(300.0, 200.0));

    // Selection should have been cleared by the observe hook.
    assert_eq!(selected.get(), None);
}

#[test]
fn typed_selection_survives_reorder() {
    let mut tree = light_tree();
    let model = ListModel::from_vec(fruit_list());
    let selected = Signal::new(Some(Fruit {
        name: "Banana",
        emoji: "🍌",
    }));
    tree.add(
        ComboBox::from_model(model.clone(), selected.clone(), |f: &Fruit| {
            f.name.to_string()
        }),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));

    // Move Banana from index 1 to index 0.
    model.move_item(1, 0);
    tree.layout(SizeProposal::exact(300.0, 200.0));

    // Selection unchanged — same T, regardless of index.
    assert_eq!(selected.get().map(|f| f.name), Some("Banana"));
}

#[test]
fn home_end_keys_jump_to_first_and_last() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(
        vec!["Apple", "Banana", "Cherry", "Date"],
        selected.clone(),
    ));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.focus(cb);

    tree.press_key(Key::End, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Date"));

    tree.press_key(Key::Home, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Apple"));
}

#[test]
fn dropdown_items_span_panel_width() {
    // Regression: DropdownItem::size_that_fits delegates to the root
    // ZStack, which queries children with UNSPECIFIED — so items
    // collapsed to the intrinsic width of their text label and
    // appeared as narrow centered stripes inside the wider dropdown
    // panel. The panel bg and RectWidgets filled the panel area but
    // the items (containing the labels) did not, producing a
    // visually-blank dropdown where only clicks that happened to
    // land on the narrow label strip changed the selection.
    //
    // Each item's bounds must match the panel's inner width
    // (accounting for the 4px outer padding on both sides).
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(
        vec!["Apple", "Banana", "Cherry"],
        selected.clone(),
    ));
    tree.layout(SizeProposal::exact(400.0, 500.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(400.0, 500.0));
    assert_eq!(tree.active_overlays().len(), 1);

    let content_ids = tree.overlay_manager().active_content_ids();
    let panel_width = tree.bounds(content_ids[0]).width;
    assert!(panel_width > 100.0, "panel should be reasonably wide");

    for name in ["Apple", "Banana", "Cherry"] {
        let id = tree
            .find_by_label(name)
            .unwrap_or_else(|| panic!("dropdown should contain {name}"));
        let w = tree.bounds(id).width;
        // Panel has 4px padding on each side — items should fill
        // the inner width (panel_width - 8).
        assert!(
            w >= panel_width - 10.0,
            "row {name} should span panel width: item={}, panel={}",
            w,
            panel_width
        );
    }
}

#[test]
fn dropdown_items_have_nonzero_bounds_when_open() {
    // Regression guard for a rendering bug where the dropdown panel
    // showed a blank surface: the item rows must each occupy a visible
    // rectangle after the overlay opens. Without this, the widget-
    // catalog demo regressed to an empty-looking dropdown even though
    // the logic tests all passed.
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(
        vec!["Apple", "Banana", "Cherry"],
        selected.clone(),
    ));
    tree.layout(SizeProposal::exact(300.0, 400.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(300.0, 400.0));
    assert_eq!(tree.active_overlays().len(), 1);

    for name in ["Apple", "Banana", "Cherry"] {
        let id = tree
            .find_by_label(name)
            .unwrap_or_else(|| panic!("dropdown should contain {name}"));
        let b = tree.bounds(id);
        assert!(
            b.width > 0.0 && b.height > 0.0,
            "{name} row should have nonzero bounds, got {:?}",
            b
        );
    }
}

#[test]
fn many_items_scroll_without_overflow_past_overlay() {
    // More items than max_visible_items (default 8): the dropdown
    // must cap at roughly max_visible * item_height, not grow to
    // fit every row.
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let many: Vec<String> = (0..20).map(|i| format!("Item {i}")).collect();
    let cb = tree.add(ComboBox::new(many, selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 800.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(300.0, 800.0));

    let content_ids = tree.overlay_manager().active_content_ids();
    let panel_bounds = tree.bounds(content_ids[0]);
    // 20 rows × 32px = 640px uncapped; expect well under that.
    assert!(
        panel_bounds.height < 400.0,
        "panel should be capped, was {}",
        panel_bounds.height
    );
    assert!(
        panel_bounds.height > 0.0,
        "panel should have visible height"
    );
}

#[test]
fn render_item_closure_receives_selection_snapshot_at_build() {
    // Documents current behavior: the `bool selected` passed to
    // `render_item` reflects the selection state at the moment the
    // dropdown panel was built. It is NOT automatically re-fired when
    // the selection changes while the dropdown is open — consumers
    // that need a reactive appearance should close over a Signal and
    // bind primitives directly. See `.render_item()` rustdoc.
    use std::sync::Mutex;
    let observed: Rc<Mutex<Vec<(String, bool)>>> = Rc::new(Mutex::new(Vec::new()));

    let mut tree = light_tree();
    let selected = Signal::new(Some("Banana".to_string()));
    let items = vec!["Apple".to_string(), "Banana".to_string()];
    let obs = observed.clone();
    let cb = tree.add(
        ComboBox::from_items(items, selected.clone(), |s: &String| s.clone())
            .render_item(move |item, is_selected| {
                obs.lock().unwrap().push((item.clone(), is_selected));
                Box::new(crate::primitives::MinSize::new(10.0, 10.0))
            }),
    );
    tree.layout(SizeProposal::exact(300.0, 300.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(300.0, 300.0));

    let calls = observed.lock().unwrap().clone();
    assert!(
        calls.contains(&("Apple".to_string(), false)),
        "Apple row should have been rendered with selected=false; got {:?}",
        calls
    );
    assert!(
        calls.contains(&("Banana".to_string(), true)),
        "Banana row should have been rendered with selected=true; got {:?}",
        calls
    );
}

#[test]
fn custom_render_item_used() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::SeqCst);

    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(
        ComboBox::new(fruits(), selected.clone()).render_item(|_item, _selected| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            // A distinctive leaf — a small fixed rect is enough to show
            // that our closure was called instead of the default row.
            Box::new(crate::primitives::MinSize::new(10.0, 10.0))
        }),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(300.0, 200.0));

    assert!(
        CALLS.load(Ordering::SeqCst) >= 3,
        "render_item should have been called at least once per item"
    );
}

// ─── Accessibility gap fixes ──────────────────────────────────────

/// Invoke `Widget::accessibility` on the widget at `id` and return the
/// resulting raw `accesskit::Node` for inspection of properties not
/// surfaced by `AccessibilityInfo` (placeholder, controls, auto_complete).
fn build_raw_a11y_node(
    tree: &mut WidgetTree,
    id: WidgetId,
) -> fern_core::accesskit::Node {
    use fern_core::accessibility::widget_id_to_node_id;
    let update = tree.sync_accessibility();
    let target = widget_id_to_node_id(id);
    update
        .nodes
        .into_iter()
        .find(|(node_id, _)| *node_id == target)
        .map(|(_, n)| n)
        .expect("accessibility node should be present for widget")
}

#[test]
fn accessibility_trigger_controls_popup() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 200.0));

    let node = build_raw_a11y_node(&mut tree, cb);
    assert!(
        !node.controls().is_empty(),
        "combo box trigger should point at its listbox via aria-controls"
    );
}

#[test]
fn accessibility_placeholder_when_no_selection() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(
        ComboBox::new(fruits(), selected.clone()).placeholder("Select a fruit"),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));

    let node = build_raw_a11y_node(&mut tree, cb);
    assert_eq!(node.placeholder(), Some("Select a fruit"));
    assert_eq!(node.value(), None);
}

#[test]
fn accessibility_value_when_selection_present() {
    let mut tree = light_tree();
    let selected = Signal::new(Some("Banana".to_string()));
    let cb = tree.add(
        ComboBox::new(fruits(), selected.clone()).placeholder("Select a fruit"),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));

    let node = build_raw_a11y_node(&mut tree, cb);
    assert_eq!(node.value(), Some("Banana"));
    assert_eq!(node.placeholder(), None);
}

// ─── Searchable mode (rich-text feature) ──────────────────────────

#[cfg(feature = "rich-text")]
#[test]
fn searchable_filters_list_to_matching_items() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let query = Signal::new(String::new());
    let cb = tree.add(
        ComboBox::new(
            vec!["Apple", "Banana", "Blueberry", "Cherry"],
            selected.clone(),
        )
        .search_query(query.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 500.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(400.0, 500.0));

    // All four items visible initially.
    for name in ["Apple", "Banana", "Blueberry", "Cherry"] {
        assert!(
            tree.find_by_label(name).is_some(),
            "expected {name} before filtering",
        );
    }

    // Set the query to "B" — only Banana and Blueberry should remain.
    query.set("B".to_string());
    tree.layout(SizeProposal::exact(400.0, 500.0));

    assert!(tree.find_by_label("Apple").is_none(), "Apple should be filtered out");
    assert!(tree.find_by_label("Cherry").is_none(), "Cherry should be filtered out");
    assert!(tree.find_by_label("Banana").is_some(), "Banana should still be visible");
    assert!(tree.find_by_label("Blueberry").is_some(), "Blueberry should still be visible");
}

#[cfg(feature = "rich-text")]
#[test]
fn searchable_custom_filter_is_consulted() {
    // Filter is called with (query, item). Route every item through
    // a closure that accepts only items whose label length equals
    // the query length — a contrived but easily-asserted predicate.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::SeqCst);

    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let query = Signal::new(String::new());
    let cb = tree.add(
        ComboBox::new(vec!["ab", "abc", "abcd"], selected.clone())
            .search_query(query.clone())
            .filter(|q, v: &String| {
                CALLS.fetch_add(1, Ordering::SeqCst);
                v.len() == q.len()
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 500.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(400.0, 500.0));

    query.set("xyz".to_string()); // length 3 → only "abc" matches
    tree.layout(SizeProposal::exact(400.0, 500.0));

    assert!(CALLS.load(Ordering::SeqCst) >= 3, "filter should have been called per item");
    assert!(tree.find_by_label("ab").is_none());
    assert!(tree.find_by_label("abc").is_some());
    assert!(tree.find_by_label("abcd").is_none());
}

#[cfg(feature = "rich-text")]
#[test]
fn accessibility_searchable_sets_autocomplete() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()).searchable(true));
    tree.layout(SizeProposal::exact(300.0, 200.0));

    let node = build_raw_a11y_node(&mut tree, cb);
    assert_eq!(
        node.auto_complete(),
        Some(fern_core::accesskit::AutoComplete::List),
        "searchable combobox must expose aria-autocomplete=list",
    );
}

#[test]
fn tab_dismisses_open_simple_dropdown() {
    // Non-searchable combos have focus on the trigger itself when
    // the dropdown is open, so the panel-level Tab handler doesn't
    // get a chance — the trigger's own `on_key` must intercept.
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(
        vec!["Apple", "Banana", "Cherry"],
        selected.clone(),
    ));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.focus(cb);

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert_eq!(tree.active_overlays().len(), 1);

    tree.press_key(Key::Tab, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert!(
        tree.active_overlays().is_empty(),
        "Tab must close the non-searchable dropdown too"
    );
}

#[test]
fn shift_tab_dismisses_open_simple_dropdown() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(
        vec!["Apple", "Banana", "Cherry"],
        selected.clone(),
    ));
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.focus(cb);

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    tree.press_key(Key::Tab, fern_core::event::Modifiers::SHIFT);
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert!(tree.active_overlays().is_empty());
}

#[cfg(feature = "rich-text")]
#[test]
fn tab_dismisses_open_searchable_dropdown() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let query = Signal::new(String::new());
    let cb = tree.add(
        ComboBox::new(vec!["Apple", "Banana", "Cherry"], selected.clone())
            .search_query(query.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 500.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(400.0, 500.0));
    assert_eq!(tree.active_overlays().len(), 1);

    tree.press_key(Key::Tab, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 500.0));
    assert!(
        tree.active_overlays().is_empty(),
        "Tab should dismiss the open dropdown so focus can leave the popup"
    );
}

#[cfg(feature = "rich-text")]
#[test]
fn shift_tab_dismisses_open_searchable_dropdown() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let query = Signal::new(String::new());
    let cb = tree.add(
        ComboBox::new(vec!["Apple", "Banana", "Cherry"], selected.clone())
            .search_query(query.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 500.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(400.0, 500.0));
    assert_eq!(tree.active_overlays().len(), 1);

    tree.press_key(Key::Tab, fern_core::event::Modifiers::SHIFT);
    tree.layout(SizeProposal::exact(400.0, 500.0));
    assert!(
        tree.active_overlays().is_empty(),
        "Shift+Tab should also dismiss the open dropdown"
    );
}

#[cfg(feature = "rich-text")]
#[test]
fn arrow_keys_navigate_filtered_list_from_search() {
    // Regression: while typing in the search field, ArrowDown /
    // ArrowUp must advance the selection through the currently
    // filtered items. Previously the arrow handling lived only on
    // the combo trigger, so bubble events from the search input
    // fell through the framework without moving the highlight.
    //
    // Home / End are deliberately NOT asserted here: `TextInput`
    // consumes them for caret-to-start / caret-to-end, which is
    // the expected text-field behavior.
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let query = Signal::new(String::new());
    let cb = tree.add(
        ComboBox::new(
            vec!["Apple", "Banana", "Blueberry", "Cherry"],
            selected.clone(),
        )
        .search_query(query.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 500.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(400.0, 500.0));

    // Narrow the filter to just the B-items, then navigate.
    query.set("B".to_string());
    tree.layout(SizeProposal::exact(400.0, 500.0));

    tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Banana"));

    tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Blueberry"));

    tree.press_key(Key::ArrowUp, fern_core::event::Modifiers::NONE);
    assert_eq!(selected.get().as_deref(), Some("Banana"));
}

#[cfg(feature = "rich-text")]
#[test]
fn enter_from_search_field_closes_dropdown() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let query = Signal::new(String::new());
    let cb = tree.add(
        ComboBox::new(vec!["Apple", "Banana"], selected.clone())
            .search_query(query.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 500.0));
    tree.click(cb);
    tree.layout(SizeProposal::exact(400.0, 500.0));
    assert_eq!(tree.active_overlays().len(), 1);

    tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 500.0));
    assert!(
        tree.active_overlays().is_empty(),
        "Enter from the search field should close the dropdown"
    );
}

#[cfg(feature = "rich-text")]
#[test]
fn searchable_opens_with_focus_in_search_field() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let query = Signal::new(String::new());
    let cb = tree.add(
        ComboBox::new(vec!["Apple", "Banana"], selected.clone())
            .search_query(query.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 500.0));

    tree.click(cb);
    tree.layout(SizeProposal::exact(400.0, 500.0));

    let focused = tree
        .focused()
        .expect("something inside the dropdown should be focused after open");
    assert_ne!(focused, cb, "focus must leave the combo trigger");
    // The focused widget should be inside the overlay content subtree.
    let overlay_root = tree.overlay_manager().active_content_ids()[0];
    let mut cur = Some(focused);
    let mut in_overlay = false;
    while let Some(id) = cur {
        if id == overlay_root {
            in_overlay = true;
            break;
        }
        cur = tree.parent(id);
    }
    assert!(
        in_overlay,
        "focused widget should be inside the dropdown panel"
    );
}

#[cfg(feature = "rich-text")]
#[test]
fn accessibility_non_searchable_omits_autocomplete() {
    let mut tree = light_tree();
    let selected = Signal::new(None::<String>);
    let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
    tree.layout(SizeProposal::exact(300.0, 200.0));

    let node = build_raw_a11y_node(&mut tree, cb);
    assert_eq!(
        node.auto_complete(),
        None,
        "non-searchable combobox must not advertise autocomplete",
    );
}
