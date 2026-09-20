#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech
"""Build the teksilo-corpus retrieval corpus.

Reads the hand-written guides under ``docs/`` (top-level ``*.md`` plus
``docs/a11y/*.md``) and the runnable demos under ``examples/*/src/**/*.rs``,
chunks them, and writes a single BM25-ready ``index.json`` into
``crates/teksilo-corpus/corpus/``.

Deliberately excluded, for three different reasons:

* ``docs/widgets/``, ``docs/data-collections/``, ``docs/settings/`` and
  ``docs/scene/`` — generated Widget-Catalog pages. A separate
  symbol-lookup path already reads their source of truth, so indexing the
  generated pages too would duplicate that path and dilute BM25 scores.
* ``docs/SUMMARY.md`` — an mdBook table of contents, not prose.
* ``docs/docking-horizontal-rail.md`` — prose, and readable prose, but
  written for a *contributor* ("delete these tests", "rewrite this
  function") about work that has not been started. It is excluded for
  audience, not for form: see ``EXCLUDED_TOP_LEVEL_DOCS``.

**Nothing is copied.** ``corpus/`` holds ``index.json`` and nothing else.
An earlier version mirrored all 158 source files into ``corpus/`` and
stored only line ranges into the copies; that made every guide and every
example exist twice in the tree, which an editor's fuzzy-open is happy to
land you in — and an edit to the copy is silently discarded by the next
regeneration. The text now lives in the index, and each chunk's ``path``
names the **original** file, which is the path a search result should be
citing anyway: ``docs/scroll-area.md`` resolves in the repository and on
GitHub; ``guides/scroll-area.md`` resolved nowhere.

index.json schema (schema version 2)::

    {
      "schema": 2,
      "teksilo_version": "0.12.1",
      "encoder": null,          // name of the dense encoder; see "Vectors" below
      "encoder_dim": null,      // dimensionality of that encoding
      "chunk_count": 1234,
      "avgdl": 142.7,           // mean chunk length in tokens (BM25's avgdl)
      "terms": ["accesskit", "button", "..."],  // the interning table, sorted
      "df": {"0": 12, "1": 340},   // corpus-wide document frequency, by term index
      "chunks": [
        {
          "id": 0,
          "kind": "guide",              // "guide" | "example" | "footer"
          "path": "docs/layout-primitives.md",  // repo-relative, the ORIGINAL
          "heading_path": ["Layout Model", "Grow (positive slack)"],
          "text": "## Grow (positive slack)\\n...",   // this chunk, verbatim
          "line_start": 41,              // inclusive, 0-based, into <path>
          "line_end": 68,                // inclusive, 0-based, into <path>
          "crate": null,                 // example crate name, else null
          "tokens": {"0": 2, "17": 5},   // term freqs, by term index: this
                                         // chunk's text PLUS its injected
                                         // context (see "Chunking rules")
          "len": 143,                    // sum of this chunk's term freqs
          "embedding": null              // {"scale": f, "q": base64}; see "Vectors"
        }
      ]
    }

The key order above is **load-bearing**: ``cargo teksilo build-vectors``
re-serializes this file through serde to write the embeddings in, and
refuses to run (``NotRoundTrippable``) unless doing so reproduces these
bytes exactly. Reorder a field here and the Rust ``Chunk`` struct in
``crates/teksilo-corpus/src/lib.rs`` has to move with it.

The file is written **compact** (no indent, ``separators=(",", ":")``,
``ensure_ascii=False``): it is a machine-read artifact, and this crate
ships to crates.io under a 10 MB package limit.

``line_start`` / ``line_end`` are **provenance**, not the storage: they say
where in the original file this text was found, so a search result can
point at it. They are inclusive and 0-based, and the invariant is::

    "\\n".join(source_file_lines[line_start : line_end + 1]) == chunk["text"]

where ``source_file_lines`` splits on ``\\n`` only and drops a trailing
empty element — Rust's ``str::lines()`` semantics, so a Rust consumer
reading the same source file gets the same slice. Every range is verified
against its source file before ``index.json`` is emitted; a mismatch
aborts the build, because a range that does not name its own text would
send a reader to the wrong lines with no way to notice.

Term strings are **interned**. ``terms`` is the sorted list of every
token in the corpus; each chunk's ``tokens`` map and the corpus-wide
``df`` map are keyed by the *decimal string* of a term's index into it
(JSON object keys must be strings). ``terms`` stays sorted so the output
is deterministic and a consumer can binary-search a query term to its
index.

Vectors
-------

``encoder``, ``encoder_dim`` and every chunk's ``embedding`` are written by
a **second pass**, ``cargo teksilo build-vectors``, and are ``null`` in
this script's output. That pass encodes each chunk, quantises the result to
int8 with a per-vector scale, and writes it back into the same
``index.json``. So the release order is::

    python3 tools/build_corpus.py      # chunk the docs, vectors become null
    cargo teksilo build-vectors        # encode, fill the vectors back in

**Never the other way round**: this script regenerates ``index.json`` from
scratch, so running it after ``build-vectors`` discards the vectors and
leaves the shipped tool on lexical-only search. That degrades rather than
breaks, which is exactly why it needs saying — nothing fails loudly.

Consequently ``--check`` compares ``index.json`` *semantically with the
three vector fields removed* (``diff_index``) rather than byte for byte:
its question is whether the chunking is current, and the vectors are not
this script's to know about. ``index.json`` is the only file in the
corpus, so that comparison is the whole check.

Chunking rules
--------------

Markdown guides are split at ATX headings (``#`` through ``######``). Each
heading starts a new chunk that runs until the next heading of *any*
level (or EOF); ``heading_path`` is the list of open ancestor headings
plus the chunk's own heading, so a chunk under nested sections carries its
full context. A fenced code block (``` ```` ``` ```) is never split across
chunks — headings inside a fence are not treated as headings. Any prose
before the first heading becomes its own chunk with an empty
``heading_path``.

Rust example files are chunked by top-level item (``use``, ``fn``,
``struct``, ``enum``, ``trait``, ``impl``, ``mod``, ``const``, ``static``,
``type``, ``macro_rules!``) where the file's brace nesting can be tracked
reliably by a simple counter; adjacent leading ``use`` statements merge
into one chunk, and each item's directly preceding comment/attribute
lines travel with it. When a file's global brace count does not balance
(a sign that string or comment content would throw off the simple
counter), the file falls back to fixed ~80-line windows with a 10-line
overlap, so nothing is ever silently mis-chunked.

Tokenization for BM25 (both for a chunk's own ``tokens`` map and the
corpus-wide ``df`` map): lowercase, split on runs of characters that are
not ``[a-z0-9_]`` (so a Rust identifier like ``list_model`` stays one
token), drop tokens of length 1, no stemming.

On top of the chunk's own text, two things are **injected** into its
``tokens`` at weight 1 — its **ancestor headings** and its source file's
**stem**:

* A chunk seeded by ``## Grow (positive slack)`` holds that heading and
  nothing above it, so the enclosing ``# Layout Model`` is a word the
  section does not contain. Every subsection of ``docs/settings.md`` had
  ``settings`` at tf 0; ``how do I persist window size`` therefore could
  not reach the sections that answer it, and the whole-document context a
  human reader gets from the page they are scrolled into was simply
  absent from the ranking.
* The stem does the same job for the filename, which is often the
  clearest single word a guide has (``scroll-area``, ``drag-and-drop``).

Only the *ancestors* are injected, never ``heading_path[-1]``: a chunk's
own heading is already the first line of its ``text``, and injecting it
again would double its weight for no reason anyone chose. The injection
changes ``tokens`` and ``len`` and deliberately does **not** change
``text`` — ``build-vectors`` carries embeddings forward by text hash, so
this is a lexical-only change and nothing needs re-encoding.

CLI
---

``python3 tools/build_corpus.py``            regenerate in place, print a summary
``python3 tools/build_corpus.py --check``    regenerate into a temp dir and diff
                                              its ``index.json`` against the
                                              committed one; exits 1 with a diff
                                              summary on drift. Ignores --out: it
                                              always checks the committed corpus/
``python3 tools/build_corpus.py --out DIR``  write ``index.json`` into DIR instead
                                              of the committed corpus/, leaving the
                                              real one untouched. Relocates the
                                              write; it is not itself a check
``python3 tools/build_corpus.py --quiet``    suppress the summary print
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import Counter
from pathlib import Path
from typing import Dict, List, Optional, Tuple

REPO_ROOT = Path(__file__).resolve().parent.parent
DOCS_DIR = REPO_ROOT / "docs"
EXAMPLES_DIR = REPO_ROOT / "examples"
CORPUS_CRATE_DIR = REPO_ROOT / "crates" / "teksilo-corpus"
DEFAULT_OUT_DIR = CORPUS_CRATE_DIR / "corpus"

# Subdirectories of docs/ that are generated Widget-Catalog pages, not
# hand-written prose. See the module docstring for why these are excluded.
EXCLUDED_DOC_SUBDIRS = {"widgets", "data-collections", "settings", "scene"}

# Two entries, excluded for two unrelated reasons.
#
# `SUMMARY.md` is not prose — an mdBook table of contents, all links.
#
# `docs/docking-horizontal-rail.md` IS prose. It is excluded because it is
# addressed to a different reader: it is an unstarted contributor backlog
# ("delete these tests", "rewrite this function") for a docking feature that
# `docs/docking.md` already documents as shipped. Measured against this
# corpus, its 6 chunks took slots #1 AND #2 on `rail orientation` — pushing
# `docs/docking.md › 5. Tabs or an activity rail`, the consumer guide's own
# section on exactly that, to #3 — and it ranked #1 on four of five plausible
# rail queries. The one consumer-actionable fact it carries is already in
# `docs/docking.md:262-272`, said better. An agent that lands on it reads
# planned work as though it were API.
#
# It keeps its book chapter and its link from `docs/docking.md`; it just
# stops competing for a consumer agent's top slot.
#
# Why this is a skip-list entry and NOT a `kind: "backlog"` value, which is
# the obvious alternative and the one the next reader will propose: the two
# existing `kind` values are *derived*. "footer" falls out of a regex on the
# heading conjoined with a link-density threshold on the body — mechanical,
# test-enforced, and computed afresh from the file on every build. "Is this
# document unstarted work?" is editorial judgement; no regex derives it, so a
# `kind: "backlog"` would have to be hand-declared in a marker at the top of
# each file. That rots the wrong way. A file whose marker was never added
# ships as an ordinary guide, wearing the tool's blessing, and nothing ever
# notices — the failure is silent and points at the agent. A skip-list rots
# the safe way: forget to add an entry and a doc is merely indexed, which is
# the status quo; the list is short, lives beside its reasons, and a reader
# reviewing one file sees every judgement call at once.
EXCLUDED_TOP_LEVEL_DOCS = {"SUMMARY.md", "docking-horizontal-rail.md"}

# A guide's closing navigation footer — "See also", "Reference", "Code
# references" — is a list of links, not prose, and it is not retrievable
# content: every target it names is either another corpus path (reachable on
# its own merits) or a source file `cargo teksilo symbol` already answers for.
#
# Retrievable, it actively outranks the real answer. BM25 normalises by chunk
# length, so a short footer whose link *paths* happen to carry the query's
# words beats the long section that explains them. Measured against this
# corpus, "how do I persist window size" ranked `docs/settings.md`'s 80-token
# `## Reference` footer FIRST — a chunk that is six markdown links and no
# sentences — while the section actually documenting `WindowStateService` came
# sixth.
#
# So the chunk is TAGGED, not dropped. It stays in the index under
# `kind == "footer"`, because `cargo teksilo show` reassembles a document from
# its chunks and nothing else — dropping the tail chunk silently truncated
# every guide that had one (28 of them at the time), which `show`'s own
# `every_document_reconstructs_byte_exactly` caught. Retrieval filters the kind
# out instead.
#
# The test is deliberately a CONJUNCTION of heading and body. Matching the
# heading alone would drop a real "Reference" section that happens to carry
# prose; matching link density alone drops prose sections that merely cite a
# lot (a measured 32 chunks, including a paragraph of accessibility narrative
# with one link per line). Together they select 30 chunks, all of them
# genuine footers.
FOOTER_HEADING_RE = re.compile(
    r"^(\d+(\.\d+)*\.?\s+)?"
    r"(see also|references?|related( references)?|code references?"
    r"|files? references?|files? to know|where the code lives"
    r"|further reading)$",
    re.I,
)
MARKDOWN_LINK_RE = re.compile(r"\[[^\]]+\]\([^)]+\)")
FOOTER_LINK_DENSITY = 0.8


def is_navigation_footer(heading_path: List[str], text: str) -> bool:
    """True for a closing link-list footer, which is navigation rather than
    content. See `FOOTER_HEADING_RE` for why both halves are required."""
    if not heading_path or not FOOTER_HEADING_RE.match(heading_path[-1].strip()):
        return False
    # Fold continuation lines into the bullet they belong to before measuring.
    # A footer bullet that happens to wrap would otherwise contribute a second,
    # link-free line and dilute the density below the threshold — making the
    # rule fire or not on where a line break landed, which is not a property
    # anyone editing a guide should have to think about.
    entries: List[str] = []
    for ln in text.splitlines()[1:]:
        if not ln.strip():
            continue
        if entries and (ln.startswith((" ", "\t")) and not ln.lstrip().startswith(("-", "*", "+"))):
            entries[-1] += " " + ln.strip()
        else:
            entries.append(ln)
    if not entries:
        return False
    linky = sum(1 for e in entries if MARKDOWN_LINK_RE.search(e))
    return linky / len(entries) >= FOOTER_LINK_DENSITY

TOKEN_RE = re.compile(r"[a-z0-9_]+")
ATX_HEADING_RE = re.compile(r"^(#{1,6})\s+(.*)$")
FENCE_RE = re.compile(r"^\s*(`{3,}|~{3,})")

RUST_ITEM_RE = re.compile(
    r"^(pub(\([\w:]+\))?\s+)?"
    r"(default\s+)?(async\s+)?(unsafe\s+)?"
    r'(extern\s+"[^"]*"\s+)?'
    r"(fn|struct|enum|trait|union|impl|mod|const|static|type|macro_rules!)\b"
)
RUST_USE_RE = re.compile(r"^(pub(\([\w:]+\))?\s+)?use\b")

WINDOW_SIZE = 80
WINDOW_OVERLAP = 10
WINDOW_STRIDE = WINDOW_SIZE - WINDOW_OVERLAP


class CorpusVerificationError(RuntimeError):
    """A chunk's line range does not name the text the chunk stores. Raised
    by :func:`verify_line_ranges`; aborts the build."""


# ---------------------------------------------------------------------------
# Line handling
# ---------------------------------------------------------------------------
#
# Chunk line ranges are the index format's one cross-language contract — a
# Rust consumer resolving a chunk back to its source file has to land on the
# same lines — so the split must mean the same thing in Python and in Rust.
# `str.splitlines`
# does NOT: it also breaks on a lone `\r`, `\x0b`, `\x0c`, `\x1c`-`\x1e`,
# `\x85`, `U+2028` and `U+2029`, none of which Rust's `str::lines()` treats
# as a line break. And `Path.read_text` applies universal-newline
# translation, so a lone `\r` in a source file would become a break in
# Python's view of the text while the bytes on disk — which is what a Rust
# consumer reads — still hold a `\r`. Both are avoided by reading bytes and
# splitting on `\n` alone.


def split_lines(text: str) -> List[str]:
    """Split ``text`` exactly as Rust's ``str::lines()`` does.

    Breaks on ``\\n`` only, drops the empty final element a trailing
    newline produces, and strips a trailing ``\\r`` from each line (so a
    CRLF file yields the same lines here and in Rust).
    """
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    return [line[:-1] if line.endswith("\r") else line for line in lines]


def read_source(path: Path) -> str:
    """Read a source file as the exact bytes on disk, decoded.

    Deliberately not ``read_text``: see the note above on universal-newline
    translation.
    """
    return path.read_bytes().decode("utf-8")


# ---------------------------------------------------------------------------
# Discovery
# ---------------------------------------------------------------------------


def read_workspace_version(repo_root: Path) -> str:
    """Read `version` from the root Cargo.toml's [workspace.package] table.

    Stdlib-only (no tomllib on Python < 3.11, and this must run on the
    Python already installed for contributors), so this is a small,
    deliberately narrow regex scan rather than a general TOML parser.
    """
    text = (repo_root / "Cargo.toml").read_text(encoding="utf-8")
    section_start = text.find("[workspace.package]")
    if section_start == -1:
        raise RuntimeError("Cargo.toml has no [workspace.package] section")
    next_section = text.find("\n[", section_start + 1)
    section_text = text[section_start:] if next_section == -1 else text[section_start:next_section]
    m = re.search(r'^\s*version\s*=\s*"([^"]+)"', section_text, re.MULTILINE)
    if not m:
        raise RuntimeError("could not find version = \"...\" in [workspace.package]")
    return m.group(1)


def discover_guides(docs_dir: Path) -> List[Path]:
    paths = [
        p
        for p in sorted(docs_dir.glob("*.md"))
        if p.name not in EXCLUDED_TOP_LEVEL_DOCS
    ]
    return paths


def discover_a11y_guides(docs_dir: Path) -> List[Path]:
    return sorted((docs_dir / "a11y").glob("*.md"))


def discover_examples(examples_dir: Path) -> List[Tuple[str, Path, Path]]:
    """Return (crate_name, path_relative_to_src, absolute_path) triples."""
    out: List[Tuple[str, Path, Path]] = []
    for crate_dir in sorted(p for p in examples_dir.iterdir() if p.is_dir()):
        src_dir = crate_dir / "src"
        if not src_dir.is_dir():
            continue
        for rs_path in sorted(src_dir.rglob("*.rs")):
            rel = rs_path.relative_to(src_dir)
            out.append((crate_dir.name, rel, rs_path))
    return out


# ---------------------------------------------------------------------------
# Tokenization
# ---------------------------------------------------------------------------


def tokenize(text: str) -> List[str]:
    return [t for t in TOKEN_RE.findall(text.lower()) if len(t) > 1]


# ---------------------------------------------------------------------------
# Markdown chunking
# ---------------------------------------------------------------------------


def chunk_markdown(text: str) -> List[Dict]:
    """Split markdown into heading-scoped chunks. See module docstring."""
    lines = split_lines(text)
    chunks: List[Dict] = []

    # Stack of (level, title) for headings currently "open".
    stack: List[Tuple[int, str]] = []
    current_heading_path: List[str] = []
    current_lines: List[str] = []
    # Source line index of each entry in `current_lines`, one-for-one.
    current_indices: List[int] = []
    have_current = False

    in_fence = False
    fence_char = ""

    def flush() -> None:
        nonlocal current_lines, current_indices, have_current
        if not have_current:
            return
        # The historical text was `"\n".join(current_lines).strip("\n")`.
        # On a join of lines (which by construction contain no "\n"), that
        # strip is exactly "drop leading and trailing *empty* lines" — note
        # empty, not whitespace-only: a "  " line survives `.strip("\n")`.
        # Trim the index list by the same rule so the range still names the
        # text that gets tokenized.
        lo, hi = 0, len(current_lines)
        while lo < hi and current_lines[lo] == "":
            lo += 1
        while hi > lo and current_lines[hi - 1] == "":
            hi -= 1
        text_block = "\n".join(current_lines[lo:hi])
        if text_block.strip():
            chunks.append(
                {
                    "heading_path": list(current_heading_path),
                    "text": text_block,
                    "line_start": current_indices[lo],
                    "line_end": current_indices[hi - 1],
                    "footer": is_navigation_footer(current_heading_path, text_block),
                }
            )
        current_lines = []
        current_indices = []
        have_current = False

    for i, line in enumerate(lines):
        fence_m = FENCE_RE.match(line)
        if fence_m:
            marker_char = fence_m.group(1)[0]
            if not in_fence:
                in_fence = True
                fence_char = marker_char
            elif marker_char == fence_char:
                in_fence = False
            current_lines.append(line)
            current_indices.append(i)
            have_current = True
            continue

        if not in_fence:
            m = ATX_HEADING_RE.match(line)
            if m:
                level = len(m.group(1))
                title = re.sub(r"\s+#+\s*$", "", m.group(2)).strip()
                flush()
                while stack and stack[-1][0] >= level:
                    stack.pop()
                current_heading_path = [t for _, t in stack] + [title]
                stack.append((level, title))
                current_lines = [line]
                current_indices = [i]
                have_current = True
                continue

        current_lines.append(line)
        current_indices.append(i)
        have_current = True

    flush()
    return chunks


# ---------------------------------------------------------------------------
# Rust chunking
# ---------------------------------------------------------------------------


def _is_blank(line: str) -> bool:
    return line.strip() == ""


def _is_comment(line: str) -> bool:
    return line.strip().startswith("//")


def _is_attr(line: str) -> bool:
    s = line.strip()
    return s.startswith("#[") or s.startswith("#![")


def _classify_item_line(stripped: str) -> Optional[str]:
    if RUST_USE_RE.match(stripped):
        return "use"
    if RUST_ITEM_RE.match(stripped):
        return "item"
    return "other"


def _rust_items_by_brace_tracking(lines: List[str]) -> Optional[List[Tuple[int, int, int, str]]]:
    """Return a list of (chunk_start, item_line, end, kind) spans, or None
    if this file isn't a good candidate for item-based chunking (its
    global brace count doesn't balance)."""
    n = len(lines)
    full_balance = sum(l.count("{") - l.count("}") for l in lines)
    if full_balance != 0:
        return None

    items: List[Tuple[int, int, int, str]] = []
    i = 0
    buf_start: Optional[int] = None

    while i < n:
        line = lines[i]
        if _is_blank(line) or _is_comment(line) or _is_attr(line):
            if buf_start is None:
                buf_start = i
            i += 1
            continue

        stripped = line.strip()
        kind = _classify_item_line(stripped)
        item_line = i
        start = buf_start if buf_start is not None else i
        buf_start = None

        depth = line.count("{") - line.count("}")
        j = i
        while not (depth <= 0 and (lines[j].rstrip().endswith(";") or lines[j].rstrip().endswith("}"))):
            if j + 1 >= n:
                break
            j += 1
            depth += lines[j].count("{") - lines[j].count("}")
        end = j
        items.append((start, item_line, end, kind))
        i = end + 1

    if buf_start is not None:
        # Trailing comment/attribute block with nothing following it.
        items.append((buf_start, buf_start, n - 1, "trailing"))

    if not items:
        return None
    return items


