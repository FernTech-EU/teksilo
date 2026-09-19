// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `cargo teksilo` — agent tooling for apps built on the teksilo framework.
//!
//! An agent working inside the teksilo repository has the guides, the worked
//! examples, the skill and the automation harness. An agent working in
//! someone's teksilo *app* has none of them: `docs/` ships in no crate, every
//! example crate is `publish = false`, and the probe harness lives in one
//! app's repository. This binary closes that gap, version-matched to whatever
//! teksilo the app actually resolved.
//!
//! Invoked as a cargo subcommand (`cargo teksilo …`), cargo passes its own
//! name as `argv[1]`, so that is stripped before parsing.

mod guard;
mod probe;
mod resolve;
mod search;
mod setup;
mod symbol;
mod vectors;

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
cargo-teksilo — agent tooling for teksilo apps

USAGE:
    cargo teksilo <COMMAND> [ARGS...]

COMMANDS:
    symbol <Name>...    Exact public API of a type, for the teksilo version
                        this app pins. Accepts the extractor's own flags:
                        --list, --all, -f json, --crate <key>.
    search <QUERY>      Search the version-matched guides and worked examples.
                        --limit N, --kind guide|example, --lexical (BM25 only,
                        skipping the vector path).
    probe [--force]     Write the automation probe harness into
                        scripts/teksilo_probe/ so an agent can drive and assert
                        on the running app. --force overwrites local edits.
    setup [--force]     probe, plus install the teksilo skill wherever the
                        agents on this machine look for one.
    version             Print this tool's version and the app's resolved teksilo.
    help                Show this message.

MAINTAINER COMMANDS (only meaningful inside a teksilo checkout):

    build-vectors       Encode the corpus and write the dense vectors back into
                        crates/teksilo-corpus/corpus/index.json. Run it AFTER
                        `python3 tools/build_corpus.py`, which regenerates that
                        file from scratch and therefore drops them.
                        --corpus <path to index.json> to point it elsewhere.

