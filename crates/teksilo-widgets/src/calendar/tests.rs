// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;
use crate::common::datetime::Date;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

#[test]
fn single_calendar_builds_with_value() {
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn single_calendar_builds_with_none_value() {
    let mut tree = light_tree();
    let date = Signal::new(None::<Date>);
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn range_calendar_builds() {
    let mut tree = light_tree();
    let range = Signal::new(None::<DateRange>);
    let id = tree.add(Calendar::range(range));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    assert!(bounds.width > 0.0);
}

#[test]
fn calendar_role_is_grid() {
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::Grid);
}

#[test]
fn calendar_label_includes_month_and_year() {
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let info = tree.accessibility_node(id);
    let name = info.name().unwrap_or("");
    // Without a registered i18n manager (headless tests), the resolver
    // returns the Fluent key as a fallback. Either form satisfies the
    // structural assertion: month identifier + year present.
    assert!(
        (name.contains("May") || name.contains("may")) && name.contains("2026"),
        "got: {name}"
    );
}

#[test]
fn calendar_value_emits_iso_string_in_single_mode() {
    // `AccessibilityInfo` only exposes role/name/etc. — read the
    // value field directly off the AccessKit `TreeUpdate`. The
    // value composes the focused-cell ISO date with a "selected:"
    // suffix when a committed selection differs (or, here, matches);
    // both halves use ISO format for AT-friendly parsing.
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let update = tree.sync_accessibility();
    let target_node_id = teksilo_core::accessibility::widget_id_to_node_id(id);
    let (_, node) = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target_node_id)
        .expect("calendar node present in AT update");
    let value = node.value().unwrap_or_default();
    assert!(
        value.contains("2026-05-02"),
        "expected ISO date in value, got: {value}"
    );
    assert!(
        value.contains("selected"),
        "expected `selected: ...` suffix when value is set, got: {value}"
    );
}

#[test]
fn calendar_rebuilds_on_month_navigation() {
    // Mutating `visible_month` after the initial layout must
    // re-`build()` the calendar body, regenerating cells with the
    // new month's dates. Before the fix, BindingLevel::Relayout
    // only triggered measure, leaving cells stuck on the original
    // month's dates.
    use crate::common::datetime::types::YearMonth;
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let calendar = Calendar::single(date);
    let visible_month = calendar.visible_month_signal();
    let id = tree.add(calendar);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let initial_descendant_count = count_descendants(&tree, id);

    // Navigate forward a month — the day-grid widget should re-build
    // with the new month's cells. The descendant count should stay
    // the same (always 6 weeks × 7 days = 42 cells), but the cell
    // labels would change. Asserting on the count ensures the
    // rebuild path actually runs and doesn't accumulate stale nodes.
    visible_month.set(YearMonth::new(2026, 6));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let after_nav_count = count_descendants(&tree, id);
    // Counts may differ by exactly 1 because the today-cell ring is
    // an optional extra node and "today" lives in only one of the
    // two months. The key invariant is "no accumulation": rebuilds
    // shouldn't leak previous-month cells into the new tree, so the
    // total stays bounded near the expected ~290 nodes per month.
    assert!(
        (initial_descendant_count as i64 - after_nav_count as i64).abs() <= 1,
        "rebuild leaked nodes: {} → {}",
        initial_descendant_count,
        after_nav_count
    );
    // Also assert we're in the right ballpark — 6 rows × 7 cells +
    // header/footer/etc. for the day grid (~290), plus the dormant
    // Months and Years zoom grids (12 cells each, mounted but not
    // visible — Switcher mounts all children to avoid rebuild
    // churn on mode flips). Total lands around 520. The check is
    // a leak detector: as long as it's bounded, we're not piling up
    // stale per-month nodes across navigations.
    assert!(
        after_nav_count > 200 && after_nav_count < 800,
        "expected calendar descendant count in 200..800, got {after_nav_count}"
    );
}

