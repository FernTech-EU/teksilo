<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# `cargo bastyde-fmt` — formatter for `bati!` blocks

`rustfmt` treats macro bodies as opaque token streams and won't descend
into a `bati!(...)` invocation, so the DSL inside is hand-formatted by
default. `cargo bastyde-fmt` fills the gap: it walks Rust source files,
finds every `bati!` invocation, reformats the body, and writes the
file back in place. Source outside `bati!` blocks is byte-for-byte
unchanged — `cargo fmt` still owns Rust formatting.

For the surface language the formatter normalizes, see
[bati-macro-reference.md](bati-macro-reference.md).

---

## Installation

The crate ships in this workspace under
[crates/cargo-bastyde-fmt](../crates/cargo-bastyde-fmt/). Install it with:

```sh
cargo install --path crates/cargo-bastyde-fmt
```

`cargo install` puts the binary in `~/.cargo/bin/cargo-bastyde-fmt`,
which Cargo picks up as the `cargo bastyde-fmt` subcommand from any
directory. Verify with:

```sh
cargo bastyde-fmt --version
```

To uninstall: `cargo uninstall cargo-bastyde-fmt`.

---

## Usage

```text
cargo bastyde-fmt [OPTIONS] [paths...]

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
cargo bastyde-fmt                              # format from CWD
cargo bastyde-fmt --check                      # CI mode: exit 1 if dirty
cargo bastyde-fmt examples/widget_catalog      # format one example
cargo bastyde-fmt src/main.rs src/build.rs     # format specific files
```

Files containing no `bati!` token are skipped before parsing — there's
no measurable cost on a workspace where most modules don't use the DSL.

Writes are atomic: the formatted output goes into a sibling
`NamedTempFile` and is `rename`d into place. An interrupted run never
leaves a truncated source file on disk.

---

## What gets normalized

The formatter rewrites the **shape** of `bati!` bodies. Rust
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
  same line: `bati!(ctx =>\n    VStack { … })` becomes
  `bati!(ctx => VStack { … })`.
- Continuation lines are aligned to where the user already had the
  body's outermost `}` in source — the closing brace of the
  reformatted output lands at the same column.

---

## Trivia preservation

Comments and blank lines between body items survive a reformat.

```rust
// before
bati!(ctx =>
    VStack {
        spacing: 12.0
        // user-added section header
        Button("Save")

        Button("Cancel")
    }
)

// after — unchanged
bati!(ctx =>
    VStack {
        spacing: 12.0
        // user-added section header
        Button("Save")

        Button("Cancel")
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

Comments **inside** Rust expressions (e.g. `Button(/* note */ "ok")`)
are preserved automatically because expression values are sliced
verbatim from source — they ride along with the rest of the slice.

---

## CI integration

Use `--check` to fail the build when any file would be reformatted:

```yaml
# .github/workflows/lint.yml (or equivalent)
- name: bastyde-fmt
  run: cargo bastyde-fmt --check
```

`--check` is read-only, prints `Would reformat: <path>` for each dirty
file, and exits 1 if any.

For a pre-commit hook, run without `--check`:

```sh
# .git/hooks/pre-commit
cargo bastyde-fmt --quiet
git add -u
```

---

## Library API

The CLI is a thin wrapper around the [bastyde-fmt](../crates/bastyde-fmt/)
library crate. Editor integrations can call into it directly:

```rust
use bastyde_fmt::{format_block, format_file, FmtConfig, FmtError};

// Format a single bati! body string (the contents inside the macro parens):
let cfg = FmtConfig::default();
let formatted = format_block("ctx => VStack { spacing: 8.0 }", &cfg)?;

// Format every bati! invocation in a Rust file's source:
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

The [bastyde-fmt-lsp](../crates/bastyde-fmt-lsp/) crate ships a minimal
Language Server Protocol server that wraps the formatter. Install:

```sh
cargo install --path crates/bastyde-fmt-lsp
```

The server speaks JSON-RPC over stdio and advertises a single
capability: `documentFormattingProvider`. Wire it into your editor
as a *secondary* formatter for Rust files (`rust-analyzer` stays
your primary).

### Helix

```toml
# ~/.config/helix/languages.toml
[language-server.bastyde-fmt-lsp]
command = "bastyde-fmt-lsp"

[[language]]
name = "rust"
language-servers = [{ name = "rust-analyzer" }, { name = "bastyde-fmt-lsp" }]
```

### VS Code

VS Code needs an extension to register an LSP server. See the
dedicated walkthrough in [bastyde-fmt-vscode.md](bastyde-fmt-vscode.md) —
it covers two paths: a five-minute *Run on Save* hook (no LSP) and a
small custom extension that registers `bastyde-fmt-lsp` for Rust
documents.

### Neovim (`nvim-lspconfig`)

```lua
local configs = require('lspconfig.configs')
configs.bastyde_fmt_lsp = {
  default_config = {
    cmd = { 'bastyde-fmt-lsp' },
    filetypes = { 'rust' },
    root_dir = require('lspconfig.util').root_pattern('Cargo.toml'),
    settings = {},
  },
}
require('lspconfig').bastyde_fmt_lsp.setup{}
```

### Behavior

- On every `textDocument/formatting` request, the server runs
  `bastyde_fmt::format_file` on the buffer contents and returns either an
  empty edit list (already canonical) or a single full-document
  `TextEdit` (entire buffer replaced with formatted output).
- A parse error in the host file or in any `bati!` body is treated as
  "leave it alone" — the server returns empty edits, mirroring how
  `rustfmt` behaves on save when Rust source is mid-edit.
- Document sync is full (mode 1): every change resends the whole
  text. Cheaper than maintaining incremental-diff state for a
  format-only server.

---

## Architecture

Four crates, layered:

- **[bastyde-parse](../crates/bastyde-parse/)** — parser and IR for the
  `bati!` DSL. Extracted from `bastyde-macros` so non-proc-macro
  consumers (the formatter, future linters, editor tooling) can build
  on the same grammar without depending on a `proc-macro = true`
  crate. The proc-macro crate now depends on it.
- **[bastyde-fmt](../crates/bastyde-fmt/)** — pure formatter library.
  Pretty-printer, byte-range trivia scanner, host-file `bati!`-macro
  visitor, LF/CRLF detection.
- **[cargo-bastyde-fmt](../crates/cargo-bastyde-fmt/)** — CLI binary. File
  walker, in-place rewriter with atomic writes, `--check` mode.
- **[bastyde-fmt-lsp](../crates/bastyde-fmt-lsp/)** — LSP server binary.
  Hand-rolled JSON-RPC over stdio (no tokio); thin wrapper around
  `format_file`.

The CLI and LSP have no `bati!` grammar knowledge — all parsing and
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

- [bati-macro-reference.md](bati-macro-reference.md) — surface
  language for the DSL the formatter operates on.
- [bati-language-spec-v3.md](bati-language-spec-v3.md) — design spec
  with worked translations of widget-catalog examples.
- [crates/bastyde/tests/bati/pass/](../crates/bastyde/tests/bati/pass/)
  — trybuild fixtures that double as canonical examples of well-
  formatted `bati!` blocks.
