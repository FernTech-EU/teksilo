// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;
use bastyde_canvas::MockTextBackend;
use bastyde_core::widget::LayoutContext;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_i18n::lit;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

// ── compute_overflow: the priority overflow algorithm (deterministic) ────────

#[test]
fn nothing_overflows_when_it_all_fits() {
    // 3 × 40 = 120 ≤ 200, no spacing → none collapsed, no chevron.
    let flags = compute_overflow(
        200.0,
        0.0,
        &[40.0, 40.0, 40.0],
        &[0, 0, 0],
        &[false; 3],
        0.0,
        3,
    );
    assert_eq!(flags, vec![false, false, false]);
}

#[test]
fn lowest_priority_overflows_first() {
    // avail 70, two 40px actions, priorities [10, 0]. Doesn't all fit (80>70),
    // so reserve the chevron and drop the lowest-priority action (#1).
    let flags = compute_overflow(70.0, 0.0, &[40.0, 40.0], &[10, 0], &[false; 2], 0.0, 2);
    assert_eq!(
        flags,
        vec![false, true],
        "the high-priority action stays inline"
    );
}

#[test]
fn ties_break_toward_the_last_declared() {
    // Equal priority → the later-declared action overflows first.
    let flags = compute_overflow(
        70.0,
        0.0,
        &[40.0, 40.0, 40.0],
        &[0, 0, 0],
        &[false; 3],
        0.0,
        3,
    );
    // avail 70: chevron(30) + one 40 = 70 fits; drop #2 then #1.
    assert_eq!(flags, vec![false, true, true]);
}

#[test]
fn always_overflow_actions_start_collapsed_even_with_room() {
    let flags = compute_overflow(1000.0, 0.0, &[40.0, 40.0], &[0, 0], &[false, true], 0.0, 2);
    assert_eq!(
        flags,
        vec![false, true],
        "always_overflow action stays in the menu"
    );
}

#[test]
fn always_overflow_does_not_distort_spacing_estimate() {
    // Regression guard: `total_slots` counts the `always_overflow` action,
    // but `inline_width` subtracts a slot for every collapsed entry — and an
    // `always_overflow` action starts collapsed — so it nets to zero and the
    // spacing estimate stays exact. With non-zero spacing and a tight fit the
    // boundary must land precisely (no over-collapse from a phantom gap).
    //
    // Two 40px actions, action #1 is always_overflow, spacing 10.
    //   inline = action0(40) + gap(10) + chevron(30) = 80
    let just_fits = compute_overflow(80.0, 0.0, &[40.0, 40.0], &[0, 0], &[false, true], 10.0, 2);
    assert_eq!(
        just_fits,
        vec![false, true],
        "action0 stays inline when the bar is exactly wide enough (80px)"
    );
    // One pixel narrower → action0 must also collapse (chevron-only, 30px).
    let too_tight = compute_overflow(79.0, 0.0, &[40.0, 40.0], &[0, 0], &[false, true], 10.0, 2);
    assert_eq!(
        too_tight,
        vec![true, true],
        "action0 collapses once the bar is below the exact-fit width"
    );
}

#[test]
fn pinned_width_reduces_room_for_actions() {
    // 160px of pinned content leaves little room → actions overflow.
    let flags = compute_overflow(200.0, 160.0, &[40.0, 40.0], &[0, 0], &[false; 2], 0.0, 4);
    // 160 + chevron(30) = 190; one 40 → 230 > 200 → both overflow.
    assert_eq!(flags, vec![true, true]);
}

// ── Integration: measurement-driven overflow + re-show + a11y ────────────────

fn themed_tree() -> WidgetTree {
    WidgetTree::new()
        .with_theme(bastyde_core::presets::intui::light())
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
}

fn many_actions(n: usize) -> Toolbar {
    let mut tb = Toolbar::new();
    for i in 0..n {
        tb = tb.action(
            ToolbarAction::new(lit!(format!("Action {i}")), || IconWidget::checkmark(16.0))
                .on_activate(|_| {}),
        );
    }
    tb
}

