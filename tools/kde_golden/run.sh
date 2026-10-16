#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech
#
# Builds the kde_golden image and regenerates every vector under vectors/.
#
#   tools/kde_golden/run.sh            # on the host: build, then run inside
#   tools/kde_golden/run.sh --inside   # in the container (run.sh does this)
#
# Needs docker (no sudo on the maintainer's machine). Not part of CI and not
# packaged; see README.md.

set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
image_tag=teksilo-kde-golden
base_digest=sha256:f58f9b97f218ee976cf928f6fe00abb929a644aded8c9dfb02f77a9b2a6e1732

if [[ ${1:-} != --inside ]]; then
    docker build -t "$image_tag" "$here"
    mkdir -p "$here/vectors"
    docker run --rm \
        --user "$(id -u):$(id -g)" \
        -e KDE_GOLDEN_IMAGE="invent-registry.kde.org/neon/docker-images/plasma@$base_digest" \
        -v "$here:/kde_golden:ro" \
        -v "$here/vectors:/out" \
        "$image_tag" \
        bash /kde_golden/run.sh --inside
    exit 0
fi

# ---- inside the container ----------------------------------------------------

out=/out
driver=/opt/kde_golden/build/kde_golden
[[ -x $driver ]] || { echo "run.sh: $driver missing; is this the kde_golden image?" >&2; exit 1; }

# A deterministic environment: no session, no platform theme, C locale, and
# every XDG directory a fresh scratch one (only XDG_DATA_DIRS keeps the system
# value, which is where the installed colour schemes live).
unset DISPLAY WAYLAND_DISPLAY KDE_FULL_SESSION KDE_SESSION_VERSION XDG_CURRENT_DESKTOP \
      QT_QPA_PLATFORMTHEME QT_STYLE_OVERRIDE QT_SCALE_FACTOR QT_SCREEN_SCALE_FACTORS \
      QT_FONT_DPI DBUS_SESSION_BUS_ADDRESS SESSION_MANAGER
export QT_QPA_PLATFORM=offscreen
export LC_ALL=C.UTF-8 LANG=C.UTF-8 LANGUAGE=
export XDG_DATA_DIRS=/usr/local/share:/usr/share

scratch=$(mktemp -d /tmp/kde_golden.XXXXXX)
trap 'rm -rf "$scratch"' EXIT

fresh_env() {
    # $1: name of a fresh scratch root; exports HOME and the XDG dirs into it.
    local root=$scratch/$1
    mkdir -p "$root"/{home,config,data,cache,etc-xdg,run}
    chmod 700 "$root/run"
    export HOME=$root/home XDG_CONFIG_HOME=$root/config XDG_DATA_HOME=$root/data \
           XDG_CACHE_HOME=$root/cache XDG_CONFIG_DIRS=$root/etc-xdg XDG_RUNTIME_DIR=$root/run
}

fresh_env math
"$driver" math "$out/math.json"

fresh_env resolve
"$driver" resolve "$out/resolve.json"

# The Breeze style reads kdeglobals itself, so give it the default scheme.
fresh_env sizes
cp /usr/share/color-schemes/BreezeLight.colors "$XDG_CONFIG_HOME/kdeglobals"
"$driver" sizes "$out/sizes.json"

for scheme in BreezeLight BreezeDark; do
    fresh_env "pixels-$scheme"
    cp "/usr/share/color-schemes/$scheme.colors" "$XDG_CONFIG_HOME/kdeglobals"
    "$driver" pixels "$scratch/pixels-$scheme.json" "$scheme"
done

python3 -I - "$out/pixels.json" "$scratch/pixels-BreezeLight.json" "$scratch/pixels-BreezeDark.json" <<'PY'
import json, sys

out, *parts = sys.argv[1:]
docs = [json.load(open(p, encoding="utf-8")) for p in parts]
with open(out, "w", encoding="utf-8") as f:
    f.write("{\n")
    f.write('"meta": %s,\n' % json.dumps(docs[0]["meta"], sort_keys=True))
    f.write('"schemes": {\n')
    for i, d in enumerate(docs):
        name = d["environment"]["scheme"]
        f.write('%s: {\n"environment": %s,\n"records": [\n' % (json.dumps(name), json.dumps(d["environment"], sort_keys=True)))
        recs = d["records"]
        for j, r in enumerate(recs):
            f.write(json.dumps(r, sort_keys=True) + ("," if j + 1 < len(recs) else "") + "\n")
        f.write("]\n}" + ("," if i + 1 < len(docs) else "") + "\n")
    f.write("}\n}\n")
PY

fresh_env accent
bash /kde_golden/accent_oracle.sh "$out/accent.json"

cp /opt/kde_golden/versions.txt "$out/versions.txt"
cat /opt/kde_golden/fonts.sha256 >"$out/fonts.sha256"
ls -l "$out"
