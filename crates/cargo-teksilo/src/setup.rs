// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One command that makes the agents configured in an app effective in it.
//!
//! Writes the probe harness, hands every detected agent the teksilo briefing
//! in **that agent's own format**, and warms the search encoder's cache so the
//! first `search` does not stall on a download.
//!
//! ## Three rules this module exists to keep
//!
//! **A project-scoped command does not write `$HOME`.** The previous version
//! did: run in an app with no `.claude/`, it silently installed into
//! `~/.claude/skills/teksilo` — a machine-wide change nobody asked for, from a
//! command whose name says "set this project up". Only [`Scope::User`], which
//! is only ever reached through `--user` or an explicit answer to an explicit
//! question, resolves paths under the home directory.
//!
//! **Detection is conservative.** Only directories that already exist count as
//! evidence that an agent is configured here. Creating `.cursor/` on the
//! chance that someone might use Cursor litters a repository with guesses, and
//! instructions installed where nothing reads them are indistinguishable from
//! no instructions at all — except that they report success.
//!
//! **These tools do not share a format.** The skill is four Markdown files
//! under a directory with YAML frontmatter Claude Code's loader understands;
//! copying that tree into `.cursor/` accomplishes exactly nothing. So: the
//! full skill where it is native, and a self-contained condensed brief
//! everywhere else, wearing whatever frontmatter that vendor documents. The
//! brief never refers to the skill, because on a machine with no Claude Code
//! the skill is not there to refer to.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

/// The merged skill, embedded at build time.
///
/// A copy of `.claude/skills/teksilo/`; CI enforces that they stay identical,
/// which is what replaced the hand-run `cp` the previous skill depended on.
static SKILL: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/embedded/skill");

/// The skill's directory name wherever it is installed.
const SKILL_NAME: &str = "teksilo";

// ---------------------------------------------------------------------------
// The brief
// ---------------------------------------------------------------------------

/// The condensed briefing, for every agent that cannot load the skill.
///
/// Self-contained on purpose. It is the *only* teksilo instruction those
/// agents will ever see, so it has to carry what teksilo is, what the tool
/// does, and — above all — the one rule that a search hit is read with
/// `cargo teksilo show` and not fetched from GitHub, because `blob/main`
/// tracks `main` and this app pinned something else.
pub const BRIEF: &str = r##"# Teksilo — read this before writing GUI code in this app

This app depends on **Teksilo**, a pure-Rust desktop GUI framework: a retained
widget tree, SwiftUI-style layout negotiation, `Signal`/`Prop` reactivity,
AccessKit accessibility, wgpu rendering.

`cargo teksilo` is a cargo subcommand that answers for the exact Teksilo
version **this app resolved** — read from its dependency graph, never from
whatever is newest. Install it at that version:

```bash
cargo install cargo-teksilo --version <the teksilo version this app pins>
```

It refuses to answer when its own minor or major differs from the app's
Teksilo, and warns when only the patch differs. That is deliberate:
`SplitView` was deleted outright in favour of `Splitter` between two minors,
and a wrong answer reads exactly like a right one.

## The five commands

| Command | What it gives you |
| --- | --- |
| `cargo teksilo symbol <Name>` | The exact public API of a type — its module header, its `pub` items with their `///` docs, and the builder methods from its inherent `impl` blocks. Run it **before** writing against a type; never invent builder methods. |
| `cargo teksilo search "<question>"` | Retrieval over Teksilo's hand-written guides and worked example crates. Neither reaches this app any other way: the guides ship in no crate, and every example is `publish = false`. Reach for it when the question is conceptual. |
| `cargo teksilo show <path>` | One of those documents in full, offline — or just the lines a hit cited (`--lines 166-172`, 1-based and inclusive). `--list` prints every available path. |
| `cargo teksilo probe` | Writes a Python automation harness into `scripts/teksilo_probe/`, for driving the running app through its automation bridge and asserting on what is on screen. |
| `cargo teksilo setup` | The harness, plus this briefing re-installed for every agent configured here. |

## The rule that matters most

**Read a search result with `cargo teksilo show <path>`. Do not fetch it from
GitHub.** The path a hit prints is real in the *framework* repository and
absent from this one, so both tempting moves are wrong: opening it locally
finds nothing, and `blob/main/` — or the published book — serves `main`, which
is a different Teksilo from the one this app pinned. `show` is offline and
version-matched.

## Working in this app

1. **Conceptual question** ("which data model do I want?", "how does
   drag-and-drop escalate to the OS?") → `cargo teksilo search "<question>"`,
   then `cargo teksilo show <path>` to read the hit in full rather than
   working from its snippet.
2. **Before using a type** → `cargo teksilo symbol <Name>`.
3. **Compile:** `cargo check -p <app-crate>`. If it fails twice on the same
   item, re-extract that type's API before a third attempt — the mental model
   is wrong, not the compiler.
4. **Verify behaviour** where it matters: headless widget tests for layout and
   state logic, a probe for "the user clicks this and that happens on screen".

If `cargo teksilo` is not installed, fall back to
`cargo doc -p teksilo-widgets --no-deps --open`, or docs.rs at the pinned
version, and to the compiler. Both are slower and the second is
version-approximate — say so rather than guessing.
"##;

/// Opening delimiter of the region this tool owns inside a shared file.
pub const BEGIN: &str = "<!-- BEGIN teksilo -->";
/// Closing delimiter of that region.
pub const END: &str = "<!-- END teksilo -->";

/// The brief wrapped in its delimiters.
pub fn region_block() -> String {
    format!("{BEGIN}\n{}\n{END}", BRIEF.trim_end())
}

/// Replace the teksilo region in `existing`, or append one.
///
/// Pure, so the property that matters — *nothing outside the markers moves* —
/// is testable without a filesystem. `AGENTS.md` and
/// `.github/copilot-instructions.md` belong to the project, not to this tool:
/// a second run must be a byte-for-byte no-op, and a run after someone edits
/// the rest of the file must leave their edit alone.
pub fn upsert_region(existing: &str, block: &str) -> String {
    if let Some(start) = existing.find(BEGIN)
        && let Some(end_offset) = existing[start..].find(END)
    {
        let end = start + end_offset + END.len();
        let mut out = String::with_capacity(existing.len() + block.len());
        out.push_str(&existing[..start]);
        out.push_str(block);
        out.push_str(&existing[end..]);
        return out;
    }

    // Appending: keep what is there byte for byte and separate the region with
    // one blank line, so it reads as its own section rather than running into
    // whatever paragraph happened to end the file.
    let mut out = existing.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
    out.push_str(block);
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// What gets written, and where
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("could not install the agent instructions: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Probe(#[from] crate::probe::ProbeError),

    #[error(
        "stdin is not a terminal, so there is nobody to answer this prompt.\n\
         Pass {hint} to run non-interactively."
    )]
    NotATerminal { hint: &'static str },

    #[error("could not find a home directory (neither $HOME nor %USERPROFILE% is set)")]
    NoHome,

    #[error(
        "{0} exists but is not valid UTF-8, so its teksilo section cannot be edited \
         without discarding the rest of the file.\n\
         This tool owns only the text between its markers; fix the file's encoding \
         and re-run."
    )]
    NotUtf8(PathBuf),
}

