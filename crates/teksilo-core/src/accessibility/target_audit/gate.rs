// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The matcher every conformance gate shares.
//!
//! The rule, not a count: **every gate reads this one allow-list vocabulary and
//! asks these same questions of it, because N copies of one matcher is how two
//! of them drift.** Three gates do so today — the stock widget catalog's in
//! `teksilo-target-conformance`, and `teksilo-charts`' and `teksilo-scene`'s —
//! and the file is written so a fourth costs a `Roster` and nothing else. What
//! lives here is the *asking*; the entries stay in each crate's own gate,
//! because they are that crate's findings.
//!
//! Every function returns findings rather than panicking, so a shipped library
//! keeps no assert-shaped API. A caller writes
//! `assert!(v.is_empty(), "{}", v.join("\n"))`.
//!
//! Every check below has a **negative** test of its own — one that seeds the
//! single defect that check exists to catch — in this module's `tests`
//! submodule. They are not optional decoration. The gates are written over
//! shipped geometry and a clean allow-list, so none of them can seed a
//! malformed entry, and before those tests existed almost nothing here was
//! held: measured, with each of [`conformance_failures`], [`stale_entries`] and
//! [`non_narrowing`] replaced by an empty `Vec` in turn, all three gates stayed
//! green; with [`roster_defects`] replaced the same way, exactly one test in one
//! gate reddened, so its other ten lints were unheld as well.
//!
//! # The census, the four questions asked of it, and why each is needed
//!
//! | function | asks |
//! | --- | --- |
//! | [`conformance_census`] | *(the shared input)* every AA failure the fixtures produce, over the whole (theme x density) product |
//! | [`conformance_failures`] | is there a failure no entry excuses? |
//! | [`stale_entries`] | does every entry, every pin, and every (theme, density) pair it claims still meet a real failure? |
//! | [`non_narrowing`] | does an entry excuse something it should **not** — a seeded regression, or a theme that does not exhibit the geometry? |
//! | [`roster_defects`] | is the list itself well formed: an owner, a justification that names any exception it claims, a pin per geometry, and a theme axis that names only themes the gate measures? |
//!
//! [`stale_entries`] and [`non_narrowing`] are opposite questions and both are
//! load-bearing. An entry that outlives what it excused is a hole waiting for
//! the next control whose name contains the same substring; an entry with no
//! size discriminator passes staleness and lets a real regression through.

use teksilo_tokens::TargetDensity;

use super::{AllowedViolation, Owner, TargetFixture, TargetViolation, audit_fixtures};
use crate::styles::Theme;

/// The three density rungs, in ladder order — the list every gate sweeps.
pub const ALL_DENSITIES: &[TargetDensity] = &[
    TargetDensity::Compact,
    TargetDensity::Comfortable,
    TargetDensity::Touch,
];

/// Type-name prefixes the shipped presets use for the private widgets they wrap
/// a delegated control in.
///
/// An allow-list `path` containing one of these spans a theme's own chrome, and
/// such an entry goes **silently stale** the day that theme renames or restacks
/// it: the path stops matching, the entry excuses nothing, and the gate reports
/// nothing wrong with the entry. A path must name framework structure, which
/// every preset's delegated row shares.
///
/// `M3` is on the list because it is the prefix Material 3 actually gives its
/// private widgets (`M3SwitchBody`); no `Widget` type in that crate starts with
/// `Material3`, which stays here only to catch a future one written longhand.
const THEME_OWNED_PREFIXES: &[&str] = &["Fluent", "MacOs", "Material3", "M3"];

/// The id every theme built from raw tokens carries, so it names no preset and
/// is never an admissible pin.
const ANONYMOUS_THEME: &str = "custom";

/// The closed set of WCAG 2.2 SC 2.5.8 exceptions, as an entry's
/// [`exception`](AllowedViolation::exception) writes them.
///
/// The field is documented as "structural rather than prose, so a roster's
/// counts can be asserted" — which only holds if the value itself is held to
/// the criterion's own list. A typo'd or invented name would otherwise pass
/// every lint (the justification echoes it) and inflate the exception count
/// with a discharge no criterion grants.
const SC_2_5_8_EXCEPTIONS: &[&str] = &[
    "Spacing",
    "Equivalent",
    "Inline",
    "User agent control",
    "Essential",
];

