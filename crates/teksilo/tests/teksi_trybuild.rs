// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Trybuild UI tests for the `teksu!` proc macro.
//!
//! Each test case under `tests/teksilo/pass/` is expected to compile
//! successfully; each case under `tests/teksilo/fail/` is expected to
//! emit a compile error matching its sibling `.stderr` file.

#[test]
fn teksi_trybuild() {
    let t = trybuild::TestCases::new();
    t.pass("tests/teksu/pass/*.rs");
    t.compile_fail("tests/teksu/fail/*.rs");
}