def _merge_adjacent_use_items(
    items: List[Tuple[int, int, int, str]]
) -> List[Tuple[int, int, int, str]]:
    merged: List[Tuple[int, int, int, str]] = []
    for start, item_line, end, kind in items:
        if (
            merged
            and kind == "use"
            and merged[-1][3] == "use"
            and start == item_line
            and start == merged[-1][2] + 1
        ):
            p_start, p_item_line, _p_end, _p_kind = merged[-1]
            merged[-1] = (p_start, p_item_line, end, "use")
        else:
            merged.append((start, item_line, end, kind))
    return merged


def _rust_label(lines: List[str], item_line: int, kind: str) -> List[str]:
    if kind == "trailing":
        return ["trailing comment"]
    raw = lines[item_line].strip()
    raw = re.sub(r"\s*\{\s*$", "", raw)
    raw = raw.rstrip(";").strip()
    if len(raw) > 100:
        raw = raw[:99].rstrip() + "…"
    return [raw] if raw else [kind]


def _window_chunks(lines: List[str]) -> List[Dict]:
    n = len(lines)
    if n == 0:
        return []
    chunks: List[Dict] = []
    start = 0
    while True:
        end = min(start + WINDOW_SIZE, n)
        block = "\n".join(lines[start:end])
        if block.strip():
            chunks.append(
                {
                    "heading_path": [f"lines {start + 1}-{end}"],
                    "text": block,
                    "line_start": start,
                    "line_end": end - 1,
                }
            )
        if end >= n:
            break
        start += WINDOW_STRIDE
    return chunks


