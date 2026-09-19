#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Generate `teksilo_probe/tools.py` from the Rust sources that define the tools.

Two files, two halves of the answer, and both are in this repository:

* `crates/teksilo-automation/src/mcp_schema.rs` — `TOOL_CATALOG`, the canonical
  list of *which* tools exist, their one-line descriptions and whether they
  mutate. It is already conformance-tested against the MCP router, so a tool
  that is registered but missing from the catalog cannot happen.
* `crates/teksilo-automation-mcp/src/server.rs` — one `#[tool]` handler per
  entry, each taking a `Parameters<FooParams>` whose fields **are** the tool's
  argument names.

Deriving the argument names rather than transcribing them is the whole point.
Every parameter struct carries `#[serde(deny_unknown_fields)]`, so a misspelt
argument is refused by the bridge — but a *valid name used for the wrong thing*
is not. The prototype harness this library generalises passed `kind="click"` to
`inject_pointer` in a dozen probes; `kind` is a real field (mouse / touch / pen)
and `action` is the one that means click, so those calls were quietly asking for
an unknown pointer kind and working only because `action` defaults to `click`.
A generated surface makes that unrepresentable.

The generated module is committed. `crates/teksilo-automation/src/mcp_schema.rs`
carries a test that fails when it drifts from `TOOL_CATALOG`, naming this script.

Usage:

    python3 generate_tools.py                 # write teksilo_probe/tools.py
    python3 generate_tools.py --check         # exit 1 if it would change
    python3 generate_tools.py --stdout        # print instead of writing
