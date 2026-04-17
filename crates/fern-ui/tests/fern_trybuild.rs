//! Trybuild UI tests for the `fern!` proc macro.
//!
//! Each test case under `tests/fern_ui/pass/` is expected to compile
//! successfully; each case under `tests/fern_ui/fail/` is expected to
//! emit a compile error matching its sibling `.stderr` file.

#[test]
fn fern_ui_trybuild() {
    let t = trybuild::TestCases::new();
    t.pass("tests/fern_ui/pass/*.rs");
    // Phase 4 will populate fail/*.rs. Keep the dir empty for now; no
    // call to t.compile_fail so trybuild doesn't choke on zero fixtures.
}
