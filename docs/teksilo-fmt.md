<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# `cargo teksilo-fmt` — formatter for `teksu!` blocks

`rustfmt` treats macro bodies as opaque token streams and won't descend
into a `teksu!(...)` invocation, so the DSL inside is hand-formatted by
default. `cargo teksilo-fmt` fills the gap: it walks Rust source files,
finds every `teksu!` invocation, reformats the body, and writes the
file back in place. Source outside `teksu!` blocks is byte-for-byte
unchanged — `cargo fmt` still owns Rust formatting.

For the surface language the formatter normalizes, see
[teksu-macro-reference.md](teksu-macro-reference.md).

---

## Installation

The crate ships in this workspace under
[crates/cargo-teksilo-fmt](../crates/cargo-teksilo-fmt/). Install it with:

```sh
cargo install --path crates/cargo-teksilo-fmt
```

`cargo install` puts the binary in `~/.cargo/bin/cargo-teksilo-fmt`,
which Cargo picks up as the `cargo teksilo-fmt` subcommand from any
directory. Verify with:

```sh
cargo teksilo-fmt --version
```

To uninstall: `cargo uninstall cargo-teksilo-fmt`.

---

## Usage

```text
cargo teksilo-fmt [OPTIONS] [paths...]

OPTIONS:
    --check                 Read-only; exit 1 if any file would change
    --quiet, -q             Suppress per-file output
    --help, -h              Print help
    --version, -V           Print version

POSITIONAL:
    paths                   Files or directories to format. Directories
                            are walked recursively for *.rs, skipping
                            target/ and hidden directories. Defaults to
                            the current directory.
```

Examples:

```sh
cargo teksilo-fmt                              # format from CWD
cargo teksilo-fmt --check                      # CI mode: exit 1 if dirty
cargo teksilo-fmt examples/widget_catalog      # format one example
cargo teksilo-fmt src/main.rs src/build.rs     # format specific files
```

Files containing no `teksu!` token are skipped before parsing — there's
no measurable cost on a workspace where most modules don't use the DSL.

Writes are atomic: the formatted output goes into a sibling
`NamedTempFile` and is `rename`d into place. An interrupted run never
leaves a truncated source file on disk.

---

## What gets normalized

The formatter rewrites the **shape** of `teksu!` bodies. Rust
expressions inside (property values, positional args, closure bodies,
escape exprs, `rust { … }` blocks) are spliced verbatim from source —
the formatter does not reformat Rust.

Layout rules (v1):

- 4-space indent per nesting level.
- Elements with a non-empty body span multiple lines:
  `Type(args) {\n    <items>\n}`.
- Elements with no body or with an empty `{}` body emit on one line
  without braces: `Divider { }` → `Divider`.
- One body item per line. Properties keep `name: value` shape.
- Property order is preserved verbatim. The macro lowering reorders
  handler properties (`on_tap`, `cursor`, …) to the end of the chain
  at compile time; that's a lowering concern, not the formatter's, so
  what you wrote is what you get back.
- The `ctx => <element>` preamble joins to the root element on the
  same line: `teksu!(ctx =>\n    VStack { … })` becomes
  `teksu!(ctx => VStack { … })`.
- Continuation lines are aligned to where the user already had the
  body's outermost `}` in source — the closing brace of the
  reformatted output lands at the same column.

---

## Trivia preservation

Comments and blank lines between body items survive a reformat.

```rust
// before
teksu!(ctx =>
    VStack {
        spacing: 12.0
        // user-added section header
        Button(lit!("Save"))

        Button(lit!("Cancel"))
    }
)

// after — unchanged
teksu!(ctx =>
    VStack {
        spacing: 12.0
        // user-added section header
        Button(lit!("Save"))

        Button(lit!("Cancel"))
    }
)
```

How it works: `syn::ParseStream` discards comments before they reach
the IR, so the formatter runs a separate pass over the original
`TokenStream` using `proc_macro2::Span::byte_range()` to record the
inter-token gaps in source. The pretty-printer then drains that table
by byte offset, emitting each comment / blank-line marker at the
right indent level before the next body item.

Multiple consecutive blank lines collapse to a single blank line.

Comments **inside** Rust expressions (e.g. `Button(/* note */ lit!("ok"))`)
are preserved automatically because expression values are sliced
verbatim from source — they ride along with the rest of the slice.

---

## CI integration

Use `--check` to fail the build when any file would be reformatted:

```yaml
# .github/workflows/lint.yml (or equivalent)
- name: teksilo-fmt
  run: cargo teksilo-fmt --check
```

`--check` is read-only, prints `Would reformat: <path>` for each dirty
file, and exits 1 if any.

For a pre-commit hook, run without `--check`:

```sh
# .git/hooks/pre-commit
cargo teksilo-fmt --quiet
git add -u
```

---

## Library API

The CLI is a thin wrapper around the [teksilo-fmt](../crates/teksilo-fmt/)
library crate. Editor integrations can call into it directly:

