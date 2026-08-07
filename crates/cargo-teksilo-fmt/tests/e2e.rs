// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end tests for the `cargo-teksilo-fmt` binary.
//!
//! Each test invokes the compiled binary against a temp dir of fixture
//! files and asserts on exit code, stdout, and resulting file contents.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn binary_path() -> std::path::PathBuf {
    // The CARGO_BIN_EXE_<name> env var points at the compiled binary
    // when this test is run via `cargo test`.
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-teksilo-fmt"))
}

fn run(args: &[&str], cwd: &Path) -> Output {
    Command::new(binary_path())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to spawn cargo-teksilo-fmt")
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

#[test]
fn help_runs() {
    let tmp = TempDir::new().unwrap();
    let out = run(&["--help"], tmp.path());
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("USAGE"));
    assert!(stdout.contains("--check"));
}

#[test]
fn version_runs() {
    let tmp = TempDir::new().unwrap();
    let out = run(&["--version"], tmp.path());
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("cargo-teksilo-fmt "));
}

#[test]
fn unknown_option_errors() {
    let tmp = TempDir::new().unwrap();
    let out = run(&["--bogus"], tmp.path());
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown option"));
}

#[test]
fn check_passes_on_clean_file() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("clean.rs");
    write(
        &f,
        "fn build() {\n    teksu!(VStack {\n        spacing: 8.0\n    });\n}\n",
    );
    let out = run(&["--check", "clean.rs"], tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn check_fails_on_dirty_file() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("dirty.rs");
    write(
        &f,
        "fn build() { teksu!(VStack { spacing: 8.0 Button(\"ok\") }); }\n",
    );
    let out = run(&["--check", "dirty.rs"], tmp.path());
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Would reformat"));
    assert!(stdout.contains("dirty.rs"));

    // Verify the file is still untouched (--check is read-only).
    let after = std::fs::read_to_string(&f).unwrap();
    assert!(after.contains("teksu!(VStack { spacing: 8.0 Button(\"ok\") })"));
}

#[test]
fn formats_in_place() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("f.rs");
    write(
        &f,
        "fn build() { teksu!(VStack { spacing: 8.0 Button(\"ok\") }); }\n",
    );
    let out = run(&["f.rs"], tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = std::fs::read_to_string(&f).unwrap();
    assert!(
        after.contains("VStack {\n"),
        "expected reformatted output, got:\n{after}"
    );
}

#[test]
fn skips_target_directory() {
    let tmp = TempDir::new().unwrap();
    let target_dir = tmp.path().join("target");
    std::fs::create_dir(&target_dir).unwrap();
    write(
        &target_dir.join("a.rs"),
        "fn build() { teksu!(VStack { spacing: 8.0 }); }\n",
    );
    // Two "fields" (a bare property plus a child), so this body does NOT
    // parse as a plain `syn::Expr` and `format_file` actually reformats
    // it — a single-field body like `VStack { spacing: 8.0 }` (used for
    // `a.rs` above, which must be skipped regardless of its content) is
    // deliberately left untouched by the formatter (see
    // `teksilo_fmt::format_file`'s doc comment), so it would never be
    // reported under `--check` and this assertion would be checking
    // nothing.
    write(
        &tmp.path().join("b.rs"),
        "fn build() { teksu!(VStack { spacing: 8.0 Button(\"ok\") }); }\n",
    );

    let out = run(&["--check", "."], tmp.path());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("b.rs"));
    assert!(
        !stdout.contains("target/a.rs") && !stdout.contains("target\\a.rs"),
        "should skip target/, got:\n{stdout}"
    );
}

#[test]
fn no_op_on_file_without_teksilo_macro() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("plain.rs");
    let original = "fn main() {\n    println!(\"no teksu here\");\n}\n";
    write(&f, original);
    let out = run(&["plain.rs"], tmp.path());
    assert!(out.status.success());
    let after = std::fs::read_to_string(&f).unwrap();
    assert_eq!(after, original);
}

#[test]
fn missing_path_errors() {
    let tmp = TempDir::new().unwrap();
    let out = run(&["does-not-exist.rs"], tmp.path());
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("path not found"));
}

#[cfg(unix)]
#[test]
fn preserves_file_mode() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("f.rs");
    write(
        &f,
        "fn build() { teksu!(VStack { spacing: 8.0 Button(\"ok\") }); }\n",
    );
    // Set a distinctive mode that differs from NamedTempFile's default
    // 0600 — using 0644 (typical .rs file mode).
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();

    let out = run(&["f.rs"], tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644, "expected 0644, got {mode:o}");
}

#[test]
fn formats_multiple_files_recursively() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp.path().join("sub").join("a.rs"),
        "fn x() { teksu!(VStack { spacing: 1.0 Button(\"a\") }); }\n",
    );
    write(
        &tmp.path().join("b.rs"),
        "fn y() { teksu!(VStack { spacing: 2.0 Button(\"b\") }); }\n",
    );

    let out = run(&["."], tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let a = std::fs::read_to_string(tmp.path().join("sub").join("a.rs")).unwrap();
    let b = std::fs::read_to_string(tmp.path().join("b.rs")).unwrap();
    assert!(a.contains("VStack {\n"), "a.rs not reformatted:\n{a}");
    assert!(b.contains("VStack {\n"), "b.rs not reformatted:\n{b}");
}
