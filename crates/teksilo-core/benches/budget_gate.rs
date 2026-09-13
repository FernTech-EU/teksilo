// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The budget gate's arithmetic, pinned.
//!
//! `pointer_dispatch.rs` runs under criterion's harness, and a
//! `harness = false` target never runs a `#[test]`. So the part of the gate
//! that decides — and in particular the separation between the hard budget and
//! the advisory numbers — is tested from here, under the ordinary libtest
//! harness, and runs with `cargo test -p teksilo-core --benches`.
//!
//! The one thing that cannot be pinned from here is the wall-clock
//! measurement itself. What can be pinned is everything that turns a
//! measurement into an exit status, which is what a regression would have to
//! get past.

// Shared with the `pointer_dispatch` bench target. Each uses a different part
// of it.
#[allow(dead_code)]
mod budget;

use std::time::Duration;

use budget::{
    ADVISORY_MARGIN, Advisory, GATE_CONTACTS, Gate, HARD_BUDGET, Measured, Report, Verdict, budget,
    budget_from, median, micros,
};

fn gate_at(per_sample: Duration) -> Gate {
    Gate {
        subject: Measured::new("subject", per_sample),
        budget: HARD_BUDGET,
    }
}

/// The number A21 states, in the unit it states it in. A silent edit of the
/// constant is the one regression the rest of this file cannot see.
#[test]
fn the_hard_budget_is_twenty_five_microseconds_at_ten_contacts() {
    assert_eq!(HARD_BUDGET, Duration::from_micros(25));
    assert_eq!(GATE_CONTACTS, 10);
}

/// A measurement inside the budget passes and exits zero.
#[test]
fn a_measurement_inside_the_budget_passes() {
    let gate = gate_at(Duration::from_nanos(24_999));
    assert_eq!(gate.verdict(), Verdict::Pass);
    assert_eq!(gate.verdict().exit_code(), 0);
}

/// The boundary is inclusive: exactly 25 µs is inside the budget, not over it.
#[test]
fn the_budget_boundary_is_inclusive() {
    assert_eq!(gate_at(HARD_BUDGET).verdict(), Verdict::Pass);
}

/// The seeded breach: one nanosecond over is a failure, and the failure is an
/// exit status a CI job can assert on.
#[test]
fn a_measurement_over_the_budget_fails_the_build() {
    let gate = gate_at(HARD_BUDGET + Duration::from_nanos(1));
    assert_eq!(gate.verdict(), Verdict::Breach);
    assert_eq!(gate.verdict().exit_code(), 1);

    let report = Report::new(gate);
    assert_eq!(report.exit_code(), 1);
    assert!(
        report.render().contains("-> FAIL"),
        "the gate line must say so in words as well as in the exit status: {}",
        report.render()
    );
}

/// The advisory numbers are reported and cannot fail a build.
///
/// This is the invariant the whole two-field shape of [`Report`] exists for: a
/// report carrying advisories a hundred times over their baseline still passes
/// as long as the gate does. `verdict` reads `gate` and nothing else, so there
/// is no path by which an advisory could reach the exit status.
#[test]
fn advisories_never_move_the_verdict() {
    let mut report = Report::new(gate_at(Duration::from_micros(1)));
    assert_eq!(report.verdict(), Verdict::Pass);

    for factor in [2, 10, 100] {
        report.advise(
            Measured::new("catastrophe", Duration::from_micros(factor)),
            Measured::new("baseline", Duration::from_nanos(10)),
        );
    }

    assert_eq!(report.verdict(), Verdict::Pass, "still the gate's answer");
    assert_eq!(report.exit_code(), 0);

    let text = report.render();
    assert_eq!(
        text.lines().filter(|l| l.starts_with("advisory")).count(),
        3,
        "every advisory is printed: {text}"
    );
    assert!(
        text.lines()
            .filter(|l| l.starts_with("advisory"))
            .all(|l| l.contains("informational")),
        "an advisory line must not read as a verdict: {text}"
    );
}

/// A breaching gate fails whatever the advisories say about it — including
/// advisories that look healthy.
#[test]
fn a_healthy_advisory_cannot_rescue_a_breached_gate() {
    let mut report = Report::new(gate_at(Duration::from_micros(80)));
    report.advise(
        Measured::new("touch", Duration::from_nanos(1_000)),
        Measured::new("mouse", Duration::from_nanos(1_000)),
    );
    assert_eq!(report.verdict(), Verdict::Breach);
    assert_eq!(report.exit_code(), 1);
}

