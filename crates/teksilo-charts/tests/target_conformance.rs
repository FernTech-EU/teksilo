// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Every target a chart owns, measured at all three densities — and a written
//! ruling on the one thing in this crate that looks like a target and is not.
//!
//! # The list, and how it was chosen
//!
//! `teksilo-charts` ships four widget types (`BarChart`, `LineChart`,
//! `PieChart` and `ChartLegend`, the last of which builds a private
//! `LegendRow` per series), and none of them implements either hit-widening
//! hook — `Widget::hit_outset` and `Widget::target_regions` appear nowhere in
//! the crate. So the selection rule cannot be "the widgets that declare a
//! mechanism", as it is for `teksilo-widgets`. It is instead: **every
//! construction that changes what is a target**, because in this crate that is
//! what varies.
//!
//! The crate does have a preview catalog — `preview_catalog.rs`, behind the
//! `preview` feature — but it holds exactly three entries, one per chart widget
//! in a default configuration, and none of the three switches below. Reusing it
//! would add nothing the explicit list has, so nothing here is feature-gated.
//!
//! Three switches do that, and every one of them is represented below:
//!
//! 1. **Whether the chart node is a target at all.** All three charts install
//!    their handler set behind `if self.show_hover_tooltip ||
//!    self.selection.is_some()`, so a chart built with `hover_tooltip(false)`
//!    and no selection installs no handler, is not focusable, and is not a
//!    pointer target. `bar_chart/decorative` measures that.
//! 2. **Whether the legend has targets.** `ChartLegend::interactive(false)` —
//!    the default — builds no child nodes at all; `interactive(true)` builds one
//!    focusable, tap-toggling `LegendRow` per series. Both are here, embedded
//!    and standalone, horizontal and vertical.
//! 3. **What the enclosing container is.** A chart with a readout installs a
//!    pointer handler on itself, which puts an eligible owner at distance zero
//!    on the bubble path of every press inside it — so the framework's
//!    miss-only slop pass cannot serve anything nested in a live chart. A
//!    standalone legend on a page has no such owner and *can* be served. The
//!    two cases measure differently and both are in the list.
//!
//! # The `Role::GraphicsObject` ruling
//!
//! A per-datum chart mark is **not** a pointer target this audit measures, and
//! [`a_chart_mark_is_not_a_target_the_audit_can_measure`] holds that claim to
//! four measurements rather than asserting it in prose. See that test for the
//! evidence and for what closing the blind spot would take.
//!
//! Reference: `docs/charts.md` §6 and §13, `docs/density-and-targets.md`.

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_charts::{
    BarChart, ChartDatum, ChartLegend, ChartModel, ChartSelection, ChartSeries, LegendOrientation,
    LegendPosition, LineChart, PieChart,
};
use teksilo_core::accessibility::target_audit::{
    AllowedViolation,
    PinnedDp::{ClearsFloor, Is},
    PinnedGeometry, SkipReason, TargetFixture, TargetMeasurement, TargetRule, TargetViolation,
    audit_fixtures, gate, measure_fixtures, measure_targets,
};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::SelectionMode;
use teksilo_i18n::lit;
use teksilo_tokens::{InputTokens, PenKind, PointerKind, TargetDensity};
use teksilo_widgets::Button;
use teksilo_widgets::primitives::{Padding, TextWidget, VStack};

// =========================================================================
// Scaffolding
// =========================================================================

fn model() -> ChartModel<String> {
    ChartModel::from_series_vec(vec![
        ChartSeries::new("Revenue").data(vec![
            ChartDatum::new("Q1".to_string(), 10.0),
            ChartDatum::new("Q2".to_string(), 25.0),
            ChartDatum::new("Q3".to_string(), 18.0),
            ChartDatum::new("Q4".to_string(), 30.0),
        ]),
        ChartSeries::new("Costs").data(vec![
            ChartDatum::new("Q1".to_string(), 6.0),
            ChartDatum::new("Q2".to_string(), 14.0),
            ChartDatum::new("Q3".to_string(), 11.0),
            ChartDatum::new("Q4".to_string(), 21.0),
        ]),
    ])
}

/// A chart on a page: the shape a chart actually ships in — a tile in a
/// dashboard, a figure in a report. A chart is 320 dp of surface, so there is no
/// "bare stack over-reports it" hazard for the chart node itself; the hazard is
/// for what is *nested* in it, and the chart's own pointer handler is what
/// denies the slop pass there.
fn on_a_page(
    tree: &mut WidgetTree,
    subject: impl teksilo_core::widget::Widget + 'static,
) -> WidgetId {
    tree.add(
        Padding::uniform(16.0).child(
            VStack::new()
                .spacing(12.0)
                .child(TextWidget::new(lit!("Quarterly")))
                .child(subject),
        ),
    )
}

/// The discriminating container: a tappable ancestor owns every near miss, so
/// nothing inside can be rescued by the slop pass. A chart tile in a
/// click-to-drill-down card is exactly this.
fn in_a_tappable_card(
    tree: &mut WidgetTree,
    subject: impl teksilo_core::widget::Widget + 'static,
) -> WidgetId {
    tree.add(
        Padding::uniform(16.0)
            .child(
                VStack::new()
                    .spacing(12.0)
                    .child(TextWidget::new(lit!("Quarterly")))
                    .child(subject),
            )
            .on_tap(|_, _| {}),
    )
}

// =========================================================================
// The fixture list
// =========================================================================