def chunk_rust(text: str) -> List[Dict]:
    lines = split_lines(text)
    items = _rust_items_by_brace_tracking(lines)
    if items is None:
        return _window_chunks(lines)

    items = _merge_adjacent_use_items(items)
    chunks: List[Dict] = []
    for start, item_line, end, kind in items:
        block = "\n".join(lines[start : end + 1])
        if not block.strip():
            continue
        chunks.append(
            {
                "heading_path": _rust_label(lines, item_line, kind),
                "text": block,
                "line_start": start,
                "line_end": end,
            }
        )
    if not chunks:
        return _window_chunks(lines)
    return chunks


# ---------------------------------------------------------------------------
# Corpus assembly
# ---------------------------------------------------------------------------


def verify_line_ranges(repo_root: Path, chunks: List[Dict]) -> int:
    """Re-read every source file and prove each chunk's line range names the
    text that chunk stores.

    Re-read, rather than re-used from the in-memory lines the chunkers worked
    on: the range's whole job is to let someone else open ``path`` and find
    this text, so it is checked the way that someone else would — by opening
    the file. Returns the number of chunks verified; raises
    :class:`CorpusVerificationError` on the first mismatch.
    """
    file_lines: Dict[str, List[str]] = {}
    for chunk in chunks:
        rel = chunk["path"]
        if rel not in file_lines:
            file_lines[rel] = split_lines(read_source(repo_root / rel))
        lines = file_lines[rel]
        start, end = chunk["line_start"], chunk["line_end"]
        if not (0 <= start <= end < len(lines)):
            raise CorpusVerificationError(
                f"chunk {chunk['id']} ({rel}): line range {start}..={end} is out of "
                f"bounds for a file of {len(lines)} lines"
            )
        rebuilt = "\n".join(lines[start : end + 1])
        expected = chunk["text"]
        if rebuilt != expected:
            raise CorpusVerificationError(
                f"chunk {chunk['id']} ({rel}): lines {start}..={end} do not reproduce "
                f"the stored text\n"
                f"  stored  ({len(expected)} chars): {expected[:200]!r}\n"
                f"  rebuilt ({len(rebuilt)} chars): {rebuilt[:200]!r}"
            )
    return len(chunks)


