// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What is installed, in both scopes, without writing anything.
//!
//! `setup` answers "make this work"; `status` answers "does it, and where".
//! They are deliberately the same question asked twice, so this module owns
//! almost no judgement: every agent row is [`setup::inspect`], the read-only
//! twin of [`setup::apply`], and the two share their content computation.
//!
//! ## Why this is its own command
//!
//! It reports on **both** scopes at once, and `setup` is scope-*selected*
//! (`--user` exclusive-or the project). A `setup --status` would therefore
//! have to consider scopes its own non-status form does not, which is a second
//! command wearing the first one's name. Keeping it separate also keeps the
//! read-only thing out of a writing command's flag space: there is no
//! `--status -y` to reason about, and no way for a mistyped status invocation
//! to edit `$HOME`.
//!
//! ## Three words, and the fourth fact
//!
//! A row is `here`, `not here` or `n/a`. Those answer "is the integration
//! working". But three states cannot carry the difference between *Cursor is
//! not used in this project* and *Cursor is used here and has no brief*, and
//! that difference is the whole of what the reader should do next. So the
//! state is one column and the reason is another — the same split
//! [`setup::USER_NOT_APPLICABLE`] exists to make, rather than inventing a
//! fourth word nobody asked for.
//!
//! Staleness rides in the detail column for the same reason: an install from
//! an older release **is** here — it is being read right now — so calling it
//! anything but `here` would be wrong. It just is not what this build writes.

use std::path::{Path, PathBuf};

use crate::{guard, resolve, setup, symbol, vectors};

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One agent's integration status, in one scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The teksilo brief is installed for this agent in this scope.
    Here,
    /// It could be installed here, and is not.
    NotHere,
    /// This scope holds no file for this agent that this tool writes.
    NotApplicable,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Here => "here",
            State::NotHere => "not here",
            State::NotApplicable => "n/a",
        }
    }
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub agent: &'static str,
    pub state: State,
    /// The path when there is one, otherwise what was looked for and missed.
    ///
    /// Never empty: a row whose state is the whole message is a row that
    /// leaves the reader with nothing to act on.
    pub detail: String,
}

/// The shared body of both scopes' rows: detected, then inspected.
///
/// Split out because the two scopes disagree about what evidence means
/// (a project marker may be a file, a user marker is always a directory) but
/// agree entirely about what to do with the answer.
fn row_for(target: &setup::Target, detected: bool, looked_at: &str, root: &Path) -> Row {
    if !detected {
        return Row {
            agent: target.agent,
            state: State::NotHere,
            detail: format!("no {looked_at}"),
        };
    }
    // Only the skill is a directory; every brief is one file.
    let path = show_path(&target.path, root, target.form == setup::Form::Skill);
    match setup::inspect(target) {
        setup::Presence::Current => Row {
            agent: target.agent,
            state: State::Here,
            detail: path,
        },
        setup::Presence::Stale => Row {
            agent: target.agent,
            state: State::Here,
            detail: format!("{path} — from another release, re-run setup"),
        },
        setup::Presence::Absent => Row {
            agent: target.agent,
            state: State::NotHere,
            // The agent IS configured here — this is the actionable case, and
            // the one a bare "not here" would make indistinguishable from an
            // agent nobody in this project uses.
            detail: format!("{looked_at} is here, brief is not"),
        },
        // Not `Absent`: `setup` would refuse here, so telling the reader to
        // run it would be advice that does nothing.
        setup::Presence::Blocked(why) => Row {
            agent: target.agent,
            state: State::NotHere,
            detail: format!("{path} {why} — setup would refuse"),
        },
    }
}

/// Every project vendor, in the vendor table's order.
pub fn project_rows(root: &Path) -> Vec<Row> {
    setup::project_candidates(root)
        .into_iter()
        .map(|c| row_for(&c.target, c.detected(), c.target.marker, root))
        .collect()
}