/// Whose configuration is being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// This repository. Never touches `$HOME`.
    Project,
    /// This user's own agent configuration, under the home directory.
    User,
}

/// How one agent's instructions are shaped on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The whole four-file skill directory, replaced as a unit.
    Skill,
    /// A file this tool owns outright: vendor frontmatter, then the brief.
    OwnFile { frontmatter: &'static str },
    /// A delimited region inside a file the project also writes.
    Region,
}

impl Form {
    /// One phrase for a plan line.
    pub fn describe(&self) -> &'static str {
        match self {
            Form::Skill => "the full skill (4 files)",
            Form::OwnFile { .. } => "a rules file (brief)",
            Form::Region => "a `teksilo` section (brief)",
        }
    }
}

/// One agent, detected, with the single path setup will write for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The vendor's own name for itself.
    pub agent: &'static str,
    /// What was found that made this agent count as configured.
    pub marker: &'static str,
    /// The one path this target writes.
    pub path: PathBuf,
    pub form: Form,
}

/// Cursor's documented frontmatter: `description`, `globs`, `alwaysApply`.
///
/// `alwaysApply: false` with a `.rs` glob rather than always-on: this is
/// advice about Rust GUI code, and a rule that fires while editing the CI YAML
/// is the kind of noise that gets a rules file deleted.
const CURSOR_FRONTMATTER: &str = "\
---
description: Teksilo GUI framework — use `cargo teksilo` for version-matched API and docs
globs: **/*.rs
alwaysApply: false
---
";

/// Windsurf's documented frontmatter: `trigger`, `globs`, `description`.
const WINDSURF_FRONTMATTER: &str = "\
---
trigger: glob
globs: **/*.rs
description: Teksilo GUI framework — use `cargo teksilo` for version-matched API and docs
---
";

/// Every project-scope agent this tool knows how to write for.
///
/// `(agent, marker, relative path, form)`. The marker is what must already
/// exist; the path is what gets written. Ordered as the plan prints them.
type Vendor = (&'static str, &'static str, &'static str, Form);

const VENDORS: &[Vendor] = &[
    (
        "Claude Code",
        ".claude/",
        ".claude/skills/teksilo",
        Form::Skill,
    ),
    (
        "Cursor",
        ".cursor/",
        ".cursor/rules/teksilo.mdc",
        Form::OwnFile {
            frontmatter: CURSOR_FRONTMATTER,
        },
    ),
    (
        "Windsurf",
        ".windsurf/",
        ".windsurf/rules/teksilo.md",
        Form::OwnFile {
            frontmatter: WINDSURF_FRONTMATTER,
        },
    ),
    (
        "GitHub Copilot",
        ".github/",
        ".github/copilot-instructions.md",
        Form::Region,
    ),
    ("Codex / AGENTS.md", "AGENTS.md", "AGENTS.md", Form::Region),
];

/// The nearest ancestor of `start` (inclusive) holding a `Cargo.toml`.
///
/// `cargo metadata` walks up on its own, but setup has to answer the question
/// *before* asking cargo anything: with no manifest anywhere there is no
/// project to set up, and the right move is to say so and ask — not to fail,
/// and emphatically not to quietly write the home directory instead.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join("Cargo.toml").is_file() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// One project-scope agent, with the marker that decides whether it is here.
///
/// Every project vendor, found or not, in the table's order — so the install
/// list, the "nothing was written for these" report and `status` all read one
/// list and cannot disagree about either the order or the detection rule.
pub struct ProjectCandidate {
    pub target: Target,
    /// The path whose existence means "this agent is configured here".
    pub marker_path: PathBuf,
}

impl ProjectCandidate {
    /// `exists`, not `is_dir`: one marker (`AGENTS.md`) is a file.
    pub fn detected(&self) -> bool {
        self.marker_path.exists()
    }
}

/// Every project vendor, detected or not.
pub fn project_candidates(root: &Path) -> Vec<ProjectCandidate> {
    VENDORS
        .iter()
        .map(|(agent, marker, path, form)| ProjectCandidate {
            target: Target {
                agent,
                marker,
                path: root.join(path),
                form: *form,
            },
            marker_path: root.join(marker.trim_end_matches('/')),
        })
        .collect()
}

/// The agents configured in this project.
pub fn project_targets(root: &Path) -> Vec<Target> {
    project_candidates(root)
        .into_iter()
        .filter(ProjectCandidate::detected)
        .map(|c| c.target)
        .collect()
}

/// The project-scope agents that were *not* found, for the closing report.
pub fn project_undetected(root: &Path) -> Vec<(&'static str, &'static str)> {
    project_candidates(root)
        .into_iter()
        .filter(|c| !c.detected())
        .map(|c| (c.target.agent, c.target.marker))
        .collect()
}

// ---------------------------------------------------------------------------
// User scope
// ---------------------------------------------------------------------------

/// `$VIBE_HOME`, or `~/.vibe`.
///
/// Vibe relocates its **whole** state directory through `VIBE_HOME`, not a
/// sub-path of it, so honouring the variable is the difference between writing
/// the file Vibe reads and writing a `~/.vibe/` it will never open.
pub fn vibe_home(home: &Path) -> PathBuf {
    match std::env::var_os("VIBE_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home.join(".vibe"),
    }
}

/// `$XDG_CONFIG_HOME/opencode`, or `~/.config/opencode`.
///
/// opencode resolves its config through the XDG base directories, so the
/// override is read here too. A machine where it landed somewhere this does
/// not predict is not a failure mode: the directory is simply not found,
/// nothing is written, and the closing report names the path that was looked
/// at — which is the one thing that makes the miss fixable.
pub fn opencode_config(home: &Path) -> PathBuf {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v).join("opencode"),
        _ => home.join(".config").join("opencode"),
    }
}

