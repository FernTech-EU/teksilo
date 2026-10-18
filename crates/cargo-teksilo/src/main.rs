// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

mod companion;
mod guard;
mod install;
mod output;
mod probe;
mod resolve;
mod search;
mod setup;
mod show;
mod status;
mod symbol;
mod update;
mod vectors;

use clap::{Args, Parser, Subcommand};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cargo {
    Teksilo(Cli),
}

#[derive(Args)]
#[command(
    version,
    about = "API lookup, documentation, and agent tooling for Teksilo"
)]
struct Cli {
    /// Suppress success messages and informational notes. Which release
    /// answered, and in which search mode, is still printed.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,
    /// Show paths and diagnostic details.
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Look up a public type or module.
    Symbol {
        /// Names and extractor options (--crate, --list, --all, -f json).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Search guides and examples.
    Search {
        #[arg(required = true)]
        query: Vec<String>,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long)]
        kind: Option<SearchKind>,
        /// Restrict results to this corpus path prefix.
        #[arg(long)]
        path: Option<String>,
        /// Use lexical search only.
        #[arg(long)]
        lexical: bool,
        /// Emit structured results.
        #[arg(long)]
        json: bool,
    },
    /// Read a document from the bundled corpus.
    Show {
        #[arg(required_unless_present = "list")]
        path: Option<String>,
        #[arg(long, value_name = "A-B")]
        lines: Option<String>,
        #[arg(long, conflicts_with_all = ["lines", "path"])]
        list: bool,
    },
    /// Install the probe harness and agent instructions.
    Init {
        /// Install for these agents, regardless of detection. Repeat to select more.
        #[arg(long = "agent", value_enum)]
        agents: Vec<setup::Agent>,
        #[arg(short = 'y', long)]
        yes: bool,
        /// Overwrite conflicting generated files.
        #[arg(long)]
        force: bool,
    },
    /// Manage agent instructions.
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Manage the automation harness.
    Probe {
        #[command(subcommand)]
        command: ProbeCommand,
    },
    /// Manage the semantic search model.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Refresh installed tooling to this release.
    Update {
        /// Update the project's agent instructions only.
        #[arg(long)]
        agent: bool,
        /// Update the agent instructions under your home directory instead.
        #[arg(long)]
        user: bool,
        /// Skip the confirmation.
        #[arg(short = 'y', long, conflicts_with = "interactive")]
        yes: bool,
        /// Confirm each action, showing its equivalent command.
        #[arg(short = 'i', long)]
        interactive: bool,
        /// List what would change, and change nothing.
        #[arg(long, conflicts_with_all = ["yes", "interactive"])]
        dry_run: bool,
        /// With --yes, also replace probe files that have local edits.
        #[arg(long)]
        force: bool,
    },
    /// Show project and tooling status.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Encode the corpus (maintainer only).
    #[command(hide = true)]
    BuildVectors {
        #[arg(long)]
        corpus: Option<String>,
    },
}
#[derive(Subcommand)]
enum AgentCommand {
    /// List supported agents and installation status.
    List {
        #[arg(long)]
        user: bool,
        #[arg(long)]
        json: bool,
    },
    /// Install instructions for named agents, creating directories as needed.
    Install {
        #[arg(required = true, value_enum)]
        agents: Vec<setup::Agent>,
        /// Install for this user rather than this project.
        #[arg(long)]
        user: bool,
        #[arg(long)]
        force: bool,
    },
}
#[derive(Subcommand)]
enum ProbeCommand {
    Install {
        #[arg(long)]
        force: bool,
    },
}
#[derive(Subcommand)]
enum ModelCommand {
    Fetch,
}
#[derive(Clone, Copy, clap::ValueEnum)]
enum SearchKind {
    Guide,
    Example,
}

