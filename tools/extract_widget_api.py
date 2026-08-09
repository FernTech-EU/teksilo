#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""
Extract public API and inline documentation from teksilo-widgets source files.

Walks `crates/teksilo-widgets/src/` recursively (top-level files plus submodule
directories like `notification/`, `tab_widget/`, `primitives/`, `animations/`),
looks up the widget file for each requested name, and emits:

  - The file's `//!` module header doc
  - Every `pub struct` / `pub enum` / `pub type` / `pub const` with its `///` doc
  - Every `pub fn` inside inherent `impl Foo { ... }` blocks with its `///` doc
  - Enum variants with their own `///` docs

Trait impls like `impl Widget for Foo` are skipped — they are internal plumbing,
not part of the widget's builder API.

`--list` shows only the widget(s) per file, not every exported type. A genuine
widget is a pub type that both `impl Widget` and is re-exported from the crate
root (`lib.rs`). Config enums (e.g. `IconLocation`) and internal helpers (e.g.
`HeaderCell`) are therefore filtered out of the listing — but every type remains
addressable by name (`extract_widget_api.py IconLocation` still works). The
flat, one-widget-per-file dirs (top-level, `primitives/`, `animations/`) keep a
lenient fallback so no conventional module is ever hidden.

Usage:
    python tools/extract_widget_api.py Button HStack
    python tools/extract_widget_api.py --all
    python tools/extract_widget_api.py --list
    python tools/extract_widget_api.py Button --format json
    python tools/extract_widget_api.py Button -o out.md
    python tools/extract_widget_api.py --md-dir docs/widgets   # mdBook catalog

`--md-dir DIR` regenerates the mdBook "Widget Catalog": one Markdown page per
widget (deep-linking to its rustdoc module page), a grouped `index.md`, and an
in-place patch of the `<!-- BEGIN/END GENERATED WIDGETS -->` region of
`docs/SUMMARY.md`. The pages are build artifacts (gitignored) — regenerate them
before `mdbook build`.
"""

from __future__ import annotations

import argparse
import difflib
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent


@dataclass(frozen=True)
class CrateSpec:
    """A crate the catalog generator can document.

    `is_widget` selects the widget-specific behaviour (impl-Widget entry filter +
    widgets-overview.md categories); other crates surface their re-exported public
    types and group by directory.
    """

    crate: str  # cargo package name, e.g. "teksilo-widgets"
    rustdoc: str  # rustdoc crate dir, e.g. "teksilo_widgets"
    md_subdir: str  # output dir under docs/, e.g. "widgets"
    title: str  # SUMMARY part / index title, e.g. "Widget Catalog"
    group: str  # top-level group label in the index for non-widget crates
    is_widget: bool

    @property
    def src(self) -> Path:
        return REPO_ROOT / "crates" / self.crate / "src"

    @property
    def marker_begin(self) -> str:
        return f"<!-- BEGIN GENERATED {self.md_subdir.upper()} -->"

    @property
    def marker_end(self) -> str:
        return f"<!-- END GENERATED {self.md_subdir.upper()} -->"


CRATE_SPECS: dict[str, CrateSpec] = {
    "widgets": CrateSpec("teksilo-widgets", "teksilo_widgets", "widgets", "Widget Catalog", "Widgets", True),
    "data": CrateSpec("teksilo-data", "teksilo_data", "data-collections", "Data Collections", "Models", False),
    "settings": CrateSpec("teksilo-settings", "teksilo_settings", "settings", "Settings", "Stores & services", False),
    "scene": CrateSpec("teksilo-scene", "teksilo_scene", "scene", "Scene", "Scene", False),
}

# The active crate, set by `main()` from `--crate` (default: widgets).
SPEC: CrateSpec = CRATE_SPECS["widgets"]

# Back-compat aliases for the widget crate (used by widget-only helpers).
WIDGETS_SRC = CRATE_SPECS["widgets"].src
PRIMITIVES_DIR = WIDGETS_SRC / "primitives"
ANIMATIONS_DIR = WIDGETS_SRC / "animations"

# Aggregator files we never treat as a catalog entry.
SKIP_FILES = {"lib.rs", "primitives.rs", "animations.rs", "layout_integration_tests.rs", "mod.rs"}


# ----------------------------------------------------------------------------
# Data model
# ----------------------------------------------------------------------------


@dataclass
class EnumVariant:
    name: str
    signature: str
    doc: str


@dataclass
class Item:
    kind: str  # 'struct' | 'enum' | 'type' | 'const' | 'fn' | 'external'
    name: str
    signature: str
    doc: str
    hidden: bool = False
    cfg: list[str] = field(default_factory=list)
    variants: list[EnumVariant] = field(default_factory=list)
    methods: list["Item"] = field(default_factory=list)


@dataclass
class ParsedFile:
    module_name: str
    file_path: Path
    header_doc: str
    cfg: list[str]
    items: list[Item]


# ----------------------------------------------------------------------------
# Line cleanup — blank out string/char literals and comments so we can count
# braces / parens without being fooled by characters inside literals.
# ----------------------------------------------------------------------------


class BlockCommentTracker:
    """Removes /* ... */ content (possibly multi-line), keeping line length.

    Does not try to handle nested block comments correctly for depth > 1;
    teksilo-widgets code does not use them.
    """

    def __init__(self) -> None:
        self.in_block = False

    def process(self, line: str) -> str:
        out: list[str] = []
        i = 0
        n = len(line)
        while i < n:
            if self.in_block:
                j = line.find("*/", i)
                if j < 0:
                    out.append(" " * (n - i))
                    return "".join(out)
                out.append(" " * (j + 2 - i))
                i = j + 2
                self.in_block = False
            else:
                j = line.find("/*", i)
                if j < 0:
                    out.append(line[i:])
                    return "".join(out)
                out.append(line[i:j])
                out.append("  ")
                i = j + 2
                self.in_block = True
        return "".join(out)


def strip_line_literals(line: str) -> str:
    """Return a line with string/char literals and line-comment content replaced
    by spaces, so brace/paren counting is safe."""
    out: list[str] = []
    i = 0
    n = len(line)
    while i < n:
        c = line[i]
        if c == "/" and i + 1 < n and line[i + 1] == "/":
            # Line comment — preserve leading `//` so we can still recognize
            # `///` and `//!`, but blank the rest.
            out.append("//")
            out.append(" " * (n - i - 2))
            return "".join(out)
        if c == '"':
            out.append(" ")
            i += 1
            while i < n:
                if line[i] == "\\" and i + 1 < n:
                    out.append("  ")
                    i += 2
                elif line[i] == '"':
                    out.append(" ")
                    i += 1
                    break
                else:
                    out.append(" ")
                    i += 1
            continue
        if c == "'":
            # Char literal or lifetime?  Char literals: 'x', '\n', '\u{...}'.
            m = re.match(r"'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^'\\])'", line[i:])
            if m:
                out.append(" " * len(m.group(0)))
                i += len(m.group(0))
                continue
            out.append(c)
            i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def preprocess(raw_lines: list[str]) -> list[str]:
    bc = BlockCommentTracker()
    return [strip_line_literals(bc.process(line)) for line in raw_lines]


# ----------------------------------------------------------------------------
# Regex helpers
# ----------------------------------------------------------------------------

DOC_OUTER = re.compile(r"^\s*///(.*)$")
DOC_INNER = re.compile(r"^\s*//!(.*)$")
ATTR_PREFIX = re.compile(r"^\s*#\[")
IMPL_PREFIX = re.compile(r"^\s*impl\b")
PUB_STRUCT = re.compile(r"^\s*pub\s+struct\s+([A-Za-z_]\w*)")
PUB_ENUM = re.compile(r"^\s*pub\s+enum\s+([A-Za-z_]\w*)")
PUB_TYPE = re.compile(r"^\s*pub\s+type\s+([A-Za-z_]\w*)")
PUB_CONST = re.compile(r"^\s*pub\s+(?:const|static)\s+([A-Za-z_]\w*)")
PUB_FN = re.compile(
    r"^\s*pub"  # only fully-public; `pub(crate)` etc. are excluded
    r"(?:\s+(?:async|const|unsafe|extern(?:\s+\"[^\"]*\")?))*"
    r"\s+fn\s+([A-Za-z_]\w*)"
)
# `impl ... Widget for X` — allows a path-qualified trait (`widget::Widget`),
# leading generics (`impl<T> Widget for Foo<T>`), and captures the target type
# name. `\bWidget\s+for` won't match `WidgetBuilder for` (no whitespace after
# `Widget`).
WIDGET_IMPL_RE = re.compile(r"impl\b[^{]*?\bWidget\s+for\s+([A-Za-z_]\w*)")


def _doc_text(m: re.Match[str]) -> str:
    """Strip one optional leading space from a /// or //! capture."""
    s = m.group(1)
    if s.startswith(" "):
        s = s[1:]
    return s


