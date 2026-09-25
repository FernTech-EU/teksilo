# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""What an act should give a reader, and what it gave.

Two kinds of result per act:

* **Checks**, which a scenario states (`expect=[...]`): what a reader should
  get from the act. Each passes, fails with the evidence, or is skipped when
  the run lacks what it needs (a check on speech, without Orca).
* **Observations**, made on every act whatever the scenario said: things that
  are wrong for any reader wherever they happen. An announcement from a node
  the bus had already been told was defunct (Orca drops it). An announcement
  in the same act and ahead of the act's first focus change (Orca stops
  speech to read a new focus, so it is cut). Orca ignoring an event as
  defunct. Orca cutting an utterance.

Evidence is quoted from the bus and from Orca's log, so a finding can be
checked without running anything again.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any, Callable

from .orca import fate, normalized, utterances


def _norm(text: str | None) -> str:
    return normalized(text or "")


def _node_matches(node: dict, role: str | None, name: str | None,
                  name_contains: str | None) -> bool:
    if role is not None and node.get("role") != role:
        return False
    if name is not None and (node.get("name") or "") != name:
        return False
    if name_contains is not None and _norm(name_contains) not in _norm(node.get("name")):
        return False
    return True


def _event_line(act: Any, event: dict) -> str:
    source = event.get("source", {})
    extra = ""
    if "text" in event:
        extra = f" text={event['text']!r}"
    elif "target" in event:
        target = event["target"]
        extra = f" -> [{target.get('role')}] {target.get('name')!r}"
    detail = f" {event.get('detail1')}" if event["type"].startswith("object:state") else ""
    return (f"{act.rel_ms(event):+8.1f} ms {event['type']}{detail} "
            f"[{source.get('role')}] {source.get('name')!r}{extra}")


def _is_focus(event: dict) -> bool:
    if event["type"] == "object:state-changed:focused":
        return event.get("detail1") == 1
    return event["type"] == "object:active-descendant-changed"


def _focus_node(event: dict) -> dict:
    if event["type"] == "object:active-descendant-changed":
        return event.get("target", {})
    return event.get("source", {})


# ---------------------------------------------------------------------------
# Checks a scenario states
# ---------------------------------------------------------------------------


@dataclass
class Check:
    kind: str
    describe: str
    run: Callable[[Any], tuple[bool, list[str]]]
    needs_orca: bool = False
    needs_tree: bool = False


def said(text: str) -> Check:
    """Orca said something containing `text`, and nothing cut it."""
    def run(act: Any) -> tuple[bool, list[str]]:
        result = fate(act.orca, text)
        evidence = [f"Orca {result}: {text!r}"]
        evidence += [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                     for u in utterances(act.orca)]
        return result == "spoken", evidence
    return Check("said", f"Orca says {text!r}", run, needs_orca=True)


def not_said(text: str) -> Check:
    """Orca said nothing containing `text`."""
    def run(act: Any) -> tuple[bool, list[str]]:
        heard = [u for u in utterances(act.orca) if _norm(text) in _norm(u.text)]
        return not heard, [f"Orca said: {u.text!r}" for u in heard]
    return Check("not_said", f"Orca does not say {text!r}", run, needs_orca=True)


def said_once(text: str) -> Check:
    """Orca said `text` exactly once in the act."""
    def run(act: Any) -> tuple[bool, list[str]]:
        heard = [u for u in utterances(act.orca) if _norm(text) in _norm(u.text)]
        return len(heard) == 1, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                                 for u in heard] or [f"Orca never said {text!r}"]
    return Check("said_once", f"Orca says {text!r} once", run, needs_orca=True)


def announced(text: str) -> Check:
    """An `object:announcement` carrying `text` reached the bus."""
    def run(act: Any) -> tuple[bool, list[str]]:
        found = [e for e in act.events if e["type"] == "object:announcement"
                 and _norm(text) in _norm(e.get("text"))]
        every = [_event_line(act, e) for e in act.events if e["type"] == "object:announcement"]
        return bool(found), every or ["no object:announcement in the act"]
    return Check("announced", f"the bus carries an announcement of {text!r}", run)


def not_announced(text: str) -> Check:
    def run(act: Any) -> tuple[bool, list[str]]:
        found = [e for e in act.events if e["type"] == "object:announcement"
                 and _norm(text) in _norm(e.get("text"))]
        return not found, [_event_line(act, e) for e in found]
    return Check("not_announced", f"no announcement of {text!r}", run)


def focused(role: str | None = None, name: str | None = None,
            name_contains: str | None = None) -> Check:
    """The act's last focus change (a focused state or an active descendant)
    landed on a matching node."""
    want = f"[{role or '*'}] {name if name is not None else name_contains or '*'!r}"

    def run(act: Any) -> tuple[bool, list[str]]:
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change on the bus in this act"]
        last = _focus_node(moves[-1])
        ok = _node_matches(last, role, name, name_contains)
        return ok, [_event_line(act, e) for e in moves]
    return Check("focused", f"focus lands on {want}", run)


