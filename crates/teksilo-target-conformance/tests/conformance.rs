// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The gate: every target in the named fixture list, measured at all three
//! densities, with the written allow-list as the only exception.
//!
//! The fixture list itself, and what each fixture must be seen to measure, live
//! in this crate's library half — see its module docs for why the list is a
//! crate rather than a test in teksilo-widgets.

use std::rc::Rc;

use teksilo_canvas::Size;
use teksilo_core::accessibility::target_audit::TargetFixture;
use teksilo_core::accessibility::target_audit::{
    AllowedViolation, Owner, PIN_TOLERANCE,
    PinnedDp::{ClearsFloor, Is},
    PinnedGeometry, TargetRule, TargetViolation, audit_fixtures, gate, measure_fixtures,
    unprojected_style_slots,
};
use teksilo_core::presets::intui;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::lit;
use teksilo_target_conformance::{
    EXPECTED_NON_TARGETS, EXPECTED_SUBJECTS, in_tappable_row, widget_fixtures,
};
use teksilo_tokens::{Color, InputTokens, TargetDensity};
use teksilo_widgets::primitives::{HStack, Padding, TextWidget};
use teksilo_widgets::*;

// =========================================================================
// The gate
// =========================================================================

/// The three rungs, from the shared gate rather than restated here — two copies
/// of one ladder is how they drift.
const ALL_DENSITIES: &[TargetDensity] = gate::ALL_DENSITIES;
const AT_COMPACT: &[TargetDensity] = &[TargetDensity::Compact];
const AT_COMFORTABLE: &[TargetDensity] = &[TargetDensity::Comfortable];
const AT_TOUCH: &[TargetDensity] = &[TargetDensity::Touch];
const ABOVE_COMPACT: &[TargetDensity] = &[TargetDensity::Comfortable, TargetDensity::Touch];

/// The presets the gate measures, one **family representative** each.
///
/// Four ids, not eight: light and dark are geometrically identical in every
/// shipped preset — a claim [`light_and_dark_are_the_same_geometry_in_every_preset`]
/// turns from an assumption into an assertion, and which halves the sweep.
const INTUI: &str = "intui.light";
const MACOS: &str = "macos.light";
const FLUENT: &str = "fluent.light";
const M3: &str = "material3.light";

/// Every theme id in the roster — what a pin writes when a geometry is
/// preset-independent, which most of them are.
const ALL_THEMES: &[&str] = &[INTUI, MACOS, FLUENT, M3];

static ROSTER: gate::Roster = gate::Roster {
    themes: &[
        gate::ThemeSubject {
            id: INTUI,
            theme: intui::light,
        },
        gate::ThemeSubject {
            id: MACOS,
            theme: teksilo_theme_macos::light,
        },
        gate::ThemeSubject {
            id: FLUENT,
            theme: teksilo_theme_fluent::light,
        },
        gate::ThemeSubject {
            id: M3,
            theme: teksilo_theme_material3::light,
        },
    ],
    allow: ALLOW_LIST,
};

/// Every conformance failure the fixture list produces under every preset,
/// computed **once** per test binary.
///
/// The sweep is four presets x three densities x the whole fixture list, so a
/// gate that recomputed it per test would pay for it four times over. The memo
/// is a `OnceLock` rather than a `lazy_static`-style cell because the tests run
/// as threads in one process and each of them wants the same answer.
fn census() -> &'static [TargetViolation] {
    static CENSUS: gate::CensusCell = gate::CensusCell::new();
    CENSUS.get(&widget_fixtures(), &ROSTER)
}