/// One user-scope agent, with the directory whose existence is the evidence
/// that this agent is configured on this machine.
pub struct UserCandidate {
    pub target: Target,
    pub dir: PathBuf,
}

impl UserCandidate {
    pub fn detected(&self) -> bool {
        self.dir.is_dir()
    }
}

/// Every user-scope agent this tool knows how to write for, found or not.
///
/// The env-resolved directories are **parameters, not reads**: a test can then
/// pin them to a temporary directory instead of mutating process-wide
/// environment variables, which two tests running concurrently in one process
/// cannot do safely.
pub fn user_candidates(home: &Path, vibe: &Path, opencode: &Path) -> Vec<UserCandidate> {
    vec![
        UserCandidate {
            target: Target {
                agent: "Claude Code",
                marker: "~/.claude/",
                path: home.join(".claude").join("skills").join(SKILL_NAME),
                form: Form::Skill,
            },
            dir: home.join(".claude"),
        },
        // Vibe and opencode both read an `AGENTS.md` at their config root, and
        // that file is the user's own — it may already hold global
        // instructions that have nothing to do with teksilo. Hence `Region`
        // rather than `OwnFile`: this tool owns the bytes between its markers
        // and nothing else in either file.
        UserCandidate {
            target: Target {
                agent: "Mistral Vibe",
                marker: "~/.vibe/",
                path: vibe.join("AGENTS.md"),
                form: Form::Region,
            },
            dir: vibe.to_path_buf(),
        },
        UserCandidate {
            target: Target {
                agent: "opencode",
                marker: "~/.config/opencode/",
                path: opencode.join("AGENTS.md"),
                form: Form::Region,
            },
            dir: opencode.to_path_buf(),
        },
    ]
}

/// The agents configured for this user.
///
/// Same conservative rule as project scope, and for a sharper reason here: the
/// directory must **already exist**. Creating `~/.vibe/` for someone who has
/// never run Vibe writes a config directory for a tool they may not have, in
/// their home rather than in a repository they can throw away.
pub fn user_targets(home: &Path) -> Vec<Target> {
    user_targets_in(home, &vibe_home(home), &opencode_config(home))
}

/// [`user_targets`] against explicitly given directories.
pub fn user_targets_in(home: &Path, vibe: &Path, opencode: &Path) -> Vec<Target> {
    user_candidates(home, vibe, opencode)
        .into_iter()
        .filter(UserCandidate::detected)
        .map(|c| c.target)
        .collect()
}

/// The user-scope agents that were *not* found, and where this looked.
///
/// The path is returned rather than the marker label because for two of the
/// three it is env-dependent: told "no ~/.vibe/", a user with `VIBE_HOME` set
/// would go and look in the wrong place.
pub fn user_undetected(home: &Path) -> Vec<(&'static str, PathBuf)> {
    user_undetected_in(home, &vibe_home(home), &opencode_config(home))
}

/// [`user_undetected`] against explicitly given directories.
pub fn user_undetected_in(
    home: &Path,
    vibe: &Path,
    opencode: &Path,
) -> Vec<(&'static str, PathBuf)> {
    user_candidates(home, vibe, opencode)
        .into_iter()
        .filter(|c| !c.detected())
        .map(|c| (c.target.agent, c.dir))
        .collect()
}

/// The agents with no user-level file this tool writes, and why not.
///
/// Data rather than prose because two commands need it: `setup --user` prints
/// it as a closing note, and `status` renders each entry as an `n/a` row. A
/// second hand-written copy is a second thing to forget.
///
/// Windsurf is the awkward member and is listed deliberately: it *does* keep a
/// global file, and this tool still does not write it — one shared file with
/// no delimiter convention is not somewhere to append unasked. Saying "n/a"
/// without the reason would read as "Windsurf has nothing", which is false.
pub const USER_NOT_APPLICABLE: &[(&str, &str)] = &[
    (
        "Cursor",
        "user rules are edited in Customize → Rules, not stored as a file",
    ),
    (
        "GitHub Copilot",
        "personal instructions live in your github.com settings",
    ),
    (
        "Windsurf",
        "one global file, ~/.codeium/windsurf/memories/global_rules.md — paste the brief there by hand",
    ),
    (
        "Codex / AGENTS.md",
        "a repository-root AGENTS.md is per-repository by definition",
    ),
];

/// Why `--user` reaches three agents and not seven.
///
/// Printed rather than silently skipped, because "Cursor was not installed for
/// you" and "Cursor has nowhere for this to go" are different facts and a user
/// with Cursor open deserves the second one.
pub fn user_scope_note() -> String {
    let mut out = String::from("The rest keep no user-level file this tool can write:\n");
    for (agent, why) in USER_NOT_APPLICABLE {
        out.push_str(&format!("  {agent:<18}{why}.\n"));
    }
    // Counted, not spelled: the last time a number in prose was typed by hand
    // here it went stale the moment the list grew.
    out.push_str(&format!(
        "Run `cargo teksilo setup` inside each project for those {}.",
        USER_NOT_APPLICABLE.len()
    ));
    out
}

/// What a written file is, and is not.
///
/// Shared by `setup` and `status` so the two cannot come to promise different
/// things. Every line either command prints reports a file **on disk**;
/// whether an agent then reads it is that agent's decision, and at least one
/// will not straight away — Grok Build wants the folder trusted first, and any
/// of them skips a gitignored instruction file without saying so.
pub const ACTIVATION_CAVEAT: &str = "\
These are files on disk. An agent picks them up on its own terms —\n\
some ask you to trust the folder first, and a gitignored instruction\n\
file is skipped silently.";

/// Where the home directory is, if the platform will say.
pub fn home_dir() -> Result<PathBuf, SetupError> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or(SetupError::NoHome)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Created,
    Updated,
    Unchanged,
}

impl Change {
    pub fn describe(&self) -> &'static str {
        match self {
            Change::Created => "created",
            Change::Updated => "updated",
            Change::Unchanged => "unchanged",
        }
    }
}

