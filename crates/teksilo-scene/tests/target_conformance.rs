// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Every target a scene owns, measured at all three densities — across **both**
//! tiers, by two different instruments, because one instrument cannot see both.
//!
//! # Why this file has two halves
//!
//! `teksilo-scene` puts two kinds of thing under one view transform:
//!
//! * **Heavyweight** content is an ordinary `Widget` placed at scene
//!   coordinates. It is an arena node with a `WidgetId`, so it is an ordinary
//!   pointer target and the shared core walker
//!   (`teksilo_core::accessibility::target_audit`) measures it exactly as it
//!   measures a button on a page. The fixture list and the gate below are that.
//! * **Lightweight** content is a `SceneItem`: paint-only, no arena node, no
//!   `WidgetId`. Nothing the core walker can be asked about exists. Its presses
//!   are delivered by the view's *own* miss-only pass over an item snapshot
//!   (`GrabSlop` in `view.rs`, which delegates to the framework's
//!   `HitSlop::for_pointer` and so spends the same tokens), and its product is
//!   an `ItemId`.
//!
//! Skipping the second tier and calling the file "scene conformance" would be a
//! lie by omission — the lightweight tier is what a corkboard, a mind map and a
//! node graph are made of. So the second half **probes it through the real
//! pointer pipeline**: it dispatches touch contacts and mouse presses at
//! increasing distances from an item's centre and reports the contiguous
//! distance at which the item's own `on_tap` still fires. Same method as the
//! core walker (propose, then confirm against the code that actually routes a
//! press), a different door, and the file says which is which everywhere.
//!
//! [`a_lightweight_item_is_invisible_to_the_core_walker`] states the blind spot
//! and what closing it would take.
//!
//! # Why a fixture list had to be constructed
//!
//! `teksilo-scene` has **no preview catalog** — no `WidgetCatalog` impl, no
//! `doc_snippet!`, nothing to reuse and so nothing to gate on a `preview`
//! feature. The list below is built from the crate's own public surface instead:
//! the three widgets it exports that take a press (`SceneView`, `SceneMinimap`,
//! `SceneScrollView`), both content tiers, the view state that changes a
//! target's *screen* size (zoom), and the one grab affordance the crate invents
//! (a magnet handle).
//!
//! Reference: `docs/teksilo-scene.md`, `docs/density-and-targets.md`.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::target_audit::{
    AllowedViolation,
    PinnedDp::{ClearsFloor, Is},
    PinnedGeometry, TargetFixture, TargetMeasurement, TargetRule, audit_fixtures, measure_fixtures,
    measure_targets,
};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::pointer::{
    BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
    PointerSample,
};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_scene::{
    Magnet, MagnetRef, MagnetRole, MagnetVerdict, MagnetismConfig, RectItem, Scene, SceneMinimap,
    SceneView,
};
use teksilo_tokens::{Color, InputTokens, PointerKind, TargetDensity};
use teksilo_widgets::Button;
use teksilo_widgets::primitives::{FixedSize, Padding, TextWidget, VStack};

const DENSITIES: [TargetDensity; 3] = [
    TargetDensity::Compact,
    TargetDensity::Comfortable,
    TargetDensity::Touch,
];

// =========================================================================
// Scaffolding
// =========================================================================

/// A square tappable widget of a caller-chosen side — the shape a node-graph
/// port button or a scene-embedded close affordance is drawn as, and small
/// enough that the framework's widening mechanisms have something to do.
#[derive(Debug)]
struct Port(f32);

impl teksilo_core::widget::Widget for Port {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(HandlerSet::new().on_tap(|_, _| {}));
        Vec::new()
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        Size::new(self.0, self.0).into()
    }
}

/// A scene under a `SceneView` filling the fixture viewport, with the view at
/// the window origin so scene coordinates and window coordinates coincide at
/// unit zoom — which is what makes every number below readable.
fn view_of(tree: &mut WidgetTree, scene: Scene) -> WidgetId {
    tree.add(SceneView::new(scene))
}

fn accept_any(_a: &MagnetRef, _b: &MagnetRef) -> MagnetVerdict {
    MagnetVerdict::accept()
}

// =========================================================================
// The heavyweight fixture list
// =========================================================================

