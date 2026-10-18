#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech
#
# Measure the KDE Breeze preset in the real widget catalog, in a private
# session.
#
#   tools/reader/kde_visual_check.sh                    # kde-light, kde-dark, system
#   tools/reader/kde_visual_check.sh --modes system     # one mode
#   tools/reader/kde_visual_check.sh --out DIR          # keep the logs
#
# Started from the desktop, it builds the debug `widget-catalog` and
# `teksilo-automation-mcp` into this checkout's `target/` (debug, so the
# automation bridge is in), then runs itself again through
# `private_session.sh`, so the catalog and `plasma-apply-colorscheme` only
# ever run in the private session: its own D-Bus bus, runtime directory,
# `kwin_wayland --virtual` and configuration, data and state homes. Inside,
# it runs `kde_visual_check.py`, whose docstring says what is measured and
# against what. Needs `kwin_wayland`, `dbus-run-session`, `gdbus` and
# `plasma-apply-colorscheme`.
#
# The exit code is the driver's: 0 when every check passes, 1 on a failed
# check, 2 when it cannot run.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

if [[ -z "${TEKSILO_READER_INNER:-}" ]]; then
    # The build runs outside the private session: cargo and its wrappers find
    # their jobserver and caches under the desktop's runtime directory.
    if ! RUSTC_WRAPPER= cargo build --quiet -p widget-catalog -p teksilo-automation-mcp \
        --manifest-path "$repo/Cargo.toml" --target-dir "$repo/target"; then
        echo "kde_visual_check.sh: the build failed" >&2
        exit 2
    fi
    exec "$here/private_session.sh" "$here/kde_visual_check.sh" "$@"
fi

exec python3 -I "$here/kde_visual_check.py" "$@"