/// Every user-scope agent: the three with a file, then the ones without.
///
/// The `n/a` rows come from [`setup::USER_NOT_APPLICABLE`], the same table
/// `setup --user` prints its closing note from, so the two can never come to
/// list different agents.
pub fn user_rows(home: &Path) -> Vec<Row> {
    user_rows_in(home, &setup::vibe_home(home), &setup::opencode_config(home))
}

/// [`user_rows`] against explicitly given directories, for tests.
pub fn user_rows_in(home: &Path, vibe: &Path, opencode: &Path) -> Vec<Row> {
    let mut rows: Vec<Row> = setup::user_candidates(home, vibe, opencode)
        .into_iter()
        .map(|c| {
            let dir = show_path(&c.dir, home, true);
            row_for(&c.target, c.detected(), &dir, home)
        })
        .collect();
    rows.extend(setup::USER_NOT_APPLICABLE.iter().map(|(agent, why)| Row {
        agent,
        state: State::NotApplicable,
        detail: (*why).to_string(),
    }));
    rows
}

// ---------------------------------------------------------------------------
// The search encoder
// ---------------------------------------------------------------------------

/// Where the model for semantic search is, if this build has one at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Encoder {
    /// Compiled without `semantic`: there is no model, and never will be.
    Unavailable,
    /// Downloaded and ready.
    Cached { dir: PathBuf, bytes: u64 },
    /// There is a cache directory and the model is not in it yet.
    Missing { dir: PathBuf },
}

impl Encoder {
    pub fn state(&self) -> State {
        match self {
            Encoder::Unavailable => State::NotApplicable,
            Encoder::Cached { .. } => State::Here,
            Encoder::Missing { .. } => State::NotHere,
        }
    }
}

pub fn encoder_status() -> Encoder {
    match vectors::encoder_cache_dir() {
        None => Encoder::Unavailable,
        Some(dir) if vectors::encoder_is_cached() => {
            let bytes = dir_size(&dir);
            Encoder::Cached { dir, bytes }
        }
        Some(dir) => Encoder::Missing { dir },
    }
}

/// Bytes under `dir`, following no symlinks and failing quietly.
///
/// Reported because "cached" alone does not distinguish a complete model from
/// an interrupted download that left a directory behind, and the size is the
/// cheapest signal that separates them.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// A path as the reader would recognise it: relative to the scope it belongs
/// to, or absolute when it escaped that scope (a relocated `$VIBE_HOME`).
fn show(path: &Path, root: &Path) -> String {
    escape(&match path.strip_prefix(root) {
        Ok(rest) => rest.display().to_string(),
        Err(_) => path.display().to_string(),
    })
}

/// An absolute path, rendered safely.
fn show_safe(path: &Path) -> String {
    escape(&path.display().to_string())
}

