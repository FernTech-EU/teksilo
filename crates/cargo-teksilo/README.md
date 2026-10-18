<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# cargo-teksilo

API lookup, offline documentation, and agent tooling for applications built on
[Teksilo](https://crates.io/crates/teksilo).

Install the version your application resolves (0.13.0 or later):

```sh
cargo install cargo-teksilo --version <version> --locked
```

For unpublished versions, use the matching framework checkout:

```sh
cargo install --path <checkout>/crates/cargo-teksilo --locked
```

## Commands

Run inside your application's Cargo project:

```sh
cargo teksilo symbol Button
cargo teksilo search "scrollable list"
cargo teksilo show docs/scroll-area.md
cargo teksilo init --agent codex --agent claude
cargo teksilo agent install cursor cline
cargo teksilo agent list
cargo teksilo probe install
cargo teksilo update
cargo teksilo model fetch
cargo teksilo status --json
cargo teksilo --version
```

An application that keeps teksilo behind a Cargo feature needs nothing extra:
when its default features leave teksilo out, the commands resolve with every
feature enabled, and `status` names the feature that turns teksilo on.

`init` installs the probe harness and agent instructions. Without `--agent`, it
uses existing configuration to detect targets. Explicit targets create their
required directories, even when no marker exists. Use `-y` to skip confirmation.

`agent install` writes instructions only, without a confirmation prompt. Targets:
`claude`, `cursor`, `windsurf`, `cline`, `copilot`, `codex`, `vibe`, `opencode`.
Claude receives a full skill; the others receive rules or a managed `AGENTS.md`
section. `--user` supports `claude`, `vibe`, and `opencode` and writes no harness.
Shared-file text outside the managed region is preserved. `--force` replaces
conflicting generated files.

`update` brings what is already installed to this release: the probe harness,
the project's agent instructions, and the `teksilo-automation-mcp` and
`cargo-teksilo-fmt` binaries on PATH, reinstalled with `cargo install`. It lists
the changes and asks before making them; `-y` skips the question, and `-i` asks
per action, showing the equivalent command; `--dry-run` only lists. Where
nobody can answer (stdin is not a terminal, or `$CI` or `$CLAUDECODE` is set),
it prints the list and exits with an error naming `-y`, rather than waiting.
`--agent` limits it to the
project's agent instructions; `--user` updates the instructions under the home
directory instead, and needs no project. It installs nothing that is absent,
and refuses when the tool is older than the app: install the matching
cargo-teksilo first. With `-y`, probe files with local edits are replaced only
with `--force`.

`status` reports the tool, the app's Teksilo, the harness, the search model,
and the automation server and formatter on PATH. A tool, server or formatter
older than the app's release is reported with the command that updates it.

`--quiet` suppresses informational messages but still says which release
answered; `--verbose` shows paths and diagnostics. `search --json`, `status --json`, and `agent list --json` emit
structured output. API declarations and document contents are never shortened.

## Requirements

`symbol` requires Python 3. Semantic search uses ONNX Runtime and a model
(~129 MB) downloaded explicitly by `model fetch`. Until cached, search uses
BM25; neither `init` nor `search` downloads the model. The model lives in the
user cache, overridable with `FASTEMBED_CACHE_DIR`.

To build without semantic search:

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

The tool follows the selected app's resolved dependency graph. Patch differences
produce a note; major or minor mismatches are refused. Multiple framework
versions at a virtual workspace root require running from the intended member.

For apps on different minors, install separate tools with `cargo install --root
<directory>` and select the appropriate binary on PATH.

The old `setup`, bare `probe`, and `version` commands are replaced by `init`,
`probe install`, and `--version`/`status`. `--no-model` is removed; model download
is explicit.

## License

MPL-2.0. Copyright (c) 2026 FernTech.
