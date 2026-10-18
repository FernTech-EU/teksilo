// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `cargo teksilo update`: bring what this tool installed to this release.
//!
//! Nothing here decides anything new. The probe's state is [`probe::inspect`],
//! an agent copy's is [`setup::inspect`], a companion binary's is
//! [`companion::inspect`], and writing goes through the same functions
//! `probe install`, `agent install` and `cargo install` use. What this module
//! adds is the selection (only what is ours, present and not current) and the
//! consent: an agent copy that differs from this release cannot be told from
//! one somebody edited, so every replacement is listed before it happens.

use std::path::{Path, PathBuf};

use crate::{companion, probe, setup};

/// Which installed tooling one run covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The project's probe harness, agent copies and the companion binaries.
    Project,
    /// The project's agent copies only.
    AgentsOnly,
    /// The agent copies under the home directory, instead of the project's.
    User,
}

impl Scope {
    pub fn from_flags(agent: bool, user: bool) -> Self {
        match (agent, user) {
            (_, true) => Scope::User,
            (true, false) => Scope::AgentsOnly,
            (false, false) => Scope::Project,
        }
    }
}

/// One thing to bring up to date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Probe {
        recorded: Option<String>,
        /// Generated files with local edits, replaced only with consent.
        edited: Vec<String>,
    },
    Agent {
        target: setup::Target,
        id: &'static str,
        user: bool,
    },
    Binary(companion::Found),
}

impl Item {
    /// The name in the closing `Updated:` line.
    pub fn name(&self) -> String {
        match self {
            Item::Probe { .. } => "probe harness".into(),
            Item::Agent {
                target, user: true, ..
            } => format!("user {}", target.agent),
            Item::Agent { target, .. } => target.agent.into(),
            Item::Binary(found) => found.companion.binary.into(),
        }
    }

