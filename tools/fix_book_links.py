#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Rewrite Markdown links that don't resolve inside the mdBook build.

The `docs/*.md` guides are authored for GitHub viewing, so they link to source
with repo-relative paths like `[button.rs](../crates/.../button.rs)`, occasionally
carry rustdoc intra-doc links (`[`X`](crate::..)`, `[`X`]: Self::..`), and link to
the TOC (`SUMMARY.md`) or to docs that were never written. None of those resolve
inside the rendered book. This script fixes every `*.md` under the given
directories IN PLACE — run it after generating the Widget Catalog and before
`mdbook build` (this is what `.github/workflows/docs.yml` does; see CLAUDE.md).

Per link target, relative to the file it appears in:

  * web URL / `#anchor` / `mailto:` / our own `../api/...`   -> keep
  * `SUMMARY.md`                                             -> `introduction.md`
  * `*.md` / `*.html` that exists in docs/ (a real chapter)  -> keep
  * `*.md` / `*.html` that does NOT exist (dangling ref)     -> de-link (plain text)
  * a relative source/asset path (`../crates/..`, `locales/x.ftl`, …) -> GitHub URL
  * a rustdoc path (`crate::X`, `Self::y`, `self`, bare `TreeView`)   -> inline code

It is idempotent (rewritten links no longer match) and mirrors qleany's CI `sed`
link-fixup, generalized for Teksilo's source and rustdoc links.

Usage:
    python3 tools/fix_book_links.py docs
    python3 tools/fix_book_links.py docs --repo-url https://github.com/ferntech-eu/teksilo --branch main
    python3 tools/fix_book_links.py --test
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent

DEFAULT_REPO_URL = "https://github.com/ferntech-eu/teksilo"
DEFAULT_BRANCH = "main"

# The book's own rustdoc tree, assembled by .github/workflows/docs.yml as
# `cp -r target/doc book/api`, holds exactly these crates. A generated catalog
# page is COMMITTED carrying the crate's docs.rs URL, so it resolves for a
# reader on GitHub where no rustdoc tree exists; inside the book the local tree
# is the better target (built from this branch, and no trip off the site), so
# the link is pointed back at /api/ here. Keep this list in step with
# `CRATE_SPECS` in extract_widget_api.py and with the `cargo doc` line in the
# workflow: a crate listed here but not built would gain a dead link, which is
# why this is an explicit list rather than a `teksilo-*` pattern.
BOOK_SRC = REPO_ROOT / "docs"
_API_CRATES = ("teksilo-widgets", "teksilo-data", "teksilo-settings", "teksilo-scene")
_DOCS_RS_RE = re.compile(
    r"^https://docs\.rs/(?:" + "|".join(re.escape(c) for c in _API_CRATES) + r")/latest/(.+)$"
)

_INLINE_RE = re.compile(r'\[([^\]]+)\]\(([^)\s]+)(\s+"[^"]*")?\)')
_REFDEF_RE = re.compile(r'^(\s*)\[([^\]]+)\]:\s*(\S+)(.*)$')
_EXT_RE = re.compile(r"\.[A-Za-z0-9]{1,6}$")


def _as_code(label: str) -> str:
    label = label.strip()
    if label.startswith("`") and label.endswith("`"):
        return label
    return f"`{label}`"


def _api_prefix(src_dir: Path) -> "str | None":
    """Book-relative prefix reaching `api/` from a page in `src_dir`.

    `docs/foo.md` -> `api/`, `docs/widgets/button.md` -> `../api/`. Returns
    None for a directory outside the book source, where there is no such path.
    """
    try:
        depth = len(src_dir.resolve().relative_to(BOOK_SRC).parts)
    except ValueError:
        return None
    return "../" * depth + "api/"


