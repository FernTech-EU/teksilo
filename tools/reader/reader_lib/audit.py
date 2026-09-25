# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Static checks on a tree as a reader walks it on AT-SPI.

These look at one snapshot and nothing else, so they only name what is wrong
for any reader in any state: a control a reader lands on with no name, a
window with no title, a node the adapter could not answer for. They are the
census half of a sweep; what a node *says* when reached is the acts' half.
"""

from __future__ import annotations

from typing import Iterator

#: Roles a reader lands on or operates, which must carry a name.
CONTROLS = {
    "push button", "toggle button", "check box", "radio button", "combo box",
    "entry", "password text", "spin button", "slider", "menu item",
    "check menu item", "radio menu item", "page tab", "link", "list item",
    "tree item", "table cell", "switch", "scroll bar", "menu", "text",
}

#: Containers a reader is told the name of when entering them.
NAMED_CONTAINERS = {"dialog", "alert", "page tab list", "list box", "tree", "table",
                    "tree table", "tool bar", "menu bar", "grouping", "landmark",
                    "scroll pane", "panel", "section", "form"}


def walk(node: dict | None, depth: int = 0, parent: dict | None = None
         ) -> Iterator[tuple[dict, int, dict | None]]:
    if not node:
        return
    yield node, depth, parent
    for child in node.get("children", []):
        yield from walk(child, depth + 1, node)


def issues(tree: dict | None) -> list[dict]:
    found: list[dict] = []

    def add(kind: str, node: dict, why: str) -> None:
        found.append({"kind": kind, "role": node.get("role"), "name": node.get("name"),
                      "path": node.get("path"), "why": why})

    for node, _depth, parent in walk(tree):
        role = node.get("role") or ""
        name = (node.get("name") or "").strip()
        states = set(node.get("states", []))
        if "error" in node:
            found.append({"kind": "unreadable-child", "role": parent and parent.get("role"),
                          "name": parent and parent.get("name"),
                          "path": parent and parent.get("path"), "why": node["error"]})
            continue
        if node.get("errors"):
            add("adapter-error", node, f"the adapter could not answer: {node['errors']}")
        if role == "frame" and not name:
            add("unnamed-window", node, "a window with no name; a reader announces nothing "
                "when it becomes active")
        if role in CONTROLS and not name and "focusable" in states:
            text = (node.get("text") or {}).get("text", "")
            if role in ("entry", "text", "password text") and node.get("relations", {}).get(
                    "labelled-by"):
                continue
            add("unnamed-control", node,
                f"a focusable {role} with no name" + (f" (its text is {text!r})" if text else ""))
        if role in NAMED_CONTAINERS and not name and role in ("dialog", "alert"):
            add("unnamed-dialog", node, f"a {role} with no name; a reader announces an "
                "untitled dialog")
        if role == "scroll pane" and not name:
            add("unnamed-scroll-pane", node, "a scroll pane with no name, which a reader "
                "may announce on entering it as a bare role")
        if role == "unknown":
            add("unknown-role", node, "a node whose role the adapter could not map")
        if "focusable" in states and "showing" not in states and "focused" not in states:
            add("focusable-offscreen", node, "focusable but not showing")
    return found


def focus_path(tree: dict | None, path: str) -> list[dict]:
    """The chain of ancestors of the node at `path`, root first."""
    chain: list[dict] = []

    def search(node: dict, trail: list[dict]) -> bool:
        trail = trail + [node]
        if node.get("path") == path:
            chain.extend(trail)
            return True
        return any(search(c, trail) for c in node.get("children", []))

    if tree:
        search(tree, [])
    return chain


def outline(tree: dict | None, max_depth: int = 40, width: int = 160) -> list[str]:
    """A text outline of the tree, one node a line."""
    lines = []
    for node, depth, _parent in walk(tree):
        if depth > max_depth:
            continue
        if "error" in node:
            lines.append("  " * depth + f"<error: {node['error']}>")
            continue
        bits = [f"[{node.get('role')}]", repr(node.get("name") or "")]
        if node.get("description"):
            bits.append(f"desc={node['description']!r}")
        states = [s for s in node.get("states", [])
                  if s not in ("enabled", "sensitive", "visible", "showing")]
        if states:
            bits.append("{" + ",".join(states) + "}")
        attrs = node.get("attributes")
        if attrs:
            bits.append(f"attrs={attrs}")
        if node.get("value"):
            bits.append(f"value={node['value']}")
        text = (node.get("text") or {}).get("text")
        if text and text != node.get("name"):
            bits.append(f"text={text[:60]!r}")
        if node.get("relations"):
            bits.append(f"rel={sorted(node['relations'])}")
        if node.get("locale"):
            bits.append(f"lang={node['locale']}")
        line = "  " * depth + " ".join(bits)
        lines.append(line if len(line) <= width else line[:width - 1] + "…")
    return lines