fn fixtures() -> Vec<TargetFixture> {
    vec![
        // ---- The view itself ---------------------------------------------
        TargetFixture::new("scene_view/empty", |t| view_of(t, Scene::new())),
        // `interactive(false)` gates **navigation** — scroll, pinch, keys, the
        // pan claim — and nothing else, so a locked view is still a pointer
        // target: it keeps the tap and hover handlers its lightweight content
        // is routed through. Measured, because the builder's name invites the
        // opposite assumption.
        TargetFixture::new("scene_view/not_interactive", |t| {
            t.add(SceneView::new(Scene::new()).interactive(false))
        }),
        // ---- Heavyweight content -----------------------------------------
        TargetFixture::new("heavyweight/button", |t| {
            let mut scene = Scene::new();
            scene.add_widget(
                Button::new(lit!("Open")),
                Rect::new(80.0, 80.0, 120.0, 32.0),
            );
            view_of(t, scene)
        }),
        // Two controls flush against each other, both at the floor: the packed
        // case, with nothing undersized in it.
        TargetFixture::new("heavyweight/two_flush_buttons", |t| {
            let mut scene = Scene::new();
            scene.add_widget(
                Button::new(lit!("Open")),
                Rect::new(80.0, 80.0, 120.0, 32.0),
            );
            scene.add_widget(
                Button::new(lit!("Close")),
                Rect::new(200.0, 80.0, 120.0, 32.0),
            );
            view_of(t, scene)
        }),
        // ---- Zoom: the one thing a scene does to a target's size ----------
        TargetFixture::new("heavyweight/button_zoomed_in", |t| {
            let mut scene = Scene::new();
            scene.add_widget(
                Button::new(lit!("Open")),
                Rect::new(80.0, 80.0, 120.0, 32.0),
            );
            t.add(SceneView::new(scene).initial_zoom(2.0))
        }),
        TargetFixture::new("heavyweight/button_zoomed_out", |t| {
            let mut scene = Scene::new();
            scene.add_widget(
                Button::new(lit!("Open")),
                Rect::new(80.0, 80.0, 120.0, 32.0),
            );
            t.add(SceneView::new(scene).initial_zoom(0.5))
        }),
        // ---- Lightweight content, seen from the walker's side -------------
        // In the list precisely so that the blind spot is a measured row count
        // rather than a paragraph.
        TargetFixture::new("lightweight/tappable_items", |t| {
            let mut scene = Scene::new();
            for i in 0..6 {
                let x = 40.0 + i as f32 * 30.0;
                let id = scene.add_item(
                    RectItem::new(Rect::new(0.0, 0.0, 12.0, 12.0)).fill(Color::RED),
                    Point::new(x, 60.0),
                );
                scene.handlers_mut(id).unwrap().on_tap(|_, _| {});
            }
            view_of(t, scene)
        }),
        // ---- Magnets ------------------------------------------------------
        TargetFixture::new("magnets/two_ports", |t| {
            let mut scene = Scene::new();
            let a = scene.add_item(
                RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(Color::RED),
                Point::new(60.0, 60.0),
            );
            scene.add_magnet(
                a,
                Magnet::new(Point::new(40.0, 20.0)).role(MagnetRole::Source),
            );
            let b = scene.add_item(
                RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(Color::BLUE),
                Point::new(260.0, 60.0),
            );
            scene.add_magnet(
                b,
                Magnet::new(Point::new(0.0, 20.0)).role(MagnetRole::Target),
            );
            t.add(SceneView::new(scene).magnetism(MagnetismConfig::new(accept_any)))
        }),
        // ---- Chrome the crate ships beside a view -------------------------
        // A minimap installs its handler only when it has an `on_click`, so a
        // decorative one is not a target and this fixture gives it the callback
        // an app that wants click-to-navigate supplies.
        TargetFixture::new("minimap/beside_a_view", |t| {
            let mut scene = Scene::new();
            scene.add_widget(
                Button::new(lit!("Open")),
                Rect::new(80.0, 80.0, 120.0, 32.0),
            );
            let view = SceneView::new(scene);
            let viewport = view.viewport_in_scene_signal();
            t.add(
                Padding::uniform(16.0).child(
                    VStack::new()
                        .spacing(12.0)
                        // The view is greedy on both axes, so without a bound it
                        // pushes the minimap past the window and the audit
                        // correctly reports the minimap as not on screen.
                        .child(FixedSize::new().height(300.0).child(view))
                        .child(
                            SceneMinimap::new(Rect::new(0.0, 0.0, 400.0, 300.0), viewport)
                                .on_click(|_scene_pt, _ctx| {}),
                        ),
                ),
            )
        }),
        TargetFixture::new("scroll_view/with_bars", |t| {
            let mut scene = Scene::new();
            scene.set_scene_rect(Some(Rect::new(0.0, 0.0, 2000.0, 2000.0)));
            scene.add_widget(
                Button::new(lit!("Open")),
                Rect::new(80.0, 80.0, 120.0, 32.0),
            );
            t.add(SceneView::new(scene).with_scroll_bars())
        }),
        // ---- The discriminating container ---------------------------------
        // A scene tile inside a tappable card: the card owns every near miss,
        // so nothing inside can be rescued by the framework's slop pass.
        TargetFixture::new("heavyweight/button_in_a_tappable_card", |t| {
            let mut scene = Scene::new();
            scene.add_widget(
                Button::new(lit!("Open")),
                Rect::new(80.0, 80.0, 120.0, 32.0),
            );
            t.add(
                Padding::uniform(16.0)
                    .child(
                        VStack::new()
                            .spacing(12.0)
                            .child(TextWidget::new(lit!("Board")))
                            .child(SceneView::new(scene)),
                    )
                    .on_tap(|_, _| {}),
            )
        }),
    ]
}

// =========================================================================
// The gate
// =========================================================================

/// **The seed.** One entry, and it is not a layout defect: it is what a camera
/// does.
///
/// The geometry is pinned rather than described: a 120 x 32 dp control placed at
/// `initial_zoom(0.5)` is 60 x 16 dp on glass, and the reach's short axis is the
/// same 16 — no mechanism widens it, because scaling the ring with the camera is
/// what would make a zoomed-out board's targets overlap. The long axis spends
/// the probe budget (25 / 33 / 45 dp), so it is recorded as clearing the floor
/// rather than pinned to a number that is really the budget.
const ALLOW_LIST: &[AllowedViolation] = &[AllowedViolation {
    path: "button_zoomed_out",
    measured: &[PinnedGeometry {
        densities: &DENSITIES,
        paints: (Is(60.0), Is(16.0)),
        reaches: (ClearsFloor, Is(16.0)),
    }],
    owner: "",
    exception: Some("Equivalent"),
    why: "A 120 x 32 dp control in a scene at 0.5 zoom is 60 x 16 dp on glass, \
          so it is under the 24 dp floor at every density -- and the framework \
          cannot fix it, because the zoom is the user's. Zooming out is how a \
          user sees more of a board; forbidding the shortfall would mean \
          forbidding zoom-out, and raising the scene-space size would change \
          what the board looks like at unit zoom. WCAG 2.2 SC 2.5.8 judges the \
          author's presentation, and the author's presentation here is the unit-\
          zoom one, which `heavyweight/button` measures as conformant; a \
          user-chosen magnification is the same class of thing as browser zoom. \
          The *Equivalent* route back is the view's own zoom (Ctrl+wheel, pinch, \
          `SceneView::zoom_to` / `fit_to_content`), and the AT tree publishes \
          each item with screen-projected bounds either way. Owner: nobody -- \
          this entry is a statement about cameras, and \
          `zoom_scales_a_heavyweight_targets_screen_size` is the measurement \
          behind it.",
}];