/// One preset a gate measures: the id its pins name, and the constructor that
/// builds it.
///
/// The id is declared rather than derived so [`roster_defects`] can check the
/// two agree — a roster whose declared id has drifted from the theme's own
/// [`ThemeId`](crate::styles::ThemeId) would silently excuse nothing.
#[derive(Clone, Copy)]
pub struct ThemeSubject {
    /// The theme's own `ThemeId`, as a pin writes it.
    pub id: &'static str,
    /// Builds the theme. The gate re-derives it per density through
    /// `Theme::with_density`, so a preset's own density projection runs.
    ///
    /// That door's default branch **replaces the whole `input` token group with
    /// the generic table**: a subject theme that installs its own ladder (a
    /// raised `min_target_conformance` above all) keeps it through the sweep
    /// only by registering a
    /// [`DensityProjection`](crate::styles::DensityProjection).
    pub theme: fn() -> Theme,
}

/// Everything a gate measures against: the presets and the allow-list.
/// (A per-preset viewport override was designed for and then measured away —
/// the comment below the struct records why.)
#[derive(Clone, Copy)]
pub struct Roster {
    /// One entry per theme family the gate sweeps.
    ///
    /// Light and dark are geometrically identical in every shipped preset, so a
    /// roster names one representative per family — a claim a gate owes a test
    /// of its own, because it is the assumption that halves the sweep.
    pub themes: &'static [ThemeSubject],
    /// The written findings the gate lets through.
    pub allow: &'static [AllowedViolation],
}

// One fixture, one viewport, every preset. A per-preset viewport override was
// designed for and then MEASURED AWAY: the one subject it was meant to serve — a
// Material 3 calendar's header title, 11.2 dp wide where Int UI's is 27.2 — does
// not move at 440, 480 or 520 dp either. A calendar sizes to its own grid, so
// the header's squeeze comes from the nav arrows inside that width and not from
// the room around it, and the shortfall is a finding rather than an artefact.
// Nothing else in any of the three fixture lists wanted one, so the mechanism
// would have been a door with nothing behind it and a second way for a gate to
// go quiet.

/// Every conformance failure the fixtures produce, under every theme in the
/// roster and at every density.
///
/// Only [`TargetRule::is_conformance_failure`](super::TargetRule::is_conformance_failure)
/// rows are returned: the other two rules are a census, not a gate. Compute it
/// once and hand it to the four questions below — each sweeps the whole product
/// and a gate that recomputed it per test would pay for it four times.
///
/// **The roster owns the theme axis, so a fixture's own
/// [`TargetFixture::with_theme`](super::TargetFixture::with_theme) is
/// overwritten here** — a list swept over a product cannot also have per-fixture
/// presets, or a pin's theme would name something the fixture never used. A
/// fixture that genuinely needs its own theme belongs in a test that calls
/// [`audit_fixtures`] or `measure_fixtures` directly, never in a list handed to
/// this function.
pub fn conformance_census(fixtures: &[TargetFixture], roster: &Roster) -> Vec<TargetViolation> {
    let mut out = Vec::new();
    for subject in roster.themes {
        let under_theme: Vec<TargetFixture> = fixtures
            .iter()
            .map(|f| (*f).with_theme(subject.theme))
            .collect();
        for &density in ALL_DENSITIES {
            out.extend(
                audit_fixtures(&under_theme, density)
                    .into_iter()
                    .filter(|v| v.rule.is_conformance_failure()),
            );
        }
    }
    out
}

