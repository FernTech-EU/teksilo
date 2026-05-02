# `cargo fern-fmt` — formatter for `fern!` blocks

`rustfmt` treats macro bodies as opaque token streams and won't descend
into a `fern!(...)` invocation, so the DSL inside is hand-formatted by
default. `cargo fern-fmt` fills the gap: it walks Rust source files,
finds every `fern!` invocation, reformats the body, and writes the
file back in place. Source outside `fern!` blocks is byte-for-byte
unchanged — `cargo fmt` still owns Rust formatting.

For the surface language the formatter normalizes, see
[fern-macro-reference.md](fern-macro-reference.md).

---

## Installation

The crate ships in this workspace under
[crates/cargo-fern-fmt](../crates/cargo-fern-fmt/). Install it with:

```sh
cargo install --path crates/cargo-fern-fmt
```

`cargo install` puts the binary in `~/.cargo/bin/cargo-fern-fmt`,
which Cargo picks up as the `cargo fern-fmt` subcommand from any
directory. Verify with:

```sh
cargo fern-fmt --version
```

To uninstall: `cargo uninstall cargo-fern-fmt`.

---

## Usage

```text
cargo fern-fmt [OPTIONS] [paths...]

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
cargo fern-fmt                              # format from CWD
cargo fern-fmt --check                      # CI mode: exit 1 if dirty
cargo fern-fmt examples/widget_catalog      # format one example
cargo fern-fmt src/main.rs src/build.rs     # format specific files
```

Files containing no `fern!` token are skipped before parsing — there's
no measurable cost on a workspace where most modules don't use the DSL.

Writes are atomic: the formatted output goes into a sibling
`NamedTempFile` and is `rename`d into place. An interrupted run never
leaves a truncated source file on disk.

---

## What gets normalized

The formatter rewrites the **shape** of `fern!` bodies. Rust
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
  same line: `fern!(ctx =>\n    VStack { … })` becomes
  `fern!(ctx => VStack { … })`.
- Continuation lines are aligned to where the user already had the
  body's outermost `}` in source — the closing brace of the
  reformatted output lands at the same column.

---

## Trivia preservation

Comments and blank lines between body items survive a reformat.

```rust
// before
fern!(ctx =>
    VStack {
        spacing: 12.0
        // user-added section header
        Button("Save")

        Button("Cancel")
    }
)

// after — unchanged
fern!(ctx =>
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
- name: fern-fmt
  run: cargo fern-fmt --check
```

`--check` is read-only, prints `Would reformat: <path>` for each dirty
file, and exits 1 if any.

For a pre-commit hook, run without `--check`:

```sh
# .git/hooks/pre-commit
cargo fern-fmt --quiet
git add -u
```

---

## Library API

The CLI is a thin wrapper around the [fern-fmt](../crates/fern-fmt/)
library crate. Editor integrations can call into it directly:

```rust
use fern_fmt::{format_block, format_file, FmtConfig, FmtError};

// Format a single fern! body string (the contents inside the macro parens):
let cfg = FmtConfig::default();
let formatted = format_block("ctx => VStack { spacing: 8.0 }", &cfg)?;

// Format every fern! invocation in a Rust file's source:
let new_source = format_file(&source_text, &cfg)?;
```

Both functions are pure: they take a `&str` and return a `String`.
There's no I/O at the library level.

`FmtConfig` is empty in v1 — every invocation produces canonical
output. Style knobs may be added later; defaults will not change for
existing invocations.

---

## Architecture

Three crates, layered:

- **[fern-parse](../crates/fern-parse/)** — parser and IR for the
  `fern!` DSL. Extracted from `fern-ui-macros` so non-proc-macro
  consumers (the formatter, future linters, editor tooling) can build
  on the same grammar without depending on a `proc-macro = true`
  crate. The proc-macro crate now depends on it.
- **[fern-fmt](../crates/fern-fmt/)** — pure formatter library.
  Pretty-printer, byte-range trivia scanner, host-file `fern!`-macro
  visitor.
- **[cargo-fern-fmt](../crates/cargo-fern-fmt/)** — CLI binary. File
  walker, in-place rewriter with atomic writes, `--check` mode.

The CLI has no fern! grammar knowledge — all parsing and printing
happens in the library crates.

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
- **No editor integration** in v1. The library API is shaped to
  support an LSP server later (a `format_block(&str)` boundary is all
  rust-analyzer needs), but no LSP plumbing is shipped.
- **No configurable rules**. Indent width, brace style, and reflow
  thresholds are fixed. Configurability may come in a later version
  through `FmtConfig`.

---

## Related

- [fern-macro-reference.md](fern-macro-reference.md) — surface
  language for the DSL the formatter operates on.
- [fern-language-spec-v3.md](fern-language-spec-v3.md) — design spec
  with worked translations of widget-catalog examples.
- [crates/fern-ui/tests/fern_ui/pass/](../crates/fern-ui/tests/fern_ui/pass/)
  — trybuild fixtures that double as canonical examples of well-
  formatted `fern!` blocks.