/// **The gate.** Zero conformance failures over the heavyweight fixture list at
/// all three densities.
#[test]
fn no_scene_target_falls_below_the_conformance_floor_at_any_density() {
    let fixtures = fixtures();
    let mut failures = Vec::new();
    for density in DENSITIES {
        for violation in audit_fixtures(&fixtures, density) {
            if !violation.rule.is_conformance_failure() {
                continue;
            }
            if ALLOW_LIST.iter().any(|e| e.matches(&violation)) {
                continue;
            }
            failures.push(violation.to_string());
        }
    }
    assert!(
        failures.is_empty(),
        "{} scene target(s) below the 24 dp WCAG 2.2 SC 2.5.8 (AA) floor:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// Every allow-list entry, every geometry it pins, and every density it claims
/// still matches something — and nothing it does not pin is excused.
///
/// Empty is the healthy state for this list. Until it is, both halves are
/// needed: the first stops an entry outliving what it excused, the second stops
/// it covering a geometry it never measured. A fixture-name matcher — which this
/// entry's `path` is — passes the first and fails the second, so the seeding is
/// what makes the name safe to key on.
#[test]
fn no_scene_allow_list_entry_is_stale() {
    let fixtures = fixtures();
    let mut seeded = 0_usize;
    for entry in ALLOW_LIST {
        assert!(!entry.why.is_empty(), "entry `{}` has no why", entry.path);
        assert!(
            !entry.owner.is_empty() || entry.why.contains("Owner: nobody"),
            "entry `{}` names no owner and does not say why it needs none",
            entry.path,
        );
        assert!(
            !entry.measured.is_empty(),
            "entry `{}` pins no measurement, so it excuses whatever its path \
             happens to match",
            entry.path,
        );
        for (index, pin) in entry.measured.iter().enumerate() {
            for &density in pin.densities {
                let violations = audit_fixtures(&fixtures, density);
                assert!(
                    violations.iter().any(|v| v.rule.is_conformance_failure()
                        && v.path.contains(entry.path)
                        && pin.covers(v)),
                    "allow-list entry `{}` pin {index} matches no conformance \
                     failure at {density:?} any more — re-measure it, narrow its \
                     densities, or delete it and its justification with it",
                    entry.path,
                );
            }
        }
    }
    for density in DENSITIES {
        for violation in audit_fixtures(&fixtures, density) {
            if !violation.rule.is_conformance_failure() {
                continue;
            }
            for axis in 0..4 {
                let mut broken = violation.clone();
                match axis {
                    0 => broken.size.width = 2.0,
                    1 => broken.size.height = 2.0,
                    2 => broken.expanded.width = 2.0,
                    _ => broken.expanded.height = 2.0,
                }
                assert!(
                    !ALLOW_LIST.iter().any(|e| e.matches(&broken)),
                    "a regression seeded on axis {axis} of a real violation is \
                     still excused, so the entry covering it is a blanket over \
                     its fixture rather than a pin on a geometry:\n  real:   \
                     {violation}\n  seeded: {broken}",
                );
                seeded += 1;
            }
        }
    }
    assert_eq!(
        seeded, 12,
        "one zoomed-out button at three densities, four scalars each — a \
         different number means the census moved and this test is proving \
         something else",
    );
}

/// The whole conformance story of this crate, as numbers, with the AA failures
/// separated from the informational rules.
///
/// * The only **AA** shortfall (`MinTargetConformance`, SC 2.5.8) is the
///   zoomed-out button, one row at each of the three densities, and the
///   allow-list says why.
/// * Everything else is `TouchTargetRecommendation` — SC 2.5.5 **AAA** and
///   Apple's HIG figure, never AA — and there are two kinds. A 12 dp scroll bar,
///   whose declared `hit_outset` ring reaches 30 dp: past the 24 dp AA floor at
///   every density, short of the 32 and 44 dp `target_size` ones. And a
///   heavyweight `Button` placed in a 32 dp scene rectangle, which is exactly
///   the `Comfortable` figure and eight short of the `Touch` one — the same finding
///   [`a_heavyweight_items_target_size_is_the_rect_the_app_declared`] states:
///   the declared rectangle does not follow the density ladder, so at `Touch`
///   every 32 dp scene rectangle in the list is under the recommendation.
///
/// The counts are exact on purpose: the fixture list is the coverage claim, so a
/// change in it has to be read rather than absorbed by a `>=`.
#[test]
fn the_only_aa_shortfall_in_this_crate_is_a_camera_state() {
    let fixtures = fixtures();
    // Compact, Comfortable, Touch. Compact has only the camera row; Comfortable
    // adds the two scroll bars and their two reported paging-track regions; Touch
    // adds every 32 dp scene rectangle in the list.
    let expected_rows = [1_usize, 5, 11];
    for (density, want_rows) in DENSITIES.into_iter().zip(expected_rows) {
        let rows = audit_fixtures(&fixtures, density);
        let mut aa = 0;
        for v in &rows {
            let subject = v.path.rsplit(" > ").next().unwrap_or("?");
            if v.rule.is_conformance_failure() {
                aa += 1;
                assert!(
                    v.path.contains("button_zoomed_out"),
                    "an unexpected AA shortfall at {density:?}: {v}",
                );
            } else {
                assert_eq!(
                    v.rule,
                    TargetRule::TouchTargetRecommendation,
                    "the spacing exception is not expected in this crate: {v}",
                );
                assert!(
                    subject == "Button" || subject == "ScrollBar",
                    "an unexpected recommendation shortfall at {density:?}: {v}",
                );
            }
        }
        assert_eq!(aa, 1, "one AA row at {density:?}");
        assert_eq!(
            rows.len(),
            want_rows,
            "row count at {density:?}:\n{}",
            rows.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}

/// The scroll bar's reach is the **exact-pass** leg of the walk — `hit_outset`,
/// not the miss-only slop pass — and it is 30 dp: a widget's own declaration,
/// the same number at every density, because an outset is not a density
/// projection.
///
/// At `Compact` the probe's own axis budget (25 dp, one past the largest floor
/// there) runs out before the ring does, so the measurement is capped and the
/// exact 30 is only readable at the two higher densities. Asserted as an
/// equality there and as "capped, and past the floor" at Compact, because a bare
/// bound would be satisfied by the bar's 12 dp painted width plus any accident.
#[test]
fn the_scroll_bars_reach_comes_from_hit_outset_and_not_from_the_slop_pass() {
    for density in DENSITIES {
        let tokens = InputTokens::for_density(density);
        let rows: Vec<TargetMeasurement> = measure_fixtures(
            &[TargetFixture::new("bars", |t| {
                let mut scene = Scene::new();
                scene.set_scene_rect(Some(Rect::new(0.0, 0.0, 2000.0, 2000.0)));
                t.add(SceneView::new(scene).with_scroll_bars())
            })],
            density,
        )
        .into_iter()
        .filter(|m| m.path.contains("ScrollBar") && m.part.is_none())
        .collect();
        assert!(!rows.is_empty(), "the bars are targets at {density:?}");
        for row in &rows {
            assert!(
                row.sources.outset && !row.sources.slop,
                "the bar's growth is its own declared ring, not a near-miss \
                 re-attribution, at {density:?}: {:?}",
                row.sources,
            );
            let across = row.size.width.min(row.size.height);
            let reached = row.expanded.width.min(row.expanded.height);
            assert!(
                (across - 12.0).abs() <= 0.05,
                "a 12 dp bar at {density:?}: {:?}",
                row.size,
            );
            if density == TargetDensity::Compact {
                assert!(
                    row.capped && reached >= tokens.min_target_conformance,
                    "at Compact the 25 dp axis budget runs out before the ring \
                     does, so the reach is a bound past the floor: {row:#?}",
                );
            } else {
                assert!(
                    (reached - 30.0).abs() <= 0.05,
                    "the declared ring reaches 30 dp at {density:?}: {:?}",
                    row.expanded,
                );
            }
        }
    }
}

/// What each fixture must be seen to measure. `None` says the subject is
/// deliberately not a pointer target.
const EXPECTATIONS: &[(&str, Option<&str>)] = &[
    ("scene_view/empty", Some("SceneView")),
    ("scene_view/not_interactive", Some("SceneView")),
    ("heavyweight/button", Some("Button")),
    ("heavyweight/two_flush_buttons", Some("Button")),
    ("heavyweight/button_zoomed_in", Some("Button")),
    ("heavyweight/button_zoomed_out", Some("Button")),
    ("lightweight/tappable_items", Some("SceneView")),
    ("magnets/two_ports", Some("SceneView")),
    ("minimap/beside_a_view", Some("SceneMinimap")),
    ("scroll_view/with_bars", Some("ScrollBar")),
    ("heavyweight/button_in_a_tappable_card", Some("Button")),
];

#[test]
fn every_scene_fixture_measures_the_subject_it_names() {
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
                "fixture `{}` has no entry in EXPECTATIONS",
                fixture.name
            ));
            continue;
        };
        let prefix = format!("{}: ", fixture.name);
        let rows: Vec<&TargetMeasurement> = measured
            .iter()
            .filter(|m| m.path.starts_with(&prefix))
            .collect();
        let names = rows
            .iter()
            .map(|m| m.path.rsplit(" > ").next().unwrap_or("?").to_string())
            .collect::<Vec<_>>()
            .join(", ");
        match expectation.1 {
            Some(widget) if !rows.iter().any(|m| m.path.contains(widget)) => {
                problems.push(format!(
                    "fixture `{}` measured no `{widget}`; it measured: {names}",
                    fixture.name
                ))
            }
            None if !rows.is_empty() => problems.push(format!(
                "fixture `{}` is declared not a target but measured: {names}",
                fixture.name
            )),
            _ => {}
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The set of measured-but-not-judged targets, exactly. A new skip is a change
/// in coverage and has to be read, not absorbed.
#[test]
fn the_skips_this_crate_produces_are_the_ones_it_should() {
    let fixtures = fixtures();
    let mut seen: Vec<String> = Vec::new();
    for density in DENSITIES {
        for m in measure_fixtures(&fixtures, density) {
            if let Some(reason) = m.skipped {
                // The fixture name prefixes the path, so strip it before taking
                // the last node — a top-level subject has no " > " in its path
                // and would otherwise be reported with the fixture name glued on.
                let tail = m.path.split_once(": ").map(|(_, t)| t).unwrap_or(&m.path);
                let subject = tail.rsplit(" > ").next().unwrap_or("?");
                let row = format!("{subject}:{reason:?}");
                if !seen.contains(&row) {
                    seen.push(row);
                }
            }
        }
    }
    seen.sort();
    assert_eq!(
        seen,
        EXPECTED_SKIPS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        "the set of measured-but-not-judged targets changed",
    );
}

/// One entry, and it is the correct one: the tappable card's own centre falls
/// inside the `SceneView` filling it, so the card delegates there and the view is
/// judged in its place.
const EXPECTED_SKIPS: &[&str] = &["Padding:DelegatesToDescendant"];

// =========================================================================
// Zoom: what a scene does to a target's size
// =========================================================================

/// A scene's zoom scales the *screen* size of every heavyweight target, and the
/// audit measures screen space — so a 32 dp control in a scene at 0.5 zoom is a
/// 16 dp target, and at 2.0 zoom a 64 dp one.
///
/// This is the one target-size question peculiar to a scene, and it is not
/// something the framework can fix: zoom is the user's, and shrinking the view is
/// how a user sees more of a board. It is measured here so nobody claims a
/// scene's targets are density-projected — the projection sets the *scene-space*
/// size and the camera decides what that is on glass.
#[test]
fn zoom_scales_a_heavyweight_targets_screen_size() {
    let at = |name: &'static str, zoom: f32| -> Size {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light().with_density(TargetDensity::Compact));
        let mut scene = Scene::new();
        scene.add_widget(
            Button::new(lit!("Open")),
            Rect::new(80.0, 80.0, 120.0, 32.0),
        );
        tree.add(SceneView::new(scene).initial_zoom(zoom));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let rows = measure_targets(&tree, TargetDensity::Compact);
        rows.iter()
            .find(|m| m.path.contains("Button"))
            .unwrap_or_else(|| panic!("{name}: no Button row in {rows:#?}"))
            .size
    };
    assert_eq!(at("unit", 1.0), Size::new(120.0, 32.0), "the painted rect");
    assert_eq!(at("half", 0.5), Size::new(60.0, 16.0), "halved at 0.5 zoom");
    assert_eq!(at("double", 2.0), Size::new(240.0, 64.0), "doubled at 2.0");
}

/// And a zoomed-out heavyweight target really is reported as undersized, which
/// is what makes the statement above a finding rather than an observation.
#[test]
fn a_zoomed_out_heavyweight_target_is_reported_undersized() {
    let fixtures = [TargetFixture::new("zoomed_out", |t| {
        let mut scene = Scene::new();
        scene.add_widget(Port(16.0), Rect::new(80.0, 80.0, 16.0, 16.0));
        t.add(SceneView::new(scene).initial_zoom(0.25))
    })];
    let violations = audit_fixtures(&fixtures, TargetDensity::Compact);
    assert!(
        violations
            .iter()
            .any(|v| v.path.contains("Port") && v.rule == TargetRule::MinTargetConformance),
        "a 16 dp port at 0.25 zoom is 4 dp on glass: {violations:#?}",
    );
}

// =========================================================================
// Two rules a scene imposes on heavyweight targets
// =========================================================================

/// **A heavyweight item's target size is the rectangle the app declared, not the
/// control's own measurement.** `Scene::add_widget(widget, local_rect)` places
/// the widget at `local_rect` and the arena's bounds are that rectangle, so a
/// `Button` whose intrinsic height is 32 dp is a 16 dp target if it was added in
/// a 16 dp rectangle.
///
/// This is the scene-specific consequence of the density work that nothing else
/// in the framework has: a density projection raises a *control's* chrome, and in
/// a scene the control's chrome is not what decides its target. An app that
/// places node-graph content at hand-computed coordinates owns those numbers, and
/// no `TargetDensity` will grow them.
#[test]
fn a_heavyweight_items_target_size_is_the_rect_the_app_declared() {
    let measured = |rect: Rect| -> Size {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light().with_density(TargetDensity::Touch));
        let mut scene = Scene::new();
        scene.add_widget(Button::new(lit!("Open")), rect);
        tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        measure_targets(&tree, TargetDensity::Touch)
            .into_iter()
            .find(|m| m.path.contains("Button"))
            .expect("the button is a target")
            .size
    };
    assert_eq!(
        measured(Rect::new(80.0, 80.0, 120.0, 16.0)),
        Size::new(120.0, 16.0),
        "a 16 dp declared rect is a 16 dp target, whatever the Button would have \
         measured, and at Touch density of all places",
    );
    assert_eq!(
        measured(Rect::new(80.0, 80.0, 120.0, 44.0)),
        Size::new(120.0, 44.0),
        "and the rect decides in the other direction too",
    );
}

/// **A heavyweight widget inside a `SceneView` gets nothing from the framework's
/// miss-only slop pass, at any density**, because the view itself always takes a
/// press: it registers tap and hover handlers unconditionally (they are how
/// lightweight content is routed), so every point inside the view already has an
/// eligible owner at distance zero and the pass's own rule refuses to beat it.
///
/// The consequence for an app: a small heavyweight affordance in a scene — a
/// node-graph port button, a card's close glyph — must be laid out at the floor.
/// There is no widening mechanism behind it. The counterpart is that the
/// *lightweight* tier is served, by the view's own pass, which is what
/// [`a_lightweight_items_reach_follows_the_pointer_and_the_density`] measures:
/// the same 12 dp box reaches 24 dp as a `SceneItem` and 12 dp as a `Widget`.
///
/// Both geometries are measured — isolated and flush against a neighbour —
/// because the redundancy trap A10 records is that a lone subject in a bare stack
/// is rescued by the pass and so cannot tell the two mechanisms apart. Here
/// neither geometry is rescued, and that is the finding.
#[test]
fn a_heavyweight_widget_in_a_scene_gets_no_help_from_the_slop_pass() {
    let reach_of = |flush: bool, density: TargetDensity| -> Vec<(Size, Size)> {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light().with_density(density));
        let mut scene = Scene::new();
        scene.add_widget(Port(16.0), Rect::new(80.0, 80.0, 16.0, 16.0));
        if flush {
            scene.add_widget(Port(16.0), Rect::new(96.0, 80.0, 16.0, 16.0));
        }
        tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        measure_targets(&tree, density)
            .into_iter()
            .filter(|m| m.path.contains("Port"))
            .map(|m| (m.size, m.expanded))
            .collect()
    };
    for density in DENSITIES {
        for flush in [false, true] {
            let rows = reach_of(flush, density);
            assert_eq!(rows.len(), if flush { 2 } else { 1 }, "one row per port");
            for (paint, reach) in rows {
                // Within the walker's own boundary epsilon: a reach is found by
                // bisecting inside one probe step, so an exact edge is reported
                // up to about 0.03 dp short of itself.
                assert!(
                    (reach.width - paint.width).abs() <= 0.05
                        && (reach.height - paint.height).abs() <= 0.05,
                    "a 16 dp heavyweight port reaches exactly its own box at \
                     {density:?} (flush: {flush}) — the view owns every near \
                     miss, so no mechanism is behind it: paints {paint:?}, \
                     reaches {reach:?}",
                );
            }
        }
    }
}

