// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The matcher measuring itself.
//!
//! [`super`] is the piece every per-crate gate delegates to, and a shared
//! matcher has the failure mode a shared anything has: it can stop asking a
//! question without any of its callers noticing. The gates it serves are written
//! over *shipped* geometry, so none of them can seed the defect a lint exists to
//! catch — a real allow-list must not contain an entry with no owner, and a real
//! roster must not name a theme that calls itself something else. That is
//! precisely why twelve of these thirteen checks passed their own deletion
//! before this file existed: the gates prove the list is clean, not that the
//! matcher would notice a dirty one.
//!
//! So every test here is a **negative**: it builds the one defect its check
//! exists to catch and asserts that check reports it. A positive test — "the
//! clean list produces no findings" — is satisfied by a function that returns
//! nothing, which is the mutation this file is the contract for.
//!
//! | mutate | reddens |
//! | --- | --- |
//! | [`conformance_census`](super::conformance_census) stops sweeping a theme or a density | [`the_census_sweeps_every_theme_at_every_density`] |
//! | it stops filtering to conformance failures | [`the_census_carries_only_conformance_failures`] |
//! | [`conformance_failures`](super::conformance_failures) stops reporting | [`an_unexcused_failure_is_reported`] |
//! | it stops consulting the allow-list | [`an_excused_failure_is_not_reported`] |
//! | [`stale_entries`](super::stale_entries) stops asking about a pin's geometry | [`an_entry_whose_geometry_moved_is_stale`] |
//! | it stops asking about a density | [`a_pin_claiming_a_density_it_never_meets_is_stale`] |
//! | it stops asking about a theme | [`a_pin_claiming_a_theme_it_never_meets_is_stale`] |
//! | [`non_narrowing`](super::non_narrowing) stops seeding the size axes | [`a_blanket_entry_survives_a_seeded_size_regression`] |
//! | it stops seeding the theme axis | [`a_theme_axis_that_decorates_rather_than_narrows_is_reported`] |
//! | it stops counting its seeds | [`a_census_that_collapsed_is_reported_as_proving_less`] |
//! | any one of [`roster_defects`](super::roster_defects)' eleven lints | that lint's own row in [`the_roster_lints`] |
//!
//! Every row above has been run: the named check was replaced by an early
//! `return`, the file touched so cargo could not reuse the object built from the
//! unmutated source, and the test confirmed red.

use teksilo_canvas::{Rect, Size, SizeProposal};

use super::*;
use crate::accessibility::target_audit::{
    AllowedViolation,
    PinnedDp::{ClearsFloor, Is},
    PinnedGeometry, ReachSources, TargetRule, TargetViolation, audit_fixtures,
};
use crate::build_context::BuildContext;
use crate::styles::{Theme, ThemeId};
use crate::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

// ---------------------------------------------------------------------------
// Fixture widgets
//
// Two, and deliberately not the richer pair in `target_audit/tests.rs`: those
// are private to that module, and the two questions that need a real tree here
// ask only whether the sweep covers the product. A 10 dp leaf inside a row that
// takes presses is the smallest tree that answers it — the row is what denies
// the miss-only slop pass, so the leaf stays undersized under every preset.
// ---------------------------------------------------------------------------

/// A leaf of a fixed size that takes presses.
#[derive(Debug)]
struct TinyLeaf(Size);

impl Widget for TinyLeaf {
    fn layout_response(&self, _p: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        self.0.into()
    }
}

/// A fixed-size container that places its children at their own intrinsic size,
/// inset from its leading edge.
#[derive(Debug)]
struct PaddedRow {
    size: Size,
    pad: f32,
    children: Vec<WidgetId>,
}