fn count_descendants(tree: &WidgetTree, root: WidgetId) -> usize {
    let mut count = 0;
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        count += 1;
        queue.extend(tree.children(id));
    }
    count
}

/// Find a day cell by the AT name it publishes — the same way a screen
/// reader addresses it. `DayCell` is `Role::GridCell` and names itself
/// "<weekday> <month> <day>, <year>".
fn find_day_cell(tree: &WidgetTree, root: WidgetId, day: u8) -> WidgetId {
    let needle = format!(" {day}, ");
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        let node = tree.accessibility_node(id);
        if node.role() == teksilo_core::accesskit::Role::GridCell
            && node.name().is_some_and(|n| n.contains(&needle))
        {
            return id;
        }
        queue.extend(tree.children(id));
    }
    panic!("no day cell for day {day}");
}

/// A day cell advertises `Action::Click`; invoking it (screen reader,
/// automation bridge) must commit the date. Without a handler the cell is
/// announced as clickable and then does nothing — the cell is not
/// arena-disabled, so its `interactable` guard has to be honoured on the
/// AT path too.
#[test]
fn access_click_on_day_cell_commits_the_date() {
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = tree.add(Calendar::single(date.clone()));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });

    let cell = find_day_cell(&tree, cal, 17);
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(cell),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });

    assert_eq!(
        date.get(),
        Some(Date::constant(2026, 5, 17)),
        "AT click on a day cell must commit that date"
    );
}

#[test]
fn range_mode_first_commit_parks_anchor_second_commit_sets_value() {
    // The first commit in range mode parks the anchor without touching the
    // bound `value` signal; only the second commit publishes the range. We
    // drive the real widget through its keyboard path (the calendar root owns
    // `.focusable` + `.on_key`, and Enter commits the focused day) rather than
    // poking module-private internals — the second commit doubles as a
    // positive control proving the keystrokes actually land.
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

    let mut tree = light_tree();
    let value: Signal<Option<DateRange>> = Signal::new(None);
    let id = tree.add(Calendar::range(value.clone()));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    assert_eq!(value.get(), None, "no value before any commit");

    tree.focus(id);

    let press = |tree: &mut WidgetTree, key: Key| {
        tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers: Modifiers::NONE,
            text: None,
        });
    };

    // First Enter: park the anchor on the focused day. `value` must stay None
    // (observers shouldn't see a transient one-day range).
    press(&mut tree, Key::Enter);
    assert_eq!(
        value.get(),
        None,
        "first commit must park the anchor without publishing a range"
    );

    // Move focus, then a second Enter commits the range.
    press(&mut tree, Key::ArrowRight);
    press(&mut tree, Key::Enter);
    let committed = value.get();
    assert!(
        committed.is_some(),
        "second commit must publish a range (also proves the keystrokes landed)"
    );
    let range = committed.unwrap();
    assert!(
        range.start < range.end,
        "ArrowRight then commit should span two adjacent days, got {range:?}"
    );
}

#[test]
fn date_range_invariant() {
    let r = DateRange::new(Date::constant(2026, 5, 5), Date::constant(2026, 5, 1));
    assert!(r.start <= r.end);
    assert_eq!(r.start, Date::constant(2026, 5, 1));
    assert_eq!(r.end, Date::constant(2026, 5, 5));
}

#[test]
fn date_range_contains_inclusive() {
    let r = DateRange::new(Date::constant(2026, 5, 1), Date::constant(2026, 5, 5));
    assert!(r.contains(Date::constant(2026, 5, 1)));
    assert!(r.contains(Date::constant(2026, 5, 3)));
    assert!(r.contains(Date::constant(2026, 5, 5)));
    assert!(!r.contains(Date::constant(2026, 4, 30)));
    assert!(!r.contains(Date::constant(2026, 5, 6)));
}

// ── Header-zoom mode behaviour ──────────────────────────────────

#[test]
fn calendar_mode_default_is_days() {
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    assert_eq!(cal.mode_signal().get(), CalendarMode::Days);
}

