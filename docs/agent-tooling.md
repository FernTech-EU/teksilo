<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Agent tooling for Teksilo apps (`cargo teksilo`)

An AI coding agent working **inside this repository** is well served: it has the
guides under `docs/`, 56 worked examples, the `teksilo` skill in `.claude/`, and
the automation bridge with a harness to drive it.

An agent working in someone's Teksilo **app** had none of that, because none of
it leaves the repository:

| Reaches a consumer | Does not |
| --- | --- |
| Crate source + `///` docs (via the registry cache) | `docs/**.md` — in no crate |
| `tests/` (shipped in the `.crate`) | `examples/*` — every one is `publish = false` |
| | `.claude/skills/` — agent instructions |
| | The probe harness for the automation bridge |

`cargo-teksilo` closes that gap. One install, and everything it answers is
matched to the Teksilo version *that app* resolved.

```bash
cargo install cargo-teksilo
cargo teksilo setup        # in the app: harness + a brief for every agent it finds
```

Verified against teksilo 0.12.1.

---

## 1. The commands

```bash
cargo teksilo symbol <Name>...   # exact public API of a type
cargo teksilo search "<query>"   # the guides and worked examples
cargo teksilo show <path>        # one of them in full, offline
cargo teksilo probe [--force]    # write the automation harness into the project
cargo teksilo setup [-y] [--user] # probe + brief every agent configured here
cargo teksilo version            # this tool, and the app's resolved Teksilo
```

`cargo teksilo build-vectors` also exists and is **maintainer-only** — see §6.

### `symbol`

The public surface of a type, for the version the app pins — not the newest
version that exists.

```console
$ cargo teksilo symbol Button
$ cargo teksilo symbol --crate data ListModel
$ cargo teksilo symbol -f json ComboBox
$ cargo teksilo symbol Theme
note: 'Theme' isn't in teksilo-widgets; resolved via the teksilo umbrella
      prelude to teksilo-core (--crate core).
```

It accepts [`tools/extract_widget_api.py`](../tools/extract_widget_api.py)'s own
flags (`--list`, `--all`, `-f`, `--crate`), because it *is* that extractor: 30
crates are queryable, and a name reached through the `teksilo` umbrella prelude
resolves to its owning crate rather than reporting "not found".

### `search`

Retrieval over the 69 hand-written guides and the 56 worked example crates — the material
that reaches no consumer today.

```console
$ cargo teksilo search "make a list scrollable"
$ cargo teksilo search "stop my window closing" --kind example
$ cargo teksilo search "focus ring" --lexical      # BM25 only
```

Hybrid by default: BM25 fused with vector similarity by reciprocal-rank fusion.
The header line always says which mode ran, so lexical results are never
presented as semantic.

A hit cites a path — `docs/scroll-area.md` — and that path does not exist in the
consumer's project, which is the whole reason the corpus ships. Left bare it
invites the two wrong moves: opening it locally, where it is absent, or fetching
it from `blob/main/` or the published book, both of which track `main` rather
than the version the app pinned. So a non-empty result ends with one footer line
naming the door that is offline *and* version-matched:

```
Read any of these in full: cargo teksilo show <path>   (offline, teksilo 0.12.1 — not GitHub, which tracks main)
```

### `show`

The document behind a hit, in full, reassembled from the corpus — no network, no
checkout, and matched to the pinned version by construction.

```console
$ cargo teksilo show docs/scroll-area.md
$ cargo teksilo show docs/scroll-area.md --lines 166-172   # the lines a hit cited
$ cargo teksilo show examples/simple_button/src/main.rs
$ cargo teksilo show --list                                # every path, one per line
```

Nothing is re-fetched and nothing is re-embedded: every chunk already carries its
own text and the 0-based inclusive line range it occupied in the original file,
and the generator's chunking covers every non-blank line of all 158 documents.
Laying the chunks back at their offsets leaves gaps exactly where the chunker
dropped a blank separator line, so the gaps come back as blank lines and the
result is the original **byte for byte** — checked for all 158 by
`every_document_reconstructs_byte_exactly`, which reads the real files whenever
the tests run inside a checkout.

`--lines` is **1-based and inclusive**, which is what `search` prints under a hit
(`(lines 166-172)`) and what an editor's gutter shows; the index stores 0-based
offsets, so the conversion happens exactly once, in `show::slice`. A range that
ends past the end clamps; one that *starts* past it is an error naming the real
length, because printing nothing would read as "this document is empty".

**stdout is the document and nothing else** — the version line, a rewritten path
and a version-mismatch warning all go to stderr — so the output can be
redirected, diffed or piped into a reader without a banner corrupting it.

