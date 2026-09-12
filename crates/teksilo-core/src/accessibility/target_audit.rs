// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Target-size conformance: measure how big every pointer target in a laid-out
//! tree **actually is to a finger**, and name the ones that fall short.
//!
//! # Reach, not rectangles
//!
//! The naive audit measures a node's rectangle, adds whatever
//! [`Widget::hit_outset`] declares and whatever the miss-only slop pass would
//! grant, and calls the sum the target. Every one of those additions is
//! conditional, and a harness that adds them unconditionally **certifies
//! targets a finger cannot reach**:
//!
//! * an outset is offered only points that *every* ancestor's rectangle already
//!   contains, because the recursion returns before it looks at a child
//!   ([`WidgetArena::hit_test_at_with`]) — so a grip whose wrapper hugs it
//!   claims nothing at all;
//! * the slop pass's top-up is `(target_size − min(w, h)) / 2`, exactly zero for
//!   a control whose own box already reaches `target_size`;
//! * a slop candidate wins only when the exact hit's whole bubble path carries
//!   no eligible handler, or it is strictly closer than the bubble owner — so a
//!   control inside a tappable row gets nothing from it;
//! * and a slot wrapper that intercepts the exact hit while carrying no handler
//!   of its own puts the slop pass back in play, which the arithmetic cannot
//!   see either way.
//!
//! So this module does not add anything up. It **proposes** a growth and then
//! **confirms** it against the real hit test: for each of the four directions
//! out of a target's centre it walks outward and asks
//! [`WidgetTree::hit_test_for`]-equivalent doors who would take a press there,
//! stopping at the first point that no longer actuates the target. What comes
//! back is the reachable cross through the point a user aims at, measured by
//! the same code the pointer router runs, with occlusion and eligibility
//! included because they were never modelled in the first place.
//!
//! Two doors are used, not one: the full one, and the same context
//! [`without_slop`](crate::pointer::hit_slop::HitContext::without_slop). The
//! difference between what they confirm is what attributes a target's reach to
//! [`Widget::hit_outset`] rather than to the slop pass — see [`ReachSources`].
//!
//! # What is a target
//!
//! Two kinds of thing are measured, and the split is the one A10 draws:
//!
//! * **A node** that [`WidgetArena::takes_a_press`] — the framework's own
//!   definition of "would act on a press", and the same predicate the slop
//!   pass's eligibility rule uses. Measured by probing.
//! * **A region** a widget reports from [`Widget::target_regions`] — a scroll
//!   bar's thumb, a slider's knob, a header cell's filter zone. A region is
//!   *measured*, not probed: the split is internal to one node, so no framework
//!   mechanism can widen it and no framework probe can observe it. A region
//!   that abuts its node's edge is credited with the growth confirmed for the
//!   node at that edge, and nothing else.
//!
//! # The three rules
//!
//! | rule | floor | is it a failure? |
//! | --- | --- | --- |
//! | [`MinTargetConformance`](TargetRule::MinTargetConformance) | `min_target_conformance`, 24 dp at **every** density | yes — WCAG 2.2 SC 2.5.8, level AA |
//! | [`SpacingException`](TargetRule::SpacingException) | the same 24 dp, but the target is isolated enough for SC 2.5.8's *spacing* exception | no — a recorded exemption |
//! | [`TouchTargetRecommendation`](TargetRule::TouchTargetRecommendation) | the density's `target_size` (24 / 32 / 44 dp) | no — 44 dp is Apple HIG and SC 2.5.5 **AAA**, never AA |
//!
//! # What a green gate says
//!
//! This module measures; it does not know what it was pointed at. Coverage is
//! whatever the fixture lists name, and there are three — the stock widget
//! catalog's (in the unpublished `teksilo-target-conformance` crate, which is
//! where it can reach a preset), teksilo-charts' and teksilo-scene's. Four
//! crates the touch programme changed own pointer targets and carry none
//! (teksilo-inspector, teksilo-terminal, teksilo-preview-ui, and
//! teksilo-webview, which has nothing this walker can see into), and of the three
//! Compact-visible exceptions `docs/density-inventory.md` §0 enumerates the lists
//! reach none. The widget list sweeps all four shipped presets; charts and scene
//! sweep Int UI alone, each for a written reason. So a green gate says *these
//! fixtures conform*, never *the framework conforms* — the boundary is written
//! out, crate by crate and with the feasibility of each missing list, in
//! `docs/accessibility-internal-audit.md` §3.7.
//!
//! Reference: `docs/density-and-targets.md`, `docs/accessibility-internal-audit.md`.
//!
//! [`Widget::hit_outset`]: crate::widget::Widget::hit_outset
//! [`Widget::target_regions`]: crate::widget::Widget::target_regions
//! [`WidgetArena::hit_test_at_with`]: crate::arena::WidgetArena::hit_test_at_with
//! [`WidgetArena::takes_a_press`]: crate::arena::WidgetArena::takes_a_press
//! [`WidgetTree::hit_test_for`]: crate::widget_tree::WidgetTree::hit_test_for

use teksilo_canvas::{Point, Rect, Size, Transform2D};
use teksilo_tokens::{InputTokens, PointerKind, TargetDensity, TargetRole};

use crate::pointer::hit_slop::HitContext;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

/// The budget for one **axis** of the probe: one more dp than the largest floor
/// this module judges against.
///
/// The budget is per axis, not per direction, and it is the *whole* floor rather
/// than half of it, because a target's reach is not symmetric: a scroll bar
/// flush against the window edge grows only inward, so half a floor per
/// direction would report 19 dp for a bar that a finger reaches across 30. The
/// second direction is probed with whatever the first left over, so an axis that
/// spends the budget is *known* to clear every floor and an axis that does not is
/// measured exactly. A violation's `expanded` is therefore always an exact
/// measurement, never a bound: a target that spent the budget is not a violation.
fn probe_limit(tokens: &InputTokens) -> f32 {
    tokens.target_size.max(tokens.min_target_conformance) + 1.0
}

/// The coarse step of the outward walk, in dp.
///
/// The walk stops at the **first** point that no longer actuates the target, so
/// it reports the contiguous reach rather than the furthest reachable point;
/// that is why it is a linear scan and not a bisection, which could step over a
/// hole and over-report. A reachable band interrupted by a gap narrower than
/// this is not detected.
const PROBE_STEP: f32 = 0.5;

/// Bisection passes used to refine the boundary inside the last
/// [`PROBE_STEP`] interval — 0.5 / 2⁴ ≈ 0.03 dp, fine enough for an exact
/// equality assertion against a token value.
const PROBE_REFINE: u32 = 4;

