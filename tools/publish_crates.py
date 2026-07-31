#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Publish the Bastyde workspace to crates.io, one crate at a time.

Drives ``cargo publish -p <crate>`` over the dependency-correct order
produced by :mod:`tools/check_release_order.py` (``--list``), and copes
with the two things that make a *first* publication awkward:

  * **crates.io rate-limits new crates.** The registry hands out a small
    burst of publishes and then refills one token every so often (at the
    time of writing: a burst of ~10 new crates, refilling ~1 per hour —
    but the script never hardcodes those numbers). With 40 crates in this
    workspace the run therefore spans many hours. On a 429 the server
    replies with the exact instant the next publish is allowed; this
    script parses that instant and sleeps until it, rather than guessing
    a backoff.

  * **The observed refill period is then reused as a pace.** Once a 429
    has been seen, the gap between the last accepted publish and the
    server's ``try again after`` instant *is* the refill period. The
    script remembers it and waits that long between subsequent publishes,
    so the remaining crates go through on the first attempt instead of
    burning a failed upload each time. ``--min-interval`` seeds that pace
    up front when you already know it; ``--max-interval`` caps a wildly
    large learned value.

Everything else is about being safely resumable across a run that long:

  * before each crate the script asks the sparse index whether that exact
    ``name@version`` is already up, and skips it if so — so re-running
    after a crash, a Ctrl-C, or a reboot picks up where it stopped;
  * a state file records what has been published plus the learned pace,
    so a resumed run does not immediately hammer the registry;
  * after each successful publish it waits for the version to actually
    appear in the index before moving on, because the next crate in the
    order may depend on it;
  * a ``crate version ... is already uploaded`` error is treated as
    success (the index is CDN-cached and can lag a stale 404).

Usage:
    python3 tools/publish_crates.py --plan          # show what would run
    python3 tools/publish_crates.py --dry-run       # cargo publish --dry-run
    python3 tools/publish_crates.py                 # the real thing
    python3 tools/publish_crates.py --limit 10      # just spend the burst
    python3 tools/publish_crates.py --start-at bastyde-widgets
    python3 tools/publish_crates.py --self-test     # parser unit checks

Authentication is cargo's: run ``cargo login`` first, or export
``CARGO_REGISTRY_TOKEN``. The script never sees or stores the token.

Note: cargo 1.90+ can publish a whole workspace in one go
(``cargo publish --workspace``), which handles ordering and index waits
itself — but it gives up on the first rate-limit refusal, which is fatal
for a 40-crate first publication. Hence this script.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field

SPARSE_INDEX = "https://index.crates.io"
USER_AGENT = "bastyde-publish-crates (https://github.com/ferntech-eu/bastyde)"

# --- Server-message recognition ---------------------------------------
#
# crates.io answers a throttled publish with HTTP 429 and a body along the
# lines of:
#
#   You have published too many new crates in a short period of time.
#   Please try again after 2026-07-23T15:04:05+0000 or email
#   help@crates.io to have your limit increased.
#
# The wording has changed over the years ("crates" vs "new crates" vs
# "versions of this crate"), so match the stable middle of the sentence
# rather than any one full phrasing.
_RATE_LIMIT_RES = [
    re.compile(r"too many (?:new )?crates? in a short period", re.I),
    re.compile(r"too many (?:versions|updates).{0,40}short period", re.I),
    re.compile(r"\b429\b|too many requests", re.I),
]
_ALREADY_UPLOADED_RE = re.compile(
    r"already (?:uploaded|exists)|crate version .* is already", re.I)

# A timestamp anywhere in the message. Accepts the RFC-3339-ish forms
# crates.io has used, with or without a zone: 2026-07-23T15:04:05+0000,
# ...Z, ...+00:00, and the space-separated "2026-07-23 15:04:05 UTC".
_TIMESTAMP_RE = re.compile(
    r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?"
    r"(?:\s*(?:Z|UTC|GMT|[+-]\d{2}:?\d{2}))?")
# Relative forms, in case a proxy or a future crates.io answers that way.
_RETRY_AFTER_SECS_RE = re.compile(
    r"retry[- ]after\s*[:=]?\s*(\d+)", re.I)
