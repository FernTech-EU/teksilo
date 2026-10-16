#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech
#
# Accent oracle: what plasma-apply-colorscheme writes to kdeglobals when an
# accent colour is applied. Runs INSIDE the kde_golden container (run.sh
# calls it); never on a desktop, because the tool rewrites the user's
# kdeglobals.
#
#   accent_oracle.sh OUT.json
#
# Requirements, checked before anything runs:
#   - XDG_CONFIG_HOME and XDG_DATA_HOME are set, are directories, and are
#     each either empty or marked as scratch by a .kde_golden_scratch file;
#   - QT_QPA_PLATFORM=offscreen;
#   - dbus-run-session and plasma-apply-colorscheme are installed.
#
# Procedure per case (one scheme, one accent), each in its own config dir
# under XDG_CONFIG_HOME:
#   1. kdeglobals is seeded with only "[General] ColorScheme=KdeGoldenNone",
#      a name no installed scheme has, because the tool refuses to apply the
#      scheme that is already current;
#   2. "plasma-apply-colorscheme SCHEME" applies the scheme (snapshot "base");
#   3. "plasma-apply-colorscheme -a ACCENT" applies the accent to the current
#      scheme (snapshot "kdeglobals"). With -a the tool ignores any scheme
#      name argument and always uses the current scheme.
# Each command runs in its own dbus-run-session.

set -euo pipefail

out=${1:?usage: accent_oracle.sh OUT.json}

die() {
    echo "accent_oracle.sh: $*" >&2
    exit 1
}