#[test]
fn wide_bar_shows_everything_narrow_bar_overflows_and_rewidening_restores() {
    let tb = many_actions(8);
    let overflowing = tb.is_overflowing();
    let mut tree = themed_tree();
    let _id = tree.add(tb);

    // Plenty of room → nothing overflows.
    tree.layout(SizeProposal::exact(2000.0, 50.0));
    assert!(!overflowing.get(), "wide bar should not overflow");

    // Squeeze → overflow kicks in.
    tree.layout(SizeProposal::exact(120.0, 50.0));
    assert!(
        overflowing.get(),
        "narrow bar should overflow into the chevron"
    );

    // Re-widen → overflow clears again. This only works because collapsed
    // items are still measurable via `measure_intrinsic` (no stale widths).
    tree.layout(SizeProposal::exact(2000.0, 50.0));
    assert!(
        !overflowing.get(),
        "re-widened bar should restore all actions inline"
    );
}

/// Probe that records what width the toolbar reports for a *constrained* width
/// proposal (the case a sizing parent issues).
#[derive(Debug)]
struct WidthProbe {
    target: WidgetId,
    out: Rc<Cell<f32>>,
}
impl Widget for WidthProbe {
    fn layout_response(
        &self,
        p: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let w = ctx
            .child_size(
                self.target,
                SizeProposal {
                    width: Some(300.0),
                    height: None,
                },
            )
            .map(|s| s.width)
            .unwrap_or(-1.0);
        self.out.set(w);
        p.resolve(0.0, 0.0).into()
    }
    fn cacheable_layout(&self) -> bool {
        false
    }
}

#[test]
fn toolbar_fills_offered_width_instead_of_reporting_natural() {
    // Regression: when given a bounded width by a sizing parent, the toolbar
    // must report THAT width (and overflow internally), not its much larger
    // natural content width — otherwise it spills outside its container.
    let tb = many_actions(8);
    let mut tree = themed_tree();
    let tb_id = tree.add(tb);
    let out = Rc::new(Cell::new(0.0));
    let _probe = tree.add(WidthProbe {
        target: tb_id,
        out: out.clone(),
    });
    tree.layout(SizeProposal::exact(2000.0, 50.0));
    assert!(
        (out.get() - 300.0).abs() < 0.5,
        "toolbar should fill the offered 300px, got {} (would spill its container)",
        out.get()
    );
}

#[test]
fn toolbar_has_toolbar_role_orientation_and_name() {
    let tb = Toolbar::new().label(lit!("Formatting")).action(
        ToolbarAction::new(lit!("Bold"), || IconWidget::checkmark(16.0)).on_activate(|_| {}),
    );
    let mut tree = themed_tree();
    let id = tree.add(tb);
    tree.layout(SizeProposal::exact(400.0, 50.0));
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), bastyde_core::accesskit::Role::Toolbar);
    assert_eq!(info.name(), Some("Formatting"));
}

#[test]
fn vertical_orientation_is_announced() {
    let tb = Toolbar::new()
        .orientation(ToolbarOrientation::Vertical)
        .action(ToolbarAction::new(lit!("A"), || IconWidget::checkmark(16.0)).on_activate(|_| {}));
    let mut tree = themed_tree();
    let id = tree.add(tb);
    tree.layout(SizeProposal::exact(60.0, 400.0));
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), bastyde_core::accesskit::Role::Toolbar);
}

#[test]
fn empty_toolbar_builds() {
    let mut tree = themed_tree();
    let id = tree.add(Toolbar::new());
    tree.layout(SizeProposal::exact(400.0, 50.0));
    assert!(tree.bounds(id).width > 0.0);
}

