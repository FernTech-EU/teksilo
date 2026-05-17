//! Trybuild UI tests for the `bati!` proc macro.
//!
//! Each test case under `tests/bastyde/pass/` is expected to compile
//! successfully; each case under `tests/bastyde/fail/` is expected to
//! emit a compile error matching its sibling `.stderr` file.

#[test]
fn bati_trybuild() {
    let t = trybuild::TestCases::new();
    t.pass("tests/bati/pass/*.rs");
    t.compile_fail("tests/bati/fail/*.rs");
}