# ----------------------------------------------------------------------------
# Low-level scanners — all operate on (raw, cleaned) parallel line lists
# ----------------------------------------------------------------------------


def find_matching_brace(
    cleaned: list[str], start_line: int, start_col: int
) -> tuple[int, int]:
    """Given that cleaned[start_line][start_col] == '{', return (line, col) of
    the matching '}'."""
    depth = 0
    for li in range(start_line, len(cleaned)):
        line = cleaned[li]
        start = start_col if li == start_line else 0
        for ci in range(start, len(line)):
            c = line[ci]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    return li, ci
    return len(cleaned) - 1, 0


def consume_attribute(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int]:
    """Parse a possibly multi-line #[ ... ] attribute starting at line i.
    Return (attribute_text_joined_on_one_line, end_line_idx)."""
    depth = 0
    started = False
    for li in range(i, len(cleaned)):
        line = cleaned[li]
        for col, c in enumerate(line):
            if c == "[":
                depth += 1
                started = True
            elif c == "]":
                depth -= 1
                if started and depth == 0:
                    if li == i:
                        return raw[i][: col + 1].strip(), i
                    parts = (
                        [raw[i].strip()]
                        + [raw[j].strip() for j in range(i + 1, li)]
                        + [raw[li][: col + 1].strip()]
                    )
                    return " ".join(p for p in parts if p), li
    # Malformed — consume a single line.
    return raw[i].strip(), i


def _join_signature(raw: list[str], start: int, end_line: int, end_col: int) -> str:
    if end_line == start:
        return raw[start][:end_col].rstrip()
    parts = [raw[start]]
    parts.extend(raw[j] for j in range(start + 1, end_line))
    parts.append(raw[end_line][:end_col])
    return "\n".join(parts).rstrip()


def consume_stmt(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int]:
    """Consume a statement ending with `;` at brace-depth 0. Returns
    (signature_including_semicolon, end_line_idx)."""
    depth = 0
    for li in range(i, len(cleaned)):
        for col, c in enumerate(cleaned[li]):
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
            elif depth == 0 and c == ";":
                return _join_signature(raw, i, li, col + 1), li
    return raw[i].rstrip(), i


def consume_item_signature(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int, tuple[int, int] | None]:
    """Parse `pub struct X ...;` or `pub struct X ... { ... }` or
    `pub enum X { ... }`. Returns (signature_without_body, end_line,
    brace_open_pos_or_None). If the item is unit/tuple (ends with ;), returns
    (sig_with_trailing_semi, line_of_semi, None). Otherwise returns
    (sig_up_to_but_not_including_brace, line_of_closing_brace, (open_line,
    open_col))."""
    paren = 0
    for li in range(i, len(cleaned)):
        for col, c in enumerate(cleaned[li]):
            if c == "(":
                paren += 1
            elif c == ")":
                paren -= 1
            elif paren == 0 and c == ";":
                return _join_signature(raw, i, li, col + 1), li, None
            elif paren == 0 and c == "{":
                sig = _join_signature(raw, i, li, col)
                close_line, _ = find_matching_brace(cleaned, li, col)
                return sig, close_line, (li, col)
    return raw[i].rstrip(), i, None


def consume_fn_signature(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int, bool]:
    """Parse a `fn name(...) -> ... { body }` or `fn name(...) -> ... ;`
    starting at line i. Returns (signature_without_brace_or_semi, end_line,
    had_body). For had_body==True, end_line is the line of the matching `}`."""
    paren = 0
    started_paren = False
    for li in range(i, len(cleaned)):
        for col, c in enumerate(cleaned[li]):
            if c == "(":
                paren += 1
                started_paren = True
            elif c == ")":
                paren -= 1
            elif paren == 0 and started_paren and c == ";":
                return _join_signature(raw, i, li, col), li, False
            elif paren == 0 and started_paren and c == "{":
                sig = _join_signature(raw, i, li, col)
                close_line, _ = find_matching_brace(cleaned, li, col)
                return sig, close_line, True
    return raw[i].rstrip(), i, False


def consume_impl_header(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int, int] | None:
    """Find the `{` that opens the impl block. Return (header_text,
    open_line, open_col) or None if malformed."""
    for li in range(i, len(cleaned)):
        for col, c in enumerate(cleaned[li]):
            if c == "{":
                header = _join_signature(raw, i, li, col)
                return header, li, col
    return None


# ----------------------------------------------------------------------------
# impl block analysis
# ----------------------------------------------------------------------------


_HRTB_RE = re.compile(r"for\s*<[^>]*>")


def _normalize_impl_header(header: str) -> str:
    """Strip higher-ranked trait bound patterns like `for<'a>` so we can
    detect the trait-impl `for` keyword cleanly."""
    return _HRTB_RE.sub(" ", header)


def is_trait_impl(header: str) -> bool:
    return re.search(r"\bfor\b", _normalize_impl_header(header)) is not None


def extract_impl_target(header: str) -> str:
    """Return the target type name of an `impl ...` header (the thing the
    methods attach to)."""
    normalized = _normalize_impl_header(header)
    # Drop the leading `impl` keyword.
    m = re.match(r"\s*impl\b", normalized)
    if not m:
        return ""
    body = normalized[m.end() :].strip()

    # Strip leading generics <...>.
    if body.startswith("<"):
        depth = 0
        j = 0
        while j < len(body):
            if body[j] == "<":
                depth += 1
            elif body[j] == ">":
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            j += 1
        body = body[j:].strip()

    if re.search(r"\bfor\b", body):
        # `Trait for Target [where ...]`
        _, _, after = body.partition(" for ")
        if not after:
            # Fallback for cases where there's no space around `for`.
            after = re.split(r"\bfor\b", body, maxsplit=1)[1]
        target_part = after.strip()
    else:
        target_part = body

    target_part = re.split(r"\bwhere\b", target_part, maxsplit=1)[0].strip()
    target_part = target_part.rstrip("{").strip()
    m2 = re.match(r"([A-Za-z_][A-Za-z0-9_]*)", target_part)
    return m2.group(1) if m2 else ""


# ----------------------------------------------------------------------------
# Per-file parser
# ----------------------------------------------------------------------------


def _is_hidden(attrs: list[str]) -> bool:
    return any(
        re.search(r"#\[\s*doc\s*\(\s*hidden\s*\)\s*\]", a) for a in attrs
    )


def _extract_cfgs(attrs: list[str]) -> list[str]:
    return [a for a in attrs if re.search(r"#\[\s*cfg\s*\(", a)]