/// The relative delta is `(subject − baseline) / baseline`, signed.
#[test]
fn the_advisory_delta_is_relative_to_its_baseline() {
    let advisory = Advisory {
        subject: Measured::new("subject", Duration::from_nanos(1_050)),
        baseline: Measured::new("baseline", Duration::from_nanos(1_000)),
    };
    assert!((advisory.delta() - 0.05).abs() < 1e-9);
    assert!((ADVISORY_MARGIN - 0.05).abs() < 1e-9);

    let faster = Advisory {
        subject: Measured::new("subject", Duration::from_nanos(900)),
        baseline: Measured::new("baseline", Duration::from_nanos(1_000)),
    };
    assert!(faster.delta() < 0.0, "a faster subject reads negative");
}

/// A baseline too fast for the clock reports zero rather than infinity: a
/// measurement below the noise floor has no meaningful ratio, and `inf` in a
/// CI log reads as a catastrophe instead of as an absence.
#[test]
fn a_zero_baseline_reports_no_delta() {
    let advisory = Advisory {
        subject: Measured::new("subject", Duration::from_nanos(1_000)),
        baseline: Measured::new("baseline", Duration::ZERO),
    };
    assert_eq!(advisory.delta(), 0.0);
}

/// The environment override can only ever tighten the budget.
#[test]
fn the_budget_override_can_only_tighten() {
    assert_eq!(budget_from(Some("1000")), Duration::from_nanos(1_000));
    assert_eq!(
        budget_from(Some("999999999")),
        HARD_BUDGET,
        "a loosening value is clamped back to the published budget"
    );
    assert_eq!(budget_from(Some(" 5000 ")), Duration::from_nanos(5_000));
}

/// Nonsense in the environment is ignored, in both directions: a stray
/// variable must not turn a green build red, and it must not turn a red one
/// green either.
#[test]
fn a_malformed_budget_override_is_ignored() {
    assert_eq!(budget_from(None), HARD_BUDGET);
    assert_eq!(budget_from(Some("")), HARD_BUDGET);
    assert_eq!(budget_from(Some("fast")), HARD_BUDGET);
    assert_eq!(budget_from(Some("-1")), HARD_BUDGET);
    assert_eq!(budget_from(Some("2.5")), HARD_BUDGET);
}

/// With nothing in the environment, the live budget is the published one.
///
/// Reads the process environment, so it is the one test here that could be
/// perturbed by the shell it runs in — which is exactly the perturbation worth
/// noticing.
#[test]
fn the_default_budget_is_the_published_one() {
    if std::env::var(budget::BUDGET_OVERRIDE_VAR).is_ok() {
        // The caller is rehearsing a breach. The clamp is what matters then,
        // and `the_budget_override_can_only_tighten` covers it.
        return;
    }
    assert_eq!(budget(), HARD_BUDGET);
}

/// The median is the middle of an odd set and the mean of the middle pair of
/// an even one — and it is what a lone descheduled round cannot move.
#[test]
fn the_median_ignores_a_single_outlier() {
    let mut odd = [
        Duration::from_nanos(30),
        Duration::from_nanos(10),
        Duration::from_nanos(20),
    ];
    assert_eq!(median(&mut odd), Duration::from_nanos(20));

    let mut even = [
        Duration::from_nanos(10),
        Duration::from_nanos(20),
        Duration::from_nanos(30),
        Duration::from_nanos(40),
    ];
    assert_eq!(median(&mut even), Duration::from_nanos(25));

    let mut with_outlier = [
        Duration::from_nanos(10),
        Duration::from_nanos(11),
        Duration::from_nanos(12),
        Duration::from_nanos(13),
        Duration::from_secs(9),
    ];
    assert_eq!(median(&mut with_outlier), Duration::from_nanos(12));
}

/// The gate line carries the measurement, the budget it was held to, and the
/// verdict — the three things a log reader needs and a job script may grep.
#[test]
fn the_gate_line_states_measurement_budget_and_verdict() {
    let report = Report::new(gate_at(Duration::from_micros(12)));
    let line = report.render();
    let gate_line = line.lines().next().expect("a report opens with its gate");
    assert!(gate_line.starts_with("gate "), "{gate_line}");
    assert!(gate_line.contains("12.00 µs/sample"), "{gate_line}");
    assert!(gate_line.contains("budget 25.00 µs"), "{gate_line}");
    assert!(gate_line.ends_with("-> PASS"), "{gate_line}");
}

/// Microseconds, from a `Duration`, for the printed lines.
#[test]
fn micros_converts_for_printing() {
    assert!((micros(Duration::from_micros(25)) - 25.0).abs() < 1e-9);
    assert!((micros(Duration::from_nanos(1_500)) - 1.5).abs() < 1e-9);
}
