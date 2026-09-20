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
mod status;
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
    ///
    /// A bare name is resolved against every teksilo crate, so `symbol
    /// ListModel` finds teksilo-data's without `--crate data`; the note on
    /// stderr says which crate answered, and names the others when more than
    /// one defines that name.
    ///
    /// Runs the Python 3 API extractor, so this one command needs `python3`
    /// on PATH — `cargo teksilo status` reports whether it is there. Every
    /// other subcommand works without it.
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

    /// Set this project up for the coding agents configured in it.
    ///
    /// Writes the probe harness, hands every detected agent the teksilo
    /// briefing in *that agent's own format* — the full skill where it is
    /// native, a self-contained condensed brief everywhere else — and fetches
    /// the search encoder so the first `search` does not stall on it.
    ///
    /// Only directories that already exist are used: instructions installed
    /// where nothing reads them are indistinguishable from none, except that
    /// they report success. The plan is printed before anything is written.
    Setup {
        /// Overwrite generated probe files you have edited.
        #[arg(long)]
        force: bool,

        /// Write the plan without asking first.
        ///
        /// Required non-interactively: with no terminal on stdin there is
        /// nobody to answer, and blocking on input that cannot come is worse
        /// than failing.
        #[arg(short = 'y', long)]
        yes: bool,

        /// Install for this user instead of this project.
        ///
        /// The only mode that writes `$HOME`. Project scope never does, not
        /// even as a fallback when the project has no agent directory.
        #[arg(long)]
        user: bool,

        /// Skip the one-off encoder download.
        ///
        /// `search` still answers without it — it degrades to BM25, which is
        /// the same thing that happens when the download fails.
        #[arg(long)]
        no_model: bool,
    },

    /// Report what is installed — this project, this user, the search model.
    ///
    /// The dry run of `setup`: every agent row is computed by the same code
    /// that decides whether a write is needed, so the two cannot come to
    /// disagree about what "installed" means. Writes nothing, and its exit
    /// code does not depend on what it finds.
    Status,

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
        Command::Status => {
            status::report(&dir);
            ExitCode::SUCCESS
        }
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
        Command::Probe { force } => cmd_probe(&dir, force),
        Command::Setup {
            force,
            yes,
            user,
            no_model,
        } => cmd_setup(&dir, force, yes, user, no_model),
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