def _text_key(text: str) -> str:
    """Content identity for a chunk, used to carry its vector across a rebuild."""
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _read_prior_vectors(index_path: Path) -> Tuple[Dict[str, Dict], Optional[str], Optional[int]]:
    """Embeddings already committed, keyed by chunk text.

    Returns ``({text_key: embedding}, encoder, encoder_dim)``. Empty when there
    is no index yet, when it carries no vectors, or when it cannot be read —
    a corpus that has never been vectorised is the normal first state, not an
    error.
    """
    try:
        prior = json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}, None, None

    encoder = prior.get("encoder")
    dim = prior.get("encoder_dim")
    if not encoder:
        return {}, None, None

    out: Dict[str, Dict] = {}
    for chunk in prior.get("chunks", []):
        emb = chunk.get("embedding")
        text = chunk.get("text")
        if emb is None or text is None:
            continue
        out[_text_key(text)] = emb
    return out, encoder, dim


def build_corpus(out_dir: Path) -> Tuple[Dict, int]:
    """Write ``index.json`` into out_dir. Nothing else is written there.

    Returns the index dict and the number of chunks whose line range was
    verified against its source file.
    """
    index_path = out_dir / "index.json"

    # BEFORE the write below: this function regenerates `index.json` from
    # scratch, and the committed vectors live in it. Reading them afterwards
    # finds the file this function just replaced and silently discards every
    # embedding.
    prior_vectors, prior_encoder, prior_dim = _read_prior_vectors(index_path)

    out_dir.mkdir(parents=True, exist_ok=True)

    chunks: List[Dict] = []

    # Vectors are owned by `cargo teksilo build-vectors`, not by this script,
    # but this script rewrites the file they live in. Regenerating must
    # therefore CARRY THEM FORWARD, or every docs edit silently discards 2540
    # embeddings and the next `--check` reports drift that has nothing to do
    # with what changed.
    #
    # The identity is the chunk's TEXT, not its id or line range: a vector is
    # valid for the text it was computed from, wherever that text later sits in
    # the file. Keying on id would carry a stale vector onto edited prose, which
    # is the one failure mode worse than having no vector at all — cosine
    # similarity against the wrong text is numerically valid and silently wrong.
    def add_chunks(raw_chunks: List[Dict], *, kind: str, path: str, crate: Optional[str]) -> None:
        # Context tokens: the file's stem, plus each chunk's ANCESTOR headings.
        # Both go through the same `tokenize` as the body, so a heading's
        # backticks, punctuation and hyphens are stripped identically and
        # `docking-horizontal-rail` arrives as three ordinary terms. Weight 1
        # (each heading contributes its words once): weight 2 was measured and
        # gained nothing over 1, and doubling a term a section never wrote is
        # a thumb on the scale nobody can later account for.
        stem_tokens = tokenize(Path(path).stem)
        for raw in raw_chunks:
            toks = tokenize(raw["text"])
            # `heading_path[-1]` is this chunk's OWN heading and is already the
            # first line of `text`; injecting it would silently double it.
            for ancestor in raw["heading_path"][:-1]:
                toks.extend(tokenize(ancestor))
            toks.extend(stem_tokens)
            # `len` is derived from `tf` below, so it follows the injection
            # automatically and BM25's length normalisation stays honest.
            tf = Counter(toks)
            # Key order here is the JSON key order, and `cargo teksilo
            # build-vectors` refuses to run unless serde reproduces it exactly
            # — see the module docstring.
            chunks.append(
                {
                    "id": len(chunks),
                    # A navigation footer stays IN the corpus — `cargo teksilo
                    # show` reassembles a document from its chunks, so dropping
                    # one truncates the file it came from — but carries its own
                    # kind so retrieval skips it. See `is_navigation_footer`.
                    "kind": "footer" if raw.get("footer") else kind,
                    "path": path,
                    "heading_path": raw["heading_path"],
                    "text": raw["text"],
                    "line_start": raw["line_start"],
                    "line_end": raw["line_end"],
                    "crate": crate,
                    # Term-keyed for now; interned to term indices below,
                    # once the corpus-wide term table is known.
                    "tokens": dict(sorted(tf.items())),
                    "len": sum(tf.values()),
                    "embedding": prior_vectors.get(_text_key(raw["text"])),
                }
            )

    # `path` is the ORIGINAL file's repo-relative path throughout: it is what a
    # search result cites, so it has to be a path that exists.

    # --- guides ---
    for src in discover_guides(DOCS_DIR):
        add_chunks(
            chunk_markdown(read_source(src)),
            kind="guide",
            path=src.relative_to(REPO_ROOT).as_posix(),
            crate=None,
        )

    # --- a11y guides ---
    for src in discover_a11y_guides(DOCS_DIR):
        add_chunks(
            chunk_markdown(read_source(src)),
            kind="guide",
            path=src.relative_to(REPO_ROOT).as_posix(),
            crate=None,
        )

    # --- examples ---
    for crate_name, _rel_path, abs_path in discover_examples(EXAMPLES_DIR):
        add_chunks(
            chunk_rust(read_source(abs_path)),
            kind="example",
            path=abs_path.relative_to(REPO_ROOT).as_posix(),
            crate=crate_name,
        )

    # --- verify every chunk's line range against its source file ---
    # Done before the index exists: a range that does not name its own text
    # points a reader at the wrong lines with nothing to notice, so nothing
    # gets emitted on failure.
    verified = verify_line_ranges(REPO_ROOT, chunks)

    # --- corpus-wide BM25 statistics ---
    df: Counter = Counter()
    for c in chunks:
        df.update(c["tokens"].keys())

    total_len = sum(c["len"] for c in chunks)
    avgdl = (total_len / len(chunks)) if chunks else 0.0

    # --- intern the term strings ---
    # `df` holds exactly the union of every chunk's token keys, so it is the
    # term table. Sorted, so the index a term interns to is deterministic and
    # a consumer can binary-search a query term straight to it.
    terms = sorted(df)
    term_index = {term: i for i, term in enumerate(terms)}
    for c in chunks:
        # Term-sorted in, index-sorted out: `terms` is sorted, so the term
        # order and the index order are the same order.
        c["tokens"] = {str(term_index[term]): n for term, n in c["tokens"].items()}

    index = {
        "schema": 2,
        "teksilo_version": read_workspace_version(REPO_ROOT),
        # Claimed only when at least one vector survived: an index naming an
        # encoder it has no vectors for would send a consumer down the semantic
        # path with nothing to compare against.
        "encoder": prior_encoder if any(c["embedding"] for c in chunks) else None,
        "encoder_dim": prior_dim if any(c["embedding"] for c in chunks) else None,
        "chunk_count": len(chunks),
        "avgdl": round(avgdl, 4),
        "terms": terms,
        "df": {str(term_index[term]): n for term, n in sorted(df.items())},
        "chunks": chunks,
    }

    # Compact: this is a machine-read artifact, and the crate ships under
    # crates.io's 10 MB package limit.
    with index_path.open("w", encoding="utf-8") as f:
        json.dump(index, f, separators=(",", ":"), ensure_ascii=False, sort_keys=False)
        f.write("\n")

    return index, verified


