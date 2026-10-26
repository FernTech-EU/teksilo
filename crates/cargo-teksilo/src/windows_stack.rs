// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Whether the app asks Windows for a larger main-thread stack, and the
//! `build.rs` that does, which `init` prints when it finds none.
//!
//! Windows gives a program's main thread 1 MiB of stack unless its executable
//! asks for more; Linux and macOS give 8 MiB. A large app's debug build can
//! pass 1 MiB while it builds its first window (Skribisto's peaked near
//! 1.4 MiB) and then ends in a stack overflow on Windows alone. A dependency's
//! link arguments never reach the app's executable, so the app asks for the
//! stack itself, in its own `build.rs`. `init` writes nothing there: it only
//! looks for the setting and prints the snippet when it finds none.

use std::path::Path;

/// A `build.rs` asking Windows for an 8 MiB main-thread stack, for every
/// executable of the package. The app guide shows the same.
pub const SNIPPET: &str = r#"fn main() {
    // Windows gives the main thread 1 MiB of stack; Linux and macOS give 8.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let stack = match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => "/STACK:8388608",
            _ => "-Wl,--stack,8388608",
        };
        println!("cargo:rustc-link-arg-bins={stack}");
    }
}"#;

/// The note `init` prints when the project sets no stack size.
pub fn hint() -> String {
    format!(
        "Note: Windows gives the main thread 1 MiB of stack (Linux and macOS give 8 MiB), \
         and a large app's debug build can overflow it while building its first window. \
         No build.rs or .cargo/config.toml here sets the stack size. To ask for 8 MiB, \
         put this in the app's build.rs (or add its body to the one it has):\n\n{SNIPPET}\n"
    )
}

/// Whether a `build.rs` under `root`, or the project's `.cargo` configuration,
/// already sets the stack size: a linker `/STACK:` (MSVC) or `--stack` (GNU).
pub fn reserves_stack(root: &Path) -> bool {
    let config = root.join(".cargo");
    ["config.toml", "config"]
        .iter()
        .any(|name| file_sets_stack(&config.join(name)))
        || build_script_sets_stack(root, 0)
}

/// Directories deep enough for a workspace's member crates.
const MAX_DEPTH: usize = 4;

fn build_script_sets_stack(dir: &Path, depth: usize) -> bool {
    if file_sets_stack(&dir.join("build.rs")) {
        return true;
    }
    if depth == MAX_DEPTH {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Build output, version control and editor folders hold no build script of the app's.
        let skipped = name.starts_with('.') || name == "target" || name == "node_modules";
        !skipped
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && build_script_sets_stack(&entry.path(), depth + 1)
    })
}

fn file_sets_stack(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .is_ok_and(|text| text.contains("/STACK:") || text.contains("--stack"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_without_the_setting_is_found() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("build.rs"), "fn main() {}").unwrap();
        assert!(!reserves_stack(root.path()));
    }

    #[test]
    fn the_snippet_in_a_members_build_script_counts() {
        let root = tempfile::tempdir().unwrap();
        let member = root.path().join("crates").join("app");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(member.join("build.rs"), SNIPPET).unwrap();
        assert!(reserves_stack(root.path()));
    }

    #[test]
    fn a_stack_set_in_the_cargo_configuration_counts() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".cargo")).unwrap();
        std::fs::write(
            root.path().join(".cargo").join("config.toml"),
            "[target.x86_64-pc-windows-msvc]\nrustflags = [\"-C\", \"link-arg=/STACK:8388608\"]\n",
        )
        .unwrap();
        assert!(reserves_stack(root.path()));
    }

    /// A dependency's build script in the target directory is not the app's.
    #[test]
    fn build_output_is_not_looked_at() {
        let root = tempfile::tempdir().unwrap();
        let built = root.path().join("target").join("debug").join("build");
        std::fs::create_dir_all(&built).unwrap();
        std::fs::write(built.join("build.rs"), SNIPPET).unwrap();
        assert!(!reserves_stack(root.path()));
    }

    #[test]
    fn the_hint_carries_the_snippet() {
        assert!(hint().contains(SNIPPET));
    }
}