/// And the same 16 dp port therefore *is* a conformance failure at every
/// density, which is what makes the rule above a finding.
#[test]
fn a_sixteen_dp_heavyweight_port_fails_at_every_density() {
    let fixtures = [TargetFixture::new("port", |t| {
        let mut scene = Scene::new();
        scene.add_widget(Port(16.0), Rect::new(80.0, 80.0, 16.0, 16.0));
        t.add(SceneView::new(scene))
    })];
    for density in DENSITIES {
        let violations = audit_fixtures(&fixtures, density);
        assert!(
            violations
                .iter()
                .any(|v| v.path.contains("Port") && v.rule == TargetRule::MinTargetConformance),
            "at {density:?}: {violations:#?}",
        );
    }
}

// =========================================================================
// The lightweight tier: the blind spot, stated as a measurement
// =========================================================================

/// **A lightweight `SceneItem` is invisible to the core walker**, and the reason
/// is structural: the walker's two legs are "an arena node that
/// `WidgetArena::takes_a_press`" and "a region a widget reported from
/// `Widget::target_regions`", and a `SceneItem` is neither. It has an `ItemId`,
/// not a `WidgetId`, and `teksilo-scene` implements neither hit hook — grep:
/// `target_regions` and `hit_outset` appear nowhere in the crate.
///
/// Measured here twice over: six tappable lightweight items produce **no** extra
/// audit row, and thirty-six produce none either.
///
/// **What closing it would take.** `SceneView::target_regions(bounds)` reporting
/// one region per visible item, projected through the live view transform. Three
/// things make that more than an afternoon's work, and they are why this is a
/// documented blind spot rather than a fix:
///
/// 1. **A region is measured, not probed.** The core walker deliberately does not
///    probe reported regions — the split is internal to one node, so no framework
///    mechanism widens it and no framework probe can observe it. But the scene's
///    items *are* widened, by the view's own `GrabSlop` pass. A region row would
///    therefore report an item's painted rectangle and understate its reach by up
///    to the profile radius per side: the audit would invent violations for items
///    a finger reaches perfectly well. `TargetRegion` would first need a way to
///    carry an already-confirmed reach.
/// 2. **Item count.** The tier exists so that thousands of items are cheap. One
///    audit row per item turns a census into a memory profile, and
///    `Widget::target_regions` returns an owned `Vec`.
/// 3. **Culling and the camera.** Only items inside the viewport have a screen
///    rectangle worth judging, and which those are changes with pan and zoom — so
///    a region list is a function of the camera, not of the widget, while the
///    walker asks for it once per pass.
///
/// The rest of this file measures the tier instead, through the pipeline that
/// actually delivers its presses. That is the honest answer available today.
#[test]
fn a_lightweight_item_is_invisible_to_the_core_walker() {
    let count_rows = |items: usize| -> usize {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light().with_density(TargetDensity::Touch));
        let mut scene = Scene::new();
        for i in 0..items {
            let id = scene.add_item(
                RectItem::new(Rect::new(0.0, 0.0, 12.0, 12.0)).fill(Color::RED),
                Point::new(20.0 + (i % 20) as f32 * 25.0, 20.0 + (i / 20) as f32 * 25.0),
            );
            scene.handlers_mut(id).unwrap().on_tap(|_, _| {});
        }
        tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        measure_targets(&tree, TargetDensity::Touch).len()
    };
    assert_eq!(count_rows(0), 1, "the view itself is the only target");
    assert_eq!(count_rows(6), 1, "six tappable items added no audit row");
    assert_eq!(
        count_rows(36),
        1,
        "nor did thirty-six — the walker's row count does not follow the \
         lightweight tier at all, which is the blind spot this test names",
    );
}

