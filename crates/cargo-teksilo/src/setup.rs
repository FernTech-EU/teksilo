// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One command that makes an agent effective in this app.
//!
//! Installs the teksilo skill where the agents on this machine will find it,
//! writes the probe harness into the project, and records what teksilo both
//! were matched to.
//!
//! ## Why detection is conservative
//!
//! Only directories that already exist are treated as evidence that an agent
//! is installed. Creating `~/.some-vendor/skills/` on the chance that a tool
//! might read it litters a home directory with guesses, and a skill installed
//! where nothing looks for it is indistinguishable from no skill at all —
//! except that it reports success. So this reports exactly what it found and
//! exactly where it wrote, and says so when it found nothing.

use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

/// The merged skill, embedded at build time.
///
/// A copy of `.claude/skills/teksilo/`; CI enforces that they stay identical,
/// which is what replaced the hand-run `cp` the previous skill depended on.
static SKILL: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/embedded/skill");

/// The skill's directory name wherever it is installed.
const SKILL_NAME: &str = "teksilo";

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("could not install the skill: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Probe(#[from] crate::probe::ProbeError),
}

/// A place an agent looks for skills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillHome {
    /// How to describe it to the user.
    pub label: &'static str,
    /// The `skills/` directory itself.
    pub dir: PathBuf,
}

/// Where the skill was installed, for reporting.
#[derive(Debug, Default)]
pub struct Installed {
    pub homes: Vec<(String, usize)>,
}

/// Find the skill directories that already exist on this machine.
///
/// `project` first: a skill beside the code is versioned with it and travels
/// with the repository, which is what a team wants. The per-user directory is
/// also offered because a solo developer usually has only that.
pub fn skill_homes(project: &Path, home: Option<&Path>) -> Vec<SkillHome> {
    let mut found = Vec::new();

    let project_claude = project.join(".claude");
    if project_claude.is_dir() {
        found.push(SkillHome {
            label: "project (.claude/skills)",
            dir: project_claude.join("skills"),
        });
    }

    if let Some(h) = home {
        let user_claude = h.join(".claude");
        if user_claude.is_dir() {
            found.push(SkillHome {
                label: "user (~/.claude/skills)",
                dir: user_claude.join("skills"),
            });
        }
    }

    found
}

/// Install the skill into every home found, returning what was written.
pub fn install_skill(homes: &[SkillHome]) -> Result<Installed, SetupError> {
    let mut installed = Installed::default();
    for home in homes {
        let dest = home.dir.join(SKILL_NAME);
        // A skill is a whole replaced unit: a file dropped between releases
        // must not survive to shadow what the new one says.
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        let count = write_dir(&SKILL, &dest)?;
        installed
            .homes
            .push((format!("{} → {}", home.label, dest.display()), count));
    }
    Ok(installed)
}

fn write_dir(dir: &Dir<'_>, dest: &Path) -> Result<usize, SetupError> {
    std::fs::create_dir_all(dest)?;
    let mut count = 0;
    for file in dir.files() {
        let target = dest.join(file.path().file_name().unwrap_or_default());
        std::fs::write(target, file.contents())?;
        count += 1;
    }
    for sub in dir.dirs() {
        let name = sub.path().file_name().unwrap_or_default();
        count += write_dir(sub, &dest.join(name))?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_claude_directory_is_found() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        let homes = skill_homes(t.path(), None);
        assert_eq!(homes.len(), 1);
        assert_eq!(homes[0].dir, t.path().join(".claude/skills"));
    }

    #[test]
    fn a_missing_directory_is_not_invented() {
        // Installing where nothing looks is indistinguishable from not
        // installing, except that it reports success.
        let t = tempfile::tempdir().unwrap();
        assert!(skill_homes(t.path(), Some(t.path())).is_empty());
        assert!(
            !t.path().join(".claude").exists(),
            "must not create anything"
        );
    }

    #[test]
    fn both_project_and_user_homes_are_found_project_first() {
        let proj = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proj.path().join(".claude")).unwrap();
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        let homes = skill_homes(proj.path(), Some(home.path()));
        assert_eq!(homes.len(), 2);
        assert!(homes[0].label.starts_with("project"));
        assert!(homes[1].label.starts_with("user"));
    }

    #[test]
    fn installing_writes_the_whole_skill_tree() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        let homes = skill_homes(t.path(), None);
        let done = install_skill(&homes).unwrap();

        let root = t.path().join(".claude/skills/teksilo");
        assert!(root.join("SKILL.md").is_file());
        assert!(root.join("reference/teksu.md").is_file());
        assert!(root.join("reference/automation.md").is_file());
        assert!(root.join("reference/teksilo_app_guide.md").is_file());
        assert_eq!(done.homes.len(), 1);
        assert!(done.homes[0].1 >= 4);
    }

    #[test]
    fn reinstalling_removes_a_file_the_previous_version_shipped() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        let homes = skill_homes(t.path(), None);
        install_skill(&homes).unwrap();

        let stale = t.path().join(".claude/skills/teksilo/reference/gone.md");
        std::fs::write(&stale, b"from an older release").unwrap();
        install_skill(&homes).unwrap();
        assert!(
            !stale.exists(),
            "a stale file must not shadow the new skill"
        );
    }

    #[test]
    fn the_embedded_skill_is_the_real_one() {
        let skill = SKILL
            .get_file("SKILL.md")
            .expect("SKILL.md must be embedded");
        let text = std::str::from_utf8(skill.contents()).unwrap();
        assert!(text.contains("name: teksilo"));
        assert!(text.contains("user_invocable: true"));
        assert!(text.len() > 5_000, "suspiciously small skill");
    }
}