/// The slack every comparison between a measured **extent** and a dp figure
/// carries, in dp.
///
/// The refinement quantum `PROBE_STEP / 2^PROBE_REFINE` (0.031 dp) bounds the
/// error of one **direction**'s boundary. Every figure this module compares — a
/// reach against a floor, a reach against the paint — is an *extent*, the sum of
/// two directions, so the worst case is two quanta and a one-quantum slack
/// judges a target that is short only by its own measurement. Three quanta
/// rather than two for the same reason [`PIN_TOLERANCE`] carries three — two
/// rows of one control can land either side of a quantum — and the third is
/// margin rather than a census mover: the widget fixture list produces the
/// identical 481-row census at two quanta and at three.
///
/// The slack is **absolute**, not a fraction of the figure it judges, because
/// the error it bounds is absolute: a bisection quantum is the same 0.031 dp on
/// a 16 dp chevron and on a 752 dp menu row. A relative test would spend most of
/// its slack on the large targets that never needed it and under-protect exactly
/// the small ones this audit exists to find.
///
/// **What widening it from the pre-programme 0.05 un-hid, measured.** 0.05 dp is
/// 1.6 quanta, under the worst case it has to cover, and what it mis-read was
/// not a forgiven failure but a **silenced** one: the shadow test in
/// [`Walker::measure_node`](Walker::measure_node) reads "the reach is materially under the
/// paint" as "another target is on top of this one", and a row filed that way
/// carries no verdict at all. Seven rows of the widget list were filed that way
/// and are now judged — the census goes 474 to 481. Four are Fluent Touch tree
/// chevrons, whose reach the probe reports 0.0625 dp (exactly two quanta) under
/// their own 16 dp paint; three are Int UI Compact menu rows, 0.05 dp under
/// their 21.8. Each is the geometry its allow-list entry already names, and no
/// control moved to produce them. The Int UI three are recorded in
/// `docs/accessibility-internal-audit.md` §3.7 as well, because they move an Int
/// UI **Compact** census and the programme's invariant is about that rung.
///
/// Pinned from **both sides** by
/// `the_shadow_slack_covers_an_extent_not_a_direction`: at least the two quanta
/// an extent carries, and at most twice that, so the figure stays a bound on the
/// probe's own error rather than becoming a tolerance on the finding. The
/// shortfall it un-hides is the subject of
/// `a_target_short_of_its_paint_only_by_measurement_noise_is_still_judged`.
const PROBE_EPSILON: f32 = 3.0 * PROBE_STEP / (1 << PROBE_REFINE) as f32;

/// Which rule a target failed to clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetRule {
    /// The reachable extent is below `InputTokens::min_target_conformance` —
    /// **24 dp at every density, never scaled** — and the spacing exception
    /// does not rescue it.
    ///
    /// WCAG 2.2 SC 2.5.8 *Target Size (Minimum)*, level **AA**. The only rule
    /// [`TargetRule::is_conformance_failure`] reports, and the only one the
    /// per-crate gates hold at zero.
    MinTargetConformance,
    /// The reachable extent is below 24 dp, but the target is spaced far enough
    /// from every other target for SC 2.5.8's *spacing* exception to apply.
    ///
    /// Not a failure. It is reported rather than silently forgiven so the audit
    /// is a census: an exemption a reader can see is an exemption a reader can
    /// argue with.
    SpacingException,
    /// The reachable extent is below the density's `target_size` — 24 / 32 /
    /// 44 dp.
    ///
    /// Not a failure, and **not AA**: 44 dp is Apple's Human Interface
    /// Guidelines minimum and WCAG 2.2 SC 2.5.5 *Target Size (Enhanced)*, which
    /// is level AAA. At `Compact` the two floors coincide, so this rule cannot
    /// fire there without `MinTargetConformance` firing as well.
    TouchTargetRecommendation,
}

impl TargetRule {
    /// Whether this rule is a conformance failure — true for
    /// [`MinTargetConformance`](Self::MinTargetConformance) alone.
    pub fn is_conformance_failure(self) -> bool {
        matches!(self, TargetRule::MinTargetConformance)
    }
}

/// Which mechanisms were confirmed to contribute to a target's reach.
///
/// Both flags can be set: a target may take its horizontal reach from an outset
/// and its vertical reach from the slop pass. Neither being set means the reach
/// is the node's own rectangle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ReachSources {
    /// At least one direction's growth was confirmed through the door with the
    /// slop pass switched **off** — so it came from [`Widget::hit_outset`]
    /// inside the exact pass.
    ///
    /// [`Widget::hit_outset`]: crate::widget::Widget::hit_outset
    pub outset: bool,
    /// At least one direction's growth was confirmed only through the **full**
    /// door — so it came from the miss-only slop pass.
    pub slop: bool,
}

impl ReachSources {
    /// Whether any mechanism grew this target past its own rectangle.
    pub fn any(self) -> bool {
        self.outset || self.slop
    }
}

/// One measured target: what it paints, what a finger can reach, and which rule
/// (if any) that reach fails.
#[derive(Debug, Clone)]
pub struct TargetMeasurement {
    /// The widget's concrete Rust type name, as `Widget::type_name` reports it.
    pub widget: &'static str,
    /// The node that owns the target.
    pub node: WidgetId,
    /// `Some(part)` for a region the widget reported from
    /// [`Widget::target_regions`](crate::widget::Widget::target_regions);
    /// `None` for the node's own rectangle.
    pub part: Option<u16>,
    /// Short type names from the arena root down to the node, `>`-separated.
    /// Deliberately not file/line: a path survives a refactor that renumbers a
    /// file.
    pub path: String,
    /// The density ladder this measurement was taken against.
    pub density: TargetDensity,
    /// The theme the tree was built with, read from the tree rather than
    /// supplied — so it cannot disagree with what was measured, the same
    /// by-construction property [`audit_at_density`] gives `density`.
    pub theme: crate::styles::ThemeId,
    /// What the target paints — the node's rectangle, or the region's.
    pub size: Size,
    /// What a coarse pointer can reach: the contiguous cross through the
    /// target's centre, confirmed against the real hit test.
    pub expanded: Size,
    /// Whether either extent ran out of probe budget and is therefore a lower
    /// bound rather than the reach itself.
    ///
    /// The axis that **decides** a verdict is never capped — `probe_limit` is
    /// one dp past the largest floor, so an axis that spends it is proof of
    /// clearing every floor — but the *other* axis of a violation can be, and
    /// then its figure here is the budget spent rather than a boundary. Measured
    /// by `a_violation_can_have_a_capped_axis_and_the_verdict_is_the_other_axis`;
    /// the shipped instances are a `Link` (17 dp tall, wider than the budget) and
    /// a `SpinBox`'s field.
    pub capped: bool,
    /// The mechanisms confirmed to have grown the target.
    pub sources: ReachSources,
    /// Whether a non-identity transform lies between this node and the screen,
    /// so the probed rectangle is the transformed bounding box rather than the
    /// painted shape.
    pub transformed: bool,
    /// The rule the reach fails, if any.
    pub rule: Option<TargetRule>,
    /// What the audit could not measure by probing, when that is the reason
    /// there is no rule verdict — see [`SkipReason`].
    pub skipped: Option<SkipReason>,
}

