# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Listen where a screen reader listens: the application on the AT-SPI bus.

Runs as a process of its own (`python3 -m reader_lib.listener --out FILE
--pid PID`), because the driver blocks between acts, and an event queued
behind a blocked main loop would be stamped with the time the loop came back
rather than the time it arrived.

It records every event the application emits on the private AT-SPI bus as one
JSON line, stamped with the system's monotonic clock (the driver's clock too,
so an act's window and its events are measured on one clock), and answers
commands on stdin with one JSON line each on stdout:

    {"cmd": "tree"}                       the whole tree, as AT-SPI exposes it
    {"cmd": "find", "role": ..., "name": ..., "name_contains": ...}
    {"cmd": "action", <how to find the node>, "action": "click"}
    {"cmd": "grab_focus", <how to find the node>}
    {"cmd": "mark", "label": ...}         a marker in the event file
    {"cmd": "quit"}

A node is found by `path`, by `"focused": true` (the node the last focus
event named), or by `role` / `name` / `name_contains` / `description_contains`
/ `nth` over a walk of the tree. Roles are AT-SPI role names as
`Atspi.Accessible.get_role_name` gives them ("push button", "check box").

The tree is what a reader walks: every node the adapter lists, which is the
tree after `accesskit_consumer`'s filter (a hidden node and its subtree are
gone, a `GenericContainer` is replaced by its children), with everything a
reader can ask a node for. See `node_record`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
import time
from typing import Any

#: The event families recorded. `object:` covers every object event the
#: AccessKit adapter emits (`accesskit_atspi_common/src/events.rs`).
LISTENED = ("object:", "window:", "document:", "focus:")

#: Text longer than this is cut in a record, so a document does not flood it.
TEXT_LIMIT = 2000


def now() -> tuple[float, str]:
    return time.monotonic(), dt.datetime.now().strftime("%H:%M:%S.%f")