#[test]
fn arrow_keys_move_roving_focus_between_actions() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let tb = many_actions(4);
    let mut tree = themed_tree();
    let id = tree.add(tb);
    tree.layout(SizeProposal::exact(2000.0, 50.0)); // wide → nothing overflows

    let first = tree
        .first_focusable_descendant(id)
        .expect("a focusable toolbar control");
    tree.focus(first);
    assert_eq!(tree.focused(), Some(first), "focus seeded on first control");

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_ne!(
        tree.focused(),
        Some(first),
        "ArrowRight should move roving focus to the next action"
    );
}

#[test]
fn rtl_swaps_the_roving_arrow_direction() {
    // On a horizontal bar under RTL the layout mirrors, so ArrowLeft advances
    // (it is the "next" key) and ArrowRight steps back — the inverse of LTR.
    use bastyde_core::environment::LayoutDirection;
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let tb = many_actions(4);
    let mut tree = themed_tree();
    tree.set_layout_direction(LayoutDirection::RightToLeft);
    let id = tree.add(tb);
    tree.layout(SizeProposal::exact(2000.0, 50.0)); // wide → nothing overflows

    let first = tree
        .first_focusable_descendant(id)
        .expect("a focusable toolbar control");

    // RTL: ArrowLeft is "next" → advances off the first control.
    tree.focus(first);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowLeft,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_ne!(
        tree.focused(),
        Some(first),
        "RTL: ArrowLeft should advance the roving focus"
    );

    // RTL: ArrowRight is "previous" → from the first control it clamps, staying.
    tree.focus(first);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(
        tree.focused(),
        Some(first),
        "RTL: ArrowRight from the first control should stay put"
    );
}

#[test]
fn roving_tab_suppresses_a_composite_controls_inner_leaf() {
    // Regression for the "Tab gets stuck on the ComboBox" bug: a composite
    // control (here a ComboBox) exposes its focusable node as an inner leaf,
    // so when roving moves OFF it, Tab must no longer land on it.
    use crate::combo_box::ComboBox;
    use crate::icon_button::IconButton;
    use crate::primitives::icon_widget::IconWidget;
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    use bastyde_core::signal::Signal;

    let view_mode = Signal::new(Some("List".to_string()));
    let tb = Toolbar::new()
        .item(
            ToolbarItem::custom(ComboBox::new(["List", "Grid", "Columns"], view_mode)).overflow_as(
                ToolbarAction::new(lit!("View mode"), || IconWidget::checkmark(16.0))
                    .on_activate(|_| {}),
            ),
        )
        .item(
            ToolbarItem::custom(IconButton::new(IconWidget::checkmark(16.0)).tooltip(lit!("Ok")))
                .overflow_as(
                    ToolbarAction::new(lit!("Ok"), || IconWidget::checkmark(16.0))
                        .on_activate(|_| {}),
                ),
        )
        .action(
            ToolbarAction::new(lit!("New"), || IconWidget::checkmark(16.0)).on_activate(|_| {}),
        );

    let mut tree = themed_tree();
    let id = tree.add(tb);
    tree.layout(SizeProposal::exact(2000.0, 50.0)); // wide → nothing overflows

    let combo_leaf = tree
        .first_focusable_descendant(id)
        .expect("the ComboBox exposes a focusable leaf");
    tree.focus(combo_leaf);

    // Arrow off the ComboBox onto the next control.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    let roving_target = tree.focused();
    assert_ne!(roving_target, Some(combo_leaf), "arrow moved off the combo");

    // With roving now on a later control, Tab must keep focus on that single
    // roving stop — NOT fall back onto the ComboBox's leaked inner leaf.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(
        tree.focused(),
        roving_target,
        "Tab must stay on the single roving stop"
    );
    assert_ne!(
        tree.focused(),
        Some(combo_leaf),
        "the composite control's inner leaf must not leak into Tab"
    );
}

