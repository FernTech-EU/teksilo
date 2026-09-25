#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech
#
# Run a command in a private, invisible desktop session, with a screen
# reader's view of it available and nothing reaching the real desktop.
#
#   tools/reader/private_session.sh python3 tools/reader/reader.py run dialogs
#
# Ported from CalendrierAccessible's `scripts/probes/headless.sh`, where every
# rule below was learned by breaking it. The session is:
#
#   * a private D-Bus session bus (`dbus-run-session`), so the AT-SPI bus, the
#     registry, KWin's own D-Bus name and every service started on demand are
#     private to the run;
#   * a private runtime directory, made with `mktemp -d`, mode 0700, and
#     exported before the bus starts. The bus starts services with the
#     environment it was started with, and with the desktop's runtime
#     directory and `DISPLAY=:0` the private `at-spi-bus-launcher` named its
#     socket `at-spi/bus_0`, the desktop's own, unlinked it to bind its own and
#     unlinked it again on exit: every program started afterwards, the user's
#     screen reader included, found no accessibility bus. The desktop socket is
#     checked before and after the run, and the run fails if it went missing;
#   * no `DISPLAY` and a private `WAYLAND_DISPLAY`, served by a private
#     `kwin_wayland --virtual`, which renders to no screen but sends frame
#     callbacks, so everything that waits on a frame runs as on a desktop;
#   * `GSETTINGS_BACKEND=memory`: turning accessibility on is a GSettings
#     write (`org.a11y.Status.IsEnabled` is stored as `toolkit-accessibility`),
#     and through dconf it would land in the user's own settings for good;
#   * no sound: PulseAudio, PipeWire and speech-dispatcher are pointed at
#     sockets that do not exist, in a runtime directory that holds none of
#     theirs. Orca, when the harness runs it, speaks to a null speech server
#     on top of that (see `orca.py`).
#
# The exit code is the command's own. Every process carrying this run's
# private bus address is killed on the way out, and nothing else.

set -uo pipefail

DESKTOP_ATSPI="/run/user/$(id -u)/at-spi/bus_0"

desktop_atspi_state() {
    if [[ -S "$DESKTOP_ATSPI" ]]; then echo present; else echo absent; fi
}

