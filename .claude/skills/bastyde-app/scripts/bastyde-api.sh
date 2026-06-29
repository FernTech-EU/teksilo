#!/usr/bin/env bash
# bastyde-api.sh — the EXACT public API of a Bastyde widget for the version THIS app
# pins. Works from any app depending on `bastyde` / `bastyde-widgets`, whether the dep
# is a crates.io, git, or path dependency — including external adopters with NO checkout.
#
#   bastyde-api.sh <Widget> [<Widget> ...]   # e.g. Button HStack Dialog
#   bastyde-api.sh --list                    # every widget
#   bastyde-api.sh -f json ComboBox          # JSON for tooling
#   bastyde-api.sh --crate data ListModel    # other crates: widgets|data|settings|scene
#                                            #   (only those present in the dep graph)
#
# How it works
#   1. `cargo metadata` resolves where THIS app's bastyde-* crates live on disk.
#   2. If they resolve inside a Bastyde checkout that ships tools/, run the framework's
#      OWN extractor (authoritative, version-matched, richest output).
#   3. Otherwise (a crates.io dep) stage a throwaway monorepo and run the bundled,
#      verbatim copy of the extractor against the registry source — same curated output,
#      no checkout required.
#
# Run this from inside your app's crate or workspace.
set -euo pipefail

# --- prerequisites ---------------------------------------------------------------------
for bin in cargo python3; do
  command -v "$bin" >/dev/null 2>&1 || { echo "error: '$bin' is required but not on PATH." >&2; exit 1; }
done

# --- 1. resolve this app's dependency graph -------------------------------------------
echo "bastyde-api: resolving dependency graph (cargo metadata)…" >&2
META_ERR=$(mktemp "${TMPDIR:-/tmp}/bastyde-api.err.XXXXXX")
if ! META=$(cargo metadata --format-version 1 --quiet 2>"$META_ERR"); then
  echo "error: 'cargo metadata' failed — run this from inside your app's crate/workspace:" >&2
  sed 's/^/  /' "$META_ERR" >&2
  rm -f "$META_ERR"
  exit 1
fi
rm -f "$META_ERR"

# name<TAB>dir for every bastyde-* package resolved in this app's graph.
if ! PKGS=$(printf '%s' "$META" | python3 -c '
import json, os, sys
m = json.load(sys.stdin)
for p in m["packages"]:
    if p["name"].startswith("bastyde-"):
        print(p["name"] + "\t" + os.path.dirname(p["manifest_path"]))
'); then
  echo "error: could not parse cargo metadata output." >&2
  exit 1
fi

WIDGETS_DIR=$(printf '%s\n' "$PKGS" | awk -F'\t' '$1=="bastyde-widgets"{print $2; exit}')
if [ -z "${WIDGETS_DIR:-}" ]; then
  if [ -n "$PKGS" ]; then
    echo "error: 'bastyde' is in the dependency tree but the 'widgets' feature is disabled." >&2
    echo "       Enable it:  bastyde = { version = \"…\", features = [\"widgets\"] }" >&2
  else
    echo "error: 'bastyde' is not in this app's dependency tree (add it, or run from the app dir)." >&2
  fi
  exit 1
fi

# A freshly-cloned, never-built project may have resolved versions but no source on disk.
if [ ! -d "$WIDGETS_DIR/src" ]; then
  echo "bastyde-api: fetching crate sources (cargo fetch)…" >&2
  cargo fetch >/dev/null 2>&1 || true
fi
if [ ! -d "$WIDGETS_DIR/src" ]; then
  echo "error: bastyde-widgets source not found at '$WIDGETS_DIR/src' (try 'cargo fetch')." >&2
  exit 1
fi

# --- 2. authoritative path: a reachable checkout ships the canonical extractor. -------
REPO_ROOT=$(cd "$WIDGETS_DIR/../.." 2>/dev/null && pwd || true)
TOOL="${REPO_ROOT:-/nonexistent}/tools/extract_widget_api.py"
if [ -n "${REPO_ROOT:-}" ] && [ -f "$TOOL" ] && [ -d "$REPO_ROOT/crates/bastyde-widgets" ]; then
  exec python3 "$TOOL" "$@" || { echo "error: failed to run '$TOOL'." >&2; exit 1; }
fi

# --- 3. external path: stage a throwaway monorepo for the bundled extractor. -----------
# Resolve this script's real directory (it may be invoked via a symlink on $PATH).
SELF="${BASH_SOURCE[0]}"
while [ -L "$SELF" ]; do
  link="$(readlink "$SELF")"
  case "$link" in
    /*) SELF="$link" ;;
    *)  SELF="$(dirname "$SELF")/$link" ;;
  esac
done
HERE="$(cd "$(dirname "$SELF")" && pwd)"
BUNDLED="$HERE/extract_widget_api.py"
[ -f "$BUNDLED" ] || { echo "error: bundled extract_widget_api.py is missing next to this script ($HERE)." >&2; exit 1; }

STAGE=$(mktemp -d "${TMPDIR:-/tmp}/bastyde-api.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/tools" "$STAGE/crates"
# Copy (NOT symlink) the tool: it does Path(__file__).resolve(), so a symlinked tool would
# resolve REPO_ROOT back to the skill dir and look for crates/ there instead of in $STAGE.
cp "$BUNDLED" "$STAGE/tools/extract_widget_api.py"
# Symlink every resolved bastyde-* crate source into the fake monorepo layout, so
# `--crate widgets|data|settings|scene` all work when those crates are in the graph.
while IFS=$'\t' read -r name dir; do
  [ -n "$name" ] && [ -d "$dir/src" ] && ln -sfn "$dir" "$STAGE/crates/$name"
done <<< "$PKGS"

python3 "$STAGE/tools/extract_widget_api.py" "$@"