An unknown path answers with help rather than "not found", since a model that
reads a bare "not found" falls back on its own memory of the guide: a bare
basename or a trailing fragment that names exactly one document is accepted
(`scroll-area.md` → `docs/scroll-area.md`), an ambiguous one lists the
candidates, a directory lists what is under it, and a typo gets its near
spellings plus the two commands that always work.

### `probe`

Writes the automation harness into `scripts/teksilo_probe/`, so an agent can
drive the running app and assert on it. See §4.

### `setup`

The harness, plus the teksilo briefing for **every coding agent this project
already configures** — each in that agent's own format — plus a one-off fetch of
the search encoder so the first `search` does not stall on a 129 MB download.

```bash
cargo teksilo setup              # plan, confirm, install
cargo teksilo setup -y           # skip the confirmation (required in CI)
cargo teksilo setup --no-model   # …and leave the encoder to download lazily
cargo teksilo setup --user       # install into $HOME instead of this project
```

**Per agent, in that agent's format.** These tools share no format, so copying
one directory into all of them accomplishes nothing:

| Agent | Detected by | Written |
| --- | --- | --- |
| Claude Code | `.claude/` | `.claude/skills/teksilo/` — the full four-file skill |
| Cursor | `.cursor/` | `.cursor/rules/teksilo.mdc` — MDC frontmatter (`description` / `globs` / `alwaysApply`) + brief |
| Windsurf | `.windsurf/` | `.windsurf/rules/teksilo.md` — `trigger: glob` frontmatter + brief |
| GitHub Copilot | `.github/` | `.github/copilot-instructions.md` — a delimited section |
| Codex and the rest | `AGENTS.md` | `AGENTS.md` — a delimited section |

The **brief** is a self-contained ~40 lines: what Teksilo is, the five commands,
that every answer is pinned to the version this app resolved, and the rule that
matters most — *read a search hit with `cargo teksilo show <path>`, never from
GitHub, because `blob/main` tracks `main` and not what this app pinned*. It
never refers to the skill, which on most of those machines is not installed.

Four properties worth relying on:

- **Detection is marker-already-exists.** `.cursor/` is never created on the
  chance you might use Cursor. Instructions written where nothing reads them are
  indistinguishable from none, except that they report success. Whatever was
  *not* found is listed at the end, with the marker that would have found it.
- **The shared files are edited through a region.** `AGENTS.md` and
  `copilot-instructions.md` belong to the project; `setup` owns only what lies
  between `<!-- BEGIN teksilo -->` and `<!-- END teksilo -->`. A second run is a
  byte-for-byte no-op, and your own text above or below is never touched.
- **Nothing is written before you see the plan.** Every path, every vendor, and
  the download with its size, then a confirmation. `-y` skips the question; with
  **no terminal on stdin the prompt is an error naming the flag that would have
  skipped it**, never a blocking read — CI and agents run this, and a hang is
  worse than a failure.
- **`--user` is the only mode that writes `$HOME`.** Project scope never falls
  back to it. Three agents keep a user-level file this can write:

  | Agent | Detected by | Written |
  | --- | --- | --- |
  | Claude Code | `~/.claude/` | `~/.claude/skills/teksilo/` — the full skill |
  | Mistral Vibe | `~/.vibe/` (or `$VIBE_HOME`) | `AGENTS.md` — a delimited section |
  | opencode | `~/.config/opencode/` (or `$XDG_CONFIG_HOME`) | `AGENTS.md` — a delimited section |

  Both env vars are honoured, because Vibe relocates its whole state directory
  through `VIBE_HOME` and opencode follows the XDG base directories — writing
  `~/.vibe/AGENTS.md` for someone who moved theirs is a file nothing reads.
  Each is gated on **the directory already existing**: this does not create a
  config directory in your home for a tool you may never have run, and it says
  which path it checked rather than only that it found nothing. The rest have
  nowhere to go, which is a finding rather than an omission: Cursor's user
  rules are edited in its settings UI, Copilot's personal instructions live in
  your github.com account, Windsurf's global rules are one file at
  `~/.codeium/windsurf/memories/global_rules.md`, and a repository-root
  `AGENTS.md` is per-repository by definition. `setup --user` prints that list
  rather than silently installing three of eight.

If the encoder fetch fails — offline, proxy, unsupported target — that is a
**warning** and setup still succeeds: `search` degrades to BM25 by design (§5).
Under `--no-default-features` there is no encoder to fetch and it says so.

---

## 2. Version binding is the design constraint

Serving 0.12 answers to an app on 0.9 is worse than serving nothing: it is
confidently wrong. Between those versions `SplitView` was deleted outright in
favour of `Splitter` with no back-compat, and `ComponentStyleSlots` grew to 42
slots. A wrong answer reads exactly like a right one.