/// `probe` — the automation harness, version-matched to the app.
///
/// Refuses on a version mismatch for the same reason `symbol` does: a harness
/// written for a teksilo the app did not resolve drives a bridge whose protocol
/// it may not speak, and fails with a symptom naming neither version.
fn cmd_probe(dir: &Path, force: bool) -> ExitCode {
    let version = match resolved_for_probe(dir) {
        Ok(v) => v,
        Err(ProbeBlock::Refused(message) | ProbeBlock::Unresolved(message)) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    match write_probe(dir, &version, force) {
        Ok(summary) => println!("{summary}"),
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    }
    println!("\nNext: read scripts/teksilo_probe/ and copy the example closest to your case.");
    ExitCode::SUCCESS
}

/// Why no harness can be written here.
///
/// Two reasons that read alike and must not be treated alike. `probe` fails on
/// either — writing a harness is the whole command. `setup` fails only on
/// `Refused`, because a version this tool must not answer for is a version it
/// must not hand an agent instructions about either; `Unresolved` merely means
/// there is nothing here to stamp a harness with, which is no reason to
/// withhold the agent scaffolding.
enum ProbeBlock {
    /// The app resolved a teksilo whose minor or major this tool cannot serve.
    Refused(String),
    /// There is no resolved teksilo at all — not a teksilo app, or no lockfile.
    Unresolved(String),
}

/// The app's resolved teksilo, or why the harness cannot be written.
fn resolved_for_probe(project: &Path) -> Result<String, ProbeBlock> {
    let resolution =
        resolve::resolve(project).map_err(|e| ProbeBlock::Unresolved(e.to_string()))?;
    match guard::check(&resolution.version) {
        guard::Verdict::Refuse { app, tool } => Err(ProbeBlock::Refused(guard::refusal_text(
            &app,
            &tool,
            "the probe harness",
        ))),
        verdict => {
            if let Some(note) = verdict.note() {
                eprintln!("{note}");
            }
            Ok(resolution.version)
        }
    }
}

/// Materialise the harness and stamp it with the version it was written for.
fn write_probe(project: &Path, version: &str, force: bool) -> Result<String, String> {
    if let Some(recorded) = probe::recorded_provenance(project)
        && recorded != version
    {
        println!(
            "note: the harness here was written for teksilo {recorded}, this app now \
             resolves {version}. Rewriting it."
        );
    }

    let written = probe::materialise(project, force).map_err(|e| e.to_string())?;
    probe::record_provenance(project, version).map_err(|e| e.to_string())?;
    Ok(format!(
        "probe harness: {} files in scripts/teksilo_probe/ \
         ({} created, {} updated, {} unchanged) for teksilo {version}",
        written.total(),
        written.created.len(),
        written.updated.len(),
        written.unchanged.len(),
    ))
}

/// `setup` — the harness, the agent instructions, and a warm encoder cache.
///
/// Plan, confirm, write, report. The plan comes first because the alternative
/// — finding out afterwards that one command rewrote five files and pulled
/// 129 MB — is how a tool loses the benefit of the doubt. And the plan is
/// exhaustive: every path that will be written appears in it.
fn cmd_setup(dir: &Path, force: bool, yes: bool, user: bool, no_model: bool) -> ExitCode {
    let project = setup::find_project_root(dir);

    // No manifest anywhere up the tree. Neither failing nor quietly writing the
    // home directory instead is right — the second is precisely the surprise
    // this rewrite removes — so say what is missing and ask. `-y` does not
    // answer this one: it suppresses a confirmation, not the choice of which
    // machine-wide directory to write.
    let scope = if user {
        setup::Scope::User
    } else if project.is_some() {
        setup::Scope::Project
    } else {
        println!(
            "No Cargo.toml in {} or any directory above it, so there is no project here\n\
             to set up. The agent instructions can still be installed for your user account.",
            dir.display()
        );
        if yes {
            eprintln!(
                "\nerror: -y suppresses a confirmation, not this choice. Pass --user to install\n\
                 under $HOME, or run this from inside your app."
            );
            return ExitCode::FAILURE;
        }
        match setup::confirm("\nInstall for this user instead, under $HOME?", "--user") {
            Ok(true) => setup::Scope::User,
            Ok(false) => {
                println!("Nothing written.");
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    };

    let root = match scope {
        setup::Scope::Project => project.clone().expect("project scope implies a manifest"),
        setup::Scope::User => match setup::home_dir() {
            Ok(home) => home,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    let targets = match scope {
        setup::Scope::Project => setup::project_targets(&root),
        setup::Scope::User => setup::user_targets(&root),
    };

    // The harness is project code — it lands in `scripts/teksilo_probe/` — so
    // it follows the project, not the scope. `--user` run from inside an app
    // still gets it, and the plan says so rather than leaving it a surprise.
    let mut probe_version = None;
    let probe_skipped = match &project {
        None => Some("no Cargo.toml here — the harness is project code".to_string()),
        Some(p) => match resolved_for_probe(p) {
            Ok(version) => {
                probe_version = Some(version);
                None
            }
            // A version this tool must not answer for is one it must not write
            // agent instructions about either: stop, with the install command.
            Err(ProbeBlock::Refused(message)) => {
                eprintln!("{message}");
                return ExitCode::FAILURE;
            }
            Err(ProbeBlock::Unresolved(message)) => Some(message),
        },
    };

    let fetch_model =
        !no_model && !vectors::encoder_is_cached() && vectors::encoder_cache_dir().is_some();

    // --- the plan ---------------------------------------------------------

    match scope {
        setup::Scope::Project => println!("Plan — project {}", root.display()),
        setup::Scope::User => {
            println!("Plan — user account {}", root.display());
            // The harness is the one thing `--user` still writes into the
            // project, so the project is named rather than left to be inferred
            // from a path in the table.
            if let Some(p) = &project {
                println!("       project      {} (harness only)", p.display());
            }
        }
    }
    println!();
    if let (Some(p), Some(version)) = (&project, &probe_version) {
        println!(
            "  {:<18} {:<40} for teksilo {version}",
            "probe harness",
            // Always project-relative: it is project code wherever the rest of
            // the plan is aimed.
            relative(&p.join("scripts/teksilo_probe"), p, true)
        );
    }
    for target in &targets {
        println!(
            "  {:<18} {:<40} {}",
            target.agent,
            relative(&target.path, &root, target.form == setup::Form::Skill),
            target.form.describe()
        );
    }
    if fetch_model {
        println!(
            "  {:<18} {:<40} download ~{} MB, once",
            "search encoder",
            vectors::encoder_cache_dir().unwrap_or_default().display(),
            vectors::ENCODER_DOWNLOAD_MB
        );
    }
    if targets.is_empty() && probe_version.is_none() && !fetch_model {
        println!("  (nothing to do)");
    }
    if let Some(reason) = &probe_skipped {
        println!("\nThe probe harness is skipped: {reason}");
    }
    println!("\nNothing outside those paths is written.");

    if !yes {
        match setup::confirm("Proceed?", "-y / --yes") {
            Ok(true) => {}
            Ok(false) => {
                println!("Nothing written.");
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // --- doing it ---------------------------------------------------------

    println!();
    if let (Some(p), Some(version)) = (&project, &probe_version) {
        match write_probe(p, version, force) {
            Ok(summary) => println!("{summary}"),
            Err(message) => {
                eprintln!("{message}");
                return ExitCode::FAILURE;
            }
        }
    }

    for target in &targets {
        match setup::apply(target) {
            Ok(done) => println!(
                "{:<18} {:<40} {} ({} file{})",
                done.agent,
                relative(&done.path, &root, target.form == setup::Form::Skill),
                done.change.describe(),
                done.files,
                if done.files == 1 { "" } else { "s" }
            ),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // A failed download is a warning, never a failure: everything else
    // succeeded, and `search` degrades to BM25 by design rather than breaking.
    if fetch_model {
        println!(
            "\nFetching the search encoder (~{} MB, once)…",
            vectors::ENCODER_DOWNLOAD_MB
        );
        match vectors::prefetch_encoder() {
            Ok(dir) => println!("search encoder: cached in {}", dir.display()),
            Err(e) => println!(
                "warning: could not fetch the search encoder: {e}\n\
                 `cargo teksilo search` still works — it degrades to BM25 (lexical) ranking,\n\
                 and will retry the download on its next run."
            ),
        }
    } else if no_model {
        println!("\nsearch encoder: skipped (--no-model). The first `search` will fetch it.");
    } else if vectors::encoder_cache_dir().is_none() {
        println!(
            "\nsearch encoder: skipped — this build has none (compiled with \
             `--no-default-features`).\n`search` uses BM25 only."
        );
    } else {
        println!("\nsearch encoder: already cached.");
    }

    // --- what was skipped, and why ----------------------------------------

    match scope {
        setup::Scope::Project => {
            let missing = setup::project_undetected(&root);
            if !missing.is_empty() {
                println!("\nNot configured in this project, so nothing was written for them:");
                for (agent, marker) in missing {
                    println!("  {agent:<18} no {marker}");
                }
                println!("Create the marker it looks for and re-run to install.");
            }
        }
        setup::Scope::User => {
            // Symmetric with project scope, and load-bearing for the two
            // agents whose directory is env-relocatable: "no ~/.vibe/" would
            // send a user with `VIBE_HOME` set to look in the wrong place, so
            // the path this actually checked is the one printed.
            let missing = setup::user_undetected(&root);
            if !missing.is_empty() {
                println!("\nNot configured for your user, so nothing was written for them:");
                for (agent, dir) in missing {
                    println!("  {agent:<18} no {}/", dir.display());
                }
                println!("Run the agent once so it creates that directory, then re-run this.");
            }
            println!("\n{}", setup::user_scope_note());
        }
    }

    // --- what was written is not the same as what is active ---------------
    //
    // Every line above reports a file this command WROTE. Whether an agent
    // then reads it is that agent's business, and at least one will not
    // straight away: Grok Build requires folder trust before it loads project
    // instructions at all (`--trust`, or an interactive grant), and any of
    // them silently skips an instruction file that happens to be gitignored.
    // Saying "installed" would claim an effect this command cannot verify —
    // the same reason it refuses to write where nothing reads.
    println!(
        "\nThese are files on disk. An agent picks them up on its own terms —\n\
         some ask you to trust the folder first, and a gitignored instruction\n\
         file is skipped silently."
    );

    // --- the surface the user now has -------------------------------------

    println!(
        "\nWhat you can run now:\n\
         \x20 cargo teksilo symbol <Name>      exact public API of a type, at the pinned version\n\
         \x20 cargo teksilo search \"<query>\"   the guides and worked examples\n\
         \x20 cargo teksilo show <path>        one of them in full, offline — not from GitHub\n\
         \x20 cargo teksilo status             what is installed, here and for you\n\
         \x20 cargo teksilo probe              rewrite the automation harness\n\
         \x20 cargo teksilo setup              this command"
    );
    ExitCode::SUCCESS
}

/// A path as the user would type it, relative to what the plan is about.
///
/// Absolute paths in a plan make the interesting part — which file — the
/// hardest thing to read. Directories keep a trailing slash, so a line naming
/// four files does not read as one.
fn relative(path: &Path, root: &Path, directory: bool) -> String {
    let shown = path.strip_prefix(root).unwrap_or(path).display();
    if directory {
        format!("{shown}/")
    } else {
        shown.to_string()
    }
}
