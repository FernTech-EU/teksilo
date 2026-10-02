// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

mod guard;
mod output;
mod probe;
mod resolve;
mod search;
mod setup;
mod show;
mod status;
mod symbol;
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
    /// Suppress success messages and informational notes.
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
                output::note(note);
            }
            Ok(r.version)
        }
    }
}
fn install_probe(root: &Path, version: &str, force: bool) -> Result<(), String> {
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
fn run(dir: &Path, command: Command) -> Result<(), String> {
    match command {
        Command::Symbol { args } => process_result(symbol::run(dir, &args)),
        Command::Search {
            query,
            limit,
            kind,
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
    }
}