/// Count active ComboBox dropdown panels — a leaked, "ghost" open dropdown
/// shows up here. Zero means no dropdown is spuriously open.
fn active_dropdown_panels(tree: &WidgetTree) -> u32 {
    tree.widget_type_histogram()
        .iter()
        .filter(|(k, _)| k.contains("DropdownPanel"))
        .map(|(_, v)| *v)
        .sum()
}

#[test]
fn example_mix_overflow_rows_dormant_while_closed_across_resizes() {
    // Mirror the over-constraint example exactly (ComboBox + IconButton +
    // separator + 5 actions) and assert that no overflow menu row renders
    // while the chevron is closed, at several widths and across a resize
    // cycle. Counts active overflow-menu node types (MenuItem rows + an
    // embedded ComboBox row).
    use crate::combo_box::ComboBox;
    use crate::icon_button::IconButton;
    use crate::primitives::icon_widget::IconWidget;
    use bastyde_core::signal::Signal;

    let view_mode = Signal::new(Some("List".to_string()));
    let menu_mode = view_mode.clone();
    let tb = Toolbar::new()
        .item(
            ToolbarItem::custom(ComboBox::new(["List", "Grid", "Columns"], view_mode))
                .overflow_widget(move || {
                    Box::new(ComboBox::new(
                        ["List", "Grid", "Columns"],
                        menu_mode.clone(),
                    ))
                }),
        )
        .item(
            ToolbarItem::custom(IconButton::new(IconWidget::checkmark(16.0)).tooltip(lit!("Ok")))
                .overflow_as(
                    ToolbarAction::new(lit!("Confirm"), || IconWidget::checkmark(16.0))
                        .on_activate(|_| {}),
                ),
        )
        .item(ToolbarItem::separator())
        .action(
            ToolbarAction::new(lit!("New Document"), || IconWidget::checkmark(16.0))
                .on_activate(|_| {}),
        )
        .action(
            ToolbarAction::new(lit!("Open Recent Project"), || IconWidget::checkmark(16.0))
                .on_activate(|_| {}),
        )
        .action(
            ToolbarAction::new(lit!("Save Document As"), || IconWidget::checkmark(16.0))
                .on_activate(|_| {}),
        )
        .action(
            ToolbarAction::new(lit!("Export to PDF"), || IconWidget::checkmark(16.0))
                .on_activate(|_| {}),
        )
        .action(
            ToolbarAction::new(lit!("Print Preview"), || IconWidget::checkmark(16.0))
                .on_activate(|_| {}),
        );

    let mut tree = themed_tree();
    let _id = tree.add(tb);

    // Active overflow-menu rows = MenuItem rows (in the closed chevron menu).
    // (The embedded ComboBox row is gated the same way.)
    let active_menu_rows = |t: &WidgetTree| -> u32 {
        t.widget_type_histogram()
            .iter()
            .filter(|(k, _)| k.contains("menu_item::MenuItem"))
            .map(|(_, v)| *v)
            .sum()
    };

    for w in [900.0_f32, 500.0, 300.0, 160.0, 900.0, 240.0] {
        tree.layout(SizeProposal::exact(w, 48.0));
        assert_eq!(
            active_menu_rows(&tree),
            0,
            "no overflow menu row may render while the chevron is closed (width {w})"
        );
    }
}

#[test]
fn overflow_menu_rows_are_dormant_while_the_chevron_is_closed() {
    // Regression: the overflow menu's rows live in the (closed) chevron
    // popover. Even though each row's `item_when` gate is `true` while its
    // inline twin is collapsed, the row must NOT render until the popover is
    // opened — a `visible_when(true)` node inside a dormant ancestor stays
    // dormant.
    let tb = many_actions(6);
    let mut tree = themed_tree();
    let _id = tree.add(tb);
    tree.layout(SizeProposal::exact(120.0, 50.0)); // narrow → actions overflow

    let active_menu_items: u32 = tree
        .widget_type_histogram()
        .iter()
        .filter(|(k, _)| k.contains("menu_item::MenuItem"))
        .map(|(_, v)| *v)
        .sum();
    assert_eq!(
        active_menu_items, 0,
        "overflow menu rows must stay dormant until the chevron is opened"
    );
}