/// The findings, each measured, none of them a mechanism that could be switched
/// on to clear it.
///
/// The counts a reader needs are asserted rather than written here: see
/// [`the_allow_lists_roster_is_what_it_says_it_is`], which pins how many entries
/// there are, how many rest on an exception, and how many are escalated. The
/// enumeration of Compact-visible exceptions the programme has taken lives in
/// `docs/density-inventory.md` §0 and nowhere else, for the same reason — a
/// count in prose is a count that drifts.
///
/// Two shapes recur. A control that paints under 24 dp on one axis **packed
/// against another target of its own kind** has no point left for either
/// widening mechanism to claim (the step buttons, the swatches, the window
/// corners). And a control whose declared `Widget::hit_outset` is **hugged by a
/// wrapper** is offered no point outside its own box, because the arena consults
/// the hook only among a node's direct children and returns before it looks at
/// one whose parent's rectangle excludes the point — A10's reach limit, and the
/// tree chevron is the shipped instance of it.
///
/// All of them are real WCAG 2.2 SC 2.5.8 (AA) shortfalls. They are here rather
/// than fixed because every remedy left is a **layout** change at Compact, and
/// the programme's invariant is that Compact does not move: taking another such
/// exception is a decision for the owner named in each entry, not for the audit
/// that found it.
const ALLOW_LIST: &[AllowedViolation] = &[
    AllowedViolation {
        path: "StepButton",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            themes: ALL_THEMES,
            paints: (Is(18.0), Is(13.0)),
            reaches: (Is(18.0), Is(13.0)),
        }],
        owner: Owner::Named("whoever revisits SpinBox's step geometry"),
        exception: Some("Equivalent"),
        why: "A SpinBox's two step buttons are stacked halves of the field's \
              `TEXT_FIELD_HEIGHT` box, and the pin says what they get from the \
              two widening mechanisms: nothing at all, at every density -- the \
              reach IS the paint. Vertically there is nothing to claim, because \
              the two are adjacent and two rings meet at the midpoint; \
              horizontally an outset could reach the floor on that axis alone, \
              which would not clear the violation. WCAG 2.2 SC 2.5.8 \
              *Equivalent*: the same value is set by typing in the field, by \
              Up/Down, and by the wheel. docs/density-inventory.md named \
              partition_targets as this row's mechanism, which cannot be right \
              -- the step buttons are their own nodes beside their own sibling, \
              the same error P25 corrected for the SplitButton chevron; that \
              row is corrected.",
    },
    AllowedViolation {
        path: "ColorSwatch",
        measured: &[
            // The ColorPicker's grid: rows are flush, so the vertical earns
            // nothing and the height is what falls short.
            PinnedGeometry {
                densities: AT_COMPACT,
                themes: ALL_THEMES,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(23.0), Is(22.0)),
            },
            PinnedGeometry {
                densities: AT_COMPACT,
                themes: ALL_THEMES,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(24.0), Is(22.0)),
            },
            PinnedGeometry {
                densities: ABOVE_COMPACT,
                themes: ALL_THEMES,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(25.0), Is(22.0)),
            },
            PinnedGeometry {
                densities: ABOVE_COMPACT,
                themes: ALL_THEMES,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(28.0), Is(22.0)),
            },
            // A single HStack run: the height earns its dp, and the two
            // swatches at the ends of the run are flush against the run's own
            // edge, so the width is what falls short -- at Compact only.
            PinnedGeometry {
                densities: AT_COMPACT,
                themes: ALL_THEMES,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(23.0), Is(24.0)),
            },
        ],
        owner: Owner::Named("whoever revisits ColorPicker's grid geometry"),
        exception: Some("Equivalent"),
        why: "A 22 dp swatch, 2 dp under the floor, whose ring earns back at \
              most 1 dp per side because every neighbour is a swatch: where two \
              rings meet the arena splits the gap halfway. Which axis falls \
              short depends on the run it is in, and both shapes are pinned \
              above -- flush rows in the ColorPicker's grid leave the height \
              short at every density, and the ends of a single HStack run leave \
              the width short at Compact only, since Comfortable's wider ring \
              clears it. WCAG 2.2 SC 2.5.8 *Equivalent*: the ColorPicker sets \
              the same value through its H/S/V spin boxes and its hex field. \
              The geometric remedy is a further docs/density-inventory.md \u{a7}0 \
              entry -- SWATCH_SIZE 22 -> 24 dp, Compact included, exactly as \
              the previewer's navigator row and the macOS control height were \
              raised -- which would clear both shapes at once.",
    },
    AllowedViolation {
        path: "Expand > Padding > TextInputField",
        measured: &[PinnedGeometry {
            densities: AT_COMPACT,
            themes: ALL_THEMES,
            paints: (ClearsFloor, Is(20.0)),
            reaches: (ClearsFloor, Is(20.0)),
        }],
        owner: Owner::Named(
            "whoever revisits SpinBox's frame inset (the same owner as the \
                step buttons above)",
        ),
        exception: None,
        why: "A SpinBox's editable field is 20 dp tall inside the frame's own \
              box because the frame reserves space above and below it, and it \
              reaches exactly that 20 dp -- the width clears the floor and is \
              not the finding. Compact only: at Comfortable and Touch the box's \
              own MinSize carries the field to 24 and 36 dp, which is the \
              density ladder working. Neither mechanism can serve it: the step \
              buttons beside it are targets at distance zero, so the slop pass \
              is denied, and an outset would have to grow into the frame it is \
              inset from. A real AA shortfall with no exception behind it -- \
              the field IS the target for placing a caret, and no conforming \
              control does that job. The remedy is a taller field at Compact, \
              which the invariant forbids.",
    },
    AllowedViolation {
        // Scoped to the fixture, deliberately: the POPULATED field's clear slot
        // is the same `HitTarget` node on the same `HStack > HitTarget` path,
        // and 16 x 16 reaching 22 x 24 at Compact is exactly the regression
        // signature `an_outsets_claim_survives_the_slop_pass_in_the_shipped_controls`
        // exists to refuse — that test runs under Int UI alone, so an unscoped
        // entry would pre-excuse the populated slot's regression under the
        // other three presets.
        path: "search_field/empty:",
        measured: &[PinnedGeometry {
            densities: AT_COMPACT,
            themes: ALL_THEMES,
            paints: (Is(16.0), Is(16.0)),
            reaches: (Is(22.0), Is(24.0)),
        }],
        owner: Owner::Named(
            "whoever decides whether a clear affordance should exist at all \
                on an empty query",
        ),
        exception: None,
        why: "A `SearchField`'s 16 dp clear slot **while the query is empty**, \
              measured by the `search_field/empty` fixture under every preset. \
              With a query typed the same slot reaches 24 x 24 and does not \
              appear here at all; empty, it reaches 22 x 24 and is 2 dp short on \
              the width -- because `HitTarget::active` withdraws the OUTSET when \
              the affordance is hidden and nothing withdraws the target. The \
              node keeps its `on_tap` and its `CursorIcon::Pointer`, so a press \
              on an empty field's trailing 16 dp is swallowed by an affordance \
              that is not painted, does nothing, and shows a hand while it does \
              it -- and the caret the press was aiming at is not placed. The \
              22 dp is what the miss-only slop pass makes of it (`src=slop`), \
              which is also why this is Compact only: above it the wider ring \
              clears the floor. NOT a size finding, and that is why no \
              SC 2.5.8 exception is claimed: a bigger invisible slot would be \
              worse. The discharge is to stop being a target when inactive, and \
              it is not a one-line change -- an outset must be declared by the \
              node that TAKES the press, so the handler cannot simply move to \
              the `visible_when`-gated glyph inside, and `HitTarget` has no \
              reactive way to drop a handler. Until then the figure is measured \
              rather than described, which is the whole difference between this \
              entry and the prose it replaces in docs/touch-and-pen.md.",
    },
    AllowedViolation {
        path: "HStack > FixedSize > TwistArrow",
        measured: &[
            // In a TreeView the wrapper hugs the chevron and no mechanism is
            // credited, so the reach is the paint at every density -- to the
            // probe's own precision, which is what `Is(16.0)` means here and
            // not literal equality. The walk reports an extent up to two
            // refinement quanta (0.031 dp each) short of itself, and these rows
            // land there: 15.96875 under every theme and density except Fluent
            // at Touch, where it is 15.9375. `PIN_TOLERANCE` is three quanta
            // wide for exactly this, and `PROBE_EPSILON` is why the Fluent Touch
            // four are judged at all rather than filed as shadowed by something
            // on top of them.
            PinnedGeometry {
                densities: ALL_DENSITIES,
                themes: ALL_THEMES,
                paints: (Is(16.0), Is(16.0)),
                reaches: (Is(16.0), Is(16.0)),
            },
            // A single StandardTreeItem on a page has no tappable row around
            // it, so the miss-only pass reaches this one -- and still leaves the
            // width short at Compact. A different geometry, named rather than
            // absorbed by the entry above.
            PinnedGeometry {
                densities: AT_COMPACT,
                themes: ALL_THEMES,
                paints: (Is(16.0), Is(16.0)),
                reaches: (Is(20.0), Is(24.0)),
            },
        ],
        owner: Owner::Named("whoever revisits the tree row's indent column"),
        exception: None,
        why: "The tree chevron's outset is INERT in the one composition it \
              ships in, and this key names the reason: \
              `StandardTreeItem::build` wraps it in \
              `FixedSize::new().width(chevron_size)`, a wrapper exactly the \
              chevron's own size on both axes. An outset is only ever offered \
              points every ancestor's rectangle already contains, so a hugged \
              grip claims nothing -- A10's reach limit, measured here in a \
              shipped widget rather than the inspector. \
              `the_chevrons_outset_is_inert_inside_a_standard_tree_row` pins \
              both halves: the reach equals the paint with no mechanism \
              credited, while the same chevron unhugged in \
              `twist_arrow_in_a_row` reaches 24 x 24 through its outset. \
              Deleting the wrapper is necessary and NOT sufficient -- at depth \
              0 the chevron is flush against the row's content box, so the \
              ring's leading half falls outside the HStack and the reach is \
              still short at Compact.",
    },
    AllowedViolation {
        path: "CellA11y > Padding > HStack > TwistArrow",
        measured: &[
            PinnedGeometry {
                densities: AT_COMPACT,
                themes: ALL_THEMES,
                paints: (Is(12.0), Is(12.0)),
                reaches: (Is(18.0), Is(24.0)),
            },
            PinnedGeometry {
                densities: AT_COMFORTABLE,
                themes: ALL_THEMES,
                paints: (Is(12.0), Is(12.0)),
                reaches: (Is(22.0), Is(28.0)),
            },
        ],
        owner: Owner::Named(
            "whoever revisits the tree table's indent column (the same \
                question as the entry above)",
        ),
        exception: Some("Equivalent"),
        why: "The same chevron in a TreeTableView cell, 12 dp rather than 16 \
              and NOT hugged -- its outset is credited, and the pins say by how \
              much: 6 of the 12 dp it needs at Compact, and enough at Touch \
              that the entry does not cover that density at all. What is short \
              is one axis and one side: the chevron sits at the leading edge of \
              its cell, so the ring's leading half falls outside the cell's \
              HStack. WCAG 2.2 SC 2.5.8 *Equivalent*: ArrowLeft / ArrowRight \
              expand and collapse the focused row. The row band that selects it \
              is not offered as the equivalent, because there is no node to \
              measure: a TreeTableView routes row presses from its own \
              rectangle, so the band's height is a row-height question and not \
              this audit's.",
    },
    AllowedViolation {
        path: "> Link",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            themes: ALL_THEMES,
            paints: (ClearsFloor, Is(17.0)),
            reaches: (ClearsFloor, Is(17.0)),
        }],
        owner: Owner::NobodyBecause("the *Inline* exception is the answer, not a deferral"),
        exception: Some("Inline"),
        why: "A link is text-height -- 17 dp, reaching exactly that at every \
              density, in both the `link` fixture and an archived \
              notification's replay action, with the long axis clearing the \
              floor in each. WCAG 2.2 SC 2.5.8 *Inline* -- the target's size is \
              constrained by the line height of the text it is set in -- which \
              is the second of the two legs `link.rs` already rests on. Its \
              FIRST leg is wrong and the sentence is corrected there: link.rs \
              said the miss-only slop pass reaches it, and the slop pass is \
              denied whenever the link sits in a row that takes presses, which \
              is where both of these links are.",
    },
    AllowedViolation {
        path: "window_frame: WindowFrame > ResizeStrip",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            themes: ALL_THEMES,
            paints: (Is(6.0), Is(6.0)),
            reaches: (Is(6.0), Is(6.0)),
        }],
        owner: Owner::Named(
            "whoever decides whether the diagonal grip should be bigger than \
                the edges it sits between",
        ),
        exception: Some("Equivalent"),
        why: "A window's four diagonal corner grips reach exactly their own \
              6 x 6 at every density, while the four EDGE strips on the same \
              path reach the floor through the same `hit_outset` -- pinned \
              together by \
              `a_window_corner_grip_is_boxed_in_by_its_own_edge_strips`. The \
              corner is boxed in by its own neighbours: the arena orders outset \
              candidates by distance to the uninflated rectangle, and a point \
              one dp inboard of a corner is already INSIDE an edge strip, at \
              distance zero. Two mutations say what the corner branch \
              (`EdgeInsets::uniform(out)`) is worth, and it is NOT the P35 \
              dead-declaration shape: deleting it costs the corner its own \
              body, which the adjacent edge then claims, and deleting the EDGE \
              branches instead lets the corners grow. So the declaration is \
              load-bearing for the grip existing at all, and powerless to grow \
              it; that named test reddens under either mutation. WCAG 2.2 \
              SC 2.5.8 *Equivalent*: the same rectangle is reached by dragging \
              the two adjacent edges, each of which conforms, and by the title \
              bar's Resize command.",
    },
    AllowedViolation {
        path: "docking: DockingLayout > SideClipPane",
        measured: &[PinnedGeometry {
            densities: AT_TOUCH,
            themes: ALL_THEMES,
            paints: (ClearsFloor, Is(38.0)),
            reaches: (Is(0.0), Is(0.0)),
        }],
        owner: Owner::Named("whoever owns A10's outset-versus-target precedence"),
        exception: None,
        why: "At Touch a dock's tab strip is UNREACHABLE at the point a user \
              aims at -- a reach of exactly zero on both axes for a strip \
              38 dp tall -- and the cause is another target's ring, not its \
              size. `DockResizeHandle::hit_outset` grows a 6 dp gutter to \
              `TargetRole::Target`, 44 dp at Touch, and the strip below it \
              starts where the gutter ends, so the tab's centre line is inside \
              the ring and every press there resizes the dock instead of \
              switching tabs -- at every dock side, in every app. A10's \
              precedence chain settles an outset against ANOTHER OUTSET by \
              distance to the uninflated rectangle; it says nothing about an \
              outset against a plain target that contains the point, and that \
              is the gap. Escalated rather than papered over: the fixes on \
              offer are all mechanism changes (an outset that loses to a \
              containing target, or a gutter that inflates to `TargetRole::\
              Grab` -- 16 dp at Touch, but 6 dp at Compact, which is the \
              gutter's own thickness and would leave it no ring there, \
              reddening P30's Compact touch tests). Pinned by \
              `the_dock_gutters_touch_ring_swallows_the_tab_strip_beside_it`, \
              which also holds the other half: at Compact the same header is \
              reachable, so this is a finding about the ring and not about the \
              strip.",
    },
    AllowedViolation {
        path: "HeaderRow > HeaderCell",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            themes: ALL_THEMES,
            paints: (Is(4.0), ClearsFloor),
            reaches: (Is(4.0), ClearsFloor),
        }],
        owner: Owner::Named("whoever closes drag-operation-census rows 7 and 8"),
        exception: None,
        why: "A table column's resize grip, reported from \
              `HeaderCell::target_regions` as `HEADER_PART_RESIZE`: 4 dp wide, \
              reaching exactly that at every density, with its height clearing \
              the floor. Two things make it unreachable by any mechanism. A \
              region is *measured*, not probed, so the only growth it can take \
              is what the walker confirmed for its node at an edge it shares -- \
              and its one shared edge is the cell's reading-order trailing one, \
              where the next header cell is flush and takes presses, so the \
              confirmed growth there is zero. And the 4 dp is a HALF-width by \
              construction: the type docs record that a divider is a boundary \
              between two cells and the grabbable band straddles it, \
              `resize_grip` inside each, so the divider a user aims at is two \
              cells' halves. `target_regions` reports only the trailing half, \
              so the audit reads half of the affordance; both are far under the \
              floor and the discrepancy is a reporting gap, not the shortfall. \
              NOT excused under SC 2.5.8 *Equivalent*: \
              docs/drag-operation-census.md row 7 rates this operation \
              *Partial* -- `on_access_action` Increment/Decrement by \
              `COLUMN_RESIZE_STEP` exists, and there is no keyboard route at \
              all, because the header cell is `.focusable(false)` and carries \
              no `on_key`. An AT action is not an equivalent control on the \
              same page. The remediation is already owned and is that row's \
              own: a focusable header row with a roving tab index, which rows 7 \
              and 8 share as a prerequisite.",
    },
    AllowedViolation {
        path: "OverlayTrigger > FilterIndicator",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            themes: ALL_THEMES,
            paints: (Is(12.0), Is(12.0)),
            reaches: (Is(12.0), Is(12.0)),
        }],
        owner: Owner::Named("whoever revisits the table header's filter affordance"),
        exception: None,
        why: "A filterable column's filter glyph, `FILTER_INDICATOR_SIZE`, \
              reaching exactly its paint at every density. The header cell \
              around it takes presses, so the miss-only pass is denied at \
              distance zero, and neither `FilterIndicator` nor the \
              `OverlayTrigger` wrapping it declares a `Widget::hit_outset` -- \
              so the glyph is the whole target. The trigger does carry \
              `on_tap`, an `on_key` Enter/Space arm and \
              `on_access_action(Click)`, but `OverlayTrigger` sets no \
              `focusable`, so the key arm is reachable only if an ancestor \
              makes it a tab stop; this entry does not claim it does, and the \
              header cell it sits in is `.focusable(false)`. No SC 2.5.8 \
              exception is claimed either: there is no conforming equivalent \
              control for opening a column's filter. The remedy is a bigger \
              glyph or a `TouchTarget` around it, both of which move Compact \
              layout, which is the programme's invariant.",
    },
    AllowedViolation {
        path: "KeyboardHighlightWrapper > ZStack > MenuItem",
        measured: &[
            // Int UI: `item_height` 24, nominal line box 18.2, flat measured
            // line 16. These three rows are judged rather than filed as
            // shadowed only because `PROBE_EPSILON` covers an extent: the row
            // reaches 21.75 against its own 21.8 of paint, 1.6 quanta short,
            // and the pre-programme 0.05 dp slack read that as another target
            // sitting on top of a menu row. Nothing about the row moved -- see
            // docs/accessibility-internal-audit.md 3.7, which records it because
            // it is an Int UI **Compact** census change.
            PinnedGeometry {
                densities: AT_COMPACT,
                themes: &[INTUI],
                paints: (ClearsFloor, Is(21.8)),
                reaches: (ClearsFloor, Is(21.8)),
            },
            // macOS: its nominal line box IS the flat measured one, so the row
            // comes out at exactly the `item_height` the preset asked for --
            // and that number is 22, which is the preset's own decision and
            // under the floor at every density.
            PinnedGeometry {
                densities: ALL_DENSITIES,
                themes: &[MACOS],
                paints: (ClearsFloor, Is(22.0)),
                reaches: (ClearsFloor, Is(22.0)),
            },
            // Material 3: the stock recipe under M3 typography, whose nominal
            // line box is the widest of the four.
            PinnedGeometry {
                densities: AT_COMPACT,
                themes: &[M3],
                paints: (ClearsFloor, Is(20.0)),
                reaches: (ClearsFloor, Is(20.0)),
            },
        ],
        owner: Owner::Named(
            "whoever owns `RecipeMenuItemStyle`'s vertical padding, jointly \
                with docs/density-inventory.md \u{a7}0",
        ),
        exception: None,
        why: "A menu row, which is the target itself -- no SC 2.5.8 exception \
              applies, and the row reaches exactly what it paints, at a height \
              under the floor. The finding is that the row does NOT come out \
              at the `item_height` its recipe asks for. `RecipeMenuItemStyle` \
              derives the row's vertical padding from a NOMINAL line box -- \
              `body.size * body.line_height` -- and then lays that padding \
              around the content's MEASURED height, so the rendered row is \
              `item_height` only where the two agree and is short by their \
              difference where they do not. The recipe's own comment claims \
              the padding is derived so the row has the full `item_height`, \
              which is what this measurement contradicts. That is a property \
              of the recipe and not of any one theme: any shaper whose line \
              box differs from the size-times-multiplier product moves the \
              row. The FIGURE, though, is this fixture list's. Nothing here \
              installs a text backend, and both headless measurers -- \
              `TextWidget`'s own fallback and `MockTextBackend` -- report a \
              flat line whatever the theme asks, so what is measured is \
              `item_height` minus the theme's nominal line box plus that flat \
              line. It is also why a preset whose nominal box happens to equal \
              the flat one gets its `item_height` exactly, and is judged only \
              if that constant is itself under the floor. Two decisions, one \
              owner: whether the padding should be derived from the measured \
              content instead -- which raises this row at Compact, and Compact \
              is the programme's invariant -- and whether the fixture list \
              should measure text through something that reads the theme.",
    },
    AllowedViolation {
        path: "window_frame: WindowFrame > VStack > Button",
        measured: &[PinnedGeometry {
            densities: AT_TOUCH,
            themes: &[M3],
            paints: (ClearsFloor, Is(48.0)),
            reaches: (Is(0.0), Is(0.0)),
        }],
        owner: Owner::Named(
            "whoever owns A10's outset-versus-target precedence (the same \
                owner as the docking entry above)",
        ),
        exception: None,
        why: "A window's top resize strip is 6 dp of paint with a coarse \
              `hit_outset` around it, and at Touch that ring reaches down into \
              the window's own content by about the density's `target_size` -- \
              so the top band of a window is claimed by the frame and not by \
              what is painted there. This is the SAME finding as the docking \
              entry above, at a different gutter: an outset takes precedence \
              over a target it overlaps, and the target reaches nothing at all. \
              It is NOT a Material 3 defect. The identical geometry is present \
              under every preset -- Int UI's button in this fixture reaches \
              44.7 x 24.0 of its own 124 x 44, Fluent's 44.8 x 24.0 of 116 x \
              44 -- and the audit files those two as \
              `ShadowedByAnotherTarget` rather than judging them, because a \
              target whose reach falls short of its own paint is limited by \
              another target rather than by its size. Material 3 is the preset \
              where the row crosses over: its 48 dp Touch `target_size` moves \
              both the ring's reach and the button's centre, and the centre \
              ends up inside the ring, which is the point at which the audit \
              stops calling it shadowed and starts calling it unreachable. So \
              the gate reports one preset and the defect belongs to all four. \
              No SC 2.5.8 exception is claimed: a button a finger cannot press \
              has no equivalent.",
    },
    AllowedViolation {
        path: "CalendarHeader > HStack > Expand > Button",
        measured: &[PinnedGeometry {
            densities: AT_TOUCH,
            themes: &[M3],
            paints: (Is(11.2), Is(48.0)),
            reaches: (Is(11.2), Is(48.0)),
        }],
        owner: Owner::Named(
            "whoever owns Calendar's header layout under a 48 dp target \
                ladder",
        ),
        exception: None,
        why: "The month/year button between a calendar's four nav arrows. A \
              calendar sizes to its own grid, so the header's width is fixed \
              before the arrows are placed and every dp the arrows take is a dp \
              the title does not get: at Material 3's 48 dp Touch `target_size` \
              the four of them take 16 dp more than Int UI's, and the title \
              ends at 11.2 dp. This is NOT the fixture's viewport, which is the \
              first thing it looks like -- the same 11.2 dp is measured at 440, \
              480 and 520 dp, because none of that width reaches the calendar. \
              Under Int UI and Fluent the same node is 27.2 dp, clearing the \
              floor by 3.2, which is how close the other three presets are to \
              the same failure. Neither hit mechanism helps: the arrows beside \
              it are targets of their own kind, so the miss-only pass is denied \
              at distance zero. The remedies are all header-layout decisions -- \
              a minimum width on the title that pushes the arrows out, a \
              two-row header, or arrows that do not follow the target ladder -- \
              and each of them changes what a calendar looks like.",
    },
];