def _classify(target: str, src_dir: Path, base: str) -> tuple[str, str | None]:
    """Return (action, new_target). action ∈ {keep, replace, github, strip}."""
    t = target.strip()

    # docs.rs (the committed, GitHub-correct form) -> the book's own /api/ tree.
    # Checked before the web-URL keep below, which would otherwise pass it through.
    m = _DOCS_RS_RE.match(t)
    if m:
        prefix = _api_prefix(src_dir)
        if prefix is not None:
            return ("replace", prefix + m.group(1))
        return ("keep", t)

    if (
        t.startswith("#")
        or "://" in t
        or t.startswith("mailto:")
        or re.match(r"(\.\./)*api/", t)  # the in-book rustdoc tree, at any depth
        # Page-local images (the generated catalog previews under
        # `docs/*/img/`). mdBook copies them into the build output, so the
        # relative path already resolves; rewriting one to a GitHub *blob*
        # URL would swap the picture for a link to an HTML page.
        or re.match(r"(\.\./)*img/", t)
    ):
        return ("keep", t)

    path, _, frag = t.partition("#")
    anchor = f"#{frag}" if frag else ""

    if path == "SUMMARY.md":
        # mdBook has no page for SUMMARY.md; the docs index is the landing page,
        # which mdBook emits as index.html (from the prefix `introduction.md`).
        return ("replace", f"index.html{anchor}")

    if path.endswith(".md") or path.endswith(".html"):
        cand = Path(os.path.normpath(str(src_dir / path)))
        # mdBook renders chapters from their .md source; treat an existing source
        # .md/.html as a live chapter, a missing one as a dangling ref.
        if cand.exists() or cand.with_suffix(".md").exists():
            return ("keep", t)
        return ("strip", None)

    if "/" in path or _EXT_RE.search(path):
        # A relative source / asset path -> absolute GitHub blob URL.
        norm = Path(os.path.normpath(str(src_dir / path)))
        try:
            rel = norm.relative_to(REPO_ROOT)
        except ValueError:
            return ("strip", None)
        return ("github", f"{base}{rel.as_posix()}{anchor}")

    # Bare identifier / rustdoc path (crate::X, Self::y, self, TreeView) -> code.
    return ("strip", None)


def fix_text(text: str, repo_url: str, branch: str, src_path: Path) -> str:
    # Resolve to an absolute path so source links map under REPO_ROOT even when
    # the caller passes a relative path (e.g. `docs/architecture.md`).
    src_path = Path(src_path).resolve()
    base = f"{repo_url.rstrip('/')}/blob/{branch}/"
    src_dir = src_path.parent

    # Pass 1: reference DEFINITIONS.
    stripped_labels: set[str] = set()
    kept: list[str] = []
    for ln in text.split("\n"):
        m = _REFDEF_RE.match(ln)
        if m:
            indent, lbl, target, rest = m.groups()
            action, new = _classify(target, src_dir, base)
            if action == "strip":
                stripped_labels.add(lbl.strip())
                continue
            if action in ("github", "replace"):
                kept.append(f"{indent}[{lbl}]: {new}{rest}")
                continue
        kept.append(ln)
    text = "\n".join(kept)

    # Pass 2: inline links.
    def repl(m: "re.Match[str]") -> str:
        label, target, title = m.group(1), m.group(2), m.group(3) or ""
        action, new = _classify(target, src_dir, base)
        if action == "strip":
            return _as_code(label)
        if action in ("github", "replace"):
            return f"[{label}]({new}{title})"
        return m.group(0)

    text = _INLINE_RE.sub(repl, text)

    # Pass 3: de-link shortcut / collapsed usages of the stripped reference labels.
    for lbl in stripped_labels:
        code = _as_code(lbl)
        text = text.replace(f"[{lbl}][]", code)
        text = re.sub(re.escape(f"[{lbl}]") + r"(?![\(\[])", code, text)
    return text


def fix_dirs(dirs: list[str], repo_url: str, branch: str) -> int:
    changed = total = 0
    for d in dirs:
        for md in sorted(Path(d).rglob("*.md")):
            total += 1
            before = md.read_text(encoding="utf-8")
            after = fix_text(before, repo_url, branch, md)
            if after != before:
                md.write_text(after, encoding="utf-8")
                changed += 1
    print(f"fix_book_links: rewrote links in {changed}/{total} Markdown files", file=sys.stderr)
    return 0


