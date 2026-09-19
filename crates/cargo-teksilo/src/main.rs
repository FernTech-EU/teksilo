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

mod guard;
mod probe;
mod resolve;
mod search;
mod setup;
mod show;
mod symbol;
mod vectors;

use std::path::Path;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

/// Invoked as a cargo subcommand, cargo passes its own name as `argv[1]`.
///
/// Wrapping the real CLI in a one-variant enum is clap's own idiom for this:
/// the `teksilo` word is consumed as a subcommand name, and `bin_name` makes
/// generated help read `cargo teksilo …` rather than `cargo-teksilo …`.
#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cargo {
    Teksilo(Cli),
}

#[derive(Args)]
#[command(
    version,
    about = "Agent tooling for teksilo apps",
    long_about = "Agent tooling for apps that depend on the teksilo GUI framework.\n\n\
                  Run from inside your app's crate or workspace — every answer is \
                  resolved from its Cargo.lock, so it always matches the teksilo you \
                  actually depend on.",
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Exact public API of a type, for the teksilo version this app pins.
    ///
    /// Accepts the API extractor's own flags — `--list`, `--all`, `-f json`,
    /// `--crate <key>` — which are forwarded verbatim.
    Symbol {
        /// Type or module names, plus any extractor flag.
        ///
        /// Collected raw rather than modelled: this command is a front end for
        /// `extract_widget_api.py`, whose flag surface is that script's to
        /// change. Re-declaring it here would mean a second place to update and
        /// a new way for the two to disagree.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Search the version-matched guides and worked examples.
    Search {
        /// What to look for.
        query: Vec<String>,

        /// Results to show.
        #[arg(long, default_value_t = 8, value_name = "N")]
        limit: usize,

        /// Restrict to one half of the corpus.
        #[arg(long, value_name = "KIND")]
        kind: Option<SearchKind>,

        /// BM25 only — skip the vector path.
        #[arg(long)]
        lexical: bool,
    },

    /// Print a corpus document in full — offline, at the pinned version.
    ///
    /// The companion to `search`, which cites a path the app does not have:
    /// `docs/` ships in no crate and every example is `publish = false`, so the
    /// only other ways to read one are GitHub (which tracks `main`, not the
    /// version this app pins) and guessing from the snippet. The text is
    /// already in the corpus; this reassembles it.
    ///
    /// The document goes to stdout and nothing else does, so it can be
    /// redirected or piped; the version line goes to stderr.
    Show {
        /// A corpus path, exactly as `search` prints it.
        ///
        /// `docs/scroll-area.md`, `examples/simple_button/src/main.rs`. A bare
        /// filename or a trailing fragment is accepted when it names one
        /// document; anything else is answered with the near spellings.
        #[arg(value_name = "PATH", required_unless_present = "list")]
        path: Option<String>,

        /// Just these lines — `A-B`, or `A` for one.
        ///
        /// 1-based and inclusive, matching the `(lines A-B)` that `search`
        /// prints under a hit and the numbers in an editor's gutter. So
        /// `--lines 1-1` is the first line.
        #[arg(long, value_name = "A-B")]
        lines: Option<String>,

        /// Every path in the corpus, one per line (counts on stderr).
        #[arg(long, conflicts_with = "lines")]
        list: bool,
    },

    /// Write the automation probe harness into scripts/teksilo_probe/.
    ///
    /// So an agent can drive the running app through the automation bridge and
    /// assert on it. Your own probes belong in scripts/, one level up; this
    /// never reads or writes them.
    Probe {
        /// Overwrite generated files you have edited.
        ///
        /// Without it a local edit is reported and kept: the harness records a
        /// checksum of everything it wrote, so it can tell its output from yours.
        #[arg(long)]
        force: bool,
    },

    /// Probe, plus install the teksilo skill where agents look for one.
    ///
    /// Only directories that already exist are used: installing a skill where
    /// nothing reads it is indistinguishable from not installing it, except
    /// that it reports success.
    Setup {
        /// Passed through to `probe`.
        #[arg(long)]
        force: bool,
    },

    /// Print this tool's version and the app's resolved teksilo.
    ///
    /// Use it when a command refuses: it shows the versions being compared.
    Version,

    /// Encode the corpus and write its vectors back (maintainer only).
    ///
    /// Run AFTER `python3 tools/build_corpus.py`, which regenerates
    /// `index.json` from scratch and therefore drops them. Requires the
    /// `semantic` feature.
    #[command(hide = true)]
    BuildVectors {
        /// The index.json to vectorise.
        #[arg(long, value_name = "PATH")]
        corpus: Option<String>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum SearchKind {
    Guide,
    Example,
}

fn main() -> ExitCode {
    let Cargo::Teksilo(cli) = Cargo::parse();

    let dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };

    match cli.command {
        Command::Version => cmd_version(&dir),
        Command::Symbol { args } => cmd_symbol(&dir, &args),
        Command::Search {
            query,
            limit,
            kind,
            lexical,
        } => {
            // `search` still takes a flat argv because its parser is a pure,
            // unit-tested function over one; rebuilding that vector keeps the
            // tests meaningful rather than testing clap.
            let mut argv: Vec<String> = query;
            argv.push("--limit".into());
            argv.push(limit.to_string());
            if let Some(k) = kind {
                argv.push("--kind".into());
                argv.push(match k {
                    SearchKind::Guide => "guide".into(),
                    SearchKind::Example => "example".into(),
                });
            }
            if lexical {
                argv.push("--lexical".into());
            }
            cmd_search(&dir, &argv)
        }
        Command::Show { path, lines, list } => {
            cmd_show(&dir, &show::ShowRequest { path, lines, list })
        }
        Command::Probe { force } => cmd_probe(&dir, force, false),
        Command::Setup { force } => cmd_probe(&dir, force, true),
        Command::BuildVectors { corpus } => {
            let mut argv = Vec::new();
            if let Some(path) = corpus {
                argv.push("--corpus".into());
                argv.push(path);
            }
            cmd_build_vectors(&dir, &argv)
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

/// `show` — a corpus document, in full, from the corpus itself.
///
/// The document is the only thing on stdout: an agent is expected to redirect
/// it, and a version banner mixed into a Markdown guide or a Rust source file
/// is a corrupted document rather than a helpful note. Everything else — the
/// version line, a rewritten path, a version-mismatch warning — goes to stderr.
fn cmd_show(dir: &Path, request: &show::ShowRequest) -> ExitCode {
    match show::run(dir, request) {
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