// =========================================================================
// The lightweight tier, measured through the pipeline that routes its presses
// =========================================================================

fn finger() -> PointerId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PointerIdAllocator::global().begin(
        BackendDeviceKey::new(0x5CE2),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )
}

fn sample(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
    PointerSample {
        pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
        phase,
        position: at,
        button: None,
        modifiers: Modifiers::default(),
        coalesced: Vec::new(),
    }
}

/// Press and release at one point with the given pointer kind, on a freshly
/// built tree, and report whether the item's own `on_tap` fired.
///
/// A fresh tree per probe on purpose: successive presses on one tree feed the
/// multi-tap recognizer, and a second press within the double-tap window and
/// slop of the first is a double tap, which fires no `on_tap`. That would have
/// made every probe past the first report a miss.
fn item_takes_a_press_at(item: Rect, at: Point, density: TargetDensity, coarse: bool) -> bool {
    let taps = Rc::new(Cell::new(0u32));
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, item.width, item.height)).fill(Color::RED),
        Point::new(item.x, item.y),
    );
    let counter = taps.clone();
    scene
        .handlers_mut(id)
        .unwrap()
        .on_tap(move |_, _| counter.set(counter.get() + 1));

    let mut tree =
        WidgetTree::new().with_theme(teksilo_core::presets::intui::light().with_density(density));
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    if coarse {
        let id = finger();
        tree.dispatch_pointer(sample(id, PointerPhase::Down, at, 0));
        tree.dispatch_pointer(sample(id, PointerPhase::Up, at, 16));
    } else {
        tree.pointer_move(at);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: at,
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: at,
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
        });
    }
    taps.get() > 0
}