_TRY_AGAIN_IN_RE = re.compile(
    r"try again in\s+(\d+)\s*(second|minute|hour)", re.I)


def now_utc() -> dt.datetime:
    return dt.datetime.now(dt.timezone.utc)


# ----------------------------------------------------------------------
# Parsing helpers (pure — covered by --self-test)
# ----------------------------------------------------------------------

def is_rate_limited(text: str) -> bool:
    """True when cargo's output is a registry throttling refusal."""
    return any(rx.search(text) for rx in _RATE_LIMIT_RES)


def is_already_uploaded(text: str) -> bool:
    """True when the failure is 'this exact version is already on the registry'."""
    return bool(_ALREADY_UPLOADED_RE.search(text))


def parse_timestamp(raw: str) -> dt.datetime | None:
    """Parse one of the timestamp spellings crates.io emits, as aware UTC.

    A timestamp with no zone is read as UTC — that is what the registry
    means, and guessing local time would under-sleep by the UTC offset.
    """
    s = raw.strip()
    # Normalize the zone suffix into something %z accepts.
    s = re.sub(r"\s*(?:UTC|GMT)$", "+0000", s)
    s = re.sub(r"Z$", "+0000", s)
    s = re.sub(r"^(\d{4}-\d{2}-\d{2}) ", r"\1T", s)
    for fmt in ("%Y-%m-%dT%H:%M:%S%z", "%Y-%m-%dT%H:%M:%S.%f%z",
                "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M:%S.%f"):
        try:
            parsed = dt.datetime.strptime(s, fmt)
        except ValueError:
            continue
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=dt.timezone.utc)
        return parsed.astimezone(dt.timezone.utc)
    return None


def parse_retry_at(text: str, reference: dt.datetime | None = None
                   ) -> dt.datetime | None:
    """Extract the instant the registry says the next publish is allowed.

    Absolute timestamps win over relative ones (crates.io sends an
    absolute instant, which survives a slow upload better than a duration
    measured from when we happened to read it).
    """
    reference = reference or now_utc()
    m = _TIMESTAMP_RE.search(text)
    if m:
        parsed = parse_timestamp(m.group(0))
        if parsed:
            return parsed
    m = _RETRY_AFTER_SECS_RE.search(text)
    if m:
        return reference + dt.timedelta(seconds=int(m.group(1)))
    m = _TRY_AGAIN_IN_RE.search(text)
    if m:
        unit = {"second": 1, "minute": 60, "hour": 3600}[m.group(2).lower()]
        return reference + dt.timedelta(seconds=int(m.group(1)) * unit)
    return None


def index_path(name: str) -> str:
    """Sparse-index path for a crate, per cargo's own bucketing rules."""
    n = name.lower()
    if len(n) == 1:
        return f"1/{n}"
    if len(n) == 2:
        return f"2/{n}"
    if len(n) == 3:
        return f"3/{n[0]}/{n}"
    return f"{n[0:2]}/{n[2:4]}/{n}"


def human_duration(seconds: float) -> str:
    seconds = int(max(0, seconds))
    h, rem = divmod(seconds, 3600)
    m, s = divmod(rem, 60)
    if h:
        return f"{h}h{m:02d}m"
    if m:
        return f"{m}m{s:02d}s"
    return f"{s}s"


# ----------------------------------------------------------------------
# Registry queries
# ----------------------------------------------------------------------

def fetch_index_versions(name: str, timeout: float = 15.0) -> set[str] | None:
    """Versions of `name` present in the sparse index.

    Returns an empty set when the crate is unknown (404 — the normal case
    for a first publication), and ``None`` when the index could not be
    reached at all, so callers can tell "not published" from "don't know".
    """
    url = f"{SPARSE_INDEX}/{index_path(name)}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            return set()
        print(f"    warning: index lookup for {name} failed: HTTP {exc.code}",
              file=sys.stderr)
        return None
    except Exception as exc:  # noqa: BLE001 — network, DNS, TLS, timeouts
        print(f"    warning: index lookup for {name} failed: {exc}",
              file=sys.stderr)
        return None

    versions: set[str] = set()
    for line in body.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            versions.add(json.loads(line)["vers"])
        except (ValueError, KeyError):
            continue
    return versions