impl Widget for PaddedRow {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.children.clone()
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    fn layout_response(&self, _p: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        self.size.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _p: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let mut x = bounds.x + self.pad;
        for child in children.iter_mut() {
            let size = ctx
                .child_size(child.id, SizeProposal::unspecified())
                .unwrap_or(Size::ZERO);
            child.origin = teksilo_canvas::Point::new(x, bounds.y + self.pad);
            child.size = size;
            x += size.width;
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }
}

/// Two targets inside a row that takes presses: a 10 dp one, which is an AA
/// conformance failure under every preset at every density, and a 26 dp one,
/// which clears the 24 dp floor and falls short of the density's 32 / 44 dp
/// *recommendation* above Compact.
///
/// Both, because a census filtered to the AA rule has to be shown dropping
/// something. The two sit flush so neither earns the spacing exception, and the
/// row denies the miss-only slop pass to both.
fn undersized_in_a_tappable_row(tree: &mut WidgetTree) -> WidgetId {
    let small = tree.add(TinyLeaf(Size::new(10.0, 10.0)).on_tap(|_, _| {}));
    let middling = tree.add(TinyLeaf(Size::new(26.0, 26.0)).on_tap(|_, _| {}));
    tree.add(
        PaddedRow {
            size: Size::new(300.0, 100.0),
            pad: 40.0,
            children: vec![small, middling],
        }
        .on_tap(|_, _| {}),
    )
}

const UNDERSIZED: &[TargetFixture] = &[TargetFixture::new(
    "undersized",
    undersized_in_a_tappable_row,
)];

// ---------------------------------------------------------------------------
// Fixture rosters and censuses
// ---------------------------------------------------------------------------

const ALPHA: &str = "alpha.light";
const BETA: &str = "beta.light";

fn alpha() -> Theme {
    crate::presets::intui::light().with_id(ALPHA)
}

fn beta() -> Theme {
    crate::presets::intui::light().with_id(BETA)
}

/// A theme whose declared roster id is not the id it carries.
fn misnamed() -> Theme {
    crate::presets::intui::light().with_id("something.else")
}

/// A theme carrying the id every theme built from raw tokens has.
fn anonymous() -> Theme {
    crate::presets::intui::light().with_id(ANONYMOUS_THEME)
}

/// One synthetic conformance failure. Every field a matcher reads is an
/// argument; the rest are what a real violation would carry.
fn violation(
    path: &str,
    theme: &str,
    density: TargetDensity,
    paints: (f32, f32),
    reaches: (f32, f32),
) -> TargetViolation {
    TargetViolation {
        widget: "TinyLeaf",
        node: WidgetId::default(),
        part: None,
        path: path.to_string(),
        density,
        theme: ThemeId::new(theme.to_string()),
        size: Size::new(paints.0, paints.1),
        expanded: Size::new(reaches.0, reaches.1),
        sources: ReachSources::default(),
        transformed: false,
        rule: TargetRule::MinTargetConformance,
    }
}

/// The 16 x 16 geometry most of these fixtures use, pinned exactly.
const SIXTEEN: PinnedGeometry = PinnedGeometry {
    densities: ALL_DENSITIES,
    themes: &[ALPHA, BETA],
    paints: (Is(16.0), Is(16.0)),
    reaches: (Is(16.0), Is(16.0)),
};

/// A well-formed entry over [`SIXTEEN`].
const GRIP: AllowedViolation = AllowedViolation {
    path: "Grip",
    measured: &[SIXTEEN],
    owner: "a fixture",
    exception: None,
    why: "a fixture entry",
};

/// The roster the fixtures above are written against.
static TWO_THEMES: Roster = Roster {
    themes: &[
        ThemeSubject {
            id: ALPHA,
            theme: alpha,
        },
        ThemeSubject {
            id: BETA,
            theme: beta,
        },
    ],
    allow: &[GRIP],
};

/// Every density, under both themes — exactly the census [`GRIP`] excuses.
fn grip_census() -> Vec<TargetViolation> {
    let mut out = Vec::new();
    for theme in [ALPHA, BETA] {
        for &density in ALL_DENSITIES {
            out.push(violation(
                "Row > Grip",
                theme,
                density,
                (16.0, 16.0),
                (16.0, 16.0),
            ));
        }
    }
    out
}

/// [`TWO_THEMES`]' themes with a different allow-list.
fn roster_with(allow: &'static [AllowedViolation]) -> Roster {
    Roster {
        themes: TWO_THEMES.themes,
        allow,
    }
}

// ---------------------------------------------------------------------------
// conformance_census
// ---------------------------------------------------------------------------

/// The product is swept: every theme in the roster, at every density.
///
/// A census taken under one theme, or at one rung, is the shape in which a
/// four-preset gate reports three presets' worth of nothing — and every question
/// below is asked *of the census*, so a sweep that lost an axis takes all four
/// with it and every one of them still reads green.
#[test]
fn the_census_sweeps_every_theme_at_every_density() {
    let census = conformance_census(UNDERSIZED, &TWO_THEMES);
    for subject in TWO_THEMES.themes {
        for &density in ALL_DENSITIES {
            assert!(
                census
                    .iter()
                    .any(|v| v.theme.as_str() == subject.id && v.density == density),
                "the census carries no row under {} at {density:?}: {census:#?}",
                subject.id,
            );
        }
    }
}

/// And only the AA rule reaches it. The other two rules are a census, not a
/// gate: a `TouchTargetRecommendation` row arriving here would be a AAA finding
/// reported as a conformance failure by every question below.
#[test]
fn the_census_carries_only_conformance_failures() {
    let census = conformance_census(UNDERSIZED, &TWO_THEMES);
    assert!(!census.is_empty(), "the fixture must produce failures");
    assert!(
        census
            .iter()
            .all(|v| v.rule == TargetRule::MinTargetConformance),
        "a non-AA rule reached the census: {census:#?}",
    );
    // And the walker really does produce one of those other rules on this same
    // fixture, so the filter above is doing work rather than describing a set
    // that happens to be empty.
    assert!(
        audit_fixtures(UNDERSIZED, TargetDensity::Touch)
            .iter()
            .any(|v| !v.rule.is_conformance_failure()),
        "the fixture must also produce a non-AA row for the filter to drop",
    );
}

// ---------------------------------------------------------------------------
// conformance_failures
// ---------------------------------------------------------------------------

/// **The gate reports.** A failure no entry excuses comes back.
#[test]
fn an_unexcused_failure_is_reported() {
    let census = vec![violation(
        "Row > Widget",
        ALPHA,
        TargetDensity::Compact,
        (10.0, 10.0),
        (10.0, 10.0),
    )];
    let findings = conformance_failures(&census, &TWO_THEMES);
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert!(
        findings[0].contains("Row > Widget") && findings[0].contains(ALPHA),
        "a finding names the path and the theme: {findings:#?}",
    );
}

/// And it consults the list rather than reporting everything: the entry's own
/// census is let through.
#[test]
fn an_excused_failure_is_not_reported() {
    assert!(
        conformance_failures(&grip_census(), &TWO_THEMES).is_empty(),
        "a pinned geometry is what an entry excuses",
    );
}

// ---------------------------------------------------------------------------
// stale_entries
// ---------------------------------------------------------------------------

/// A pin whose geometry moved matches nothing, and is reported.
///
/// This is what a pinned measurement is for: the figure is the matcher, so a
/// control that changed size stops being excused *and* the entry stops being
/// justified, in one step.
#[test]
fn an_entry_whose_geometry_moved_is_stale() {
    let census: Vec<TargetViolation> = grip_census()
        .into_iter()
        .map(|mut v| {
            v.size = Size::new(20.0, 20.0);
            v.expanded = Size::new(20.0, 20.0);
            v
        })
        .collect();
    let findings = stale_entries(&census, &TWO_THEMES);
    assert_eq!(
        findings.len(),
        ALL_DENSITIES.len() * 2,
        "every (theme, density) pair the pin claims is reported: {findings:#?}",
    );
    assert!(findings[0].contains("Grip"), "{findings:#?}");
}

/// A pin is held to **every** density it claims, not to one of them.
///
/// An entry that pinned three rungs and only ever meets one carries two
/// exemptions nothing justifies.
#[test]
fn a_pin_claiming_a_density_it_never_meets_is_stale() {
    let census: Vec<TargetViolation> = grip_census()
        .into_iter()
        .filter(|v| v.density != TargetDensity::Touch)
        .collect();
    let findings = stale_entries(&census, &TWO_THEMES);
    assert_eq!(findings.len(), 2, "one per theme: {findings:#?}");
    assert!(
        findings.iter().all(|f| f.contains("Touch")),
        "{findings:#?}"
    );
}

/// And to every theme it claims. A geometry excused under a preset that does not
/// produce it is a regression excused in advance — the shape the Fluent
/// selection pill shipped in.
#[test]
fn a_pin_claiming_a_theme_it_never_meets_is_stale() {
    let census: Vec<TargetViolation> = grip_census()
        .into_iter()
        .filter(|v| v.theme.as_str() != BETA)
        .collect();
    let findings = stale_entries(&census, &TWO_THEMES);
    assert_eq!(findings.len(), ALL_DENSITIES.len(), "{findings:#?}");
    assert!(findings.iter().all(|f| f.contains(BETA)), "{findings:#?}");
}

// ---------------------------------------------------------------------------
// non_narrowing
// ---------------------------------------------------------------------------

/// An entry that cannot tell a 2 dp regression from the geometry it pinned is
/// reported, per axis.
///
/// The seed is 2 dp on one scalar at a time, so what the check asks of every
/// excused violation is: *does driving this axis to 2 dp break the match?* With
/// `PinnedDp`'s two forms it does, unless the pin itself sits at 2 dp —
/// `ClearsFloor` refuses a figure below the floor, which is the half of that
/// form a reader forgets, and an exact `Is` refuses anything a
/// [`PIN_TOLERANCE`](crate::accessibility::target_audit::PIN_TOLERANCE) away.
/// So the entry seeded here pins 2 dp on the two width scalars and
/// `ClearsFloor` on the two height ones, and the check reports exactly the two
/// axes it cannot discriminate on. Which is also the honest statement of what
/// the size half of this question is worth today: it is a standing assertion
/// that nothing in a list admits a 2 dp figure, and it is what would catch a
/// pin written at one, or a third `PinnedDp` form that matched anything.
#[test]
fn a_blanket_entry_survives_a_seeded_size_regression() {
    static BLANKET: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            themes: &[ALPHA, BETA],
            paints: (Is(2.0), ClearsFloor),
            reaches: (Is(2.0), ClearsFloor),
        }],
        owner: "a fixture",
        exception: None,
        why: "a fixture entry",
    }];
    let census: Vec<TargetViolation> = grip_census()
        .into_iter()
        .map(|mut v| {
            v.size = Size::new(2.0, 30.0);
            v.expanded = Size::new(2.0, 30.0);
            v
        })
        .collect();
    let roster = roster_with(BLANKET);
    assert!(
        stale_entries(&census, &roster).is_empty(),
        "the entry matches the real census, so staleness cannot catch it — \
         which is why this question exists",
    );
    let findings = non_narrowing(&census, &roster, 0);
    for axis in [0, 2] {
        assert!(
            findings.iter().any(|f| f.contains(&format!("axis {axis}"))),
            "a 2 dp seed on axis {axis} is still excused and must be reported: \
             {findings:#?}",
        );
    }
    for axis in [1, 3] {
        assert!(
            !findings.iter().any(|f| f.contains(&format!("axis {axis}"))),
            "axis {axis} is pinned `ClearsFloor`, which refuses 2 dp, so the \
             seed there must break the match: {findings:#?}",
        );
    }
}

