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
//!
//! And it is why a remedy that cannot work is worse than no remedy: the model
//! spends its turn on the dead command and *then* falls back on memory. That
//! is what [`FIRST_PUBLISHED`] exists to prevent.

use std::path::{Path, PathBuf};

/// The version of teksilo this binary was built to serve.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The first teksilo version for which a matching cargo-teksilo exists.
///
/// This tool is younger than the framework it serves. Teksilo shipped nineteen
/// tagged releases — 0.2.0 through 0.12.1 — before `crates/cargo-teksilo` was
/// written at all, so for an app on any of them there is no version-matched tool, and
/// there never will be: crates.io is append-only, and this crate is not going
/// to be back-published against tags that predate it. A checkout at one of
/// those tags has no `crates/cargo-teksilo` directory either, so `--path` and
/// `cargo run -p` are just as dead there as `cargo install --version`.
///
/// Nothing in the build graph can stand in for this constant. `CARGO_PKG_VERSION`
/// is the version being released *today* and moves every release; this floor
/// is a fact about history and never moves again once 0.13.0 ships.
///
/// If ever in doubt, err HIGH. Too high withholds a command that would have
/// worked, and the reader can still try it. Too low prints a command that
/// cannot work, which is the defect this constant was introduced to fix.
pub const FIRST_PUBLISHED: (u64, u64, u64) = (0, 13, 0);

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

/// A teksilo source tree on this machine that the caller has confirmed exists.
///
/// The refusal tells the reader to go read the source instead of guessing, and
/// that instruction is worth more with an address on it. It has to be an
/// address that is really there: a resolved registry dependency has a `dir`
/// long before anything has been fetched into it, and naming an empty path
/// would reproduce, in miniature, the very defect of printing a command that
/// cannot work. So both branches are existence-checked — `checkout_root`
/// requires `tools/` and `crates/teksilo-widgets/`, `has_src` requires the
/// package's `src/` to be a directory.
pub fn readable_sources(resolution: &crate::resolve::Resolution) -> Option<PathBuf> {
    if let Some(root) = resolution.checkout_root() {
        return Some(root);
    }
    let widgets = resolution.widgets()?;
    widgets.has_src().then(|| widgets.dir.clone())
}

/// The message printed when a command refuses.
///
/// `what` names the thing that is unavailable, e.g. `"symbol lookup"`.
/// `sources` is a teksilo source tree on disk, if the caller found one — see
/// [`readable_sources`].
///
/// Default output is a diagnosis and one remedy. Verbose output adds source
/// paths and a checkout install option for unpublished versions.
pub fn refusal_text(app: &str, tool: &str, what: &str, sources: Option<&Path>) -> String {
    let help = match semver_parts(app) {
        Some(parts) if parts < FIRST_PUBLISHED => {
            "use the resolved crate sources, or upgrade to Teksilo 0.13.0 or later".to_string()
        }
        Some(_) => format!("cargo install cargo-teksilo --version {app} --locked"),
        None => "install cargo-teksilo from the matching framework checkout".to_string(),
    };
    let mut message = format!("tool {tool} cannot serve {what} for Teksilo {app}\nhelp: {help}");
    if crate::output::verbose() {
        if let Some(path) = sources {
            message.push_str(&format!("\nSources: {}", path.display()));
        }
        message.push_str("\nFor unpublished versions: cargo install --path <checkout>/crates/cargo-teksilo --locked");
    }
    message
}

/// The note printed when a command answers across a patch-level difference.
pub fn degraded_text(app: &str, tool: &str) -> String {
    format!("note: app uses Teksilo {app}; tool uses {tool} (compatible patch versions)")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(app: &str, tool: &str, what: &str) -> String {
        refusal_text(app, tool, what, None)
    }

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
    fn refusals_give_one_usable_remedy() {
        let old = refusal("0.12.1", TOOL_VERSION, "search");
        assert!(!old.contains("cargo install"));
        assert!(old.contains("0.13.0"));
        let published = refusal("0.14.0", "0.13.0", "search");
        assert!(published.contains("cargo install cargo-teksilo --version 0.14.0 --locked"));
        assert_eq!(published.lines().count(), 2);
        assert!(!refusal("unknown", TOOL_VERSION, "search").contains("--version"));
    }

    #[test]
    fn the_floor_is_never_above_this_binarys_own_version() {
        // Catches the tempting edit of bumping FIRST_PUBLISHED to "the next
        // release" — which would make THIS binary claim it does not exist,
        // refusing with a message saying no matched tool is available while
        // being the matched tool.
        let tool = semver_parts(TOOL_VERSION).expect("this crate's own version is a semver triple");
        assert!(
            tool >= FIRST_PUBLISHED,
            "TOOL_VERSION {TOOL_VERSION} is below FIRST_PUBLISHED {FIRST_PUBLISHED:?}"
        );
    }

    #[test]
    fn the_degraded_note_names_both_versions() {
        let t = degraded_text("0.12.0", "0.12.1");
        assert!(t.contains("0.12.0") && t.contains("0.12.1"));
    }
}
