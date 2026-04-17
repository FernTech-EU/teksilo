//! Trybuild UI tests for the `fern!` proc macro.
//!
//! Each test case under `tests/fern_ui/pass/` is expected to compile
//! successfully; each case under `tests/fern_ui/fail/` is expected to
//! emit a compile error matching its sibling `.stderr` file.

#[test]
fn fern_ui_trybuild() {
    let t = trybuild::TestCases::new();
    t.pass("tests/fern_ui/pass/*.rs");
    t.compile_fail("tests/fern_ui/fail/*.rs");
}