/// A gate's census, computed once per test binary.
///
/// The sweep is the whole fixture list at three densities under every preset in
/// the roster, and a gate's tests run as threads in one process, so each of them
/// wants the same answer. Every gate used to carry its own `OnceLock` and its
/// own copy of this paragraph; the policy lives here instead, so a fourth gate
/// costs a [`Roster`] and nothing else.
///
/// ```ignore
/// static CENSUS: gate::CensusCell = gate::CensusCell::new();
/// fn census() -> &'static [TargetViolation] { CENSUS.get(&fixtures(), &ROSTER) }
/// ```
pub struct CensusCell(std::sync::OnceLock<Vec<TargetViolation>>);

impl CensusCell {
    /// An empty cell, `const` so a gate can declare one as a `static`.
    pub const fn new() -> Self {
        Self(std::sync::OnceLock::new())
    }

    /// The census, computing it on the first call.
    pub fn get(&self, fixtures: &[TargetFixture], roster: &Roster) -> &[TargetViolation] {
        self.0.get_or_init(|| conformance_census(fixtures, roster))
    }
}

impl Default for CensusCell {
    fn default() -> Self {
        Self::new()
    }
}

/// What the gate would report for `fixtures` — the census and the matcher in one
/// call.
///
/// The liveness half of a gate's "a deliberately undersized fixture fails" test:
/// a fixture no entry names must come back out of the gate's own path, not just
/// out of the walker. Each gate then asserts on the failure text it expects,
/// which is the part that is genuinely its own.
pub fn reported_failures(fixtures: &[TargetFixture], roster: &Roster) -> Vec<String> {
    conformance_failures(&conformance_census(fixtures, roster), roster)
}

/// **The gate.** Every conformance failure no entry excuses.
pub fn conformance_failures(census: &[TargetViolation], roster: &Roster) -> Vec<String> {
    census
        .iter()
        .filter(|v| !roster.allow.iter().any(|e| e.matches(v)))
        .map(|v| v.to_string())
        .collect()
}

/// Every entry, every pinned geometry and every (theme, density) pair it claims
/// must still match a real failure.
///
/// Strictly stronger than "the entry matches something": an entry that pinned
/// three geometries and only ever meets one has two exemptions nothing
/// justifies, and an entry claiming a theme the control conforms under is
/// excusing a regression in advance.
pub fn stale_entries(census: &[TargetViolation], roster: &Roster) -> Vec<String> {
    let mut out = Vec::new();
    for entry in roster.allow {
        for (index, pin) in entry.measured.iter().enumerate() {
            for &density in pin.densities {
                for theme in pin.themes {
                    let met = census.iter().any(|v| {
                        v.density == density
                            && v.theme.as_str() == *theme
                            && v.path.contains(entry.path)
                            && pin.covers(v)
                    });
                    if !met {
                        out.push(format!(
                            "`{}` pin {index} matches no conformance failure at {density:?} \
                             under {theme} any more. Either the geometry moved — re-measure it \
                             and rewrite the pin — or the control now conforms there, in which \
                             case narrow the pin's densities or themes, or delete it and its \
                             justification with it.",
                            entry.path,
                        ));
                    }
                }
            }
        }
    }
    out
}