check_scratch() {
    local name=$1 dir=${!1:-}
    [[ -n $dir ]] || die "$name is not set; refusing to touch a real configuration"
    [[ -d $dir ]] || die "$name=$dir is not a directory"
    [[ $dir == /* ]] || die "$name=$dir is not absolute"
    if [[ -n $(ls -A "$dir") && ! -e $dir/.kde_golden_scratch ]]; then
        die "$name=$dir is neither empty nor marked with .kde_golden_scratch; refusing"
    fi
    touch "$dir/.kde_golden_scratch"
}

check_scratch XDG_CONFIG_HOME
check_scratch XDG_DATA_HOME
[[ ${QT_QPA_PLATFORM:-} == offscreen ]] || die "QT_QPA_PLATFORM must be offscreen"
command -v dbus-run-session >/dev/null || die "dbus-run-session is not installed"
command -v plasma-apply-colorscheme >/dev/null || die "plasma-apply-colorscheme is not installed"
command -v kwriteconfig6 >/dev/null || die "kwriteconfig6 is not installed"

accents=(e93a9a e93d58 e9643a e8cb2d 3dd425 00d3b8 3daee9 b875dc 926ee4 686b6f)
work=$XDG_CONFIG_HOME
schemes_dir=$XDG_DATA_HOME/color-schemes
mkdir -p "$schemes_dir"

# Two derived schemes, built in the scratch data dir from the installed files
# (never copied into the repository): one with a TintFactor, one with the
# title bar accent-coloured.
derive() {
    local name=$1 from=$2 key=$3 value=$4
    cp "/usr/share/color-schemes/$from.colors" "$schemes_dir/$name.colors"
    kwriteconfig6 --file "$schemes_dir/$name.colors" --group General --key Name "$name"
    kwriteconfig6 --file "$schemes_dir/$name.colors" --group General --key ColorScheme "$name"
    kwriteconfig6 --file "$schemes_dir/$name.colors" --group General --key "$key" "$value"
}
derive KdeGoldenTintLight BreezeLight TintFactor 0.25
derive KdeGoldenTitlebarDark BreezeDark TitlebarIsAccentColored true

schemes=(BreezeLight BreezeDark KdeGoldenTintLight KdeGoldenTitlebarDark)
manifest=$work/manifest.tsv
: >"$manifest"

n=0
for scheme in "${schemes[@]}"; do
    for accent in "${accents[@]}"; do
        n=$((n + 1))
        case_dir=$work/case-$(printf %02d "$n")
        mkdir -p "$case_dir/config"
        printf '[General]\nColorScheme=KdeGoldenNone\n' >"$case_dir/config/kdeglobals"
        XDG_CONFIG_HOME=$case_dir/config dbus-run-session -- \
            plasma-apply-colorscheme "$scheme" >"$case_dir/apply.out" 2>"$case_dir/apply.err" || true
        grep -q "Successfully applied the color scheme" "$case_dir/apply.out" ||
            die "applying $scheme failed: $(cat "$case_dir/apply.out" "$case_dir/apply.err")"
        cp "$case_dir/config/kdeglobals" "$case_dir/base.kdeglobals"
        XDG_CONFIG_HOME=$case_dir/config dbus-run-session -- \
            plasma-apply-colorscheme -a "#$accent" >"$case_dir/accent.out" 2>"$case_dir/accent.err" || true
        grep -q "Successfully applied the accent color" "$case_dir/accent.out" ||
            die "accent #$accent on $scheme failed: $(cat "$case_dir/accent.out" "$case_dir/accent.err")"
        printf '%s\t%s\t%s\n' "$scheme" "#$accent" "$case_dir" >>"$manifest"
    done
done

python3 -I - "$manifest" "$schemes_dir" "$out" <<'PY'
import hashlib, json, os, sys

manifest, schemes_dir, out = sys.argv[1:4]


def parse(path):
    """kdeglobals / .colors -> {group: {key: raw value}}, file order kept.

    Group headers like [Colors:Header][Inactive] become "Colors:Header][Inactive".
    Keys keep any [$x] flags verbatim. Values are the raw text after the
    first '=' (KConfig strips surrounding whitespace, and so does this).
    Localised keys (Name[de]) are dropped: they carry translations only.
    """
    groups, cur = {}, None
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            s = line.strip()
            if not s or s.startswith("#"):
                continue
            if s.startswith("[") and s.endswith("]"):
                cur = s[1:-1]
                groups.setdefault(cur, {})
                continue
            if "=" not in s or cur is None:
                continue
            k, v = s.split("=", 1)
            k, v = k.strip(), v.strip()
            if "[" in k and "[$" not in k:
                continue
            groups[cur][k] = v
    return groups


def digest(path, algo):
    with open(path, "rb") as f:
        return hashlib.new(algo, f.read()).hexdigest()


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


rows = [l.rstrip("\n").split("\t") for l in open(manifest, encoding="utf-8") if l.strip()]
schemes, cases = {}, []
for scheme, accent, case_dir in rows:
    if scheme not in schemes:
        derived = os.path.join(schemes_dir, scheme + ".colors")
        if os.path.exists(derived):
            src, origin = derived, "derived"
        else:
            src, origin = "/usr/share/color-schemes/%s.colors" % scheme, "installed"
        rec = {
            "origin": origin,
            "source": src if origin == "installed" else "derived (scratch data dir)",
            "sha1": digest(src, "sha1"),
            "sha256": digest(src, "sha256"),
            "input": parse(src),
            "base": parse(os.path.join(case_dir, "base.kdeglobals")),
            "base_sha256": digest(os.path.join(case_dir, "base.kdeglobals"), "sha256"),
        }
        if scheme == "KdeGoldenTintLight":
            rec["derived_from"] = "BreezeLight"
            rec["changes"] = {"General": {"Name": scheme, "ColorScheme": scheme, "TintFactor": "0.25"}}
        if scheme == "KdeGoldenTitlebarDark":
            rec["derived_from"] = "BreezeDark"
            rec["changes"] = {"General": {"Name": scheme, "ColorScheme": scheme, "TitlebarIsAccentColored": "true"}}
        schemes[scheme] = rec
    else:
        # The no-accent snapshot must not depend on the case.
        if parse(os.path.join(case_dir, "base.kdeglobals")) != schemes[scheme]["base"]:
            schemes[scheme]["base_differs_between_cases"] = True
    kg = os.path.join(case_dir, "config", "kdeglobals")
    files = sorted(
        os.path.relpath(os.path.join(dp, fn), os.path.join(case_dir, "config"))
        for dp, _, fns in os.walk(os.path.join(case_dir, "config"))
        for fn in fns
    )
    cases.append({
        "scheme": scheme,
        "accent": accent,
        "commands": [["plasma-apply-colorscheme", scheme], ["plasma-apply-colorscheme", "-a", accent]],
        "stdout": read(os.path.join(case_dir, "accent.out")).strip(),
        "kdeglobals": parse(kg),
        "kdeglobals_sha256": digest(kg, "sha256"),
        "config_files": files,
    })

versions = {}
try:
    for l in open("/opt/kde_golden/versions.txt", encoding="utf-8"):
        k, _, v = l.rstrip("\n").partition("\t")
        if k:
            versions[k] = v
except OSError:
    pass

meta = {
    "kind": "accent",
    "generator": "tools/kde_golden/accent_oracle.sh",
    "image": os.environ.get("KDE_GOLDEN_IMAGE", "unknown"),
    "packages": versions,
    "procedure": "see accent_oracle.sh header and README.md",
    "readme": "tools/kde_golden/README.md documents every field",
}

with open(out, "w", encoding="utf-8") as f:
    f.write("{\n")
    f.write('"meta": %s,\n' % json.dumps(meta, sort_keys=True, ensure_ascii=False))
    f.write('"schemes": {\n')
    names = list(schemes)
    for i, name in enumerate(names):
        f.write("%s: %s%s\n" % (json.dumps(name), json.dumps(schemes[name], ensure_ascii=False), "," if i + 1 < len(names) else ""))
    f.write("},\n")
    f.write('"cases": [\n')
    for i, c in enumerate(cases):
        f.write("%s%s\n" % (json.dumps(c, ensure_ascii=False), "," if i + 1 < len(cases) else ""))
    f.write("]\n}\n")
PY
echo "accent_oracle.sh: wrote $out ($n cases)"
