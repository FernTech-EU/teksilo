# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Scenarios, and the one every example gets for free: the Tab walk."""

from __future__ import annotations

import importlib
import pkgutil
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

from .checks import _is_focus, _focus_node


@dataclass
class Scenario:
    """How to launch one example and what to do to it.

    `package` is the cargo package, whose binary is `target/debug/<binary or
    package>`. `lang` sets the application's `LANG`; `orca_lang` Orca's, which
    decides the language of its role names and messages.
    """

    name: str
    package: str
    body: Callable[[Any], None]
    description: str = ""
    binary: str | None = None
    args: list[str] = field(default_factory=list)
    env: dict[str, str] = field(default_factory=dict)
    lang: str | None = None
    orca_lang: str | None = None


def registry() -> dict[str, Scenario]:
    """Every scenario in `tools/reader/scenarios/`, by name."""
    import scenarios  # noqa: PLC0415 - the package beside `reader_lib`

    found: dict[str, Scenario] = {}
    for info in pkgutil.iter_modules([str(Path(scenarios.__file__).parent)]):
        try:
            module = importlib.import_module(f"scenarios.{info.name}")
        except Exception as exc:  # one broken module must not stop the others
            import sys

            print(f"reader: skipping scenarios/{info.name}.py, which does not import: "
                  f"{type(exc).__name__}: {exc}", file=sys.stderr)
            continue
        for scenario in getattr(module, "SCENARIOS", []):
            if scenario.name in found:
                raise ValueError(f"two scenarios are named {scenario.name!r}")
            found[scenario.name] = scenario
    return found


def tab_walk(run: Any, *, stops: int = 40, chord: str = "Tab", label: str = "Tab") -> list[dict]:
    """Press `chord` until focus comes back to where the walk started or
    `stops` presses have been made, one act a press. Returns what each press
    landed on, as the bus reported it."""
    landed: list[dict] = []
    first_path = None
    for i in range(stops):
        with run.act(f"{label} {i + 1}", should="focus moves to the next control, "
                     "and the reader says what it is", settle=0.4, record=1.3) as act:
            run.key(chord)
        run.collect_events(act)
        moves = [e for e in act.events if _is_focus(e)]
        node = _focus_node(moves[-1]) if moves else {}
        landed.append(node)
        path = node.get("path")
        if not path:
            continue
        if first_path is None:
            first_path = path
        elif path == first_path:
            break
    return landed


def tab_walk_scenario(name: str, package: str, *, args: list[str] | None = None,
                      stops: int = 40, description: str = "", **extra: Any) -> Scenario:
    def body(run: Any) -> None:
        tab_walk(run, stops=stops)
    return Scenario(name, package, body, description or
                    f"Tab through {package} and record what each stop says",
                    args=list(args or []), **extra)