fn fixtures() -> Vec<TargetFixture> {
    vec![
        // ---- The chart node itself, per kind -----------------------------
        TargetFixture::new("bar_chart", |t| on_a_page(t, BarChart::new(model()))),
        TargetFixture::new("bar_chart/selection", |t| {
            on_a_page(
                t,
                BarChart::new(model()).selection(ChartSelection::new(SelectionMode::Multi)),
            )
        }),
        // A chart with neither a readout nor a selection installs no handler
        // set at all, so it is not a target. In the list so that a change which
        // made it one is measured rather than missed.
        TargetFixture::new("bar_chart/decorative", |t| {
            on_a_page(t, BarChart::new(model()).hover_tooltip(false))
        }),
        TargetFixture::new("line_chart", |t| on_a_page(t, LineChart::new(model()))),
        TargetFixture::new("line_chart/selection", |t| {
            on_a_page(
                t,
                LineChart::new(model()).selection(ChartSelection::new(SelectionMode::Multi)),
            )
        }),
        TargetFixture::new("pie_chart", |t| on_a_page(t, PieChart::new(model()))),
        TargetFixture::new("pie_chart/selection", |t| {
            on_a_page(
                t,
                PieChart::new(model()).selection(ChartSelection::new(SelectionMode::Multi)),
            )
        }),
        // ---- A widget nested inside a chart ------------------------------
        // The donut centre slot is the one place an app puts an ordinary
        // control inside a chart, and the chart's own pointer handler sits
        // above it — so this is where a small control gets no help.
        TargetFixture::new("pie_chart/donut_centre_button", |t| {
            on_a_page(
                t,
                PieChart::new(model())
                    .donut(0.55)
                    .center(Button::new(lit!("Total"))),
            )
        }),
        // ---- The legend, embedded ----------------------------------------
        TargetFixture::new("bar_chart/legend_static", |t| {
            on_a_page(
                t,
                BarChart::new(model())
                    .legend(true)
                    .legend_position(LegendPosition::Bottom),
            )
        }),
        TargetFixture::new("bar_chart/legend_interactive_bottom", |t| {
            on_a_page(
                t,
                BarChart::new(model())
                    .legend(true)
                    .legend_interactive(true)
                    .legend_position(LegendPosition::Bottom),
            )
        }),
        TargetFixture::new("bar_chart/legend_interactive_trailing", |t| {
            on_a_page(
                t,
                BarChart::new(model())
                    .legend(true)
                    .legend_interactive(true)
                    .legend_position(LegendPosition::Trailing),
            )
        }),
        TargetFixture::new("line_chart/legend_interactive_trailing", |t| {
            on_a_page(
                t,
                LineChart::new(model())
                    .legend(true)
                    .legend_interactive(true)
                    .legend_position(LegendPosition::Trailing),
            )
        }),
        // PieChart exposes `legend(bool)` and no `legend_interactive`, so its
        // legend has no target at any density. Measured, not assumed.
        TargetFixture::new("pie_chart/legend", |t| {
            on_a_page(t, PieChart::new(model()).legend(true))
        }),
        // ---- The legend, standalone --------------------------------------
        // `docs/charts.md` §6 names this as the escape hatch for the Compact
        // shortfall: "places a standalone legend outside the chart where it is
        // not inside a pointer-handling ancestor". Whether that actually clears
        // the floor is a measurement, and these two take it.
        TargetFixture::new("legend/standalone_horizontal", |t| {
            on_a_page(
                t,
                ChartLegend::new(model())
                    .interactive(true)
                    .orientation(LegendOrientation::Horizontal),
            )
        }),
        TargetFixture::new("legend/standalone_vertical", |t| {
            on_a_page(
                t,
                ChartLegend::new(model())
                    .interactive(true)
                    .orientation(LegendOrientation::Vertical),
            )
        }),
        TargetFixture::new("legend/in_a_tappable_card", |t| {
            in_a_tappable_card(
                t,
                ChartLegend::new(model())
                    .interactive(true)
                    .orientation(LegendOrientation::Vertical),
            )
        }),
        // ---- A chart inside something tappable ---------------------------
        TargetFixture::new("bar_chart/in_a_tappable_card", |t| {
            in_a_tappable_card(t, BarChart::new(model()))
        }),
    ]
}

// =========================================================================
// The gate
// =========================================================================

/// What an interactive legend row **paints** at Compact.
///
/// `legend_painted_line_height`: `LEGEND_SWATCH_SIZE` against the Tiny label's
/// line box. Written as a literal because a `const` table cannot call that
/// expression, and held to it by
/// [`the_pinned_legend_row_geometry_is_what_the_widget_computes`].
const LEGEND_ROW_PAINT: f32 = 13.2;

/// What the miss-only slop pass tops one up to where nothing denies it: the
/// paint plus **one** side of `(24 − paint) / 2`. The other side is the
/// neighbouring row, at distance zero.
const LEGEND_ROW_TOPPED_UP: f32 = 18.6;

/// This crate's gate installs **Int UI only**, which is a coverage boundary and
/// not an oversight: a chart carries no Tier-3 chrome from any shipped preset
/// (`ChartStyle`'s default impl lives in this crate, not in a theme), so a
/// preset changes a chart's geometry only through typography. Naming the theme
/// on the pin rather than leaving it implicit is what makes that boundary
/// visible, and what makes a preset added later a decision rather than a
/// silence.
const INTUI: &str = "intui.light";