#[test]
fn collapsing_a_combobox_does_not_leak_its_dropdown_open() {
    // Regression for the "3 ghost option rows beneath the combo" bug: a
    // `ComboBox` gated by the toolbar's `visible_when` collapse must NOT have
    // its dropdown panel reactivated when the combo reappears.
    use crate::combo_box::ComboBox;
    use bastyde_core::signal::Signal;

    let view_mode = Signal::new(Some("List".to_string()));
    let tb = Toolbar::new()
        .item(
            ToolbarItem::custom(ComboBox::new(["List", "Grid", "Columns"], view_mode)).overflow_as(
                ToolbarAction::new(lit!("View"), || IconWidget::checkmark(16.0))
                    .on_activate(|_| {}),
            ),
        )
        .action(
            ToolbarAction::new(lit!("New"), || IconWidget::checkmark(16.0)).on_activate(|_| {}),
        );
    let mut tree = themed_tree();
    let _id = tree.add(tb);

    tree.layout(SizeProposal::exact(2000.0, 50.0)); // wide
    assert_eq!(active_dropdown_panels(&tree), 0, "closed dropdown at start");
    tree.layout(SizeProposal::exact(40.0, 50.0)); // narrow → combo collapses
    tree.layout(SizeProposal::exact(2000.0, 50.0)); // wide again → reappears
    tree.layout(SizeProposal::exact(2000.0, 50.0)); // settle
    assert_eq!(
        active_dropdown_panels(&tree),
        0,
        "no ComboBox dropdown should be open after collapse→reappear"
    );
}

#[test]
fn embedded_overflow_widget_combobox_does_not_leak_its_dropdown() {
    // Same guarantee for an embedded (`overflow_widget`) ComboBox living in
    // the chevron menu, exercised across a collapse/reappear cycle.
    use crate::combo_box::ComboBox;
    use bastyde_core::signal::Signal;

    let view_mode = Signal::new(Some("List".to_string()));
    let menu_mode = view_mode.clone();
    let tb = Toolbar::new()
        .item(
            ToolbarItem::custom(ComboBox::new(["List", "Grid", "Columns"], view_mode))
                .overflow_widget(move || {
                    Box::new(ComboBox::new(
                        ["List", "Grid", "Columns"],
                        menu_mode.clone(),
                    ))
                }),
        )
        .action(
            ToolbarAction::new(lit!("New"), || IconWidget::checkmark(16.0)).on_activate(|_| {}),
        );
    let mut tree = themed_tree();
    let _id = tree.add(tb);

    tree.layout(SizeProposal::exact(2000.0, 50.0));
    tree.layout(SizeProposal::exact(40.0, 50.0));
    tree.layout(SizeProposal::exact(2000.0, 50.0));
    tree.layout(SizeProposal::exact(2000.0, 50.0));
    assert_eq!(
        active_dropdown_panels(&tree),
        0,
        "neither the inline nor the embedded ComboBox dropdown should leak open"
    );
}

#[test]
fn overflow_widget_makes_a_custom_widget_collapsible() {
    // `overflow_widget` embeds a live widget in the menu; like `overflow_as`
    // it makes the custom collapse into the chevron when it doesn't fit.
    use crate::combo_box::ComboBox;
    use bastyde_core::signal::Signal;

    let view_mode = Signal::new(Some("List".to_string()));
    let menu_mode = view_mode.clone();
    let tb = Toolbar::new().item(
        ToolbarItem::custom(ComboBox::new(["List", "Grid", "Columns"], view_mode)).overflow_widget(
            move || {
                Box::new(ComboBox::new(
                    ["List", "Grid", "Columns"],
                    menu_mode.clone(),
                ))
            },
        ),
    );
    let overflowing = tb.is_overflowing();
    let mut tree = themed_tree();
    let _id = tree.add(tb);
    tree.layout(SizeProposal::exact(40.0, 50.0)); // far narrower than the combo
    assert!(
        overflowing.get(),
        "a widget-form collapsible should overflow into the menu when it doesn't fit"
    );
}