/// Make a path printable on one line.
///
/// These paths come from `$VIBE_HOME`, `$XDG_CONFIG_HOME` and the working
/// directory — none of them chosen by this tool. A newline in one would split
/// a row in half and let the tail pose as another agent's line, so control
/// characters are shown rather than obeyed. `Path::display` is already lossy
/// for non-UTF-8, which is the right trade in a report.
fn escape(shown: &str) -> String {
    shown
        .chars()
        .map(|c| match c {
            '\n' => "\\n".to_string(),
            '\r' => "\\r".to_string(),
            '\t' => "\\t".to_string(),
            c if c.is_control() => format!("\\u{{{:04x}}}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

/// [`show`], with a trailing `/` when the thing named is a directory.
///
/// Not decoration: `.cursor` and `.cursor/` are different claims, and a
/// reader told "no .cursor" cannot tell whether to create a file or a folder.
fn show_path(path: &Path, root: &Path, directory: bool) -> String {
    let shown = show(path, root);
    if directory {
        format!("{shown}/")
    } else {
        shown
    }
}

const AGENT_W: usize = 19;
const STATE_W: usize = 10;

pub fn print_rows(rows: &[Row]) {
    for row in rows {
        println!(
            "  {:<AGENT_W$}{:<STATE_W$}{}",
            row.agent,
            row.state.word(),
            row.detail
        );
    }
}

/// The whole report. Read-only; the exit code never depends on what it finds.
pub fn report(dir: &Path) {
    println!("cargo-teksilo {}", crate::guard::TOOL_VERSION);

    // --- project ----------------------------------------------------------
    match setup::find_project_root(dir) {
        Some(root) => {
            // The version is what every other command's answers are pinned to,
            // so a status report that omitted it would describe an
            // installation without saying what it was matched against.
            // `resolve_locked`, never `resolve`: plain `cargo metadata`
            // resolves, and resolving WRITES — it creates a missing Cargo.lock
            // and rewrites a stale one. A command whose first claim is "reads
            // only" must not leave a lockfile behind, so it asks in the form
            // cargo refuses rather than writes, and reports the refusal.
            match resolve::resolve_locked(&root) {
                Ok(r) => {
                    println!("app resolved teksilo {}", r.version);
                    // status is the dry run of every version-gated command, not
                    // only of `setup`: an app whose teksilo this tool refuses to
                    // answer for should learn it here, rather than from the
                    // first `symbol` that refuses.
                    match guard::check(&r.version) {
                        guard::Verdict::Ok => {}
                        guard::Verdict::Degraded { app, tool } => {
                            println!("{}", guard::degraded_text(&app, &tool))
                        }
                        guard::Verdict::Refuse { app, tool } => println!(
                            "\n{}",
                            guard::refusal_text(
                                &app,
                                &tool,
                                "symbol and search",
                                guard::readable_sources(&r).as_deref(),
                            )
                        ),
                    }
                }
                Err(_) => println!(
                    "app resolved teksilo — unknown: no up-to-date Cargo.lock here, and\n\
                     writing one is the thing this command will not do"
                ),
            }
            println!("\nProject  {}", show_safe(&root));
            print_rows(&project_rows(&root));
            println!(
                "\n  Mistral Vibe, opencode, Grok Build and Cline all read a project's\n  \
                 AGENTS.md, so the AGENTS.md row above serves them too — Cline has a\n  \
                 row of its own because it also reads a directory this can write."
            );
        }
        None => {
            println!(
                "\nProject  none — no Cargo.toml in {} or above it",
                dir.display()
            );
        }
    }

    // --- user -------------------------------------------------------------
    match setup::home_dir() {
        Ok(home) => {
            println!("\nUser  {}", show_safe(&home));
            print_rows(&user_rows(&home));
        }
        Err(e) => println!("\nUser  unavailable — {e}"),
    }

    // --- the search encoder ----------------------------------------------
    let encoder = encoder_status();
    println!(
        "\nSearch encoder  {} ({} dimensions)",
        vectors::ENCODER_ID,
        vectors::ENCODER_DIM
    );
    let (state, detail) = match &encoder {
        Encoder::Cached { dir, bytes } => (
            encoder.state(),
            format!("{} ({} on disk)", show_safe(dir), mib(*bytes)),
        ),
        // One line, no hand-rolled column alignment: the path alone can pass
        // a terminal's width, so a padded second physical line lands wherever
        // the wrap happened to leave it rather than under the column.
        Encoder::Missing { dir } => (
            encoder.state(),
            format!(
                "{} — not downloaded (~{} MB)",
                show_safe(dir),
                vectors::ENCODER_DOWNLOAD_MB,
            ),
        ),
        Encoder::Unavailable => (
            encoder.state(),
            "this build has no encoder (compiled with `--no-default-features`);\n  \
             search uses BM25 only, which is a supported mode and not a fault"
                .to_string(),
        ),
    };
    println!(
        "  {:<AGENT_W$}{:<STATE_W$}{}",
        "model",
        state.word(),
        detail
    );
    if matches!(encoder, Encoder::Missing { .. }) {
        println!("  `cargo teksilo setup` fetches it, or the next `search` will.");
        println!("  Until then `search` uses BM25 only.");
    }

    // --- the symbol extractor's interpreter -------------------------------
    //
    // Reported for the same reason the encoder is: `symbol` is the one
    // subcommand that needs something this binary cannot ship. The API
    // extractor is a Python script (one source of truth, byte-identical to
    // `tools/extract_widget_api.py` under a CI diff — which is exactly what a
    // Rust rewrite would cost us), so `symbol` needs an interpreter that a
    // Rust-only machine has no reason to have. Without this row the only way
    // to find that out is to run `symbol` and read the failure.
    println!("\nSymbol extractor  Python 3");
    let (state, detail) = match symbol::find_python() {
        Some(p) => (State::Here, show_safe(&p)),
        None => (
            State::NotHere,
            "not on PATH — `symbol` needs it; every other subcommand works without it".to_string(),
        ),
    };
    println!(
        "  {:<AGENT_W$}{:<STATE_W$}{}",
        "interpreter",
        state.word(),
        detail
    );

    // What every `here` above does and does not mean. Shared with `setup`, so
    // the two commands cannot come to promise different things.
    println!("\n{}", setup::ACTIVATION_CAVEAT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn find<'a>(rows: &'a [Row], agent: &str) -> &'a Row {
        rows.iter()
            .find(|r| r.agent == agent)
            .unwrap_or_else(|| panic!("no row for {agent}"))
    }

    // --- the three states --------------------------------------------------

    #[test]
    fn an_agent_nobody_uses_here_is_not_here() {
        let t = temp();
        let rows = project_rows(t.path());
        assert!(rows.iter().all(|r| r.state == State::NotHere));
        // Every one names what it looked for, so the reader can act.
        assert!(rows.iter().all(|r| r.detail.starts_with("no ")));
    }

    #[test]
    fn an_agent_that_is_here_without_a_brief_is_distinguishable() {
        // The distinction three words cannot carry, and the reason the detail
        // column exists: this is the actionable case.
        let t = temp();
        std::fs::create_dir_all(t.path().join(".cursor")).unwrap();
        let rows = project_rows(t.path());
        let row = find(&rows, "Cursor");
        assert_eq!(row.state, State::NotHere);
        assert!(
            row.detail.contains(".cursor/") && row.detail.contains("brief is not"),
            "must say the agent is here and the brief is not: {}",
            row.detail
        );
    }

    #[test]
    fn a_fresh_install_reports_here_with_its_path() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".cursor")).unwrap();
        let targets = setup::project_targets(t.path());
        setup::apply(&targets[0]).unwrap();

        let rows = project_rows(t.path());
        let row = find(&rows, "Cursor");
        assert_eq!(row.state, State::Here);
        assert_eq!(row.detail, ".cursor/rules/teksilo.mdc");
    }

    #[test]
    fn an_older_releases_brief_is_still_here_but_says_so() {
        let t = temp();
        std::fs::create_dir_all(t.path().join(".cursor/rules")).unwrap();
        std::fs::write(
            t.path().join(".cursor/rules/teksilo.mdc"),
            "---\ndescription: from 0.9\n---\nolder brief\n",
        )
        .unwrap();

        let rows = project_rows(t.path());
        let row = find(&rows, "Cursor");
        assert_eq!(
            row.state,
            State::Here,
            "an outdated brief is being read right now — it is here"
        );
        assert!(row.detail.contains("another release"), "{}", row.detail);
    }

    // --- scopes ------------------------------------------------------------

    #[test]
    fn project_rows_never_look_at_home() {
        let t = temp();
        let home = temp();
        std::fs::create_dir_all(home.path().join(".claude/skills/teksilo")).unwrap();
        let rows = project_rows(t.path());
        let row = find(&rows, "Claude Code");
        assert_eq!(row.state, State::NotHere);
    }

    #[test]
    fn user_scope_lists_the_writable_three_and_the_rest_as_na() {
        let home = temp();
        let vibe = home.path().join(".vibe");
        let opencode = home.path().join(".config/opencode");
        std::fs::create_dir_all(&vibe).unwrap();

        let rows = user_rows_in(home.path(), &vibe, &opencode);
        assert_eq!(find(&rows, "Mistral Vibe").state, State::NotHere);
        assert_eq!(find(&rows, "opencode").state, State::NotHere);
        assert_eq!(find(&rows, "Claude Code").state, State::NotHere);
        for (agent, _) in setup::USER_NOT_APPLICABLE {
            assert_eq!(
                find(&rows, agent).state,
                State::NotApplicable,
                "{agent} has no user-level file this tool writes"
            );
        }
    }

    #[test]
    fn an_na_row_always_says_why() {
        // "n/a" with no reason reads as "this agent has nothing", which for
        // Windsurf is false — it has a global file we decline to write.
        let home = temp();
        let rows = user_rows_in(
            home.path(),
            &home.path().join(".vibe"),
            &home.path().join(".config/opencode"),
        );
        for row in rows.iter().filter(|r| r.state == State::NotApplicable) {
            assert!(!row.detail.trim().is_empty(), "{} has no reason", row.agent);
        }
        assert!(find(&rows, "Windsurf").detail.contains("global_rules.md"));
    }

    #[test]
    fn a_relocated_user_directory_is_shown_absolute() {
        // `show` strips the scope root; a $VIBE_HOME outside it cannot be
        // stripped, and printing a bare relative tail would be a lie.
        let home = temp();
        let elsewhere = temp();
        let vibe = elsewhere.path().join("vibe-state");
        std::fs::create_dir_all(&vibe).unwrap();
        let rows = user_rows_in(home.path(), &vibe, &home.path().join(".config/opencode"));
        let detail = &find(&rows, "Mistral Vibe").detail;
        assert!(
            detail.contains("vibe-state"),
            "a relocated directory must be shown in full, not as a bare tail: {detail}"
        );
    }

    // --- the encoder -------------------------------------------------------

    #[test]
    fn the_encoder_state_matches_the_build() {
        let e = encoder_status();
        if cfg!(feature = "semantic") {
            assert_ne!(
                e.state(),
                State::NotApplicable,
                "a semantic build always has a cache directory to report on"
            );
        } else {
            assert_eq!(e, Encoder::Unavailable);
            assert_eq!(e.state(), State::NotApplicable);
        }
    }

    #[test]
    fn dir_size_sums_a_tree_and_survives_a_missing_one() {
        let t = temp();
        std::fs::write(t.path().join("a"), vec![0u8; 100]).unwrap();
        std::fs::create_dir(t.path().join("sub")).unwrap();
        std::fs::write(t.path().join("sub/b"), vec![0u8; 28]).unwrap();
        assert_eq!(dir_size(t.path()), 128);
        assert_eq!(dir_size(&t.path().join("nope")), 0);
    }

    #[test]
    fn status_writes_nothing() {
        // The one property that makes this safe to run anywhere.
        let t = temp();
        let listing = |d: &Path| {
            let mut names: Vec<_> = std::fs::read_dir(d)
                .unwrap()
                .flatten()
                .map(|e| e.file_name())
                .collect();
            names.sort();
            names
        };

        let before = listing(t.path());
        let _ = project_rows(t.path());
        let _ = user_rows_in(
            t.path(),
            &t.path().join(".vibe"),
            &t.path().join(".config/opencode"),
        );
        let _ = encoder_status();

        assert_eq!(before, listing(t.path()));
        assert!(!t.path().join(".vibe").exists());
        assert!(!t.path().join(".claude").exists());
        assert!(!t.path().join(".config").exists());
    }
}
