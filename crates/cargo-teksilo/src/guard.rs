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
/// Three regimes, decided by the app's version:
///
/// 1. below [`FIRST_PUBLISHED`] — no install command at all, because all three
///    routes are dead at those tags;
/// 2. at or above it — both routes, unconditionally, because publication is
///    not observable offline (a `path` or `git` dep resolves a version that is
///    in no registry, and a private registry breaks the branch the other way);
/// 3. unparseable — no `--version` can be built from it, so only the checkout
///    routes are offered.
pub fn refusal_text(app: &str, tool: &str, what: &str, sources: Option<&Path>) -> String {
    let (f_major, f_minor, f_patch) = FIRST_PUBLISHED;
    let floor = format!("{f_major}.{f_minor}.{f_patch}");

    let head = format!(
        "cargo-teksilo {tool} cannot serve {what} for an app on teksilo {app}.\n\
         \n\
         The public API changed between these versions, so answering would mean\n\
         guessing."
    );

    let body = match semver_parts(app) {
        Some(parts) if parts < FIRST_PUBLISHED => format!(
            "There is no matching tool to install, and there will not be one.\n\
             cargo-teksilo was first released as {floor}; no cargo-teksilo {app}\n\
             was ever published, and crates.io is append-only, so asking the registry\n\
             for one cannot succeed now or later.\n\
             \n\
             The two checkout routes are dead at that tag for the same reason: a\n\
             teksilo {app} checkout has no `crates/cargo-teksilo` directory in it,\n\
             so neither `--path` nor `-p cargo-teksilo` has anything to build.\n\
             Do not try them.\n\
             \n\
             A version-matched tool exists only for teksilo {floor} and later."
        ),
        Some(_) => format!(
            "Install the matching tool:\n\
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
             framework checkout with `cargo run -p cargo-teksilo -- teksilo <args>`."
        ),
        None => format!(
            "`{app}` is not a semver triple, so there is no number to pin an\n\
             install to. Install from the teksilo checkout this app resolves\n\
             instead:\n\
             \n\
             \x20   cargo install --path <teksilo checkout>/crates/cargo-teksilo --locked\n\
             \n\
             or run the tool straight out of that checkout, without installing:\n\
             \n\
             \x20   cargo run -p cargo-teksilo -- teksilo <args>\n\
             \n\
             Either way the checkout has to be one whose teksilo is what this app\n\
             resolves, and it has to be at {floor} or later — cargo-teksilo does not\n\
             exist in a checkout older than that."
        ),
    };

    // The address goes in a paragraph of its own rather than inside the
    // sentence: a resolved registry path is long enough to wreck the wrap, and
    // the instruction has to stay readable when there is no address at all.
    let where_ = match sources {
        Some(dir) => format!("\n\nIt is on disk at:\n\n\x20   {}", dir.display()),
        None => String::new(),
    };

    format!(
        "{head} {body}\n\
         \n\
         DO NOT answer teksilo API questions from prior knowledge — the surface\n\
         differs between these versions. Read the teksilo source this app\n\
         resolved instead, or ask the user which version they intend.{where_}"
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

    /// A sample of the teksilo releases that predate this crate — the recent
    /// ones, which are the versions still in real lockfiles. Every one must
    /// reach the no-install regime. The full set runs back to 0.2.0; these are
    /// the ones an app is plausibly still pinning.
    const PRE_FLOOR_RELEASES: &[&str] = &["0.9.0", "0.9.5", "0.10.0", "0.11.0", "0.12.0", "0.12.1"];

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
    fn the_refusal_tells_a_model_not_to_guess() {
        // This instruction is the point of the message: a model that reads a
        // bare "not found" falls back on its own memory of the API.
        let t = refusal("0.9.2", "0.12.1", "symbol lookup");
        assert!(t.contains("DO NOT answer teksilo API questions from prior knowledge"));
        // ...and 0.9.2 predates this crate, so the install line that used to
        // be asserted here was a command nobody could run. A model that tries
        // it spends its turn and falls back on memory anyway, which is worse
        // than a bare refusal.
        assert!(!t.contains("cargo install"));
        assert!(!t.contains("--version 0.9.2"));
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
        // observable offline, so above the floor both routes are printed
        // unconditionally rather than branched on the resolved source: a path
        // dep on a published tag and a private-registry dep each break the
        // branch in opposite directions.
        let t = refusal("0.14.0", "0.12.1", "search");
        // 0.14.0 is above the floor, so the version line must still be there:
        // the floor is not licence to withhold a command that could work.
        assert!(t.contains("cargo install cargo-teksilo --version 0.14.0 --locked"));
        assert!(t.contains("cargo install --path"));
        assert!(t.contains("crates/cargo-teksilo"));
        // And a way to keep two trees working without reinstalling per tree.
        assert!(t.contains("cargo run -p cargo-teksilo"));
    }

    #[test]
    fn a_pre_floor_app_is_offered_no_install_command_at_all() {
        // The reviewer's case, and the reason this regime exists: Skribisto
        // pins 0.12.1, and all three remedies the message used to print are
        // impossible for it.
        let t = refusal("0.12.1", TOOL_VERSION, "symbol lookup");
        assert!(!t.contains("cargo install"), "{t}");
        assert!(!t.contains("cargo run -p cargo-teksilo -- teksilo"), "{t}");
        // It names the floor, so the reader learns WHY rather than just being
        // told no, and knows what a working setup would look like.
        assert!(t.contains("0.13.0"), "{t}");
        // And it says the checkout routes are dead too, so the model does not
        // reach for the obvious next idea on its own.
        assert!(t.contains("crates/cargo-teksilo"), "{t}");
        assert!(t.contains("Do not try them"), "{t}");
    }

    #[test]
    fn every_published_pre_floor_teksilo_reaches_that_regime() {
        for app in PRE_FLOOR_RELEASES {
            let t = refusal(app, TOOL_VERSION, "search");
            assert!(!t.contains("cargo install"), "{app}: {t}");
            assert!(
                t.contains("DO NOT answer teksilo API questions from prior knowledge"),
                "{app}"
            );
        }
    }

    #[test]
    fn an_unparseable_version_yields_no_version_flag() {
        for app in ["not-a-version", "", "1.2.3.4", "main"] {
            let t = refusal(app, TOOL_VERSION, "search");
            assert!(!t.contains("--version"), "{app}: {t}");
            // But it is not a dead end: the checkout routes need no version.
            assert!(t.contains("cargo install --path"), "{app}: {t}");
            assert!(t.contains("cargo run -p cargo-teksilo"), "{app}: {t}");
        }
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
    fn a_confirmed_source_directory_is_named() {
        let dir = Path::new("/home/dev/teksilo");
        let t = refusal_text("0.12.1", TOOL_VERSION, "search", Some(dir));
        assert!(t.contains("/home/dev/teksilo"), "{t}");
        // Without one the sentence still reads, it just has no address.
        let bare = refusal("0.12.1", TOOL_VERSION, "search");
        assert!(bare.contains("Read the teksilo source this app"), "{bare}");
        assert!(!bare.contains("It is on disk at"), "{bare}");
    }

    #[test]
    fn the_degraded_note_names_both_versions() {
        let t = degraded_text("0.12.0", "0.12.1");
        assert!(t.contains("0.12.0") && t.contains("0.12.1"));
    }
}
