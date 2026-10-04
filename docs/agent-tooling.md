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
`symbol` requires Python 3; other commands do not.

`status` reports the resolved app version and installed tooling without
changing the lockfile. `--verbose` adds paths and diagnostics. `--quiet`
suppresses informational output while preserving results and errors.

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