So the tool reads the app's **`Cargo.lock`**, via `cargo metadata` — the
*resolved* graph, never a parsed manifest. That matters: `teksilo = { workspace =
true, features = [...] }` puts the real pin in a different file entirely, and a
manifest parser that does not know this returns `None` and disables the check it
was written to perform.

| Difference | Behaviour |
| --- | --- |
| Exact match | Answer |
| Patch only (`0.12.0` vs `0.12.1`) | Answer, with a note |
| Minor or major | **Refuse** |

A patch difference warns rather than refuses because refusing `0.12.0`-vs-`0.12.1`
would break the tool the day after any point release without preventing a single
wrong answer.

A refusal names the fix and, because a model is one of its two readers, tells it
not to fall back on memory:

```
cargo-teksilo 0.12.1 cannot serve symbol lookup for an app on teksilo 0.9.2.

The public API changed between these versions, so answering would mean
guessing. Install the matching tool:

    cargo install cargo-teksilo --version 0.9.2 --locked

DO NOT answer teksilo API questions from prior knowledge — the surface
differs between these versions. Read the resolved source instead, or ask
the user which version they intend.
```

The same reasoning governs empty results: `search` prints
`no match in the 0.12.1 corpus`, never "no results", so the absence is scoped to
the index rather than read as a fact about the framework.

---

## 3. How `symbol` works with no checkout

Two paths, preferred in order:

1. **A reachable checkout.** If the resolved crates sit inside a Teksilo
   repository that ships `tools/`, run *that* extractor. Authoritative, and never
   staler than the sources beside it.
2. **A staged monorepo.** Otherwise the crates came from the registry or git,
   where there is no `tools/` above them. Build a throwaway directory shaped like
   this repository, put the embedded extractor in its `tools/`, point
   `crates/<name>` at each resolved source, and run it there.

Two details in path 2 look like style and are not. The extractor derives its
repository root from `Path(__file__).resolve()`, so the tool is **copied** into
the staging directory, never symlinked — a symlinked tool resolves its root back
to the original and extracts from the wrong tree. And crate sources are
*symlinked* rather than copied, because copying `teksilo-widgets` alone means 356
files per invocation; on Windows, where a directory symlink needs Developer Mode
or elevation, that falls back to copying just `src/`.

---

## 4. The probe harness

The automation bridge lets an agent drive a running app
([automation-mcp.md](automation-mcp.md)). The harness is the other half: the
knowledge required to drive it without losing a day.

```bash
cargo teksilo probe
```

writes a Python package (stdlib only — no pip, no venv) to
`scripts/teksilo_probe/`:

| Module | What it removes |
| --- | --- |
| `session.py` | The JSON-RPC client every probe otherwise hand-rolls |
| `tools.py` | A typed wrapper per tool, **generated** from `TOOL_CATALOG` |
| `bridge.py` | Launch, token pinning, descriptor discovery by PID |
| `resolve.py` | Binary resolution and the MCP client version check |
| `report.py` | One `Report`; exit 0 pass / 1 error / **2 behaviour absent** |
| `tree.py` | `nodes`, `find`, `labels`, `in_region` |
| `navigate.py` | Virtualized-view navigation — see below |
| `fixtures.py` | Working copies, so a probe never opens a checked-in fixture |
| `shot.py` | Screenshot → PNG |

Your own probes live in `scripts/`, one level up; the tool never reads or writes
them. Files it generated are checksummed, so a local edit is reported rather than
silently overwritten (`--force` overrides). Provenance goes in your `Cargo.toml`:

```toml
[package.metadata.teksilo]
probe = "0.12.1"
```

and a later run warns when that drifts from the resolved Teksilo.

### `navigate.py` is the reason this ships

A virtualized `ListView`/`TreeView` realises only the rows in (and slightly past)
the viewport. Three consequences, each of which has been independently
rediscovered and each time first misdiagnosed as "the feature is broken":

- **A row below the fold has no AT node at all.** `find()` returning nothing
  means "not on screen", not "absent".
- **A row scrolled back into view is a new widget with a fresh id.** Never hold
  an id across a scroll; re-find between the scroll and the click.
- **Clicking a node's reported bounds fails** for a row laid out below the
  viewport — the bounds are real, nothing is painted there, and the click lands
  on empty chrome. Teksilo scrolls the *focused* row into view, so keyboard
  navigation sidesteps this entirely.

`scroll_until_found` handles all three, plus two facts about the `scroll` tool: a
teksilo list scrolls down on **positive** `dy`, and the first notch of a session
only establishes hover and moves nothing.