def wait_for_index(name: str, version: str, timeout: float,
                   poll: float = 5.0) -> bool:
    """Block until `name@version` is visible in the index (or `timeout`).

    The next crate in the release order may depend on this one, and cargo
    resolves that dependency from the registry, not from the workspace.
    cargo's own post-publish wait usually covers this; the extra poll
    turns "usually" into "checked".
    """
    deadline = time.monotonic() + timeout
    while True:
        versions = fetch_index_versions(name)
        if versions and version in versions:
            return True
        if time.monotonic() >= deadline:
            return False
        time.sleep(poll)


# ----------------------------------------------------------------------
# Workspace inspection
# ----------------------------------------------------------------------

def release_order(root: str) -> list[str]:
    """The publishable crates, dependencies first, from check_release_order.py."""
    script = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                          "check_release_order.py")
    if not os.path.exists(script):
        raise SystemExit(f"error: {script} not found — it produces the order")
    proc = subprocess.run(
        [sys.executable, script, "--root", root, "--list"],
        capture_output=True, text=True)
    names = [ln.strip() for ln in proc.stdout.splitlines() if ln.strip()]
    if proc.returncode != 0:
        # --list exits non-zero on a release-blocking cycle; publishing
        # into a cycle would strand the workspace half-uploaded.
        sys.stderr.write(proc.stderr)
        raise SystemExit("error: check_release_order.py reported a "
                         "release-blocking problem — refusing to publish")
    if not names:
        raise SystemExit("error: check_release_order.py listed no crates")
    return names


def crate_versions(root: str) -> dict[str, str]:
    """name -> version for every workspace member, via `cargo metadata`."""
    proc = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=root, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit("error: `cargo metadata` failed")
    meta = json.loads(proc.stdout)
    return {pkg["name"]: pkg["version"] for pkg in meta["packages"]}


# ----------------------------------------------------------------------
# Run state (resumable across crashes, reboots, and Ctrl-C)
# ----------------------------------------------------------------------

@dataclass
class State:
    path: str
    version: str = ""
    published: list[str] = field(default_factory=list)
    # Learned registry pace, in seconds: the gap the server enforced
    # between two accepted publishes. None until a 429 teaches us one.
    interval: float | None = None
    # Wall-clock (epoch seconds) of the last accepted publish. Kept across
    # runs so a resumed run does not immediately spend a token it doesn't
    # have.
    last_publish: float | None = None
    burst: int | None = None  # publishes accepted before the first 429

    @classmethod
    def load(cls, path: str, version: str) -> State:
        try:
            with open(path, encoding="utf-8") as fh:
                data = json.load(fh)
        except (OSError, ValueError):
            return cls(path=path, version=version)
        if data.get("version") != version:
            # A different workspace version: the published list refers to
            # other artifacts and must not gate this run.
            return cls(path=path, version=version)
        return cls(path=path, version=version,
                   published=list(data.get("published", [])),
                   interval=data.get("interval"),
                   last_publish=data.get("last_publish"),
                   burst=data.get("burst"))

    def save(self) -> None:
        os.makedirs(os.path.dirname(self.path) or ".", exist_ok=True)
        tmp = self.path + ".tmp"
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump({"version": self.version, "published": self.published,
                       "interval": self.interval,
                       "last_publish": self.last_publish,
                       "burst": self.burst}, fh, indent=2)
        os.replace(tmp, self.path)  # atomic: a Ctrl-C never truncates it


# ----------------------------------------------------------------------
# Publishing
# ----------------------------------------------------------------------

def log(msg: str) -> None:
    print(f"[{dt.datetime.now().strftime('%H:%M:%S')}] {msg}", flush=True)