/// What one target's write actually did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub agent: &'static str,
    pub path: PathBuf,
    pub change: Change,
    /// Files written for this target — 4 for the skill, 1 otherwise.
    pub files: usize,
}

/// The whole contents of a file this tool owns outright.
///
/// A function rather than an inline `format!` so [`apply`] and [`inspect`]
/// cannot drift about what "installed and current" means.
fn own_file(frontmatter: &str) -> String {
    format!("{frontmatter}\n{BRIEF}")
}

/// Read a file this tool shares with the project, for [`Form::Region`].
///
/// A missing file is the empty one — `upsert_region` then appends, and
/// `write_if_changed` reports it as created. Every *other* failure is an
/// error, which is the whole point of this function existing.
///
/// It replaced `read_to_string(..).unwrap_or_default()`, which mapped a
/// decode failure onto "the file is empty" and so rewrote a project's
/// `AGENTS.md` down to nothing but our own region. One byte of Latin-1 in a
/// file this tool does not own was enough to destroy it, silently, while
/// reporting success — against a module whose stated promise is that "a run
/// after someone edits the rest of the file must leave their edit alone".
fn read_shared(path: &Path) -> Result<String, SetupError> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| SetupError::NotUtf8(path.to_path_buf())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

/// Write one target, reporting whether anything actually moved.
pub fn apply(target: &Target) -> Result<Outcome, SetupError> {
    let (change, files) = match target.form {
        Form::Skill => install_skill(&target.path)?,
        Form::OwnFile { frontmatter } => {
            (write_if_changed(&target.path, &own_file(frontmatter))?, 1)
        }
        Form::Region => {
            let existing = read_shared(&target.path)?;
            let updated = upsert_region(&existing, &region_block());
            (write_if_changed(&target.path, &updated)?, 1)
        }
    };
    Ok(Outcome {
        agent: target.agent,
        path: target.path.clone(),
        change,
        files,
    })
}

/// Whether this tool's own content is at a target, and whether it is current.
///
/// The contract, which the `inspect_matches_apply` test pins for every form:
/// **`inspect` returns [`Current`](Presence::Current) exactly when `apply`
/// would return [`Change::Unchanged`]**. That equivalence is the whole point
/// of the type — `status` is the dry run of `setup`, and a status that
/// computed "installed" its own way would eventually disagree with the
/// command it claims to predict.
///
/// The other two are not symmetrical with `Change`, and deliberately so:
/// [`Absent`](Presence::Absent) becomes `Created` for a file that does not
/// exist but `Updated` for one that exists without our region. `Presence`
/// describes *our* content; `Change` describes the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Byte-for-byte what this build writes.
    Current,
    /// Ours, but not this build's — an older release wrote it.
    Stale,
    /// Nothing of ours is here.
    Absent,
    /// `apply` would fail here rather than install: the reason, in a phrase.
    ///
    /// Distinct from `Absent` because the two lead somewhere different. An
    /// absent brief is installed by running `setup`; a blocked one is not, and
    /// reporting it as merely missing would send a reader to a command that
    /// will refuse — the same "advice that does nothing" this type exists to
    /// prevent.
    Blocked(&'static str),
}

/// What [`apply`] would find, without writing anything.
///
/// Every arm mirrors the corresponding arm of `apply` and shares its content
/// computation, so the two are one decision expressed twice rather than two
/// decisions that happen to agree today.
pub fn inspect(target: &Target) -> Presence {
    match target.form {
        Form::Skill => inspect_skill(&target.path),
        Form::OwnFile { frontmatter } => match std::fs::read(&target.path) {
            Ok(current) if current == own_file(frontmatter).as_bytes() => Presence::Current,
            // A file at our exclusive path that is not what we write is ours
            // from an older release. Nothing else puts a file there.
            Ok(_) => Presence::Stale,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Presence::Absent,
            // A directory at the path, or no permission: `apply` errors here,
            // so claiming "not installed, run setup" would be false advice.
            Err(_) => Presence::Blocked("cannot be read"),
        },
        Form::Region => {
            let existing = match read_shared(&target.path) {
                Ok(text) => text,
                Err(SetupError::NotUtf8(_)) => return Presence::Blocked("is not valid UTF-8"),
                Err(_) => return Presence::Blocked("cannot be read"),
            };
            // The marker, not the file, is the evidence: this file belongs to
            // the project and usually exists without us having written a word.
            if !existing.contains(BEGIN) {
                return Presence::Absent;
            }
            if existing == upsert_region(&existing, &region_block()) {
                Presence::Current
            } else {
                Presence::Stale
            }
        }
    }
}

/// [`inspect`] for a skill tree: current iff every file matches and no strays.
fn inspect_skill(dest: &Path) -> Presence {
    if !dest.exists() {
        return Presence::Absent;
    }
    let mut wanted = Vec::new();
    collect(&SKILL, Path::new(""), &mut wanted);

    let mut any = false;
    let mut all = true;
    for (relative, bytes) in &wanted {
        match std::fs::read(dest.join(relative)) {
            Ok(current) if current == *bytes => any = true,
            _ => all = false,
        }
    }
    if !any {
        // A directory of this name holding none of our files is not a stale
        // install — it is somebody else's directory, and saying "installed,
        // outdated" about it would be a claim we have no evidence for.
        return Presence::Absent;
    }

    let keep: Vec<&Path> = wanted.iter().map(|(p, _)| p.as_path()).collect();
    if all && !has_strays(dest, dest, &keep) {
        Presence::Current
    } else {
        Presence::Stale
    }
}

/// The read-only twin of [`remove_strays`]: is there a file we would delete?
///
/// Files only, matching `remove_strays`, which prunes an emptied directory but
/// does not count it as a change — so an empty leftover directory must not
/// make this report `Stale` either, or `status` would advertise work that
/// `setup` would then report as `unchanged`.
fn has_strays(root: &Path, dir: &Path, keep: &[&Path]) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        // Cannot look, so cannot claim there is nothing. `Current` means
        // "verified identical"; an unreadable subtree is not that.
        return true;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if has_strays(root, &path, keep) {
                return true;
            }
        } else if let Ok(relative) = path.strip_prefix(root)
            && !is_wanted(relative, keep)
        {
            return true;
        }
    }
    false
}