/// A pin naming a theme the census does not exhibit is reported, even where its
/// own theme's geometry is pinned exactly.
///
/// This is the question the size seeds cannot ask, and the one the Fluent pill
/// needed: `alpha` really does produce this geometry, so every size seed is
/// narrowed; `beta` does not, and the pin excuses it there regardless.
#[test]
fn a_theme_axis_that_decorates_rather_than_narrows_is_reported() {
    let census: Vec<TargetViolation> = grip_census()
        .into_iter()
        .filter(|v| v.theme.as_str() != BETA)
        .collect();
    let findings = non_narrowing(&census, &TWO_THEMES, 0);
    assert!(
        findings.iter().any(|f| f.contains(BETA)),
        "the geometry is excused under {BETA}, where nothing produces it: \
         {findings:#?}",
    );
}

/// The guard's own guard: a census that collapsed proves less than the question
/// above reads as proving, and says so.
#[test]
fn a_census_that_collapsed_is_reported_as_proving_less() {
    let findings = non_narrowing(&grip_census(), &TWO_THEMES, 1_000);
    assert_eq!(findings.len(), 2, "one per theme: {findings:#?}");
    assert!(
        findings.iter().all(|f| f.contains("the census shrank")),
        "{findings:#?}",
    );
}

// ---------------------------------------------------------------------------
// roster_defects
// ---------------------------------------------------------------------------