/// **The allow-list narrows.** Nothing it excuses survives a seeded regression,
/// on any of the four scalars or on the theme.
///
/// The size seeds prove the size discriminator narrows. The theme seed proves
/// the theme discriminator narrows rather than decorates, and it asks a question
/// the size seeds cannot: a geometry excused under one preset must not be
/// excused under a preset that does not produce it. The seed is checked against
/// the **census**, not against the matcher, so the assertion is not a
/// restatement of [`PinnedGeometry::covers`](super::PinnedGeometry::covers).
///
/// `min_seeds_per_theme` guards the guard: a census that shrank silently proves
/// less than this reads as proving.
pub fn non_narrowing(
    census: &[TargetViolation],
    roster: &Roster,
    min_seeds_per_theme: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    // The floor a moved seed carries, per (theme, density). The census's own
    // rows are the authority — they carry the floor the walker actually judged
    // that preset with, the same "read it off the row" rule every matcher
    // follows — and a pair the census never measured falls back to deriving
    // through the theme's own density door, once per pair rather than once per
    // seed (a preset build re-runs its whole projection to answer one `f32`).
    let mut floors: std::collections::HashMap<(&str, TargetDensity), f32> =
        std::collections::HashMap::new();
    for v in census {
        floors
            .entry((v.theme.as_str(), v.density))
            .or_insert(v.conformance_floor);
    }
    for subject in roster.themes {
        let mut seeded = 0_usize;
        for violation in census.iter().filter(|v| v.theme.as_str() == subject.id) {
            if !roster.allow.iter().any(|e| e.matches(violation)) {
                // Reported by `conformance_failures`; seeding into an
                // unexcused failure proves nothing.
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
                if roster.allow.iter().any(|e| e.matches(&broken)) {
                    out.push(format!(
                        "a regression seeded on axis {axis} of a real violation is still \
                         excused, so the entry covering it is a blanket over its region rather \
                         than a pin on a geometry:\n  real:   {violation}\n  seeded: {broken}",
                    ));
                }
                seeded += 1;
            }
            for other in roster.themes {
                if other.id == subject.id {
                    continue;
                }
                let mut moved = violation.clone();
                moved.theme = crate::styles::ThemeId::new(other.id);
                // The floor moves with the theme. A clone that says it is
                // `other` while carrying the subject theme's floor would judge
                // a `ClearsFloor` axis for one preset against another preset's
                // number — the divergence `TargetViolation::conformance_floor`
                // exists to close, reintroduced here by a falsified clone. The
                // seed asks what this entry would do if the same finding
                // appeared under `other`, and that question is only well posed
                // if the whole row is `other`'s — which is why the floor comes
                // off the census's own rows under `other` first, and only falls
                // back to `Theme::with_density` for a pair the census never
                // measured (that door resets a projection-less theme's custom
                // ladder to the generic table).
                moved.conformance_floor = *floors
                    .entry((other.id, violation.density))
                    .or_insert_with(|| {
                        (other.theme)()
                            .with_density(violation.density)
                            .input
                            .min_target_conformance
                    });
                for entry in roster.allow.iter().filter(|e| e.matches(&moved)) {
                    for pin in entry.measured.iter().filter(|p| p.covers(&moved)) {
                        // Two things are deliberately NOT compared as literal
                        // equality. The path is matched through the ENTRY's own
                        // substring, because a preset wraps a delegated control
                        // in chrome of its own and the same finding reaches the
                        // audit under two different paths. And the geometry is
                        // matched by the PIN, not by a fixed tolerance, because
                        // an axis a pin writes as `ClearsFloor` is one it has
                        // said is not the finding — a link that reaches 45 dp
                        // under one preset and 49 under another is the same
                        // finding to the pin, and must be to this check too.
                        let exhibited = census.iter().any(|v| {
                            v.theme.as_str() == other.id
                                && v.path.contains(entry.path)
                                && pin.covers(v)
                        });
                        if !exhibited {
                            out.push(format!(
                                "`{}` excuses this geometry under {} as well, where the census \
                                 produces no such failure — so the theme axis decorates the pin \
                                 rather than narrowing it:\n  real:   {violation}\n  seeded: \
                                 {moved}",
                                entry.path, other.id,
                            ));
                        }
                    }
                }
                seeded += 1;
            }
        }
        if seeded < min_seeds_per_theme {
            out.push(format!(
                "only {seeded} regressions were seeded under {} — the census shrank, so this \
                 test is proving less than it reads as proving",
                subject.id,
            ));
        }
    }
    out
}

