// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::{
    path::Path,
    process::{Command, Output},
};

fn project() -> tempfile::TempDir {
    project_at(env!("CARGO_PKG_VERSION"))
}
fn project_at(version: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"teksilo\"\nversion = \"{version}\"\nedition = \"2021\"\n[workspace]\n"
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
fn a_teksilo_behind_a_feature_is_still_found() {
    let teksilo = project();
    let app = tempfile::tempdir().unwrap();
    std::fs::create_dir(app.path().join("src")).unwrap();
    std::fs::write(app.path().join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(
        app.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n\
             [dependencies]\nteksilo = {{ path = {:?}, optional = true }}\n\
             [features]\nui = [\"dep:teksilo\"]\n",
            teksilo.path()
        ),
    )
    .unwrap();
    let result = run(
        app.path(),
        &["search", "scroll", "--json", "--quiet", "--limit", "1"],
    );
    ok(&result);
    let result = run(app.path(), &["status", "--json"]);
    ok(&result);
    let data: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(data["teksilo_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(data["teksilo_enabled_by"], serde_json::json!(["ui"]));
}
#[test]
fn status_says_why_it_has_no_version() {
    let root = project();
    let result = run(root.path(), &["status"]);
    ok(&result);
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("Teksilo  unknown (no Cargo.lock yet, and status does not create one)")
    );
    assert!(!root.path().join("Cargo.lock").exists());
    ok(&run(
        root.path(),
        &["search", "scroll", "--quiet", "--limit", "1"],
    ));
    let result = run(root.path(), &["status"]);
    let expected = format!("Teksilo  {}\n", env!("CARGO_PKG_VERSION"));
    assert!(String::from_utf8_lossy(&result.stdout).contains(&expected));
}
#[cfg(unix)]
#[test]
fn status_reports_the_automation_server_ahead_on_the_path() {
    use std::os::unix::fs::PermissionsExt;
    let root = project();
    let bin = tempfile::tempdir().unwrap();
    let server = bin.path().join("teksilo-automation-mcp");
    std::fs::write(&server, "#!/bin/sh\necho 'teksilo-automation-mcp 0.1.0'\n").unwrap();
    std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(bin.path().to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_cargo-teksilo"))
        .args(["teksilo", "status", "--json"])
        .current_dir(root.path())
        .env("PATH", &path)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap();
    ok(&result);
    let data: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(data["automation_server"]["state"], "mismatch");
    assert_eq!(data["automation_server"]["version"], "0.1.0");
    assert_eq!(
        data["automation_server"]["expected"],
        env!("CARGO_PKG_VERSION")
    );
}
#[test]
fn quiet_still_says_which_release_answered() {
    let root = project();
    let result = run(
        root.path(),
        &["search", "scroll", "--quiet", "--limit", "1"],
    );
    ok(&result);
    let banner = format!("Teksilo {} · lexical", env!("CARGO_PKG_VERSION"));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).lines().next(),
        Some(banner.as_str())
    );

    let tool = env!("CARGO_PKG_VERSION");
    let (minor, patch) = tool.rsplit_once('.').unwrap();
    let app = format!("{minor}.{}", patch.parse::<u32>().unwrap() + 1);
    let root = project_at(&app);
    let result = run(root.path(), &["show", "docs/agent-tooling.md", "--quiet"]);
    ok(&result);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains(&format!("app uses Teksilo {app}; tool uses {tool}")));
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

/// Run with fake companion binaries ahead on PATH, each reporting `version`,
/// so `update` never reaches the real ones and never runs `cargo install`.
#[cfg(unix)]
fn run_with_companions(root: &Path, version: &str, args: &[&str]) -> Output {
    use std::os::unix::fs::PermissionsExt;
    let bin = root.join("fake-bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["teksilo-automation-mcp", "cargo-teksilo-fmt"] {
        let script = bin.join(name);
        std::fs::write(&script, format!("#!/bin/sh\necho '{name} {version}'\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_cargo-teksilo"))
        .arg("teksilo")
        .args(args)
        .current_dir(root)
        .env("PATH", path)
        .env("CARGO_NET_OFFLINE", "true")
        .env("FASTEMBED_CACHE_DIR", root.join("model-cache"))
        .output()
        .unwrap()
}
#[cfg(unix)]
#[test]
fn update_refreshes_what_is_installed_and_nothing_else() {
    let root = project();
    let tool = env!("CARGO_PKG_VERSION");
    ok(&run(
        root.path(),
        &["agent", "install", "claude", "--quiet"],
    ));
    let skill = root.path().join(".claude/skills/teksilo/SKILL.md");
    let shipped = std::fs::read_to_string(&skill).unwrap();
    std::fs::write(&skill, "an earlier release's skill").unwrap();

    let listing = "Update   Claude Code instructions (.claude/skills/teksilo)";
    let untouched = || {
        assert_eq!(
            std::fs::read_to_string(&skill).unwrap(),
            "an earlier release's skill"
        )
    };
    // Nobody to ask: the listing, then an error naming the flag, never a wait.
    let result = run_with_companions(root.path(), tool, &["update"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("pass --yes once they agree"));
    assert!(String::from_utf8_lossy(&result.stdout).contains(listing));
    untouched();
    // `-i` is only questions, so it refuses before printing any.
    let result = run_with_companions(root.path(), tool, &["update", "-i"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("update --dry-run"));
    assert!(result.stdout.is_empty());
    untouched();
    // A dry run lists and succeeds.
    let result = run_with_companions(root.path(), tool, &["update", "--dry-run"]);
    ok(&result);
    assert!(String::from_utf8_lossy(&result.stdout).contains(listing));
    untouched();

    let result = run_with_companions(root.path(), tool, &["update", "-y"]);
    ok(&result);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("Updated: Claude Code."));
    assert!(stdout.contains("help: restart the agent session"));
    assert!(!stdout.contains("MCP server"));
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), shipped);
    // Only what was there: no harness, no other agent.
    assert!(!root.path().join("scripts").exists());
    assert!(!root.path().join("AGENTS.md").exists());

    let manifest = std::fs::read_to_string(root.path().join("Cargo.toml")).unwrap();
    let result = run_with_companions(root.path(), tool, &["update", "-y"]);
    ok(&result);
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "Nothing to update.\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("Cargo.toml")).unwrap(),
        manifest
    );
}
#[cfg(unix)]
#[test]
fn status_names_the_command_for_an_older_formatter() {
    // The app one patch ahead, the fakes at this release: one patch older.
    let tool = env!("CARGO_PKG_VERSION");
    let (minor, patch) = tool.rsplit_once('.').unwrap();
    let app = format!("{minor}.{}", patch.parse::<u32>().unwrap() + 1);
    let root = project_at(&app);
    ok(&run(
        root.path(),
        &["show", "docs/agent-tooling.md", "--quiet"],
    )); // writes Cargo.lock
    let result = run_with_companions(root.path(), tool, &["status"]);
    ok(&result);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains(&format!("Fmt      {tool} (older than {app})")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("MCP      {tool} (older than {app})")),
        "{stdout}"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    for name in ["cargo-teksilo-fmt", "teksilo-automation-mcp"] {
        assert!(
            stderr.contains(&format!("`cargo install {name} --version {app} --locked`")),
            "{stderr}"
        );
    }
}
#[test]
fn an_older_tool_says_so_and_does_not_update() {
    let tool = env!("CARGO_PKG_VERSION");
    let (minor, patch) = tool.rsplit_once('.').unwrap();
    let app = format!("{minor}.{}", patch.parse::<u32>().unwrap() + 1);
    let root = project_at(&app);
    ok(&run(
        root.path(),
        &["agent", "install", "claude", "--quiet"],
    ));
    let skill = root.path().join(".claude/skills/teksilo/SKILL.md");
    std::fs::write(&skill, "a newer release's skill").unwrap();

    let result = run(root.path(), &["status", "--quiet"]);
    ok(&result);
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains(&format!("Tool     {tool} (older than {app})"))
    );
    let help = format!("`cargo install cargo-teksilo --version {app} --locked`");
    assert!(String::from_utf8_lossy(&result.stderr).contains(&help));

    let result = run(root.path(), &["update", "-y"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains(&help));
    assert_eq!(
        std::fs::read_to_string(&skill).unwrap(),
        "a newer release's skill"
    );
}
#[test]
fn update_user_refreshes_home_copies_outside_any_project() {
    let outside = tempfile::tempdir().unwrap();
    let home = outside.path().join("home");
    let user_run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_cargo-teksilo"))
            .arg("teksilo")
            .args(args)
            .current_dir(outside.path())
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("VIBE_HOME", home.join("vibe"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .output()
            .unwrap()
    };
    ok(&user_run(&[
        "agent", "install", "claude", "--user", "--quiet",
    ]));
    let skill = home.join(".claude/skills/teksilo/SKILL.md");
    let shipped = std::fs::read_to_string(&skill).unwrap();
    std::fs::write(&skill, "an earlier release's skill").unwrap();

    let result = user_run(&["update", "--user", "-y"]);
    ok(&result);
    assert!(String::from_utf8_lossy(&result.stdout).contains("Updated: user Claude Code."));
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), shipped);
    assert!(!home.join("vibe").exists());
    assert!(!outside.path().join("Cargo.lock").exists());
}