def sleep_until(deadline: dt.datetime, reason: str) -> None:
    """Sleep to `deadline`, printing an occasional countdown.

    Chunked rather than one long `time.sleep` so Ctrl-C is responsive and
    an hours-long wait shows signs of life.
    """
    total = (deadline - now_utc()).total_seconds()
    if total <= 0:
        return
    log(f"{reason}: waiting {human_duration(total)} "
        f"(until {deadline.astimezone().strftime('%Y-%m-%d %H:%M:%S %Z')})")
    next_report = time.monotonic() + 300
    while True:
        remaining = (deadline - now_utc()).total_seconds()
        if remaining <= 0:
            return
        time.sleep(min(5.0, remaining))
        if time.monotonic() >= next_report and remaining > 60:
            log(f"    still waiting — {human_duration(remaining)} to go")
            next_report = time.monotonic() + 300


def run_cargo_publish(root: str, name: str, dry_run: bool,
                      extra: list[str]) -> tuple[int, str]:
    """Run cargo publish for one crate, teeing its output. Returns (rc, output)."""
    cmd = ["cargo", "publish", "-p", name]
    if dry_run:
        cmd.append("--dry-run")
    cmd.extend(extra)
    log(f"$ {' '.join(cmd)}")
    proc = subprocess.Popen(cmd, cwd=root, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True,
                            bufsize=1)
    chunks: list[str] = []
    assert proc.stdout is not None
    for line in proc.stdout:
        chunks.append(line)
        sys.stdout.write("    " + line)
        sys.stdout.flush()
    proc.wait()
    return proc.returncode, "".join(chunks)


@dataclass
class Options:
    root: str
    dry_run: bool
    plan: bool
    limit: int | None
    min_interval: float | None
    max_interval: float
    max_wait: float
    retry_margin: float
    index_timeout: float
    skip_index_check: bool
    keep_going: bool
    extra_cargo: list[str]