def _parse_enum_variants(
    raw: list[str], cleaned: list[str], start: int, end: int
) -> list[EnumVariant]:
    """Parse variants from cleaned[start..=end] which is the interior of the
    enum body (between `{` and `}`)."""
    variants: list[EnumVariant] = []
    v_doc: list[str] = []
    i = start
    while i <= end:
        line = raw[i]
        cln = cleaned[i]
        stripped = line.strip()

        m = DOC_OUTER.match(line)
        if m:
            v_doc.append(_doc_text(m))
            i += 1
            continue
        if stripped.startswith("#["):
            _, end_i = consume_attribute(raw, cleaned, i)
            i = end_i + 1
            continue
        if not stripped:
            i += 1
            continue

        # Start of a variant. Collect until `,` at brace/paren-depth 0, or end.
        paren = 0
        brace = 0
        end_line = None
        end_col = None
        for li in range(i, end + 1):
            for col, c in enumerate(cleaned[li]):
                if c == "(":
                    paren += 1
                elif c == ")":
                    paren -= 1
                elif c == "{":
                    brace += 1
                elif c == "}":
                    brace -= 1
                elif paren == 0 and brace == 0 and c == ",":
                    end_line, end_col = li, col
                    break
            if end_line is not None:
                break
        if end_line is None:
            end_line = end
            end_col = len(cleaned[end])

        sig = _join_signature(raw, i, end_line, end_col).strip()
        name_m = re.match(r"(\w+)", sig)
        name = name_m.group(1) if name_m else "<?>"
        variants.append(
            EnumVariant(
                name=name,
                signature=sig,
                doc="\n".join(v_doc).rstrip(),
            )
        )
        v_doc = []
        i = end_line + 1
    return variants


