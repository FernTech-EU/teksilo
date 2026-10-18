<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Agent tooling

`cargo teksilo` provides API lookup, documentation search, and an automation
harness for the Teksilo version resolved by your application.

## Install and initialize

Install the tool version matching the app (0.13.0 or later), then run inside
its Cargo project:

```sh
cargo install cargo-teksilo --version <version> --locked
cargo teksilo init
```

`init` installs the harness and instructions for detected agents after a short
confirmation. Use `-y` in scripts. To select agents explicitly, including ones
with no existing configuration:

```sh
cargo teksilo init --agent codex --agent claude -y
cargo teksilo agent install cursor cline
cargo teksilo agent list
```

Explicit selection creates directories as needed. Agent IDs are `claude`,
`cursor`, `windsurf`, `cline`, `copilot`, `codex`, `vibe`, and `opencode`.
Claude receives the full skill; other targets receive instructions in their
rules format or a managed `AGENTS.md` section. Surrounding shared-file text is
preserved. Use `--force` to replace conflicting generated files.

`agent install` installs instructions only. User installation supports Claude,
Vibe, and opencode, with no project harness:

```sh
cargo teksilo agent install claude --user
cargo teksilo agent list --user
```

## Commands

```sh
cargo teksilo symbol Button
cargo teksilo symbol --crate data ListModel
cargo teksilo search "make a list scrollable"
cargo teksilo search "make a list scrollable" --json
cargo teksilo show docs/scroll-area.md --lines 166-172
cargo teksilo probe install
cargo teksilo status
cargo teksilo status --json
cargo teksilo --version
```

Search paths refer to the bundled corpus. Read them with `show`, even when
those files are absent from your application. `show --list` lists the corpus.
An application built against a teksilo checkout rather than a release still
gets the release's text from `search` and `show`; both say so when the
checkout's copy of a document they print is different.
`symbol` requires Python 3; other commands do not.

An application that keeps teksilo behind a Cargo feature needs nothing extra:
when its default features leave teksilo out, the commands resolve with every
feature enabled, and `status` names the feature that turns teksilo on.

`status` reports the resolved app version and installed tooling without
changing the lockfile. When this tool, or the automation server or formatter on
PATH, is older than the app's release, it prints the command that updates it. `--verbose` adds paths and diagnostics. `--quiet`
suppresses informational output while preserving results, errors, and the
line that says which release answered.

## Updating after a framework upgrade

After moving the app to a new Teksilo release, install the matching tool, then
update what it installed:

```sh
cargo install cargo-teksilo --version <version> --locked
cargo teksilo update
```

`update` lists what differs from this release and asks before changing it: the
probe harness, the project's agent instructions, and the
`teksilo-automation-mcp` and `cargo-teksilo-fmt` binaries on PATH, which it
reinstalls with `cargo install` into the root they came from. Nothing absent is
installed. Use `-y` in scripts, `-i` to confirm each action (it also shows the
equivalent command, such as `cargo teksilo agent install claude --force`), or
`--dry-run` to list the changes and make none.

```sh
cargo teksilo update --agent    # the project's agent instructions only
cargo teksilo update --user     # the instructions under your home directory instead
```

An agent copy that differs from this release may hold local edits, which
`update` replaces once confirmed. Probe files with local edits are listed by
name; with `-y`, they are replaced only with `--force`. A tool older than the
app is refused, because it would write an older release's files; `status` says
so and prints the `cargo install` command.

`update` and `init` never wait for an answer nobody can give. When stdin is not
a terminal, or `$CI` or `$CLAUDECODE` is set, they print the listing and exit
with an error that names `-y`, and `-i` refuses before printing anything. The
shipped instructions tell an agent to run `--dry-run`, show the user the list,
and pass `-y` once they agree. A session that is already running keeps the
instructions it started with, and an MCP client keeps the server it spawned, so
`update` ends by saying which to restart.

## Semantic search

Download the model explicitly:

```sh
cargo teksilo model fetch
```

Until the model is cached, search uses BM25. Initialization and lookup commands
do not download weights. `search --lexical` always uses BM25. To build without
ONNX Runtime:

```sh
cargo install cargo-teksilo --no-default-features
```

## Focused search

Use an exact type name when you know it. Matching headings receive a score bonus
of at most 10%, keeping relevance to the full query primary. Ordinary prose
queries retain the usual ranking. To exclude unrelated domains, restrict the
corpus path:

```sh
cargo teksilo search "scrolling history" --path docs/charts.md
cargo teksilo search "worker samples" --path examples/chart_demo/ --kind example
```

`--path` is a case-sensitive prefix over bundled paths, applied to both lexical
and semantic candidates before fusion. It does not search your filesystem.
An unmatched prefix returns no results. Use `show --list` to find available paths.
The app guide is also searchable at `crates/teksilo/src/app_guide.md`.

## Version matching

Patch differences produce a short note. Major and minor mismatches are refused.
Within a workspace, resolution follows the selected app. Conflicting framework
versions at a virtual workspace root are rejected; run from the intended app.
For unpublished versions, install from the matching framework checkout with
`cargo install --path <checkout>/crates/cargo-teksilo --locked`.

## CLI migration

The previous interface has been removed:

| Previous | Replacement |
| --- | --- |
| `setup` | `init` |
| `setup --user` | `agent install <agents...> --user` |
| `probe` | `probe install` |
| `version` | `--version` for the tool; `status` for the app |
| Automatic model download / `--no-model` | Explicit `model fetch` |

## Reference

- [Command source](../crates/cargo-teksilo/src/)
- [Automation MCP](automation-mcp.md)
- [Engineering reference](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/agent-tooling.md)