# ---------------------------------------------------------------------------
# Diffing (for --check)
# ---------------------------------------------------------------------------


VECTOR_FIELDS = ("encoder", "encoder_dim")


def _without_vectors(index_path: Path) -> Dict:
    """An index with the three vector fields removed, for comparison.

    ``--check`` asks one question: *is the chunking current?* The vectors are
    written by a second pass (``cargo teksilo build-vectors``) that this
    script knows nothing about and always regenerates away, so comparing them
    would report drift on every committed corpus that has them — which is
    every released one. Removing exactly those three fields keeps the check
    byte-exact about everything this script is actually responsible for.
    """
    index = json.loads(index_path.read_text(encoding="utf-8"))
    for field in VECTOR_FIELDS:
        index.pop(field, None)
    for chunk in index.get("chunks", []):
        chunk.pop("embedding", None)
    return index


def diff_index(committed: Path, fresh: Path) -> List[str]:
    """Compare two ``index.json`` files, ignoring the vector fields.

    ``index.json`` is the only file the corpus has, so this *is* ``--check``.
    """
    if not committed.is_file():
        return ["  - missing from the committed corpus: index.json"]
    try:
        left = _without_vectors(committed)
        right = _without_vectors(fresh)
    except (OSError, ValueError) as exc:
        return [f"  - content differs: index.json (could not compare: {exc})"]
    if left == right:
        return []
    problems = [
        "  - content differs: index.json (ignoring encoder/encoder_dim/embedding)"
    ]
    for key in sorted(set(left) | set(right)):
        if left.get(key) != right.get(key):
            if key == "chunks":
                n_left, n_right = len(left.get(key, [])), len(right.get(key, []))
                changed = sum(
                    1
                    for a, b in zip(left.get(key, []), right.get(key, []))
                    if a != b
                )
                problems.append(
                    f"      chunks: {n_left} committed vs {n_right} generated, "
                    f"{changed} differing in the overlap"
                )
            else:
                problems.append(f"      {key} differs")
    return problems


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def format_summary(index: Dict, out_dir: Path, verified: int) -> str:
    def is_a11y(chunk: Dict) -> bool:
        return chunk["path"].startswith("docs/a11y/")

    guide_chunks = [c for c in index["chunks"] if c["kind"] == "guide"]
    example_chunks = [c for c in index["chunks"] if c["kind"] == "example"]

    n_guides = sum(1 for c in guide_chunks if not is_a11y(c))
    n_a11y_chunks = sum(1 for c in guide_chunks if is_a11y(c))

    guide_files = len({c["path"] for c in guide_chunks if not is_a11y(c)})
    a11y_files = len({c["path"] for c in guide_chunks if is_a11y(c)})
    example_files = len({c["path"] for c in example_chunks})
    example_crates = len({c["crate"] for c in example_chunks})

    index_bytes = (out_dir / "index.json").stat().st_size
    other_files = sorted(
        p.relative_to(out_dir).as_posix()
        for p in out_dir.rglob("*")
        if p.is_file() and p.name != "index.json"
    )

    lines = [
        "teksilo-corpus build summary",
        f"  guides:       {guide_files} files, {n_guides} chunks",
        f"  a11y guides:  {a11y_files} files, {n_a11y_chunks} chunks",
        f"  examples:     {example_crates} crates, {example_files} files, "
        f"{len(example_chunks)} chunks",
        f"  chunk_count:  {index['chunk_count']}",
        f"  avgdl:        {index['avgdl']}",
        f"  df terms:     {len(index['df'])}",
        f"  line ranges:  {verified} chunks verified against their source files",
        f"  index.json:   {index_bytes:,} bytes",
    ]
    # The corpus is one file. Say so when it is, and name the strays when it
    # is not — a left-over tree from an older layout would ship inside the
    # crate and count against the crates.io package limit in silence.
    if other_files:
        lines.append(
            f"  WARNING:      {len(other_files)} unexpected file(s) beside index.json: "
            + ", ".join(other_files[:5])
            + (" …" if len(other_files) > 5 else "")
        )
    return "\n".join(lines)