#[test]
fn calendar_mode_demote_chain() {
    assert_eq!(CalendarMode::Days.demote(), CalendarMode::Months);
    assert_eq!(CalendarMode::Months.demote(), CalendarMode::Years);
    // Years is the coarsest; further demote is a no-op.
    assert_eq!(CalendarMode::Years.demote(), CalendarMode::Years);
}

#[test]
fn calendar_mode_signal_writable_for_programmatic_zoom() {
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    let mode = cal.mode_signal();
    mode.set(CalendarMode::Years);
    assert_eq!(mode.get(), CalendarMode::Years);
    let mut tree = light_tree();
    let _id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    // Mode persists across the build pass — read again to confirm.
    assert_eq!(mode.get(), CalendarMode::Years);
}

#[test]
fn years_grid_decade_calculation() {
    use crate::calendar::zoom_grid::YearsGrid;
    assert_eq!(YearsGrid::decade_of(2026), 2020);
    assert_eq!(YearsGrid::decade_of(2020), 2020);
    assert_eq!(YearsGrid::decade_of(2029), 2020);
    assert_eq!(YearsGrid::decade_of(2030), 2030);
    assert_eq!(YearsGrid::decade_of(1999), 1990);
    assert_eq!(YearsGrid::decade_of(0), 0);
}

#[test]
fn calendar_title_button_click_demotes_mode() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    let mode = cal.mode_signal();
    let mut tree = light_tree();
    let id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    assert_eq!(mode.get(), CalendarMode::Days);

    // Click in the horizontal centre of the calendar at a y near
    // the top — that's where the header label sits ("May 2026").
    let bounds = tree.bounds(id);
    let click_pos = Point::new(bounds.x + bounds.width / 2.0, bounds.y + 20.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        click_pos,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        click_pos,
        PointerButton::Primary,
        Modifiers::NONE,
    ));

    // After the click, mode should have demoted to Months.
    assert_eq!(
        mode.get(),
        CalendarMode::Months,
        "tap on header title should demote mode Days → Months; \
         got {:?}. The TitleButton's on_tap is not firing.",
        mode.get()
    );
}

#[test]
fn calendar_months_body_does_not_collapse_to_left_edge() {
    // Regression for the zoom-mode "body collapses to leading edge"
    // bug: in Months / Years mode, cells were rendered at zero
    // wanted-width, the row's natural width fell to ~spacing, and
    // the parent VStack assigned cross-axis width = wanted (small).
    // Visually the body shrank to a tiny column on the left.
    //
    // Fix: each zoom cell is wrapped in `FixedSize(cell_width,
    // cell_height)` so the row reports a real natural width.
    // Verify by checking that descendants of the calendar in zoom
    // mode have bounds spanning a meaningful fraction of the
    // calendar's width (not collapsed to ~zero).
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    let mode = cal.mode_signal();
    mode.set(CalendarMode::Months);
    let mut tree = light_tree();
    let id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    // Second pass settles visibility / activation flips.
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });

    // Walk the tree; the maximum-x cell across all descendants
    // should land beyond ~50% of the calendar's width if the months
    // grid distributes its cells. Pre-fix the rightmost descendant
    // sat near x=8 (just inside the outer padding).
    let cal_bounds = tree.bounds(id);
    let mut max_right: f32 = 0.0;
    let mut stack = vec![id];
    while let Some(node_id) = stack.pop() {
        let b = tree.bounds(node_id);
        if b.width > 0.0 {
            max_right = max_right.max(b.right());
        }
        for child in tree.children(node_id) {
            stack.push(child);
        }
    }
    let half_width = cal_bounds.x + cal_bounds.width * 0.5;
    assert!(
        max_right > half_width,
        "in Months mode the zoom body should distribute past the centre — \
         max-right child x = {max_right}, calendar mid x = {half_width}, \
         cal_bounds = {cal_bounds:?}"
    );
}

