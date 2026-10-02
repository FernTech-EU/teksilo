<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Agent tooling

`cargo teksilo` provides version-aware API lookup, documentation search, and an
application automation harness. Run it inside the application's Cargo project.

## Setup

```sh
cargo install cargo-teksilo
cargo teksilo setup
```

Setup installs the probe harness under `scripts/teksilo_probe/` and configures
supported coding agents already present in the project. `--user` installs agent
instructions at user scope; the harness remains project-local.

## Common operations

```sh
cargo teksilo symbol Button
cargo teksilo symbol --crate data ListModel
cargo teksilo search "make a list scrollable"
cargo teksilo show docs/scroll-area.md
cargo teksilo show docs/scroll-area.md --lines 166-172
cargo teksilo probe
cargo teksilo status
cargo teksilo version
```

| Command | Purpose |
| --- | --- |
| `symbol` | Extract public API signatures from crate source |
| `search` | Find guides and worked examples in the bundled corpus |
| `show` | Read a corpus document without a checkout or network request |
| `probe` | Install the automation harness |
| `setup` | Install the harness and supported agent instructions |
| `status` | Inspect installed instructions, harness, and search model |
| `version` | Compare tool and resolved framework versions |

A search result's `docs/...` path belongs to the bundled corpus. Use `show` to
read it; it need not exist in the application's directory.

## Version requirements

The tool reads the framework version resolved by Cargo. An exact match is
accepted; a patch-version difference produces a note; a major or minor
mismatch is refused. The tool was introduced in Teksilo 0.13.0.

Use the tool version matching the application. Online documentation follows
the repository and can describe a newer API than the application has installed.

## Search without ONNX Runtime

If the default installation cannot build ONNX Runtime, install lexical search:

```sh
cargo install cargo-teksilo --no-default-features
```

API lookup, document retrieval, and harness setup remain available.

## Reference

- [Command source](../crates/cargo-teksilo/src/)
- [Automation MCP](automation-mcp.md)
- [Corpus build tool](../tools/build_corpus.py)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/agent-tooling.md)
are retained in the repository.
