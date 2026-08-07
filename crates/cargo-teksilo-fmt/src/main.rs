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
//! # Usage
//!
//! ```text
//! cargo teksilo-fmt [paths...]
//!
//! Options:
//!   --check                Read-only; exit 1 if any file would change
//!   --quiet, -q            Suppress per-file output (errors still printed)
//!   --help, -h             Print this help
//!   --version, -V          Print version
//! ```
//!
//! Paths may be files or directories; directories are walked
//! recursively for `*.rs`, skipping `target/`. With no paths,
//! defaults to the current directory.

use std::path::PathBuf;
use std::process;

mod run;
mod walk;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let args = skip_cargo_subcommand(&args);

    let cfg = match Config::parse(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Run `cargo teksilo-fmt --help` for usage.");
            process::exit(2);
        }
    };

    if cfg.help {
        print_help();
        return;
    }
    if cfg.version {
        println!("cargo-teksilo-fmt {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let outcome = run::run(&cfg);
    process::exit(outcome.exit_code());
}

fn skip_cargo_subcommand(args: &[String]) -> &[String] {
    // Invoked as `cargo teksilo-fmt`: argv[0]=binary, argv[1]="teksilo-fmt"
    if args.get(1).map(|s| s == "teksilo-fmt").unwrap_or(false) {
        &args[2..]
    } else if args.len() > 1 {
        &args[1..]
    } else {
        &[]
    }
}

#[derive(Debug, Default)]
pub(crate) struct Config {
    pub paths: Vec<PathBuf>,
    pub check: bool,
    pub quiet: bool,
    pub help: bool,
    pub version: bool,
}

impl Config {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut cfg = Config::default();
        for arg in args {
            match arg.as_str() {
                "--help" | "-h" => cfg.help = true,
                "--version" | "-V" => cfg.version = true,
                "--check" => cfg.check = true,
                "--quiet" | "-q" => cfg.quiet = true,
                other if other.starts_with("--") => {
                    return Err(format!("unknown option: {other}"));
                }
                other if other.starts_with('-') && other.len() > 1 => {
                    return Err(format!("unknown short option: {other}"));
                }
                _ => cfg.paths.push(PathBuf::from(arg)),
            }
        }
        if cfg.paths.is_empty() {
            cfg.paths.push(PathBuf::from("."));
        }
        Ok(cfg)
    }
}

fn print_help() {
    println!(
        r#"cargo teksilo-fmt — formatter for teksu! DSL blocks

USAGE:
    cargo teksilo-fmt [OPTIONS] [paths...]

OPTIONS:
    --check                 Read-only; exit 1 if any file would change
    --quiet, -q             Suppress per-file output
    --help, -h              Print this help
    --version, -V           Print version

POSITIONAL:
    paths                   Files or directories to format. Directories
                            are walked recursively for *.rs, skipping
                            target/. Defaults to the current directory.

EXAMPLES:
    cargo teksilo-fmt                            # format from CWD
    cargo teksilo-fmt --check                    # CI mode
    cargo teksilo-fmt examples/widget_catalog    # format one example
    cargo teksilo-fmt src/main.rs                # format one file
"#
    );
}