static ROSTER: gate::Roster = gate::Roster {
    themes: &[gate::ThemeSubject {
        id: INTUI,
        theme: teksilo_core::presets::intui::light,
    }],
    allow: ALLOW_LIST,
};

/// Every conformance failure the fixture list produces, computed once.
fn census() -> Vec<TargetViolation> {
    gate::conformance_census(&fixtures(), &ROSTER)
}

/// **The seed.** One entry, for a shortfall this crate had already recorded in
/// prose before the audit existed, with the decision that produced it — and two
/// pinned geometries, because the row measures differently depending on whether
/// anything on its bubble path denies it the slop pass.
const ALLOW_LIST: &[AllowedViolation] = &[AllowedViolation {
    path: "LegendRow",
    measured: &[
        // Inside a chart, or in a tappable card: an eligible owner sits at
        // distance zero, so the row reaches exactly what it paints.
        PinnedGeometry {
            densities: &[TargetDensity::Compact],
            themes: &[INTUI],
            paints: (ClearsFloor, Is(LEGEND_ROW_PAINT)),
            reaches: (ClearsFloor, Is(LEGEND_ROW_PAINT)),
        },
        // Standalone on a page, or a bottom legend whose neighbour is not a
        // target: the pass tops the row up on one side and it is still short.
        PinnedGeometry {
            densities: &[TargetDensity::Compact],
            themes: &[INTUI],
            paints: (ClearsFloor, Is(LEGEND_ROW_PAINT)),
            reaches: (ClearsFloor, Is(LEGEND_ROW_TOPPED_UP)),
        },
    ],
    owner: "whoever takes the Compact-layout decision docs/charts.md \u{a7}6 defers",
    exception: Some("Equivalent"),
    why: "An interactive legend row paints a swatch beside a Tiny label -- the \
          pinned height above, which is `legend_painted_line_height` -- and \
          `legend_row_extent` raises it to the density's `target_size` at \
          Comfortable and Touch but leaves it at what it paints at Compact, \
          because raising it there is a Compact layout change and the \
          programme's invariant is that Compact layout does not move. That is \
          written down as a recorded shortfall in docs/charts.md SS6 and listed \
          again in SS13, with the reasoning that neither hit-widening mechanism \
          can reach the row where it sits -- which the two pins measure from \
          both sides: the slop pass reaches it only where nothing on the bubble \
          path denies it, and even then only from one side. WCAG 2.2 SC 2.5.8 \
          *Equivalent* discharges it: the same series is toggled by Space on \
          the focused row, and the row is a `Role::CheckBox` an assistive \
          technology can Focus and Click.",
}];

