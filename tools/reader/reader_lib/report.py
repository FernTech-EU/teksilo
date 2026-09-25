# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""A run, written down: `run.json` for tools, `report.txt` for people."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from . import audit
from .checks import transcript


def status_of(run: Any) -> int:
    """0 when every check passed, 2 when a check failed or an observation was
    made, 1 when an act could not be done or the run stopped before its acts
    were done (a run that did nothing has passed nothing)."""
    if getattr(run, "stopped", None):
        return 1
    worst = 0
    for act in run.acts:
        if act.error and worst == 0:
            worst = 1
        for result in act.results:
            if result["status"] in ("FAIL", "observed"):
                worst = 2
            elif result["status"] == "error" and worst == 0:
                worst = 1
    return worst


def text(run: Any) -> str:
    out: list[str] = []
    scenario = run.scenario
    out.append(f"# {scenario.name}: {scenario.description}")
    out.append(f"package {scenario.package} {' '.join(scenario.args)}".rstrip())
    out.append(f"Orca: {'on' if run.orca is not None else 'off'}; output {run.out_dir}")
    if getattr(run, "stopped", None):
        out.append(f"THE RUN STOPPED BEFORE ITS ACTS WERE DONE: {run.stopped}")
    for note in run.notes:
        out.append(f"note: {note}")
    warnings = run.listener.libatspi_warnings() if run.listener is not None else {}
    if any(warnings.values()):
        out.append("libatspi refused the application's cache signals: "
                   + ", ".join(f"{k} {v}x" for k, v in warnings.items())
                   + " (accesskit_unix sends them flattened; see listener.log)")
    for act in run.acts:
        out.append("")
        out.append(f"== {act.label}" + (f"  ({act.should})" if act.should else ""))
        if act.steps:
            out.append("   steps: " + "; ".join(act.steps))
        if act.bridge_used:
            out.append("   (the automation bridge was used in this act)")
        if act.orca_lag_ms is not None and act.orca_lag_ms > 500:
            out.append(f"   (Orca received this act's first event {act.orca_lag_ms:.0f} ms "
                       "after the bus carried it)")
        if act.orca_missing:
            out.append(f"   (Orca never logged receiving {act.orca_missing} of this act's events)")
        if act.error:
            out.append(f"   COULD NOT RUN: {act.error}")
        for row in transcript(act):
            out.append("   " + row)
        for result in act.results:
            out.append(f"   {result['status']:>8}  {result['check']}")
            if result["status"] != "pass":
                for line in result["evidence"][:8]:
                    out.append(f"             {line}")
        if act.tree is not None and (act.label == "launch" or act.label.startswith("tree:")):
            problems = audit.issues(act.tree)
            if problems:
                out.append("   tree audit:")
                for p in problems:
                    out.append(f"     {p['kind']}: [{p['role']}] {p['name']!r}: {p['why']}")
    out.append("")
    out.append({0: "every check passed, nothing observed",
                1: "the run stopped, or an act could not be done: nothing here is a pass",
                2: "a check failed or an observation was made"}[status_of(run)])
    return "\n".join(out) + "\n"


def write(run: Any) -> None:
    data = {
        "scenario": run.scenario.name,
        "package": run.scenario.package,
        "args": run.scenario.args,
        "description": run.scenario.description,
        "orca": run.orca is not None,
        "notes": run.notes,
        "stopped": getattr(run, "stopped", None),
        "libatspi_warnings": run.listener.libatspi_warnings() if run.listener is not None else {},
        "status": status_of(run),
        "acts": [act.to_json() for act in run.acts],
        "audit": [dict(p, act=act.label) for act in run.acts if act.tree is not None
                  for p in audit.issues(act.tree)],
    }
    Path(run.out_dir, "run.json").write_text(json.dumps(data, ensure_ascii=False, indent=1),
                                             encoding="utf-8")
    Path(run.out_dir, "report.txt").write_text(text(run), encoding="utf-8")
    for act in run.acts:
        if act.tree is not None:
            safe = "".join(c if c.isalnum() else "-" for c in act.label)[:60]
            Path(run.out_dir, f"tree-{safe}.txt").write_text(
                "\n".join(audit.outline(act.tree)) + "\n", encoding="utf-8")