/// The step of the outward walk, in dp — the resolution of every lightweight
/// reach reported below, and the tolerance every comparison against it carries.
const LIGHTWEIGHT_PROBE_STEP: f32 = 0.5;

/// The contiguous reach out of an item's centre, summed over both directions of
/// each axis. The lightweight-tier counterpart of the core walker's probe: walk
/// outward, stop at the first point the item no longer takes.
fn lightweight_reach(item: Rect, density: TargetDensity, coarse: bool) -> Size {
    let centre = item.center();
    let mut extent = [0.0_f32; 4];
    for (index, (dx, dy)) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]
        .into_iter()
        .enumerate()
    {
        let mut d = LIGHTWEIGHT_PROBE_STEP;
        // Well past the largest floor plus the largest offer, so a reach that
        // clears everything is measured rather than clipped.
        while d <= 60.0 {
            let at = Point::new(centre.x + dx * d, centre.y + dy * d);
            if item_takes_a_press_at(item, at, density, coarse) {
                extent[index] = d;
                d += LIGHTWEIGHT_PROBE_STEP;
            } else {
                break;
            }
        }
    }
    Size::new(extent[0] + extent[1], extent[2] + extent[3])
}

/// A lightweight item's reach is **its rectangle for a mouse** and **its
/// rectangle plus the density's slop offer for a finger** — the same arithmetic
/// the framework spends on a small widget, applied to an item box by the view's
/// own miss-only pass.
///
/// Asserted against `HitSlop::outset_for` rather than against literals, because
/// the claim is that the scene spends the framework's number and not one of its
/// own. The mouse arm is the load-bearing half: the mouse profile's radius is
/// `0.0` at every density, so if it ever moves, a tolerance has escaped the
/// miss-only pass it is supposed to live in.
#[test]
fn a_lightweight_items_reach_follows_the_pointer_and_the_density() {
    // 12 dp: small enough that every density has an offer to make.
    let item = Rect::new(200.0, 200.0, 12.0, 12.0);
    for density in DENSITIES {
        let tokens = InputTokens::for_density(density);

        let mouse = lightweight_reach(item, density, false);
        assert!(
            (mouse.width - 12.0).abs() <= LIGHTWEIGHT_PROBE_STEP
                && (mouse.height - 12.0).abs() <= LIGHTWEIGHT_PROBE_STEP,
            "a mouse reaches the item's own box and nothing more at {density:?}, \
             got {mouse:?}",
        );

        let offer =
            teksilo_core::pointer::hit_slop::HitSlop::for_pointer(PointerKind::Touch, &tokens)
                .outset_for(item.size());
        let want = 12.0 + 2.0 * offer;
        let touch = lightweight_reach(item, density, true);
        assert!(
            (touch.width - want).abs() <= LIGHTWEIGHT_PROBE_STEP
                && (touch.height - want).abs() <= LIGHTWEIGHT_PROBE_STEP,
            "a finger reaches the item's box plus the framework's own offer of \
             {offer} dp per side at {density:?}: wanted {want}, got {touch:?}",
        );
    }
}

