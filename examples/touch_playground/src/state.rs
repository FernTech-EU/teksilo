// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared playground state: what the pad saw, and what each scenario decided.
//!
//! Every field is a `Signal`, so a panel binds to the ones it displays and
//! nothing polls. The state is cloned into each panel at build time — it is a
//! handle, like every other reactive model in the framework.

use std::collections::BTreeMap;

use teksilo::canvas::Point;
use teksilo::core::signal::Signal;
use teksilo::tokens::{PointerKind, ScrollPhysics, TargetDensity};

/// One live pointer, as the pad last saw it.
///
/// Everything here comes from a `WidgetEvent` the pad received plus the
/// `EventContext` that delivered it — not from the tree's pointer table, which
/// no widget can reach. See [`crate::pad`] for why that distinction decides the
/// whole shape of this example.
#[derive(Clone, Debug, PartialEq)]
pub struct PointerRecord {
    /// The raw identity, the same number a `TEKSILO_TRACE_INPUT` line prints.
    pub id: u64,
    pub kind: PointerKind,
    /// The W3C primary flag as the sample carried it.
    pub primary: bool,
    /// Whether any button is held after the last sample — for a contact, down.
    pub down: bool,
    /// Where the last sample landed, in the pad's own coordinates.
    pub position: Point,
    /// Tip pressure as the device reported it, `None` when it reports none.
    pub pressure: Option<f32>,
    /// Stylus tilt in degrees, `(x, y)`.
    pub tilt: Option<(f32, f32)>,
    /// Stylus barrel rotation in degrees.
    pub twist: Option<f32>,
    /// The contact patch, where the digitizer sizes it.
    pub contact: Option<(f32, f32)>,
    /// Speed in dp/s, from a per-pointer velocity tracker the pad feeds.
    pub speed: f32,
    /// The `TouchAction` frozen for this pointer's press, formatted.
    pub touch_action: String,
    /// Whether the framework holds a press for the pad for this pointer.
    pub pressed: bool,
    /// Whether the pad still holds this pointer's capture.
    pub owns: bool,
}

/// The kinetic constants the tuning panel drives, as offsets from the theme's.
///
/// Held as a separate struct rather than a whole `ScrollPhysicsTokens` so the
/// panel's reset is "drop these", not "remember what the theme said" — the
/// theme is re-projected from the preset on every apply.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KineticKnobs {
    /// Which simulation family the fling runs.
    pub physics: ScrollPhysics,
    /// Android `ViewConfiguration`'s scroll friction. Lower coasts further.
    pub clamping_friction: f32,
    /// Flutter `BouncingScrollSimulation`'s per-second velocity retention.
    pub bouncing_decay_per_second: f32,
    /// The gain applied to travel past a boundary.
    pub rubber_band_factor: f32,
    /// Whether the demo surfaces follow the finger past their own end.
    pub rubber_band: bool,
}

impl KineticKnobs {
    /// The shipped values, read from the token defaults rather than retyped.
    pub fn shipped() -> Self {
        let tokens = teksilo::tokens::ScrollPhysicsTokens::DEFAULT;
        Self {
            physics: tokens.physics,
            clamping_friction: tokens.clamping_friction,
            bouncing_decay_per_second: tokens.bouncing_decay_per_second,
            rubber_band_factor: tokens.rubber_band_factor,
            rubber_band: false,
        }
    }

    /// Fold these knobs into a token set.
    pub fn apply(
        &self,
        mut tokens: teksilo::tokens::ScrollPhysicsTokens,
    ) -> teksilo::tokens::ScrollPhysicsTokens {
        tokens.physics = self.physics;
        tokens.clamping_friction = self.clamping_friction;
        tokens.bouncing_decay_per_second = self.bouncing_decay_per_second;
        tokens.rubber_band_factor = self.rubber_band_factor;
        tokens
    }
}

/// A scenario's identity. The outcome map is keyed by it, and the same constant
/// names the scenario's heading, so a readout and a panel cannot disagree about
/// which scenario they are describing.
pub type ScenarioId = &'static str;

