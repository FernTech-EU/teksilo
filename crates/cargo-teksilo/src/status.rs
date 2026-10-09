// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use crate::{guard, resolve, setup, symbol, vectors};
use std::path::Path;

/// Machine-readable agent status uses the same target selection as installation.
pub fn agent_data(root: &Path, user: bool) -> Vec<serde_json::Value> {
    setup::AGENTS
        .iter()
        .map(
            |agent| match setup::selected_targets(root, &[*agent], user) {
                Ok(targets) => {
                    let target = &targets[0];
                    let state = match setup::inspect(target) {
                        setup::Presence::Current => "installed",
                        setup::Presence::Stale => "modified",
                        setup::Presence::Absent => "missing",
                        setup::Presence::Blocked(_) => "blocked",
                    };
                    serde_json::json!({"agent":agent.id(), "state":state, "path":target.path})
                }
                Err(_) => {
                    serde_json::json!({"agent":agent.id(), "state":"unsupported", "path":null})
                }
            },
        )
        .collect()
}
pub fn agents(root: &Path, user: bool) {
    let rows = agent_data(root, user);
    if crate::output::json() {
        println!(
            "{}",
            serde_json::json!({"scope":if user { "user" } else { "project" }, "agents":rows})
        );
        return;
    }
    for row in rows {
        println!(
            "{:<12} {}",
            row["agent"].as_str().unwrap(),
            row["state"].as_str().unwrap()
        );
        if crate::output::verbose()
            && let Some(path) = row["path"].as_str()
        {
            eprintln!("  {path}");
        }
    }
}
pub fn report(dir: &Path) {
    let root = setup::find_project_root(dir);
    let resolution = root.as_ref().map(|r| resolve::resolve_locked(r));
    let version = resolution
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .map(|r| r.version.as_str());
    let enabled_by = resolution
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|r| r.enabled_by.as_deref());
    let failure = resolution.as_ref().and_then(|r| r.as_ref().err());
    let error = failure.map(ToString::to_string);
    let rows = root
        .as_ref()
        .map(|r| agent_data(r, false))
        .unwrap_or_default();
    // A copy under the home directory is read in every project, so one that an
    // earlier release installed goes on advising every app on the machine, and
    // the project rows above cannot show it.
    let user_rows = setup::home_dir()
        .map(|home| agent_data(&home, true))
        .unwrap_or_default();
    let model = if vectors::encoder_cache_dir().is_none() {
        "unavailable"
    } else if vectors::encoder_is_cached() {
        "cached"
    } else {
        "missing"
    };
    let python = symbol::find_python();
    let probe = root
        .as_ref()
        .and_then(|r| crate::probe::recorded_provenance(r));
    let compatible = version.map(|v| guard::check(v).may_answer());
    if crate::output::json() {
        println!(
            "{}",
            serde_json::json!({"tool_version":guard::TOOL_VERSION,"project":root,"teksilo_version":version,"teksilo_enabled_by":enabled_by,"compatible":compatible,"resolution_error":error,"agents":rows,"user_agents":user_rows,"model":model,"python":python,"probe_version":probe})
        );
        return;
    }
    println!(
        "Project  {}",
        root.as_ref()
            .map(|r| r.display().to_string())
            .unwrap_or_else(|| "none".into())
    );
    let unresolved = match failure {
        Some(resolve::ResolveError::NoLockfile) => {
            " (no Cargo.lock yet, and status does not create one)"
        }
        Some(resolve::ResolveError::NotADependency) => " (not a dependency of this project)",
        Some(_) => " (run with --verbose to see why)",
        None => "",
    };
    println!(
        "Teksilo  {}{}{}{}",
        version.unwrap_or("unknown"),
        unresolved,
        enabled_by.map(optional_suffix).unwrap_or_default(),
        if compatible == Some(false) {
            " (incompatible)"
        } else {
            ""
        }
    );
    if matches!(failure, Some(resolve::ResolveError::NoLockfile)) {
        crate::output::note(
            "help: any other cargo teksilo command, or `cargo generate-lockfile`, creates it",
        );
    }
    println!("Probe    {}", probe.as_deref().unwrap_or("missing"));
    println!("Model    {model}");
    println!(
        "Python   {}",
        if python.is_some() {
            "available"
        } else {
            "missing"
        }
    );
    let mut paths = std::collections::BTreeSet::new();
    let installed: Vec<_> = rows
        .iter()
        .filter(|r| r["state"] == "installed" && paths.insert(r["path"].as_str()))
        .filter_map(|r| r["agent"].as_str())
        .collect();
    println!(
        "Agents   {}",
        if installed.is_empty() {
            "none".into()
        } else {
            installed.join(", ")
        }
    );
    let (user, differing) = user_summary(&user_rows);
    println!("User     {user}");
    for agent in differing {
        crate::output::note(format!(
            "help: the {agent} instructions under your home directory differ from \
             this release's; `cargo teksilo agent install {agent} --user --force` \
             replaces them, local edits included"
        ));
    }
    if crate::output::verbose() {
        if let Some(error) = error {
            eprintln!("Resolution: {error}");
        }
        if let Some(root) = root {
            agents(&root, false);
        }
        if let Some(path) = python {
            eprintln!("Python: {}", path.display());
        }
    }
}