/// Why a candidate target was measured but not judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkipReason {
    /// A press at the node's centre actuates a **descendant** that takes a
    /// press of its own. The descendant is the target a user aims at and is
    /// audited on its own row; the ancestor is a delegation wrapper.
    DelegatesToDescendant,
    /// A press at the node's centre lands in a different overlay layer. Overlay
    /// stacking is transient and is not a property of the target's size.
    ObscuredByOverlay,
    /// The node's rectangle is empty, so there is no point at which it could be
    /// pressed at all. That is an activation or layout defect and this audit
    /// measures size, not existence.
    EmptyRectangle,
    /// The point a user aims at — the target's centre — is not on screen: it
    /// lies outside the window's client area, or outside a `clips_children`
    /// ancestor's rectangle.
    ///
    /// A data view's rows past the viewport, a control an over-constrained row
    /// pushed out of its panel: the press that would reach them is a scroll or a
    /// resize, not a tap, and their size is audited wherever they are laid into
    /// view. Reporting them would fill the audit with off-screen rows — and
    /// would mis-attribute an over-constraint to target size.
    NotOnScreen,
    /// A `Grab` region enclosed by a `Target` region the same node reported: an
    /// affordance of that target rather than an independent one.
    ///
    /// A slider's knob is what a user aims at, but a press anywhere on the track
    /// takes the value and starts the drag, so the track is the target and the
    /// knob is its indicator — which is what `Slider::target_regions` says in as
    /// many words. The enclosing `Target` is judged in its place. A scroll bar's
    /// thumb has no enclosing Target (its track *pages*, it does not drag) and
    /// stays judged.
    AffordanceOfItsOwnTarget,
    /// The node's reach falls short of its **own painted rectangle**, so what
    /// limits it is not its size: another target — usually an ancestor row or an
    /// overlapping sibling — already owns part of the area this node paints.
    ///
    /// That other target is audited on its own row, and reporting this one as
    /// undersized would mis-attribute an overlapping-targets question to target
    /// size. A table cell inside a selectable row and a swatch grid behind its
    /// swatches are the two shapes this catches.
    ///
    /// The comparison is against the *smaller* of the painted rectangle and the
    /// measurement window, so a wide target whose reach is merely capped is
    /// still judged.
    ShadowedByAnotherTarget,
}

/// A target whose reach fails one of the three rules.
///
/// The shape A19 specifies, plus the fields the recorded ways a target audit can
/// lie forced on it: [`sources`](Self::sources) (which mechanism actually
/// delivers the press) and [`transformed`](Self::transformed) (whether the
/// measurement was taken on a transformed bounding box).
///
/// [`capped`](TargetMeasurement::capped) is **not** carried, and the reason is
/// not that a violation cannot be capped — one can, on the axis that does not
/// decide the verdict. It is that the deciding axis never is, so a violation's
/// verdict rests on an exact measurement; a reader quoting the other axis of a
/// wide violation should quote it as the probe budget.
#[derive(Debug, Clone)]
pub struct TargetViolation {
    /// The widget's concrete Rust type name.
    pub widget: &'static str,
    /// The node that owns the target.
    pub node: WidgetId,
    /// `Some(part)` for a reported region, `None` for the node's rectangle.
    pub part: Option<u16>,
    /// Short type names from the arena root down to the node.
    pub path: String,
    /// The density ladder this violation was found at.
    pub density: TargetDensity,
    /// The theme the tree was built with. Carried because a failure list over
    /// four presets is unreadable without it, and because an allow-list pin
    /// discriminates on it.
    pub theme: crate::styles::ThemeId,
    /// What the target paints.
    pub size: Size,
    /// What a coarse pointer can reach — always an exact measurement.
    pub expanded: Size,
    /// The mechanisms confirmed to have grown the target.
    pub sources: ReachSources,
    /// Whether the measurement was taken on a transformed bounding box.
    pub transformed: bool,
    /// Which rule the reach fails.
    pub rule: TargetRule,
}

impl std::fmt::Display for TargetViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} at {:?} under {}: {}",
            self.rule, self.density, self.theme, self.path
        )?;
        if let Some(part) = self.part {
            write!(f, " [part {part}]")?;
        }
        write!(
            f,
            " — paints {:.1}×{:.1}, reaches {:.1}×{:.1}",
            self.size.width, self.size.height, self.expanded.width, self.expanded.height
        )?;
        if self.sources.any() {
            write!(
                f,
                " (grown by{}{})",
                if self.sources.outset {
                    " hit_outset"
                } else {
                    ""
                },
                if self.sources.slop { " slop" } else { "" },
            )?;
        }
        if self.transformed {
            write!(f, " (transformed)")?;
        }
        Ok(())
    }
}

/// Every pointer target in `tree`, measured against **the tree's own** token
/// ladder — the audit's reasoning, not only its verdict.
///
/// The ladder comes from `tree.theme().input`, not from the generic
/// [`InputTokens::for_density`] table, because a theme may install its own: a
/// preset that raises `target_size` above the generic rung would otherwise be
/// measured against a probe budget smaller than its own recommendation, and the
/// measurement would be capped by the audit rather than by the tree. Reading the
/// tree's tokens is also what makes an app-installed ladder auditable at all.
///
/// `density` names the ladder the tree was **built** at: a control's painted
/// size is baked in `build()`, so judging a Compact tree against the Touch
/// ladder measures a mixture of the two. It must therefore agree with the
/// theme's own `input.density`, which a debug build asserts;
/// [`audit_at_density`] is the door that cannot get it wrong.
pub fn measure_targets(tree: &WidgetTree, density: TargetDensity) -> Vec<TargetMeasurement> {
    let tokens = tree.theme().input;
    debug_assert_eq!(
        tokens.density, density,
        "the tree was built at {:?} and is being judged against {:?}: a \
         control's painted size is baked in `build()`, so this measures a \
         mixture of two ladders. Build the tree at the density you audit, or \
         use `audit_at_density`.",
        tokens.density, density,
    );
    // `tokens.density` rather than the argument: a release build compiles the
    // assertion above away, and a row whose `density` names one ladder while its
    // verdict came from another is incoherent rather than merely wrong.
    let walker = Walker::new(tree, tokens.density, &tokens);
    walker.run()
}

/// Every target in `tree` whose reach fails one of the three rules.
///
/// The gate reads [`TargetRule::is_conformance_failure`] to separate the AA
/// failures from the two informational rules; see the module docs for why all
/// three are returned.
pub fn target_audit(tree: &WidgetTree, density: TargetDensity) -> Vec<TargetViolation> {
    measure_targets(tree, density)
        .into_iter()
        .filter_map(|m| {
            m.rule.map(|rule| TargetViolation {
                widget: m.widget,
                node: m.node,
                part: m.part,
                path: m.path,
                density: m.density,
                theme: m.theme,
                size: m.size,
                expanded: m.expanded,
                sources: m.sources,
                transformed: m.transformed,
                rule,
            })
        })
        .collect()
}

