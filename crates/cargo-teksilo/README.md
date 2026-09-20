<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# cargo-teksilo

Agent tooling for applications built on [Teksilo](https://crates.io/crates/teksilo):
exact API lookup, offline documentation search, and the automation probe harness —
all matched to the teksilo version **your** app resolved.

```bash
cargo install cargo-teksilo
```

Install the version your app resolved, not the newest — the tool refuses to
answer across a minor. If your app pins teksilo by `path` or `git`, that version
was never published, so install from the framework checkout instead:

```bash
cargo install --path <teksilo checkout>/crates/cargo-teksilo --locked
```

## Why it exists

Four things never leave the Teksilo repository: the guides under `docs/` ship in
no crate, every example crate is `publish = false`, the agent skill lives in
`.claude/`, and the probe harness lived only in one app's repo. An agent working
in your project has the crate source and a lockfile; everything else is either
missing or is `main` on GitHub, which is a *different* Teksilo from the one you
pinned.

This tool closes that gap, and closes it **version-matched**. It reads your
`Cargo.lock` through `cargo metadata`, and when its own version cannot serve the
teksilo your app resolved, it refuses and says so rather than answering from a
different API. A bare "not found" is how a model ends up confidently inventing a
widget that was renamed three releases ago.

## Commands

```bash
cargo teksilo symbol Button           # exact public API, for the version you pin
cargo teksilo symbol ListModel        # any type in any teksilo crate, no flag needed
cargo teksilo search "<question>"     # hybrid BM25 + vector over guides and examples
cargo teksilo show <corpus path>      # a hit's document in full, offline
cargo teksilo probe                   # write the probe harness into scripts/teksilo_probe/
cargo teksilo setup                   # brief every agent configured in this project
cargo teksilo status                  # what is installed, and what is missing
```

`setup` writes each agent's own format — the full skill for Claude Code, and a
self-contained brief wearing the right frontmatter for Cursor, Windsurf, Copilot
and `AGENTS.md`. Shared files are edited through a marker region, so a re-run is
a byte-for-byte no-op.

## Two runtime requirements, both contained

**`symbol` needs `python3` on PATH.** The API extractor is a Python script, kept
byte-identical to the framework's own copy under a CI diff so there is exactly
one source of truth for the public surface. Every other command works without it.
`cargo teksilo status` reports whether the interpreter is there.

**`search` is better with the `semantic` feature, and works without it.**
`semantic` is default-on and pulls `fastembed` → ONNX Runtime plus two C/C++
`sys` crates. If that will not build on your machine:

```bash
cargo install cargo-teksilo --no-default-features
```

That is a fully working tool — `symbol`, `show`, `probe`, `setup`, and BM25
`search`. CI builds both configurations on Linux, macOS and Windows, so the
escape hatch is proven rather than hoped for. Hybrid retrieval is supported on
those three; musl/Alpine, BSD, 32-bit and air-gapped machines get the lexical
path, documented here rather than discovered on failure.

The encoder weights (~129 MB) download once into a per-user cache
(`$XDG_CACHE_HOME` on Linux, `~/Library/Caches/teksilo/fastembed` on macOS,
`%LOCALAPPDATA%` on Windows), overridable with `FASTEMBED_CACHE_DIR`. They are
never written into your project. `cargo teksilo setup` fetches them eagerly so
the cost lands on the command that announced it; `--no-model` leaves it lazy.
If the encoder cannot initialise for any reason, `search` falls back to BM25 and
says so in its header.

## One binary per machine

The version guard is per-invocation, but `cargo install` puts one binary on your
PATH. If you work in two apps on different teksilo minors, install the second
with `--root <dir>` and put that `<dir>/bin` first on PATH for that tree, or run
the tool straight out of a framework checkout:

```bash
cargo run -p cargo-teksilo -- teksilo symbol Button
```

## License

MPL-2.0. Copyright (c) 2026 FernTech.