def check_vectors(index_path: Path, *, quiet: bool = False) -> int:
    """Assert the committed index is fully vectorised, without an encoder.

    This is the cheap half of the two-pass build's safety net, and it is
    complete rather than approximate — which is worth explaining, because the
    obvious CI step (re-run `build-vectors` and diff) costs a 128 MB model
    download and an ONNX Runtime build on every run.

    `build_corpus.py` carries embeddings forward keyed on each chunk's **text
    hash**, so a chunk whose text changed finds no prior vector and is written
    with ``embedding: null``. Therefore "every chunk has a vector" is not merely
    a completeness check — it is a *freshness* check: a stale vector cannot
    survive a regeneration, and a missing one is exactly what a forgotten
    second pass leaves behind. Verified by experiment: editing one guide and
    regenerating nulls exactly that guide's changed chunk and nothing else.

    What it does not cover is a corpus re-encoded by a *different* model. That
    is caught at query time instead, where the index's recorded encoder is
    compared against the one the binary was built with — two encoders at one
    dimension produce arithmetically valid, semantically meaningless
    similarities, so it has to be refused rather than detected here.
    """
    try:
        index = json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        print(f"teksilo-corpus: cannot read {index_path}: {e}", file=sys.stderr)
        return 1

    chunks = index.get("chunks", [])
    missing = [c for c in chunks if not c.get("embedding")]
    encoder = index.get("encoder")
    dim = index.get("encoder_dim")

    if not encoder or not dim:
        print(
            "teksilo-corpus has NO vectors: the index names no encoder.\n"
            "The corpus is built in two passes; the second was not run:\n"
            "    cargo run -p cargo-teksilo --features semantic -- teksilo build-vectors\n"
            "Without it `cargo teksilo search` silently falls back to lexical.",
            file=sys.stderr,
        )
        return 1

    if missing:
        shown = sorted({c["path"] for c in missing})[:10]
        print(
            f"teksilo-corpus is PARTIALLY vectorised: {len(missing)} of {len(chunks)} "
            f"chunks have no embedding.\n"
            "Their sources changed since the last encode, so the carry-forward "
            "dropped their vectors. Re-run the second pass:\n"
            "    cargo run -p cargo-teksilo --features semantic -- teksilo build-vectors\n"
            "Affected sources:",
            file=sys.stderr,
        )
        for path in shown:
            print(f"  {path}", file=sys.stderr)
        if len({c["path"] for c in missing}) > len(shown):
            print("  …", file=sys.stderr)
        return 1

    if not quiet:
        print(f"teksilo-corpus: {len(chunks)} chunks, all vectorised with {encoder} ({dim}).")
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Build the teksilo-corpus retrieval corpus.")
    parser.add_argument(
        "--check",
        action="store_true",
        help="regenerate into a temp dir and diff against the committed corpus/; exit 1 on drift",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="write the generated corpus into this directory instead of crates/teksilo-corpus/corpus",
    )
    parser.add_argument(
        "--check-vectors",
        action="store_true",
        help="verify every chunk carries an embedding and the encoder is named; exit 1 otherwise",
    )
    parser.add_argument("--quiet", action="store_true", help="suppress the summary print")
    args = parser.parse_args(argv)

    if args.check_vectors:
        return check_vectors(DEFAULT_OUT_DIR / "index.json", quiet=args.quiet)

    if args.check:
        import tempfile

        with tempfile.TemporaryDirectory(prefix="teksilo-corpus-check-") as tmp:
            tmp_out = Path(tmp) / "corpus"
            try:
                build_corpus(tmp_out)
            except CorpusVerificationError as e:
                print(f"teksilo-corpus line-range verification FAILED:\n{e}", file=sys.stderr)
                return 1
            problems = diff_index(
                DEFAULT_OUT_DIR / "index.json", tmp_out / "index.json"
            )
            if problems:
                print(
                    "teksilo-corpus is STALE: crates/teksilo-corpus/corpus/ does not match "
                    "what tools/build_corpus.py generates.\n"
                    "Run `python3 tools/build_corpus.py` and commit the result.\n"
                    "Differences:",
                    file=sys.stderr,
                )
                for p in problems:
                    print(p, file=sys.stderr)
                return 1
            if not args.quiet:
                print("teksilo-corpus is up to date (no drift).")
            return 0

    out_dir = args.out if args.out is not None else DEFAULT_OUT_DIR
    try:
        index, verified = build_corpus(out_dir)
    except CorpusVerificationError as e:
        print(f"teksilo-corpus line-range verification FAILED:\n{e}", file=sys.stderr)
        return 1
    if not args.quiet:
        print(format_summary(index, out_dir, verified))
    return 0


if __name__ == "__main__":
    sys.exit(main())