def event(type_prefix: str, role: str | None = None, name_contains: str | None = None,
          detail1: int | None = None, text_contains: str | None = None) -> Check:
    """At least one event of this type, from a matching source."""
    def pick(act: Any) -> list[dict]:
        return [e for e in act.events if e["type"].startswith(type_prefix)
                and _node_matches(e.get("source", {}), role, None, name_contains)
                and (detail1 is None or e.get("detail1") == detail1)
                and (text_contains is None or _norm(text_contains) in _norm(e.get("text")))]

    def run(act: Any) -> tuple[bool, list[str]]:
        found = pick(act)
        return bool(found), [_event_line(act, e) for e in found] or \
            [f"no {type_prefix} event from [{role or '*'}] {name_contains or '*'!r}"]
    return Check("event", f"a {type_prefix} event from [{role or '*'}] "
                 f"{name_contains or '*'!r}", run)


def no_event(type_prefix: str, role: str | None = None,
             name_contains: str | None = None) -> Check:
    def run(act: Any) -> tuple[bool, list[str]]:
        found = [e for e in act.events if e["type"].startswith(type_prefix)
                 and _node_matches(e.get("source", {}), role, None, name_contains)]
        return not found, [_event_line(act, e) for e in found]
    return Check("no_event", f"no {type_prefix} event from [{role or '*'}] "
                 f"{name_contains or '*'!r}", run)


def _walk(tree: dict | None):
    if not tree:
        return
    stack = [tree]
    while stack:
        node = stack.pop()
        yield node
        stack.extend(reversed(node.get("children", [])))


def in_tree(role: str | None = None, name: str | None = None,
            name_contains: str | None = None, state: str | None = None,
            description_contains: str | None = None) -> Check:
    """After the act, the tree a reader walks holds a matching node."""
    def run(act: Any) -> tuple[bool, list[str]]:
        for node in _walk(act.tree):
            if _node_matches(node, role, name, name_contains) \
                    and (state is None or state in node.get("states", [])) \
                    and (description_contains is None
                         or _norm(description_contains) in _norm(node.get("description"))):
                return True, [f"found [{node.get('role')}] {node.get('name')!r} "
                              f"states={node.get('states')}"]
        near = [n for n in _walk(act.tree) if _node_matches(n, role, name, name_contains)]
        if near:
            return False, [f"found [{n.get('role')}] {n.get('name')!r} but "
                           f"states={n.get('states')} description={n.get('description')!r}"
                           for n in near[:5]]
        return False, ["no such node in the tree after the act"]
    return Check("in_tree", f"the tree holds [{role or '*'}] "
                 f"{name if name is not None else name_contains or '*'!r}", run, needs_tree=True)


def not_in_tree(role: str | None = None, name: str | None = None,
                name_contains: str | None = None) -> Check:
    def run(act: Any) -> tuple[bool, list[str]]:
        found = [n for n in _walk(act.tree) if _node_matches(n, role, name, name_contains)]
        return not found, [f"found [{n.get('role')}] {n.get('name')!r}" for n in found]
    return Check("not_in_tree", f"the tree holds no [{role or '*'}] "
                 f"{name if name is not None else name_contains or '*'!r}", run, needs_tree=True)


def focus_stays(role: str | None = None, name: str | None = None,
                name_contains: str | None = None) -> Check:
    """No focus change in the act, and the node that last took focus before
    it (on the bus) matches: focus stayed where it was, as when Escape closes
    a popup and hands focus back to nothing new."""
    def run(act: Any) -> tuple[bool, list[str]]:
        moves = [e for e in act.events if _is_focus(e)]
        if moves:
            return False, [_event_line(act, e) for e in moves]
        before = [e for e in act.history if _is_focus(e) and e["mono"] < act.start_mono]
        if not before:
            return False, ["focus never moved on the bus before this act"]
        holder = _focus_node(before[-1])
        ok = _node_matches(holder, role, name, name_contains)
        return ok, [f"focus stayed on [{holder.get('role')}] {holder.get('name')!r}"]
    want = f"[{role or '*'}] {name if name is not None else name_contains or '*'!r}"
    return Check("focus_stays", f"focus stays on {want}", run)


def custom(describe: str, fn: Callable[[Any], tuple[bool, list[str]]], *,
           needs_orca: bool = False, needs_tree: bool = False) -> Check:
    return Check("custom", describe, fn, needs_orca=needs_orca, needs_tree=needs_tree)