class Listener:
    def __init__(self, out_path: str, pid: int) -> None:
        import gi

        gi.require_version("Atspi", "2.0")
        from gi.repository import Atspi, GLib

        self.Atspi = Atspi
        self.GLib = GLib
        self.pid = pid
        self.out = open(out_path, "w", encoding="utf-8", buffering=1)
        self.seq = 0
        self.last_focus: Any = None
        self.known: dict[str, Any] = {}
        #: The role and name each path had when last read, for an event whose
        #: node is gone before it can be asked.
        self.seen: dict[str, tuple[str, str]] = {}

    # -- Recording ----------------------------------------------------------

    def write(self, record: dict) -> None:
        self.seq += 1
        record["seq"] = self.seq
        self.out.write(json.dumps(record, ensure_ascii=False) + "\n")

    def describe(self, acc: Any) -> dict:
        if acc is None:
            return {}
        path = getattr(acc, "path", "") or ""
        if path:
            self.known[path] = acc
        described = {"path": path}
        try:
            described.update(name=acc.get_name() or "", role=acc.get_role_name() or "")
            if path:
                self.seen[path] = (described["role"], described["name"])
        except Exception:  # the node is already gone: say what it was
            role, name = self.seen.get(path, ("", ""))
            described.update(name=name, role=f"gone {role}".strip(), gone=True)
        return described

    def on_event(self, event: Any) -> None:
        mono, wall = now()
        try:
            if event.source.get_process_id() != self.pid:
                return
        except Exception:
            # A source gone before it could be asked still belongs to the
            # application when it is the only one on the bus; keep the event,
            # marked, rather than lose a defunct notice.
            pass
        if event.type == "object:state-changed:focused" and event.detail1 == 1:
            self.last_focus = event.source
        record = {
            "mono": mono,
            "wall": wall,
            "type": event.type,
            "detail1": event.detail1,
            "detail2": event.detail2,
            "source": self.describe(event.source),
        }
        data = event.any_data
        if isinstance(data, str):
            record["text"] = data[:TEXT_LIMIT]
        elif isinstance(data, (int, float)) and not isinstance(data, bool):
            record["value"] = data
        elif data is not None and hasattr(data, "get_name"):
            record["target"] = self.describe(data)
        self.write(record)

    # -- The tree -------------------------------------------------------------

    def app(self) -> Any:
        desktop = self.Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            child = desktop.get_child_at_index(i)
            try:
                if child is not None and child.get_process_id() == self.pid:
                    return child
            except Exception:
                continue
        return None

    def node_record(self, acc: Any, depth: int, children: bool, budget: list[int]) -> dict:
        """Everything a reader can ask one node for."""
        Atspi = self.Atspi
        record: dict[str, Any] = self.describe(acc)
        budget[0] -= 1

        def attempt(key: str, fn) -> None:
            try:
                value = fn()
            except Exception as exc:
                record.setdefault("errors", {})[key] = type(exc).__name__
                return
            if value not in (None, "", [], {}):
                record[key] = value

        attempt("description", acc.get_description)
        if hasattr(acc, "get_help_text"):
            attempt("help_text", acc.get_help_text)
        attempt("accessible_id", acc.get_accessible_id)
        attempt("locale", acc.get_object_locale)
        attempt("states", lambda: sorted(s.value_nick for s in acc.get_state_set().get_states()))
        attempt("attributes", lambda: dict(acc.get_attributes() or {}))
        attempt("interfaces", lambda: sorted(acc.get_interfaces()))
        attempt("index_in_parent", acc.get_index_in_parent)

        def relations() -> dict:
            found: dict[str, list[str]] = {}
            for rel in acc.get_relation_set() or []:
                kind = rel.get_relation_type().value_nick
                targets = [getattr(rel.get_target(i), "path", "")
                           for i in range(rel.get_n_targets())]
                found.setdefault(kind, []).extend(targets)
            return found

        attempt("relations", relations)
        interfaces = record.get("interfaces", [])
        if "Action" in interfaces:
            def actions() -> list[dict]:
                return [{"name": Atspi.Action.get_action_name(acc, i),
                         "description": Atspi.Action.get_action_description(acc, i),
                         "key_binding": Atspi.Action.get_key_binding(acc, i)}
                        for i in range(Atspi.Action.get_n_actions(acc))]
            attempt("actions", actions)
        if "Value" in interfaces:
            def value() -> dict:
                found = {"current": Atspi.Value.get_current_value(acc),
                         "minimum": Atspi.Value.get_minimum_value(acc),
                         "maximum": Atspi.Value.get_maximum_value(acc)}
                try:
                    found["increment"] = Atspi.Value.get_minimum_increment(acc)
                except Exception:
                    pass
                if hasattr(Atspi.Value, "get_text"):
                    try:
                        found["text"] = Atspi.Value.get_text(acc)
                    except Exception:
                        pass
                return found
            attempt("value", value)
        if "Text" in interfaces:
            def text() -> dict:
                count = Atspi.Text.get_character_count(acc)
                found = {"characters": count,
                         "text": Atspi.Text.get_text(acc, 0, min(count, TEXT_LIMIT))}
                try:
                    found["caret"] = Atspi.Text.get_caret_offset(acc)
                except Exception:
                    pass
                return found
            attempt("text", text)
        if "Selection" in interfaces:
            attempt("selected_children",
                    lambda: Atspi.Selection.get_n_selected_children(acc))
        if "Component" in interfaces:
            def extents() -> list[int]:
                rect = Atspi.Component.get_extents(acc, Atspi.CoordType.WINDOW)
                return [rect.x, rect.y, rect.width, rect.height]
            attempt("extents", extents)
        try:
            count = acc.get_child_count()
        except Exception as exc:
            record.setdefault("errors", {})["child_count"] = type(exc).__name__
            count = 0
        record["child_count"] = count
        if children and count and budget[0] > 0:
            kids = []
            for i in range(count):
                if budget[0] <= 0:
                    record["truncated"] = True
                    break
                try:
                    child = acc.get_child_at_index(i)
                except Exception as exc:
                    kids.append({"error": f"{type(exc).__name__}: {exc}"})
                    continue
                if child is None:
                    kids.append({"error": "no child at this index"})
                    continue
                kids.append(self.node_record(child, depth + 1, True, budget))
            record["children"] = kids
        return record

    def tree(self, limit: int) -> dict:
        root = self.app()
        if root is None:
            return {"error": "the application is not on the AT-SPI bus"}
        # libatspi caches what it has read of each node (states, interfaces)
        # and AccessKit's cache signals, which would refresh it, are refused
        # (see the listener log): without this, a node read once keeps its
        # old interfaces and states in every later tree.
        try:
            root.clear_cache()
        except Exception:
            pass
        budget = [limit]
        tree = self.node_record(root, 0, True, budget)
        return {"tree": tree, "nodes_left": budget[0]}

    # -- Finding a node ---------------------------------------------------------

    def walk(self, limit: int = 20000):
        root = self.app()
        if root is None:
            return
        stack, seen = [root], 0
        while stack and seen < limit:
            node = stack.pop()
            seen += 1
            yield node
            try:
                count = node.get_child_count()
            except Exception:
                continue
            for i in reversed(range(count)):
                try:
                    child = node.get_child_at_index(i)
                except Exception:
                    continue
                if child is not None:
                    stack.append(child)

    def matches(self, acc: Any, spec: dict) -> bool:
        try:
            if "role" in spec and acc.get_role_name() != spec["role"]:
                return False
            name = acc.get_name() or ""
            if "name" in spec and name != spec["name"]:
                return False
            if "name_contains" in spec and spec["name_contains"].casefold() not in name.casefold():
                return False
            if "name_startswith" in spec and not name.startswith(spec["name_startswith"]):
                return False
            if "description_contains" in spec and spec["description_contains"].casefold() \
                    not in (acc.get_description() or "").casefold():
                return False
            if "state" in spec:
                states = {s.value_nick for s in acc.get_state_set().get_states()}
                if spec["state"] not in states:
                    return False
        except Exception:
            return False
        return True

    def resolve(self, spec: dict) -> Any:
        if spec.get("focused"):
            return self.last_focus
        if "path" in spec:
            return self.known.get(spec["path"])
        nth = int(spec.get("nth", 0))
        for node in self.walk():
            if self.matches(node, spec):
                if nth == 0:
                    return node
                nth -= 1
        return None

    # -- Acting -------------------------------------------------------------------

    def action(self, spec: dict) -> dict:
        Atspi = self.Atspi
        node = self.resolve(spec)
        if node is None:
            return {"error": f"no node matches {spec}"}
        if node.get_action_iface() is None or Atspi.Action.get_n_actions(node) == 0:
            return {"error": f"{self.describe(node)} offers no action on AT-SPI"}
        wanted = spec.get("action")
        index = 0
        names = [Atspi.Action.get_action_name(node, i)
                 for i in range(Atspi.Action.get_n_actions(node))]
        if isinstance(wanted, str):
            if wanted not in names:
                return {"error": f"{self.describe(node)} has no action {wanted!r}: {names}"}
            index = names.index(wanted)
        elif isinstance(wanted, int):
            index = wanted
        mono, wall = now()
        record = {"mono": mono, "wall": wall, "type": "harness:action",
                  "action": names[index], "source": self.describe(node)}
        self.write(record)
        ok = Atspi.Action.do_action(node, index)
        record["done"] = bool(ok)
        return record

    def grab_focus(self, spec: dict) -> dict:
        node = self.resolve(spec)
        if node is None:
            return {"error": f"no node matches {spec}"}
        mono, wall = now()
        record = {"mono": mono, "wall": wall, "type": "harness:grab-focus",
                  "source": self.describe(node)}
        self.write(record)
        record["done"] = bool(self.Atspi.Component.grab_focus(node))
        return record

    def command(self, message: dict) -> dict:
        cmd = message.get("cmd")
        if cmd == "tree":
            return self.tree(int(message.get("limit", 6000)))
        if cmd == "find":
            node = self.resolve(message)
            if node is None:
                return {"node": None}
            return {"node": self.node_record(node, 0, False, [1])}
        if cmd == "find_all":
            limit = int(message.get("limit", 200))
            found = []
            for node in self.walk():
                if self.matches(node, message):
                    found.append(self.node_record(node, 0, False, [1]))
                    if len(found) >= limit:
                        break
            return {"nodes": found}
        if cmd == "action":
            return self.action(message)
        if cmd == "grab_focus":
            return self.grab_focus(message)
        if cmd == "mark":
            mono, wall = now()
            record = {"mono": mono, "wall": wall, "type": "harness:mark",
                      "label": message.get("label", "")}
            self.write(record)
            return record
        if cmd == "ping":
            mono, wall = now()
            return {"mono": mono, "wall": wall, "app": self.app() is not None}
        return {"error": f"unknown command {cmd!r}"}

    def run(self) -> int:
        Atspi, GLib = self.Atspi, self.GLib

        def on_command(_channel: Any, _condition: Any) -> bool:
            line = sys.stdin.readline()
            if not line:
                Atspi.event_quit()
                return False
            try:
                message = json.loads(line)
            except ValueError:
                print(json.dumps({"error": f"not JSON: {line!r}"}), flush=True)
                return True
            if message.get("cmd") == "quit":
                print(json.dumps({"bye": True}), flush=True)
                Atspi.event_quit()
                return False
            try:
                reply = self.command(message)
            except Exception as exc:
                reply = {"error": f"{type(exc).__name__}: {exc}"}
            print(json.dumps(reply, ensure_ascii=False), flush=True)
            return True

        listener = Atspi.EventListener.new(self.on_event)
        for kind in LISTENED:
            listener.register(kind)
        GLib.io_add_watch(GLib.IOChannel.unix_new(0), GLib.PRIORITY_DEFAULT,
                          GLib.IOCondition.IN | GLib.IOCondition.HUP, on_command)
        print(json.dumps({"ready": True}), flush=True)
        Atspi.event_main()
        return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--pid", type=int, required=True)
    args = parser.parse_args(argv)
    return Listener(args.out, args.pid).run()


if __name__ == "__main__":
    sys.exit(main())
