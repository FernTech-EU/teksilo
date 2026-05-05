//! Trybuild UI tests for the `tr!` proc macro.
//!
//! Each test case under `tests/ui/pass/` is expected to compile
//! successfully; each case under `tests/ui/fail/` is expected to emit
//! a compile error matching its sibling `.stderr` file.
//!
//! Because every test case would otherwise need its own `locales/`
//! directory next to the .rs source, we instead point the proc macro
//! at a single shared fixture via the `FERN_I18N_SOURCE_PATH`
//! environment variable (an override supported by
//! [`fern_i18n_macros::tr`] since Phase C). Setting the env var here
//! propagates to the child `cargo` processes that trybuild spawns.

#[test]
fn ui() {
    // Point the proc macro at the shared fixture for every test case.
    // The path is made absolute so it survives trybuild's temp-dir
    // cargo invocations. `std::env::set_var` mutates the current
    // process's environment; child processes (trybuild's cargo calls)
    // inherit it on spawn.
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui/locales/en-US.ftl");
    // SAFETY: single-threaded test entry; no other thread observes env.
    unsafe {
        std::env::set_var("FERN_I18N_SOURCE_PATH", fixture);
    }

    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
    t.compile_fail("tests/ui/fail/*.rs");
}