/// The list itself, and the roster it names: every structural property an entry
/// owes before any measurement is consulted, plus the theme-axis rules.
///
/// The theme half is what stops the gate going quiet: without it a roster could
/// gain a preset no entry ever mentions and no pin ever claims, and every one of
/// that preset's failures would be reported by [`conformance_failures`] — or,
/// worse, a roster could name a preset the gate never actually measures.
pub fn roster_defects(census: &[TargetViolation], roster: &Roster) -> Vec<String> {
    let mut out = Vec::new();
    for subject in roster.themes {
        let built = (subject.theme)();
        if built.id.as_str() != subject.id {
            out.push(format!(
                "the roster calls this theme `{}` and the theme calls itself `{}`, so every \
                 pin naming the first excuses nothing",
                subject.id, built.id,
            ));
        }
        if subject.id == ANONYMOUS_THEME {
            out.push(format!(
                "`{ANONYMOUS_THEME}` is the id every theme built from raw tokens carries, so a \
                 roster naming it measures no particular preset",
            ));
        }
    }
    let roster_ids: Vec<&str> = roster.themes.iter().map(|s| s.id).collect();

    for entry in roster.allow {
        if entry.why.is_empty() {
            out.push(format!(
                "allow-list entry `{}` carries no justification",
                entry.path,
            ));
        }
        if entry.owner.text().is_empty() {
            out.push(format!(
                "allow-list entry `{}` leaves its {} empty — an entry is a debt with an \
                 address, or a written reason there is no debt",
                entry.path,
                match entry.owner {
                    Owner::Named(_) => "owner",
                    Owner::NobodyBecause(_) => "reason for having no owner",
                },
            ));
        }
        if let Some(name) = entry.exception {
            if !SC_2_5_8_EXCEPTIONS.contains(&name) {
                out.push(format!(
                    "`{}` claims `{name}`, which is not one of SC 2.5.8's exceptions \
                     (Spacing / Equivalent / Inline / User agent control / Essential)",
                    entry.path,
                ));
            }
            if !entry.why.contains(name) {
                out.push(format!(
                    "`{}` claims the SC 2.5.8 *{name}* exception in a field its justification \
                     never mentions",
                    entry.path,
                ));
            }
        }
        if entry.measured.is_empty() {
            out.push(format!(
                "allow-list entry `{}` pins no measurement, so it excuses whatever its path \
                 happens to match",
                entry.path,
            ));
        }
        for segment in entry.path.split(" > ") {
            // A path's first segment may carry the fixture's name
            // (`"docking: DockingLayout"`), which is the form several real
            // entries use — so the name is stripped before the prefix test, or
            // a theme-owned type in exactly that position escapes the lint.
            let segment = segment.rsplit(": ").next().unwrap_or(segment).trim();
            if THEME_OWNED_PREFIXES.iter().any(|p| segment.starts_with(p)) {
                out.push(format!(
                    "allow-list entry `{}` names `{segment}`, a type one preset owns. A path \
                     across a theme's private chrome goes silently stale the day that theme \
                     restacks it — the entry stops matching and nothing reports it. Name \
                     framework structure instead, and put the theme on the pin.",
                    entry.path,
                ));
            }
        }
        for (index, pin) in entry.measured.iter().enumerate() {
            if pin.densities.is_empty() {
                out.push(format!("`{}` pin {index} claims no density", entry.path));
            }
            if pin.themes.is_empty() {
                out.push(format!(
                    "`{}` pin {index} claims no theme, so it excuses a geometry under every \
                     preset the gate ever gains",
                    entry.path,
                ));
            }
            for theme in pin.themes {
                if *theme == ANONYMOUS_THEME {
                    out.push(format!(
                        "`{}` pin {index} names `{ANONYMOUS_THEME}`, which every app theme \
                         shares — a pin on it is a pin on no preset",
                        entry.path,
                    ));
                }
                if !roster_ids.contains(theme) {
                    out.push(format!(
                        "`{}` pin {index} names `{theme}`, which this gate does not measure",
                        entry.path,
                    ));
                }
            }
        }
    }

    for subject in roster.themes {
        let reached = roster
            .allow
            .iter()
            .any(|e| e.measured.iter().any(|m| m.themes.contains(&subject.id)));
        let clean = !census.iter().any(|v| v.theme.as_str() == subject.id);
        if !reached && !clean {
            out.push(format!(
                "{} produces conformance failures and no entry names it, so this preset is \
                 absent from the list without being clean",
                subject.id,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests;
