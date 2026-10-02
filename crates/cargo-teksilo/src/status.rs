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
    let error = resolution
        .as_ref()
        .and_then(|r| r.as_ref().err())
        .map(ToString::to_string);
    let rows = root
        .as_ref()
        .map(|r| agent_data(r, false))
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
            serde_json::json!({"tool_version":guard::TOOL_VERSION,"project":root,"teksilo_version":version,"compatible":compatible,"resolution_error":error,"agents":rows,"model":model,"python":python,"probe_version":probe})
        );
        return;
    }
    println!(
        "Project  {}",
        root.as_ref()
            .map(|r| r.display().to_string())
            .unwrap_or_else(|| "none".into())
    );
    println!(
        "Teksilo  {}{}",
        version.unwrap_or("unknown"),
        if compatible == Some(false) {
            " (incompatible)"
        } else {
            ""
        }
    );
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
    fn user_status_exposes_unsupported_targets() {
        let root = tempfile::tempdir().unwrap();
        let rows = agent_data(root.path(), true);
        assert_eq!(
            rows.iter().find(|r| r["agent"] == "cursor").unwrap()["state"],
            "unsupported"
        );
    }
}