"""

from __future__ import annotations

import argparse
import re
import sys
import textwrap
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_OUT = HERE / "teksilo_probe" / "tools.py"

#: Where the two Rust sources live, relative to the repository root.
SCHEMA_REL = Path("crates/teksilo-automation/src/mcp_schema.rs")
SERVER_REL = Path("crates/teksilo-automation-mcp/src/server.rs")


# ---------------------------------------------------------------------------
# Locating the Rust sources
# ---------------------------------------------------------------------------


def repo_root(start: Path | None = None) -> Path:
    """Walk up from `start` until both Rust sources are visible.

    Derived from `__file__` rather than the working directory: the generator is
    run from the repository root, from `embedded/probe/`, and from a build
    script's arbitrary cwd, and all three must resolve the same two files.
    """
    here = (start or HERE).resolve()
    for candidate in (here, *here.parents):
        if (candidate / SCHEMA_REL).is_file() and (candidate / SERVER_REL).is_file():
            return candidate
    raise SystemExit(
        f"cannot find {SCHEMA_REL} and {SERVER_REL} above {here}.\n"
        "Run this from a teksilo checkout, or pass --schema / --server."
    )


# ---------------------------------------------------------------------------
# Parsing `mcp_schema.rs`
# ---------------------------------------------------------------------------

_ENTRY_RE = re.compile(
    r"ToolDescriptor\s*\{\s*"
    r"name:\s*(?P<name>\"(?:[^\"\\]|\\.)*\")\s*,\s*"
    r"description:\s*(?P<desc>\"(?:[^\"\\]|\\.)*\")\s*,\s*"
    r"mutating:\s*(?P<mut>true|false)\s*,?\s*\}",
    re.S,
)


def rust_string(literal: str) -> str:
    """Decode a Rust `"..."` literal — the escapes that appear in the catalog."""
    body = literal[1:-1]
    out: list[str] = []
    i = 0
    while i < len(body):
        ch = body[i]
        if ch == "\\" and i + 1 < len(body):
            nxt = body[i + 1]
            out.append({"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(nxt, nxt))
            i += 2
            continue
        out.append(ch)
        i += 1
    return "".join(out)


class Tool:
    """One catalog entry, joined to its parameter struct."""

    def __init__(self, name: str, description: str, mutating: bool) -> None:
        self.name = name
        self.description = description
        self.mutating = mutating
        self.params: list[Param] = []

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"Tool({self.name!r}, params={[p.name for p in self.params]})"


def parse_catalog(text: str) -> list[Tool]:
    """Every `ToolDescriptor` in `TOOL_CATALOG`, in declaration order."""
    body = text.split("pub const TOOL_CATALOG", 1)
    if len(body) != 2:
        raise SystemExit("no `pub const TOOL_CATALOG` in the schema source")
    tools = [
        Tool(rust_string(m.group("name")), rust_string(m.group("desc")), m.group("mut") == "true")
        for m in _ENTRY_RE.finditer(body[1])
    ]
    if not tools:
        raise SystemExit("`TOOL_CATALOG` parsed to zero tools — the shape changed")
    return tools


# ---------------------------------------------------------------------------
# Parsing `server.rs`
# ---------------------------------------------------------------------------

_STRUCT_RE = re.compile(r"pub struct (?P<name>\w+)\s*\{(?P<body>[^}]*)\}", re.S)
_FIELD_RE = re.compile(r"^\s*pub (?P<name>\w+):\s*(?P<ty>[^,]+),\s*$", re.M)
_HANDLER_RE = re.compile(
    r"async fn (?P<fn>\w+)\(\s*&self,\s*Parameters\(\w+\):\s*Parameters<(?P<params>\w+)>",
    re.S,
)

#: Rust type -> (python annotation, python default expression).
_SCALARS = {
    "u64": "int",
    "u32": "int",
    "usize": "int",
    "i64": "int",
    "f32": "float",
    "f64": "float",
    "bool": "bool",
    "String": "str",
}


class Param:
    def __init__(self, name: str, rust_ty: str) -> None:
        self.name = name
        rust_ty = rust_ty.strip()
        self.optional = rust_ty.startswith("Option<")
        inner = rust_ty[len("Option<") : -1].strip() if self.optional else rust_ty
        self.rust_inner = inner
        self.annotation = _annotate(inner)

    def signature(self) -> str:
        if self.optional:
            return f"{self.name}: {self.annotation} | None = None"
        return f"{self.name}: {self.annotation}"


def _annotate(inner: str) -> str:
    if inner in _SCALARS:
        return _SCALARS[inner]
    if inner == "SettleArg":
        # A settle policy is a plain mapping on the wire:
        # {clock_millis, max_anim_frames, layout_after, settle_timeout_ms}.
        return "Mapping[str, Any]"
    if inner.startswith("Vec<"):
        return "Sequence[Mapping[str, Any]]"
    if inner.startswith("["):
        return "Sequence[float]"
    return "Any"


def parse_server(text: str) -> tuple[dict[str, str], dict[str, list[Param]]]:
    """`(tool name -> params struct, params struct -> fields)`."""
    structs: dict[str, list[Param]] = {}
    for m in _STRUCT_RE.finditer(text):
        fields = [
            Param(f.group("name"), f.group("ty"))
            for f in _FIELD_RE.finditer(m.group("body"))
        ]
        structs[m.group("name")] = fields
    handlers = {m.group("fn"): m.group("params") for m in _HANDLER_RE.finditer(text)}
    return handlers, structs


# ---------------------------------------------------------------------------
# Emitting
# ---------------------------------------------------------------------------

HEADER = '''\
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Typed wrappers — one per automation tool. **Generated; do not edit.**

Regenerate with `python3 generate_tools.py` from a teksilo checkout. The names,
descriptions and `mutating` flags come from `TOOL_CATALOG`
(`crates/teksilo-automation/src/mcp_schema.rs`); the argument names and types
come from the `#[tool]` handlers' parameter structs
(`crates/teksilo-automation-mcp/src/server.rs`).

Every parameter struct on the Rust side carries `#[serde(deny_unknown_fields)]`,
so a misspelt argument is refused rather than ignored — but a *valid name used
for the wrong thing* is not. `inject_pointer` has both a `kind` (mouse / touch /
pen) and an `action` (click / double_click / down / up / move), and a harness
that passed `kind="click"` was quietly asking for an unknown pointer kind and
working only because `action` defaults to `click`. Calling these wrappers makes
that mistake unrepresentable.

`None` arguments are dropped before the call, so an omitted argument is genuinely
omitted and the Rust side's own default applies.

Each function takes the `Session` first and returns the tool's payload:

    from teksilo_probe import tools
    tree = tools.snapshot_tree(session)
    tools.inject_key(session, key="Enter", command=True)
"""

from __future__ import annotations

from typing import Any, Mapping, Sequence

