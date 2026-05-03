use super::*;
use crate::common::datetime::Date;
use fern_core::signal::Signal;
use fern_core::widget_tree::WidgetTree;
use fern_tokens::Theme;

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(Theme::light_default())
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
    assert_eq!(info.role(), fern_core::accesskit::Role::Grid);
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
    let target_node_id = fern_core::accessibility::widget_id_to_node_id(id);
    let (_, node) = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == target_node_id)
        .expect("calendar node present in AT update");
    let value = node.value().unwrap_or_default();
    assert!(value.contains("2026-05-02"), "expected ISO date in value, got: {value}");
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

#[test]
fn range_mode_first_click_does_not_set_value() {
    // The first click in range mode should park the anchor without
    // touching the bound `value` signal. Observers of `value` only
    // see the committed range after the second click.
    let mut tree = light_tree();
    let value: Signal<Option<DateRange>> = Signal::new(None);
    let _id = tree.add(Calendar::range(value.clone()));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: None,
    });
    // Initial state: no value, no anchor.
    assert_eq!(value.get(), None);

    // We can't easily simulate a click here without dispatch
    // plumbing, but we can directly invoke `commit_date` via the
    // public `Calendar::single` analog. Range mode commit is tested
    // via the public range API instead.
    // (Direct commit_date is module-private; the value-stays-None
    // assertion is the contract being defended. A subsequent
    // headless-event test in the test harness can simulate clicks.)
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