def _parse_impl_body(
    raw: list[str], cleaned: list[str], start: int, end: int
) -> list[Item]:
    """Parse inside an inherent impl block body (between `{` and `}`). Emit
    only public associated items: `pub fn`, `pub const`, `pub type`."""
    methods: list[Item] = []
    doc_buf: list[str] = []
    attr_buf: list[str] = []

    def clear() -> None:
        doc_buf.clear()
        attr_buf.clear()

    i = start
    while i <= end:
        line = raw[i]
        cln = cleaned[i]
        stripped = line.strip()

        m = DOC_OUTER.match(line)
        if m:
            doc_buf.append(_doc_text(m))
            i += 1
            continue
        if stripped.startswith("#["):
            attr, end_i = consume_attribute(raw, cleaned, i)
            attr_buf.append(attr)
            i = end_i + 1
            continue
        if not stripped:
            # Blank line between /// block and item would break attachment in
            # Rust. Detect by peeking ahead.
            if doc_buf or attr_buf:
                j = i + 1
                while j <= end and not raw[j].strip():
                    j += 1
                if j > end:
                    clear()
                else:
                    nxt = raw[j]
                    if not (
                        DOC_OUTER.match(nxt) or nxt.strip().startswith("#[")
                    ):
                        clear()
            i += 1
            continue

        if PUB_FN.match(line):
            name = PUB_FN.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line, had_body = consume_fn_signature(raw, cleaned, i)
            methods.append(
                Item(
                    kind="fn",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        if PUB_CONST.match(line):
            name = PUB_CONST.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line = consume_stmt(raw, cleaned, i)
            methods.append(
                Item(
                    kind="const",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        if PUB_TYPE.match(line):
            name = PUB_TYPE.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line = consume_stmt(raw, cleaned, i)
            methods.append(
                Item(
                    kind="type",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        # Non-pub item inside impl — skip, count as body. For simplicity we
        # advance one line; brace balancing is handled by the outer caller
        # which computed `end`.
        clear()
        i += 1
    return methods


def parse_file(path: Path, module_name: str, cfg: list[str]) -> ParsedFile:
    raw = path.read_text(encoding="utf-8").splitlines()
    cleaned = preprocess(raw)
    n = len(raw)

    # --- Module header: the //! block near the top. Every file opens with two
    # `// SPDX-...` license comments (and occasionally a `#![...]` inner attr)
    # BEFORE the //! header, so skip those leading lines until the //! block
    # starts; once it starts, stop at the first non-//! non-blank line.
    header_lines: list[str] = []
    started = False
    i = 0
    while i < n:
        stripped = raw[i].strip()
        if not stripped:
            if started:
                header_lines.append("")
            i += 1
            continue
        m = DOC_INNER.match(raw[i])
        if m:
            started = True
            header_lines.append(_doc_text(m))
            i += 1
            continue
        if not started and (stripped.startswith("//") or stripped.startswith("#![")):
            # Leading license comment / inner attribute before the header.
            i += 1
            continue
        # Stop at the first real (non-//!) line once we've passed the preamble.
        break
    while header_lines and not header_lines[-1].strip():
        header_lines.pop()
    header_doc = "\n".join(header_lines)

    items: list[Item] = []
    item_by_name: dict[str, Item] = {}

    doc_buf: list[str] = []
    attr_buf: list[str] = []

    def clear() -> None:
        doc_buf.clear()
        attr_buf.clear()

    def ensure_item(name: str) -> Item:
        it = item_by_name.get(name)
        if it is not None:
            return it
        placeholder = Item(
            kind="external", name=name, signature="", doc=""
        )
        items.append(placeholder)
        item_by_name[name] = placeholder
        return placeholder

    i = 0
    while i < n:
        line = raw[i]
        stripped = line.strip()

        # Skip //! anywhere (module header already captured).
        if DOC_INNER.match(line):
            i += 1
            continue

        m = DOC_OUTER.match(line)
        if m:
            doc_buf.append(_doc_text(m))
            i += 1
            continue

        if stripped.startswith("#["):
            attr, end_i = consume_attribute(raw, cleaned, i)
            attr_buf.append(attr)
            i = end_i + 1
            continue

        if not stripped:
            # Blank line between pending docs and an unrelated item breaks
            # attachment; detect and clear.
            if doc_buf or attr_buf:
                j = i + 1
                while j < n and not raw[j].strip():
                    j += 1
                if j >= n:
                    clear()
                else:
                    nxt = raw[j]
                    if not (DOC_OUTER.match(nxt) or nxt.strip().startswith("#[")):
                        clear()
            i += 1
            continue

        # impl block
        if IMPL_PREFIX.match(line):
            parsed = consume_impl_header(raw, cleaned, i)
            if parsed is None:
                clear()
                i += 1
                continue
            header, open_line, open_col = parsed
            close_line, _ = find_matching_brace(cleaned, open_line, open_col)

            if is_trait_impl(header):
                # Skip the whole block — internal plumbing.
                clear()
                i = close_line + 1
                continue

            target = extract_impl_target(header)
            methods = _parse_impl_body(
                raw, cleaned, open_line + 1, close_line - 1
            )
            parent = ensure_item(target) if target else None
            if parent is not None:
                parent.methods.extend(methods)
            clear()
            i = close_line + 1
            continue

        if PUB_STRUCT.match(line):
            name = PUB_STRUCT.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line, _open = consume_item_signature(raw, cleaned, i)
            item = Item(
                kind="struct",
                name=name,
                signature=sig.strip(),
                doc="\n".join(doc_buf).rstrip(),
                hidden=_is_hidden(attr_buf),
                cfg=_extract_cfgs(attr_buf),
            )
            items.append(item)
            item_by_name[name] = item
            clear()
            i = end_line + 1
            continue

        if PUB_ENUM.match(line):
            name = PUB_ENUM.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line, open_pos = consume_item_signature(raw, cleaned, i)
            variants: list[EnumVariant] = []
            if open_pos is not None:
                open_line, _oc = open_pos
                variants = _parse_enum_variants(
                    raw, cleaned, open_line + 1, end_line - 1
                )
            item = Item(
                kind="enum",
                name=name,
                signature=sig.strip(),
                doc="\n".join(doc_buf).rstrip(),
                hidden=_is_hidden(attr_buf),
                cfg=_extract_cfgs(attr_buf),
                variants=variants,
            )
            items.append(item)
            item_by_name[name] = item
            clear()
            i = end_line + 1
            continue

        if PUB_TYPE.match(line):
            name = PUB_TYPE.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line = consume_stmt(raw, cleaned, i)
            items.append(
                Item(
                    kind="type",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        if PUB_CONST.match(line):
            name = PUB_CONST.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line = consume_stmt(raw, cleaned, i)
            items.append(
                Item(
                    kind="const",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        if PUB_FN.match(line):
            name = PUB_FN.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line, _had_body = consume_fn_signature(raw, cleaned, i)
            items.append(
                Item(
                    kind="fn",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        # Anything else (use, mod, private items…) — drop pending metadata.
        clear()
        i += 1

    # Drop `external` placeholder items that never received any methods —
    # they're types defined elsewhere that we don't care about.
    items = [
        it
        for it in items
        if it.kind != "external" or it.methods
    ]
    return ParsedFile(
        module_name=module_name,
        file_path=path,
        header_doc=header_doc,
        cfg=cfg,
        items=items,
    )


# ----------------------------------------------------------------------------
# Widget registry — discover files, build name lookup
# ----------------------------------------------------------------------------


@dataclass
class Registry:
    files: list[Path]
    cfg_by_file: dict[Path, list[str]]
    type_to_file: dict[str, Path]  # lowercased type name -> file
    module_to_file: dict[str, Path]  # lowercased file stem -> file
    type_display: dict[Path, list[str]]  # file -> all exported type names
    widget_display: dict[Path, list[str]]  # file -> just the widget type(s)
    exported: set[str] = field(default_factory=set)  # names re-exported from lib.rs


def _stem_to_camel(stem: str) -> str:
    """`date_edit` -> `DateEdit`, `button` -> `Button`."""
    return "".join(part[:1].upper() + part[1:] for part in stem.split("_") if part)


def _is_test_file(p: Path) -> bool:
    return p.name == "tests.rs" or p.name.endswith("_tests.rs")


_EXPORT_TOKEN_RE = re.compile(r"[A-Z][A-Za-z0-9_]*")


def _parse_public_exports(lib_rs: Path) -> set[str]:
    """Return every type-ish name re-exported via `pub use ...;` in lib.rs.

    The crate root lists its public surface explicitly (no `::*` globs), so the
    CamelCase / SCREAMING tokens inside each `pub use` statement are exactly the
    publicly reachable type and const names. snake_case path segments are
    lowercase and so excluded by the leading-uppercase requirement.
    """
    if not lib_rs.exists():
        return set()
    text = lib_rs.read_text(encoding="utf-8")
    exported: set[str] = set()
    for m in re.finditer(r"\bpub\s+use\b(.*?);", text, re.DOTALL):
        exported.update(_EXPORT_TOKEN_RE.findall(m.group(1)))
    return exported


def _pick_widget_names(
    stem: str,
    pub_names: list[str],
    widget_impls: set[str],
    exported: set[str],
    nested: bool,
    is_widget: bool = True,
) -> list[str]:
    """Choose which names to surface for a file in `--list` / the catalog.

    For non-widget crates a catalog entry is simply a file's re-exported public
    type(s), preferring the one matching the file stem; a file with no re-exported
    type gets no page (this drops impl-split modules like `view/gestures_impl.rs`
    and internal helpers).

    For the widget crate a genuine widget is a pub type that both `impl Widget`
    and is re-exported. Nested submodule files have no fallback (keeps helpers
    like `HeaderCell` out); top-level files keep the lenient historical fallback.
    """
    camel = _stem_to_camel(stem)
    if not is_widget:
        candidates = [n for n in pub_names if n in exported]
        if camel in candidates:
            return [camel]
        return candidates

    candidates = [n for n in pub_names if n in widget_impls and n in exported]
    if camel in candidates:
        return [camel]
    if candidates:
        return candidates
    if nested:
        return []

    # Top-level fallback — preserve historical behaviour.
    widget_pub = [n for n in pub_names if n in widget_impls]
    if camel in widget_pub:
        return [camel]
    if widget_pub:
        return widget_pub
    if camel in pub_names:
        return [camel]
    return pub_names


def _collect_cfgs_from_aggregator(aggregator: Path, base: Path) -> dict[Path, list[str]]:
    """Parse an aggregator file (lib.rs, primitives.rs) and return
    file-path -> list of #[cfg(...)] attrs attached to its `pub mod` line."""
    out: dict[Path, list[str]] = {}
    if not aggregator.exists():
        return out
    raw = aggregator.read_text(encoding="utf-8").splitlines()
    pending: list[str] = []
    for line in raw:
        s = line.strip()
        if not s or s.startswith("//"):
            continue
        if s.startswith("#["):
            if re.search(r"#\[\s*cfg\s*\(", s):
                pending.append(s)
            continue
        m = re.match(r"(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+(\w+)\s*;", s)
        if m:
            name = m.group(1)
            target = base / f"{name}.rs"
            if target.exists():
                out[target.resolve()] = list(pending)
            pending = []
            continue
        # Any other statement resets pending cfgs.
        pending = []
    return out


def build_registry() -> Registry:
    src = SPEC.src
    if not src.exists():
        raise SystemExit(f"{SPEC.crate} src not found at {src}")

    # Recursive discovery so types defined in submodule directories are found
    # too. Shallow paths sort first so the conventional top-level file wins any
    # name/module collision.
    files = sorted(
        (
            p
            for p in src.rglob("*.rs")
            if p.name not in SKIP_FILES and not _is_test_file(p)
        ),
        key=lambda p: (len(p.relative_to(src).parts), str(p)),
    )

    cfg_by_file: dict[Path, list[str]] = {}
    cfg_by_file.update(_collect_cfgs_from_aggregator(src / "lib.rs", src))
    if SPEC.is_widget:
        cfg_by_file.update(
            _collect_cfgs_from_aggregator(src / "primitives.rs", src / "primitives")
        )
        cfg_by_file.update(
            _collect_cfgs_from_aggregator(src / "animations.rs", src / "animations")
        )

    # Nested files inherit the cfg of their top-level ancestor module
    # (e.g. color_picker/swatch.rs inherits color_picker.rs's rich-text gate).
    for fp in files:
        rel = fp.relative_to(src)
        if len(rel.parts) > 1 and fp.resolve() not in cfg_by_file:
            ancestor = (src / f"{rel.parts[0]}.rs").resolve()
            inherited = cfg_by_file.get(ancestor)
            if inherited:
                cfg_by_file[fp.resolve()] = list(inherited)

    exported = _parse_public_exports(src / "lib.rs")

    type_to_file: dict[str, Path] = {}
    module_to_file: dict[str, Path] = {}
    type_display: dict[Path, list[str]] = {}
    widget_display: dict[Path, list[str]] = {}
    type_re = re.compile(r"^\s*pub\s+(?:struct|enum|type|trait)\s+([A-Za-z_]\w*)", re.MULTILINE)

    for fp in files:
        module_to_file.setdefault(fp.stem.lower(), fp)
        text = fp.read_text(encoding="utf-8")
        names = [m.group(1) for m in type_re.finditer(text)]
        type_display[fp] = names
        widget_impls = (
            {m.group(1) for m in WIDGET_IMPL_RE.finditer(text)} if SPEC.is_widget else set()
        )
        # `primitives/` and `animations/` are flat one-type-per-file collections,
        # like the top-level dir, so they keep the lenient fallback. Per-widget
        # submodule dirs use strict re-export filtering to drop internal helpers.
        rel_parts = fp.relative_to(src).parts
        lenient = len(rel_parts) == 1 or (
            len(rel_parts) == 2 and rel_parts[0] in ("primitives", "animations")
        )
        widget_display[fp] = _pick_widget_names(
            fp.stem, names, widget_impls, exported, nested=not lenient,
            is_widget=SPEC.is_widget,
        )
        for name in names:
            type_to_file.setdefault(name.lower(), fp)

    return Registry(
        files=files,
        cfg_by_file=cfg_by_file,
        type_to_file=type_to_file,
        module_to_file=module_to_file,
        type_display=type_display,
        widget_display=widget_display,
        exported=exported,
    )


def resolve_name(reg: Registry, name: str) -> Path | None:
    key = name.lower()
    if key in reg.type_to_file:
        return reg.type_to_file[key]
    if key in reg.module_to_file:
        return reg.module_to_file[key]
    return None


# ----------------------------------------------------------------------------
# Formatters
# ----------------------------------------------------------------------------


def _fmt_cfg(cfg: list[str]) -> str:
    return " ".join(cfg)


def format_markdown(pf: ParsedFile) -> str:
    out: list[str] = []
    rel = pf.file_path.relative_to(REPO_ROOT) if REPO_ROOT in pf.file_path.parents else pf.file_path
    out.append(f"# `{pf.module_name}.rs`")
    out.append("")
    out.append(f"> Source: [{rel}]({rel})")
    if pf.cfg:
        out.append(f"> cfg: `{_fmt_cfg(pf.cfg)}`")
    out.append("")

    if pf.header_doc:
        out.append(pf.header_doc.rstrip())
        out.append("")

    _emit_items_md(pf.items, out)

    return "\n".join(out).rstrip() + "\n"


def _emit_items_md(items: list[Item], out: list[str]) -> None:
    """Render the struct/enum/type/const/fn items of a file as Markdown.

    Shared by `format_markdown` (the `--format md` output) and
    `format_catalog_markdown` (the mdBook catalog pages) so both render the API
    surface identically.
    """
    for it in items:
        if it.kind == "external":
            # Type defined elsewhere, but has methods in this file.
            out.append(f"## `impl {it.name}`  *(methods defined in this file)*")
            out.append("")
            _emit_methods_md(it, out)
            continue

        flags: list[str] = []
        if it.hidden:
            flags.append("hidden")
        if it.cfg:
            flags.append(_fmt_cfg(it.cfg))
        flags_str = f"  *({'; '.join(flags)})*" if flags else ""

        if it.kind == "struct":
            out.append(f"## `pub struct {it.name}`{flags_str}")
        elif it.kind == "enum":
            out.append(f"## `pub enum {it.name}`{flags_str}")
        elif it.kind == "type":
            out.append(f"## `pub type {it.name}`{flags_str}")
        elif it.kind == "const":
            out.append(f"## `pub const {it.name}`{flags_str}")
        elif it.kind == "fn":
            out.append(f"## `pub fn {it.name}(...)`{flags_str}")
        out.append("")

        if it.doc:
            out.append(it.doc.rstrip())
            out.append("")

        if it.kind == "struct":
            out.append("```rust")
            out.append(f"{it.signature} {{ /* fields */ }}"
                       if "{" not in it.signature and not it.signature.rstrip().endswith(";")
                       else it.signature)
            out.append("```")
            out.append("")
        elif it.kind == "enum":
            out.append("```rust")
            out.append(f"{it.signature} {{ /* variants */ }}")
            out.append("```")
            out.append("")
        elif it.kind in ("type", "const", "fn"):
            out.append("```rust")
            sig = it.signature
            if it.kind == "fn" and not sig.rstrip().endswith(";"):
                sig = sig + ";"
            out.append(sig)
            out.append("```")
            out.append("")

        if it.kind == "enum" and it.variants:
            out.append("### Variants")
            out.append("")
            for v in it.variants:
                doc = v.doc.replace("\n", " ").strip()
                if doc:
                    out.append(f"- **`{v.name}`** — {doc}")
                else:
                    out.append(f"- **`{v.name}`**")
            out.append("")

        if it.methods:
            _emit_methods_md(it, out)


def _emit_methods_md(it: Item, out: list[str]) -> None:
    out.append("### Methods")
    out.append("")
    for m in it.methods:
        flags: list[str] = []
        if m.hidden:
            flags.append("hidden")
        if m.cfg:
            flags.append(_fmt_cfg(m.cfg))
        flags_str = f"  *({'; '.join(flags)})*" if flags else ""
        # Collapse multi-line signatures onto one line: a `#### `...`` heading is
        # an inline-code span, so a wrapped signature (e.g. a long `composite_tooltip(
        # …, impl Widget + 'static)`) would leave the span unclosed and break the
        # heading's rendering.
        sig = " ".join(m.signature.split())
        out.append(f"#### `{sig}`{flags_str}")
        out.append("")
        if m.doc:
            out.append(m.doc.rstrip())
            out.append("")


def format_text(pf: ParsedFile) -> str:
    out: list[str] = []
    out.append(f"=== {pf.module_name}.rs ===")
    out.append(f"Path: {pf.file_path}")
    if pf.cfg:
        out.append(f"cfg: {_fmt_cfg(pf.cfg)}")
    out.append("")
    if pf.header_doc:
        out.append(pf.header_doc.rstrip())
        out.append("")

    for it in pf.items:
        if it.kind == "external":
            out.append(f"--- impl {it.name} (methods defined in this file) ---")
        else:
            tag = it.kind
            flags = []
            if it.hidden:
                flags.append("hidden")
            if it.cfg:
                flags.append(_fmt_cfg(it.cfg))
            fs = f" [{'; '.join(flags)}]" if flags else ""
            out.append(f"--- {tag} {it.name}{fs} ---")
            if it.doc:
                out.append(_indent(it.doc, "  "))
            if it.signature:
                out.append(f"  {it.signature.strip()}")

        if it.variants:
            out.append("  variants:")
            for v in it.variants:
                if v.doc:
                    out.append(f"    - {v.name}: {v.doc.splitlines()[0]}")
                else:
                    out.append(f"    - {v.name}")

        if it.methods:
            out.append("  methods:")
            for m in it.methods:
                flags = []
                if m.hidden:
                    flags.append("hidden")
                if m.cfg:
                    flags.append(_fmt_cfg(m.cfg))
                fs = f" [{'; '.join(flags)}]" if flags else ""
                out.append(f"    • {m.signature.strip()}{fs}")
                if m.doc:
                    out.append(_indent(m.doc, "      "))
        out.append("")

    return "\n".join(out).rstrip() + "\n"


def _indent(s: str, prefix: str) -> str:
    return "\n".join(prefix + line for line in s.splitlines())


def format_json(pfs: list[ParsedFile]) -> str:
    def item_to_dict(it: Item) -> dict:
        return {
            "kind": it.kind,
            "name": it.name,
            "signature": it.signature,
            "doc": it.doc,
            "hidden": it.hidden,
            "cfg": it.cfg,
            "variants": [v.__dict__ for v in it.variants],
            "methods": [item_to_dict(m) for m in it.methods],
        }

    payload = [
        {
            "module": pf.module_name,
            "file": str(pf.file_path),
            "cfg": pf.cfg,
            "header_doc": pf.header_doc,
            "items": [item_to_dict(it) for it in pf.items],
        }
        for pf in pfs
    ]
    return json.dumps(payload, indent=2)


# ----------------------------------------------------------------------------
# mdBook catalog generator
#
# Emits one Markdown page per widget into a book sub-directory (default
# `docs/widgets/`), a grouped `index.md`, and patches the auto-generated region
# of `docs/SUMMARY.md`. Each page deep-links to the widget's rustdoc module page
# so the mdBook "discovery" layer and the rustdoc "API reference" layer compose.
# ----------------------------------------------------------------------------


SUMMARY_BEGIN = "<!-- BEGIN GENERATED WIDGETS -->"
SUMMARY_END = "<!-- END GENERATED WIDGETS -->"

# Prepended to every generated page so they satisfy the SPDX pre-commit hook
# (the catalog Markdown is committed). Matches the repo's `.md` header style.
_MD_SPDX_HEADER = [
    "<!-- SPDX-License-Identifier: MPL-2.0 -->",
    "<!-- SPDX-FileCopyrightText: 2026 FernTech -->",
    "",
]

_OVERVIEW = REPO_ROOT / "docs" / "widgets-overview.md"
_OV_SECTION_RE = re.compile(r"^#{2,3}\s+(.+?)(?:\s+[—-].*)?$")
_OV_LINK_RE = re.compile(r"\]\([^)]*crates/teksilo-widgets/src/([^)\s]+?\.rs)[^)]*\)")


def _overview_category_map() -> "tuple[dict[str, str], list[str]]":
    """Parse docs/widgets-overview.md into {src-relative-path -> section} plus the
    section order. This is the single source of truth for catalog grouping, so the
    catalog index matches the hand-maintained overview (data-collection views land
    under "Data-driven widgets", buttons under "Buttons", etc.)."""
    mapping: dict[str, str] = {}
    order: list[str] = []
    if not _OVERVIEW.exists():
        return mapping, order
    cur: str | None = None
    for ln in _OVERVIEW.read_text(encoding="utf-8").splitlines():
        s = ln.strip()
        if s.startswith("#"):
            m = _OV_SECTION_RE.match(s)
            if m:
                name = m.group(1).strip()
                if name.lower() in ("cross-references", "styling status"):
                    cur = None
                else:
                    cur = name
                    if cur not in order:
                        order.append(cur)
            continue
        if cur and s.startswith("- "):
            for lm in _OV_LINK_RE.finditer(ln):
                mapping[lm.group(1)] = cur
    return mapping, order


def _catalog_title(reg: "Registry", pf: ParsedFile) -> str:
    """Human-facing title for a widget page (its primary widget type name)."""
    names = reg.widget_display.get(pf.file_path) or []
    if names:
        return names[0]
    return _stem_to_camel(pf.module_name)


def _catalog_category(fp: Path, overview: "dict[str, str] | None" = None) -> str:
    """Group label for a catalog file. The widget crate prefers the section it
    appears under in widgets-overview.md; other crates group by directory."""
    rel = fp.relative_to(SPEC.src)
    if SPEC.is_widget:
        if overview:
            hit = overview.get(rel.as_posix())
            if hit:
                return hit
        if len(rel.parts) == 1:
            return "Other"  # top-level file not in the overview
        top = rel.parts[0]
        return {
            "primitives": "Layout primitives",
            "animations": "Animations",
        }.get(top, f"{_stem_to_camel(top)} (submodule)")
    # Non-widget crate: top-level files share one group; submodules group by dir.
    if len(rel.parts) == 1:
        return SPEC.group
    return _stem_to_camel(rel.parts[0])


def _build_slugs(parsed: list[ParsedFile]) -> dict[Path, str]:
    """Stable, collision-free page slugs. Top-level files keep their clean stem
    (`button`, `list_model`); a genuine stem collision across directories falls
    back to the dir-prefixed path (`notification_log`)."""
    slugs: dict[Path, str] = {}
    used: set[str] = {"index"}  # reserved for the catalog landing page
    for pf in parsed:
        slug = pf.module_name
        if slug in used:
            rel = pf.file_path.relative_to(SPEC.src).with_suffix("")
            slug = "_".join(rel.parts)
        while slug in used:  # still collides (e.g. a top-level index.rs)
            slug = f"{slug}_"
        used.add(slug)
        slugs[pf.file_path] = slug
    return slugs


def _rustdoc_module_url(api_base: str, fp: Path, api_dir: "Path | None" = None) -> str:
    """rustdoc module-index URL for a catalog file, e.g.
    `button.rs` -> `<base>/teksilo_widgets/button/index.html`.

    With a built rustdoc tree (`api_dir`) the URL falls back to the nearest
    ancestor module that actually has a page — covering private `mod`s and
    cfg-gated modules that rustdoc omits. Without it, nested files (other than the
    widget crate's public `primitives/` & `animations/`) link to their top-level
    module as a best-effort guess.
    """
    rel = fp.relative_to(SPEC.src).with_suffix("")
    parts = list(rel.parts)

    def url(ps: list[str]) -> str:
        tail = "/".join([SPEC.rustdoc, *ps, "index.html"])
        return f"{api_base.rstrip('/')}/{tail}"

    if api_dir is not None:
        cand = list(parts)
        while cand:
            disk = Path(api_dir) / SPEC.rustdoc / Path(*cand) / "index.html"
            if disk.exists():
                return url(cand)
            cand = cand[:-1]
        return url([])

    if len(parts) >= 2 and not (SPEC.is_widget and parts[0] in ("primitives", "animations")):
        parts = parts[:1]
    return url(parts)


def _first_sentence(text: str, limit: int = 160) -> str:
    """First sentence of a module header, for the catalog index brief."""
    for line in text.splitlines():
        s = line.strip()
        if not s or s.startswith("#") or s.startswith("```"):
            continue
        # Stop at the first sentence boundary (". " followed by a capital).
        m = re.search(r"\.(?:\s|$)", s)
        sentence = s[: m.start()] if m else s
        sentence = sentence.strip()
        if len(sentence) > limit:
            sentence = sentence[: limit - 1].rstrip() + "…"
        return sentence
    return ""


# Doc text swept from Rust source carries link targets that don't resolve inside
# the mdBook catalog: rustdoc intra-doc links (`[`X`](crate::..)`, `[`X`](Self::..)`,
# `[`X`](self)`, bare `[`X`](TreeView)`, and the reference form `[`X`]` + `[`X`]:
# crate::X`) and repo-relative file links (`[x](../crates/..)`, `[x](locales/..)`).
# A catalog page is prose plus the single rustdoc-API link we add ourselves, so we
# KEEP only web URLs, in-page anchors, and our own `../api/...` link, and reduce
# every other link to plain inline code.
_INLINE_LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
_REF_DEF_RE = re.compile(r"^(\s*)\[([^\]]+)\]:\s*(\S+).*$")


def _as_code(label: str) -> str:
    label = label.strip()
    if label.startswith("`") and label.endswith("`"):
        return label
    return f"`{label}`"


def _catalog_keep_target(target: str) -> bool:
    """A link target that resolves inside the catalog as-is."""
    t = target.strip()
    return (
        t.startswith("#")
        or t.startswith("../api/")
        or t.startswith("mailto:")
        or "://" in t
    )


def _clean_catalog_links(md: str) -> str:
    """Reduce non-resolvable links in catalog doc text to plain inline code."""
    # Pass 1: drop reference DEFINITIONS we can't keep; remember their labels.
    dropped: set[str] = set()
    kept: list[str] = []
    for ln in md.split("\n"):
        m = _REF_DEF_RE.match(ln)
        if m and not _catalog_keep_target(m.group(3)):
            dropped.add(m.group(2).strip())
            continue
        kept.append(ln)
    md = "\n".join(kept)

    # Pass 2: inline links — keep web/anchor/api, strip the rest to code.
    def repl(m: "re.Match[str]") -> str:
        if _catalog_keep_target(m.group(2)):
            return m.group(0)
        return _as_code(m.group(1))

    md = _INLINE_LINK_RE.sub(repl, md)

    # Pass 3: shortcut / collapsed usages of the dropped labels -> code
    # (but not `[label](...)` inline or `[label][ref]` full-reference forms).
    for lbl in dropped:
        code = _as_code(lbl)
        md = md.replace(f"[{lbl}][]", code)
        md = re.sub(re.escape(f"[{lbl}]") + r"(?![\(\[])", code, md)
    # Pass 4: any remaining rustdoc shortcut link `[`X`]` (a backticked label with
    # no inline target and no reference definition) -> plain code, so the brackets
    # don't leak into the rendered book.
    md = re.sub(r"\[(`[^`\]]+`)\](?![\(\[:])", r"\1", md)
    return md


def _catalog_abilities(pf: ParsedFile, title: str) -> str:
    """A scannable inline list of the primary widget's builder methods."""
    primary = None
    for it in pf.items:
        if it.kind in ("struct", "external") and it.name == title and it.methods:
            primary = it
            break
    if primary is None:
        for it in pf.items:
            if it.methods:
                primary = it
                break
    if primary is None:
        return ""
    names = [
        f"`{m.name}`"
        for m in primary.methods
        if not m.hidden and m.name not in ("new", "default")
    ]
    return ", ".join(names)


def format_catalog_markdown(
    pf: ParsedFile,
    *,
    title: str,
    slug: str,
    api_base: str,
    img_dir: Path,
    api_dir: "Path | None" = None,
) -> str:
    """Render one widget's mdBook catalog page."""
    out: list[str] = list(_MD_SPDX_HEADER)
    out.append(f"# {title}")
    out.append("")

    if (img_dir / f"{slug}.png").exists():
        out.append(f"![{title} preview](img/{slug}.png)")
        out.append("")

    if pf.cfg:
        out.append(f"> Available under: `{_fmt_cfg(pf.cfg)}`")
        out.append("")

    if pf.header_doc:
        out.append(pf.header_doc.rstrip())
        out.append("")

    abilities = _catalog_abilities(pf, title)
    if abilities:
        out.append("## Builder methods at a glance")
        out.append("")
        out.append(abilities)
        out.append("")

    out.append("## API reference")
    out.append("")
    out.append(
        f"📖 [Full rustdoc API for this module]"
        f"({_rustdoc_module_url(api_base, pf.file_path, api_dir)})"
    )
    out.append("")
    _emit_items_md(pf.items, out)

    # The module header + item docs were swept from rustdoc-style source, so they
    # carry links that don't resolve in the book — reduce them to plain code.
    return _clean_catalog_links("\n".join(out).rstrip()) + "\n"


def format_catalog_index(
    reg: "Registry", parsed: list[ParsedFile], slugs: dict[Path, str]
) -> str:
    """The catalog landing page: every widget grouped by category, with a brief."""
    overview, ov_order = _overview_category_map()
    groups: dict[str, list[tuple[str, str, str]]] = {}
    for pf in parsed:
        cat = _catalog_category(pf.file_path, overview)
        title = _catalog_title(reg, pf)
        brief = _clean_catalog_links(_first_sentence(pf.header_doc))
        groups.setdefault(cat, []).append((title, slugs[pf.file_path], brief))

    # Order by the overview's section order, then any remaining groups (submodule
    # helpers, uncategorised) alphabetically.
    ordered = [c for c in ov_order if c in groups]
    ordered += sorted(c for c in groups if c not in ordered)

    out: list[str] = list(_MD_SPDX_HEADER)
    out.append(f"# {SPEC.title}")
    out.append("")
    noun = "widget" if SPEC.is_widget else "type"
    out.append(
        f"Every public {noun} in `{SPEC.crate}`, grouped by category. Each page "
        "links to its full rustdoc API reference."
    )
    out.append("")
    for cat in ordered:
        out.append(f"## {cat}")
        out.append("")
        for title, slug, brief in sorted(groups[cat], key=lambda t: t[0].lower()):
            line = f"- [{title}]({slug}.md)"
            if brief:
                line += f" — {brief}"
            out.append(line)
        out.append("")
    return "\n".join(out).rstrip() + "\n"


def _summary_block(
    reg: "Registry", parsed: list[ParsedFile], slugs: dict[Path, str], md_subdir: str
) -> str:
    """The auto-generated `docs/SUMMARY.md` region for the active crate: an
    `Overview` link followed by one alphabetically-sorted chapter per type (flat,
    so the static `# <title>` part header in SUMMARY.md groups them)."""
    lines = [SPEC.marker_begin, f"- [Overview]({md_subdir}/index.md)"]
    for pf in sorted(parsed, key=lambda p: _catalog_title(reg, p).lower()):
        title = _catalog_title(reg, pf)
        lines.append(f"- [{title}]({md_subdir}/{slugs[pf.file_path]}.md)")
    lines.append(SPEC.marker_end)
    return "\n".join(lines)


def patch_summary(summary_path: Path, block: str, begin: str, end: str) -> bool:
    """Replace the `begin`..`end` marked region of SUMMARY.md with `block`.
    Returns False if the markers are absent (caller then prints guidance)."""
    text = summary_path.read_text(encoding="utf-8")
    if begin not in text or end not in text:
        return False
    pre = text[: text.index(begin)]
    post = text[text.index(end) + len(end):]
    summary_path.write_text(pre + block + post, encoding="utf-8")
    return True


def merge_submodule_items(reg: "Registry", pf: ParsedFile) -> ParsedFile:
    """Fold a widget's submodule types into its own page.

    A widget laid out as `foo.rs` + `foo/` keeps helpers in the directory, and
    those files get no page of their own (they carry no `impl Widget`). But some
    of what lives there is genuinely public API re-exported from `lib.rs` —
    `segmented_control/id.rs`'s `SegmentId`, `tab_widget/id.rs`'s `TabId`. Those
    belong on the parent widget's page rather than nowhere at all.

    Only re-exported names are merged, so internal helpers stay out, and a type
    the parent file already documents is never duplicated.
    """
    sub_dir = pf.file_path.parent / pf.file_path.stem
    if not sub_dir.is_dir():
        return pf

    have = {item.name for item in pf.items}
    extra: list[Item] = []
    for sub in sorted(sub_dir.glob("*.rs")):
        if sub.name in SKIP_FILES or _is_test_file(sub):
            continue
        # A submodule that earns its own page (it declares a re-exported
        # `impl Widget` type, like `tab_widget/bar.rs`'s `TabBar`) is
        # documented there — merging it here too would duplicate it.
        if reg.widget_display.get(sub):
            continue
        sub_pf = parse_file(sub, sub.stem, reg.cfg_by_file.get(sub.resolve(), []))
        for item in sub_pf.items:
            if item.name in have or item.name not in reg.exported:
                continue
            have.add(item.name)
            extra.append(item)

    if not extra:
        return pf
    return ParsedFile(
        module_name=pf.module_name,
        file_path=pf.file_path,
        header_doc=pf.header_doc,
        cfg=pf.cfg,
        items=pf.items + extra,
    )


def cmd_md_dir(
    reg: "Registry", md_dir: str, api_base: str, api_dir: "Path | None" = None
) -> int:
    out_dir = Path(md_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    img_dir = out_dir / "img"

    # Only the curated set (same files `--list` shows: a non-empty `widget_display`
    # — i.e. files carrying a public widget / re-exported type). Internal helpers
    # and impl-split modules carry none, so they get no catalog page.
    target = [fp for fp in reg.files if reg.widget_display.get(fp)]
    parsed = [
        merge_submodule_items(
            reg, parse_file(fp, fp.stem, reg.cfg_by_file.get(fp.resolve(), []))
        )
        for fp in target
    ]
    slugs = _build_slugs(parsed)

    for pf in parsed:
        slug = slugs[pf.file_path]
        page = format_catalog_markdown(
            pf,
            title=_catalog_title(reg, pf),
            slug=slug,
            api_base=api_base,
            img_dir=img_dir,
            api_dir=api_dir,
        )
        (out_dir / f"{slug}.md").write_text(page, encoding="utf-8")
    (out_dir / "index.md").write_text(
        format_catalog_index(reg, parsed, slugs), encoding="utf-8"
    )

    book_src = REPO_ROOT / "docs"
    try:
        md_subdir = out_dir.resolve().relative_to(book_src.resolve()).as_posix()
    except ValueError:
        md_subdir = out_dir.name
    block = _summary_block(reg, parsed, slugs, md_subdir)
    summary = book_src / "SUMMARY.md"
    if summary.exists() and patch_summary(summary, block, SPEC.marker_begin, SPEC.marker_end):
        note = "patched docs/SUMMARY.md"
    else:
        note = (
            f"SUMMARY markers not found — add this region to docs/SUMMARY.md:\n"
            f"{SPEC.marker_begin}\n{SPEC.marker_end}"
        )
    print(
        f"Wrote {len(parsed)} catalog pages + index.md to {out_dir} ({note})",
        file=sys.stderr,
    )
    return 0


def run_self_tests() -> int:
    """Smoke tests for the catalog generator (`--test`). stdlib-only, runs
    against the live source tree."""
    import tempfile

    reg = build_registry()
    fp = reg.module_to_file["button"]
    pf = parse_file(fp, fp.stem, reg.cfg_by_file.get(fp.resolve(), []))

    page = format_catalog_markdown(
        pf, title="Button", slug="button", api_base="../api", img_dir=Path("/nonexistent")
    )
    assert page.startswith("<!-- SPDX-License-Identifier"), page[:40]
    assert "\n# Button\n" in page, page[:80]
    assert "## API reference" in page, "missing API reference section"
    assert "Full rustdoc API" in page, "missing rustdoc deep link"
    assert "teksilo_widgets/button/index.html" in page, "wrong rustdoc url"

    slugs = _build_slugs([pf])
    idx = format_catalog_index(reg, [pf], slugs)
    assert "\n# Widget Catalog\n" in idx, idx[:80]
    assert "[Button](button.md)" in idx, "index missing button link"

    # SUMMARY marker round-trip.
    with tempfile.TemporaryDirectory() as td:
        sp = Path(td) / "SUMMARY.md"
        sp.write_text(
            f"# Index\n\n{SUMMARY_BEGIN}\nstale\n{SUMMARY_END}\n\n# Tail\n",
            encoding="utf-8",
        )
        block = _summary_block(reg, [pf], slugs, "widgets")
        assert patch_summary(sp, block, SUMMARY_BEGIN, SUMMARY_END), "patch returned False"
        result = sp.read_text(encoding="utf-8")
        assert "stale" not in result, "stale content survived"
        assert "[Overview](widgets/index.md)" in result, "catalog overview missing"
        assert "[Button](widgets/button.md)" in result, "widget chapter missing"
        assert "# Index" in result and "# Tail" in result, "surrounding text clobbered"

    # Slug collision fallback.
    assert _rustdoc_module_url("../api", PRIMITIVES_DIR / "hstack.rs").endswith(
        "teksilo_widgets/primitives/hstack/index.html"
    ), "rustdoc url for nested module wrong"
    # Nested per-widget submodule -> public parent module.
    assert _rustdoc_module_url("../api", WIDGETS_SRC / "tab_widget" / "bar.rs").endswith(
        "teksilo_widgets/tab_widget/index.html"
    ), "rustdoc url for private submodule should fall back to parent"

    # Catalog link cleaning: keep web/anchor/api, strip rustdoc + file links to code.
    nz = _clean_catalog_links(
        "[`HStack`](crate::primitives::HStack), [a](Self::alignment), [b](self), "
        "[c](TreeView), [d](../crates/x.rs), [api](../api/x.html), [web](https://x.io)\n"
        "see [`Ref`].\n\n[`Ref`]: crate::Ref"
    )
    for bad in ("](crate::", "](Self::", "](self)", "](TreeView)", "](../crates/", "[`Ref`]:"):
        assert bad not in nz, f"{bad} survived: {nz}"
    assert "`HStack`" in nz and "see `Ref`." in nz, nz
    assert "](../api/x.html)" in nz and "](https://x.io)" in nz, nz

    # Non-widget crate generalization (teksilo-data): re-exported types become
    # catalog entries, rustdoc links target the crate's own rustdoc dir.
    global SPEC
    _prev = SPEC
    try:
        SPEC = CRATE_SPECS["data"]
        dreg = build_registry()
        dfp = dreg.module_to_file["list_model"]
        assert dreg.widget_display.get(dfp), "list_model.rs should be a catalog entry"
        dpf = parse_file(dfp, dfp.stem, dreg.cfg_by_file.get(dfp.resolve(), []))
        dpage = format_catalog_markdown(
            dpf, title="ListModel", slug="list_model", api_base="../api",
            img_dir=Path("/nonexistent"),
        )
        assert "teksilo_data/list_model/index.html" in dpage, dpage[:400]
        didx = format_catalog_index(dreg, [dpf], _build_slugs([dpf]))
        assert "\n# Data Collections\n" in didx, didx[:80]
    finally:
        SPEC = _prev

    print("extract_widget_api.py self-tests passed.", file=sys.stderr)
    return 0


# ----------------------------------------------------------------------------
# CLI
# ----------------------------------------------------------------------------


def cmd_list(reg: Registry) -> int:
    # Show exported type names grouped by file.
    rows: list[tuple[str, str]] = []
    for fp, names in sorted(reg.widget_display.items(), key=lambda kv: kv[0].name):
        if not names:
            continue
        rel = fp.relative_to(REPO_ROOT) if REPO_ROOT in fp.parents else fp
        cfg = reg.cfg_by_file.get(fp.resolve(), [])
        cfg_s = f"  {{{_fmt_cfg(cfg)}}}" if cfg else ""
        rows.append((fp.stem, f"  {', '.join(names)}  ({rel}){cfg_s}"))

    if not rows:
        print("No catalog files found.", file=sys.stderr)
        return 1

    print(f"{len(rows)} files under {SPEC.src.relative_to(REPO_ROOT)}:\n")
    for stem, body in sorted(rows):
        print(f"{stem}:")
        print(body)
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Extract public API + docs from teksilo-widgets source files."
        ),
    )
    parser.add_argument(
        "widgets",
        nargs="*",
        help="Widget names (type or module name, case-insensitive). "
        "e.g. Button HStack Dialog",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="Extract every widget file.",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="List available widgets and exit.",
    )
    parser.add_argument(
        "--format",
        "-f",
        choices=("md", "text", "json"),
        default="md",
        help="Output format (default: md).",
    )
    parser.add_argument(
        "--output",
        "-o",
        help="Write output to this file instead of stdout.",
    )
    parser.add_argument(
        "--md-dir",
        metavar="DIR",
        help="Generate one mdBook catalog page per widget into DIR "
        "(e.g. docs/widgets), plus index.md, and patch the generated region of "
        "docs/SUMMARY.md. Ignores positional widget names (always emits all).",
    )
    parser.add_argument(
        "--api-base",
        default="../api",
        help="Base URL/path for the rustdoc API links in catalog pages "
        "(default: ../api, i.e. rustdoc published next to the book).",
    )
    parser.add_argument(
        "--api-dir",
        metavar="DIR",
        help="Path to the built rustdoc tree (e.g. target/doc). When given, each "
        "catalog page's API link falls back to the nearest module that actually "
        "has a page, so private/cfg-gated modules don't 404.",
    )
    parser.add_argument(
        "--crate",
        choices=list(CRATE_SPECS),
        default="widgets",
        help="Which crate to extract / catalog (default: widgets).",
    )
    parser.add_argument(
        "--catalog-all",
        action="store_true",
        help="Generate the mdBook catalog for ALL crates into their default "
        "docs/<dir> and patch each SUMMARY region.",
    )
    parser.add_argument(
        "--test",
        action="store_true",
        help=argparse.SUPPRESS,  # run the catalog-generator smoke tests and exit
    )
    args = parser.parse_args(argv)

    if args.test:
        return run_self_tests()

    global SPEC

    def _api_dir() -> "Path | None":
        # Auto-use the built rustdoc tree if present, so a plain run still
        # resolves deep-links for private / cfg-gated modules instead of 404ing.
        if args.api_dir:
            return Path(args.api_dir)
        _doc = REPO_ROOT / "target" / "doc"
        return _doc if (_doc / SPEC.rustdoc).exists() else None

    if args.catalog_all:
        rc = 0
        for key, spec in CRATE_SPECS.items():
            SPEC = spec
            reg = build_registry()
            out = REPO_ROOT / "docs" / spec.md_subdir
            rc |= cmd_md_dir(reg, str(out), args.api_base, _api_dir())
        return rc

    SPEC = CRATE_SPECS[args.crate]
    reg = build_registry()

    if args.list:
        return cmd_list(reg)

    if args.md_dir:
        return cmd_md_dir(reg, args.md_dir, args.api_base, _api_dir())

    if args.all:
        target_files = list(reg.files)
    elif args.widgets:
        target_files = []
        seen: set[Path] = set()
        for name in args.widgets:
            fp = resolve_name(reg, name)
            if fp is None:
                known = sorted(set(reg.type_to_file) | set(reg.module_to_file))
                hints = difflib.get_close_matches(name.lower(), known, n=3)
                hint_str = (
                    f" Did you mean: {', '.join(hints)}?" if hints else ""
                )
                print(
                    f"error: unknown widget '{name}'.{hint_str}", file=sys.stderr
                )
                return 2
            if fp not in seen:
                seen.add(fp)
                target_files.append(fp)
    else:
        parser.print_help(sys.stderr)
        print(
            "\nPass one or more widget names, --all, or --list.",
            file=sys.stderr,
        )
        return 2

    parsed: list[ParsedFile] = []
    for fp in target_files:
        cfg = reg.cfg_by_file.get(fp.resolve(), [])
        pf = parse_file(fp, fp.stem, cfg)
        parsed.append(pf)

    if args.format == "json":
        rendered = format_json(parsed) + "\n"
    elif args.format == "text":
        rendered = "\n".join(format_text(pf) for pf in parsed)
    else:
        rendered = "\n---\n\n".join(format_markdown(pf) for pf in parsed)

    if args.output:
        Path(args.output).write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