/// Whether a file found on disk is one the embedded skill ships.
///
/// Case-**insensitive**, which is the safe direction rather than the tidy one.
///
/// The skill is written with `fs::write(dest.join("SKILL.md"))` and audited
/// with `read_dir`. On a case-insensitive filesystem — APFS and NTFS, so most
/// desktops — those two disagree: the write resolves onto an existing
/// `skill.md`, while `read_dir` still reports the entry under the case stored
/// on disk. An exact `Path` comparison therefore classified a file we had
/// *just written* as a leftover and deleted it, leaving the skill with no
/// `SKILL.md` while `apply` reported `Updated`, and `status` had sent the user
/// there by calling the install stale.
///
/// Folding can only err the other way, and only on a case-sensitive
/// filesystem: a genuine leftover whose name differs from a shipped file's by
/// case alone survives a reinstall. A stale file kept is recoverable. A live
/// file deleted is not.
///
/// ASCII folding is enough because every shipped path is ASCII, and a fold
/// that guessed at non-ASCII case would be a second way to be wrong.
fn is_wanted(relative: &Path, keep: &[&Path]) -> bool {
    let found = relative.as_os_str().to_string_lossy();
    keep.iter()
        .any(|k| k.as_os_str().to_string_lossy().eq_ignore_ascii_case(&found))
}

/// Write `content` only when it differs, so a re-run is a genuine no-op.
///
/// Not just cosmetic: rewriting identical bytes bumps the mtime, which is
/// enough to make a watcher rebuild and a `git status` look no different while
/// every incremental tool disagrees.
fn write_if_changed(path: &Path, content: &str) -> Result<Change, SetupError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::read(path) {
        Ok(current) if current == content.as_bytes() => Ok(Change::Unchanged),
        Ok(_) => {
            std::fs::write(path, content)?;
            Ok(Change::Updated)
        }
        Err(_) => {
            std::fs::write(path, content)?;
            Ok(Change::Created)
        }
    }
}

/// Install the skill tree at `dest`, replacing it as a unit.
///
/// A skill is one document in four files: a file dropped between releases must
/// not survive to shadow what the new one says. Stale files are removed rather
/// than the whole directory being deleted first, so an unchanged reinstall
/// touches nothing at all.
fn install_skill(dest: &Path) -> Result<(Change, usize), SetupError> {
    let existed = dest.exists();
    let mut wanted = Vec::new();
    collect(&SKILL, Path::new(""), &mut wanted);

    let mut moved = false;
    for (relative, bytes) in &wanted {
        let target = dest.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::read(&target) {
            Ok(current) if current == *bytes => {}
            _ => {
                std::fs::write(&target, bytes)?;
                moved = true;
            }
        }
    }

    let keep: Vec<&Path> = wanted.iter().map(|(p, _)| p.as_path()).collect();
    if existed {
        moved |= remove_strays(dest, dest, &keep)?;
    }

    let change = match (existed, moved) {
        (false, _) => Change::Created,
        (true, true) => Change::Updated,
        (true, false) => Change::Unchanged,
    };
    Ok((change, wanted.len()))
}

fn collect<'a>(dir: &Dir<'a>, prefix: &Path, out: &mut Vec<(PathBuf, &'a [u8])>) {
    for file in dir.files() {
        let name = file.path().file_name().unwrap_or_default();
        out.push((prefix.join(name), file.contents()));
    }
    for sub in dir.dirs() {
        let name = sub.path().file_name().unwrap_or_default();
        collect(sub, &prefix.join(name), out);
    }
}

/// Delete anything under `dir` the embedded skill no longer ships.
fn remove_strays(root: &Path, dir: &Path, keep: &[&Path]) -> Result<bool, SetupError> {
    let mut removed = false;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            removed |= remove_strays(root, &path, keep)?;
            // An emptied directory is as much a leftover as the file was.
            if std::fs::read_dir(&path)?.next().is_none() {
                std::fs::remove_dir(&path)?;
            }
        } else if let Ok(relative) = path.strip_prefix(root)
            && !is_wanted(relative, keep)
        {
            std::fs::remove_file(&path)?;
            removed = true;
        }
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Asking
// ---------------------------------------------------------------------------

/// Refuse to prompt when nobody can answer.
///
/// CI and coding agents run this command. A blocking `read_line` against a
/// closed stdin hangs until something kills it, which is strictly worse than
/// an error — so the error names the flag that would have avoided the question.
pub fn require_interactive(hint: &'static str) -> Result<(), SetupError> {
    if std::io::stdin().is_terminal() {
        Ok(())
    } else {
        Err(SetupError::NotATerminal { hint })
    }
}