def _self_test() -> int:
    r, b = DEFAULT_REPO_URL, DEFAULT_BRANCH
    src = REPO_ROOT / "docs" / "architecture.md"  # a real guide path, dir = docs/

    def fx(s):
        return fix_text(s, r, b, src)

    # docs.rs (the committed catalog form) -> the book's own /api/ tree, at the
    # depth of the page it appears in.
    page = REPO_ROOT / "docs" / "widgets" / "center_button.md"
    out = fix_text(
        "[API](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/"
        "notification/center_button/index.html)",
        r, b, page,
    )
    assert out == "[API](../api/teksilo_widgets/notification/center_button/index.html)", out
    # A guide sits one level higher, so it reaches api/ without the ../.
    out = fx("[API](https://docs.rs/teksilo-data/latest/teksilo_data/index.html)")
    assert out == "[API](api/teksilo_data/index.html)", out
    # Already rewritten: idempotent, and still kept by the ../api/ rule.
    assert fx("[API](api/teksilo_data/index.html)") == "[API](api/teksilo_data/index.html)"
    # A crate the book does NOT ship rustdoc for keeps its docs.rs URL rather
    # than gaining a link into a tree that has no page for it.
    keep = "[c](https://docs.rs/teksilo-core/latest/teksilo_core/index.html)"
    assert fx(keep) == keep, fx(keep)
    # A third-party docs.rs link is untouched (docs/terminal.md has two).
    keep = "[pty](https://docs.rs/portable-pty)"
    assert fx(keep) == keep, fx(keep)

    # source link (one ../) -> GitHub blob URL, anchor preserved
    out = fx("see [button.rs](../crates/teksilo-widgets/src/button.rs#L42)")
    assert out == f"see [button.rs]({r}/blob/{b}/crates/teksilo-widgets/src/button.rs#L42)", out
    # the same with a RELATIVE src_path (how fix_dirs calls it) must also resolve
    rel = fix_text(
        "[x](../crates/teksilo-widgets/src/button.rs)", r, b, Path("docs/architecture.md")
    )
    assert rel == f"[x]({r}/blob/{b}/crates/teksilo-widgets/src/button.rs)", rel
    # the in-book rustdoc reference is left alone
    assert fx("[API](../api/x/index.html)") == "[API](../api/x/index.html)"
    # an existing chapter (.md present in docs/) is kept
    assert fx("[arch](architecture.md)") == "[arch](architecture.md)"
    # a dangling chapter link is de-linked
    assert fx("see [themes](image-themes.md) now") == "see `themes` now"
    # SUMMARY.md -> index.html (mdBook's landing page)
    assert fx("[index](SUMMARY.md)") == "[index](index.html)"
    # intra-doc inline + bare + reference form
    assert fx("use [`HStack`](crate::primitives::HStack) and [a](self) and [b](TreeView)") == (
        "use `HStack` and `a` and `b`"
    )
    ri = fx("see [`Ref`] now.\n\n[`Ref`]: crate::Ref")
    assert "[`Ref`]:" not in ri and "see `Ref` now." in ri, ri
    # reference-style source def -> GitHub URL (body shortcut still resolves)
    img = fx("![Button preview](img/button.png)")
    assert img == "![Button preview](img/button.png)", img

    rd = fx("see [`T`].\n\n[`T`]: ../crates/teksilo-widgets/src/toolbar.rs")
    assert f"[`T`]: {r}/blob/{b}/crates/teksilo-widgets/src/toolbar.rs" in rd, rd
    # idempotent
    assert fx(out) == out, "not idempotent"
    print("fix_book_links self-tests passed.", file=sys.stderr)
    return 0


def main(argv: list[str]) -> int:
    p = argparse.ArgumentParser(description="Fix Markdown links for the mdBook build.")
    p.add_argument("dirs", nargs="*", default=["docs"], help="Directories to scan (default: docs).")
    p.add_argument("--repo-url", default=DEFAULT_REPO_URL)
    p.add_argument("--branch", default=DEFAULT_BRANCH)
    p.add_argument("--test", action="store_true", help=argparse.SUPPRESS)
    args = p.parse_args(argv)
    if args.test:
        return _self_test()
    return fix_dirs(args.dirs or ["docs"], args.repo_url, args.branch)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