def publish_all(order: list[str], versions: dict[str, str],
                state: State, opt: Options) -> int:
    if opt.min_interval is not None and (state.interval is None
                                         or state.interval < opt.min_interval):
        state.interval = opt.min_interval

    accepted_this_run = 0
    failures: list[str] = []
    total = len(order)

    for idx, name in enumerate(order, 1):
        version = versions.get(name)
        if version is None:
            log(f"({idx}/{total}) {name}: SKIP — not a workspace member "
                f"(cargo metadata does not know it)")
            failures.append(name)
            continue

        head = f"({idx}/{total}) {name} {version}"

        if name in state.published:
            log(f"{head}: already published in a previous run — skipping")
            continue

        if not opt.skip_index_check:
            known = fetch_index_versions(name)
            if known is not None and version in known:
                log(f"{head}: already on crates.io — skipping")
                state.published.append(name)
                state.save()
                continue

        if opt.plan:
            log(f"{head}: would publish")
            continue

        if opt.limit is not None and accepted_this_run >= opt.limit:
            log(f"{head}: --limit {opt.limit} reached — stopping here")
            break

        # --- Proactive pacing -------------------------------------------
        # Once the server has taught us its refill period, wait it out
        # instead of offering an upload that is certain to be refused.
        # Set when the registry actually accepted an upload, as opposed to
        # telling us this version was already there — a rejected duplicate
        # spends no rate-limit token, so it must not restart the pacing
        # clock and cost us an idle hour.
        consumed_token = False

        while True:
            if (state.interval and state.last_publish is not None
                    and not opt.dry_run):
                due = dt.datetime.fromtimestamp(
                    state.last_publish + state.interval, dt.timezone.utc)
                if due > now_utc():
                    sleep_until(due, f"{name}: pacing to the registry's "
                                     f"{human_duration(state.interval)} refill")

            log(f"{head}: publishing")
            rc, out = run_cargo_publish(opt.root, name, opt.dry_run,
                                        opt.extra_cargo)

            if rc == 0:
                consumed_token = True
                break

            if is_already_uploaded(out):
                # The pre-flight index check can see a stale CDN 404; the
                # registry is the authority and it says this is done.
                log(f"{head}: already uploaded (registry) — treating as done")
                rc = 0
                break

            if is_rate_limited(out):
                retry_at = parse_retry_at(out)
                if retry_at is None:
                    # Throttled but no parseable instant: fall back to the
                    # learned pace, else a conservative hour.
                    fallback = state.interval or 3600.0
                    retry_at = now_utc() + dt.timedelta(seconds=fallback)
                    log(f"{head}: rate-limited, no retry instant in the "
                        f"message — backing off {human_duration(fallback)}")
                retry_at += dt.timedelta(seconds=opt.retry_margin)

                # Learn the refill period: with the bucket empty, the gap
                # from the last accepted publish to the server's "try
                # again after" IS one refill.
                if state.last_publish is not None:
                    learned = retry_at.timestamp() - state.last_publish
                    if 0 < learned <= opt.max_interval:
                        if state.interval is None or learned > state.interval:
                            state.interval = learned
                            log(f"    learned registry pace: "
                                f"{human_duration(learned)} between publishes")
                if state.burst is None:
                    state.burst = len(state.published)
                    log(f"    registry accepted {state.burst} publish(es) "
                        f"before throttling")
                state.save()

                wait = (retry_at - now_utc()).total_seconds()
                if wait > opt.max_wait:
                    log(f"{head}: registry asks for {human_duration(wait)}, "
                        f"over --max-wait {human_duration(opt.max_wait)} — "
                        f"stopping. Re-run to resume.")
                    return 1
                sleep_until(retry_at, f"{name}: registry rate limit")
                continue  # retry the same crate

            # Anything else is a real failure: a build error, a bad
            # manifest, a missing token, a dirty tree.
            log(f"{head}: FAILED (exit {rc})")
            failures.append(name)
            if not opt.keep_going:
                log("stopping (pass --keep-going to continue past failures)")
                state.save()
                return 1
            break

        if rc != 0:
            continue

        if opt.dry_run:
            log(f"{head}: dry-run OK")
            continue

        state.published.append(name)
        if consumed_token:
            state.last_publish = time.time()
            accepted_this_run += 1
        state.save()
        log(f"{head}: published")

        # The next crate may depend on this one, and cargo resolves that
        # from the registry.
        if not opt.skip_index_check and idx < total:
            if wait_for_index(name, version, opt.index_timeout):
                log(f"{head}: visible in the index")
            else:
                log(f"{head}: warning — not visible in the index after "
                    f"{human_duration(opt.index_timeout)}; "
                    f"a dependent crate may fail to resolve it")

    state.save()

    remaining = [n for n in order if n not in state.published
                 and n not in failures]
    log("")
    log(f"done: {len(state.published)}/{total} published"
        + (f", {len(failures)} failed" if failures else "")
        + (f", {len(remaining)} remaining" if remaining else ""))
    if failures:
        log(f"failed: {', '.join(failures)}")
    if remaining and not opt.plan and not opt.dry_run:
        log(f"remaining: {', '.join(remaining)}")
        log("re-run this script to continue where it stopped")
    return 1 if failures else 0


# ----------------------------------------------------------------------
# Self-test (pure parsers only — no network, no cargo)
# ----------------------------------------------------------------------

