// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pointer-dispatch performance budget: the numbers, and the arithmetic
//! that turns a measurement into a verdict.
//!
//! Two different things are stated here and they are deliberately kept apart.
//!
//! The **hard budget** — 25 µs per pointer sample at ten simultaneous contacts
//! — is a build gate. A 16.6 ms frame has to survive a burst of coalesced
//! samples with room left over for layout and paint, so the budget is set at
//! roughly a hundred samples of headroom per frame. [`Report::verdict`] answers
//! it and the bench binary turns that answer into a process exit status, so
//! `cargo bench -p teksilo-core --bench pointer_dispatch -- --gate-only` fails
//! outright on a breach. CI builds this target but does not run it.
//!
//! The **advisory** — the relative delta between a measurement and its mouse
//! baseline — is a number to read, not a wall to hit. Shared CI runners
//! routinely swing more than 5 % at microsecond scale, so a job that failed on
//! it would fail on the weather. It is reported and never fails.
//!
//! The separation is structural rather than a promise in a comment:
//! [`Report::verdict`] takes `&self` and reads *only* [`Report::gate`]. An
//! [`Advisory`] cannot reach it. `advisories_never_move_the_verdict` in
//! `budget_gate.rs` pins that.

use std::fmt::Write as _;
use std::time::Duration;

/// The number of simultaneous contacts the hard budget is stated at.
///
/// Ten because that is the contact cap — the largest number of pointers the
/// table will admit, and therefore the worst case a frame can be asked to
/// dispatch.
pub const GATE_CONTACTS: usize = 10;

/// The hard budget: at most this long per pointer sample, at
/// [`GATE_CONTACTS`] contacts.
pub const HARD_BUDGET: Duration = Duration::from_micros(25);

/// The relative delta an advisory comparison is read against. Printed beside
/// every advisory so the number has a scale; never enforced.
pub const ADVISORY_MARGIN: f64 = 0.05;

/// The environment variable that tightens the budget for a local run.
///
/// It can only ever make the gate stricter — [`budget_from`] clamps whatever
/// it reads to [`HARD_BUDGET`] — so it is a way to rehearse a breach or to run
/// a stricter gate on a fast machine, never a way to get past one.
pub const BUDGET_OVERRIDE_VAR: &str = "TEKSILO_DISPATCH_BUDGET_NS";

/// What the gate decided.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Inside the budget.
    Pass,
    /// Over the budget. The build fails.
    Breach,
}

impl Verdict {
    /// The process exit status this verdict calls for.
    pub fn exit_code(self) -> i32 {
        match self {
            Verdict::Pass => 0,
            Verdict::Breach => 1,
        }
    }

    /// The word printed on the gate line.
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Breach => "FAIL",
        }
    }
}

/// The budget this run is held to: [`HARD_BUDGET`], or a stricter one from
/// [`BUDGET_OVERRIDE_VAR`].
pub fn budget() -> Duration {
    budget_from(std::env::var(BUDGET_OVERRIDE_VAR).ok().as_deref())
}

/// [`budget`] with the environment read for it, so the clamp is testable.
///
/// A value that does not parse is ignored rather than fatal: a stray variable
/// in a shell must not be able to turn a green build red by accident, and it
/// cannot turn a red one green either.
pub fn budget_from(raw: Option<&str>) -> Duration {
    match raw.and_then(|s| s.trim().parse::<u64>().ok()) {
        Some(ns) => Duration::from_nanos(ns).min(HARD_BUDGET),
        None => HARD_BUDGET,
    }
}

/// One measured quantity: what it is, and how long one sample of it took.
#[derive(Clone, Debug)]
pub struct Measured {
    /// How the line names it.
    pub name: String,
    /// Time per sample, not per batch — the batch size is divided out by the
    /// caller that knows it.
    pub per_sample: Duration,
}

impl Measured {
    /// A measurement of `name` at `per_sample`.
    pub fn new(name: impl Into<String>, per_sample: Duration) -> Self {
        Self {
            name: name.into(),
            per_sample,
        }
    }
}