#[test]
fn zz_census() {
    let mut totals: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    for v in census() {
        let subject = v.path.split(": ").next().unwrap_or("?").to_string();
        let leaf = v.path.rsplit(" > ").next().unwrap_or("?").to_string();
        println!(
            "CENSUS {} {:?} {subject} {leaf} paints {:.1}x{:.1} reaches {:.1}x{:.1} src={}{}",
            v.theme,
            v.density,
            v.size.width,
            v.size.height,
            v.expanded.width,
            v.expanded.height,
            if v.sources.outset { "outset " } else { "" },
            if v.sources.slop { "slop" } else { "" },
        );
        *totals
            .entry((v.theme.to_string(), format!("{:?}", v.density)))
            .or_default() += 1;
    }
    for ((theme, density), n) in totals {
        println!("CENSUS-TOTAL {theme} {density} {n}");
    }
}

/// **The gate.** Zero conformance failures, at all three densities and under
/// every preset in the roster, except the written entries above.
#[test]
fn no_target_falls_below_the_conformance_floor_at_any_density() {
    let failures = gate::conformance_failures(census(), &ROSTER);
    assert!(
        failures.is_empty(),
        "{} target(s) below the 24 dp WCAG 2.2 SC 2.5.8 (AA) floor:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// Every allow-list entry, every geometry it pins, and every **(theme, density)**
/// pair it claims still matches a real failure.
///
/// Holding a pin to every pair of the product rather than to every density is
/// strictly stronger, and it is the check that would have caught Fluent's
/// selection pill the day it landed: an entry pinning a chevron's 16 dp reach
/// says nothing about a theme where the chevron reaches nothing at all.
///
/// The failure mode of an allow-list is not that it is long, it is that it
/// outlives what it excused: an entry matching nothing is a hole waiting for the
/// next control whose name happens to contain the same substring.
#[test]
fn no_allow_list_entry_is_stale() {
    let findings = gate::stale_entries(census(), &ROSTER);
    assert!(findings.is_empty(), "{}", findings.join("\n"));
}

/// The roster's own properties: structural, and no longer theme-blind.
///
/// It used to read only the const — no tree was built — so it would have stayed
/// green while every preset gate was red. It now asks, through the shared
/// matcher, whether each preset the gate measures is either reached by an entry
/// or proven clean by the census, whether every pin names a theme, whether any
/// names `custom` (the id every app theme shares), whether an entry's claimed
/// SC 2.5.8 exception is named in its justification, and whether any entry's
/// `path` crosses a type one preset owns — which would go silently stale the day
/// that preset restacked its chrome.
///
/// The counts stay here, because they are this list's and not the matcher's.
/// They are asserted rather than written in prose for the reason the paragraph
/// over [`ALLOW_LIST`] used to prove: it carried four counts and not one held.
#[test]
fn the_allow_lists_roster_is_what_it_says_it_is() {
    let findings = gate::roster_defects(census(), &ROSTER);
    assert!(findings.is_empty(), "{}", findings.join("\n"));

    assert_eq!(ALLOW_LIST.len(), 14, "fourteen written findings");
    let on_an_exception = ALLOW_LIST.iter().filter(|e| e.exception.is_some()).count();
    assert_eq!(
        on_an_exception,
        5,
        "five rest on an SC 2.5.8 exception: {:?}",
        ALLOW_LIST
            .iter()
            .filter(|e| e.exception.is_some())
            .map(|e| e.path)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        ALLOW_LIST.len() - on_an_exception,
        9,
        "and nine are real failures escalated to an owner",
    );
    assert_eq!(
        ALLOW_LIST
            .iter()
            .filter(|e| matches!(e.owner, Owner::NobodyBecause(_)))
            .count(),
        1,
        "exactly one entry needs no owner — the *Inline* exception is the \
         answer there, not a deferral",
    );
}

/// **The roster's four ids stand for eight.** Light and dark are the same
/// geometry in every shipped preset.
///
/// This is a **guard**, not a discriminator: it exists to turn the assumption
/// that halves the sweep into an assertion. If it ever reddens, the answer is
/// not to widen a tolerance — it is that the gate must run all eight ids, and
/// every pin in the list has to be re-read against the appearance it was
/// measured under.
///
/// The comparison drops the theme column and compares the whole multiset of
/// failures, not the totals: two appearances can agree on a count and disagree
/// on which rows they are.
#[test]
fn light_and_dark_are_the_same_geometry_in_every_preset() {
    static DARK: gate::Roster = gate::Roster {
        themes: &[
            gate::ThemeSubject {
                id: "intui.dark",
                theme: intui::dark,
            },
            gate::ThemeSubject {
                id: "macos.dark",
                theme: teksilo_theme_macos::dark,
            },
            gate::ThemeSubject {
                id: "fluent.dark",
                theme: teksilo_theme_fluent::dark,
            },
            gate::ThemeSubject {
                id: "material3.dark",
                theme: teksilo_theme_material3::dark,
            },
        ],
        allow: &[],
    };
    /// The failure, with the theme column dropped and the family kept.
    fn keys(violations: &[TargetViolation]) -> Vec<String> {
        let mut out: Vec<String> = violations
            .iter()
            .map(|v| {
                format!(
                    "{} {:?} {:?} {} {:.4}x{:.4} -> {:.4}x{:.4}",
                    v.theme.as_str().split('.').next().unwrap_or("?"),
                    v.density,
                    v.rule,
                    v.path,
                    v.size.width,
                    v.size.height,
                    v.expanded.width,
                    v.expanded.height,
                )
            })
            .collect();
        out.sort();
        out
    }
    let light = keys(census());
    let dark = keys(&gate::conformance_census(&widget_fixtures(), &DARK));
    // Both are sorted, so `==` IS the multiset comparison: it holds every key
    // AND how many times each occurs. A set difference plus a length check is
    // not the same question — it accepts light `{A, A, B}` against dark
    // `{A, B, B}`, which is two appearances disagreeing on which rows they are.
    if light != dark {
        let only_light: Vec<&String> = light.iter().filter(|k| !dark.contains(k)).collect();
        let only_dark: Vec<&String> = dark.iter().filter(|k| !light.contains(k)).collect();
        panic!(
            "an appearance changed a geometry, so one representative per family is no longer \
             enough: {} light rows against {} dark\n  light only: {only_light:#?}\n  dark only: \
             {only_dark:#?}\n(both lists empty means the two agree on every key and differ on how \
             many times one occurs)",
            light.len(),
            dark.len(),
        );
    }
}

/// An allow-list `path` may not span a type one preset owns.
///
/// Such an entry does not excuse too much — it excuses too little, and does it
/// **silently**: the day the preset restacks its chrome the path stops matching,
/// the entry covers nothing, and the only thing that notices is the staleness
/// test, which reports it as a geometry that moved rather than as a path that
/// named the wrong thing. The theme belongs on the pin, where the roster can
/// check it.
///
/// Asserted against a synthetic list, because the real one must not contain the
/// defect this describes.
#[test]
fn no_entry_names_a_theme_owned_wrapper_in_its_path() {
    static BAD: gate::Roster = gate::Roster {
        themes: &[gate::ThemeSubject {
            id: INTUI,
            theme: intui::light,
        }],
        allow: &[AllowedViolation {
            path: "StandardListItem > FluentRowFrame > ZStack",
            measured: &[PinnedGeometry {
                densities: AT_COMPACT,
                themes: &[INTUI],
                paints: (Is(16.0), Is(16.0)),
                reaches: (Is(16.0), Is(16.0)),
            }],
            owner: Owner::Named("nobody, this entry is a fixture"),
            exception: None,
            why: "a synthetic entry, to hold the lint",
        }],
    };
    let findings = gate::roster_defects(&[], &BAD);
    assert!(
        findings.iter().any(|f| f.contains("FluentRowFrame")),
        "the lint must name the offending segment; it reported {findings:#?}",
    );
}

/// **The allow-list narrows**, on the size axis and on the theme axis.
///
/// This is the half a staleness test cannot reach. Staleness asks whether each
/// entry still matches something; this asks the opposite question — whether an
/// entry matches something it should not.
///
/// Each of a real violation's four scalars is driven to 2 dp, and its theme is
/// re-stamped with every other preset in the roster. The size seeds prove the
/// size discriminator narrows; only the theme seed proves the theme
/// discriminator narrows rather than decorates, and it is checked against the
/// **census** — a geometry may be excused under a preset only where that preset
/// really produces it — so the assertion is not a restatement of the matcher.
#[test]
fn the_allow_list_excuses_only_the_geometry_it_pinned() {
    // Per preset: four size seeds and three theme seeds for each covered
    // violation — so seven times that preset's own census, and the figure below
    // is the smallest of the four **exactly**. Exactly, not comfortably under:
    // this number is the only thing standing between "every geometry in the list
    // was seeded against" and "the census quietly collapsed and the seeding
    // proved nothing", and a slack figure buys a silent collapse the room to
    // hide in. It reddens when the census shrinks — re-measure with `zz_census`
    // (the CENSUS-TOTAL lines), take the smallest preset's total, and multiply
    // by seven.
    let findings = gate::non_narrowing(census(), &ROSTER, 7 * 116);
    assert!(findings.is_empty(), "{}", findings.join("\n"));

    // And the seed floor above is a floor: it cannot see a census that GREW,
    // which is exactly how a newly-appearing geometry an existing pin happens
    // to cover slips past all three questions — a second 18 x 13 `StepButton`
    // anywhere in the tree would be excused with no written entry naming its
    // site. The count is the other half, exactly as the charts and scene gates
    // hold it; re-derive with `zz_census` (sum the CENSUS-TOTAL lines) when a
    // fixture or a preset legitimately moves it, and re-read any entry whose
    // region the new rows fall into.
    assert_eq!(
        census().len(),
        481,
        "the (theme x density) census moved — this test is proving something \
         else until the figure, the per-preset seed floor above, and the \
         entries covering any new rows are re-derived together",
    );
}

/// The type of the node a measurement is about — the last segment of its path.
fn measured_leaf(path: &str) -> &str {
    let tail = path.rsplit(" > ").next().unwrap_or(path);
    tail.rsplit(": ").next().unwrap_or(tail)
}

/// The list is not a stub: every fixture is audited, and every fixture that
/// claims a subject is seen to measure it.
#[test]
fn every_fixture_measures_the_subject_it_names() {
    let fixtures = widget_fixtures();
    assert!(
        fixtures.len() >= 40,
        "the fixture list is the coverage claim; it holds {}",
        fixtures.len(),
    );
    let mut problems = Vec::new();
    // The reverse direction first: a table row whose fixture was renamed or
    // deleted is a claim about nothing, silently — the `.find` below never
    // visits it — and a duplicated name shadows its second row the same way
    // (`.find` reads the first).
    for (index, (name, _)) in EXPECTED_SUBJECTS.iter().enumerate() {
        if !fixtures.iter().any(|f| f.name == *name) {
            problems.push(format!(
                "EXPECTED_SUBJECTS row `{name}` names no fixture — the claim it holds is \
                 about nothing; delete it, or rename it with the fixture",
            ));
        }
        if EXPECTED_SUBJECTS[..index].iter().any(|(n, _)| n == name) {
            problems.push(format!(
                "EXPECTED_SUBJECTS names `{name}` twice — the first row shadows the \
                 second, which is dead",
            ));
        }
    }
    // And the fixture list itself: the expectation lookup is first-match, and a
    // fixture-prefixed path (`"window_frame: …"`) merges twins' rows — so a
    // duplicated fixture name lets a broken copy hide behind a healthy one.
    for (index, fixture) in fixtures.iter().enumerate() {
        if fixtures[..index].iter().any(|f| f.name == fixture.name) {
            problems.push(format!(
                "`widget_fixtures()` names `{}` twice — the expectation and \
                 allow-list matchers cannot tell the twins apart",
                fixture.name,
            ));
        }
    }
    for (name, _) in EXPECTED_NON_TARGETS {
        if !fixtures.iter().any(|f| f.name == *name) {
            problems.push(format!(
                "EXPECTED_NON_TARGETS row `{name}` names no fixture",
            ));
        }
    }
    for subject in ROSTER.themes {
        let under_theme: Vec<TargetFixture> = fixtures
            .iter()
            .map(|f| (*f).with_theme(subject.theme))
            .collect();
        let measured = measure_fixtures(&under_theme, TargetDensity::Compact);
        for fixture in &fixtures {
            let Some((_, expected)) = EXPECTED_SUBJECTS
                .iter()
                .find(|(name, _)| *name == fixture.name)
            else {
                if subject.id == INTUI {
                    problems.push(format!(
                        "fixture `{}` has no entry in EXPECTED_SUBJECTS — say what it must \
                         measure, or say that it is not a target",
                        fixture.name,
                    ));
                }
                continue;
            };
            let prefix = format!("{}: ", fixture.name);
            let leaves: Vec<&str> = measured
                .iter()
                .filter(|m| m.path.starts_with(&prefix))
                .map(|m| measured_leaf(&m.path))
                .collect();
            for widget in *expected {
                if !leaves.contains(widget) {
                    problems.push(format!(
                        "under {}, fixture `{}` measured no `{widget}` node; it measured: {}",
                        subject.id,
                        fixture.name,
                        if leaves.is_empty() {
                            "nothing".to_string()
                        } else {
                            leaves.join(", ")
                        },
                    ));
                }
            }
            if expected.is_empty() {
                // The `&[]` row claims the subject is deliberately NOT a
                // pointer target — the row wrapper's own target still shows up
                // here, so "nothing measured" cannot be the assertion. The
                // claim is held by name: the subject's type must not appear as
                // a measured leaf under any preset, which is what reddens the
                // day a Badge gains a press handler instead of it slipping
                // into the census.
                match EXPECTED_NON_TARGETS
                    .iter()
                    .find(|(name, _)| *name == fixture.name)
                {
                    Some((_, subject_type)) => {
                        if leaves.contains(subject_type) {
                            problems.push(format!(
                                "under {}, fixture `{}`'s subject `{subject_type}` IS a \
                                 measured target, and its EXPECTED_SUBJECTS row says it \
                                 deliberately is not — if it became one on purpose, give \
                                 it a real expectation",
                                subject.id, fixture.name,
                            ));
                        }
                    }
                    None => {
                        if subject.id == INTUI {
                            problems.push(format!(
                                "fixture `{}` claims no target of its own but has no \
                                 EXPECTED_NON_TARGETS row naming its subject's type — \
                                 that row is what holds the claim",
                                fixture.name,
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A deliberately undersized fixture **must** fail, and it must fail for the
/// right reason: the row around it owns every near miss, so no mechanism can
/// rescue a 10 dp control.
///
/// **The harness's liveness proof, and it has two halves** — because the two can
/// fail separately and only one of them used to be held. The first loop calls
/// [`audit_fixtures`] and asserts on the rule, which proves the *walker* still
/// measures. The second runs the same fixture through the two functions the gate
/// itself is made of — [`gate::conformance_census`] and
/// [`gate::conformance_failures`], against the real [`ROSTER`] — which proves
/// the *gate* still reports. Measured: with `conformance_failures` replaced by
/// an empty `Vec`, the first half passes, and so does every other test in this
/// binary; only the second half reddens. A gate that reports zero because it
/// asks nothing looks exactly like a framework that conforms.
#[test]
fn a_deliberately_undersized_fixture_fails() {
    let undersized = vec![TargetFixture::new("undersized", |t| {
        use teksilo_core::widget_builder::WidgetBuilder;
        t.add(
            Padding::uniform(20.0)
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(TextWidget::new(lit!("Row label")))
                        .child(
                            teksilo_widgets::primitives::FixedSize::new()
                                .width(10.0)
                                .height(10.0)
                                .child(RectWidget::new().background(Color::from_hex("#FF0000")))
                                .on_tap(|_, _| {}),
                        ),
                )
                .on_tap(|_, _| {}),
        )
    })];
    for &density in ALL_DENSITIES {
        let violations = audit_fixtures(&undersized, density);
        assert!(
            violations
                .iter()
                .any(|v| v.rule == TargetRule::MinTargetConformance),
            "a 10 dp control inside a tappable row must fail at {density:?}: {violations:#?}",
        );
    }

    // The second half. The same fixture, through the gate's own path: the
    // census it computes over all four presets, and the matcher it hands that
    // census to. Nothing in the real allow-list names this fixture, so every
    // row of it must come back out.
    let reported = gate::reported_failures(&undersized, &ROSTER);
    for subject in ROSTER.themes {
        assert!(
            reported
                .iter()
                .any(|f| f.contains("undersized") && f.contains(subject.id)),
            "the gate reports no failure for the undersized fixture under {}: \
             {reported:#?}",
            subject.id,
        );
    }
}

/// An app-installed Tier-3 style slot is **reported**, and the control it owns
/// is measured at the size its author froze it at — so a control that could not
/// follow the density ladder is attributed to the style that owns its metrics
/// rather than looking like a framework defect.
///
/// Built through a [`TargetFixture`] at each density rather than by switching
/// one tree with `set_input_density`, for the reason `audit_fixtures` states:
/// the switch marks the arena's roots for rebuild and a layout primitive at the
/// root re-attaches child ids the rebuild has already destroyed. Measured on the
/// shape this test used to build — `Padding > HStack > [TextWidget, Button]`
/// under `audit_at_density` — the tree went from **6 nodes to 2**, the custom
/// button was gone, and the single violation left was the 800 x 600 `Padding`
/// root: the assertion passed while measuring nothing it named.
///
/// Three legs, because the reporting alone would not show the consequence:
///
/// 1. [`unprojected_style_slots`] names the slot, and names nothing for the
///    stock preset;
/// 2. the custom button paints 12 x 12 at **every** density — `with_density`
///    preserves an app slot verbatim, which is the documented behaviour — and
///    is an SC 2.5.8 failure at each one;
/// 3. the **stock** button in the same fixture shape clears the floor at every
///    density, which is what makes leg 2 the slot's doing and not the fixture's.
#[test]
fn an_app_installed_style_slot_is_reported() {
    assert_eq!(
        unprojected_style_slots(&tiny_button_theme()),
        vec!["button"]
    );
    assert!(
        !unprojected_style_slots(&intui::light()).contains(&"button"),
        "the stock preset installs no slot to report",
    );

    // And the same under every preset in the roster, which is the leg that used
    // to be Int UI's alone: the function early-returned empty for any theme
    // carrying a `DensityProjection`, and all three sibling presets carry one,
    // so an app style installed on top of macOS, Fluent or Material 3 was
    // reported as following a ladder it does not follow. `grid_view` is the
    // probe because no shipped preset installs it — a slot a preset DOES own is
    // overwritten by that preset's own projection at the next density change,
    // so it is genuinely not frozen and is correctly not named.
    for subject in ROSTER.themes {
        let bare = (subject.theme)();
        assert!(
            unprojected_style_slots(&bare).is_empty(),
            "{} rebuilds every slot it installs, so it has none to report: {:?}",
            subject.id,
            unprojected_style_slots(&bare),
        );
        let mut with_app_slot = bare.clone();
        with_app_slot.style_slots.grid_view =
            Some(Rc::new(teksilo_widgets::styles::RecipeGridViewStyle));
        assert_eq!(
            unprojected_style_slots(&with_app_slot),
            vec!["grid_view"],
            "an app slot on top of {} is frozen at every density and must be \
             attributed",
            subject.id,
        );
    }

    let custom = [TargetFixture::new("app_slot", |t| {
        in_tappable_row(t, Button::new(lit!("Save")))
    })
    .with_theme(tiny_button_theme)];
    let stock = [TargetFixture::new("stock", |t| {
        in_tappable_row(t, Button::new(lit!("Save")))
    })];
    for &density in ALL_DENSITIES {
        let measured = measure_fixtures(&custom, density);
        let button = measured
            .iter()
            .find(|m| measured_leaf(&m.path) == "Button")
            .unwrap_or_else(|| {
                panic!("the custom-styled button is measured at {density:?}: {measured:#?}")
            });
        assert_dp(
            button.size,
            (12.0, 12.0),
            "a hand-written style's body is frozen at its own metrics",
        );
        assert_dp(
            button.expanded,
            (12.0, 12.0),
            "reaching exactly its own paint — no mechanism serves it, and a \
             reach of zero would mean the fixture is measuring an occlusion \
             rather than the slot",
        );
        assert_eq!(
            button.rule,
            Some(TargetRule::MinTargetConformance),
            "and 12 dp inside a tappable row fails the floor at {density:?}: {button:#?}",
        );

        let measured = measure_fixtures(&stock, density);
        let button = measured
            .iter()
            .find(|m| measured_leaf(&m.path) == "Button")
            .unwrap_or_else(|| panic!("the stock button is measured at {density:?}"));
        assert_dp(
            button.expanded,
            (
                InputTokens::for_density(density).target_size + 1.0,
                InputTokens::for_density(density).target_size,
            ),
            "the stock style follows the ladder, so the same shape conforms \
             (the width is the probe budget spent, not a boundary)",
        );
        assert_eq!(
            button.rule, None,
            "and carries no verdict at {density:?}: {button:#?}",
        );
    }
}

/// The IntUI preset with one **app-installed** `ButtonStyle` — a hand-written
/// style whose metrics no density projection will re-derive.
fn tiny_button_theme() -> teksilo_core::styles::Theme {
    let mut theme = intui::light();
    theme.style_slots.button = Some(Rc::new(TinyButtonStyle));
    theme
}

/// A 12 dp button body, standing in for any app-installed style whose author
/// wrote its dimensions by hand.
///
/// `cfg.label` is attached rather than dropped. A style that drops it leaves the
/// pre-built label subtree parented to nothing, which makes it a second arena
/// **root** — laid out at the whole viewport and tested before the real root, so
/// it swallows every press in the tree and every target measures a reach of
/// zero. That is a bug in the style, and a fixture built on one measures the
/// bug rather than the slot.
#[derive(Debug)]
struct TinyButtonStyle;

impl teksilo_core::styles::ButtonStyle for TinyButtonStyle {
    fn make_body(
        &self,
        cfg: &teksilo_core::styles::ButtonStyleConfig,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> WidgetId {
        let label = cfg.label;
        ctx.add(
            teksilo_widgets::primitives::FixedSize::new()
                .width(12.0)
                .height(12.0)
                .child(
                    teksilo_widgets::primitives::ZStack::new()
                        .child(RectWidget::new().background(Color::from_hex("#888888")))
                        .child(label),
                ),
        )
    }
}

/// The two mechanisms the shipped controls declare are actually reached, and the
/// audit can tell them apart.
///
/// * A `ScrollBar` reports its thumb from `Widget::target_regions`, so the audit
///   sees a target the node's own rectangle hides.
/// * A `Splitter`'s gutter takes its reach from `Widget::hit_outset` inside the
///   exact pass, not from the slop pass — the panes on either side own every
///   near miss at distance zero.
#[test]
fn the_declared_mechanisms_are_reached_and_attributed() {
    let fixtures = widget_fixtures();
    let measured = measure_fixtures(&fixtures, TargetDensity::Compact);

    let regions: Vec<_> = measured.iter().filter(|m| m.part.is_some()).collect();
    assert!(
        !regions.is_empty(),
        "no widget in the list reported a target region; the `target_regions` leg \
         of the walk is measuring nothing",
    );

    let outset_grown: Vec<_> = measured.iter().filter(|m| m.sources.outset).collect();
    assert!(
        !outset_grown.is_empty(),
        "no widget in the list took its reach from `hit_outset`; the exact-pass \
         leg of the walk is measuring nothing",
    );
}

// =========================================================================
// The findings, pinned
// =========================================================================
//
// Each of the three tests below asserts the measurement one `ALLOW_LIST` entry
// rests on. An excuse written as prose is an excuse nobody re-derives; written
// as an equality it reddens the day the geometry changes, and the entry has to
// be re-read then rather than outliving what it excused.

/// Every measurement one fixture produces for one leaf type, at one density.
fn measurements_of(
    fixture: &str,
    leaf: &str,
    density: TargetDensity,
) -> Vec<teksilo_core::accessibility::target_audit::TargetMeasurement> {
    let one: Vec<TargetFixture> = widget_fixtures()
        .into_iter()
        .filter(|f| f.name == fixture)
        .collect();
    assert_eq!(
        one.len(),
        1,
        "fixture `{fixture}` is not in the list exactly once",
    );
    measure_fixtures(&one, density)
        .into_iter()
        .filter(|m| measured_leaf(&m.path) == leaf)
        .collect()
}

/// The probe finds each boundary by bisecting inside one `PROBE_STEP`, and a
/// reach is an **extent** — two directions' boundaries added — so it can land
/// up to two refinement quanta (0.0625 dp) short of itself; the diff's own
/// Fluent Touch chevrons do. The tolerance is therefore [`PIN_TOLERANCE`], the
/// same three-quantum slack every pinned figure carries (the pre-programme
/// 0.05 sat *inside* the probe's error band and flaked the first time a
/// boundary bisected two quanta short). A *paint* is compared the same way for
/// symmetry; both are equalities, not bounds — a bound would be satisfied by a
/// target sized by something other than itself.
#[track_caller]
fn assert_dp(actual: Size, expected: (f32, f32), what: &str) {
    assert!(
        (actual.width - expected.0).abs() <= PIN_TOLERANCE
            && (actual.height - expected.1).abs() <= PIN_TOLERANCE,
        "{what}: measured {:.4} x {:.4}, expected {:.2} x {:.2}",
        actual.width,
        actual.height,
        expected.0,
        expected.1,
    );
}

/// `TwistArrow::hit_outset` is inert inside `StandardTreeItem`, and the same
/// chevron in a bare row reaches the floor.
///
/// The hook is consulted only among a node's direct children and the recursion
/// returns before it looks at a child whose own parent excludes the point, so a
/// wrapper the size of its child offers the outset nothing.
/// `StandardTreeItem::build` wraps the chevron in
/// `FixedSize::new().width(chevron_size)`, which is exactly that wrapper.
///
/// Reddens the day the wrapper goes: the chevron's reach in a TreeView then
/// stops equalling its paint, and the `HStack > FixedSize > TwistArrow`
/// allow-list entry stops matching with it.
#[test]
fn the_chevrons_outset_is_inert_inside_a_standard_tree_row() {
    for &density in ALL_DENSITIES {
        let in_a_tree = measurements_of("tree_view", "TwistArrow", density);
        assert!(
            !in_a_tree.is_empty(),
            "no chevron measured in a TreeView at {density:?} — the fixture \
             stopped building one, which is a blind gate, not a pass",
        );
        for m in &in_a_tree {
            assert_dp(m.size, (16.0, 16.0), "the tree chevron's paint moved");
            assert_dp(
                m.expanded,
                (16.0, 16.0),
                &format!(
                    "at {density:?} the chevron inside a StandardTreeItem \
                     reached past its own box, so a mechanism now delivers here \
                     and the `HStack > FixedSize > TwistArrow` allow-list entry \
                     is out of date"
                ),
            );
            assert!(
                !m.sources.any(),
                "and no mechanism may be credited: {:?}",
                m.sources,
            );
        }
    }

    // The discriminator: the same chevron, one wrapper fewer, clears the floor
    // at Compact through its outset. Without this half the test above would pass
    // just as well if the outset had never worked anywhere.
    let bare = measurements_of("twist_arrow_in_a_row", "TwistArrow", TargetDensity::Compact);
    assert_eq!(bare.len(), 1);
    assert_dp(bare[0].size, (12.0, 12.0), "the bare chevron's paint");
    assert_dp(
        bare[0].expanded,
        (24.0, 24.0),
        "the chevron's outset does not reach the floor even unhugged, so this \
         file is measuring a broken mechanism rather than a hugged one",
    );
    assert!(
        bare[0].sources.outset,
        "and it must be the outset that delivers it, not the slop pass",
    );
}

/// A window's diagonal corner grip is boxed in by its own edge strips, while
/// those edges reach the floor through the same `hit_outset`.
///
/// Outset candidates are ordered by distance to the uninflated rectangle, and a
/// point one dp inboard of a corner is already *inside* an edge strip, at
/// distance zero — so the corner branch cannot grow the grip. It is still
/// load-bearing, which is the part reading cannot tell you: deleting it drops the
/// corner to **0 x 0**, because the corner's own body is claimed by the adjacent
/// edge and declaring an outset is what puts it in the same candidate list to win
/// it back. Deleting the edge branches instead lets the corners reach 24 x 24.
/// The equality below reddens under either mutation.
#[test]
fn a_window_corner_grip_is_boxed_in_by_its_own_edge_strips() {
    let strips = measurements_of("window_frame", "ResizeStrip", TargetDensity::Compact);
    assert_eq!(strips.len(), 8, "a frame has four edges and four corners");

    let corners: Vec<_> = strips
        .iter()
        .filter(|m| m.size.width == 6.0 && m.size.height == 6.0)
        .collect();
    assert_eq!(corners.len(), 4, "four corners");
    for m in &corners {
        assert_dp(
            m.expanded,
            (6.0, 6.0),
            "a corner grip grew, so its outset now claims something and the \
             allow-list entry is out of date",
        );
    }

    let edges: Vec<_> = strips
        .iter()
        .filter(|m| m.size.width > 6.0 || m.size.height > 6.0)
        .collect();
    assert_eq!(edges.len(), 4, "four edges");
    for m in &edges {
        // The thin axis is the one the outset grows; the long axis spends the
        // probe budget and is reported as the cap.
        let thin_reach = if m.size.height == 6.0 {
            m.expanded.height
        } else {
            m.expanded.width
        };
        assert!(
            (thin_reach - 24.0).abs() <= PIN_TOLERANCE,
            "an edge strip must reach the Compact floor across its thickness — \
             it is the same `hit_outset` the corner cannot use — measured \
             {thin_reach:.4}",
        );
        assert!(m.sources.outset, "and it is the outset that gets it there");
    }
}

/// At Touch a dock's tab strip is unreachable at the point a user aims at,
/// because the resize gutter above it inflates to `TargetRole::Target` (44 dp)
/// and its ring reaches past the strip's centre line.
///
/// Reach 0 for a header that paints 95.8 x 38 is not a size failure; it is an
/// outset-versus-target precedence gap, and the number is what makes that
/// legible.
#[test]
fn the_dock_gutters_touch_ring_swallows_the_tab_strip_beside_it() {
    let gutters = measurements_of("docking", "DockResizeHandle", TargetDensity::Touch);
    assert_eq!(gutters.len(), 2, "a leading side and a bottom side");
    let horizontal = gutters
        .iter()
        .find(|m| m.size.height == 6.0)
        .expect("the bottom side's gutter is 6 dp tall");
    assert!(
        (horizontal.expanded.height - 44.0).abs() <= PIN_TOLERANCE,
        "the gutter's Touch ring is what covers the strip; if it shrank, this \
         finding is stale — measured {:.4}",
        horizontal.expanded.height,
    );

    let bars = measurements_of("docking", "TabBar", TargetDensity::Touch);
    assert_eq!(bars.len(), 1);
    assert_dp(
        bars[0].size,
        (900.0, 38.0),
        "the strip is the full-width bottom dock's",
    );

    // The header is the thing a user aims at, and it is what goes dark.
    let touch = measurements_of("docking", "TabHeader", TargetDensity::Touch);
    assert_eq!(touch.len(), 1);
    assert_dp(
        touch[0].expanded,
        (0.0, 0.0),
        "the dock tab header is reachable again at Touch — the \
         `docking: DockingLayout > SideClipPane` allow-list entry is out of date",
    );
    assert_eq!(touch[0].rule, Some(TargetRule::MinTargetConformance));

    // And at Compact the same header is reachable, which is what makes this a
    // finding about the ring's size rather than about the strip. Without this
    // half the test would pass on a header that was never reachable at all.
    let compact = measurements_of("docking", "TabHeader", TargetDensity::Compact);
    assert_eq!(compact.len(), 1);
    assert_eq!(
        compact[0].rule, None,
        "at Compact the ring is 24 dp and stops short of the header's centre",
    );
    assert!(compact[0].expanded.width > 0.0 && compact[0].expanded.height > 0.0);
}

/// The tab close button exists as a target only where a finger can find it.
///
/// It is gated behind hover below `RevealPolicy::Always`, and a finger produces
/// no hover — so at Compact the fixture measures no close button at all, and at
/// Touch it measures one at the full 44 dp. Both halves matter: the first says
/// the audit is not silently missing a control, the second says the reveal policy
/// actually lands.
#[test]
fn a_hover_revealed_close_button_is_a_target_only_at_touch_density() {
    let at_compact = measurements_of("tab_widget/closable", "IconButton", TargetDensity::Compact);
    assert!(
        at_compact.is_empty(),
        "a hover-revealed close button must not exist unhovered at Compact: \
         {at_compact:#?}",
    );
    let at_touch = measurements_of("tab_widget/closable", "IconButton", TargetDensity::Touch);
    assert!(
        !at_touch.is_empty(),
        "at Touch `RevealPolicy::Always` installs no hover gate, so every tab's \
         close button must be a target",
    );
    for m in &at_touch {
        assert_dp(
            m.size,
            (44.0, 44.0),
            "the close button follows the density ladder",
        );
        assert_dp(m.expanded, (44.0, 44.0), "and it reaches its own paint");
    }
}

/// The expectation check reads the **measured node**, not a name that appears
/// somewhere above it.
///
/// The hole this closes was live: `DropZone` and `DropTarget` appear in the path
/// of every row their subtree produces, so `path.contains("DropZone")` was
/// satisfied by the `Button` inside while the wrapper itself is deliberately not
/// a press target at all. This asserts both halves against real measurements —
/// the name is there, and it is not a leaf — so a regression to `contains` fails
/// here rather than quietly widening every entry in the table.
#[test]
fn the_expectation_check_reads_the_measured_node_not_its_ancestors() {
    assert_eq!(measured_leaf("f: A > B > C"), "C");
    assert_eq!(measured_leaf("f: Alone"), "Alone");

    let one: Vec<TargetFixture> = widget_fixtures()
        .into_iter()
        .filter(|f| f.name == "drop_zone")
        .collect();
    let measured = measure_fixtures(&one, TargetDensity::Compact);
    assert!(
        measured.iter().any(|m| m.path.contains("DropZone")),
        "the subject must be in the path of what it contains",
    );
    assert!(
        !measured
            .iter()
            .any(|m| measured_leaf(&m.path) == "DropZone"),
        "a `DropZone` installs `on_drag_hover` / `on_drop` and nothing else, and \
         a drop is not a press — so it must never be a measured node, and an \
         expectation naming it would be satisfied by its child",
    );
}

/// The two discriminators the census cannot exercise, because the geometries
/// they refuse do not occur today.
///
/// [`the_allow_list_excuses_only_the_geometry_it_pinned`] seeds regressions into
/// violations that exist; these two are about violations that do not. The
/// `window_frame` fixture builds eight `ResizeStrip`s on one identical path —
/// four conforming edges and four boxed-in corners — so an entry keyed on the
/// path alone would excuse an edge the day it regressed, and nothing in the
/// census would notice because the edges are not violations. Same for a finding
/// that exists at one density: without a per-pin density list the entry would
/// cover the other two, where the control is reachable.
#[test]
fn the_allow_lists_discriminators_narrow_by_size_and_by_density() {
    let violation =
        |path: &str, paints: (f32, f32), reaches: (f32, f32), density: TargetDensity| {
            TargetViolation {
                widget: "ResizeStrip",
                node: WidgetId::default(),
                part: None,
                path: path.to_string(),
                density,
                theme: teksilo_core::styles::ThemeId::new(INTUI),
                // Int UI carries the generic ladder at every density — the
                // subject of `an_int_ui_trees_ladder_is_the_generic_table` in
                // teksilo-core — so this is the floor the walker would stamp.
                conformance_floor: InputTokens::for_density(density).min_target_conformance,
                size: Size::new(paints.0, paints.1),
                expanded: Size::new(reaches.0, reaches.1),
                sources: Default::default(),
                transformed: false,
                rule: TargetRule::MinTargetConformance,
            }
        };
    let corner_entry = ALLOW_LIST
        .iter()
        .find(|e| e.path == "window_frame: WindowFrame > ResizeStrip")
        .expect("the corner-grip finding");
    let strip = "window_frame: WindowFrame > ResizeStrip";
    assert!(
        corner_entry.matches(&violation(
            strip,
            (6.0, 6.0),
            (6.0, 6.0),
            TargetDensity::Compact
        )),
        "the 6 x 6 corner reaching its own paint is the finding",
    );
    assert!(
        !corner_entry.matches(&violation(
            strip,
            (600.0, 6.0),
            (600.0, 12.0),
            TargetDensity::Compact
        )),
        "a 600 x 6 EDGE strip on the same path must not be excused — it reaches \
         the floor across its thickness today, and the day it stops the gate has \
         to say so",
    );

    let dock_entry = ALLOW_LIST
        .iter()
        .find(|e| e.path == "docking: DockingLayout > SideClipPane")
        .expect("the dock tab-strip finding");
    let dock = "docking: DockingLayout > SideClipPane > DockSidePanel";
    assert!(
        dock_entry.matches(&violation(
            dock,
            (900.0, 38.0),
            (0.0, 0.0),
            TargetDensity::Touch
        )),
        "the finding is at Touch",
    );
    assert!(
        !dock_entry.matches(&violation(
            dock,
            (900.0, 38.0),
            (0.0, 0.0),
            TargetDensity::Compact
        )),
        "and it must not cover Compact, where the same strip is reachable",
    );
    assert!(
        !dock_entry.matches(&violation(
            dock,
            (900.0, 38.0),
            (900.0, 20.0),
            TargetDensity::Touch
        )),
        "nor a strip that is merely undersized rather than unreachable — the \
         finding is a reach of zero, and a different shortfall in the same \
         region is a different finding",
    );
}

/// An outset's claim survives the miss-only slop pass — measured in the two
/// shipped controls where the two mechanisms fight, at the two densities where
/// each fight is visible.
///
/// The mechanisms otherwise contradict each other and the outset always loses: a
/// grip only ever claims a point at a **positive** distance from its own shape,
/// which is exactly the condition under which a slop-eligible node lying under
/// the ring is strictly closer. `arena.rs`'s `won_through_outset` is what stops
/// that, and this is the measurement it rests on. Reverting it costs:
///
/// | subject | density | with | without |
/// | --- | --- | ---: | ---: |
/// | `SearchField`'s 16 dp clear button, beside its own field | Compact | 24 dp | 22 dp |
/// | `TableView`'s 12 dp scroll bar, beside 28 dp rows | Touch | 32 dp | 18 dp |
///
/// Both halves matter, and neither is the whole story on its own. The Compact
/// row is a **regression in a shipped control at the density CI runs at** — the
/// field beside the clear button is itself under 24 dp, so it is a slop
/// candidate there. The Touch row is the reach going **down** as density rises,
/// because `up_to` 44 turns a 28 dp table row into a candidate that a 24 dp
/// `up_to` left inert. A test that measured only one of them would report half
/// the mechanism.
#[test]
fn an_outsets_claim_survives_the_slop_pass_in_the_shipped_controls() {
    let clear = measurements_of("search_field", "HitTarget", TargetDensity::Compact);
    assert_eq!(
        clear.len(),
        1,
        "a search field with text has one clear button"
    );
    assert_dp(clear[0].size, (16.0, 16.0), "the clear button's paint");
    assert_dp(
        clear[0].expanded,
        (24.0, 24.0),
        "the clear button must keep the whole floor its outset claims — 22 dp \
         means the field beside it took the leading half back through the \
         miss-only pass, which is a Compact regression",
    );
    assert!(clear[0].sources.outset, "and the outset must be credited");
    assert_eq!(clear[0].rule, None, "so it carries no verdict");

    let bar = measurements_of("table_view", "ScrollBar", TargetDensity::Touch);
    let node = bar
        .iter()
        .find(|m| m.part.is_none() && m.size.width > 0.0)
        .expect("the scroll bar's own node");
    assert_dp(
        node.size,
        (12.0, 268.0),
        "the table's scroll bar paints 12 dp",
    );
    assert!(
        (node.expanded.width - 32.0).abs() <= PIN_TOLERANCE,
        "the bar must keep its Touch ring across its thickness — 18 dp means a \
         28 dp row beside it won the ring back, and the reach then falls as the \
         density rises — measured {:.4}",
        node.expanded.width,
    );
    assert!(node.sources.outset, "and the outset must be credited");
    assert_eq!(
        node.rule,
        Some(TargetRule::TouchTargetRecommendation),
        "32 dp clears the 24 dp AA floor and is short of the 44 dp Touch \
         recommendation, which is informational: {node:#?}",
    );
}