/// **The gate.** Zero conformance failures over the whole fixture list at all
/// three densities, except the written entry above.
#[test]
fn no_chart_target_falls_below_the_conformance_floor_at_any_density() {
    let failures = gate::conformance_failures(&census(), &ROSTER);
    assert!(
        failures.is_empty(),
        "{} chart target(s) below the 24 dp WCAG 2.2 SC 2.5.8 (AA) floor:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// Every allow-list entry, every geometry it pins, and every (theme, density)
/// pair it claims still matches something — and nothing it does not pin is
/// excused.
///
/// Both halves, because they ask opposite questions. An entry that outlives what
/// it excused is a hole waiting for the next control whose name contains the
/// same substring; an entry that matches something it should *not* is the same
/// hole from the other side, and it is the one a path-only matcher walks into.
/// Both are the shared matcher's, so this crate cannot drift from the other
/// gates on what either question means.
///
/// The third half is this crate's own: an entry that pins *nothing* at a density
/// where its path does produce a failure. The gate already reports that failure,
/// so this only names it better — but naming it better is the difference between
/// "widen the entry with a reason" and "add a pin and move on".
#[test]
fn no_chart_allow_list_entry_is_stale() {
    let census = census();
    let mut findings = gate::roster_defects(&census, &ROSTER);
    findings.extend(gate::stale_entries(&census, &ROSTER));
    // Ten Compact legend rows, four scalars each, and no other theme to seed
    // into — a smaller number means the census moved.
    findings.extend(gate::non_narrowing(&census, &ROSTER, 40));
    for entry in ALLOW_LIST {
        if let Some(name) = entry.exception
            && !entry.why.contains(name)
        {
            findings.push(format!(
                "`{}` claims the SC 2.5.8 *{name}* exception in a field its justification \
                 never mentions",
                entry.path,
            ));
        }
        for density in DENSITIES {
            let claimed = entry
                .measured
                .iter()
                .any(|p| p.densities.contains(&density));
            if claimed {
                continue;
            }
            if census
                .iter()
                .any(|v| v.density == density && v.path.contains(entry.path))
            {
                findings.push(format!(
                    "allow-list entry `{}` pins nothing at {density:?}, but there is a failure \
                     there — the gate is reporting it, which is correct; widen the entry only \
                     with a written reason and a measurement",
                    entry.path,
                ));
            }
        }
    }
    assert!(findings.is_empty(), "{}", findings.join("\n"));
}

/// The two pinned legend-row figures are the ones the widget computes, so the
/// literals in [`ALLOW_LIST`] cannot drift from the code they describe.
///
/// `legend_painted_line_height` is `pub(crate)`, so an integration test cannot
/// call it; what it *can* do is rebuild its expression from the two public
/// things it reads — `LEGEND_SWATCH_SIZE` and the Tiny role's line box — and
/// hold the literal to that.
#[test]
fn the_pinned_legend_row_geometry_is_what_the_widget_computes() {
    let typography = teksilo_core::presets::intui::light().typography;
    let label = teksilo_tokens::TextStyleRole::Tiny.resolve(&typography);
    let painted = teksilo_charts::style::LEGEND_SWATCH_SIZE.max(label.size * 1.2);
    assert!(
        (LEGEND_ROW_PAINT - painted).abs() <= 0.05,
        "the pinned legend-row paint is {LEGEND_ROW_PAINT}, the widget computes \
         {painted} — re-measure the allow-list entry",
    );
    let floor =
        teksilo_tokens::InputTokens::for_density(TargetDensity::Compact).min_target_conformance;
    let one_side = painted + (floor - painted) / 2.0;
    assert!(
        (LEGEND_ROW_TOPPED_UP - one_side).abs() <= 0.05,
        "the pinned topped-up reach is {LEGEND_ROW_TOPPED_UP}, one side of the \
         miss-only top-up gives {one_side}",
    );
}

const DENSITIES: [TargetDensity; 3] = [
    TargetDensity::Compact,
    TargetDensity::Comfortable,
    TargetDensity::Touch,
];

// =========================================================================
// The list is not a stub
// =========================================================================

/// What each fixture must be seen to measure. `None` says the fixture's subject
/// is deliberately not a pointer target.
const EXPECTATIONS: &[(&str, Option<&str>)] = &[
    ("bar_chart", Some("BarChart")),
    ("bar_chart/selection", Some("BarChart")),
    ("bar_chart/decorative", None),
    ("line_chart", Some("LineChart")),
    ("line_chart/selection", Some("LineChart")),
    ("pie_chart", Some("PieChart")),
    ("pie_chart/selection", Some("PieChart")),
    ("pie_chart/donut_centre_button", Some("Button")),
    ("bar_chart/legend_static", Some("BarChart")),
    ("bar_chart/legend_interactive_bottom", Some("LegendRow")),
    ("bar_chart/legend_interactive_trailing", Some("LegendRow")),
    ("line_chart/legend_interactive_trailing", Some("LegendRow")),
    ("pie_chart/legend", Some("PieChart")),
    ("legend/standalone_horizontal", Some("LegendRow")),
    ("legend/standalone_vertical", Some("LegendRow")),
    ("legend/in_a_tappable_card", Some("LegendRow")),
    ("bar_chart/in_a_tappable_card", Some("BarChart")),
];

#[test]
fn every_chart_fixture_measures_the_subject_it_names() {
    let fixtures = fixtures();
    assert_eq!(
        fixtures.len(),
        EXPECTATIONS.len(),
        "every fixture owes an expectation and every expectation a fixture",
    );
    let measured = measure_fixtures(&fixtures, TargetDensity::Compact);
    let mut problems = Vec::new();
    for fixture in &fixtures {
        let Some(expectation) = EXPECTATIONS.iter().find(|(n, _)| *n == fixture.name) else {
            problems.push(format!(
                "fixture `{}` has no entry in EXPECTATIONS — say what it must \
                 measure, or say that it is not a target",
                fixture.name,
            ));
            continue;
        };
        let prefix = format!("{}: ", fixture.name);
        let rows: Vec<&TargetMeasurement> = measured
            .iter()
            .filter(|m| m.path.starts_with(&prefix))
            .collect();
        match expectation.1 {
            Some(widget) => {
                if !rows.iter().any(|m| m.path.contains(widget)) {
                    problems.push(format!(
                        "fixture `{}` measured no `{widget}`; it measured: {}",
                        fixture.name,
                        rows.iter()
                            .map(|m| m.path.rsplit(" > ").next().unwrap_or("?").to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                }
            }
            None => {
                if !rows.is_empty() {
                    problems.push(format!(
                        "fixture `{}` is declared not a target but measured: {}",
                        fixture.name,
                        rows.iter()
                            .map(|m| m.path.rsplit(" > ").next().unwrap_or("?").to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// =========================================================================
// The ruling: is a chart mark a target?
// =========================================================================

/// **A per-datum chart mark is not a pointer target this audit can measure**,
/// and the reason is structural rather than a gap in the walker.
///
/// The audit has exactly two legs. One measures a node the arena reports as
/// taking a press; the other measures a region a widget reports from
/// `Widget::target_regions`. A mark is neither, and each half of that is
/// measured below:
///
/// * **It is not a node.** A mark reaches assistive technology as a synthetic
///   `Role::GraphicsObject` child pushed onto the chart's own
///   `AccessNodeBuilder` (`hit::emit_mark_node`). A synthetic AT child has an
///   `accesskit::NodeId` and no `WidgetId`, so there is nothing for
///   `WidgetArena::takes_a_press` to be asked about and nothing for the probe
///   to actuate. `hit::mark_tolerance` says this in as many words: *"all of a
///   chart's marks live inside the one chart node, the slop pass's product is a
///   node id, and a press inside the chart's own rectangle is not a miss."*
/// * **It is not a reported region.** No chart implements
///   `Widget::target_regions`, so the region leg finds nothing — asserted here
///   as the absence of any `part` row, which is what makes this test redden if
///   somebody implements the hook without revisiting the ruling.
///
/// The measurement that makes it a *finding* rather than a description: the
/// number of rows a chart fixture produces does not change when the number of
/// marks changes by an order of magnitude. Four data points and forty measure
/// the same targets.
///
/// **Is a mark a target in WCAG's sense?** Yes — it accepts a pointer action:
/// a press moves the readout, and where the chart has a `ChartSelection` a tap
/// selects the datum. But its size is the data's, not the designer's: a bar's
/// width is the category band divided by the series count, and a line point is
/// a fixed small dot, so no geometry the framework owns can raise it and SC
/// 2.5.8's *Essential* exception is the honest reading. What discharges it is
/// *Equivalent*: `hit::drive_readout_keys` moves the readout with the arrow
/// keys, Enter and Space commit the focused datum to the selection, and
/// `emit_mark_node` advertises `Click` and `ScrollIntoView` per datum with
/// `hit::handle_mark_action` implementing both.
///
/// **What closing the blind spot would take**, if a later package decides a
/// mark should be judged:
///
/// 1. `Widget::target_regions` on the three chart widgets, reporting one region
///    per mark. This is reporting-only — the pointer router does not read
///    `target_regions` at all (grep: no occurrence in
///    `widget_tree/pointer_router.rs`), so it is not the behaviour change P33
///    refused when it declined the wedge hit hooks.
/// 2. A paint-order problem that has to be solved first: mark geometry is
///    computed in `paint()` (`*self.marks.borrow_mut() = marks` in each chart's
///    `paint`), and the audit lays out without painting, so a
///    `target_regions` reading that cache would report an empty list in exactly
///    the tree the audit measures. Either the geometry moves to layout or the
///    audit learns to paint first.
/// 3. A ruling on the floor. A mark's reach is its rectangle plus a collar of
///    `HitSlop::for_pointer(kind, tokens).radius` (`hit::mark_tolerance`),
///    tie-broken to the nearest mark — 8 dp for a finger, 0 for a mouse. A
///    four-category chart clears 24 dp on that; a forty-category one cannot, at
///    any width a screen has. Judging marks against 24 dp would produce a
///    permanent violation per datum, which is a census, not a gate.
#[test]
fn a_chart_mark_is_not_a_target_the_audit_can_measure() {
    // (1) No chart reports a region, so the region leg of the walk finds
    //     nothing to attribute to a datum.
    //
    //     Asserted twice, and the second time is the one that bites. The
    //     fixture driver lays out and never paints, and a chart's mark
    //     geometry is computed in `paint` — so an unpainted tree would report
    //     no regions even from a chart that *did* implement
    //     `Widget::target_regions` over its mark cache. Measured: adding such
    //     an implementation to `BarChart` left every assertion in this file
    //     green. So the second half renders first, which populates the cache,
    //     and only then measures.
    //
    //     That is also a limitation of the harness worth naming: any
    //     `target_regions` implementation whose answer comes from a paint-time
    //     cache is invisible to `audit_fixtures`, which lays out and stops.
    let chart_only = vec![
        TargetFixture::new("bar", |t| t.add(BarChart::new(model()))),
        TargetFixture::new("line", |t| t.add(LineChart::new(model()))),
        TargetFixture::new("pie", |t| t.add(PieChart::new(model()))),
    ];
    let measured = measure_fixtures(&chart_only, TargetDensity::Touch);
    assert!(
        measured.iter().all(|m| m.part.is_none()),
        "a chart reported a target region from an unpainted tree: {:#?}",
        measured
            .iter()
            .filter(|m| m.part.is_some())
            .collect::<Vec<_>>(),
    );
    for painted in painted_chart_measurements() {
        assert!(
            painted.iter().all(|m| m.part.is_none()),
            "a chart reported a target region once its marks existed; the \
             GraphicsObject ruling in this test's doc comment has to be re-taken \
             before the assertions below mean anything: {:#?}",
            painted
                .iter()
                .filter(|m| m.part.is_some())
                .collect::<Vec<_>>(),
        );
    }

    // (2) Each chart contributes exactly its own node, whatever the data.
    assert_eq!(
        measured.len(),
        3,
        "three chart nodes and nothing else: {measured:#?}",
    );

    // (3) The count is independent of the number of marks. Ten times the data,
    //     the same targets — which is the whole claim, in one measurement.
    let dense = vec![
        TargetFixture::new("bar", |t| t.add(BarChart::new(wide_model()))),
        TargetFixture::new("line", |t| t.add(LineChart::new(wide_model()))),
        TargetFixture::new("pie", |t| t.add(PieChart::new(wide_model()))),
    ];
    assert_eq!(
        measure_fixtures(&dense, TargetDensity::Touch).len(),
        measured.len(),
        "the audit's row count followed the data, so something in the walk is \
         seeing marks after all",
    );

    // (4) And the collar a mark is actually reached by, so the figure cited in
    //     the doc comment above is asserted rather than remembered.
    for density in DENSITIES {
        let tokens = InputTokens::for_density(density);
        assert_eq!(
            teksilo_core::pointer::hit_slop::HitSlop::for_pointer(PointerKind::Mouse, &tokens)
                .radius,
            0.0,
            "a mouse earns no mark collar at {density:?}",
        );
        assert_eq!(
            teksilo_core::pointer::hit_slop::HitSlop::for_pointer(PointerKind::Touch, &tokens)
                .radius,
            8.0,
            "a finger's mark collar is the profile radius at {density:?}",
        );
    }
}

/// The three charts, laid out **and painted** once so the mark geometry each one
/// caches in `paint` exists, then measured.
///
/// The audit's own fixture driver never paints. Everything that reads a
/// paint-time cache is therefore invisible to it, which is why the blind-spot
/// test above takes its measurement here instead.
fn painted_chart_measurements() -> Vec<Vec<TargetMeasurement>> {
    let mut out = Vec::new();
    for kind in 0..3 {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light().with_density(TargetDensity::Touch));
        match kind {
            0 => {
                tree.add(BarChart::new(model()));
            }
            1 => {
                tree.add(LineChart::new(model()));
            }
            _ => {
                tree.add(PieChart::new(model()));
            }
        }
        tree.layout(SizeProposal::exact(400.0, 260.0));
        let _ = tree.render();
        out.push(measure_targets(&tree, TargetDensity::Touch));
    }
    out
}

/// Forty categories in one series — ten times `model`'s mark count.
fn wide_model() -> ChartModel<String> {
    ChartModel::from_series_vec(vec![
        ChartSeries::new("Revenue").data(
            (0..40)
                .map(|i| ChartDatum::new(format!("C{i}"), 5.0 + i as f32))
                .collect::<Vec<_>>(),
        ),
    ])
}

// =========================================================================
// What the switches actually do
// =========================================================================

/// A chart with neither a readout nor a selection is not a target — and one
/// with either is.
///
/// This is the switch every other fixture depends on: if a decorative chart
/// silently became focusable, the audit would start judging a 320 dp surface
/// and report nothing, while a decorative chart would have acquired a tab stop
/// nobody asked for.
#[test]
fn a_decorative_chart_is_not_a_target_and_a_live_one_is() {
    let live = measure_fixtures(
        &[TargetFixture::new("live", |t| {
            on_a_page(t, BarChart::new(model()))
        })],
        TargetDensity::Compact,
    );
    assert!(
        live.iter().any(|m| m.path.contains("BarChart")),
        "a chart with a readout is a target: {live:#?}",
    );

    let inert = measure_fixtures(
        &[TargetFixture::new("inert", |t| {
            on_a_page(t, BarChart::new(model()).hover_tooltip(false))
        })],
        TargetDensity::Compact,
    );
    assert!(
        !inert.iter().any(|m| m.path.contains("BarChart")),
        "a chart with no readout and no selection installs no handler set, so \
         it is not a pointer target: {inert:#?}",
    );
}

/// The legend row is the crate's one real target-size question, and it is a
/// **layout** answer: `legend_row_extent` leaves the row at what it paints at
/// Compact and raises it to the density's `target_size` above.
///
/// Asserted as an equality at each density rather than as a bound — including
/// at Compact, which is the density the finding is at and the one a bound
/// would have said nothing about. It used to read `height < target_size` there,
/// which a 2 dp row satisfies as happily as a 13 dp one: A10's fifth face, a
/// `>=` (or here a `<`) the fixture satisfies for an unrelated reason. The
/// Compact figure is [`LEGEND_ROW_PAINT`], pinned to the widget's own
/// expression by
/// [`the_pinned_legend_row_geometry_is_what_the_widget_computes`].
#[test]
fn an_interactive_legend_row_is_raised_by_layout_above_compact() {
    let fixtures = [TargetFixture::new("legend", |t| {
        on_a_page(
            t,
            ChartLegend::new(model())
                .interactive(true)
                .orientation(LegendOrientation::Vertical),
        )
    })];
    for density in DENSITIES {
        let rows: Vec<TargetMeasurement> = measure_fixtures(&fixtures, density)
            .into_iter()
            .filter(|m| m.path.contains("LegendRow"))
            .collect();
        assert_eq!(rows.len(), 2, "one row per series at {density:?}");
        let want = InputTokens::for_density(density).target_size;
        for row in &rows {
            if density == TargetDensity::Compact {
                assert!(
                    (row.size.height - LEGEND_ROW_PAINT).abs() <= 0.05,
                    "Compact keeps the row at exactly what it paints \
                     ({LEGEND_ROW_PAINT} dp): {row:#?}",
                );
                assert!(
                    LEGEND_ROW_PAINT < want,
                    "and that is below the floor, which is the finding",
                );
            } else {
                assert_eq!(
                    row.size.height, want,
                    "above Compact the row is given the density's target_size",
                );
            }
        }
    }
}

/// A live chart's own pointer handler denies the slop pass to everything nested
/// in it, which is why `docs/charts.md` §6 says an interactive legend's Compact
/// shortfall cannot be widened where it sits.
///
/// The measurement is a comparison, not an absolute: the same legend rows,
/// once inside a chart and once on a page, and the page copy must reach further
/// vertically. Compact is the density that shows it — above Compact the rows
/// are already at `target_size` and the slop pass has nothing to offer either
/// way, by the arithmetic in `HitSlop::outset_for`.
#[test]
fn a_live_charts_own_handler_denies_the_slop_pass_to_its_legend() {
    let embedded = measure_fixtures(
        &[TargetFixture::new("embedded", |t| {
            on_a_page(
                t,
                BarChart::new(model())
                    .legend(true)
                    .legend_interactive(true)
                    .legend_position(LegendPosition::Bottom),
            )
        })],
        TargetDensity::Compact,
    );
    let standalone = measure_fixtures(
        &[TargetFixture::new("standalone", |t| {
            on_a_page(
                t,
                ChartLegend::new(model())
                    .interactive(true)
                    .orientation(LegendOrientation::Horizontal),
            )
        })],
        TargetDensity::Compact,
    );

    let reach = |rows: &[TargetMeasurement]| -> f32 {
        rows.iter()
            .filter(|m| m.path.contains("LegendRow") && m.skipped.is_none())
            .map(|m| m.expanded.height)
            .fold(0.0_f32, f32::max)
    };
    let inside = reach(&embedded);
    let outside = reach(&standalone);
    assert!(
        outside > inside,
        "a standalone legend row must reach further than the same row inside a \
         live chart, or docs/charts.md §6's escape hatch is fiction — inside \
         {inside:.2}, outside {outside:.2}",
    );
}

// =========================================================================
// A deliberately undersized subject must fail
// =========================================================================

/// The harness's liveness proof for this crate, in two halves — because the two
/// can fail separately: a 10 dp tappable control in a card that is itself
/// tappable, beside a chart, must fail at every density (the *walker* measures),
/// and the same fixture through [`gate::conformance_census`] and
/// [`gate::conformance_failures`] must come back out (the *gate* reports). The
/// first half calls [`audit_fixtures`] directly, so on its own it would pass
/// with the matcher returning nothing.
///
/// **Why the subject is not in the donut's centre slot**, which would have been
/// the more pointed place for it: a `PieChart` gives its centre widget the whole
/// donut hole, so the slot cannot be undersized. A `Widget` whose
/// `layout_response` asks for 10 x 10 comes back well past the floor —
/// [`a_donut_centre_slot_is_stretched_to_the_hole`] measures it — which is also
/// why the `pie_chart/donut_centre_button` fixture's `Button` measures as a
/// conformant target rather than as a small one. The chart decides the slot's
/// size, so the only way a control inside a chart is undersized is if the
/// chart's own hole is.
#[test]
fn a_deliberately_undersized_subject_fails() {
    let undersized = vec![TargetFixture::new("undersized", |t| {
        t.add(
            Padding::uniform(16.0)
                .child(
                    VStack::new()
                        .spacing(12.0)
                        .child(BarChart::new(model()))
                        .child(Tiny),
                )
                .on_tap(|_, _| {}),
        )
    })];
    for density in DENSITIES {
        let violations = audit_fixtures(&undersized, density);
        assert!(
            violations
                .iter()
                .any(|v| v.path.contains("Tiny") && v.rule == TargetRule::MinTargetConformance),
            "a 10 dp control in a tappable card must fail at {density:?}: \
             {violations:#?}",
        );
    }

    // The second half: the gate's own path. Nothing in the allow-list names
    // this fixture, so every row of it must be reported.
    let census = gate::conformance_census(&undersized, &ROSTER);
    let reported = gate::conformance_failures(&census, &ROSTER);
    assert!(
        reported.iter().any(|f| f.contains("Tiny")),
        "the gate reports no failure for the undersized fixture: {reported:#?}",
    );
}

/// A `PieChart`'s centre slot is stretched to the donut hole, so a control put
/// there cannot be undersized — the measurement the test above rests on.
#[test]
fn a_donut_centre_slot_is_stretched_to_the_hole() {
    let rows = measure_fixtures(
        &[TargetFixture::new("donut", |t| {
            on_a_page(t, PieChart::new(model()).donut(0.55).center(Tiny))
        })],
        TargetDensity::Compact,
    );
    let tiny = rows
        .iter()
        .find(|m| m.path.contains("Tiny"))
        .expect("the centre slot is mounted and is a target");
    assert!(
        tiny.size.width > 24.0 && tiny.size.height > 24.0,
        "a 10 x 10 request came back as {:?} — the chart sizes the slot, not the \
         widget in it",
        tiny.size,
    );
    assert_eq!(tiny.rule, None, "and it is therefore conformant");
}

/// A 10 dp tappable leaf. Deliberately not built from `FixedSize` + `on_tap`:
/// the audit reports the *node that takes the press*, and a wrapper would put
/// that node's name in the path instead of this one's.
#[derive(Debug)]
struct Tiny;

impl teksilo_core::widget::Widget for Tiny {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(teksilo_core::widget_builder::HandlerSet::new().on_tap(|_, _| {}));
        Vec::new()
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        Size::new(10.0, 10.0).into()
    }
}

// =========================================================================
// The two things P33 refused, re-measured
// =========================================================================

/// P33 declined to give a pie chart the wedge hit hooks, on the measurement
/// that a chart is **one arena node**: a press in a corner of the chart's
/// rectangle resolves to the chart itself, and the slop outset a chart-sized
/// node earns is exactly zero at all three densities.
///
/// The second half is arithmetic and is asserted directly; the first is a hit
/// test and is asserted through the audit's own view of the tree — one
/// candidate for the whole chart, no descendants.
#[test]
fn a_chart_sized_node_earns_no_slop_outset_at_any_density() {
    let chart = Rect::new(0.0, 0.0, 320.0, 220.0);
    for density in DENSITIES {
        let tokens = InputTokens::for_density(density);
        for kind in [
            PointerKind::Mouse,
            PointerKind::Touch,
            PointerKind::Pen(PenKind::Pen),
        ] {
            assert_eq!(
                teksilo_core::pointer::hit_slop::HitSlop::for_pointer(kind, &tokens)
                    .outset_for(chart.size()),
                0.0,
                "a {kind:?} earns no outset on a {:.0}x{:.0} node at {density:?} \
                 — `up_to` is at most 44 dp and the formula is \
                 ((up_to - min(w,h))/2).clamp(0, radius)",
                chart.width,
                chart.height,
            );
        }
    }

    let mut tree = WidgetTree::new()
        .with_theme(teksilo_core::presets::intui::light().with_density(TargetDensity::Touch));
    tree.add(PieChart::new(model()));
    tree.layout(SizeProposal::exact(320.0, 220.0));
    let rows = measure_targets(&tree, TargetDensity::Touch);
    assert_eq!(
        rows.len(),
        1,
        "a pie chart is one node with no target sub-structure: {rows:#?}",
    );
    assert!(
        rows[0].skipped.is_none(),
        "and it is judged: {:#?}",
        rows[0]
    );
}

/// Nothing in this crate is skipped for a reason that would hide a real
/// shortfall.
///
/// `SkipReason` is how the walker says "measured, not judged", and every value
/// of it is a defensible non-judgement *for the right subject*. This test does
/// not forbid skips; it records which ones this crate produces, so a change
/// that starts skipping a control the gate used to judge is a test failure
/// rather than a silent drop in coverage.
#[test]
fn the_skips_this_crate_produces_are_the_ones_it_should() {
    let fixtures = fixtures();
    let mut seen: Vec<(String, SkipReason)> = Vec::new();
    for density in DENSITIES {
        for m in measure_fixtures(&fixtures, density) {
            if let Some(reason) = m.skipped {
                let subject = m.path.rsplit(" > ").next().unwrap_or("?").to_string();
                if !seen.iter().any(|(s, r)| *s == subject && *r == reason) {
                    seen.push((subject, reason));
                }
            }
        }
    }
    seen.sort_by(|a, b| (&a.0, format!("{:?}", a.1)).cmp(&(&b.0, format!("{:?}", b.1))));
    let named: Vec<String> = seen.iter().map(|(s, r)| format!("{s}:{r:?}")).collect();
    // Deliberately an exact set: a new skip is a coverage change and has to be
    // read, not absorbed.
    assert_eq!(
        named,
        EXPECTED_SKIPS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        "the set of measured-but-not-judged targets changed",
    );
}

/// The skips this crate produces, `<subject>:<SkipReason>`, sorted.
///
/// One entry, and it is the correct one: a donut's centre slot holds a real
/// widget target, and the chart's own centre — the point a user aims at when
/// aiming at the chart — is inside it, so the chart node delegates there and the
/// slot is judged in its place.
const EXPECTED_SKIPS: &[&str] = &["PieChart:DelegatesToDescendant"];

// =========================================================================
// The whole conformance story of this crate, as numbers
// =========================================================================

/// Every AA failure in this crate is an interactive legend row at `Compact`,
/// there are exactly ten of them across the list, and above `Compact` the crate
/// has **no** violation rows at all — not even the informational
/// `TouchTargetRecommendation`.
///
/// The counts are exact on purpose. The fixture list is the coverage claim, so a
/// change in the count is a change in what is covered and has to be read rather
/// than absorbed by a `>=`.
#[test]
fn the_only_conformance_failure_in_this_crate_is_the_compact_legend_row() {
    let fixtures = fixtures();

    let compact = audit_fixtures(&fixtures, TargetDensity::Compact);
    for v in &compact {
        assert!(
            v.path.contains("LegendRow") && v.rule == TargetRule::MinTargetConformance,
            "an unexpected violation shape at Compact: {v}",
        );
    }
    assert_eq!(
        compact.len(),
        10,
        "ten interactive legend rows across the list fall short at Compact; \
         the count changed:\n{}",
        compact
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    );

    for density in [TargetDensity::Comfortable, TargetDensity::Touch] {
        let rows = audit_fixtures(&fixtures, density);
        assert!(
            rows.is_empty(),
            "nothing in this crate falls short of any of the three floors at \
             {density:?}:\n{}",
            rows.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}

/// A legend row inside a tappable card gets **nothing** from either widening
/// mechanism: it reaches exactly what it paints, to the dp.
///
/// This is the discriminating half of the fixture list, stated as an equality.
/// The same row on a page reaches further, which is what
/// [`a_live_charts_own_handler_denies_the_slop_pass_to_its_legend`] compares —
/// but only an equality against the paint proves that the mechanisms gave
/// *nothing*, rather than merely less.
#[test]
fn a_legend_row_under_a_tappable_ancestor_reaches_exactly_what_it_paints() {
    let rows: Vec<TargetMeasurement> = measure_fixtures(
        &[TargetFixture::new("card", |t| {
            in_a_tappable_card(
                t,
                ChartLegend::new(model())
                    .interactive(true)
                    .orientation(LegendOrientation::Vertical),
            )
        })],
        TargetDensity::Compact,
    )
    .into_iter()
    .filter(|m| m.path.contains("LegendRow"))
    .collect();
    assert_eq!(rows.len(), 2, "one row per series");
    for row in &rows {
        // Within the walker's own boundary epsilon: the reach is found by
        // bisecting inside one probe step, so an exact edge is reported up to
        // PROBE_STEP / 2^PROBE_REFINE (about 0.03 dp) short of itself.
        assert!(
            (row.expanded.height - row.size.height).abs() <= 0.05,
            "the card owns every near miss, so the row reaches its paint and no \
             more: {row:#?}",
        );
        assert!(
            !row.sources.any(),
            "and neither mechanism is credited: {:?}",
            row.sources,
        );
    }
}