/// Ask a yes/no question, defaulting to no.
///
/// Defaulting to no because every caller is about to write files: a bare
/// Return should leave the disk alone.
pub fn confirm(question: &str, hint: &'static str) -> Result<bool, SetupError> {
    use std::io::Write;

    require_interactive(hint)?;
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    // --- detection ---------------------------------------------------------

    #[test]
    fn a_missing_directory_is_not_invented() {
        // Installing where nothing looks is indistinguishable from not
        // installing, except that it reports success.
        let t = temp();
        assert!(project_targets(t.path()).is_empty());
        assert!(
            !t.path().join(".claude").exists(),
            "must not create anything"
        );
        assert!(!t.path().join("AGENTS.md").exists());
    }

    #[test]
    fn each_vendor_is_detected_by_its_own_marker() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        std::fs::create_dir_all(t.path().join(".cursor")).unwrap();
        std::fs::create_dir_all(t.path().join(".windsurf")).unwrap();
        std::fs::create_dir_all(t.path().join(".github")).unwrap();
        std::fs::write(t.path().join("AGENTS.md"), "# Agents\n").unwrap();

        let found = project_targets(t.path());
        let agents: Vec<_> = found.iter().map(|t| t.agent).collect();
        assert_eq!(
            agents,
            [
                "Claude Code",
                "Cursor",
                "Windsurf",
                "GitHub Copilot",
                "Codex / AGENTS.md"
            ]
        );
        assert!(project_undetected(t.path()).is_empty());
    }

    #[test]
    fn two_vendors_in_one_project_both_get_a_target() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        std::fs::create_dir_all(t.path().join(".github")).unwrap();
        let found = project_targets(t.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].form, Form::Skill);
        assert_eq!(found[1].form, Form::Region);
        assert_eq!(
            project_undetected(t.path())
                .iter()
                .map(|(a, _)| *a)
                .collect::<Vec<_>>(),
            ["Cursor", "Windsurf", "Codex / AGENTS.md"]
        );
    }

    #[test]
    fn project_scope_never_resolves_a_path_under_home() {
        // The regression this rewrite exists for: a project-scoped run used to
        // fall back to ~/.claude when the project had no .claude of its own.
        let t = temp();
        let home = temp();
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        // Nothing in the project, everything in the "home": still nothing.
        assert!(project_targets(t.path()).is_empty());
        for target in project_targets(t.path()) {
            assert!(target.path.starts_with(t.path()));
        }
    }

    // --- user scope --------------------------------------------------------

    /// The three user-scope directories, none of them created.
    ///
    /// Tests pass these explicitly rather than setting `VIBE_HOME` /
    /// `XDG_CONFIG_HOME`: the process environment is shared by every test in
    /// the binary, and `cargo test` runs them concurrently.
    fn user_dirs(home: &Path) -> (PathBuf, PathBuf) {
        (home.join(".vibe"), home.join(".config/opencode"))
    }

    #[test]
    fn a_home_with_nothing_in_it_gets_nothing_written() {
        // The gate the whole of user scope turns on: a directory that does not
        // exist is a tool that was never run, and this command does not create
        // a config directory in someone's home on the chance they might.
        let home = temp();
        let (vibe, opencode) = user_dirs(home.path());
        assert!(user_targets_in(home.path(), &vibe, &opencode).is_empty());
        assert!(!vibe.exists());
        assert!(!opencode.exists());
        assert!(!home.path().join(".claude").exists());

        let missing: Vec<_> = user_undetected_in(home.path(), &vibe, &opencode)
            .into_iter()
            .map(|(a, _)| a)
            .collect();
        assert_eq!(missing, ["Claude Code", "Mistral Vibe", "opencode"]);
    }

    #[test]
    fn each_user_agent_is_detected_by_its_own_directory() {
        let home = temp();
        let (vibe, opencode) = user_dirs(home.path());
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        std::fs::create_dir_all(&vibe).unwrap();
        std::fs::create_dir_all(&opencode).unwrap();

        let found = user_targets_in(home.path(), &vibe, &opencode);
        assert_eq!(
            found.iter().map(|t| t.agent).collect::<Vec<_>>(),
            ["Claude Code", "Mistral Vibe", "opencode"]
        );
        assert_eq!(found[0].path, home.path().join(".claude/skills/teksilo"));
        assert_eq!(found[0].form, Form::Skill);
        assert_eq!(found[1].path, vibe.join("AGENTS.md"));
        assert_eq!(found[1].form, Form::Region);
        assert_eq!(found[2].path, opencode.join("AGENTS.md"));
        assert_eq!(found[2].form, Form::Region);
        assert!(user_undetected_in(home.path(), &vibe, &opencode).is_empty());
    }

    #[test]
    fn one_installed_agent_does_not_pull_in_the_other_two() {
        let home = temp();
        let (vibe, opencode) = user_dirs(home.path());
        std::fs::create_dir_all(&vibe).unwrap();

        let found = user_targets_in(home.path(), &vibe, &opencode);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].agent, "Mistral Vibe");
        assert_eq!(
            user_undetected_in(home.path(), &vibe, &opencode)
                .into_iter()
                .map(|(a, _)| a)
                .collect::<Vec<_>>(),
            ["Claude Code", "opencode"]
        );
    }

    #[test]
    fn a_relocated_config_directory_is_where_the_file_lands() {
        // `VIBE_HOME` moves Vibe's whole state directory and opencode follows
        // `XDG_CONFIG_HOME`. Writing `~/.vibe/AGENTS.md` for a user who
        // relocated theirs is a file nothing reads — the failure this tool
        // exists to avoid, in the one scope where it lands in $HOME.
        let elsewhere = temp();
        let home = temp();
        let vibe = elsewhere.path().join("vibe-state");
        let opencode = elsewhere.path().join("xdg/opencode");
        std::fs::create_dir_all(&vibe).unwrap();
        std::fs::create_dir_all(&opencode).unwrap();

        let found = user_targets_in(home.path(), &vibe, &opencode);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].path, vibe.join("AGENTS.md"));
        assert_eq!(found[1].path, opencode.join("AGENTS.md"));
        assert!(
            !home.path().join(".vibe").exists() && !home.path().join(".config").exists(),
            "the default locations must not be touched when the env moved them"
        );
    }

    #[test]
    fn the_defaults_are_the_documented_ones() {
        // Guards the fallback arm of both resolvers — the path taken on every
        // machine that sets neither variable, which is most of them.
        let home = Path::new("/home/someone");
        // SAFETY-adjacent: these read the ambient environment, so assert only
        // what holds either way — the suffix under whatever base was chosen.
        assert!(vibe_home(home).ends_with(".vibe") || std::env::var_os("VIBE_HOME").is_some());
        assert!(
            opencode_config(home).ends_with("opencode"),
            "opencode always lives in a directory of its own name"
        );
    }

    // --- finding the project ----------------------------------------------

    #[test]
    fn the_project_root_is_the_nearest_manifest_above() {
        let t = temp();
        std::fs::write(t.path().join("Cargo.toml"), "[package]\n").unwrap();
        let deep = t.path().join("crates/app/src");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            find_project_root(&deep).unwrap().canonicalize().unwrap(),
            t.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn no_manifest_anywhere_is_none_rather_than_a_guess() {
        let t = temp();
        assert_eq!(find_project_root(t.path()), None);
    }

    // --- the shared-file region -------------------------------------------

    #[test]
    fn a_region_is_appended_to_a_file_that_has_none() {
        let out = upsert_region(
            "# My project\n\nRules here.\n",
            "<!-- BEGIN teksilo -->\nX\n<!-- END teksilo -->",
        );
        assert!(out.starts_with("# My project\n\nRules here.\n\n"));
        assert!(out.contains(BEGIN) && out.contains(END));
    }

    #[test]
    fn a_second_run_changes_nothing() {
        let block = region_block();
        let once = upsert_region("# My project\n", &block);
        let twice = upsert_region(&once, &block);
        assert_eq!(once, twice, "setup must be idempotent on a shared file");
    }

    #[test]
    fn nothing_outside_the_markers_is_disturbed() {
        let before = format!("head\n\n{BEGIN}\nstale brief\n{END}\n\ntail\n");
        let after = upsert_region(&before, &format!("{BEGIN}\nfresh brief\n{END}"));
        assert_eq!(
            after,
            format!("head\n\n{BEGIN}\nfresh brief\n{END}\n\ntail\n")
        );
    }

    #[test]
    fn an_empty_file_gets_no_leading_blank_line() {
        let out = upsert_region("", &region_block());
        assert!(out.starts_with(BEGIN));
    }

    // --- data loss, found by review and reproduced ------------------------

    #[test]
    fn a_shared_file_that_is_not_utf8_stops_setup_instead_of_being_destroyed() {
        // Found by review and reproduced against the real binary: one Latin-1
        // byte in a project's AGENTS.md and `setup -y` replaced the whole file
        // with nothing but our region. `read_to_string(..).unwrap_or_default()`
        // read "cannot decode" as "the file is empty".
        let t = temp();
        let path = t.path().join("AGENTS.md");
        let original = b"# My project rules\n\nNever delete this. \xff\n";
        std::fs::write(&path, original).unwrap();

        let target = project_targets(t.path())
            .into_iter()
            .find(|t| t.form == Form::Region)
            .unwrap();

        assert!(
            matches!(inspect(&target), Presence::Blocked(_)),
            "an undecodable shared file is not `Absent` — setup cannot install here"
        );
        assert!(
            matches!(apply(&target), Err(SetupError::NotUtf8(_))),
            "setup must refuse rather than rewrite a file it cannot read whole"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            original,
            "the project's own bytes must survive untouched"
        );
    }

    #[test]
    fn a_missing_shared_file_is_still_created() {
        // The other half of the same change: `read_shared` must keep treating
        // "not found" as the empty file, or Region targets stop installing.
        let t = temp();
        std::fs::write(t.path().join("AGENTS.md"), "seed").unwrap();
        let target = project_targets(t.path())
            .into_iter()
            .find(|t| t.form == Form::Region)
            .unwrap();
        std::fs::remove_file(&target.path).unwrap();

        assert_eq!(inspect(&target), Presence::Absent);
        assert_eq!(apply(&target).unwrap().change, Change::Created);
        assert_eq!(inspect(&target), Presence::Current);
    }

    #[test]
    fn a_differently_cased_skill_file_is_not_deleted() {
        // Found by review and reproduced against the real binary on APFS:
        // `status` called the install stale, and the `setup` it recommended
        // then DELETED SKILL.md and reported success. The write resolves
        // case-insensitively onto the existing entry; `read_dir` reports the
        // case on disk; an exact comparison called it a stray.
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        let target = project_targets(t.path()).remove(0);
        apply(&target).unwrap();

        let upper = target.path.join("SKILL.md");
        let body = std::fs::read(&upper).unwrap();
        std::fs::remove_file(&upper).unwrap();
        std::fs::write(target.path.join("skill.md"), &body).unwrap();

        // On a case-insensitive filesystem this is the same file under another
        // name, so nothing is stale and nothing may be removed. On a
        // case-sensitive one it is a genuinely separate file — also not to be
        // deleted, since folding errs only towards keeping.
        apply(&target).unwrap();
        assert!(
            std::fs::read(target.path.join("SKILL.md")).is_ok(),
            "the skill's own SKILL.md must survive a reinstall"
        );
    }

    #[test]
    fn a_real_stray_is_still_removed() {
        // The case fold must not turn `remove_strays` into a no-op.
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        let target = project_targets(t.path()).remove(0);
        apply(&target).unwrap();

        let stray = target.path.join("reference/from_an_older_release.md");
        std::fs::write(&stray, b"x").unwrap();
        assert_eq!(inspect(&target), Presence::Stale);
        assert_eq!(apply(&target).unwrap().change, Change::Updated);
        assert!(!stray.exists(), "a genuine leftover is still pruned");
    }

    // --- inspect is the dry run of apply -----------------------------------

    /// Every project form, for the equivalence test below.
    fn one_of_each_form(root: &Path) -> Vec<Target> {
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::create_dir_all(root.join(".cursor")).unwrap();
        std::fs::write(root.join("AGENTS.md"), "# Ours\n\nKeep this.\n").unwrap();
        let targets = project_targets(root);
        let forms: Vec<Form> = targets.iter().map(|t| t.form).collect();
        assert!(forms.contains(&Form::Skill), "need a Skill target");
        assert!(forms.contains(&Form::Region), "need a Region target");
        assert!(
            forms.iter().any(|f| matches!(f, Form::OwnFile { .. })),
            "need an OwnFile target"
        );
        targets
    }

    #[test]
    fn inspect_matches_apply() {
        // The contract `Presence` exists for: `status` predicts `setup`.
        // Checked for every form, in all three of the states a target can be
        // in — nothing there, an older release's content, and current.
        let t = temp();
        let targets = one_of_each_form(t.path());

        for target in &targets {
            assert_eq!(
                inspect(target),
                Presence::Absent,
                "{}: nothing of ours is installed yet",
                target.agent
            );
        }

        for target in &targets {
            let change = apply(target).unwrap().change;
            assert_ne!(change, Change::Unchanged, "{}: first write", target.agent);
            assert_eq!(
                inspect(target),
                Presence::Current,
                "{}: current immediately after apply",
                target.agent
            );
            // The equivalence, stated directly.
            assert_eq!(
                apply(target).unwrap().change,
                Change::Unchanged,
                "{}: apply agrees it is current",
                target.agent
            );
        }
    }

    #[test]
    fn an_older_releases_content_inspects_as_stale_for_every_form() {
        let t = temp();
        let targets = one_of_each_form(t.path());
        for target in &targets {
            apply(target).unwrap();
        }

        for target in &targets {
            match target.form {
                Form::Skill => {
                    std::fs::write(target.path.join("SKILL.md"), b"from 0.9").unwrap();
                }
                Form::OwnFile { .. } => {
                    std::fs::write(&target.path, b"from 0.9").unwrap();
                }
                Form::Region => {
                    let stale = format!("# Ours\n\n{BEGIN}\nfrom 0.9\n{END}\n");
                    std::fs::write(&target.path, stale).unwrap();
                }
            }
            assert_eq!(
                inspect(target),
                Presence::Stale,
                "{}: an older release's content",
                target.agent
            );
            // And `apply` calls the same situation a change, not a no-op.
            assert_eq!(
                apply(target).unwrap().change,
                Change::Updated,
                "{}: apply rewrites it",
                target.agent
            );
        }
    }

    #[test]
    fn a_stray_file_makes_the_skill_stale_but_an_empty_directory_does_not() {
        // `remove_strays` prunes an emptied directory WITHOUT counting it as a
        // change, so `has_strays` must ignore one too — otherwise `status`
        // advertises work that `setup` then reports as `unchanged`.
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        let target = project_targets(t.path()).remove(0);
        apply(&target).unwrap();
        assert_eq!(inspect(&target), Presence::Current);

        std::fs::create_dir_all(target.path.join("leftover")).unwrap();
        assert_eq!(
            inspect(&target),
            Presence::Current,
            "an empty leftover directory is not a change apply would report"
        );
        assert_eq!(apply(&target).unwrap().change, Change::Unchanged);

        std::fs::write(target.path.join("from_0_9.md"), b"x").unwrap();
        assert_eq!(inspect(&target), Presence::Stale);
        assert_eq!(apply(&target).unwrap().change, Change::Updated);
    }

    #[test]
    fn a_shared_file_without_our_markers_is_absent_not_stale() {
        // AGENTS.md usually exists before this tool ever runs. Calling that
        // "an outdated teksilo install" would be a claim with no evidence.
        let t = temp();
        std::fs::write(t.path().join("AGENTS.md"), "# Someone else's\n").unwrap();
        let target = project_targets(t.path())
            .into_iter()
            .find(|t| t.form == Form::Region)
            .unwrap();
        assert_eq!(inspect(&target), Presence::Absent);
    }

    #[test]
    fn a_foreign_directory_at_the_skill_path_is_absent_not_stale() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude/skills/teksilo")).unwrap();
        std::fs::write(
            t.path().join(".claude/skills/teksilo/unrelated.md"),
            b"not ours",
        )
        .unwrap();
        let target = project_targets(t.path()).remove(0);
        assert_eq!(inspect(&target), Presence::Absent);
    }

    #[test]
    fn inspect_writes_nothing() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".cursor")).unwrap();
        let target = project_targets(t.path()).remove(0);
        assert_eq!(inspect(&target), Presence::Absent);
        assert!(
            !t.path().join(".cursor/rules").exists(),
            "inspecting must not create the directory apply would"
        );
    }

    #[test]
    fn the_user_scope_note_lists_every_not_applicable_agent() {
        // The note is rendered from the table `status` renders its n/a rows
        // from, so the two can never list different agents.
        let note = user_scope_note();
        for (agent, why) in USER_NOT_APPLICABLE {
            assert!(note.contains(agent), "note omits {agent}");
            assert!(note.contains(why), "note omits why for {agent}");
        }
        assert!(
            note.contains(&format!("those {}", USER_NOT_APPLICABLE.len())),
            "the count must be derived, not typed: {note}"
        );
    }

    // --- writing -----------------------------------------------------------

    #[test]
    fn installing_writes_the_whole_skill_tree() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        let targets = project_targets(t.path());
        let done = apply(&targets[0]).unwrap();

        let root = t.path().join(".claude/skills/teksilo");
        assert!(root.join("SKILL.md").is_file());
        assert!(root.join("reference/teksu.md").is_file());
        assert!(root.join("reference/automation.md").is_file());
        assert!(root.join("reference/teksilo_app_guide.md").is_file());
        assert_eq!(done.change, Change::Created);
        assert!(done.files >= 4);
    }

    #[test]
    fn reinstalling_removes_a_file_the_previous_version_shipped() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        let targets = project_targets(t.path());
        apply(&targets[0]).unwrap();

        let stale = t.path().join(".claude/skills/teksilo/reference/gone.md");
        std::fs::write(&stale, b"from an older release").unwrap();
        let again = apply(&targets[0]).unwrap();
        assert!(
            !stale.exists(),
            "a stale file must not shadow the new skill"
        );
        assert_eq!(again.change, Change::Updated);
    }

    #[test]
    fn an_unchanged_reinstall_reports_unchanged() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        std::fs::create_dir_all(t.path().join(".cursor")).unwrap();
        std::fs::write(t.path().join("AGENTS.md"), "# Agents\n").unwrap();
        let targets = project_targets(t.path());
        for target in &targets {
            apply(target).unwrap();
        }
        for target in &targets {
            assert_eq!(
                apply(target).unwrap().change,
                Change::Unchanged,
                "{} re-wrote itself",
                target.agent
            );
        }
    }

    #[test]
    fn a_rules_file_carries_its_vendors_frontmatter_and_the_brief() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".cursor")).unwrap();
        std::fs::create_dir_all(t.path().join(".windsurf")).unwrap();
        for target in project_targets(t.path()) {
            apply(&target).unwrap();
        }

        let cursor = std::fs::read_to_string(t.path().join(".cursor/rules/teksilo.mdc")).unwrap();
        assert!(cursor.starts_with("---\ndescription:"));
        assert!(cursor.contains("alwaysApply: false"));
        assert!(cursor.contains("cargo teksilo show"));

        let windsurf =
            std::fs::read_to_string(t.path().join(".windsurf/rules/teksilo.md")).unwrap();
        assert!(windsurf.starts_with("---\ntrigger: glob"));
        assert!(windsurf.contains("cargo teksilo show"));
    }

    #[test]
    fn the_brief_stands_alone() {
        // It is the only teksilo instruction a non-Claude agent ever sees, so
        // it must not point at a skill that may not be installed.
        assert!(!BRIEF.contains("SKILL.md"));
        assert!(!BRIEF.contains("reference/"));
        assert!(BRIEF.contains("cargo teksilo symbol"));
        assert!(BRIEF.contains("cargo teksilo search"));
        assert!(BRIEF.contains("cargo teksilo show"));
        assert!(BRIEF.contains("cargo teksilo probe"));
        assert!(BRIEF.contains("cargo teksilo setup"));
        assert!(
            BRIEF.contains("blob/main"),
            "the GitHub warning is the point"
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