def evaluate(check: Check, act: Any, have_orca: bool) -> dict:
    if act.error:
        return {"kind": check.kind, "check": check.describe, "status": "error",
                "evidence": [act.error]}
    if check.needs_orca and not have_orca:
        return {"kind": check.kind, "check": check.describe, "status": "skipped",
                "evidence": ["needs Orca: run with --orca"]}
    try:
        ok, evidence = check.run(act)
    except Exception as exc:  # a broken check is not a finding
        return {"kind": check.kind, "check": check.describe, "status": "error",
                "evidence": [f"the check itself failed: {type(exc).__name__}: {exc}"]}
    return {"kind": check.kind, "check": check.describe,
            "status": "pass" if ok else "FAIL", "evidence": evidence}


# ---------------------------------------------------------------------------
# Observations made on every act
# ---------------------------------------------------------------------------


def observations(act: Any) -> list[dict]:
    found: list[dict] = []

    def note(kind: str, text: str, evidence: list[str]) -> None:
        found.append({"kind": kind, "check": text, "status": "observed",
                      "evidence": evidence})

    first_focus = next((e for e in act.events if _is_focus(e)), None)
    for e in act.events:
        if e["type"] != "object:announcement":
            continue
        earlier_defunct = any(
            d["type"] == "object:state-changed:defunct" and d.get("detail1") == 1
            and d["source"].get("path") == e["source"].get("path") and d["seq"] < e["seq"]
            for d in act.history)
        if earlier_defunct:
            note("announced-from-defunct",
                 f"{e.get('text')!r} came from a node the bus had already been told was "
                 "defunct, which Orca drops",
                 [_event_line(act, e), f"path {e['source'].get('path')}"])
        # Only an announcement that went out with the focus change, in one
        # accessibility update: the consumer hands an update's node changes to
        # the adapters before its focus event, and Orca stops speech for the
        # new focus. One sent well before a focus move the act made later (a
        # Tab pressed after it) is not the race this is about.
        if first_focus is not None and e["seq"] < first_focus["seq"] \
                and (first_focus["mono"] - e["mono"]) * 1000 <= SAME_UPDATE_MS:
            note("announced-before-focus",
                 f"{e.get('text')!r} reached the bus "
                 f"{(first_focus['mono'] - e['mono']) * 1000:.1f} ms before the act's focus "
                 "change, which Orca interrupts to read the new focus",
                 [_event_line(act, e), _event_line(act, first_focus)])
    last_event = None
    for line in act.orca:
        if line.is_event:
            last_event = line
        if line.is_defunct_drop:
            dropped = last_event.text if last_event is not None else ""
            # Orca drops the focus loss of a node that has just been removed
            # on every close of a menu, dialog or list: nothing a reader needs.
            if HARMLESS_DROP.search(dropped):
                continue
            if app_name(act) and last_event is not None and \
                    f"in [application: '{app_name(act)}']" not in dropped:
                continue
            note("orca-dropped-defunct", "Orca ignored an event whose source was defunct",
                 [f"{last_event.stamp} {dropped}" if last_event else "",
                  f"{line.stamp} {line.text}"])
    for u in utterances(act.orca):
        # Orca's echo of a key is cut by the next key, as for any fast typist.
        if u.cut and not u.echo:
            note("orca-cut", f"Orca's {u.text!r} was cut by a stop "
                 f"{(u.cut_after or 0) * 1000:.0f} ms in (estimated)",
                 [f"{u.stamp} said {u.text!r}"])
    return found


#: An announcement this close before the act's first focus change went out in
#: the same accessibility update as it (one frame is about 16 ms).
SAME_UPDATE_MS = 25.0

#: Events Orca drops as defunct with no loss to a reader: the focus leaving a
#: node that was just removed, and the removal itself.
HARMLESS_DROP = re.compile(r"object:state-changed:focused for .* \(0, |"
                           r"object:children-changed:remove|object:state-changed:showing .* \(0, ")


def app_name(act: Any) -> str:
    return getattr(act, "app_name", "") or ""


def transcript(act: Any, skip_bounds: bool = True) -> list[str]:
    """The act's events and Orca's speech, in one timeline."""
    rows: list[tuple[float, str]] = []
    for e in act.events:
        if skip_bounds and e["type"] == "object:bounds-changed":
            continue
        if e["type"].startswith("harness:"):
            rows.append((act.rel_ms(e), f"{act.rel_ms(e):+8.1f} ms == {e['type']} "
                         f"{e.get('action') or e.get('label') or ''} "
                         f"[{e.get('source', {}).get('role')}] "
                         f"{e.get('source', {}).get('name')!r}"))
            continue
        rows.append((act.rel_ms(e), "  " + _event_line(act, e)))
    from .orca import seconds

    start = seconds(act.start_wall) if act.start_wall else 0.0
    for u in utterances(act.orca):
        ms = (seconds(u.stamp) - start) * 1000.0
        rows.append((ms, f"{ms:+8.1f} ms ORCA SAYS{' (CUT)' if u.cut else ''}: {u.text!r}"))
    rows.sort(key=lambda r: r[0])
    return [text for _, text in rows]