/// Switch `tree` to `density`, re-lay it out at `viewport`, and audit it.
///
/// The door a fixture list should use: it makes the tree's density and the
/// audited ladder the same thing by construction, which is the one way
/// [`target_audit`] can be handed a tree it cannot measure.
pub fn audit_at_density(
    tree: &mut WidgetTree,
    density: TargetDensity,
    viewport: Size,
) -> Vec<TargetViolation> {
    tree.set_input_density(density);
    tree.layout(teksilo_canvas::SizeProposal::exact(
        viewport.width,
        viewport.height,
    ));
    target_audit(tree, density)
}

/// One named subject in a crate's fixture list.
///
/// The audit is driven by an explicit list rather than by the widgets preview
/// catalog, which is an avowedly partial "Coverage in v1" set with a written
/// skip list and no entry for most of the controls the touch programme
/// changed — and which teksilo-scene has none of at all.
///
/// A fixture should build the control **in the container it actually ships in**.
/// A control in a bare stack over-reports its reach: with no eligible handler on
/// the bubble path, the miss-only slop pass catches every near miss, so the
/// fixture certifies a target that a real tappable row would deny. That is the
/// redundancy trap A10 records, and a fixture list is where it is either avoided
/// or walked into wholesale.
pub struct TargetFixture {
    /// The name that prefixes every violation's `path`, so a failure names its
    /// subject before it names a widget type.
    pub name: &'static str,
    /// The viewport to lay the fixture out at.
    pub viewport: Size,
    /// The theme to install **before** the subject is built, so its recipes bake
    /// the audited density's dimensions. Defaults to the IntUI light preset.
    pub theme: fn() -> crate::styles::Theme,
    /// Builds the subject and returns its root. Must not install a theme of its
    /// own — that would discard the density the driver just set.
    pub build: fn(&mut WidgetTree) -> WidgetId,
}

impl TargetFixture {
    /// A fixture at the default 800 × 600 viewport.
    pub const fn new(name: &'static str, build: fn(&mut WidgetTree) -> WidgetId) -> Self {
        Self::sized(name, 800.0, 600.0, build)
    }

    /// A fixture at an explicit viewport, for a subject that needs room (a
    /// docking shell) or deliberately little of it (an overflowing toolbar).
    pub const fn sized(
        name: &'static str,
        width: f32,
        height: f32,
        build: fn(&mut WidgetTree) -> WidgetId,
    ) -> Self {
        Self {
            name,
            viewport: Size { width, height },
            theme: crate::presets::intui::light,
            build,
        }
    }

    /// The same fixture under a named preset — the door a theme crate's own
    /// conformance test uses.
    pub const fn with_theme(mut self, theme: fn() -> crate::styles::Theme) -> Self {
        self.theme = theme;
        self
    }
}

/// Build, lay out and audit every fixture at `density`, tagging each violation
/// with its fixture's name.
///
/// The density is installed **before** the subject is built, not switched
/// afterwards. That is both faithful — a control's painted size is baked in
/// `build()`, so building at the density is what an app launched at that density
/// does — and necessary: `WidgetTree::set_input_density` marks the arena's roots
/// for rebuild, and a layout primitive at the root re-attaches child ids the
/// rebuild has already destroyed, so a switch on a tree rooted in a `Padding` or
/// an `HStack` leaves two nodes where there were nine. A fixture rooted in a
/// composing widget survives a switch; the driver does not depend on which it
/// is.
pub fn audit_fixtures(fixtures: &[TargetFixture], density: TargetDensity) -> Vec<TargetViolation> {
    let mut out = Vec::new();
    for fixture in fixtures {
        let tree = build_fixture(fixture, density);
        for mut violation in target_audit(&tree, density) {
            violation.path = format!("{}: {}", fixture.name, violation.path);
            out.push(violation);
        }
    }
    out
}

/// Build one fixture at `density`, laid out and ready to audit.
fn build_fixture(fixture: &TargetFixture, density: TargetDensity) -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme((fixture.theme)().with_density(density));
    (fixture.build)(&mut tree);
    tree.layout(teksilo_canvas::SizeProposal::exact(
        fixture.viewport.width,
        fixture.viewport.height,
    ));
    tree
}

/// Every measurement every fixture produces, for a test that needs to see the
/// audit's reasoning rather than only its failures.
pub fn measure_fixtures(
    fixtures: &[TargetFixture],
    density: TargetDensity,
) -> Vec<TargetMeasurement> {
    let mut out = Vec::new();
    for fixture in fixtures {
        let tree = build_fixture(fixture, density);
        for mut m in measure_targets(&tree, density) {
            m.path = format!("{}: {}", fixture.name, m.path);
            out.push(m);
        }
    }
    out
}

/// The Tier-3 style slots a theme carries that **no** density projection will
/// re-derive — the styles whose dimensions therefore stay whatever their author
/// wrote them as, at every density.
///
/// Asked by deriving the theme at two densities and naming the slots that are
/// still the same object in both (`ComponentStyleSlots::unchanged_against`),
/// which is what "the ladder did not reach this" means operationally. A shipped
/// preset registers a [`DensityProjection`](crate::styles::DensityProjection)
/// whose function rebuilds every slot the preset owns, so none of a preset's own
/// chrome is named here; a theme with no projection is cloned verbatim, so all
/// of its installed slots are.
///
/// The case it serves is an **app-installed** slot, and it is the reason this is
/// not a one-line `if projection.is_some() { return empty }`. A projection
/// rebuilds the slots *it* owns and leaves the app's alone — that is the
/// contract `DensityProjection`'s own docs state — so an app style installed on
/// top of macOS, Fluent or Material 3 is exactly as frozen as one installed on
/// top of Int UI, and refusing to look at a projected theme reported nothing in
/// the common case. (An app slot in a field the preset *also* owns is a
/// different story: the projection overwrites it at the next density change, so
/// it is not frozen and is correctly not named.)
///
/// Reported rather than fixed: a hand-written Tier-3 style owns its own metrics,
/// and the framework overruling them would be worse than saying so.
pub fn unprojected_style_slots(theme: &crate::styles::Theme) -> Vec<&'static str> {
    let compact = theme.with_density(TargetDensity::Compact);
    let touch = theme.with_density(TargetDensity::Touch);
    compact.style_slots.unchanged_against(&touch.style_slots)
}

// ---------------------------------------------------------------------------
// The allow-list vocabulary the per-crate gates share
// ---------------------------------------------------------------------------