Run from inside your app's crate or workspace — the answer is resolved from
its Cargo.lock, so it always matches the teksilo you actually depend on.
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `cargo teksilo foo` execs us as `cargo-teksilo teksilo foo`.
    if args.first().map(String::as_str) == Some("teksilo") {
        args.remove(0);
    }

    let dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };

    match args.first().map(String::as_str) {
        None | Some("help") | Some("-h") | Some("--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V") | Some("--version") => {
            println!("cargo-teksilo {}", guard::TOOL_VERSION);
            ExitCode::SUCCESS
        }
        Some("version") if wants_help(&args) => {
            print!("{VERSION_USAGE}");
            ExitCode::SUCCESS
        }
        Some("version") => cmd_version(&dir),
        Some("symbol") => cmd_symbol(&dir, &args[1..]),
        Some("search") => cmd_search(&dir, &args[1..]),
        Some("build-vectors") => cmd_build_vectors(&dir, &args[1..]),
        Some("probe") if wants_help(&args) => {
            print!("{PROBE_USAGE}");
            ExitCode::SUCCESS
        }
        Some("setup") if wants_help(&args) => {
            print!("{SETUP_USAGE}");
            ExitCode::SUCCESS
        }
        Some("probe") => cmd_probe(&dir, has_flag(&args, "--force"), false),
        Some("setup") => cmd_probe(&dir, has_flag(&args, "--force"), true),
        Some(other) => {
            eprintln!("error: unknown command `{other}`\n");
            print!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_version(dir: &Path) -> ExitCode {
    println!("cargo-teksilo {}", guard::TOOL_VERSION);
    match resolve::resolve(dir) {
        Ok(r) => {
            println!("app resolved teksilo {}", r.version);
            for c in r.crates.values() {
                println!("  {:<28} {}  {}", c.name, c.version, c.dir.display());
            }
            match guard::check(&r.version) {
                guard::Verdict::Ok => println!("versions match"),
                guard::Verdict::Degraded { app, tool } => {
                    println!("{}", guard::degraded_text(&app, &tool))
                }
                guard::Verdict::Refuse { app, tool } => {
                    println!("\n{}", guard::refusal_text(&app, &tool, "symbol lookup"))
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_symbol(dir: &Path, args: &[String]) -> ExitCode {
    match symbol::run(dir, args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_search(dir: &Path, args: &[String]) -> ExitCode {
    match search::run(dir, args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// `build-vectors` — the release-time pass that fills the corpus's vectors in.
///
/// Not part of the consumer surface: it rewrites a file that only exists in a
/// teksilo checkout, and it needs the `semantic` feature (the encoder) to do
/// anything at all. Both failures are reported as themselves rather than as a
/// missing file or an empty result.
fn cmd_build_vectors(dir: &Path, args: &[String]) -> ExitCode {
    match vectors::build(dir, args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// Whether this invocation is asking for the command's usage.
///
/// `symbol` and `search` answer `--help` themselves (one forwards it to the
/// extractor, the other parses it), so only the three commands that take no
/// query need this. Without it they read `--help` as an unrecognised argument
/// and do the work anyway — which for `setup` means writing files at someone
/// who was asking what it would do.
fn wants_help(args: &[String]) -> bool {
    args.iter().skip(1).any(|a| a == "--help" || a == "-h")
}

const PROBE_USAGE: &str = "\
cargo teksilo probe — write the automation probe harness into this project

USAGE:
    cargo teksilo probe [--force]

Writes a stdlib-only Python package to scripts/teksilo_probe/ so an agent can
drive the running app through the automation bridge and assert on it, plus three
worked examples to copy from. Your own probes belong in scripts/, one level up —
this command never reads or writes them.

OPTIONS:
    --force    Overwrite generated files you have edited. Without it, a local
               edit is reported and kept: the harness records a checksum of
               everything it wrote, so it can tell its own output from yours.
    -h, --help Show this message.

The harness is matched to the teksilo this app resolved, and the version it was
written for is recorded in Cargo.toml under [package.metadata.teksilo]. A later
run warns when that has drifted.
";

const SETUP_USAGE: &str = "\
cargo teksilo setup — make an agent effective in this project

USAGE:
    cargo teksilo setup [--force]

Runs `probe`, then installs the teksilo skill wherever the agents on this
machine already look for one. Only directories that already exist are used:
installing a skill where nothing reads it is indistinguishable from not
installing it, except that it reports success.

OPTIONS:
    --force    Passed through to `probe` — overwrite generated files you edited.
    -h, --help Show this message.
";

const VERSION_USAGE: &str = "\
cargo teksilo version — what this tool is, and what this app resolved

USAGE:
    cargo teksilo version

Prints this tool's version, every teksilo crate in the app's resolved dependency
graph with the path it resolved to, and whether the two match. Use it when a
command refuses: it shows the versions the refusal is comparing.

`cargo teksilo --version` prints just this tool's version.
";

/// `probe`, and with `also_skill` the rest of `setup`.
///
/// Both refuse on a version mismatch for the same reason `symbol` does: a
/// harness written for a teksilo the app did not resolve drives a bridge whose
/// protocol it may not speak, and fails with a symptom naming neither version.
fn cmd_probe(dir: &Path, force: bool, also_skill: bool) -> ExitCode {
    let resolution = match resolve::resolve(dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let verdict = guard::check(&resolution.version);
    if !verdict.may_answer() {
        let guard::Verdict::Refuse { app, tool } = &verdict else {
            unreachable!()
        };
        eprintln!("{}", guard::refusal_text(app, tool, "the probe harness"));
        return ExitCode::FAILURE;
    }
    if let Some(note) = verdict.note() {
        eprintln!("{note}");
    }

    if let Some(recorded) = probe::recorded_provenance(dir)
        && recorded != resolution.version
    {
        println!(
            "note: the harness here was written for teksilo {recorded}, this app now \
             resolves {}. Rewriting it.",
            resolution.version
        );
    }

    let written = match probe::materialise(dir, force) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = probe::record_provenance(dir, &resolution.version) {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }
    println!(
        "probe harness: {} files in scripts/teksilo_probe/ \
         ({} created, {} updated, {} unchanged) for teksilo {}",
        written.total(),
        written.created.len(),
        written.updated.len(),
        written.unchanged.len(),
        resolution.version
    );

    if also_skill {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let homes = setup::skill_homes(dir, home.as_deref());
        if homes.is_empty() {
            println!(
                "skill: no agent skill directory found (looked for .claude/ here and in $HOME).\n\
                 Create one and re-run `cargo teksilo setup` to install it."
            );
        } else {
            match setup::install_skill(&homes) {
                Ok(done) => {
                    for (where_, count) in done.homes {
                        println!("skill: {count} files -> {where_}");
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    println!("\nNext: read scripts/teksilo_probe/ and copy the example closest to your case.");
    ExitCode::SUCCESS
}
