//! Trybuild UI test for the nested directory layout (architecture
//! §12.2.3). A separate test binary from `tests/trybuild.rs` because
//! each sets a different `FERN_I18N_SOURCE_*` env var and we rely on
//! per-process env isolation — cargo runs each `tests/*.rs` as its
//! own binary, so env vars set in one don't leak into the other.

#[test]
fn ui_nested() {
    // Point the proc macro at the nested fixture for every test case
    // in this binary. Must be set before trybuild spawns child cargo
    // processes, which inherit the parent env.
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/nested_fixture");
    // SAFETY: single-threaded test entry; no other thread observes env.
    unsafe {
        // Clear the file-mode override so `FERN_I18N_SOURCE_DIR`
        // alone wins in `resolve_source`'s precedence check.
        std::env::remove_var("FERN_I18N_SOURCE_PATH");
        std::env::set_var("FERN_I18N_SOURCE_DIR", fixture_dir);
    }

    let t = trybuild::TestCases::new();
    t.pass("tests/ui_nested/pass/*.rs");
}