/// Tolerance for a pinned dp figure: the probe's refinement quantum
/// (`PROBE_STEP` / 2^`PROBE_REFINE`, 0.031 dp) three times over, **rounded up**
/// to a round number — 0.09375 dp of error, written 0.1. The rounding is why
/// this is a literal where `PROBE_EPSILON` is an expression: that one is a
/// bound on the probe and must track it exactly; this one is a legibility
/// figure a human writes pins against.
///
/// A pin is therefore written as the round number the geometry implies, and
/// still fails on any dp-scale regression. Three quanta rather than one because
/// the walker reports a boundary up to one quantum short of itself and two rows
/// of the same control can land either side of one.
pub const PIN_TOLERANCE: f32 = 0.1;

/// One axis of a geometry an allow-list entry pins.
///
/// Two forms, because the two carry different weight. An exact figure **is** the
/// finding and must not move without someone re-reading the entry. An axis that
/// already clears the floor is *not* the finding, and on several real entries it
/// is a text measurement — a link's width, a dock tab's label, a legend row's —
/// so pinning a number there would make a gate fail on a font change instead of
/// on a regression. It still asserts something: a value below the floor does not
/// match, so a real shortfall on that axis fails the gate.
#[derive(Clone, Copy, Debug)]
pub enum PinnedDp {
    /// Exactly this many dp, to [`PIN_TOLERANCE`].
    Is(f32),
    /// At least [`InputTokens::min_target_conformance`] — 24 dp at every
    /// density.
    ClearsFloor,
}

impl PinnedDp {
    /// Whether `actual` dp satisfies this pin, against the density's floor.
    pub fn matches(self, actual: f32, floor: f32) -> bool {
        match self {
            PinnedDp::Is(dp) => (actual - dp).abs() <= PIN_TOLERANCE,
            PinnedDp::ClearsFloor => actual + PIN_TOLERANCE >= floor,
        }
    }
}

/// One geometry an allow-list entry excuses: what the target paints, what a
/// coarse pointer reaches, and the densities that pair is measured at.
///
/// This is an entry's **pinned measurement**, and it is the matcher as well —
/// which is the point. A figure written only in prose is a test with no
/// assertion: it can be wrong the day it is written and wronger every package
/// after. A figure that is also the matcher cannot drift, because a violation
/// whose geometry stops equalling it stops being excused and fails its gate.
pub struct PinnedGeometry {
    /// Every density this geometry is measured at. A gate's staleness test holds
    /// an entry to *every* one of them, so it cannot claim a density where the
    /// control actually conforms.
    pub densities: &'static [TargetDensity],
    /// Every theme id this geometry is measured under, matched **literally**.
    ///
    /// The theme axis belongs here rather than on [`AllowedViolation`] because
    /// it is the same question `densities` asks: at which points of the
    /// (theme x density) product does this geometry hold? One entry can excuse
    /// one of its geometries under every preset and another under three.
    ///
    /// A prefix or substring match would be the theme-axis form of a blanket
    /// over a region, which is the failure a pin exists to prevent; and
    /// `ThemeId::default()` is `"custom"`, shared by every app theme, so a
    /// gate's roster check refuses it outright.
    pub themes: &'static [&'static str],
    /// What the target paints.
    pub paints: (PinnedDp, PinnedDp),
    /// What a coarse pointer reaches. A figure equal to the probe budget
    /// (`target_size + 1`: 25 / 33 / 45 dp) is the budget **spent**, not a
    /// boundary — that axis is known to clear every floor and the verdict is the
    /// other axis's — so those are written [`PinnedDp::ClearsFloor`].
    pub reaches: (PinnedDp, PinnedDp),
}

impl PinnedGeometry {
    /// Whether `v` is one of the violations this geometry pins.
    pub fn covers(&self, v: &TargetViolation) -> bool {
        let floor = InputTokens::for_density(v.density).min_target_conformance;
        self.densities.contains(&v.density)
            && self.themes.iter().any(|t| *t == v.theme.as_str())
            && self.paints.0.matches(v.size.width, floor)
            && self.paints.1.matches(v.size.height, floor)
            && self.reaches.0.matches(v.expanded.width, floor)
            && self.reaches.1.matches(v.expanded.height, floor)
    }
}

/// One conformance failure a crate's gate lets through, and why.
///
/// An entry is not a silence: it is a written finding with an address. It
/// carries the WCAG exception it rests on or says plainly that it is a real
/// failure, an [`owner`](Self::owner), and one [`PinnedGeometry`] per geometry
/// it excuses — which is also the only thing it excuses.
///
/// The shape is here rather than in each gate because all three gates need the
/// same matcher and the same rule, and three copies of a matcher is how two of
/// them drift. The *entries* stay in their gates: they are that crate's
/// findings.
///
/// A gate owes this list two tests, and they ask opposite questions. One asks
/// whether every entry, every pinned geometry and every claimed density still
/// matches a real failure — an entry that outlives what it excused is a hole
/// waiting for the next control whose name contains the same substring. The
/// other asks whether an entry matches something it should **not**, by seeding a
/// regression into each geometry the census produces; that is the question a
/// path-only or fixture-name-only matcher fails.
pub struct AllowedViolation {
    /// A substring of the violation's `path`.
    pub path: &'static str,
    /// Every geometry this entry excuses, pinned. Nothing else is excused.
    pub measured: &'static [PinnedGeometry],
    /// Who decides. Empty only where the exception *is* the answer and there is
    /// nothing left to decide.
    pub owner: &'static str,
    /// The SC 2.5.8 exception this rests on, or `None` for a real failure
    /// escalated to its owner. Structural rather than prose, so a roster's
    /// counts can be asserted instead of remembered.
    pub exception: Option<&'static str>,
    /// Why this one is let through.
    pub why: &'static str,
}

impl AllowedViolation {
    /// Whether this entry excuses `v` — its path matches and one of its pinned
    /// geometries covers it.
    pub fn matches(&self, v: &TargetViolation) -> bool {
        v.path.contains(self.path) && self.measured.iter().any(|m| m.covers(v))
    }
}

pub mod gate;

// ---------------------------------------------------------------------------
// The walker
// ---------------------------------------------------------------------------

/// A candidate collected on the tree walk, before any probing.
struct Candidate {
    node: WidgetId,
    widget: &'static str,
    path: String,
    /// The node's own rectangle in screen space (the transformed bounding box
    /// when a transform is in play).
    screen: Rect,
    transformed: bool,
    /// The regions the widget reported, already mapped to screen space.
    regions: Vec<(u16, TargetRole, Rect)>,
}

struct Walker<'a> {
    tree: &'a WidgetTree,
    density: TargetDensity,
    theme: crate::styles::ThemeId,
    tokens: &'a InputTokens,
    limit: f32,
    /// The window's client area, from the tree's last layout proposal.
    ///
    /// A press outside it never reaches this tree, so a probe there must not be
    /// credited — otherwise a control at the leading edge of a window is
    /// reported as reaching into space that is not part of the surface. This is
    /// the sixth way the measurement can over-report, and the only one the
    /// design did not name.
    viewport: Option<Rect>,
}