if [[ -z "${TEKSILO_READER_INNER:-}" ]]; then
    if [[ $# -eq 0 ]]; then
        echo "usage: $0 <command> [args...]" >&2
        exit 64
    fi
    for tool in dbus-run-session kwin_wayland gdbus; do
        if ! command -v "$tool" >/dev/null; then
            echo "private_session.sh: $tool is not installed, so there is no private session to make" >&2
            exit 1
        fi
    done
    before="$(desktop_atspi_state)"
    export TEKSILO_READER_INNER=1
    # Under the temporary directory, not a deep scratch path: a Unix socket
    # address holds 108 bytes, and the runtime directory prefixes every one.
    runtime="$(mktemp -d "${TMPDIR:-/tmp}/teksilo-reader-run.XXXXXX")" || exit 1
    chmod 700 "$runtime"
    socket="wayland-teksilo-reader-$$"
    # Configuration and data homes of the session's own, so nothing started in
    # it reads the user's (KWin would take the keyboard layout from
    # `~/.config/kxkbrc`, and a letter pressed by the harness would type what
    # that layout puts on the key) or writes to them (an example that persists
    # its settings). The cache stays the user's: the shader cache in it is
    # what keeps a cold start of a GPU application in seconds.
    mkdir -m 700 "$runtime/config" "$runtime/data" "$runtime/state"
    # English messages unless a scenario asks for another language, so what
    # Orca says (its role names included) reads the same on every machine.
    env -u DISPLAY -u WAYLAND_SOCKET -u AT_SPI_BUS_ADDRESS -u LANGUAGE -u LC_ALL \
        -u LC_MESSAGES -u LC_CTYPE -u LC_NUMERIC -u LC_TIME \
        XDG_RUNTIME_DIR="$runtime" WAYLAND_DISPLAY="$socket" \
        XDG_CONFIG_HOME="$runtime/config" XDG_DATA_HOME="$runtime/data" \
        XDG_STATE_HOME="$runtime/state" \
        XKB_DEFAULT_LAYOUT=us XKB_DEFAULT_MODEL=pc105 \
        LANG="${TEKSILO_READER_LANG:-en_US.UTF-8}" \
        GSETTINGS_BACKEND=memory \
        PULSE_SERVER="unix:$runtime/no-pulseaudio" \
        PIPEWIRE_REMOTE="$runtime/no-pipewire" \
        SPEECHD_ADDRESS="unix_socket:$runtime/no-speech-dispatcher" \
        SPEECHD_CMD=/bin/false \
        GST_AUDIO_SINK=fakesink \
        ALSA_CONFIG_PATH=/dev/null \
        dbus-run-session -- "$0" "$@" 2>&1 \
        | grep --line-buffered -v -E \
            '^(dbus-daemon\[|\(/usr/libexec/xdg-desktop-portal|\(xdg-desktop-portal-gtk:|\(at-spi2-registryd:|\*\* \(xdg-desktop-portal|SpiRegistry|kf\.wallet|qt\.qpa\.|qt\.accessibility\.atspi|kwin_|fusermount3?:|error: fuse init failed|Failed to create wl_display|This application failed to start because no Qt platform plugin|Available platform plugins are|This indicates a bug in someone.s code|The overwriting error message was|Ignoring invalid max threads|Gdk-Message|The Wayland connection broke|Failed to write to the pipe|$)'
    status="${PIPESTATUS[0]}"
    rm -rf "$runtime"
    after="$(desktop_atspi_state)"
    if [[ "$before" == present && "$after" != present ]]; then
        echo "private_session.sh: THE DESKTOP'S AT-SPI SOCKET $DESKTOP_ATSPI WENT MISSING during this run." >&2
        echo "  The user's screen reader will find no accessibility bus until at-spi-bus-launcher restarts." >&2
        exit 70
    fi
    exit "$status"
fi

log_dir="${TEKSILO_READER_SCRATCH:-${TMPDIR:-/tmp}}"
mkdir -p "$log_dir"
socket="$WAYLAND_DISPLAY"
kwin_log="$log_dir/private-kwin-$$.log"

# Without the name it is about to serve, which is not a compositor to connect to.
#
# `KWIN_WAYLAND_NO_PERMISSION_CHECKS=1` lets this KWin's clients bind the
# interfaces it keeps for trusted programs, `org_kde_kwin_fake_input` among
# them, which is how `fake_key` presses real keys. This KWin serves this run's
# private session and nothing else, so what it lets a client do reaches
# nothing outside the run; the desktop's KWin is not touched.
env -u WAYLAND_DISPLAY KWIN_WAYLAND_NO_PERMISSION_CHECKS=1 kwin_wayland --virtual --no-lockscreen --no-global-shortcuts \
    --socket "$socket" --width 1280 --height 900 >"$kwin_log" 2>&1 &
kwin=$!

cleanup() {
    kill "$kwin" 2>/dev/null
    wait "$kwin" 2>/dev/null
    # Every process on this run's private bus, the bus's on-demand services
    # included, and nothing else: the address names a socket only this run owns.
    local proc env_file
    for proc in /proc/[0-9]*; do
        env_file="$proc/environ"
        [[ -r "$env_file" ]] || continue
        [[ "${proc#/proc/}" == "$$" ]] && continue
        if { tr '\0' '\n' <"$env_file"; } 2>/dev/null \
            | grep -qxF "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS"; then
            kill "${proc#/proc/}" 2>/dev/null
        fi
    done
}
trap cleanup EXIT

for _ in $(seq 1 100); do
    [[ -S "$XDG_RUNTIME_DIR/$socket" ]] && break
    kill -0 "$kwin" 2>/dev/null || break
    sleep 0.1
done
if [[ ! -S "$XDG_RUNTIME_DIR/$socket" ]]; then
    echo "private_session.sh: the private KWin did not start; its log is $kwin_log" >&2
    tail -n 20 "$kwin_log" >&2
    exit 1
fi

# Accessibility on, in memory only: `accesskit_unix` exposes the window on the
# AT-SPI bus only while `org.a11y.Status.IsEnabled` says so.
if ! gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.freedesktop.DBus.Properties.Set \
    org.a11y.Status IsEnabled '<true>' >/dev/null; then
    echo "private_session.sh: accessibility could not be turned on in the private session" >&2
    exit 1
fi

export WAYLAND_DISPLAY="$socket"
export TEKSILO_READER_KWIN_LOG="$kwin_log"
unset DISPLAY
"$@"