#[test]
fn calendar_title_button_clickable_across_centered_band() {
    // Verify the click target spans the full width between the
    // chevron buttons — the bug fix's whole point. Clicks at three
    // positions across the centred band (just-right-of-prev-chevrons,
    // dead-center, and just-left-of-next-chevrons) all flip mode.
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    let mode = cal.mode_signal();
    let mut tree = light_tree();
    let id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let bounds = tree.bounds(id);
    let header_y = bounds.y + 20.0;

    for pct in [0.30_f32, 0.50, 0.70] {
        // Reset mode each iteration (tests interact independently).
        mode.set(CalendarMode::Days);
        let click_pos = Point::new(bounds.x + bounds.width * pct, header_y);
        tree.dispatch_event(WidgetEvent::pointer_down(
            click_pos,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            click_pos,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        assert_eq!(
            mode.get(),
            CalendarMode::Months,
            "click at {pct:.0}% of header width should flip mode; \
             title button bounds don't span the centred band."
        );
    }
}

/// The label of the first weekday column header — the observable form of
/// the calendar's first-day-of-week.
fn first_weekday_header(tree: &mut WidgetTree, root: WidgetId) -> Option<String> {
    fn collect(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
        out.push(id);
        for c in tree.children(id) {
            collect(tree, c, out);
        }
    }
    let mut ids = Vec::new();
    collect(tree, root, &mut ids);

    let update = tree.sync_accessibility();
    ids.iter().find_map(|id| {
        let target = teksilo_core::accessibility::widget_id_to_node_id(*id);
        update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .filter(|(_, n)| n.role() == teksilo_core::accesskit::Role::ColumnHeader)
            .and_then(|(_, n)| n.label().map(|s| s.to_string()))
    })
}

#[test]
fn calendar_re_derives_its_first_day_of_week_when_the_locale_switches() {
    // Regression, same class as `DateEdit`: the first day of week is read
    // from the locale in `build()`, and `set_locale` only marks layout +
    // paint. en-US starts the week on Sunday, fr-FR on Monday.
    let mut tree = light_tree();
    tree.set_locale("en-US".to_string());
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(400.0),
    });
    let en = first_weekday_header(&mut tree, id).expect("weekday header");
    assert!(
        en.contains("sunday"),
        "en-US should start the week on Sunday; got `{en}`"
    );

    tree.set_locale("fr-FR".to_string());
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(400.0),
    });
    let fr = first_weekday_header(&mut tree, id).expect("weekday header");
    assert!(
        fr.contains("monday"),
        "fr-FR should start the week on Monday after the switch; got `{fr}`"
    );
}

#[test]
fn zoom_cell_label_is_hidden_from_accessibility_tree() {
    // `ZoomCell` (Role::GridCell, used by both MonthsGrid and YearsGrid)
    // already carries the cell's text as its own name, so the embedded
    // `TextWidget` inside it must not reach the AT tree as a second,
    // duplicate-named stop.
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = Calendar::single(date);
    cal.mode_signal().set(CalendarMode::Months);
    let mut tree = light_tree();
    let _id = tree.add(cal);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    // Second pass settles visibility / activation flips (same as
    // `calendar_months_body_does_not_collapse_to_left_edge`).
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let update = tree.sync_accessibility();

    // Without a registered i18n manager, resolution falls back to the
    // literal Fluent key — computed the same way `MonthsGrid::build`
    // computes the cell's own label, so this matches regardless of
    // whether a manager is installed.
    let expected =
        teksilo_i18n::resolve_message_widget(crate::common::datetime::month_long_key(5), &[]);

    let cell_node = update
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == teksilo_core::accesskit::Role::GridCell
                && node.label() == Some(expected.as_str())
        })
        .map(|(_, node)| node)
        .expect("May zoom cell present in AT update");
    assert_eq!(
        cell_node.label(),
        Some(expected.as_str()),
        "hiding the embedded label must not take the cell's own name away with it",
    );

    assert!(
        !update.nodes.iter().any(
            |(_, node)| node.role() == teksilo_core::accesskit::Role::Label
                && node.label() == Some(expected.as_str())
        ),
        "the zoom cell's embedded label must not survive as a duplicate-named node",
    );
}

