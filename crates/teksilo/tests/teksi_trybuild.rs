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

use std::path::PathBuf;
use std::process::Command;

/// Cargo profile for the generated trybuild project.
///
/// trybuild compiles every fixture into its own target directory, so the
/// whole `teksilo` dependency graph is built again there — and built
/// *twice*, because trybuild's build step passes `--diagnostic-width=140`
/// in RUSTFLAGS while its run step does not, and cargo hashes RUSTFLAGS
/// into the artifact metadata. With full debuginfo that is two graphs of
/// several GB each plus two links per fixture of binaries reaching 500 MB.
/// On a cold CI runner the linker ran out of resources at the tenth
/// fixture, and the job overran its timeout before the last one.
///
/// The fixtures only need to compile and start, so line tables are all the
/// debuginfo they can use (a panicking fixture still reports its line).
/// Cargo discovers this file from the generated project directory, which
/// sits below trybuild's target directory; the workspace's own profiles are
/// untouched.
const TRYBUILD_PROFILE: &str = "[profile.dev]\ndebug = \"line-tables-only\"\n";

/// Reads `target_directory` the same way trybuild does, so the config lands
/// in the directory trybuild actually builds into — which follows
/// `CARGO_TARGET_DIR` and `build.target-dir`, not the repository layout.
fn cargo_target_dir() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version=1"])
        .output()
        .expect("run `cargo metadata`");
    assert!(output.status.success(), "`cargo metadata` failed");
    let json = String::from_utf8(output.stdout).expect("cargo metadata is UTF-8");
    // `--no-deps` metadata carries no nested quotes around this key, and a
    // path is the only string value following it, so a minimal scan is
    // enough — only `\\` and `\"` need unescaping (Windows paths).
    let key = "\"target_directory\":\"";
    let start = json.find(key).expect("cargo metadata has target_directory") + key.len();
    let mut raw = String::new();
    let mut chars = json[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => raw.push(chars.next().expect("unterminated escape")),
            c => raw.push(c),
        }
    }
    PathBuf::from(raw)
}

/// Installs [`TRYBUILD_PROFILE`] where the generated project will find it.
/// Idempotent: rewrites the file on every run so an edit here takes effect.
fn install_trybuild_profile() {
    let dir = cargo_target_dir()
        .join("tests")
        .join("trybuild")
        .join(".cargo");
    std::fs::create_dir_all(&dir).expect("create trybuild .cargo dir");
    std::fs::write(dir.join("config.toml"), TRYBUILD_PROFILE).expect("write trybuild config");
}

#[test]
fn teksi_trybuild() {
    install_trybuild_profile();
    let t = trybuild::TestCases::new();
    t.pass("tests/teksi/pass/*.rs");
    t.compile_fail("tests/teksi/fail/*.rs");
}
