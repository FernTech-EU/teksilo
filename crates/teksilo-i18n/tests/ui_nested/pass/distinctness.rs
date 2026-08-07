// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Verifies that the option-B nested-separator scheme prevents
//! collisions between paths that would all have mapped to the same
//! Fluent key under the old dash-joining behavior. Each of the four
//! `tr!` calls below resolves to a different message defined in
//! `nested_fixture/distinct.ftl`.

use teksilo_i18n::tr;

fn main() {
    // Flat single-segment path. Within-segment `_` → `-` gives
    // `foo-bar-baz`; no `__` joiner because there's only one segment.
    let flat = tr!(foo_bar_baz());
    assert_eq!(flat.resolve_now(), "flat");

    // Two segments: `foo` / `bar_baz`. Within-segment `_` → `-`
    // gives `foo` + `bar-baz`, joined with `__` → `foo__bar-baz`.
    let two_a = tr!(foo::bar_baz());
    assert_eq!(two_a.resolve_now(), "two-seg-a");

    // Two segments: `foo_bar` / `baz`. Within-segment `_` → `-`
    // gives `foo-bar` + `baz`, joined with `__` → `foo-bar__baz`.
    let two_b = tr!(foo_bar::baz());
    assert_eq!(two_b.resolve_now(), "two-seg-b");

    // Three segments: `foo` / `bar` / `baz`. No within-segment
    // conversion, `__` joiner at both boundaries → `foo__bar__baz`.
    let three = tr!(foo::bar::baz());
    assert_eq!(three.resolve_now(), "three-seg");
}
