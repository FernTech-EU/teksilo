# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Key names to the evdev codes `fake_key` presses.

A chord is written the way a person would: `"Tab"`, `"Shift+Tab"`, `"Alt+F"`,
`"Ctrl+Shift+Z"`, `"F10"`. Letters and digits are the keys of a US layout,
which the private session pins (`XKB_DEFAULT_LAYOUT=us`, see
`private_session.sh`), so `"A"` is the key that types `a` there and `"Shift+A"`
the one that types `A`.

Codes are those of `linux/input-event-codes.h`, which is what
`org_kde_kwin_fake_input.keyboard_key` takes.
"""

from __future__ import annotations

#: Named keys. Case-insensitive on lookup.
NAMED = {
    "esc": 1, "escape": 1,
    "minus": 12, "-": 12, "equal": 13, "=": 13,
    "backspace": 14, "tab": 15,
    "leftbrace": 26, "[": 26, "rightbrace": 27, "]": 27,
    "enter": 28, "return": 28,
    "ctrl": 29, "control": 29, "leftctrl": 29,
    "semicolon": 39, ";": 39, "apostrophe": 40, "'": 40, "grave": 41, "`": 41,
    "shift": 42, "leftshift": 42, "backslash": 43, "\\": 43,
    "comma": 51, ",": 51, "dot": 52, "period": 52, ".": 52, "slash": 53, "/": 53,
    "rightshift": 54,
    "alt": 56, "leftalt": 56,
    "space": 57, " ": 57,
    "capslock": 58, "caps_lock": 58,
    "f1": 59, "f2": 60, "f3": 61, "f4": 62, "f5": 63, "f6": 64, "f7": 65, "f8": 66,
    "f9": 67, "f10": 68, "f11": 87, "f12": 88,
    "kpminus": 74, "kpplus": 78,
    "rightctrl": 97, "rightalt": 100, "altgr": 100,
    "home": 102, "up": 103, "pageup": 104, "page_up": 104, "left": 105, "right": 106,
    "end": 107, "down": 108, "pagedown": 109, "page_down": 109,
    "insert": 110, "delete": 111, "del": 111,
    "super": 125, "meta": 125, "leftmeta": 125,
    "menu": 127, "contextmenu": 127, "compose": 127,
}

# The letter rows of a US keyboard, in evdev order.
for _row, _start in (("qwertyuiop", 16), ("asdfghjkl", 30), ("zxcvbnm", 44)):
    for _i, _ch in enumerate(_row):
        NAMED[_ch] = _start + _i
for _i, _ch in enumerate("1234567890"):
    NAMED[_ch] = 2 + _i

#: The keys held while the last key of a chord is tapped.
MODIFIERS = {"shift", "leftshift", "rightshift", "ctrl", "control", "leftctrl",
             "rightctrl", "alt", "leftalt", "rightalt", "altgr", "super", "meta",
             "leftmeta"}

#: Characters of a US layout that need Shift, and the key under them.
SHIFTED = {
    "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8",
    "(": "9", ")": "0", "_": "-", "+": "=", "{": "[", "}": "]", ":": ";", '"': "'",
    "~": "`", "|": "\\", "<": ",", ">": ".", "?": "/",
}


class KeyError_(ValueError):
    """A key name `fake_key` has no code for."""


def code(name: str) -> int:
    found = NAMED.get(name.lower())
    if found is None:
        raise KeyError_(f"no key named {name!r}")
    return found


def chord_steps(chord: str) -> list[str]:
    """`"Ctrl+Shift+Z"` as `fake_key` steps: hold, tap, release in reverse.

    A lone `+` is the plus key's own name, so `"Ctrl++"` is Ctrl with `=`
    shifted, written out as `"Ctrl+Shift+="` by whoever needs it.
    """
    parts = chord.split("+") if chord != "+" else ["+"]
    parts = [p for p in parts if p != ""] or ["+"]
    *held, last = parts
    for name in held:
        if name.lower() not in MODIFIERS:
            raise KeyError_(f"{name!r} in {chord!r} is not a modifier")
    steps = [f"+{code(h)}" for h in held]
    steps.append(f"={code(last)}")
    steps.extend(f"-{code(h)}" for h in reversed(held))
    return steps


def text_steps(text: str) -> list[str]:
    """Type `text` on a US layout: each character a tap, shifted where needed."""
    steps: list[str] = []
    for ch in text:
        if ch in SHIFTED:
            steps.extend(chord_steps(f"Shift+{SHIFTED[ch]}"))
        elif ch.isalpha() and ch.isascii() and ch.isupper():
            steps.extend(chord_steps(f"Shift+{ch.lower()}"))
        elif ch == "\n":
            steps.append(f"={code('Return')}")
        elif ch.lower() in NAMED and (ch.isascii()):
            steps.append(f"={code(ch)}")
        else:
            raise KeyError_(f"{ch!r} cannot be typed on a US layout by key; "
                            "use the bridge's type_text for it")
    return steps