// ---------------------------------------------------------------------------
// Touch
// ---------------------------------------------------------------------------

/// A day cell is a tap target, not a manipulator: its activation is `on_tap`,
/// so it already lands on the release, and its 32 dp box already clears the
/// 24 dp floor and follows the density ladder above it. A finger tap selects a
/// day, and that is the whole of what the controls sweep owes it.
#[test]
fn a_touch_tap_selects_a_day_on_release() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_core::pointer::PointerPhase;

    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let id = tree.add(Calendar::single(date.clone()));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    let cell = day_cell(&tree, id, 17).expect("the grid has day cells");
    assert!(
        cell.width >= 24.0 && cell.height >= 24.0,
        "a Compact day cell measured {cell:?}",
    );

    let before = date.get();
    let at = cell.center();
    let contact = finger();
    tree.dispatch_pointer(touch(contact, PointerPhase::Down, at, 0));
    assert_eq!(date.get(), before, "the press selects nothing");
    tree.dispatch_pointer(touch(contact, PointerPhase::Up, at, 30));
    assert_ne!(date.get(), before, "the release selects the day under it");
}

/// And the calendar deliberately does **not** declare `touch_action(NONE)`: a
/// finger that comes to rest on a day cell and then drags is scrolling the
/// surface the calendar sits in, because a day cell produces no value from the
/// press position and has no drag of its own to protect.
#[test]
fn a_finger_pan_over_the_calendar_scrolls_its_container() {
    use crate::button::press_test_support::{finger, touch};
    use teksilo_canvas::Point;
    use teksilo_core::event::EventResponse;
    use teksilo_core::pointer::PointerPhase;
    use teksilo_core::pointer::touch_action::{PanAxes, PanClaim};
    use teksilo_core::widget_builder::WidgetBuilder;

    let scrolled = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let count = scrolled.clone();
    let mut tree = light_tree();
    let date = Signal::new(Some(Date::constant(2026, 5, 2)));
    let cal = tree.add(Calendar::single(date.clone()));
    let _page = tree.add(
        crate::primitives::VStack::new()
            .child(cal)
            .scroll_container(PanAxes::BOTH)
            .pan_claim(PanClaim::vertical())
            .on_scroll(move |_e, _c| {
                count.set(count.get() + 1);
                EventResponse::Handled
            }),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(600.0),
    });
    let cell = day_cell(&tree, cal, 17).expect("the grid has day cells");
    let before = date.get();

    let contact = finger();
    let start = cell.center();
    tree.dispatch_pointer(touch(contact, PointerPhase::Down, start, 0));
    for (i, dy) in [40.0_f32, 90.0, 150.0].into_iter().enumerate() {
        let at = Point::new(start.x, start.y - dy);
        tree.dispatch_pointer(touch(contact, PointerPhase::Move, at, 20 + i as u64 * 20));
    }
    tree.dispatch_pointer(touch(
        contact,
        PointerPhase::Up,
        Point::new(start.x, start.y - 150.0),
        100,
    ));
    assert!(scrolled.get() > 0, "the pan never reached the scroller");
    assert_eq!(date.get(), before, "and it selected nothing on the way");
}

/// The nth `CalendarDayCell` in the grid, in layout order.
fn day_cell(
    tree: &WidgetTree,
    calendar: teksilo_core::widget_id::WidgetId,
    n: usize,
) -> Option<teksilo_canvas::Rect> {
    let mut cells: Vec<teksilo_canvas::Rect> = Vec::new();
    let mut stack = vec![calendar];
    while let Some(id) = stack.pop() {
        if tree
            .widget_type_name(id)
            .is_some_and(|t| t.contains("DayCell"))
        {
            cells.push(tree.bounds(id));
        }
        stack.extend(tree.children(id).iter().copied());
    }
    cells.sort_by(|a, b| {
        (a.y, a.x)
            .partial_cmp(&(b.y, b.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    cells.into_iter().nth(n)
}
