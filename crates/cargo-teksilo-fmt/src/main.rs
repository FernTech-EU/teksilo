// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `cargo teksilo-fmt` — formatter for `teksu!` DSL blocks in Rust source.
//!
//! Run as a cargo subcommand: `cargo teksilo-fmt [paths...]`
//! Install via: `cargo install --path crates/cargo-teksilo-fmt`
//!
//! # Modes
//!
//! - **Format in place** (default): rewrite each `.rs` file whose
//!   `teksu!` blocks aren't canonical. Writes are atomic (write-temp +
//!   rename via `tempfile::NamedTempFile`).
//! - **`--check`**: don't write; print `Would reformat: <path>` for
//!   every file that would change and exit 1 if any. CI mode.
//!
//! Source outside `teksu!(...)` blocks is left byte-for-byte unchanged
//! — `cargo fmt` owns Rust formatting.
//!
//! The command line itself is declared by [`Config`], so
//! `cargo teksilo-fmt --help` is generated from the same place the
//! program reads its options — there is no second copy to drift.

use std::path::PathBuf;
use std::process;

use clap::Parser;

mod run;
mod walk;

/// Keeps the uppercase section headings this tool has always printed.
///
/// clap 4 renders `Usage:` / `Options:` in sentence case. Those headings are
/// observable output — `tests/e2e.rs::help_runs` asserts on `USAGE` — so the
/// template pins the established shape rather than letting the help text
/// change as a side effect of swapping the parser.
const HELP_TEMPLATE: &str = "\
{about-with-newline}
USAGE:
    {usage}

OPTIONS:
{options}

POSITIONAL:
{positionals}{after-help}";

const AFTER_HELP: &str = "\
EXAMPLES:
    cargo teksilo-fmt                            # format from CWD
    cargo teksilo-fmt --check                    # CI mode
    cargo teksilo-fmt examples/widget_catalog    # format one example
    cargo teksilo-fmt src/main.rs                # format one file";

const LONG_ABOUT: &str = "\
cargo teksilo-fmt — formatter for teksu! DSL blocks

Rewrites every teksu!(...) block in the files it is given into canonical form.
Source outside those blocks is left byte-for-byte unchanged — cargo fmt owns
Rust formatting. Writes are atomic (write-temp + rename), so an interrupted run
never leaves a truncated source file behind.";

/// The whole command line, and the configuration [`run::run`] reads.
///
/// One type rather than a parser plus a config struct: the options *are* the
/// configuration, and a second type would only be somewhere for the two to
/// disagree.
///
/// Note this is a single-level command, not the `Cargo { Teksilo(..) }` enum
/// `cargo teksilo` uses. That idiom makes the subcommand word mandatory, and
/// this binary has always run directly as `cargo-teksilo-fmt …` too — which is
/// how `tests/e2e.rs` drives it. [`strip_cargo_subcommand`] removes the word
/// when cargo supplies it; `bin_name` keeps the generated help reading
/// `cargo teksilo-fmt …` either way.
#[derive(Debug, Parser)]
#[command(
    name = "cargo-teksilo-fmt",
    bin_name = "cargo teksilo-fmt",
    version,
    about = "cargo teksilo-fmt — formatter for teksu! DSL blocks",
    long_about = LONG_ABOUT,
    help_template = HELP_TEMPLATE,
    after_help = AFTER_HELP
)]
pub(crate) struct Config {
    /// Files or directories to format.
    ///
    /// Directories are walked recursively for `*.rs`, skipping `target/`.
    /// A path that does not exist is an error, not a silent skip.
    #[arg(value_name = "paths", default_value = ".")]
    pub paths: Vec<PathBuf>,

    /// Read-only; exit 1 if any file would change.
    ///
    /// The CI mode. Nothing is written; every file whose `teksu!` blocks are
    /// not canonical is printed as `Would reformat: <path>`.
    #[arg(long)]
    pub check: bool,

    /// Suppress per-file output.
    ///
    /// Errors are still printed, and a quiet run that hit an unparseable file
    /// still exits 1 — quiet hides the inventory, never a failure.
    #[arg(long, short)]
    pub quiet: bool,
}

fn main() {
    let cfg = parse(std::env::args().collect());
    let outcome = run::run(&cfg);
    process::exit(outcome.exit_code());
}

/// Parse `argv`, or let clap report the usage error and exit.
///
/// Every diagnostic is clap's, verbatim. An earlier version of this re-worded
/// the unrecognised-flag error back to this tool's historical "unknown option:
/// --bogus" because `tests/e2e.rs` asserted that string — but the test was
/// pinning an implementation detail rather than a contract, and the shim left
/// this binary disagreeing with its sibling `cargo-teksilo-telemetry-lint` for
/// no reason a user benefits from. clap's wording also suggests the near-miss
/// flag, which the hand-written message never did.
///
/// Exit codes are clap's and already match what this tool always used: 2 for a
/// usage error, 0 for `--help` and `--version`.
fn parse(argv: Vec<String>) -> Config {
    Config::parse_from(strip_cargo_subcommand(argv))
}

/// Drop the `teksilo-fmt` word cargo passes as `argv[1]`.
///
/// `cargo teksilo-fmt --check` reaches this binary as
/// `[cargo-teksilo-fmt, teksilo-fmt, --check]`; run directly it is
/// `[cargo-teksilo-fmt, --check]`. Both forms work, so the word is removed
/// only when it is actually the first argument. `argv[0]` stays: clap reads
/// the whole vector, program name included.
fn strip_cargo_subcommand(mut argv: Vec<String>) -> Vec<String> {
    if argv.get(1).is_some_and(|arg| arg == "teksilo-fmt") {
        argv.remove(1);
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn command_definition_is_valid() {
        Config::command().debug_assert();
    }

    #[test]
    fn strips_the_cargo_subcommand_word() {
        assert_eq!(
            strip_cargo_subcommand(argv(&["cargo-teksilo-fmt", "teksilo-fmt", "--check"])),
            argv(&["cargo-teksilo-fmt", "--check"])
        );
    }

    #[test]
    fn keeps_a_direct_invocation_intact() {
        assert_eq!(
            strip_cargo_subcommand(argv(&["cargo-teksilo-fmt", "--check"])),
            argv(&["cargo-teksilo-fmt", "--check"])
        );
        assert_eq!(
            strip_cargo_subcommand(argv(&["cargo-teksilo-fmt"])),
            argv(&["cargo-teksilo-fmt"])
        );
    }

    #[test]
    fn no_paths_means_the_current_directory() {
        let cfg = Config::try_parse_from(argv(&["cargo-teksilo-fmt"])).unwrap();
        assert_eq!(cfg.paths, vec![PathBuf::from(".")]);
        assert!(!cfg.check);
        assert!(!cfg.quiet);
    }

    #[test]
    fn flags_and_paths_mix_in_any_order() {
        let cfg = Config::try_parse_from(argv(&[
            "cargo-teksilo-fmt",
            "a.rs",
            "--check",
            "-q",
            "b.rs",
        ]))
        .unwrap();
        assert_eq!(
            cfg.paths,
            vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]
        );
        assert!(cfg.check);
        assert!(cfg.quiet);
    }

    #[test]
    fn an_unrecognised_flag_is_refused_and_named() {
        // The contract is refusal, not wording: this used to assert a
        // hand-rolled "unknown option" message that only existed because an
        // e2e test pinned it. clap's own error names the flag and suggests the
        // near miss, which the replaced message never did.
        let err = Config::try_parse_from(argv(&["cargo-teksilo-fmt", "--bogus"])).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
        assert!(err.to_string().contains("--bogus"), "got: {err}");
    }
}
