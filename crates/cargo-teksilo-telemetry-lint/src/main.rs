// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `cargo teksilo-telemetry-lint` — schema-drift linter for teksilo-telemetry
//! event manifests.
//!
//! Run as a cargo subcommand: `cargo teksilo-telemetry-lint`
//! Install via: `cargo install --path crates/cargo-teksilo-telemetry-lint`
//!
//! # Checks performed
//!
//! 1. **Manifest parse** — catches YAML syntax errors before anything else.
//! 2. **Required fields** — `expires`, `bug`, `description`, `category`.
//! 3. **Valid category** — one of `intent | lifecycle | navigation | census | custom`.
//! 4. **Duplicate event / prop names**.
//! 5. **Unknown prop types**.
//! 6. **`enum` props with empty `values`**.
//! 7. **`expires` in the past** (warning, configurable with `--fail-on-warnings`).
//! 8. **Unused events** — declared in manifest but no call site in `src/`.
//!
//! The command line itself is declared by [`Cli`], so
//! `cargo teksilo-telemetry-lint --help` is generated from the same place the
//! program reads its options — there is no second copy to drift.

use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;

mod checks;
mod manifest;

/// Keeps the uppercase section headings this tool has always printed.
///
/// clap 4 renders `Usage:` / `Options:` in sentence case; the linter's output
/// is read in CI logs beside its sibling `cargo teksilo-fmt`, so both pin the
/// established shape rather than changing it as a side effect of a parser swap.
const HELP_TEMPLATE: &str = "\
{about-with-newline}
USAGE:
    {usage}

OPTIONS:
{options}{after-help}";

/// The checks and worked invocations, kept out of the flag list.
///
/// The numbered list is the linter's contract with a manifest author: it says
/// what a clean run has actually established, which no per-flag help text can.
const AFTER_HELP: &str = "\
CHECKS:
    1. YAML parse + schema-version coherence
    2. Required fields: expires, bug, description, category
    3. Valid category (intent|lifecycle|navigation|census|custom)
    4. Duplicate event / prop names
    5. Unknown prop types
    6. enum props without values list
    7. expires past today (warning)
    8. Declared events with no emit_* call sites in src/ (warning)

EXAMPLES:
    cargo teksilo-telemetry-lint
    cargo teksilo-telemetry-lint --manifest telemetry/app_events.yaml --src src --src lib
    cargo teksilo-telemetry-lint --fail-on-warnings   # for CI";

const LONG_ABOUT: &str = "\
cargo teksilo-telemetry-lint — Teksilo telemetry schema drift linter

Reads an events.yaml manifest and the sources that emit against it, and reports
where the two have drifted apart: a manifest that no longer parses, an event
missing a field the codegen requires, a prop type nothing can encode, an expiry
date that has passed, an event nobody emits any more.

Errors always fail the run. Warnings — an expired event, an unused one — do not,
unless --fail-on-warnings is given.";

/// The whole command line.
///
/// A single-level command rather than the `Cargo { Teksilo(..) }` enum
/// `cargo teksilo` uses: that idiom makes the subcommand word mandatory, and
/// this binary has always run directly as `cargo-teksilo-telemetry-lint …`
/// too. [`strip_cargo_subcommand`] removes the word when cargo supplies it;
/// `bin_name` keeps the generated help reading `cargo teksilo-telemetry-lint …`
/// either way.
#[derive(Debug, Parser)]
#[command(
    name = "cargo-teksilo-telemetry-lint",
    bin_name = "cargo teksilo-telemetry-lint",
    about = "cargo teksilo-telemetry-lint — Teksilo telemetry schema drift linter",
    long_about = LONG_ABOUT,
    help_template = HELP_TEMPLATE,
    after_help = AFTER_HELP
)]
struct Cli {
    /// Path to the events.yaml manifest.
    ///
    /// Relative to the current directory, so the linter is normally run from
    /// the crate that owns the manifest.
    #[arg(long, value_name = "PATH", default_value = "telemetry/events.yaml")]
    manifest: PathBuf,

    /// Source dir to scan for `emit_*` call sites (repeatable).
    ///
    /// Only the unused-event check reads these. Repeat the flag for a crate
    /// whose emitters are split across roots — `--src src --src lib` — or an
    /// event emitted from the second root is reported as emitted from nowhere.
    #[arg(long, value_name = "DIR", default_value = "src")]
    src: Vec<PathBuf>,

    /// Exit 1 when warnings are present (CI mode).
    ///
    /// Warnings are the drift that has not broken anything yet: an expiry date
    /// that has passed, an event with no call site left. They are worth failing
    /// a pipeline over and not worth failing a local run over, which is why
    /// this is a flag rather than the default.
    #[arg(long)]
    fail_on_warnings: bool,

    /// Output findings as newline-delimited JSON.
    ///
    /// One object per finding — `severity`, `location`, `message` — with the
    /// summary line suppressed, for a tool reading this instead of a human.
    #[arg(long)]
    json: bool,

    /// Suppress the summary line.
    ///
    /// The per-finding lines are still printed, and the exit code is unchanged.
    #[arg(long, short)]
    quiet: bool,
}

fn main() {
    let cli = Cli::parse_from(strip_cargo_subcommand(std::env::args().collect()));
    run(cli);
}