/// How the `Teksilo` line says that the app's default features leave teksilo
/// off, so that "0.15.1" is not read as what a plain `cargo build` links.
fn optional_suffix(features: &[String]) -> String {
    match features {
        [] => " (optional)".into(),
        [one] => format!(" (optional, feature `{one}`)"),
        many => format!(
            " (optional, features {})",
            many.iter()
                .map(|f| format!("`{f}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The report's `User` line, and the agents whose user-scope copy differs from
/// the one this release installs.
///
/// A differing copy is listed rather than left out, as the project line does,
/// because at user scope it is the likelier case: nothing refreshes it when an
/// app moves to a newer release.
fn user_summary(rows: &[serde_json::Value]) -> (String, Vec<&str>) {
    let mut paths = std::collections::BTreeSet::new();
    let mut shown = Vec::new();
    let mut differing = Vec::new();
    for row in rows {
        let (Some(agent), Some(state)) = (row["agent"].as_str(), row["state"].as_str()) else {
            continue;
        };
        if !matches!(state, "installed" | "modified") || !paths.insert(row["path"].as_str()) {
            continue;
        }
        if state == "modified" {
            shown.push(format!("{agent} (differs from {})", guard::TOOL_VERSION));
            differing.push(agent);
        } else {
            shown.push(agent.to_string());
        }
    }
    let line = if shown.is_empty() {
        "none".to_string()
    } else {
        shown.join(", ")
    };
    (line, differing)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agent_status_tracks_explicit_installation_without_markers() {
        let root = tempfile::tempdir().unwrap();
        let before = agent_data(root.path(), false);
        assert!(before.iter().all(|r| r["state"] == "missing"));
        let target = setup::selected_targets(root.path(), &[setup::Agent::Claude], false)
            .unwrap()
            .remove(0);
        setup::apply(&target).unwrap();
        let after = agent_data(root.path(), false);
        assert_eq!(
            after.iter().find(|r| r["agent"] == "claude").unwrap()["state"],
            "installed"
        );
        assert_eq!(
            after.iter().find(|r| r["agent"] == "cursor").unwrap()["state"],
            "missing"
        );
    }
    #[test]
    fn a_user_scope_copy_that_differs_from_this_release_is_flagged() {
        let home = tempfile::tempdir().unwrap();
        // Only Claude's row: the other two user agents follow `VIBE_HOME` and
        // `XDG_CONFIG_HOME`, which a test cannot keep out of the temp home.
        let claude = || {
            let mut rows = agent_data(home.path(), true);
            rows.retain(|r| r["agent"] == "claude");
            rows
        };
        assert_eq!(user_summary(&claude()).0, "none");

        let target = setup::selected_targets(home.path(), &[setup::Agent::Claude], true)
            .unwrap()
            .remove(0);
        setup::apply(&target).unwrap();
        let rows = claude();
        let (line, differing) = user_summary(&rows);
        assert_eq!(line, "claude");
        assert!(differing.is_empty());

        std::fs::write(target.path.join("SKILL.md"), "an earlier release's skill").unwrap();
        let rows = claude();
        let (line, differing) = user_summary(&rows);
        assert_eq!(
            line,
            format!("claude (differs from {})", guard::TOOL_VERSION)
        );
        assert_eq!(differing, ["claude"]);
    }
    #[test]
    fn user_status_exposes_unsupported_targets() {
        let root = tempfile::tempdir().unwrap();
        let rows = agent_data(root.path(), true);
        assert_eq!(
            rows.iter().find(|r| r["agent"] == "cursor").unwrap()["state"],
            "unsupported"
        );
    }
}
