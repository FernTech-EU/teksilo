// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Refusing to answer for a teksilo this tool does not match.
//!
//! Serving 0.12 answers to an app on 0.9 is worse than serving nothing: it is
//! confidently wrong, which is the failure this whole tool exists to prevent.
//! Between those two versions `SplitView` was deleted outright in favour of
//! `Splitter` with no back-compat, and `ComponentStyleSlots` grew to 42 slots.
//! An answer drawn from the wrong one reads exactly like an answer drawn from
//! the right one.
//!
//! So the rule is: **never serve a version the consumer did not resolve.**
//! Degrade to a version-exact subset where one can be built (see
//! [`Verdict::Degraded`]); refuse only where answering would mean guessing.
//!
//! ## The refusal has two readers
//!
//! A human debugging their setup, and a model deciding what to do next. The
//! second is why the wording is load-bearing: a model that receives a bare
//! "not found" falls back on its own memory of the API, which is precisely the
//! hallucination this is meant to stop. So a refusal carries an instruction,
//! not just a diagnosis.

/// The version of teksilo this binary was built to serve.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a command may do, given the versions in play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Versions match: answer normally.
    Ok,
    /// Versions differ only in patch level: answer, but say so.
    ///
    /// Teksilo's own release history is the argument for this being a warning
    /// rather than a refusal — patch releases in this project are fixes, and
    /// refusing 0.12.0-vs-0.12.1 would make the tool useless the day after any
    /// point release without preventing a single wrong answer.
    Degraded { app: String, tool: String },
    /// Minor or major differ: refuse.
    Refuse { app: String, tool: String },
}

impl Verdict {
    /// Whether the command may produce an answer at all.
    ///
    /// The one question every command asks before doing any work.
    pub fn may_answer(&self) -> bool {
        !matches!(self, Verdict::Refuse { .. })
    }

    /// The note to print before answering, if any.
    ///
    /// `None` for an exact match; a warning for a patch-level difference.
    /// A refusal has no note — it has [`refusal_text`] instead.
    pub fn note(&self) -> Option<String> {
        match self {
            Verdict::Ok | Verdict::Refuse { .. } => None,
            Verdict::Degraded { app, tool } => Some(degraded_text(app, tool)),
        }
    }
}

/// Compare the app's resolved teksilo against this tool's version.
pub fn check(app_version: &str) -> Verdict {
    let tool = TOOL_VERSION;
    if app_version == tool {
        return Verdict::Ok;
    }
    match (semver_parts(app_version), semver_parts(tool)) {
        (Some((a_major, a_minor, _)), Some((t_major, t_minor, _)))
            if a_major == t_major && a_minor == t_minor =>
        {
            Verdict::Degraded {
                app: app_version.to_string(),
                tool: tool.to_string(),
            }
        }
        _ => Verdict::Refuse {
            app: app_version.to_string(),
            tool: tool.to_string(),
        },
    }
}