def self_test() -> int:
    failures = 0

    def check(label: str, got, want) -> None:
        nonlocal failures
        if got != want:
            print(f"FAIL {label}: got {got!r}, want {want!r}")
            failures += 1

    check("index_path 4+", index_path("bastyde-core"), "ba/st/bastyde-core")
    check("index_path 3", index_path("abc"), "3/a/abc")
    check("index_path 2", index_path("ab"), "2/ab")
    check("index_path 1", index_path("a"), "1/a")
    check("index_path case", index_path("Bastyde"), "ba/st/bastyde")

    utc = dt.timezone.utc
    check("ts offset", parse_timestamp("2026-07-23T15:04:05+0000"),
          dt.datetime(2026, 7, 23, 15, 4, 5, tzinfo=utc))
    check("ts colon offset", parse_timestamp("2026-07-23T15:04:05+00:00"),
          dt.datetime(2026, 7, 23, 15, 4, 5, tzinfo=utc))
    check("ts zulu", parse_timestamp("2026-07-23T15:04:05Z"),
          dt.datetime(2026, 7, 23, 15, 4, 5, tzinfo=utc))
    check("ts space UTC", parse_timestamp("2026-07-23 15:04:05 UTC"),
          dt.datetime(2026, 7, 23, 15, 4, 5, tzinfo=utc))
    check("ts naive is UTC", parse_timestamp("2026-07-23T15:04:05"),
          dt.datetime(2026, 7, 23, 15, 4, 5, tzinfo=utc))
    check("ts fractional", parse_timestamp("2026-07-23T15:04:05.123456+0000"),
          dt.datetime(2026, 7, 23, 15, 4, 5, 123456, tzinfo=utc))
    check("ts garbage", parse_timestamp("not a date"), None)

    msg_new = ("error: failed to publish to registry at https://crates.io\n"
               "Caused by:\n  the remote server responded with an error "
               "(status 429 Too Many Requests): You have published too many "
               "new crates in a short period of time. Please try again after "
               "2026-07-23T15:04:05+0000 or email help@crates.io to have your "
               "limit increased.")
    check("rate limit detected", is_rate_limited(msg_new), True)
    check("rate limit instant", parse_retry_at(msg_new),
          dt.datetime(2026, 7, 23, 15, 4, 5, tzinfo=utc))
    check("rate limit not already-uploaded", is_already_uploaded(msg_new), False)

    msg_existing = ("You have published too many versions of this crate in a "
                    "short period of time. Please try again after "
                    "2026-07-23 15:04:05 UTC or email help@crates.io")
    check("versions phrasing detected", is_rate_limited(msg_existing), True)
    check("versions phrasing instant", parse_retry_at(msg_existing),
          dt.datetime(2026, 7, 23, 15, 4, 5, tzinfo=utc))

    ref = dt.datetime(2026, 1, 1, tzinfo=utc)
    check("retry-after header", parse_retry_at("Retry-After: 90", ref),
          ref + dt.timedelta(seconds=90))
    check("try again in", parse_retry_at("please try again in 5 minutes", ref),
          ref + dt.timedelta(minutes=5))

    dup = ("error: failed to publish to registry at https://crates.io\n"
           "Caused by:\n  the remote server responded with an error: "
           "crate version `0.6.2` is already uploaded")
    check("already uploaded", is_already_uploaded(dup), True)
    check("already uploaded is not a rate limit", is_rate_limited(dup), False)

    build_err = "error[E0432]: unresolved import `foo::bar`"
    check("build error is not a rate limit", is_rate_limited(build_err), False)
    check("build error is not already-uploaded",
          is_already_uploaded(build_err), False)

    check("duration hours", human_duration(3725), "1h02m")
    check("duration minutes", human_duration(125), "2m05s")
    check("duration seconds", human_duration(9), "9s")

    print("self-test: " + ("OK" if not failures else f"{failures} failure(s)"))
    return 1 if failures else 0