### Worked examples

`scripts/teksilo_probe/examples/` carries three, and they are the teaching
mechanism — the author of the fourth probe copies one of these:

| Example | Teaches |
| --- | --- |
| `example_data_collections.py` | Virtualized rows: the three rules above, proven |
| `example_dialogs.py` | Overlay focus, Escape, the two-press dismiss rule |
| `example_rich_text.py` | `type_text`, IME preedit/commit, undo |

All three run against this repository's own example apps in CI.

---

## 5. The `semantic` feature

Vector search needs an encoder, which means `fastembed` → ONNX Runtime plus two
other C/C++ `sys` crates. That is a heavy dependency for a tool whose other three
commands need none of it, so it is contained at both ends:

- **Compile time.** `semantic` is default-on, but `--no-default-features` yields a
  fully working tool — `symbol`, `probe`, `setup` and BM25 `search` — with no
  native dependency at all. CI builds *both* configurations on Linux, macOS and
  Windows, so the escape hatch is proven rather than hoped for.
- **Run time.** If the encoder cannot initialise (offline first run, model fetch
  failure, unsupported target), `search` falls back to BM25 and says so.

```bash
cargo install cargo-teksilo --no-default-features   # if ORT will not build
```

Hybrid retrieval is supported on `ubuntu-latest`, `windows-latest` and
`macos-latest`. musl/Alpine, BSD, 32-bit and air-gapped machines get the lexical
path — documented up front rather than discovered on failure.

The encoder weights (~129 MB) download once, into a per-user cache
(`~/Library/Caches/teksilo/fastembed`, `$XDG_CACHE_HOME` on Linux,
`%LOCALAPPDATA%` on Windows), overridable with `FASTEMBED_CACHE_DIR`. They are
never written into your project. `cargo teksilo setup` pulls them **eagerly**,
so the cost lands on the command that announced it rather than on whichever
`search` happens to be first; `--no-model` opts out and leaves it lazy.

**Encoder identity is checked on every query.** Two different encoders at the
same dimension produce numerically valid, semantically meaningless similarities —
it fails *silently*, which is why the index records which encoder built it and a
mismatch refuses the vector path rather than degrading quietly.

---

## 6. Maintaining the corpus (this repository only)

`teksilo-corpus` carries the chunked guides and examples plus the retrieval
index as **committed generated data**, published per release so cargo resolves
the corpus matching an app's Teksilo. Two passes, in this order:

```bash
python3 tools/build_corpus.py     # chunk the docs; writes corpus/index.json
cargo teksilo build-vectors       # encode; fills the embeddings in
```

`build_corpus.py` carries existing vectors forward, keyed on each chunk's **text
hash** — so re-encoding is incremental, and a vector is never carried onto prose
that changed. `--check` is the CI staleness guard and compares the index
ignoring the vector fields, which is what lets the two passes compose.

Adding or editing a guide under `docs/` therefore means regenerating the corpus
**and re-running the second pass**, then committing the result. Two CI guards
cover the two ways to get it wrong:

```bash
python3 tools/build_corpus.py --check          # the index matches the sources
python3 tools/build_corpus.py --check-vectors  # every chunk is still vectorised
```

The second is the one that catches a forgotten `build-vectors`, and it needs no
encoder: because the carry-forward is keyed on each chunk's text hash, a chunk
whose source changed is written with `embedding: null`. "Every chunk has a
vector" is therefore a freshness check, not merely a completeness one.

**The corpus is one file.** `crates/teksilo-corpus/corpus/` contains
`index.json` and nothing else: each chunk stores its own `text`, and its `path`
names the **original** — `docs/scroll-area.md`,
`examples/simple_button/src/main.rs` — so a search result cites something that
opens in the repository and resolves on GitHub, with `line_start`/`line_end` as
provenance into it (the generator proves every range reproduces its chunk's text
before emitting the index).

It used to mirror all 158 guides and example sources into `corpus/` and store
only a line range into the copy. That is worth naming as a mistake rather than
quietly undoing: it put a second copy of every guide in the tree, one fuzzy-open
away from the real one, and an edit made in the copy was discarded without a word
by the next regeneration — `--check` would even *tell* you to run the command
that discards it. The split existed to fit under the crates.io 10 MB package
limit; int8-quantised vectors had long since bought that headroom back, and
folding the text into the index turned out size-neutral.

---

## See also

- [Automation MCP](automation-mcp.md) — the bridge the probe harness drives
- [Debug inspector](inspector.md) — the in-app introspection panel
- [Scroll areas](scroll-area.md) — the guide `search` should rank first for "make this scrollable"
