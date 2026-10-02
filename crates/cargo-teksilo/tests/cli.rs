// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::{
    path::Path,
    process::{Command, Output},
};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"teksilo\"\nversion = \"{}\"\nedition = \"2021\"\n[workspace]\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    dir
}
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-teksilo"))
        .arg("teksilo")
        .args(args)
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env("FASTEMBED_CACHE_DIR", root.join("model-cache"))
        .output()
        .unwrap()
}
fn ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn explicit_install_creates_markers_and_preserves_shared_content() {
    let root = project();
    std::fs::write(root.path().join("AGENTS.md"), "# Local instructions\n").unwrap();
    let result = run(
        root.path(),
        &[
            "agent", "install", "codex", "claude", "cursor", "cline", "--quiet",
        ],
    );
    ok(&result);
    assert!(result.stdout.is_empty());
    assert!(
        root.path()
            .join(".claude/skills/teksilo/SKILL.md")
            .is_file()
    );
    assert!(root.path().join(".cursor/rules/teksilo.mdc").is_file());
    assert!(root.path().join(".clinerules/teksilo.md").is_file());
    let shared = std::fs::read_to_string(root.path().join("AGENTS.md")).unwrap();
    assert!(shared.starts_with("# Local instructions\n"));
    assert!(shared.contains("cargo teksilo agent install"));
    assert!(!root.path().join("scripts").exists());
    assert!(!root.path().join("model-cache").exists());
    std::fs::write(
        root.path().join("AGENTS.md"),
        shared.replace("## Commands", "## My commands"),
    )
    .unwrap();
    assert!(
        !run(root.path(), &["agent", "install", "codex"])
            .status
            .success()
    );
    ok(&run(root.path(), &["agent", "install", "codex", "--force"]));
    assert!(
        std::fs::read_to_string(root.path().join("AGENTS.md"))
            .unwrap()
            .starts_with("# Local instructions\n")
    );
    ok(&run(root.path(), &["agent", "install", "claude", "cursor"]));
    std::fs::write(root.path().join(".cursor/rules/teksilo.mdc"), "my rules").unwrap();
    assert!(
        !run(root.path(), &["agent", "install", "cursor"])
            .status
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join(".cursor/rules/teksilo.mdc")).unwrap(),
        "my rules"
    );
    ok(&run(
        root.path(),
        &["agent", "install", "cursor", "--force"],
    ));
}
#[test]
fn init_installs_selected_targets_without_model_download() {
    let root = project();
    let result = run(
        root.path(),
        &["init", "--agent", "claude", "--agent", "codex", "-y"],
    );
    ok(&result);
    assert_eq!(String::from_utf8_lossy(&result.stdout).lines().count(), 1);
    assert!(
        root.path()
            .join("scripts/teksilo_probe/session.py")
            .is_file()
    );
    assert!(root.path().join("AGENTS.md").is_file());
    assert!(!root.path().join("model-cache").exists());
    let result = run(root.path(), &["agent", "list", "--json"]);
    ok(&result);
    let data: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(
        data["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["agent"] == "claude" && a["state"] == "installed")
    );
}
#[test]
fn structured_queries_are_payload_only_and_status_is_read_only() {
    let root = project();
    let result = run(root.path(), &["status", "--json"]);
    ok(&result);
    let data: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(data["teksilo_version"].is_null());
    assert!(!root.path().join("Cargo.lock").exists());
    let result = run(
        root.path(),
        &["search", "scroll", "--json", "--quiet", "--limit", "2"],
    );
    ok(&result);
    let data: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(data["results"].as_array().unwrap().len(), 2);
    assert!(data["mode"].as_str().unwrap().contains("lexical"));
    assert!(!root.path().join("model-cache").exists());
    let result = run(root.path(), &["show", "docs/agent-tooling.md", "--quiet"]);
    ok(&result);
    assert!(result.stderr.is_empty());
    assert!(String::from_utf8_lossy(&result.stdout).contains("cargo teksilo agent install"));
}
#[test]
fn obsolete_commands_are_removed_and_version_is_available() {
    let root = project();
    for args in [
        vec!["setup"],
        vec!["version"],
        vec!["probe"],
        vec!["init", "--no-model"],
    ] {
        assert!(!run(root.path(), &args).status.success());
    }
    ok(&run(root.path(), &["--version"]));
    let result = run(root.path(), &["init"]);
    assert!(!result.status.success());
    assert!(!root.path().join("scripts").exists());
}

#[test]
fn user_install_creates_missing_configuration_without_project_writes() {
    let root = project();
    let home = root.path().join("home");
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-teksilo"))
        .args([
            "teksilo", "agent", "install", "claude", "vibe", "opencode", "--user", "--quiet",
        ])
        .current_dir(root.path())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("VIBE_HOME", home.join("vibe"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .output()
        .unwrap();
    ok(&output);
    assert!(output.stdout.is_empty());
    assert!(home.join(".claude/skills/teksilo/SKILL.md").is_file());
    assert!(home.join("vibe/AGENTS.md").is_file());
    assert!(home.join("config/opencode/AGENTS.md").is_file());
    assert!(!root.path().join("scripts").exists());
    assert!(!root.path().join("Cargo.lock").exists());
}
