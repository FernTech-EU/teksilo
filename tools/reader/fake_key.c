// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// Press real keys inside a private KWin, through `org_kde_kwin_fake_input`.
//
// Built by `tools/reader/reader_lib/run.py` into the target directory, never
// committed as a binary. The keys go through the compositor's seat, so they
// reach the application through winit, exactly as a keyboard's do: window-level
// keys (F10, Alt, Caps Lock) included, which the automation bridge's
// `inject_key` cannot drive because it writes into the widget tree.
//
//     fake_key +56 =33 -56 wait:50 =68
//     fake_key --stdin
//
// `+code` presses, `-code` releases, `=code` taps (press, then release),
// `wait:ms` pauses, and `move:x,y` puts the pointer at a position of the
// output. A code is a Linux evdev key code (`linux/input-event-codes.h`),
// which is what the protocol's `keyboard_key` takes.
//
// With `--stdin`, it stays connected and reads one line of steps at a time,
// answering each with `ok` (or `error: ...`) once the compositor has taken
// them. KWin adds a fake-input device, with a pointer, for every client and
// removes it when the client leaves, so a client per key makes the seat's
// pointer come and go under the application; winit 0.30 panicked on that
// ("failed to get pointer data"). One client for the whole run keeps the seat
// still.
//
// Exits 0 once the compositor has taken every request, 1 when it offers no
// fake input, 2 on a usage error. Refuses to run unless `XDG_RUNTIME_DIR` is
// a runtime directory the reader harness made (`teksilo-reader-run.*`):
// pressed on the desktop's own compositor, these keys would land in whatever
// window the user has open.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wayland-client.h>
#include "fake-input-client-protocol.h"

static struct org_kde_kwin_fake_input *fake;

static void on_global(void *data, struct wl_registry *registry, uint32_t name,
                      const char *interface, uint32_t version) {
    (void)data;
    if (strcmp(interface, org_kde_kwin_fake_input_interface.name) == 0 && version >= 4) {
        fake = wl_registry_bind(registry, name, &org_kde_kwin_fake_input_interface, 4);
    }
}

static void on_global_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {on_global, on_global_remove};

static void pause_ms(long ms) {
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    nanosleep(&ts, NULL);
}

// One step. Returns 0, or 2 when the step is not one.
static int step(struct wl_display *display, const char *arg) {
    if (strncmp(arg, "wait:", 5) == 0) {
        wl_display_roundtrip(display);
        pause_ms(atol(arg + 5));
        return 0;
    }
    if (strncmp(arg, "move:", 5) == 0) {
        double x = 0, y = 0;
        if (sscanf(arg + 5, "%lf,%lf", &x, &y) != 2) {
            return 2;
        }
        org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_double(x),
                                                        wl_fixed_from_double(y));
        wl_display_roundtrip(display);
        return 0;
    }
    char op = arg[0];
    uint32_t code = (uint32_t)strtoul(arg + 1, NULL, 10);
    if ((op != '+' && op != '-' && op != '=') || code == 0) {
        return 2;
    }
    if (op == '+' || op == '=') {
        org_kde_kwin_fake_input_keyboard_key(fake, code, WL_KEYBOARD_KEY_STATE_PRESSED);
    }
    if (op == '=') {
        wl_display_roundtrip(display);
        pause_ms(20);
    }
    if (op == '-' || op == '=') {
        org_kde_kwin_fake_input_keyboard_key(fake, code, WL_KEYBOARD_KEY_STATE_RELEASED);
    }
    wl_display_roundtrip(display);
    pause_ms(15);
    return 0;
}

static int serve(struct wl_display *display) {
    char line[8192];
    while (fgets(line, sizeof line, stdin) != NULL) {
        int status = 0;
        for (char *word = strtok(line, " \t\r\n"); word != NULL; word = strtok(NULL, " \t\r\n")) {
            if (step(display, word) != 0) {
                printf("error: not a step: %s\n", word);
                status = 2;
                break;
            }
        }
        if (status == 0) {
            wl_display_roundtrip(display);
            printf("ok\n");
        }
        fflush(stdout);
    }
    return 0;
}

int main(int argc, char **argv) {
    const char *runtime = getenv("XDG_RUNTIME_DIR");
    if (runtime == NULL || strstr(runtime, "teksilo-reader-run.") == NULL) {
        fprintf(stderr, "fake_key: refusing: XDG_RUNTIME_DIR is not a reader harness's private one\n");
        return 2;
    }
    struct wl_display *display = wl_display_connect(NULL);
    if (display == NULL) {
        fprintf(stderr, "fake_key: no Wayland display\n");
        return 1;
    }
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    wl_display_roundtrip(display);
    if (fake == NULL) {
        fprintf(stderr, "fake_key: the compositor offers no org_kde_kwin_fake_input v4\n");
        return 1;
    }
    org_kde_kwin_fake_input_authenticate(fake, "teksilo reader harness", "scripted keys in a private session");
    if (argc == 2 && strcmp(argv[1], "--stdin") == 0) {
        int status = serve(display);
        wl_display_disconnect(display);
        return status;
    }
    for (int i = 1; i < argc; i++) {
        if (step(display, argv[i]) != 0) {
            fprintf(stderr, "fake_key: not a step: %s\n", argv[i]);
            return 2;
        }
    }
    wl_display_roundtrip(display);
    wl_display_disconnect(display);
    return 0;
}