pub const SCENARIO_LIST_ROW: ScenarioId = "list row in a scroller";
pub const SCENARIO_SLIDER: ScenarioId = "slider in a scroller";
pub const SCENARIO_TEXT: ScenarioId = "text selection in a scroller";
pub const SCENARIO_GRIP: ScenarioId = "splitter grip in a scroller";
pub const SCENARIO_NESTED: ScenarioId = "nested scrollers at the boundary";

/// Every scenario, in the order the panel stacks them.
pub const ALL_SCENARIOS: &[ScenarioId] = &[
    SCENARIO_LIST_ROW,
    SCENARIO_SLIDER,
    SCENARIO_TEXT,
    SCENARIO_GRIP,
    SCENARIO_NESTED,
];

/// Everything the playground shares between its panes.
#[derive(Clone)]
pub struct PlaygroundState {
    /// The pad's live contacts, sorted by identity.
    pub pointers: Signal<Vec<PointerRecord>>,
    /// The last hovering-capable pointer to reach the pad, and its kind.
    ///
    /// The pad's own answer, not the tree's: the tree elects a hover owner
    /// across the whole window and nothing a widget can reach asks it. See
    /// [`crate::pad`].
    pub hover_owner: Signal<Option<(u64, PointerKind)>>,
    /// A rolling log of what the pad saw, newest last. Bounded; see
    /// [`Self::log`].
    pub events: Signal<Vec<String>>,
    /// The active density.
    pub density: Signal<TargetDensity>,
    /// Bumped whenever the tree must be rebuilt for a token change to reach the
    /// widgets that snapshot it. [`crate::Root`] binds this one — not `density`
    /// — at `BindingLevel::Rebuild`, because the kinetic knobs need the same
    /// rebuild and `Signal`'s equality guard would filter a re-set of a density
    /// that did not change.
    pub rebuild: Signal<u64>,
    /// The kinetic knobs the tuning panel drives.
    pub kinetic: Signal<KineticKnobs>,
    /// Who won each scenario's last gesture, keyed by [`ScenarioId`].
    pub outcomes: Signal<BTreeMap<ScenarioId, String>>,
}

/// How many lines the event log keeps. A touch session produces a move sample
/// per frame per contact, so an unbounded log is a leak with a sixty-a-second
/// fill rate.
const LOG_CAPACITY: usize = 14;

impl PlaygroundState {
    pub fn new(density: TargetDensity) -> Self {
        Self {
            pointers: Signal::new(Vec::new()),
            hover_owner: Signal::new(None),
            events: Signal::new(Vec::new()),
            density: Signal::new(density),
            rebuild: Signal::new(0),
            kinetic: Signal::new(KineticKnobs::shipped()),
            outcomes: Signal::new(BTreeMap::new()),
        }
    }

    /// Ask for a rebuild. See [`Self::rebuild`].
    pub fn bump_rebuild(&self) {
        self.rebuild.set(self.rebuild.get().wrapping_add(1));
    }

    /// Append one line to the event log, dropping the oldest past
    /// [`LOG_CAPACITY`].
    pub fn log(&self, line: impl Into<String>) {
        let mut lines = self.events.get();
        lines.push(line.into());
        let len = lines.len();
        if len > LOG_CAPACITY {
            lines.drain(..len - LOG_CAPACITY);
        }
        self.events.set(lines);
    }

    /// Record which contender won `scenario`'s last gesture.
    ///
    /// The string is what the readout shows and what the scenario tests assert,
    /// so it is the scenario's whole observable answer. Keep it short enough to
    /// read at a glance on a tablet.
    pub fn report(&self, scenario: ScenarioId, outcome: impl Into<String>) {
        let outcome = outcome.into();
        let mut map = self.outcomes.get();
        let changed = map.get(scenario).map(|s| s != &outcome).unwrap_or(true);
        if !changed {
            return;
        }
        self.log(format!("{scenario}: {outcome}"));
        map.insert(scenario, outcome);
        self.outcomes.set(map);
    }

    /// What `scenario` last decided, or the placeholder shown before its first
    /// gesture.
    pub fn outcome(&self, scenario: ScenarioId) -> String {
        self.outcomes
            .get()
            .get(scenario)
            .cloned()
            .unwrap_or_else(|| "—".to_string())
    }
}

impl std::fmt::Debug for PlaygroundState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaygroundState")
            .field("density", &self.density.get())
            .field("pointers", &self.pointers.get().len())
            .finish()
    }
}