```rust
use teksilo_fmt::{format_block, format_file, FmtConfig, FmtError};

// Format a single teksu! body string (the contents inside the macro parens):
let cfg = FmtConfig::default();
let formatted = format_block("ctx => VStack { spacing: 8.0 }", &cfg)?;

// Format every teksu! invocation in a Rust file's source:
let new_source = format_file(&source_text, &cfg)?;
```

Both functions are pure: they take a `&str` and return a `String`.
There's no I/O at the library level. `format_file` detects the host
file's line ending convention (LF or CRLF) and applies it to every
newline the formatter emits, so a CRLF file round-trips as CRLF.

`FmtConfig` is empty in v1 — every invocation produces canonical
output. Style knobs may be added later; defaults will not change for
existing invocations.

---

## Editor integration (LSP)

The [teksilo-fmt-lsp](../crates/teksilo-fmt-lsp/) crate ships a minimal
Language Server Protocol server that wraps the formatter. Install:

```sh
cargo install --path crates/teksilo-fmt-lsp
```

The server speaks JSON-RPC over stdio and advertises a single
capability: `documentFormattingProvider`. Wire it into your editor
as a *secondary* formatter for Rust files (`rust-analyzer` stays
your primary).

### Helix

```toml
# ~/.config/helix/languages.toml
[language-server.teksilo-fmt-lsp]
command = "teksilo-fmt-lsp"

[[language]]
name = "rust"
language-servers = [{ name = "rust-analyzer" }, { name = "teksilo-fmt-lsp" }]
```

### VS Code

VS Code needs an extension to register an LSP server. See the
dedicated walkthrough in [teksilo-fmt-vscode.md](teksilo-fmt-vscode.md) —
it covers two paths: a five-minute *Run on Save* hook (no LSP) and a
small custom extension that registers `teksilo-fmt-lsp` for Rust
documents.

### Neovim (`nvim-lspconfig`)

```lua
local configs = require('lspconfig.configs')
configs.teksilo_fmt_lsp = {
  default_config = {
    cmd = { 'teksilo-fmt-lsp' },
    filetypes = { 'rust' },
    root_dir = require('lspconfig.util').root_pattern('Cargo.toml'),
    settings = {},
  },
}
require('lspconfig').teksilo_fmt_lsp.setup{}
```

### Behavior

- On every `textDocument/formatting` request, the server runs
  `teksilo_fmt::format_file` on the buffer contents and returns either an
  empty edit list (already canonical) or a single full-document
  `TextEdit` (entire buffer replaced with formatted output).
- A parse error in the host file or in any `teksu!` body is treated as
  "leave it alone" — the server returns empty edits, mirroring how
  `rustfmt` behaves on save when Rust source is mid-edit.
- Document sync is full (mode 1): every change resends the whole
  text. Cheaper than maintaining incremental-diff state for a
  format-only server.

---

## Architecture

Four crates, layered:

- **[teksilo-parse](../crates/teksilo-parse/)** — parser and IR for the
  `teksu!` DSL. Extracted from `teksilo-macros` so non-proc-macro
  consumers (the formatter, future linters, editor tooling) can build
  on the same grammar without depending on a `proc-macro = true`
  crate. The proc-macro crate now depends on it.
- **[teksilo-fmt](../crates/teksilo-fmt/)** — pure formatter library.
  Pretty-printer, byte-range trivia scanner, host-file `teksu!`-macro
  visitor, LF/CRLF detection.
- **[cargo-teksilo-fmt](../crates/cargo-teksilo-fmt/)** — CLI binary. File
  walker, in-place rewriter with atomic writes, `--check` mode.
- **[teksilo-fmt-lsp](../crates/teksilo-fmt-lsp/)** — LSP server binary.
  Hand-rolled JSON-RPC over stdio (no tokio); thin wrapper around
  `format_file`.

The CLI and LSP have no `teksu!` grammar knowledge — all parsing and
printing happens in the library crates.

---

## Limitations (v1)

- **Closure / multi-line expression bodies** inside property values
  use uniform dedent + reindent. The output is round-trip stable
  (`format(format(x)) == format(x)`) but brace alignment inside a
  closure body may drift one level from what hand-formatting would
  produce. Hand-fix as needed; the alignment doesn't affect parsing.
- **Empty bodies normalize to bodyless form**: `Divider { }` becomes
  `Divider`. The two are semantically identical to the macro; the
  formatter picks one.
- **`ctx =>` line-break collapse**: a `ctx =>` preamble joins to the
  root element on the same line, even if the user wrote them across
  lines. Multi-line preambles aren't a documented form anywhere; the
  formatter standardizes on the joined shape.
- **No configurable rules**. Indent width, brace style, and reflow
  thresholds are fixed. Configurability may come in a later version
  through `FmtConfig`.

---

## Related

- [teksu-macro-reference.md](teksu-macro-reference.md) — surface
  language for the DSL the formatter operates on.
- [teksu-language-spec-v3.md](teksu-language-spec-v3.md) — design spec
  with worked translations of widget-catalog examples.
- [crates/teksilo/tests/teksu/pass/](../crates/teksilo/tests/teksu/pass/)
  — trybuild fixtures that double as canonical examples of well-
  formatted `teksu!` blocks.