# ----------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", default=None,
                    help="workspace root (default: parent of this script's dir)")
    ap.add_argument("--plan", action="store_true",
                    help="list what would be published (queries the index, "
                         "never runs cargo publish)")
    ap.add_argument("--dry-run", action="store_true",
                    help="pass --dry-run to cargo publish. Only meaningful "
                         "for crates whose dependencies are already on "
                         "crates.io — verification resolves from the registry")
    ap.add_argument("--yes", "-y", action="store_true",
                    help="skip the confirmation prompt")
    ap.add_argument("--limit", type=int, default=None,
                    help="stop after N successful publishes this run (e.g. "
                         "--limit 10 to spend the burst and come back later)")
    ap.add_argument("--start-at", metavar="CRATE",
                    help="skip everything before CRATE in the release order")
    ap.add_argument("--only", metavar="LIST",
                    help="comma-separated crates to publish (still in "
                         "release order)")
    ap.add_argument("--min-interval", type=float, default=None, metavar="SECS",
                    help="seed the pace between publishes when you already "
                         "know the registry's refill period (e.g. 3600)")
    ap.add_argument("--max-interval", type=float, default=6 * 3600,
                    metavar="SECS",
                    help="reject a learned pace longer than this "
                         "(default: 6h — guards against a bogus timestamp)")
    ap.add_argument("--max-wait", type=float, default=24 * 3600, metavar="SECS",
                    help="stop instead of sleeping longer than this for one "
                         "rate limit (default: 24h)")
    ap.add_argument("--retry-margin", type=float, default=15.0, metavar="SECS",
                    help="extra seconds added to the server's retry instant, "
                         "for clock skew (default: 15)")
    ap.add_argument("--index-timeout", type=float, default=300.0,
                    metavar="SECS",
                    help="how long to wait for a published version to appear "
                         "in the sparse index (default: 300)")
    ap.add_argument("--skip-index-check", action="store_true",
                    help="do not query the sparse index at all (offline / "
                         "rely purely on the state file)")
    ap.add_argument("--keep-going", action="store_true",
                    help="continue after a non-rate-limit failure")
    ap.add_argument("--state", default=None, metavar="PATH",
                    help="state file (default: "
                         "<root>/target/publish-crates-state.json)")
    ap.add_argument("--cargo-arg", action="append", default=[], metavar="ARG",
                    help="extra argument for cargo publish (repeatable), "
                         "e.g. --cargo-arg=--no-verify")
    ap.add_argument("--self-test", action="store_true",
                    help="run the parser unit checks and exit")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    if args.root:
        root = os.path.abspath(args.root)
    else:
        root = os.path.abspath(os.path.join(os.path.dirname(__file__),
                                            os.pardir))

    order = release_order(root)
    versions = crate_versions(root)

    if args.only:
        wanted = {n.strip() for n in args.only.split(",") if n.strip()}
        unknown = wanted - set(order)
        if unknown:
            raise SystemExit("error: not publishable crates in this "
                             f"workspace: {', '.join(sorted(unknown))}")
        order = [n for n in order if n in wanted]
    if args.start_at:
        if args.start_at not in order:
            raise SystemExit(f"error: --start-at {args.start_at} is not in "
                             "the release order")
        order = order[order.index(args.start_at):]

    # Shared-version workspace (release.toml sets `shared-version = true`),
    # so any member's version keys the state file; prefer the umbrella.
    ws_version = versions.get("bastyde") or versions.get(order[0], "")
    state_path = args.state or os.path.join(root, "target",
                                            "publish-crates-state.json")
    state = State.load(state_path, ws_version)

    opt = Options(root=root, dry_run=args.dry_run, plan=args.plan,
                  limit=args.limit, min_interval=args.min_interval,
                  max_interval=args.max_interval, max_wait=args.max_wait,
                  retry_margin=args.retry_margin,
                  index_timeout=args.index_timeout,
                  skip_index_check=args.skip_index_check,
                  keep_going=args.keep_going, extra_cargo=args.cargo_arg)

    log(f"workspace: {root}")
    log(f"crates: {len(order)} in release order, version {ws_version}")
    if state.published:
        log(f"state: {len(state.published)} already published "
            f"(from {state_path})")
    if state.interval:
        log(f"state: learned registry pace {human_duration(state.interval)}")

    if not (args.plan or args.dry_run):
        if not (os.environ.get("CARGO_REGISTRY_TOKEN")
                or os.path.exists(os.path.expanduser(
                    "~/.cargo/credentials.toml"))
                or os.path.exists(os.path.expanduser("~/.cargo/credentials"))):
            log("warning: no CARGO_REGISTRY_TOKEN and no ~/.cargo/credentials"
                " — run `cargo login` first")
        if not args.yes:
            pending = [n for n in order if n not in state.published]
            print(f"\nAbout to publish {len(pending)} crate(s) at version "
                  f"{ws_version} to crates.io.")
            print("Publishing is PERMANENT — a version can be yanked but "
                  "never replaced or deleted.")
            try:
                reply = input("Type 'publish' to continue: ").strip()
            except (EOFError, KeyboardInterrupt):
                print()
                return 130
            if reply != "publish":
                print("aborted")
                return 130

    try:
        return publish_all(order, versions, state, opt)
    except KeyboardInterrupt:
        print()
        state.save()
        log("interrupted — state saved; re-run this script to resume")
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