/// One advisory comparison: a measurement against the baseline it is read
/// against.
#[derive(Clone, Debug)]
pub struct Advisory {
    /// What is being compared.
    pub subject: Measured,
    /// What it is compared to.
    pub baseline: Measured,
}

impl Advisory {
    /// `(subject − baseline) / baseline`, positive when the subject is slower.
    ///
    /// `0.0` for a zero baseline: a measurement too fast for the clock has no
    /// meaningful ratio, and reporting `inf` would read as a catastrophe
    /// rather than as "below the noise floor".
    pub fn delta(&self) -> f64 {
        let base = self.baseline.per_sample.as_secs_f64();
        if base <= 0.0 {
            return 0.0;
        }
        (self.subject.per_sample.as_secs_f64() - base) / base
    }
}

/// What the gate measured and what it is held to.
#[derive(Clone, Debug)]
pub struct Gate {
    /// The measurement the budget applies to.
    pub subject: Measured,
    /// The budget in force for this run.
    pub budget: Duration,
}

impl Gate {
    /// Whether the measurement fits.
    pub fn verdict(&self) -> Verdict {
        if self.subject.per_sample <= self.budget {
            Verdict::Pass
        } else {
            Verdict::Breach
        }
    }
}

/// Everything one budget run produced.
///
/// The gate and the advisories are separate fields on purpose: `verdict` reads
/// the first and cannot see the second.
#[derive(Clone, Debug)]
pub struct Report {
    /// The one measurement that can fail the build.
    pub gate: Gate,
    /// Measurements that are printed and nothing else.
    pub advisories: Vec<Advisory>,
}

impl Report {
    /// A report whose only enforceable claim is `gate`.
    pub fn new(gate: Gate) -> Self {
        Self {
            gate,
            advisories: Vec::new(),
        }
    }

    /// Add an advisory comparison. Cannot affect [`Self::verdict`].
    pub fn advise(&mut self, subject: Measured, baseline: Measured) {
        self.advisories.push(Advisory { subject, baseline });
    }

    /// The verdict — from the gate alone.
    pub fn verdict(&self) -> Verdict {
        self.gate.verdict()
    }

    /// The process exit status this report calls for.
    pub fn exit_code(&self) -> i32 {
        self.verdict().exit_code()
    }

    /// The whole report as text, gate line first.
    ///
    /// Every advisory line says `advisory` and every advisory carries the word
    /// `informational`, so neither a human skimming a CI log nor a `grep` in a
    /// job script can mistake one for the gate.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let g = &self.gate;
        let _ = writeln!(
            out,
            "gate      {}: {:.2} µs/sample (budget {:.2} µs) -> {}",
            g.subject.name,
            micros(g.subject.per_sample),
            micros(g.budget),
            g.verdict().label(),
        );
        for a in &self.advisories {
            let _ = writeln!(
                out,
                "advisory  {}: {:.2} µs/sample vs {:.2} µs baseline ({}) \
                 -> {:+.1} % (informational, margin {:.0} %)",
                a.subject.name,
                micros(a.subject.per_sample),
                micros(a.baseline.per_sample),
                a.baseline.name,
                a.delta() * 100.0,
                ADVISORY_MARGIN * 100.0,
            );
        }
        out
    }
}

/// A `Duration` in microseconds, for printing.
pub fn micros(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

/// The median of `samples`, which it sorts in place.
///
/// The median rather than the mean because a bench run competing with the rest
/// of a CI runner collects occasional enormous outliers, and one descheduled
/// round must not decide a build.
pub fn median(samples: &mut [Duration]) -> Duration {
    assert!(!samples.is_empty(), "the median of nothing is undefined");
    samples.sort_unstable();
    let mid = samples.len() / 2;
    if samples.len() % 2 == 1 {
        samples[mid]
    } else {
        (samples[mid - 1] + samples[mid]) / 2
    }
}