/// A lightweight item whose box already reaches the density's `target_size`
/// earns **nothing** from the pass — the arithmetic excludes it, not a rule.
///
/// The discriminator A11 records: once a control's own box is at `target_size`,
/// its top-up is exactly zero and no point outside the box is in reach. Stated
/// here for the lightweight tier, so a later change that made the scene's pass
/// unconditional is a failure rather than an improvement nobody noticed.
#[test]
fn a_large_lightweight_item_earns_no_top_up() {
    let item = Rect::new(200.0, 200.0, 48.0, 48.0);
    for density in DENSITIES {
        let touch = lightweight_reach(item, density, true);
        assert!(
            (touch.width - 48.0).abs() <= LIGHTWEIGHT_PROBE_STEP,
            "a 48 dp item is already past every `up_to`, so a finger reaches its \
             box and no more at {density:?}: {touch:?}",
        );
    }
}

/// The floors, applied to the lightweight tier by hand — the verdict the core
/// walker would have given if it could see these targets.
///
/// A 12 dp item clears the 24 dp SC 2.5.8 floor under a finger at every density,
/// and a 6 dp item does **not** at any of them. That second half is this half of
/// the file's liveness proof: a measurement that reports a real shortfall is a
/// measurement that is looking.
#[test]
fn the_lightweight_tier_judged_against_the_same_floors() {
    for density in DENSITIES {
        let floor = InputTokens::for_density(density).min_target_conformance;

        let ok = lightweight_reach(Rect::new(200.0, 200.0, 12.0, 12.0), density, true);
        assert!(
            ok.width + LIGHTWEIGHT_PROBE_STEP >= floor
                && ok.height + LIGHTWEIGHT_PROBE_STEP >= floor,
            "a 12 dp lightweight item clears the {floor} dp floor under a finger \
             at {density:?}: {ok:?}",
        );

        let short = lightweight_reach(Rect::new(200.0, 200.0, 6.0, 6.0), density, true);
        assert!(
            short.width < floor && short.height < floor,
            "a 6 dp lightweight item cannot be lifted to {floor} dp by an offer \
             capped at the profile radius, and the measurement must say so at \
             {density:?}: {short:?}",
        );
    }
}

// =========================================================================
// Magnet handles: the crate's own grab affordance
// =========================================================================