#[test]
fn a_collapsible_custom_overflows_when_it_does_not_fit() {
    use crate::primitives::TextWidget;
    // A custom widget made collapsible via `.overflow_as(..)` participates in
    // overflow just like an action — it collapses into the chevron menu.
    let tb = Toolbar::new().item(
        ToolbarItem::custom(TextWidget::new(lit!("A very wide collapsible widget"))).overflow_as(
            ToolbarAction::new(lit!("Wide"), || IconWidget::checkmark(16.0)).on_activate(|_| {}),
        ),
    );
    let overflowing = tb.is_overflowing();
    let mut tree = themed_tree();
    let _id = tree.add(tb);
    tree.layout(SizeProposal::exact(40.0, 50.0)); // far narrower than the widget
    assert!(
        overflowing.get(),
        "a collapsible custom should overflow into the menu when it doesn't fit"
    );
}

#[test]
fn a_pinned_custom_never_overflows() {
    use crate::primitives::TextWidget;
    // Without a menu form, a custom widget is pinned: it never collapses, so
    // there is no chevron even when it spills the bar.
    let tb = Toolbar::new().item(ToolbarItem::custom(TextWidget::new(lit!(
        "Wide pinned content"
    ))));
    let overflowing = tb.is_overflowing();
    let mut tree = themed_tree();
    let _id = tree.add(tb);
    tree.layout(SizeProposal::exact(40.0, 50.0));
    assert!(!overflowing.get(), "pinned customs never collapse");
}

#[test]
fn pinned_custom_widget_stays_inline_under_pressure() {
    use crate::primitives::TextWidget;
    // A pinned label + many actions in a narrow bar: the actions overflow but
    // the pinned widget is never collapsed (it's not an action).
    let tb = Toolbar::new()
        .item(ToolbarItem::custom(TextWidget::new(lit!("Search:"))))
        .action(ToolbarAction::new(lit!("One"), || IconWidget::checkmark(16.0)).on_activate(|_| {}))
        .action(ToolbarAction::new(lit!("Two"), || IconWidget::checkmark(16.0)).on_activate(|_| {}))
        .action(
            ToolbarAction::new(lit!("Three"), || IconWidget::checkmark(16.0)).on_activate(|_| {}),
        );
    let overflowing = tb.is_overflowing();
    let mut tree = themed_tree();
    let _id = tree.add(tb);
    tree.layout(SizeProposal::exact(110.0, 50.0));
    assert!(overflowing.get(), "actions should overflow in a tight bar");
}

#[test]
fn tooltip_appears_on_hover() {
    // A ToolbarAction with a plain tooltip forwards it to the inline Button.
    // Hovering the button and waiting out the delay should show one overlay.
    let tb = Toolbar::new().action(
        ToolbarAction::new(lit!("Save"), || IconWidget::checkmark(16.0))
            .tooltip(lit!("Tip"))
            .on_activate(|_| {}),
    );
    let mut tree = themed_tree();
    let id = tree.add(tb);
    tree.layout(SizeProposal::exact(300.0, 50.0));
    // The tooltip is attached to the inline Button, which is the first focusable
    // descendant of the toolbar.
    let btn = tree
        .first_focusable_descendant(id)
        .expect("toolbar action should be focusable");
    tree.pointer_move(tree.bounds(btn).center());
    tree.advance_time(std::time::Duration::from_secs(1));
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "tooltip should appear after hovering the toolbar action"
    );
    assert!(tree.find_by_label("Tip").is_some());
}