    /// The listing lines, verb first, aligned like `init`'s.
    pub fn describe(&self, root: &Path) -> Vec<String> {
        match self {
            Item::Probe { recorded, edited } => {
                let mut lines = vec![match recorded {
                    Some(v) => format!("Update   probe harness (from {v})"),
                    None => "Update   probe harness".into(),
                }];
                if !edited.is_empty() {
                    lines.push(format!(
                        "         replaces local edits: {}",
                        edited
                            .iter()
                            .map(|f| format!("scripts/teksilo_probe/{f}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                lines
            }
            Item::Agent { target, user, .. } => vec![format!(
                "Update   {}{} instructions ({})",
                if *user { "user " } else { "" },
                target.agent,
                relative(&target.path, root).display()
            )],
            Item::Binary(found) => vec![format!(
                "Install  {} {} (replaces {})",
                found.companion.binary,
                found.expected,
                found.version().unwrap_or("a copy of unknown version")
            )],
        }
    }

    /// The command that does the same thing by hand.
    pub fn manual(&self) -> String {
        match self {
            Item::Probe { edited, .. } if !edited.is_empty() => {
                "cargo teksilo probe install --force".into()
            }
            Item::Probe { .. } => "cargo teksilo probe install".into(),
            Item::Agent { id, user, .. } => format!(
                "cargo teksilo agent install {id}{} --force",
                if *user { " --user" } else { "" }
            ),
            Item::Binary(found) => found.install().map(|i| i.display()).unwrap_or_default(),
        }
    }

    /// Do it. `consented` is whether the reader accepted this item having seen
    /// its listing, which is what lets a probe file with local edits go.
    pub fn apply(&self, root: &Path, version: &str, consented: bool) -> Result<(), String> {
        match self {
            Item::Probe { .. } => crate::install_probe(root, version, consented),
            Item::Agent { target, .. } => setup::apply(target).map(drop).map_err(|e| e.to_string()),
            Item::Binary(found) => match found.install() {
                Some(install) => install.run(),
                None => Ok(()),
            },
        }
    }

    /// Files before binaries: an install is slow and needs the network, and a
    /// failed one should leave the files done.
    fn order(&self) -> u8 {
        match self {
            Item::Probe { .. } => 0,
            Item::Agent { user: false, .. } => 1,
            Item::Agent { user: true, .. } => 2,
            Item::Binary(_) => 3,
        }
    }
}

/// What a run of `scope` at `root` would change, in the order it changes it.
///
/// `root` is the project, or the home directory for [`Scope::User`].
/// `companions` are only considered for [`Scope::Project`]; they are passed in
/// so a test never depends on what is on its PATH.
pub fn plan(
    root: &Path,
    scope: Scope,
    version: &str,
    companions: Vec<companion::Found>,
) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    if scope == Scope::Project
        && let probe::ProbeState::Stale { recorded, edited } = probe::inspect(root, version)
    {
        // Named only when it is another release's: "from 0.19.0" on a 0.19.0
        // harness with an edited file would read as an upgrade that is not.
        let recorded = recorded.filter(|r| r != version);
        items.push(Item::Probe { recorded, edited });
    }
    let user = scope == Scope::User;
    let mut seen: Vec<PathBuf> = Vec::new();
    for agent in setup::AGENTS {
        // An agent with no target in this scope (cursor at user scope) has
        // nothing installed to update.
        let Ok(mut targets) = setup::selected_targets(root, &[*agent], user) else {
            continue;
        };
        let target = targets.remove(0);
        // Vibe and opencode share codex's `AGENTS.md` in a project; the first
        // agent naming a path owns its line.
        if seen.contains(&target.path) {
            continue;
        }
        seen.push(target.path.clone());
        match setup::inspect(&target) {
            setup::Presence::Stale => items.push(Item::Agent {
                target,
                id: agent.id(),
                user,
            }),
            setup::Presence::Blocked(reason) => {
                return Err(format!("{}: {reason}", target.path.display()));
            }
            setup::Presence::Current | setup::Presence::Absent => {}
        }
    }
    if scope == Scope::Project {
        items.extend(
            companions
                .into_iter()
                .filter(|found| found.install().is_some())
                .map(Item::Binary),
        );
    }
    items.sort_by_key(Item::order);
    Ok(items)
}

fn relative<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(
            t.path().join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        t
    }

    fn target(root: &Path, agent: setup::Agent, user: bool) -> setup::Target {
        setup::selected_targets(root, &[agent], user)
            .unwrap()
            .remove(0)
    }

    fn found(binary: companion::Companion, version: Option<&str>) -> companion::Found {
        companion::inspect(
            binary,
            "0.19.1",
            || version.map(|_| PathBuf::from("/nowhere/bin-x")),
            |_| version.map(String::from),
        )
    }

    fn names(items: &[Item]) -> Vec<String> {
        items.iter().map(Item::name).collect()
    }

    #[test]
    fn nothing_installed_plans_nothing() {
        let p = project();
        assert!(
            plan(p.path(), Scope::Project, "0.19.1", vec![])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_current_install_plans_nothing() {
        let p = project();
        setup::apply(&target(p.path(), setup::Agent::Claude, false)).unwrap();
        crate::install_probe(p.path(), "0.19.1", false).unwrap();
        let companions = vec![
            found(companion::MCP, Some("0.19.1")),
            found(companion::FMT, None),
        ];
        assert!(
            plan(p.path(), Scope::Project, "0.19.1", companions)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_edited_skill_is_planned_and_applying_it_clears_the_plan() {
        let p = project();
        let claude = target(p.path(), setup::Agent::Claude, false);
        setup::apply(&claude).unwrap();
        std::fs::write(claude.path.join("SKILL.md"), "an earlier release's skill").unwrap();

        let items = plan(p.path(), Scope::Project, "0.19.1", vec![]).unwrap();
        assert_eq!(names(&items), ["Claude Code"]);
        assert_eq!(
            items[0].manual(),
            "cargo teksilo agent install claude --force"
        );
        assert_eq!(
            items[0].describe(p.path()),
            ["Update   Claude Code instructions (.claude/skills/teksilo)"]
        );
        items[0].apply(p.path(), "0.19.1", false).unwrap();
        assert!(
            plan(p.path(), Scope::Project, "0.19.1", vec![])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn agents_sharing_agents_md_are_one_item() {
        let p = project();
        let codex = target(p.path(), setup::Agent::Codex, false);
        setup::apply(&codex).unwrap();
        let text = std::fs::read_to_string(&codex.path).unwrap();
        std::fs::write(
            &codex.path,
            text.replace("## Commands", "## Older commands"),
        )
        .unwrap();
        let items = plan(p.path(), Scope::Project, "0.19.1", vec![]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].manual(),
            "cargo teksilo agent install codex --force"
        );
    }

    #[test]
    fn a_probe_recorded_for_another_release_is_planned() {
        let p = project();
        crate::install_probe(p.path(), "0.19.0", false).unwrap();
        let items = plan(p.path(), Scope::Project, "0.19.1", vec![]).unwrap();
        assert_eq!(
            items,
            [Item::Probe {
                recorded: Some("0.19.0".into()),
                edited: vec![],
            }]
        );
        assert_eq!(
            items[0].describe(p.path()),
            ["Update   probe harness (from 0.19.0)"]
        );
    }

    #[test]
    fn an_edited_probe_file_needs_consent() {
        let p = project();
        crate::install_probe(p.path(), "0.19.1", false).unwrap();
        let file = p.path().join("scripts/teksilo_probe/session.py");
        std::fs::write(&file, "# mine\n").unwrap();

        let items = plan(p.path(), Scope::Project, "0.19.1", vec![]).unwrap();
        assert_eq!(items[0].manual(), "cargo teksilo probe install --force");
        assert_eq!(
            items[0].describe(p.path())[1],
            "         replaces local edits: scripts/teksilo_probe/session.py"
        );
        assert!(items[0].apply(p.path(), "0.19.1", false).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# mine\n");
        items[0].apply(p.path(), "0.19.1", true).unwrap();
        assert_ne!(std::fs::read_to_string(&file).unwrap(), "# mine\n");
    }

    #[test]
    fn only_companions_that_need_it_are_installed() {
        let p = project();
        let items = plan(
            p.path(),
            Scope::Project,
            "0.19.1",
            vec![
                found(companion::MCP, Some("0.19.0")),
                found(companion::FMT, Some("0.19.2")),
            ],
        )
        .unwrap();
        assert_eq!(names(&items), ["teksilo-automation-mcp"]);
        assert_eq!(
            items[0].describe(p.path()),
            ["Install  teksilo-automation-mcp 0.19.1 (replaces 0.19.0)"]
        );
        let items = plan(
            p.path(),
            Scope::Project,
            "0.19.1",
            vec![
                found(companion::MCP, None),
                found(companion::FMT, Some("0.18.4")),
            ],
        )
        .unwrap();
        assert_eq!(names(&items), ["cargo-teksilo-fmt"]);
    }

    #[test]
    fn agents_only_leaves_the_probe_and_the_binaries() {
        let p = project();
        crate::install_probe(p.path(), "0.19.0", false).unwrap();
        let claude = target(p.path(), setup::Agent::Claude, false);
        setup::apply(&claude).unwrap();
        std::fs::write(claude.path.join("SKILL.md"), "older").unwrap();
        let items = plan(
            p.path(),
            Scope::AgentsOnly,
            "0.19.1",
            vec![found(companion::MCP, Some("0.19.0"))],
        )
        .unwrap();
        assert_eq!(names(&items), ["Claude Code"]);
    }

    #[test]
    fn user_scope_covers_only_the_home_copies() {
        // The home directory doubles as a project here, with a stale probe and
        // a stale project-form agent, to show neither is picked up.
        let home = project();
        crate::install_probe(home.path(), "0.19.0", false).unwrap();
        let claude = target(home.path(), setup::Agent::Claude, true);
        setup::apply(&claude).unwrap();
        std::fs::write(claude.path.join("SKILL.md"), "older").unwrap();
        let mut items = plan(
            home.path(),
            Scope::User,
            "0.19.1",
            vec![found(companion::MCP, Some("0.19.0"))],
        )
        .unwrap();
        // Only Claude's row: vibe and opencode follow `VIBE_HOME` and
        // `XDG_CONFIG_HOME`, which a test cannot keep out of the temp home.
        items.retain(|i| {
            matches!(i, Item::Agent { id: "claude", .. }) || !matches!(i, Item::Agent { .. })
        });
        assert_eq!(names(&items), ["user Claude Code"]);
        assert_eq!(
            items[0].manual(),
            "cargo teksilo agent install claude --user --force"
        );
    }

    #[test]
    fn flags_select_the_scope() {
        assert_eq!(Scope::from_flags(false, false), Scope::Project);
        assert_eq!(Scope::from_flags(true, false), Scope::AgentsOnly);
        assert_eq!(Scope::from_flags(false, true), Scope::User);
        assert_eq!(Scope::from_flags(true, true), Scope::User);
    }
}
