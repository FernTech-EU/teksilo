// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Trybuild UI tests for the `teksu!` proc macro.
//!
//! Each test case under `tests/teksi/pass/` is expected to compile
//! successfully; each case under `tests/teksi/fail/` is expected to
//! emit a compile error matching its sibling `.stderr` file.
//!
//! The glob has to name the directory that exists. trybuild treats a pattern
//! matching nothing as "no tests enabled yet" and the test then PASSES, so a
//! misspelled path here does not fail — it silently retires every fixture.

#[test]
fn teksi_trybuild() {
    let t = trybuild::TestCases::new();
    t.pass("tests/teksi/pass/*.rs");
    t.compile_fail("tests/teksi/fail/*.rs");
}