__all__ = [
'''

FOOTER_HEAD = '''

#: Every tool name, in `TOOL_CATALOG` order. The Rust conformance test in
#: `crates/teksilo-automation/src/mcp_schema.rs` compares this against the
#: catalog, so a tool added there without regenerating this file fails the build.
TOOL_NAMES = (
'''

FOOTER_TAIL = ''')

#: The tools that mutate UI state (and so accept a `settle` policy).
MUTATING = frozenset(
    name
    for name, mutating in (
'''


def render(tools: list[Tool]) -> str:
    out = [HEADER]
    for tool in tools:
        out.append(f'    "{tool.name}",\n')
    out.append('    "TOOL_NAMES",\n    "MUTATING",\n]\n')

    for tool in tools:
        out.append("\n\n")
        out.append(render_tool(tool))

    out.append(FOOTER_HEAD)
    for tool in tools:
        out.append(f'    "{tool.name}",\n')
    out.append(FOOTER_TAIL)
    for tool in tools:
        out.append(f'        ("{tool.name}", {tool.mutating}),\n')
    out.append("    )\n    if mutating\n)\n")
    return "".join(out)


def render_tool(tool: Tool) -> str:
    required = [p for p in tool.params if not p.optional]
    optional = [p for p in tool.params if p.optional]

    parts = ["session"] + [p.signature() for p in required]
    if optional:
        parts.append("*")
        parts += [p.signature() for p in optional]
    signature = f"def {tool.name}({', '.join(parts)}) -> Any:"
    if len(signature) > 88:
        joined = ",\n    ".join(parts)
        signature = f"def {tool.name}(\n    {joined},\n) -> Any:"

    wrapped = textwrap.wrap(tool.description, width=74) or [tool.name]
    doc_body = "\n    ".join(wrapped)
    doc = f'    """{doc_body}\n\n'
    doc += "    Mutating" if tool.mutating else "    Read-only"
    doc += " tool.\n    \"\"\"\n"

    lines = [signature, "\n", doc]
    if tool.params:
        lines.append("    args = {\n")
        for p in tool.params:
            lines.append(f'        "{p.name}": {p.name},\n')
        lines.append("    }\n")
        lines.append('    return session.call("%s", **_present(args))\n' % tool.name)
    else:
        lines.append('    return session.call("%s")\n' % tool.name)
    return "".join(lines)


PRESENT_HELPER = '''

def _present(args: Mapping[str, Any]) -> dict:
    """Drop the arguments the caller left out.

    Not a cosmetic filter: the Rust parameter structs use `deny_unknown_fields`
    and every optional field is an `Option`, so sending an explicit `null` for
    one is different from omitting it — and only omission lets the Rust side's
    own default apply.
    """
    return {k: v for k, v in args.items() if v is not None}'''.rstrip("\n")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--schema", type=Path, help="path to mcp_schema.rs")
    ap.add_argument("--server", type=Path, help="path to server.rs")
    ap.add_argument("-o", "--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument("--check", action="store_true", help="exit 1 if out of date")
    ap.add_argument("--stdout", action="store_true", help="print instead of writing")
    args = ap.parse_args(argv)

    if args.schema and args.server:
        schema_path, server_path = args.schema, args.server
    else:
        root = repo_root()
        schema_path = args.schema or root / SCHEMA_REL
        server_path = args.server or root / SERVER_REL

    tools = parse_catalog(schema_path.read_text(encoding="utf-8"))
    handlers, structs = parse_server(server_path.read_text(encoding="utf-8"))

    missing = []
    for tool in tools:
        struct = handlers.get(tool.name)
        if struct is None:
            missing.append(tool.name)
            continue
        tool.params = list(structs.get(struct, []))
    if missing:
        raise SystemExit(
            "no `#[tool]` handler found in server.rs for: " + ", ".join(missing) +
            "\nThe catalog and the router are conformance-tested against each "
            "other, so this means the handler signature shape changed and this "
            "generator's regex needs updating."
        )

    text = render(tools)
    # The helper the generated calls use goes just under the imports, before the
    # first tool, so the module reads top-down.
    marker = "\n\n\ndef " + tools[0].name
    text = text.replace(marker, PRESENT_HELPER + marker, 1)

    if args.stdout:
        sys.stdout.write(text)
        return 0
    if args.check:
        current = args.out.read_text(encoding="utf-8") if args.out.exists() else ""
        if current != text:
            print(f"{args.out} is out of date; run `python3 {Path(__file__).name}`")
            return 1
        print(f"{args.out} is up to date ({len(tools)} tools)")
        return 0

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8")
    print(f"wrote {args.out} — {len(tools)} tools, "
          f"{sum(len(t.params) for t in tools)} parameters")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
