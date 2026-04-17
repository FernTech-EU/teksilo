#!/usr/bin/env python3
"""
Extract public API and inline documentation from fern-widgets source files.

Walks `crates/fern-widgets/src/` (and `src/primitives/`), looks up the widget
file for each requested name, and emits:

  - The file's `//!` module header doc
  - Every `pub struct` / `pub enum` / `pub type` / `pub const` with its `///` doc
  - Every `pub fn` inside inherent `impl Foo { ... }` blocks with its `///` doc
  - Enum variants with their own `///` docs

Trait impls like `impl Widget for Foo` are skipped — they are internal plumbing,
not part of the widget's builder API.

Usage:
    python tools/extract_widget_api.py Button HStack
    python tools/extract_widget_api.py --all
    python tools/extract_widget_api.py --list
    python tools/extract_widget_api.py Button --format json
    python tools/extract_widget_api.py Button -o out.md
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
WIDGETS_SRC = REPO_ROOT / "crates" / "fern-widgets" / "src"
PRIMITIVES_DIR = WIDGETS_SRC / "primitives"

# Aggregator files we never treat as "a widget".
SKIP_FILES = {"lib.rs", "primitives.rs", "layout_integration_tests.rs", "mod.rs"}


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
    fern-widgets code does not use them.
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

    # --- Module header: //! lines at the very top (possibly after attrs).
    header_lines: list[str] = []
    i = 0
    while i < n:
        stripped = raw[i].strip()
        if not stripped:
            if header_lines:
                header_lines.append("")
            i += 1
            continue
        m = DOC_INNER.match(raw[i])
        if m:
            header_lines.append(_doc_text(m))
            i += 1
            continue
        # Stop at first non-//! non-blank line — module header is always the
        # very first thing in a widget file in this codebase.
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
    type_display: dict[Path, list[str]]  # file -> original-cased type names


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
    if not WIDGETS_SRC.exists():
        raise SystemExit(f"fern-widgets src not found at {WIDGETS_SRC}")

    files: list[Path] = []
    for p in sorted(WIDGETS_SRC.glob("*.rs")):
        if p.name in SKIP_FILES:
            continue
        files.append(p)
    if PRIMITIVES_DIR.exists():
        for p in sorted(PRIMITIVES_DIR.glob("*.rs")):
            if p.name in SKIP_FILES:
                continue
            files.append(p)

    cfg_by_file: dict[Path, list[str]] = {}
    cfg_by_file.update(
        _collect_cfgs_from_aggregator(WIDGETS_SRC / "lib.rs", WIDGETS_SRC)
    )
    cfg_by_file.update(
        _collect_cfgs_from_aggregator(
            WIDGETS_SRC / "primitives.rs", PRIMITIVES_DIR
        )
    )

    type_to_file: dict[str, Path] = {}
    module_to_file: dict[str, Path] = {}
    type_display: dict[Path, list[str]] = {}
    type_re = re.compile(r"^\s*pub\s+(?:struct|enum|type)\s+([A-Za-z_]\w*)", re.MULTILINE)

    for fp in files:
        module_to_file[fp.stem.lower()] = fp
        text = fp.read_text(encoding="utf-8")
        names = [m.group(1) for m in type_re.finditer(text)]
        type_display[fp] = names
        for name in names:
            type_to_file.setdefault(name.lower(), fp)

    return Registry(
        files=files,
        cfg_by_file=cfg_by_file,
        type_to_file=type_to_file,
        module_to_file=module_to_file,
        type_display=type_display,
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

    for it in pf.items:
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

    return "\n".join(out).rstrip() + "\n"


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
        out.append(f"#### `{m.signature.strip()}`{flags_str}")
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
# CLI
# ----------------------------------------------------------------------------


def cmd_list(reg: Registry) -> int:
    # Show exported type names grouped by file.
    rows: list[tuple[str, str]] = []
    for fp, names in sorted(reg.type_display.items(), key=lambda kv: kv[0].name):
        if not names:
            continue
        rel = fp.relative_to(REPO_ROOT) if REPO_ROOT in fp.parents else fp
        cfg = reg.cfg_by_file.get(fp.resolve(), [])
        cfg_s = f"  {{{_fmt_cfg(cfg)}}}" if cfg else ""
        rows.append((fp.stem, f"  {', '.join(names)}  ({rel}){cfg_s}"))

    if not rows:
        print("No widget files found.", file=sys.stderr)
        return 1

    print(f"{len(rows)} widget files under {WIDGETS_SRC.relative_to(REPO_ROOT)}:\n")
    for stem, body in sorted(rows):
        print(f"{stem}:")
        print(body)
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Extract public API + docs from fern-widgets source files."
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
    args = parser.parse_args(argv)

    reg = build_registry()

    if args.list:
        return cmd_list(reg)

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