impl<'a> Walker<'a> {
    fn new(tree: &'a WidgetTree, density: TargetDensity, tokens: &'a InputTokens) -> Self {
        let proposal = tree.last_proposal();
        let viewport = match (proposal.width, proposal.height) {
            (Some(w), Some(h)) => Some(Rect::new(0.0, 0.0, w, h)),
            _ => None,
        };
        Self {
            tree,
            density,
            theme: tree.theme().id.clone(),
            tokens,
            limit: probe_limit(tokens),
            viewport,
        }
    }

    fn run(&self) -> Vec<TargetMeasurement> {
        let candidates = self.collect();
        // Every candidate's *painted* extent, inflated by the largest growth
        // the two mechanisms could offer. Used only by the spacing exception,
        // and deliberately an over-estimate of a neighbour's reach so the
        // exception errs towards NOT excusing an undersized target.
        let neighbours: Vec<Neighbour> = candidates
            .iter()
            .flat_map(|c| {
                std::iter::once((c.node, None, c.screen)).chain(
                    c.regions
                        .iter()
                        .filter(|(_, role, _)| *role != TargetRole::Decoration)
                        .map(move |(part, _, rect)| (c.node, Some(*part), *rect)),
                )
            })
            .map(|(node, part, rect)| Neighbour {
                node,
                part,
                rect: rect.expand(self.max_growth(node, rect)),
            })
            .collect();

        let mut out = Vec::new();
        for candidate in &candidates {
            let mut growth = [0.0_f32; 4];
            if let Some(m) = self.measure_node(candidate, &neighbours, &mut growth) {
                out.push(m);
            }
            // A `Grab` that lies inside a `Target` the SAME node reported is an
            // affordance *of* that target, not an independent one: a slider's
            // knob is where a user aims, but the whole track takes the press and
            // moves it, which is what the widget's own `target_regions` doc
            // says. It is measured and reported so the audit is a census, and
            // judged against no floor — the enclosing Target is judged instead.
            // A grab with no enclosing Target (a scroll bar's thumb, whose track
            // pages rather than dragging) stays judged.
            let enclosing: Vec<Rect> = candidate
                .regions
                .iter()
                .filter(|(_, role, _)| *role == TargetRole::Target)
                .map(|(_, _, rect)| *rect)
                .collect();
            for &(part, role, rect) in &candidate.regions {
                if role == TargetRole::Decoration {
                    continue;
                }
                let affordance =
                    role == TargetRole::Grab && enclosing.iter().any(|t| t.contains(rect.center()));
                let mut m = self.measure_region(candidate, part, role, rect, &growth, &neighbours);
                if affordance {
                    m.rule = None;
                    m.skipped = Some(SkipReason::AffordanceOfItsOwnTarget);
                }
                out.push(m);
            }
        }
        out
    }

    /// The largest outset either widening mechanism could offer a node of
    /// `rect` — an upper bound, unconfirmed, used for the spacing exception's
    /// neighbour extents and by nothing that decides a verdict.
    fn max_growth(&self, node: WidgetId, rect: Rect) -> f32 {
        let arena = &self.tree.arena;
        let outset = arena
            .get(node)
            .map(|n| n.widget.hit_outset(PointerKind::Touch, self.tokens))
            .unwrap_or(teksilo_canvas::EdgeInsets::ZERO);
        let widest = outset
            .top
            .max(outset.bottom)
            .max(outset.leading)
            .max(outset.trailing);
        let slop = crate::pointer::hit_slop::HitSlop::for_pointer(PointerKind::Touch, self.tokens)
            .outset_for(rect.size());
        if widest.is_finite() {
            widest.max(slop)
        } else {
            slop
        }
    }

    fn collect(&self) -> Vec<Candidate> {
        let mut out = Vec::new();
        for root in self.tree.roots() {
            self.collect_from(root, &mut Vec::new(), &mut out);
        }
        out
    }

    fn collect_from(&self, id: WidgetId, path: &mut Vec<&'static str>, out: &mut Vec<Candidate>) {
        let arena = &self.tree.arena;
        if !arena.is_active(id) {
            return;
        }
        let Some(node) = arena.get(id) else { return };
        // A decorative subtree is pruned whole by the exact pass, so nothing
        // inside it is a target.
        if node.hit_transparent {
            return;
        }
        let widget = node.widget.type_name();
        path.push(short_name(widget));
        let bounds = arena.bounds(id);
        let (screen, transformed) = self.to_screen(id, bounds);
        // `target_regions` answers in the space it was asked about, so ask in
        // bounds space and map the answers the same way the node's own
        // rectangle was mapped.
        let regions: Vec<(u16, TargetRole, Rect)> = node
            .widget
            .target_regions(bounds)
            .into_iter()
            .map(|r| (r.part, r.role, self.to_screen(id, r.rect).0))
            .collect();
        // `event_pass_through` absorbs nothing itself; `takes_a_press` is the
        // framework's own "would act on a press", the same predicate the slop
        // pass's eligibility rule reads.
        let is_target = arena.takes_a_press(id) && !node.event_pass_through;
        if is_target || !regions.is_empty() {
            out.push(Candidate {
                node: id,
                widget,
                path: path.join(" > "),
                screen,
                transformed,
                regions: if is_target { regions } else { Vec::new() },
            });
        }
        for &child in arena.children(id) {
            self.collect_from(child, path, out);
        }
        path.pop();
    }

    /// Map a rectangle in `id`'s bounds space to screen space.
    fn to_screen(&self, id: WidgetId, rect: Rect) -> (Rect, bool) {
        let arena = &self.tree.arena;
        // A content transform (SceneView) maps its *children*, not itself, so
        // the node's own rectangle lives in its parent's effective space.
        let content = arena.get(id).map(|n| n.content_transform).unwrap_or(false);
        let t = if content {
            arena
                .parent(id)
                .map(|p| arena.effective_transform(p))
                .unwrap_or(Transform2D::IDENTITY)
        } else {
            arena.effective_transform(id)
        };
        if t.is_identity() {
            (rect, false)
        } else {
            (t.apply_rect(rect), true)
        }
    }