/// Drop the `teksilo-telemetry-lint` word cargo passes as `argv[1]`.
///
/// `cargo teksilo-telemetry-lint --json` reaches this binary as
/// `[cargo-teksilo-telemetry-lint, teksilo-telemetry-lint, --json]`; run
/// directly it is `[cargo-teksilo-telemetry-lint, --json]`. Both forms work,
/// so the word is removed only when it is actually the first argument.
/// `argv[0]` stays: clap reads the whole vector, program name included.
fn strip_cargo_subcommand(mut argv: Vec<String>) -> Vec<String> {
    if argv
        .get(1)
        .is_some_and(|arg| arg == "teksilo-telemetry-lint")
    {
        argv.remove(1);
    }
    argv
}

fn run(config: Cli) {
    let manifest_path = config.manifest;
    let src_dirs = config.src;

    // Read manifest.
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            emit_error_line(
                config.json,
                &format!("{}", manifest_path.display()),
                &format!("cannot read manifest: {e}"),
            );
            process::exit(1);
        }
    };

    // Parse.
    let schema = match manifest::parse_schema(&content) {
        Ok(s) => s,
        Err(e) => {
            emit_error_line(config.json, &manifest_path.display().to_string(), &e);
            process::exit(1);
        }
    };

    // Run checks.
    let src_refs: Vec<&Path> = src_dirs.iter().map(PathBuf::as_path).collect();
    let issues = checks::run_checks(&schema, &src_refs, config.fail_on_warnings);

    // Emit findings.
    let error_count = issues
        .iter()
        .filter(|i| i.severity == checks::Severity::Error)
        .count();
    let warning_count = issues
        .iter()
        .filter(|i| i.severity == checks::Severity::Warning)
        .count();

    for issue in &issues {
        if config.json {
            println!(
                "{{\"severity\":\"{}\",\"location\":\"{}\",\"message\":\"{}\"}}",
                if issue.severity == checks::Severity::Error {
                    "error"
                } else {
                    "warning"
                },
                issue.location,
                issue.message.replace('"', "\\\""),
            );
        } else {
            let prefix = if issue.severity == checks::Severity::Error {
                "\x1b[31merror\x1b[0m"
            } else {
                "\x1b[33mwarning\x1b[0m"
            };
            println!("{prefix} [{}]: {}", issue.location, issue.message);
        }
    }

    if !config.quiet {
        let manifest_name = manifest_path.display();
        if issues.is_empty() {
            println!("\x1b[32m✓\x1b[0m {manifest_name}: no issues found");
        } else {
            println!(
                "\x1b[31m✗\x1b[0m {manifest_name}: {error_count} error(s), {warning_count} warning(s)"
            );
        }
    }

    let should_fail = error_count > 0 || (config.fail_on_warnings && warning_count > 0);
    if should_fail {
        process::exit(1);
    }
}

fn emit_error_line(json: bool, location: &str, message: &str) {
    if json {
        println!(
            "{{\"severity\":\"error\",\"location\":\"{location}\",\"message\":\"{message}\"}}"
        );
    } else {
        eprintln!("\x1b[31merror\x1b[0m [{location}]: {message}");
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::CommandFactory;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn strips_the_cargo_subcommand_word() {
        assert_eq!(
            strip_cargo_subcommand(argv(&[
                "cargo-teksilo-telemetry-lint",
                "teksilo-telemetry-lint",
                "--json"
            ])),
            argv(&["cargo-teksilo-telemetry-lint", "--json"])
        );
        assert_eq!(
            strip_cargo_subcommand(argv(&["cargo-teksilo-telemetry-lint", "--json"])),
            argv(&["cargo-teksilo-telemetry-lint", "--json"])
        );
    }

    #[test]
    fn defaults_match_the_documented_ones() {
        let cli = Cli::try_parse_from(argv(&["cargo-teksilo-telemetry-lint"])).unwrap();
        assert_eq!(cli.manifest, PathBuf::from("telemetry/events.yaml"));
        assert_eq!(cli.src, vec![PathBuf::from("src")]);
        assert!(!cli.fail_on_warnings && !cli.json && !cli.quiet);
    }

    #[test]
    fn src_is_repeatable_and_replaces_the_default() {
        let cli = Cli::try_parse_from(argv(&[
            "cargo-teksilo-telemetry-lint",
            "--src",
            "src",
            "--src",
            "lib",
        ]))
        .unwrap();
        assert_eq!(cli.src, vec![PathBuf::from("src"), PathBuf::from("lib")]);
    }

    #[test]
    fn a_typo_for_a_flag_is_rejected_rather_than_silently_ignored() {
        // The hand-rolled parser dropped stray positionals, so
        // `cargo teksilo-telemetry-lint fail-on-warnings` — the flag with its
        // dashes forgotten — printed "no issues found" and exited 0. In CI that
        // reads as a gate that passed, when it is a gate that never ran. That is
        // exactly the failure this lint exists to catch, so it is now an error.
        let err = Cli::try_parse_from(argv(&["cargo-teksilo-telemetry-lint", "fail-on-warnings"]))
            .expect_err("a stray positional must not be accepted");
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
        // Note what clap does *not* do here: a bare word gets no "did you mean"
        // suggestion, because clap only offers one for `--typo` forms. The value
        // is the refusal itself — the run stops instead of reporting success.
        assert!(
            err.to_string().contains("fail-on-warnings"),
            "the error should name the offending argument, got: {err}"
        );
    }
}
