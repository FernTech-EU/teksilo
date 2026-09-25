#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""What a screen reader gets from a Teksilo example, recorded.

    tools/reader/reader.py list
    tools/reader/reader.py run dialogs --orca
    tools/reader/reader.py run dialogs datetime --orca --out target/reader-sweep
    tools/reader/reader.py tabwalk spin-box --orca
    tools/reader/reader.py tree widget-catalog -- --tab overlays

Every command launches the example in a private, invisible session
(`private_session.sh`): its own D-Bus session, AT-SPI bus, runtime directory
and KWin, no `DISPLAY`, settings in memory, no sound. Started from the
desktop, this script re-runs itself inside one, so nothing it does can reach
the desktop's windows, its accessibility bus or its speakers. See
`docs/a11y/reader-harness.md` for what it records and what that proves.

Everything after a lone `--` is the example's own arguments, wherever the
options before it are.

Exit 0 when every check passed and nothing was observed, 2 when a check failed
or an observation was made (a finding), 1 when something could not be run.
"""

from __future__ import annotations

import argparse
import os
import sys
import traceback
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from reader_lib import report  # noqa: E402
from reader_lib.run import Options, Run, RunError, inside_private_session  # noqa: E402
from reader_lib.scenario import Scenario, registry, tab_walk  # noqa: E402


def reexec_privately(argv: list[str]) -> int:
    script = HERE / "private_session.sh"
    os.execv(str(script), [str(script), sys.executable, str(Path(__file__).resolve()), *argv])
    return 1  # not reached


def run_one(scenario: Scenario, options: Options) -> int:
    run = Run(scenario, options)
    print(f"--- {scenario.name}: {run.out_dir}", flush=True)
    try:
        run.start()
        scenario.body(run)
    except RunError as exc:
        run.stopped = str(exc)
        run.note(f"the run stopped: {exc}")
        print(f"could not run {scenario.name}: {exc}", file=sys.stderr)
    except Exception as exc:  # a bug in a scenario is not a finding
        run.stopped = f"the scenario itself failed: {type(exc).__name__}: {exc}"
        run.note("the scenario itself failed:\n" + "".join(
            traceback.format_exception(type(exc), exc, exc.__traceback__)))
        print(run.notes[-1], file=sys.stderr)
    finally:
        try:
            if run.listener is not None:
                run.collect()
        finally:
            run.stop()
    report.write(run)
    print(report.text(run), flush=True)
    return report.status_of(run)


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    # The example's own arguments are everything after a lone `--`, taken out
    # before parsing: argparse would not accept them after the options.
    example_args: list[str] = []
    if "--" in argv:
        cut = argv.index("--")
        argv, example_args = argv[:cut], argv[cut + 1:]
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("list", help="list the scenarios")

    def common(p: argparse.ArgumentParser) -> None:
        p.add_argument("--orca", action="store_true",
                       help="run Orca 46.1 beside the example, silent, and read its log")
        p.add_argument("--out", type=Path, help="where to write each run (default target/reader)")
        p.add_argument("--binary", help="drive this binary instead of target/debug/<package>")
        p.add_argument("--trees", action="store_true", help="take a tree after every act")
        p.add_argument("--settle", type=float, default=None)
        p.add_argument("--record", type=float, default=None)

    run_p = sub.add_parser("run", help="run named scenarios")
    run_p.add_argument("names", nargs="+")
    common(run_p)
    walk_p = sub.add_parser("tabwalk", help="Tab through a package and record every stop")
    walk_p.add_argument("package")
    walk_p.add_argument("--stops", type=int, default=40)
    walk_p.add_argument("--lang")
    walk_p.add_argument("--env", action="append", default=[], metavar="NAME=VALUE",
                        help="set in the example's environment (repeatable)")
    walk_p.add_argument("args", nargs="*", help="after --: the example's own arguments")
    common(walk_p)
    tree_p = sub.add_parser("tree", help="launch a package and write the tree a reader walks")
    tree_p.add_argument("package")
    tree_p.add_argument("--lang")
    tree_p.add_argument("--env", action="append", default=[], metavar="NAME=VALUE",
                        help="set in the example's environment (repeatable)")
    tree_p.add_argument("args", nargs="*", help="after --: the example's own arguments")
    common(tree_p)
    args = parser.parse_args(argv)

    if args.command == "list":
        for name, scenario in sorted(registry().items()):
            print(f"{name:<28} {scenario.package:<24} {scenario.description}")
        return 0

    if inside_private_session() is not None:
        return reexec_privately(argv + (["--", *example_args] if example_args else []))
    if hasattr(args, "args"):
        args.args = list(args.args) + example_args
    env: dict[str, str] = {}
    for item in getattr(args, "env", []):
        name, sep, value = item.partition("=")
        if not sep or not name:
            print(f"--env takes NAME=VALUE, not {item!r}", file=sys.stderr)
            return 1
        env[name] = value

    options = Options(orca=args.orca, binary=args.binary, out_dir=args.out, trees=args.trees)
    if args.settle is not None:
        options.settle = args.settle
    if args.record is not None:
        options.record = args.record

    if args.command == "run":
        known = registry()
        missing = [n for n in args.names if n not in known]
        if missing:
            print(f"no scenario named {', '.join(missing)}; see `reader.py list`",
                  file=sys.stderr)
            return 1
        statuses = [run_one(known[n], options) for n in args.names]
    elif args.command == "tabwalk":
        scenario = Scenario(f"tabwalk-{args.package}", args.package,
                            lambda run: tab_walk(run, stops=args.stops),
                            f"Tab through {args.package}", args=args.args, lang=args.lang,
                            env=env)
        statuses = [run_one(scenario, options)]
    else:
        scenario = Scenario(f"tree-{args.package}", args.package, lambda run: None,
                            f"the tree of {args.package} at launch", args=args.args,
                            lang=args.lang, env=env)
        statuses = [run_one(scenario, options)]
    return 1 if 1 in statuses else (2 if 2 in statuses else 0)


if __name__ == "__main__":
    sys.exit(main())