    fn measure_node(
        &self,
        candidate: &Candidate,
        neighbours: &[Neighbour],
        growth: &mut [f32; 4],
    ) -> Option<TargetMeasurement> {
        let arena = &self.tree.arena;
        if !arena.takes_a_press(candidate.node) {
            return None;
        }
        let rect = candidate.screen;
        let base =
            |skipped: Option<SkipReason>, expanded: Size, capped, sources| TargetMeasurement {
                widget: candidate.widget,
                node: candidate.node,
                part: None,
                path: candidate.path.clone(),
                density: self.density,
                theme: self.theme.clone(),
                size: rect.size(),
                expanded,
                capped,
                sources,
                transformed: candidate.transformed,
                rule: None,
                skipped,
            };
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return Some(base(
                Some(SkipReason::EmptyRectangle),
                Size::new(0.0, 0.0),
                false,
                ReachSources::default(),
            ));
        }
        let centre = rect.center();
        if !self.on_screen(candidate.node, centre) {
            return Some(base(
                Some(SkipReason::NotOnScreen),
                Size::new(0.0, 0.0),
                false,
                ReachSources::default(),
            ));
        }
        match self.owner_at(centre, true) {
            Some(owner) if owner == candidate.node => {}
            Some(owner) if self.is_descendant(owner, candidate.node) => {
                return Some(base(
                    Some(SkipReason::DelegatesToDescendant),
                    Size::new(0.0, 0.0),
                    false,
                    ReachSources::default(),
                ));
            }
            other => {
                // A hit that landed in an overlay layer this node is not part
                // of is transient stacking, not a size property. Anything else
                // — a sibling painted on top, nothing at all — leaves the
                // target genuinely unreachable at the point a user aims at.
                if other.is_some_and(|o| self.in_foreign_overlay(o, candidate.node)) {
                    return Some(base(
                        Some(SkipReason::ObscuredByOverlay),
                        Size::new(0.0, 0.0),
                        false,
                        ReachSources::default(),
                    ));
                }
                let mut m = base(None, Size::new(0.0, 0.0), false, ReachSources::default());
                m.rule = Some(TargetRule::MinTargetConformance);
                return Some(m);
            }
        }