/// Whether a press `d` dp to the right of item A's source magnet grabs the port
/// and completes a wire to B's target magnet.
///
/// A port drag is the only thing this press can be: A is not draggable, and the
/// press is outside A's box.
fn a_port_drag_connects_from(d: f32, density: TargetDensity, coarse: bool) -> bool {
    let connections = Rc::new(Cell::new(0u32));
    let counter = connections.clone();
    let cfg = MagnetismConfig::new(accept_any)
        .on_connect(move |_conn, _ctx| counter.set(counter.get() + 1));

    let mut scene = Scene::new();
    let a = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(Color::RED),
        Point::new(100.0, 100.0),
    );
    scene.add_magnet(
        a,
        Magnet::new(Point::new(40.0, 20.0)).role(MagnetRole::Source),
    );
    let b = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(Color::BLUE),
        Point::new(300.0, 100.0),
    );
    scene.add_magnet(
        b,
        Magnet::new(Point::new(0.0, 20.0)).role(MagnetRole::Target),
    );

    let mut tree =
        WidgetTree::new().with_theme(teksilo_core::presets::intui::light().with_density(density));
    tree.add(SceneView::new(scene).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // A's magnet is at scene (140, 120); B's at (300, 120).
    let from = Point::new(140.0 + d, 120.0);
    let to = Point::new(300.0, 120.0);
    if coarse {
        let id = finger();
        tree.dispatch_pointer(sample(id, PointerPhase::Down, from, 0));
        tree.dispatch_pointer(sample(id, PointerPhase::Move, to, 16));
        tree.dispatch_pointer(sample(id, PointerPhase::Up, to, 32));
    } else {
        tree.pointer_move(from);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: from,
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove { position: to });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: to,
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
        });
    }
    connections.get() > 0
}

/// The radius within which a press grabs a magnet handle, by walking outward
/// until the port drag stops connecting.
fn magnet_grab_radius(density: TargetDensity, coarse: bool) -> f32 {
    let mut last = 0.0_f32;
    let mut d = LIGHTWEIGHT_PROBE_STEP;
    while d <= 60.0 {
        if a_port_drag_connects_from(d, density, coarse) {
            last = d;
            d += LIGHTWEIGHT_PROBE_STEP;
        } else {
            break;
        }
    }
    last
}

/// A magnet handle is a grab affordance a finger has to hit, and its reach is
/// `MagnetismConfig::capture_px` for a mouse plus what a disc of that diameter
/// earns from the pointer's own profile — so the *diameter* a finger can grab it
/// by clears the 24 dp floor at every density with no app-side number.
///
/// Asserted against the tokens rather than against literals, and again with the
/// mouse arm as the load-bearing half.
#[test]
fn a_magnet_handles_grab_reach_is_kind_aware() {
    let capture_px = 14.0; // MagnetismConfig::new's default.
    for density in DENSITIES {
        let tokens = InputTokens::for_density(density);
        let disc = Size::new(capture_px * 2.0, capture_px * 2.0);

        let mouse = magnet_grab_radius(density, false);
        assert!(
            (mouse - capture_px).abs() <= LIGHTWEIGHT_PROBE_STEP,
            "a mouse grabs a magnet within the declared capture radius and no \
             further at {density:?}: wanted {capture_px}, got {mouse}",
        );

        let offer =
            teksilo_core::pointer::hit_slop::HitSlop::for_pointer(PointerKind::Touch, &tokens)
                .outset_for(disc);
        let want = capture_px + offer;
        let touch = magnet_grab_radius(density, true);
        assert!(
            (touch - want).abs() <= LIGHTWEIGHT_PROBE_STEP,
            "a finger grabs it within the capture radius plus the framework's own \
             offer of {offer} dp at {density:?}: wanted {want}, got {touch}",
        );
        assert!(
            2.0 * touch + LIGHTWEIGHT_PROBE_STEP >= tokens.min_target_conformance,
            "and the grabbable diameter clears the {} dp floor at {density:?}: {}",
            tokens.min_target_conformance,
            2.0 * touch,
        );
    }
}

/// A magnet handle produces no audit row either: it is a point on an item, not a
/// node, so the same blind spot covers it. Named separately because a magnet is
/// the one thing in this crate that is *only* a grab affordance — there is no
/// enclosing widget whose size stands in for it.
#[test]
fn a_magnet_handle_is_invisible_to_the_core_walker() {
    let rows = measure_fixtures(
        &[TargetFixture::new("magnets", |t| {
            let mut scene = Scene::new();
            let a = scene.add_item(
                RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(Color::RED),
                Point::new(100.0, 100.0),
            );
            scene.add_magnet(
                a,
                Magnet::new(Point::new(40.0, 20.0)).role(MagnetRole::Source),
            );
            t.add(SceneView::new(scene).magnetism(MagnetismConfig::new(accept_any)))
        })],
        TargetDensity::Touch,
    );
    assert_eq!(
        rows.len(),
        1,
        "the view, and nothing for the magnet: {rows:#?}"
    );
    assert!(rows[0].path.contains("SceneView"));
}

// =========================================================================
// A deliberately undersized heavyweight subject must fail
// =========================================================================

/// The heavyweight half's liveness proof: a 10 dp widget at scene coordinates,
/// inside a card that is itself tappable, must fail at every density.
#[test]
fn a_deliberately_undersized_heavyweight_subject_fails() {
    let undersized = vec![TargetFixture::new("undersized", |t| {
        let mut scene = Scene::new();
        scene.add_widget(Port(10.0), Rect::new(80.0, 80.0, 10.0, 10.0));
        t.add(
            Padding::uniform(16.0)
                .child(
                    VStack::new()
                        .spacing(12.0)
                        .child(TextWidget::new(lit!("Board")))
                        .child(SceneView::new(scene)),
                )
                .on_tap(|_, _| {}),
        )
    })];
    for density in DENSITIES {
        let violations = audit_fixtures(&undersized, density);
        assert!(
            violations
                .iter()
                .any(|v| v.path.contains("Port") && v.rule == TargetRule::MinTargetConformance),
            "a 10 dp scene widget in a tappable card must fail at {density:?}: \
             {violations:#?}",
        );
    }
}