/// Each of the eleven lints, seeded one at a time.
///
/// A table rather than eleven functions because the shape is identical in every
/// row — build the one defect, assert the lint names it — and because a table is
/// what makes a missing row visible. The row count is asserted, so a lint added
/// without a row here is a failure rather than a silence.
#[test]
fn the_roster_lints() {
    /// A roster naming a theme whose own `ThemeId` is something else.
    static DRIFTED_ID: Roster = Roster {
        themes: &[ThemeSubject {
            id: ALPHA,
            theme: misnamed,
        }],
        allow: &[GRIP],
    };
    /// A roster naming the id every raw-token theme carries.
    static ANONYMOUS_ROSTER: Roster = Roster {
        themes: &[ThemeSubject {
            id: ANONYMOUS_THEME,
            theme: anonymous,
        }],
        allow: &[],
    };
    static NO_WHY: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[SIXTEEN],
        owner: "a fixture",
        exception: None,
        why: "",
    }];
    static NO_OWNER: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[SIXTEEN],
        owner: "",
        exception: None,
        why: "a fixture entry that names nobody and does not say why",
    }];
    static NO_OWNER_EXCUSED: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[SIXTEEN],
        owner: "",
        exception: Some("Inline"),
        why: "No owner: the exception is the answer, not a deferral.",
    }];
    static NO_PIN: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[],
        owner: "a fixture",
        exception: None,
        why: "a fixture entry",
    }];
    // Both positions the lint has to reach: a theme-owned type in the
    // fixture-name-prefixed FIRST segment, which the lint has to strip the name
    // off before it can see, and one in an ordinary later segment.
    static THEME_OWNED_PATH: &[AllowedViolation] = &[AllowedViolation {
        path: "row: FluentRowFrame > MacOsRowFrame > Grip",
        measured: &[SIXTEEN],
        owner: "a fixture",
        exception: None,
        why: "a fixture entry",
    }];
    static NO_DENSITY: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[PinnedGeometry {
            densities: &[],
            ..SIXTEEN
        }],
        owner: "a fixture",
        exception: None,
        why: "a fixture entry",
    }];
    static NO_THEME: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[PinnedGeometry {
            themes: &[],
            ..SIXTEEN
        }],
        owner: "a fixture",
        exception: None,
        why: "a fixture entry",
    }];
    static ANONYMOUS_PIN: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[PinnedGeometry {
            themes: &[ANONYMOUS_THEME],
            ..SIXTEEN
        }],
        owner: "a fixture",
        exception: None,
        why: "a fixture entry",
    }];
    static UNMEASURED_THEME: &[AllowedViolation] = &[AllowedViolation {
        path: "Grip",
        measured: &[PinnedGeometry {
            themes: &["gamma.light"],
            ..SIXTEEN
        }],
        owner: "a fixture",
        exception: None,
        why: "a fixture entry",
    }];

    // (what the lint is, the roster carrying the defect, the census it is asked
    // about, and a substring only that lint's message contains)
    let rows: Vec<(&str, Roster, Vec<TargetViolation>, &str)> = vec![
        (
            "a roster id that has drifted from the theme's own",
            DRIFTED_ID,
            Vec::new(),
            "calls itself",
        ),
        (
            "a roster naming the anonymous id",
            ANONYMOUS_ROSTER,
            Vec::new(),
            "measures no particular preset",
        ),
        (
            "an entry with no justification",
            roster_with(NO_WHY),
            grip_census(),
            "carries no justification",
        ),
        (
            "an entry with no owner and no marker saying why",
            roster_with(NO_OWNER),
            grip_census(),
            "a debt with an address",
        ),
        (
            "an entry that pins nothing",
            roster_with(NO_PIN),
            grip_census(),
            "pins no measurement",
        ),
        (
            "a path across a type one preset owns",
            roster_with(THEME_OWNED_PATH),
            Vec::new(),
            "FluentRowFrame",
        ),
        (
            "a pin claiming no density",
            roster_with(NO_DENSITY),
            grip_census(),
            "claims no density",
        ),
        (
            "a pin claiming no theme",
            roster_with(NO_THEME),
            grip_census(),
            "claims no theme",
        ),
        (
            "a pin naming the anonymous id",
            roster_with(ANONYMOUS_PIN),
            grip_census(),
            "a pin on no preset",
        ),
        (
            "a pin naming a theme the gate does not measure",
            roster_with(UNMEASURED_THEME),
            grip_census(),
            "which this gate does not measure",
        ),
        (
            "a preset that fails and that no entry names",
            roster_with(&[]),
            grip_census(),
            "absent from the list without being clean",
        ),
    ];
    assert_eq!(rows.len(), 11, "one row per lint");

    for (what, roster, census, expected) in &rows {
        let findings = roster_defects(census, roster);
        assert!(
            findings.iter().any(|f| f.contains(expected)),
            "{what}: no finding contains `{expected}`: {findings:#?}",
        );
    }

    // The theme-owned-path row above carries the defect twice, and the lint owes
    // a finding for each: the segment behind a fixture name is the one a plain
    // `split(" > ")` cannot see, and it is the form the real list's own entries
    // are written in.
    let both = roster_defects(&[], &roster_with(THEME_OWNED_PATH));
    assert!(
        both.iter().any(|f| f.contains("`FluentRowFrame`"))
            && both.iter().any(|f| f.contains("`MacOsRowFrame`")),
        "the lint must name the type behind the fixture name as well as the \
         plain one; it reported {both:#?}",
    );

    // The clean roster every row was derived from carries none of them, so each
    // row above reports the defect it seeded and not the fixture's own noise.
    let clean = roster_defects(&grip_census(), &TWO_THEMES);
    assert!(clean.is_empty(), "{clean:#?}");
    // And the owner lint's escape hatch is one spelling rather than a family:
    // an entry whose justification carries the marker is accepted.
    let excused = roster_defects(&grip_census(), &roster_with(NO_OWNER_EXCUSED));
    assert!(excused.is_empty(), "{excused:#?}");
}