/// `1.2.3` / `1.2.3-rc.1` → `(1, 2, 3)`. `None` if it is not a semver triple.
fn semver_parts(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// The message printed when a command refuses.
///
/// `what` names the thing that is unavailable, e.g. `"symbol lookup"`.
pub fn refusal_text(app: &str, tool: &str, what: &str) -> String {
    format!(
        "cargo-teksilo {tool} cannot serve {what} for an app on teksilo {app}.\n\
         \n\
         The public API changed between these versions, so answering would mean\n\
         guessing. Install the matching tool:\n\
         \n\
         \x20   cargo install cargo-teksilo --version {app} --locked\n\
         \n\
         If that version was never published — the app pins teksilo by `path` or\n\
         `git`, which is normal for an app developed alongside the framework —\n\
         install from the checkout the app resolves instead:\n\
         \n\
         \x20   cargo install --path <teksilo checkout>/crates/cargo-teksilo --locked\n\
         \n\
         Both routes install ONE binary per machine, so switching between two\n\
         apps on different minors means reinstalling. To keep both, install the\n\
         second with `--root <dir>` and put that `<dir>/bin` first on PATH for\n\
         that tree, or skip installing and run the tool straight out of the\n\
         framework checkout with `cargo run -p cargo-teksilo -- teksilo <args>`.\n\
         \n\
         DO NOT answer teksilo API questions from prior knowledge — the surface\n\
         differs between these versions. Read the resolved source instead, or ask\n\
         the user which version they intend."
    )
}

/// The note printed when a command answers across a patch-level difference.
pub fn degraded_text(app: &str, tool: &str) -> String {
    format!(
        "note: this app resolved teksilo {app}, this tool is {tool}. \
         Answering anyway (same minor series); install \
         `cargo-teksilo --version {app}` for an exact match."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exact_match_answers() {
        assert_eq!(check(TOOL_VERSION), Verdict::Ok);
    }

    #[test]
    fn a_patch_difference_degrades_rather_than_refusing() {
        let (major, minor, patch) = semver_parts(TOOL_VERSION).unwrap();
        let other = format!("{major}.{minor}.{}", patch + 1);
        let v = check(&other);
        assert!(matches!(v, Verdict::Degraded { .. }), "got {v:?}");
        assert!(v.may_answer());
    }

    #[test]
    fn a_minor_difference_refuses() {
        let (major, minor, _) = semver_parts(TOOL_VERSION).unwrap();
        let other = format!("{major}.{}.0", minor + 1);
        let v = check(&other);
        assert!(matches!(v, Verdict::Refuse { .. }), "got {v:?}");
        assert!(!v.may_answer());
    }

    #[test]
    fn a_major_difference_refuses() {
        let (major, _, _) = semver_parts(TOOL_VERSION).unwrap();
        let v = check(&format!("{}.0.0", major + 1));
        assert!(matches!(v, Verdict::Refuse { .. }), "got {v:?}");
    }

    #[test]
    fn the_0_9_to_0_12_case_refuses() {
        // The concrete motivating case: SplitView was deleted between these.
        assert!(
            matches!(check("0.9.2"), Verdict::Refuse { .. }) || TOOL_VERSION.starts_with("0.9")
        );
    }

    #[test]
    fn unparseable_versions_refuse_rather_than_guess() {
        assert!(matches!(check("not-a-version"), Verdict::Refuse { .. }));
        assert!(matches!(check(""), Verdict::Refuse { .. }));
        assert!(matches!(check("1.2.3.4"), Verdict::Refuse { .. }));
    }

    #[test]
    fn prerelease_parses_to_its_core() {
        assert_eq!(semver_parts("2.0.0-rc.13"), Some((2, 0, 0)));
        assert_eq!(semver_parts("1.2.3+build"), Some((1, 2, 3)));
    }

    #[test]
    fn the_refusal_tells_a_model_not_to_guess() {
        // This instruction is the point of the message: a model that reads a
        // bare "not found" falls back on its own memory of the API.
        let t = refusal_text("0.9.2", "0.12.1", "symbol lookup");
        assert!(t.contains("DO NOT answer teksilo API questions from prior knowledge"));
        assert!(t.contains("cargo install cargo-teksilo --version 0.9.2 --locked"));
    }

    #[test]
    fn the_refusal_offers_a_route_for_an_unpublished_version() {
        // The crates.io line is the remedy for an app that resolved teksilo
        // from the registry. An app pinning the framework by `path` or `git`
        // — the normal shape for one developed alongside it — resolves a
        // version that was never published, so `cargo install --version` for
        // it fails with "could not find `cargo-teksilo` in registry". A model
        // reading a remedy that cannot work is back to guessing, which is the
        // one thing this message exists to prevent. Publication is not
        // observable offline, so both routes are printed unconditionally
        // rather than branched on the resolved source: a path dep on a
        // published tag and a private-registry dep each break the branch in
        // opposite directions.
        let t = refusal_text("0.14.0", "0.12.1", "search");
        assert!(t.contains("cargo install --path"));
        assert!(t.contains("crates/cargo-teksilo"));
        // And a way to keep two trees working without reinstalling per tree.
        assert!(t.contains("cargo run -p cargo-teksilo"));
    }

    #[test]
    fn the_degraded_note_names_both_versions() {
        let t = degraded_text("0.12.0", "0.12.1");
        assert!(t.contains("0.12.0") && t.contains("0.12.1"));
    }
}