#[test]
fn compact_toolbar_reports_the_button_height_not_the_panel_padding() {
    // Regression: the toolbar's surface `Panel` must add no padding, else a
    // compact bar reports ~46 dp for a 22 dp button and spills a tight slot
    // (e.g. a dock header). A 2-action compact bar is exactly one button tall.
    #[derive(Debug)]
    struct HeightProbe {
        target: WidgetId,
        out: Rc<Cell<f32>>,
    }
    impl Widget for HeightProbe {
        fn layout_response(
            &self,
            p: SizeProposal,
            ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            let h = ctx
                .child_size(self.target, SizeProposal::unspecified())
                .map(|s| s.height)
                .unwrap_or(-1.0);
            self.out.set(h);
            p.resolve(0.0, 0.0).into()
        }
        fn cacheable_layout(&self) -> bool {
            false
        }
    }
    let bar = Toolbar::new()
        .compact(true)
        .action(ToolbarAction::new(lit!("A"), || {
            IconWidget::checkmark(16.0)
        }))
        .action(ToolbarAction::new(lit!("B"), || {
            IconWidget::checkmark(16.0)
        }));
    let mut tree = themed_tree();
    let id = tree.add(bar);
    let out = Rc::new(Cell::new(0.0));
    tree.add(HeightProbe {
        target: id,
        out: out.clone(),
    });
    tree.layout(SizeProposal::exact(400.0, 400.0));
    let compact_size = crate::styles::recipe_icon_button_style::ICON_BUTTON_SIZE_COMPACT;
    assert!(
        (out.get() - compact_size).abs() < 1.0,
        "compact toolbar should be one Compact button tall ({compact_size}), got {}",
        out.get()
    );
}

#[test]
fn compact_toolbar_overflows_gradually_and_re_expands() {
    // Regression (dock header): a *compact* toolbar measures its natural content
    // via `measure_intrinsic`, so items that have collapsed into the overflow
    // (and gone dormant) still count. Otherwise the measured content shrinks as
    // items collapse, the bar collapses wholesale, and it never re-expands when
    // the slot widens again.
    let mut tb = Toolbar::new().compact(true);
    for i in 0..6 {
        tb = tb.action(ToolbarAction::new(lit!(format!("A{i}")), || {
            IconWidget::checkmark(16.0)
        }));
    }
    let overflowed = tb.is_overflowing().clone();
    let mut tree = themed_tree();
    tree.add(tb);

    tree.layout(SizeProposal::exact(400.0, 30.0));
    assert!(!overflowed.get(), "wide: everything inline");

    tree.layout(SizeProposal::exact(70.0, 30.0));
    assert!(overflowed.get(), "narrow: overflow kicks in");

    tree.layout(SizeProposal::exact(400.0, 30.0));
    assert!(
        !overflowed.get(),
        "re-widened: the bar restores every action inline (the regression)"
    );
}

#[test]
fn menu_action_opens_a_dropdown_on_click() {
    // A ToolbarAction with `.menu(..)` renders its inline control as a
    // PopoverIconButton: clicking it opens the MenuList (not firing on_activate).
    use bastyde_core::event::PointerButton;
    let tb = Toolbar::new().action(
        ToolbarAction::new(lit!("Sort"), || IconWidget::checkmark(16.0))
            .menu(|| MenuList::new().item(MenuItem::new(lit!("By name")).on_activate_fn(|_| {}))),
    );
    let mut tree = themed_tree();
    let id = tree.add(tb);
    tree.layout(SizeProposal::exact(300.0, 50.0));
    let btn = tree
        .first_focusable_descendant(id)
        .expect("dropdown trigger should be focusable");
    let c = tree.bounds(btn).center();
    tree.pointer_down_button(c, PointerButton::Primary);
    tree.pointer_up_button(c, PointerButton::Primary);
    assert!(
        !tree.active_overlays().is_empty(),
        "clicking a menu action opens its dropdown popover"
    );
    assert!(
        tree.find_by_label("By name").is_some(),
        "the dropdown shows its menu items"
    );
}