fn main() -> ExitCode {
    let Cargo::Teksilo(cli) = Cargo::parse();
    let json = matches!(
        &cli.command,
        Command::Status { json: true }
            | Command::Search { json: true, .. }
            | Command::Agent {
                command: AgentCommand::List { json: true, .. }
            }
    );
    output::set(output::Options {
        quiet: cli.quiet,
        verbose: cli.verbose,
        json,
    });
    let result = std::env::current_dir()
        .map_err(|e| e.to_string())
        .and_then(|dir| run(&dir, cli.command));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if !e.is_empty() {
                eprintln!(
                    "{}",
                    if e.starts_with("error:") {
                        e
                    } else {
                        format!("error: {e}")
                    }
                );
            }
            ExitCode::FAILURE
        }
    }
}
fn process_result<E: std::fmt::Display>(result: Result<i32, E>) -> Result<(), String> {
    match result {
        Ok(0) => Ok(()),
        Ok(_) => Err(String::new()),
        Err(e) => Err(e.to_string()),
    }
}
fn project(dir: &Path) -> Result<PathBuf, String> {
    setup::find_project_root(dir)
        .ok_or_else(|| "no Cargo.toml found\nhelp: run inside a Cargo project".into())
}
fn version(root: &Path) -> Result<String, String> {
    resolution(root).map(|r| r.version)
}
/// The app's resolution, refused when this tool cannot serve it.
fn resolution(root: &Path) -> Result<resolve::Resolution, String> {
    let r = resolve::resolve(root).map_err(|e| e.to_string())?;
    match guard::check(&r.version) {
        guard::Verdict::Refuse { app, tool } => Err(guard::refusal_text(
            &app,
            &tool,
            "this command",
            guard::readable_sources(&r).as_deref(),
        )),
        verdict => {
            if let Some(note) = verdict.note() {
                output::provenance(note);
            }
            Ok(r)
        }
    }
}
pub(crate) fn install_probe(root: &Path, version: &str, force: bool) -> Result<(), String> {
    let written = probe::materialise(root, force).map_err(|e| e.to_string())?;
    probe::record_provenance(root, version).map_err(|e| e.to_string())?;
    if output::verbose() {
        eprintln!(
            "Probe: {} files in {}",
            written.total(),
            root.join("scripts/teksilo_probe").display()
        );
    }
    Ok(())
}
fn install_agents(targets: &[setup::Target], force: bool) -> Result<(), String> {
    for target in targets {
        setup::check_conflict(target, force)?;
    }
    for target in targets {
        let done = setup::apply(target).map_err(|e| e.to_string())?;
        if output::verbose() {
            eprintln!(
                "{}: {} ({}, {} files)",
                done.agent,
                done.path.display(),
                done.change.describe(),
                done.files
            );
        }
    }
    Ok(())
}
fn success(message: impl std::fmt::Display) {
    if !output::quiet() {
        println!("{message}");
    }
}
fn update(
    dir: &Path,
    scope: update::Scope,
    yes: bool,
    interactive: bool,
    dry_run: bool,
    force: bool,
) -> Result<(), String> {
    // `-i` is nothing but questions, so refuse before printing half of them.
    if interactive && let Some(reason) = setup::nobody_to_ask() {
        return Err(format!(
            "--interactive asks before each action, and {reason}\n\
             help: run `cargo teksilo update --dry-run` to list the changes"
        ));
    }
    let (root, version, header) = if scope == update::Scope::User {
        // As `agent install --user`: no project, and the copies are this
        // release's whatever a project nearby resolves.
        let home = setup::home_dir().map_err(|e| e.to_string())?;
        let header = format!("Home     {}", home.display());
        (home, guard::TOOL_VERSION.to_string(), header)
    } else {
        let root = project(dir)?;
        let r = resolution(&root)?;
        // A patch-older tool passes the guard, and would write an older
        // release's files over the ones it is meant to refresh.
        if guard::tool_is_older(&r.version) {
            return Err(format!(
                "tool {} cannot update tooling for Teksilo {}\nhelp: run `{}`, then `cargo teksilo update` again",
                guard::TOOL_VERSION,
                r.version,
                guard::self_install(&r.version, r.checkout_root().as_deref()).display()
            ));
        }
        let header = format!("Project  {}\nTeksilo  {}", root.display(), r.version);
        (root, r.version, header)
    };
    let companions = if scope == update::Scope::Project {
        [companion::MCP, companion::FMT]
            .into_iter()
            .map(|c| companion::inspect_path(c, &version))
            .collect()
    } else {
        Vec::new()
    };
    let items = update::plan(&root, scope, &version, companions)?;
    if items.is_empty() {
        success("Nothing to update.");
        return Ok(());
    }
    println!("{header}\n");
    for item in &items {
        for line in item.describe(&root) {
            println!("{line}");
        }
    }
    if dry_run {
        return Ok(());
    }
    let confirm = |question: &str| setup::confirm(question, "--yes").map_err(|e| e.to_string());
    let mut accepted = Vec::new();
    let mut skipped = Vec::new();
    if interactive {
        for item in &items {
            println!();
            for line in item.describe(&root) {
                println!("{line}");
            }
            println!("         $ {}", item.manual());
            if confirm("Apply?")? {
                accepted.push(item);
            } else {
                skipped.push(item);
            }
        }
    } else if yes || confirm("\nProceed?")? {
        accepted.extend(&items);
    } else {
        return Ok(());
    }
    // Agreeing to a listing that names the edited probe files is consent to
    // replacing them; `--yes` agrees to a listing nobody read.
    let consented = !yes || force;
    let mut updated = Vec::new();
    for item in &accepted {
        if let Err(e) = item.apply(&root, &version, consented) {
            if !updated.is_empty() {
                success(format!("Updated: {}.", updated.join(", ")));
            }
            return Err(e);
        }
        updated.push(item.name());
    }
    if !updated.is_empty() {
        success(format!("Updated: {}.", updated.join(", ")));
    }
    // What is already running keeps the old copy: an agent read its
    // instructions when its session started, and its MCP client spawned the
    // server once.
    let mut restart = Vec::new();
    if accepted
        .iter()
        .any(|i| matches!(i, update::Item::Agent { .. }))
    {
        restart.push("the agent session, to read the new instructions");
    }
    if accepted
        .iter()
        .any(|i| matches!(i, update::Item::Binary(found) if found.companion == companion::MCP))
    {
        restart.push("the MCP server, to run the new teksilo-automation-mcp");
    }
    if !restart.is_empty() {
        success(format!("help: restart {}", restart.join(", and ")));
    }
    if !skipped.is_empty() {
        success(format!(
            "Skipped: {}.",
            skipped
                .iter()
                .map(|i| i.name())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(())
}
fn run(dir: &Path, command: Command) -> Result<(), String> {
    match command {
        Command::Symbol { args } => process_result(symbol::run(dir, &args)),
        Command::Search {
            query,
            limit,
            kind,
            path,
            lexical,
            ..
        } => {
            let mut args = query;
            args.extend(["--limit".into(), limit.to_string()]);
            if let Some(kind) = kind {
                args.extend([
                    "--kind".into(),
                    match kind {
                        SearchKind::Guide => "guide",
                        SearchKind::Example => "example",
                    }
                    .into(),
                ]);
            }
            if let Some(path) = path {
                args.extend(["--path".into(), path]);
            }
            if lexical {
                args.push("--lexical".into());
            }
            process_result(search::run(dir, &args))
        }
        Command::Show { path, lines, list } => {
            process_result(show::run(dir, &show::ShowRequest { path, lines, list }))
        }
        Command::Status { .. } => {
            status::report(dir);
            Ok(())
        }
        Command::Update {
            agent,
            user,
            yes,
            interactive,
            dry_run,
            force,
        } => update(
            dir,
            update::Scope::from_flags(agent, user),
            yes,
            interactive,
            dry_run,
            force,
        ),
        Command::Probe {
            command: ProbeCommand::Install { force },
        } => {
            let root = project(dir)?;
            let version = version(&root)?;
            install_probe(&root, &version, force)?;
            success("Installed: probe harness.");
            Ok(())
        }
        Command::Init { agents, yes, force } => {
            let root = project(dir)?;
            let version = version(&root)?;
            let targets = setup::selected_targets(&root, &agents, false)?;
            for target in &targets {
                setup::check_conflict(target, force)?;
            }
            if !yes {
                println!(
                    "Project  {}\nTeksilo  {version}\n\nInstall  probe harness",
                    root.display()
                );
                for target in &targets {
                    println!("Install  {} instructions", target.agent);
                }
                if !setup::confirm("\nProceed?", "--yes").map_err(|e| e.to_string())? {
                    return Ok(());
                }
            }
            install_probe(&root, &version, force)?;
            install_agents(&targets, force)?;
            let mut names = vec!["probe harness"];
            names.extend(targets.iter().map(|t| t.agent));
            success(format!("Installed: {}.", names.join(", ")));
            Ok(())
        }
        Command::Agent {
            command:
                AgentCommand::Install {
                    agents,
                    user,
                    force,
                },
        } => {
            let root = if user {
                setup::home_dir().map_err(|e| e.to_string())?
            } else {
                project(dir)?
            };
            if !user {
                version(&root)?;
            }
            let targets = setup::selected_targets(&root, &agents, user)?;
            install_agents(&targets, force)?;
            success(format!(
                "Installed: {}.",
                targets
                    .iter()
                    .map(|t| t.agent)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            Ok(())
        }
        Command::Agent {
            command: AgentCommand::List { user, .. },
        } => {
            let root = if user {
                setup::home_dir().map_err(|e| e.to_string())?
            } else {
                setup::find_project_root(dir).unwrap_or_else(|| dir.to_path_buf())
            };
            status::agents(&root, user);
            Ok(())
        }
        Command::Model {
            command: ModelCommand::Fetch,
        } => {
            if output::verbose() {
                eprintln!("Search model: ~{} MB", vectors::ENCODER_DOWNLOAD_MB);
            }
            let dir = vectors::prefetch_encoder().map_err(|e| e.to_string())?;
            if output::verbose() {
                eprintln!("Model cache: {}", dir.display());
            }
            success("Search model ready.");
            Ok(())
        }
        Command::BuildVectors { corpus } => {
            let args = corpus
                .map(|p| vec!["--corpus".into(), p])
                .unwrap_or_default();
            process_result(vectors::build(dir, &args))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_agents_and_output_flags_parse() {
        assert!(
            Cargo::try_parse_from([
                "cargo", "teksilo", "init", "--agent", "codex", "--agent", "claude", "-y"
            ])
            .is_ok()
        );
        assert!(
            Cargo::try_parse_from(["cargo", "teksilo", "agent", "install", "codex", "cursor"])
                .is_ok()
        );
        assert!(Cargo::try_parse_from(["cargo", "teksilo", "search", "button", "--json"]).is_ok());
        assert!(
            Cargo::try_parse_from(["cargo", "teksilo", "--quiet", "--verbose", "status"]).is_err()
        );
        assert!(Cargo::try_parse_from(["cargo", "teksilo", "setup"]).is_err());
        for args in [
            &["update"][..],
            &["update", "--agent"],
            &["update", "--user"],
            &["update", "--agent", "--user", "-i"],
            &["update", "-y", "--force"],
            &["update", "--dry-run"],
            &["update", "--user", "--dry-run"],
        ] {
            let argv = ["cargo", "teksilo"].iter().chain(args);
            assert!(Cargo::try_parse_from(argv).is_ok(), "{args:?}");
        }
        assert!(Cargo::try_parse_from(["cargo", "teksilo", "update", "-y", "-i"]).is_err());
        assert!(Cargo::try_parse_from(["cargo", "teksilo", "update", "--dry-run", "-y"]).is_err());
    }
}
