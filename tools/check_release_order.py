#!/usr/bin/env python3
"""Analyze inter-crate dependencies across the Bastyde workspace.

Scans every crate's ``Cargo.toml``, extracts dependencies on *other*
``bastyde-*`` crates (split into normal/build vs dev), then:

  * flags **release-blocking cycles** — cycles in the normal + build
    dependency graph that make a clean ``cargo publish`` impossible.
    cargo can publish a crate only once every one of its normal/build
    dependencies is already on the registry, so a normal-dep cycle
    cannot be released in any order.
  * reports **dev-dependency back-edges** separately — a dev-dependency
    that points "backwards" (at a crate that already depends on this one
    through normal deps). cargo *allows* these: dev-deps are stripped
    from the package a downstream crate consumes, so they never
    participate in the publish-time resolution. They're listed for
    awareness, not as blockers.
  * prints a valid **release order** (topological sort, dependencies
    first), grouped into "waves" — every crate in a wave has all of its
    dependencies satisfied by earlier waves, so a wave can be published
    in any order / in parallel.

Pure standard library; no ``tomllib`` needed (works on Python 3.9). The
TOML reading is intentionally minimal — it understands the dependency
forms this workspace actually uses:

    dep = { workspace = true }
    dep = "1.2"
    dep.workspace = true
    dep = { package = "bastyde-real-name", ... }   # rename
    [target.'cfg(...)'.dependencies] / [build-dependencies] / [dev-dependencies]

Usage:
    python3 tools/check_release_order.py [--root DIR] [--json]
                                         [--include-examples]

Exit status is non-zero when a release-blocking cycle is found, so the
script can gate CI.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass, field

INTERNAL_PREFIX = "bastyde-"

# A dependency line's left-hand side: `name`, `name.workspace`, `name.path`, ...
# We only need the bare crate name (everything before the first '.' or '=').
_DEP_KEY_RE = re.compile(r"^\s*([A-Za-z0-9_.-]+?)\s*(?:=|\.[A-Za-z0-9_-]+\s*=)")
_PACKAGE_RENAME_RE = re.compile(r'package\s*=\s*"([^"]+)"')
_PACKAGE_NAME_RE = re.compile(r'^\s*name\s*=\s*"([^"]+)"')
_PUBLISH_RE = re.compile(r"^\s*publish\s*=\s*(true|false)\b")
_PUBLISH_WS_RE = re.compile(r"^\s*publish\.workspace\s*=\s*true")
_SECTION_RE = re.compile(r"^\s*\[([^\]]+)\]\s*$")


@dataclass
class Crate:
    name: str
    path: str  # directory containing Cargo.toml, relative to root
    publish: bool  # True if this crate is publishable (publish != false)
    normal_deps: set[str] = field(default_factory=set)  # incl. build-deps
    dev_deps: set[str] = field(default_factory=set)


def _classify_section(header: str) -> str | None:
    """Map a TOML section header to 'normal', 'dev', or None.

    Handles plain and target-specific forms:
        [dependencies] / [target.'cfg(...)'.dependencies]      -> normal
        [build-dependencies] / [target...build-dependencies]   -> normal
        [dev-dependencies] / [target...dev-dependencies]       -> dev
    """
    h = header.strip()
    # Order matters: "dev-dependencies" and "build-dependencies" both end
    # in "dependencies", so test the more specific suffixes first.
    if h == "dev-dependencies" or h.endswith(".dev-dependencies"):
        return "dev"
    if h == "build-dependencies" or h.endswith(".build-dependencies"):
        return "normal"
    if h == "dependencies" or h.endswith(".dependencies"):
        return "normal"
    return None


def _strip_comment(line: str) -> str:
    """Drop a trailing TOML comment. Naive but sufficient here: we never
    have '#' inside the dependency keys/values this workspace uses."""
    in_str = False
    for i, ch in enumerate(line):
        if ch == '"':
            in_str = not in_str
        elif ch == "#" and not in_str:
            return line[:i]
    return line


def parse_cargo_toml(text: str) -> tuple[str | None, bool, set[str], set[str]]:
    """Return (package_name, publishable, normal_internal_deps, dev_internal_deps).

    package_name is None for a virtual manifest (the workspace root).
    """
    name: str | None = None
    publish = True  # absent `publish` means publishable
    publish_seen = False
    normal: set[str] = set()
    dev: set[str] = set()

    current = None  # 'package', 'normal', 'dev', or None
    for raw in text.splitlines():
        line = _strip_comment(raw)
        m = _SECTION_RE.match(line)
        if m:
            header = m.group(1).strip()
            if header == "package":
                current = "package"
            else:
                current = _classify_section(header)
            continue

        if current == "package":
            nm = _PACKAGE_NAME_RE.match(line)
            if nm:
                name = nm.group(1)
            elif _PUBLISH_WS_RE.match(line):
                # `publish.workspace = true` inherits the workspace value,
                # which in this repo is `false`. Treat as non-publishable
                # unless a later explicit `publish = true` overrides.
                if not publish_seen:
                    publish = False
            else:
                pm = _PUBLISH_RE.match(line)
                if pm:
                    publish = pm.group(1) == "true"
                    publish_seen = True
            continue

        if current in ("normal", "dev"):
            km = _DEP_KEY_RE.match(line)
            if not km:
                continue
            key = km.group(1).split(".")[0]
            rename = _PACKAGE_RENAME_RE.search(line)
            dep_name = rename.group(1) if rename else key
            if dep_name.startswith(INTERNAL_PREFIX):
                (normal if current == "normal" else dev).add(dep_name)

    return name, publish, normal, dev


def discover_crates(root: str, include_examples: bool) -> dict[str, Crate]:
    """Scan the workspace for member crates and parse their manifests."""
    crates: dict[str, Crate] = {}
    skip_dirs = {".git", "target", "node_modules", ".idea", ".vscode"}
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in skip_dirs]
        if "Cargo.toml" not in filenames:
            continue
        manifest = os.path.join(dirpath, "Cargo.toml")
        rel = os.path.relpath(dirpath, root)
        try:
            with open(manifest, encoding="utf-8") as fh:
                text = fh.read()
        except OSError as exc:
            print(f"warning: cannot read {manifest}: {exc}", file=sys.stderr)
            continue
        name, publish, normal, dev = parse_cargo_toml(text)
        if name is None:
            continue  # virtual manifest (workspace root)
        is_example = rel.startswith("examples" + os.sep) or rel == "examples"
        if is_example and not include_examples:
            # Still useful to know examples exist, but they are leaf
            # consumers (nothing depends on them) and are not published,
            # so they never affect cycles or release order.
            continue
        crates[name] = Crate(name=name, path=rel, publish=publish,
                              normal_deps=normal, dev_deps=dev)
    return crates


def tarjan_sccs(nodes: list[str], edges: dict[str, set[str]]) -> list[list[str]]:
    """Tarjan's strongly-connected-components. Returns SCCs (each a list of
    node names). An SCC with >1 member, or a single node with a self-edge,
    is a cycle."""
    index_of: dict[str, int] = {}
    low: dict[str, int] = {}
    on_stack: set[str] = set()
    stack: list[str] = []
    counter = [0]
    out: list[list[str]] = []

    import sys as _sys
    # Iterative Tarjan to avoid recursion limits on large graphs.
    for root in nodes:
        if root in index_of:
            continue
        work = [(root, iter(sorted(edges.get(root, ()))))]
        index_of[root] = low[root] = counter[0]
        counter[0] += 1
        stack.append(root)
        on_stack.add(root)
        while work:
            node, it = work[-1]
            advanced = False
            for succ in it:
                if succ not in edges and succ not in index_of:
                    # Edge to a node outside our node set; skip.
                    if succ not in index_of:
                        continue
                if succ not in index_of:
                    index_of[succ] = low[succ] = counter[0]
                    counter[0] += 1
                    stack.append(succ)
                    on_stack.add(succ)
                    work.append((succ, iter(sorted(edges.get(succ, ())))))
                    advanced = True
                    break
                elif succ in on_stack:
                    low[node] = min(low[node], index_of[succ])
            if advanced:
                continue
            work.pop()
            if work:
                parent = work[-1][0]
                low[parent] = min(low[parent], low[node])
            if low[node] == index_of[node]:
                comp = []
                while True:
                    w = stack.pop()
                    on_stack.discard(w)
                    comp.append(w)
                    if w == node:
                        break
                out.append(comp)
    return out


def reachable(start: str, edges: dict[str, set[str]], universe: set[str]) -> set[str]:
    """All nodes reachable from `start` along `edges` (within `universe`)."""
    seen: set[str] = set()
    stack = [start]
    while stack:
        n = stack.pop()
        for m in edges.get(n, ()):
            if m in universe and m not in seen:
                seen.add(m)
                stack.append(m)
    return seen


def release_waves(nodes: list[str], deps: dict[str, set[str]]) -> tuple[list[list[str]], list[str]]:
    """Kahn-style layered topological sort (dependencies first).

    Returns (waves, leftover). `leftover` is non-empty only when a cycle
    prevents a full ordering."""
    universe = set(nodes)
    remaining = {n: {d for d in deps.get(n, ()) if d in universe} for n in nodes}
    waves: list[list[str]] = []
    placed: set[str] = set()
    while True:
        ready = sorted(n for n, ds in remaining.items()
                       if n not in placed and ds <= placed)
        if not ready:
            break
        waves.append(ready)
        placed.update(ready)
    leftover = sorted(n for n in nodes if n not in placed)
    return waves, leftover


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", default=None,
                    help="workspace root (default: parent of this script's dir)")
    ap.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    ap.add_argument("--include-examples", action="store_true",
                    help="include examples/* crates in the graph (default: skip)")
    args = ap.parse_args()

    if args.root:
        root = os.path.abspath(args.root)
    else:
        root = os.path.abspath(os.path.join(os.path.dirname(__file__), os.pardir))

    crates = discover_crates(root, args.include_examples)
    if not crates:
        print(f"error: no bastyde-* crates found under {root}", file=sys.stderr)
        return 2

    names = sorted(crates)
    name_set = set(names)

    # Restrict every edge to the discovered universe (drops external path
    # deps and any crate excluded from the workspace).
    normal_edges = {n: {d for d in crates[n].normal_deps if d in name_set} for n in names}
    dev_edges = {n: {d for d in crates[n].dev_deps if d in name_set} for n in names}

    # --- Release-blocking cycles: SCCs in the normal+build graph ---
    sccs = tarjan_sccs(names, normal_edges)
    blocking_cycles: list[list[str]] = []
    for comp in sccs:
        if len(comp) > 1:
            blocking_cycles.append(sorted(comp))
        elif len(comp) == 1 and comp[0] in normal_edges.get(comp[0], set()):
            blocking_cycles.append(comp)  # self-dependency

    # --- Dev-dependency back-edges (allowed, informational) ---
    # A dev edge A->B is a "back-edge" when B can already reach A through
    # normal deps (so it would close a cycle if it were a normal dep).
    dev_back_edges: list[tuple[str, str]] = []
    for a in names:
        for b in sorted(dev_edges.get(a, ())):
            if a in reachable(b, normal_edges, name_set):
                dev_back_edges.append((a, b))

    # --- Release order over the publishable normal graph ---
    waves, leftover = release_waves(names, normal_edges)

    if args.json:
        payload = {
            "root": root,
            "crates": {
                n: {
                    "path": crates[n].path,
                    "publish": crates[n].publish,
                    "normal_deps": sorted(normal_edges[n]),
                    "dev_deps": sorted(dev_edges[n]),
                }
                for n in names
            },
            "blocking_cycles": blocking_cycles,
            "dev_back_edges": dev_back_edges,
            "release_waves": waves,
            "unordered_due_to_cycle": leftover,
        }
        print(json.dumps(payload, indent=2))
        return 1 if blocking_cycles else 0

    # --- Human-readable report ---
    print(f"Bastyde dependency report  ({len(names)} internal crates under {root})")
    print("=" * 72)

    print("\nPer-crate internal dependencies (normal/build + dev):")
    width = max(len(n) for n in names)
    for n in names:
        nd = ", ".join(sorted(normal_edges[n])) or "-"
        line = f"  {n:<{width}}  {nd}"
        pub = "" if crates[n].publish else "  [publish=false]"
        print(line + pub)
        if dev_edges[n]:
            print(f"  {'':<{width}}  (dev) {', '.join(sorted(dev_edges[n]))}")

    print("\n" + "-" * 72)
    if blocking_cycles:
        print("RELEASE-BLOCKING CYCLES (normal/build dependency graph):")
        for cyc in blocking_cycles:
            print(f"  !!  {' -> '.join(cyc)} -> {cyc[0]}")
        print("\n  These cannot be published in any order. Break the cycle")
        print("  (e.g. move the offending dependency to [dev-dependencies],")
        print("  or extract the shared code into a lower crate).")
    else:
        print("No release-blocking cycles in the normal/build dependency graph. OK")

    if dev_back_edges:
        print("\nDev-dependency back-edges (allowed by cargo, informational):")
        for a, b in dev_back_edges:
            print(f"  i   {a}  --dev-->  {b}   ({b} already depends on {a} via normal deps)")
        print("  These are fine: dev-deps are not part of the published")
        print("  dependency graph, so they never block `cargo publish`.")

    print("\n" + "-" * 72)
    print("Release order (publish dependencies first; crates in a wave are")
    print("mutually independent and may be published in any order):")
    if waves:
        for i, wave in enumerate(waves, 1):
            print(f"  Wave {i}:")
            for n in wave:
                tag = "" if crates[n].publish else "  [publish=false]"
                print(f"      {n}{tag}")
    if leftover:
        print("\n  NOT ORDERED (caught in a cycle):")
        for n in leftover:
            print(f"      {n}")

    # Flat order for copy/paste into a publish script.
    if not leftover:
        flat = [n for wave in waves for n in wave]
        print("\nFlat release order:")
        print("  " + " ".join(flat))

    return 1 if blocking_cycles else 0


if __name__ == "__main__":
    raise SystemExit(main())
