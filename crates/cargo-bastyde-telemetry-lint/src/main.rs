//! `cargo bastyde-telemetry-lint` — schema-drift linter for bastyde-telemetry
//! event manifests.
//!
//! Run as a cargo subcommand: `cargo bastyde-telemetry-lint`
//! Install via: `cargo install --path crates/cargo-bastyde-telemetry-lint`
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
//! # Usage
//!
//! ```text
//! cargo bastyde-telemetry-lint [OPTIONS]
//!
//! Options:
//!   --manifest <PATH>         Path to the events.yaml manifest
//!                             [default: telemetry/events.yaml]
//!   --src <DIR>               Source directory to scan for emit_* calls
//!                             (can be repeated; default: src)
//!   --fail-on-warnings        Exit with non-zero when warnings are present
//!   --json                    Output findings as JSON (one object per line)
//!   --quiet                   Suppress summary line
//! ```

use std::path::{Path, PathBuf};
use std::process;

mod checks;
mod manifest;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // When invoked as `cargo bastyde-telemetry-lint`, Cargo passes the
    // subcommand name as the first arg. Skip it.
    let args = skip_cargo_subcommand(&args);

    let config = match Config::parse(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Run with --help for usage.");
            process::exit(2);
        }
    };

    if config.help {
        print_help();
        return;
    }

    run(config);
}

fn skip_cargo_subcommand(args: &[String]) -> &[String] {
    // `cargo bastyde-telemetry-lint` → argv[0]=binary, argv[1]="bastyde-telemetry-lint"
    if args
        .get(1)
        .map(|s| s == "bastyde-telemetry-lint")
        .unwrap_or(false)
    {
        &args[2..]
    } else if args.len() > 1 {
        &args[1..]
    } else {
        &[]
    }
}

#[derive(Debug, Default)]
struct Config {
    manifest_path: Option<PathBuf>,
    src_dirs: Vec<PathBuf>,
    fail_on_warnings: bool,
    json_output: bool,
    quiet: bool,
    help: bool,
}

impl Config {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut cfg = Config::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--help" | "-h" => cfg.help = true,
                "--fail-on-warnings" => cfg.fail_on_warnings = true,
                "--json" => cfg.json_output = true,
                "--quiet" | "-q" => cfg.quiet = true,
                "--manifest" => {
                    let path = iter.next().ok_or("--manifest requires a path")?;
                    cfg.manifest_path = Some(PathBuf::from(path));
                }
                "--src" => {
                    let dir = iter.next().ok_or("--src requires a directory")?;
                    cfg.src_dirs.push(PathBuf::from(dir));
                }
                other if other.starts_with("--") => {
                    return Err(format!("unknown option: {other}"));
                }
                _ => {} // ignore positional args
            }
        }
        Ok(cfg)
    }
}

fn run(config: Config) {
    // Resolve manifest path.
    let manifest_path = config
        .manifest_path
        .unwrap_or_else(|| PathBuf::from("telemetry/events.yaml"));

    // Resolve source directories.
    let src_dirs: Vec<PathBuf> = if config.src_dirs.is_empty() {
        vec![PathBuf::from("src")]
    } else {
        config.src_dirs
    };

    // Read manifest.
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            emit_error_line(
                config.json_output,
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
            emit_error_line(config.json_output, &manifest_path.display().to_string(), &e);
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
        if config.json_output {
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

fn print_help() {
    println!(
        r#"cargo bastyde-telemetry-lint — Bastyde telemetry schema drift linter

USAGE:
    cargo bastyde-telemetry-lint [OPTIONS]

OPTIONS:
    --manifest <PATH>       Path to events.yaml [default: telemetry/events.yaml]
    --src <DIR>             Source dir to scan (repeatable) [default: src]
    --fail-on-warnings      Exit 1 when warnings are present (CI mode)
    --json                  Output findings as newline-delimited JSON
    --quiet, -q             Suppress summary line
    --help, -h              Print this help

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
    cargo bastyde-telemetry-lint
    cargo bastyde-telemetry-lint --manifest telemetry/app_events.yaml --src src --src lib
    cargo bastyde-telemetry-lint --fail-on-warnings   # for CI
"#
    );
}