        let mut sources = ReachSources::default();
        let mut capped = false;
        // Per **axis**, because that is what the shadow test below needs: an
        // axis that spent its whole budget stopped for lack of probe, not for
        // lack of target, and comparing such a reach against the paint would
        // report every target wider than one floor as shadowed.
        let mut capped_axis = [false; 2];
        let mut extent = [0.0_f32; 4];
        for (index, dir) in DIRECTIONS.iter().enumerate() {
            // The budget belongs to the axis: the second direction gets what the
            // first left over, so a one-sided reach is measured in full.
            let spent = if index % 2 == 0 {
                0.0
            } else {
                extent[index - 1]
            };
            let budget = (self.limit - spent).max(0.0);
            let reach = self.reach(centre, *dir, candidate.node, budget);
            extent[index] = reach.distance;
            capped |= reach.capped;
            capped_axis[index / 2] |= reach.capped;
            let inside = match index {
                0 | 1 => rect.width / 2.0,
                _ => rect.height / 2.0,
            };
            growth[index] = (reach.distance - inside).max(0.0);
            if reach.distance > inside + PROBE_STEP {
                if reach.confirmed_without_slop {
                    sources.outset = true;
                } else {
                    sources.slop = true;
                }
            }
        }
        let expanded = Size::new(extent[0] + extent[1], extent[2] + extent[3]);
        let mut m = base(None, expanded, capped, sources);
        // A node that does not even own the rectangle it paints is limited by
        // another target, not by its size — so the reach is compared against
        // the paint, on each axis, and **only where the probe actually reached
        // a boundary**.
        //
        // A `capped` axis spent its whole [`probe_limit`] budget without finding
        // one: it stopped for lack of probe, not for lack of target, so its
        // reach is a lower bound and says nothing about who owns the rest of the
        // paint. Comparing it against the paint reports every target wider than
        // one floor as shadowed by arithmetic alone, which is how a 320 dp chart
        // and a 73 dp button in its centre slot both came back
        // `ShadowedByAnotherTarget` with nothing on top of them — and, worse,
        // never judged. A capped axis needs no verdict of its own: `probe_limit`
        // is one dp past the largest floor, so an axis that spends it is known
        // to clear every floor.
        //
        // [`PROBE_EPSILON`] is the slack, and it is the same one
        // [`classify`](Self::classify) uses: both compare an extent, so both
        // carry two directions' worth of refinement error.
        let shadowed = expanded.width > 0.0
            && expanded.height > 0.0
            && ((!capped_axis[0] && expanded.width + PROBE_EPSILON < rect.width)
                || (!capped_axis[1] && expanded.height + PROBE_EPSILON < rect.height));
        if shadowed {
            m.skipped = Some(SkipReason::ShadowedByAnotherTarget);
            return Some(m);
        }
        m.rule = self.classify(
            expanded,
            rect,
            TargetRole::Target,
            (candidate.node, None),
            neighbours,
        );
        Some(m)
    }

    /// A reported region is **measured**, not probed: the split is internal to
    /// one node, so no framework mechanism widens it and no framework probe can
    /// observe it. The one credit it takes is the growth the node itself was
    /// *confirmed* to have at an edge the region shares with it — so a scroll
    /// bar's track, which spans the bar's whole width, inherits the bar's own
    /// outset, and an interior zone inherits nothing.
    #[allow(clippy::too_many_arguments)]
    fn measure_region(
        &self,
        candidate: &Candidate,
        part: u16,
        role: TargetRole,
        rect: Rect,
        growth: &[f32; 4],
        neighbours: &[Neighbour],
    ) -> TargetMeasurement {
        let node = candidate.screen;
        if !self.on_screen(candidate.node, rect.center()) {
            return TargetMeasurement {
                widget: candidate.widget,
                node: candidate.node,
                part: Some(part),
                path: candidate.path.clone(),
                density: self.density,
                theme: self.theme.clone(),
                size: rect.size(),
                expanded: Size::new(0.0, 0.0),
                capped: false,
                sources: ReachSources::default(),
                transformed: candidate.transformed,
                rule: None,
                skipped: Some(SkipReason::NotOnScreen),
            };
        }
        let mut grown = rect;
        // Reading order is already resolved: these are screen edges.
        let shares = [
            (rect.x - node.x).abs() <= 0.01,
            (rect.right() - node.right()).abs() <= 0.01,
            (rect.y - node.y).abs() <= 0.01,
            (rect.bottom() - node.bottom()).abs() <= 0.01,
        ];
        if shares[0] {
            grown.x -= growth[0];
            grown.width += growth[0];
        }
        if shares[1] {
            grown.width += growth[1];
        }
        if shares[2] {
            grown.y -= growth[2];
            grown.height += growth[2];
        }
        if shares[3] {
            grown.height += growth[3];
        }
        TargetMeasurement {
            widget: candidate.widget,
            node: candidate.node,
            part: Some(part),
            path: candidate.path.clone(),
            density: self.density,
            theme: self.theme.clone(),
            size: rect.size(),
            expanded: grown.size(),
            capped: false,
            sources: ReachSources::default(),
            transformed: candidate.transformed,
            rule: self.classify(
                grown.size(),
                rect,
                role,
                (candidate.node, Some(part)),
                neighbours,
            ),
            skipped: None,
        }
    }

    /// Judge a reach against the three rules.
    ///
    /// `role` decides only the recommendation floor: a `Grab` region's
    /// `grab_size` is a **paint** size the design fixes on purpose ("visual
    /// fixed, hit grows"), so a grab is held to the 24 dp conformance floor and
    /// to nothing above it.
    fn classify(
        &self,
        reach: Size,
        painted: Rect,
        role: TargetRole,
        identity: (WidgetId, Option<u16>),
        neighbours: &[Neighbour],
    ) -> Option<TargetRule> {
        let smaller = reach.width.min(reach.height);
        if smaller + PROBE_EPSILON < self.tokens.min_target_conformance {
            return Some(
                if self.spacing_exception_applies(painted, identity, neighbours) {
                    TargetRule::SpacingException
                } else {
                    TargetRule::MinTargetConformance
                },
            );
        }
        let recommended = match role {
            TargetRole::Grab => self.tokens.min_target_conformance,
            _ => self.tokens.target_size,
        };
        if smaller + PROBE_EPSILON < recommended {
            return Some(TargetRule::TouchTargetRecommendation);
        }
        None
    }

    /// WCAG 2.2 SC 2.5.8's *spacing* exception: a 24 dp circle centred on the
    /// undersized target's bounding box may not intersect another target, nor
    /// another undersized target's circle.
    ///
    /// Neighbour extents are the **inflated** rectangles computed in
    /// [`Walker::run`], so the test errs towards refusing the exception. The
    /// target's own rectangle is excluded by an exact-equality check rather
    /// than by identity, which also excludes a region that coincides with its
    /// node.
    fn spacing_exception_applies(
        &self,
        painted: Rect,
        identity: (WidgetId, Option<u16>),
        neighbours: &[Neighbour],
    ) -> bool {
        let radius = self.tokens.min_target_conformance / 2.0;
        let centre = painted.center();
        for neighbour in neighbours {
            if (neighbour.node, neighbour.part) == identity {
                continue;
            }
            if crate::pointer::hit_slop::rect_distance(neighbour.rect, centre) < radius {
                return false;
            }
        }
        true
    }

    /// The contiguous reach out of `centre` along `dir`, and which door
    /// confirmed the last point of it.
    fn reach(&self, centre: Point, dir: (f32, f32), node: WidgetId, limit: f32) -> Reach {
        let mut last = 0.0_f32;
        let mut d = PROBE_STEP;
        while d <= limit {
            if self.actuates(centre, dir, d, node, true) {
                last = d;
                d += PROBE_STEP;
            } else {
                break;
            }
        }
        if last + PROBE_STEP > limit {
            return Reach {
                distance: last,
                capped: true,
                confirmed_without_slop: last > 0.0 && self.actuates(centre, dir, last, node, false),
            };
        }
        // Refine inside the failed step: the boundary is somewhere in
        // (last, last + PROBE_STEP).
        let mut low = last;
        let mut high = (last + PROBE_STEP).min(limit);
        for _ in 0..PROBE_REFINE {
            let mid = (low + high) / 2.0;
            if self.actuates(centre, dir, mid, node, true) {
                low = mid;
            } else {
                high = mid;
            }
        }
        Reach {
            distance: low,
            capped: false,
            confirmed_without_slop: low > 0.0 && self.actuates(centre, dir, low, node, false),
        }
    }

    fn actuates(
        &self,
        from: Point,
        dir: (f32, f32),
        distance: f32,
        node: WidgetId,
        with_slop: bool,
    ) -> bool {
        let at = Point::new(from.x + dir.0 * distance, from.y + dir.1 * distance);
        if self.viewport.is_some_and(|v| !v.contains(at)) {
            return false;
        }
        self.owner_at(at, with_slop) == Some(node)
    }

    /// Who would act on a press at `at` — the deepest node on the hit's own
    /// path that [`WidgetArena::takes_a_press`], which is the framework's
    /// definition of an eligible handler.
    ///
    /// [`WidgetArena::takes_a_press`]: crate::arena::WidgetArena::takes_a_press
    fn owner_at(&self, at: Point, with_slop: bool) -> Option<WidgetId> {
        let surfaces = self.tree.text_surfaces();
        let read_only = |id: WidgetId| surfaces.is_read_only(id);
        let hit = HitContext::new(PointerKind::Touch, self.tokens)
            .direction(self.tree.layout_direction)
            .read_only_probe(&read_only);
        let hit = if with_slop { hit } else { hit.without_slop() };
        let target = self.tree.hit_test_with(at, None, None, &hit)?;
        let mut current = Some(target);
        while let Some(id) = current {
            if self.tree.arena.takes_a_press(id) {
                return Some(id);
            }
            current = self.tree.arena.parent(id);
        }
        None
    }

    /// Whether `point` is inside the window and inside every `clips_children`
    /// ancestor of `id` — the two ways a laid-out target can be off screen.
    fn on_screen(&self, id: WidgetId, point: Point) -> bool {
        if self.viewport.is_some_and(|v| !v.contains(point)) {
            return false;
        }
        let arena = &self.tree.arena;
        let mut current = arena.parent(id);
        while let Some(ancestor) = current {
            if arena
                .get(ancestor)
                .map(|n| n.clips_children)
                .unwrap_or(false)
            {
                let (rect, _) = self.to_screen(ancestor, arena.bounds(ancestor));
                if !rect.contains(point) {
                    return false;
                }
            }
            current = arena.parent(ancestor);
        }
        true
    }

    fn is_descendant(&self, maybe_descendant: WidgetId, of: WidgetId) -> bool {
        let mut current = self.tree.arena.parent(maybe_descendant);
        while let Some(id) = current {
            if id == of {
                return true;
            }
            current = self.tree.arena.parent(id);
        }
        false
    }

    /// Whether `owner` sits inside an overlay layer that does not contain
    /// `node` — the stacking case that is not a target-size property.
    fn in_foreign_overlay(&self, owner: WidgetId, node: WidgetId) -> bool {
        let contains = |root: WidgetId, id: WidgetId| id == root || self.is_descendant(id, root);
        self.tree
            .overlay_manager
            .active_content_ids()
            .into_iter()
            .any(|content| contains(content, owner) && !contains(content, node))
    }
}

struct Reach {
    distance: f32,
    capped: bool,
    confirmed_without_slop: bool,
}

/// leading, trailing, up, down — in screen space, so the reading direction is
/// already resolved by the hit context.
const DIRECTIONS: [(f32, f32); 4] = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)];

/// One other target's outer extent, for the spacing exception. Identity is
/// carried so a target is never its own neighbour — inflating the rectangle
/// makes a geometric self-match impossible to spot.
struct Neighbour {
    node: WidgetId,
    part: Option<u16>,
    rect: Rect,
}

/// The last `::`-separated component of a Rust type path, with any generic
/// arguments dropped — `teksilo_widgets::button::Button` → `Button`.
fn short_name(type_name: &'static str) -> &'static str {
    let head = type_name.split('<').next().unwrap_or(type_name);
    head.rsplit("::").next().unwrap_or(head)
}

#[cfg(test)]
mod tests;
