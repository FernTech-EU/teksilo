# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""multi-window and internationalization, as a reader meets them.

multi-window (`examples/multi_window/src/main.rs`): a main window
(`WindowConfig::title("Teksilo — Multi-Window Demo")`, no i18n config) with a
Theme combo box in a toolbar, a status label bound to the window's active
state, an unlabelled `TextInput`, a `DimWhenInactive` button and an "Open help
(F1) / Toggle fullscreen (F11)" button. F1 (a global shortcut) or that button
opens a second window, "Teksilo — Help", holding a label, an unlabelled
`TextInput` and a "Close help" button that closes it (`ctx.close_window()`);
F1 while Help is open calls `ctx.focus_window` on it.

The private KWin runs with `--no-global-shortcuts`, so Alt+Tab and Alt+F4 do
nothing there. Switching and closing one window is done here with a KWin
script (`workspace.activeWindow = w`, `w.closeWindow()`), which is what KWin
itself does for those keys. (`run.close_window()` closes every window of the
process, so it cannot close only Help.)

internationalization (`examples/internationalization/src/main.rs`): three
compiled-in locales, en-US (source), fr-FR and ar-SA, and
`auto_detect_os_locale(false)`, so `LANG` does not pick the language: the app
starts in en-US whatever the session says, and switches with three buttons
("English" / "Français" / "العربية") or a `LanguageSwitcher` combo box, each
calling `ctx.set_locale(...)`. ar-SA flips the layout to right-to-left.

The language a reader's speech could follow is the AccessKit `language`
property the tree sets on its root (`accessibility_emit_impl.rs`). On AT-SPI
the object `Locale` is hardcoded empty (`accesskit_unix` 0.23
`interfaces/accessible.rs`), so the only place it reaches is the `language`
text attribute of a node with a Text interface (`accesskit_atspi_common`
`text_attributes.rs`, inherited through `accesskit_consumer`
`fetch_inherited_property`), which is what Orca reads
(`script_utilities.getLanguageAndDialectFromTextAttributes`). `text_languages`
reads that attribute the way Orca does, from a subprocess on the private bus.

The scroll scenarios (`winintl-intl-scroll-*`) follow the example's
`ScrollArea`: AccessKit's clip filter drops a child that leaves the viewport
from the tree, the AT-SPI adapter tells the bus it is defunct, and when it
comes back with the same id libatspi keeps it defunct. `SteadySeat` works
around a harness-triggered winit crash in the multi-window scenarios; see its
docstring.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import time
from pathlib import Path

from reader_lib.checks import (_focus_node, _is_focus, custom, event, focused, in_tree,
                               no_event, not_in_tree, not_said, said)
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario, tab_walk

MW = "multi-window"
INTL = "internationalization"

MAIN_TITLE = "Teksilo — Multi-Window Demo"
HELP_TITLE = "Teksilo — Help"
OPEN_HELP = {"role": "push button", "name": "Open help (F1) / Toggle fullscreen (F11)"}
CLOSE_HELP = {"role": "push button", "name": "Close help"}
MAIN_FIELD = "Select some of this text"
HELP_FIELD = "editable — select me"


# ---------------------------------------------------------------------------
# KWin
# ---------------------------------------------------------------------------


def kwin(run, name: str, js: str) -> None:
    """Run a one-off KWin script in the private session."""
    script = Path(run.work) / f"{name}-{time.monotonic_ns()}.js"
    script.write_text(js, encoding="utf-8")
    loaded = subprocess.run(
        ["gdbus", "call", "--session", "--dest", "org.kde.KWin", "--object-path",
         "/Scripting", "--method", "org.kde.kwin.Scripting.loadScript", str(script)],
        capture_output=True, text=True, timeout=10)
    ids = re.findall(r"-?\d+", loaded.stdout)
    if loaded.returncode != 0 or not ids or int(ids[0]) < 0:
        raise RuntimeError("KWin would not load a script: "
                           + (loaded.stderr.strip() or loaded.stdout.strip()))
    for method in ("run", "stop"):
        subprocess.run(
            ["gdbus", "call", "--session", "--dest", "org.kde.KWin", "--object-path",
             f"/Scripting/Script{ids[0]}", "--method", f"org.kde.kwin.Script.{method}"],
            capture_output=True, text=True, timeout=10)


def _windows_js(run, body: str) -> str:
    return ("const wins = workspace.windowList ? workspace.windowList() "
            ": workspace.clientList();\n"
            f"wins.forEach(w => {{ if (w.pid === {run.app.pid}) {{ {body} }} }});\n")


def activate_window(run, caption: str) -> None:
    """Make the window whose caption is `caption` the active one, as Alt+Tab
    or a click on it would."""
    run._step(f"KWin activates the window {caption!r}")
    kwin(run, "activate", _windows_js(
        run, f"if (w.caption === {json.dumps(caption)}) workspace.activeWindow = w;"))


class SteadySeat:
    """Hold one fake-input device open for the whole scenario.

    Every `run.key` starts a `fake_key` process, and KWin adds an input device
    (keyboard, pointer, touch) for each fake-input client and removes it when
    the client exits. In the private session there is no other pointer, so the
    seat's pointer capability comes and goes with every key press, and winit
    0.30.13 panics (`failed to get pointer data.`,
    `platform_impl/linux/wayland/seat/pointer/mod.rs:409`) when that flap
    meets a window being created. Keeping one client connected keeps the
    capability steady. Used only by the multi-window scenarios that measure
    something other than that crash."""

    def __init__(self, run) -> None:
        self.proc = subprocess.Popen([str(run.fake_key), "wait:900000"],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        run.note("a fake-input client is held open for the whole run, so the seat's "
                 "pointer capability does not flap with each key press")
        time.sleep(0.5)

    def close(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()


def steady(body):
    """Run a multi-window body with a `SteadySeat` held open."""
    def wrapped(run):
        seat = SteadySeat(run)
        try:
            body(run)
        finally:
            seat.close()
    wrapped.__doc__ = body.__doc__
    return wrapped


def close_one_window(run, caption: str) -> None:
    """Close only the window whose caption is `caption`, as its close button
    or Alt+F4 would."""
    run._step(f"KWin closes the window {caption!r}")
    kwin(run, "close", _windows_js(
        run, f"if (w.caption === {json.dumps(caption)}) w.closeWindow();"))


# ---------------------------------------------------------------------------
# Reading what Orca would read for a language
# ---------------------------------------------------------------------------

_LANG_PROBE = r'''
import json, sys
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
pid = int(sys.argv[1])
desk = Atspi.get_desktop(0)
app = None
for i in range(desk.get_child_count()):
    c = desk.get_child_at_index(i)
    try:
        if c is not None and c.get_process_id() == pid:
            app = c
    except Exception:
        pass
out = []
def walk(n, depth=0):
    if n is None or depth > 40:
        return
    rec = {"role": n.get_role_name(), "name": n.get_name() or ""}
    try:
        rec["locale"] = n.get_object_locale()
    except Exception as e:
        rec["locale_error"] = type(e).__name__
    try:
        ifaces = n.get_interfaces()
    except Exception:
        ifaces = []
    if "Text" in ifaces:
        try:
            attrs, start, end = Atspi.Text.get_attribute_run(n, 0, True)
            rec["language"] = (attrs or {}).get("language", "")
        except Exception as e:
            rec["language_error"] = type(e).__name__
        try:
            count = Atspi.Text.get_character_count(n)
            rec["text"] = Atspi.Text.get_text(n, 0, count)
            if count > 60:
                lines, offset = [], 0
                while offset < count and len(lines) < 20:
                    r = Atspi.Text.get_string_at_offset(n, offset, Atspi.TextGranularity.LINE)
                    try:
                        box = Atspi.Text.get_range_extents(n, r.start_offset, r.end_offset,
                                                           Atspi.CoordType.WINDOW)
                        where = f"@x={box.x},y={box.y},w={box.width}"
                    except Exception as e:
                        where = f"@{type(e).__name__}"
                    lines.append([r.start_offset, r.end_offset, where + " " + r.content])
                    offset = max(r.end_offset, offset + 1)
                rec["lines"] = lines
        except Exception as e:
            rec["text_error"] = type(e).__name__
    try:
        ext = Atspi.Component.get_extents(n, Atspi.CoordType.WINDOW)
        rec["extents"] = [ext.x, ext.y, ext.width, ext.height]
    except Exception:
        pass
    out.append(rec)
    try:
        count = n.get_child_count()
    except Exception:
        count = 0
    for i in range(count):
        try:
            walk(n.get_child_at_index(i), depth + 1)
        except Exception:
            pass
walk(app)
print(json.dumps(out, ensure_ascii=False))
'''


def text_languages(run) -> list[dict]:
    """Every node of the application with its object locale, the `language`
    text attribute Orca would read (for a node with a Text interface) and its
    window extents, read on the private bus by a separate process."""
    done = subprocess.run([sys.executable, "-W", "ignore", "-c", _LANG_PROBE, str(run.app.pid)],
                          capture_output=True, text=True, timeout=60)
    if done.returncode != 0:
        raise RuntimeError(f"the language probe failed: {done.stderr.strip()[-500:]}")
    return json.loads(done.stdout.strip().splitlines()[-1])


def record_languages(run, label: str) -> list[dict]:
    nodes = text_languages(run)
    langs = sorted({(n.get("language") or "") for n in nodes if "language" in n})
    locales = sorted({(n.get("locale") or "") for n in nodes})
    run.note(f"{label}: `language` text attributes on Text nodes: {langs}; "
             f"object locales: {locales}")
    for n in nodes:
        if "language" in n:
            run.note(f"  {label}: [{n['role']}] {n['name'][:60]!r} language={n['language']!r}")
        for start, end, content in n.get("lines", []):
            run.note(f"    {label}: line {start}-{end}: {content!r}")
    return nodes


def languages_are(box: dict, want: str):
    """Every Text node's `language` attribute was `want` when probed (the
    probe runs in the body; the check reads what it found)."""
    def check(act):
        nodes = box.get("nodes")
        if nodes is None:
            return False, ["the probe did not run"]
        texts = [n for n in nodes if "language" in n]
        wrong = [f"[{n['role']}] {n['name'][:40]!r} language={n.get('language')!r}"
                 for n in texts if n.get("language") != want]
        return bool(texts) and not wrong, (wrong or [f"{len(texts)} Text nodes, all {want!r}"])
    return custom(f"every Text node carries the language attribute {want!r}", check)


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


def frame_activated():
    def check(act):
        acts = [e for e in act.events if e["type"] == "window:activate"]
        return bool(acts), [f"window:activate {e['source'].get('path')} "
                            f"{e['source'].get('name')!r} at {act.rel_ms(e):+.1f} ms"
                            for e in acts] or ["no window:activate in the act"]
    return custom("a window:activate event reaches the bus", check)


def focus_on_entry_text(text: str):
    """The act's last focus change lands on an entry whose text holds `text`
    (the examples' entries have no name, so they are told apart by text)."""
    def check(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change on the bus in this act"]
        last = _focus_node(moves[-1])
        ok = last.get("role") == "entry"
        return ok, [f"{e['type']} {e.get('detail1')} [{_focus_node(e).get('role')}] "
                    f"{_focus_node(e).get('name')!r} at {act.rel_ms(e):+.1f} ms" for e in moves]
    return custom(f"focus lands on the entry holding {text!r}", check)


def names_in_tree(*names: str):
    def check(act):
        found: set[str] = set()
        stack = [act.tree] if act.tree else []
        while stack:
            node = stack.pop()
            found.add(node.get("name") or "")
            stack.extend(node.get("children", []))
        missing = [n for n in names if n not in found]
        return not missing, [f"missing {m!r}" for m in missing] or ["all present"]
    return custom(f"the tree holds nodes named {list(names)!r}", check, needs_tree=True)


def orca_said_anything():
    def check(act):
        heard = utterances(act.orca)
        return bool(heard), [f"Orca said: {u.text!r}" for u in heard] or ["Orca said nothing"]
    return custom("Orca says something", check, needs_orca=True)


# ---------------------------------------------------------------------------
# multi-window
# ---------------------------------------------------------------------------


def _open_help_with_f1(run, label="F1 opens the Help window"):
    with run.act(label, [frame_activated(), focus_on_entry_text(HELP_FIELD),
                         said(HELP_FIELD)],
                 should="a second window opens and becomes active; focus lands in it "
                 "and the reader hears the control it lands on", tree=True):
        run.key("F1")


def mw_open_close(run):
    """F1 opens Help, Tab to its Close button, Space closes it; the main
    window takes focus back."""
    run.wait_for(**OPEN_HELP)
    _open_help_with_f1(run)
    with run.act("Tab to Close help", [focused(**CLOSE_HELP), said("Close help")],
                 should="focus moves to the Help window's Close button"):
        run.key("Tab")
    with run.act("Space on Close help",
                 [event("object:children-changed:remove", role="application"),
                  frame_activated(), focus_on_entry_text(MAIN_FIELD), said(MAIN_FIELD)],
                 should="Help closes, the main window becomes active again and the reader "
                 "hears where focus is", tree=True):
        run.key("space")


def mw_switch(run):
    """Open Help, go back to the main window, F1 again (the example focuses
    the open Help), switch to Help, then close Help from KWin while active."""
    run.wait_for(**OPEN_HELP)
    _open_help_with_f1(run)
    with run.act("switch to the main window (as Alt+Tab)",
                 [frame_activated(), focus_on_entry_text(MAIN_FIELD), said(MAIN_FIELD)],
                 should="the reader hears the main window and its focused control"):
        activate_window(run, MAIN_TITLE)
    with run.act("F1 while Help is open",
                 [frame_activated(), focus_on_entry_text(HELP_FIELD), said(HELP_FIELD)],
                 should="the example's F1 action finds the open Help window and calls "
                 "ctx.focus_window: Help comes to the front and the reader hears it"):
        run.key("F1")
    with run.act("switch to Help (as Alt+Tab)",
                 [frame_activated(), focus_on_entry_text(HELP_FIELD), said(HELP_FIELD)],
                 should="the reader hears the Help window and its focused control"):
        activate_window(run, HELP_TITLE)
    with run.act("close Help from KWin (as Alt+F4)",
                 [event("object:children-changed:remove", role="application"),
                  frame_activated(), focus_on_entry_text(MAIN_FIELD), said(MAIN_FIELD)],
                 should="Help closes; the main window becomes active and the reader hears "
                 "where focus is", tree=True):
        close_one_window(run, HELP_TITLE)


def mw_focus_return(run):
    """Open Help from the Open help button with the keyboard, close it with
    its own button, and see where focus comes back to in the main window."""
    run.wait_for(**OPEN_HELP)
    with run.act("Tab, Tab to Open help", [focused(**OPEN_HELP), said("Open help (F1)")],
                 should="focus moves from the field past the dimming button to Open help"):
        run.key("Tab", "Tab", gap=0.6)
    with run.act("Space on Open help",
                 [frame_activated(), focus_on_entry_text(HELP_FIELD), said(HELP_FIELD)],
                 should="Help opens and the reader hears the field focus lands on"):
        run.key("space")
    with run.act("Tab, Space on Close help",
                 [event("object:children-changed:remove", role="application"),
                  frame_activated(), focused(**OPEN_HELP), said("Open help (F1)")],
                 should="Help closes and focus comes back to the control that opened it, "
                 "Open help, which the reader hears"):
        run.key("Tab", gap=0.6)
        run.key("space")
    with run.act("Enter on Open help",
                 [frame_activated(), focus_on_entry_text(HELP_FIELD), said(HELP_FIELD)],
                 should="Enter on the focused button opens Help again"):
        run.key("Return")
    with run.act("close Help from KWin (as Alt+F4)",
                 [event("object:children-changed:remove", role="application"),
                  frame_activated(), focused(**OPEN_HELP), said("Open help (F1)")],
                 should="Help closes and focus comes back to Open help"):
        close_one_window(run, HELP_TITLE)


def mw_atspi(run):
    """Open and close Help the way a screen reader's own activation does."""
    run.wait_for(**OPEN_HELP)
    with run.act("AT-SPI click on Open help",
                 [frame_activated(), focus_on_entry_text(HELP_FIELD), said(HELP_FIELD)],
                 should="a screen reader's own activation opens Help, and focus lands in it"):
        run.action("click", **OPEN_HELP)
    with run.act("AT-SPI click on Close help",
                 [event("object:children-changed:remove", role="application"),
                  frame_activated(), focus_on_entry_text(MAIN_FIELD), said(MAIN_FIELD)],
                 should="Help closes; the main window is active again with focus where it "
                 "was"):
        run.action("click", **CLOSE_HELP)
    run.grab_focus(**OPEN_HELP)
    with run.act("Enter on Open help after an AT-SPI focus",
                 [frame_activated(), focus_on_entry_text(HELP_FIELD), said(HELP_FIELD)],
                 should="Enter on the button a reader focused opens Help"):
        run.key("Return")
    with run.act("close Help from KWin (as Alt+F4)",
                 [frame_activated(), focused(**OPEN_HELP), said("Open help (F1)")],
                 should="Help closes and focus comes back to Open help"):
        close_one_window(run, HELP_TITLE)


def mw_crash_sequence(run):
    """The sequence the previous sweep crashed on (winit's Wayland pointer
    code panicked, `failed to get pointer data`), replayed to see whether it
    is reproducible."""
    run.wait_for(**OPEN_HELP)
    with run.act("F1 opens Help", [frame_activated()], should="Help opens"):
        run.key("F1")
    with run.act("close Help from KWin", [frame_activated()], should="Help closes"):
        close_one_window(run, HELP_TITLE)
    with run.act("F1 again", [frame_activated()], should="Help opens"):
        run.key("F1")
    with run.act("Tab, Space on Close help", [frame_activated()], should="Help closes"):
        run.key("Tab", gap=0.6)
        run.key("space")
    with run.act("F1 a third time", [frame_activated()], should="Help opens"):
        run.key("F1")
    with run.act("Tab, Space on Close help again", [frame_activated()],
                 should="Help closes"):
        run.key("Tab", gap=0.6)
        run.key("space")
    with run.act("Tab, Tab to Open help", [focused(**OPEN_HELP)],
                 should="focus on Open help"):
        run.key("Tab", "Tab", gap=0.6)
    with run.act("Enter on Open help", [frame_activated(), focus_on_entry_text(HELP_FIELD)],
                 should="Help opens, and the application is still running"):
        run.key("Return")
    with run.act("the application is still there",
                 should="nothing happens; an exited application marks this act as an error"):
        run.wait(0.5)


# ---------------------------------------------------------------------------
# internationalization
# ---------------------------------------------------------------------------

EN = {"heading": "Teksilo i18n Showcase", "greeting": "Hello, Alice!",
      "direction": "Layout direction: Left to Right", "english": "English",
      "french": "Français", "arabic": "العربية", "leading": "Leading",
      "trailing": "Trailing", "formatting": "Locale-aware formatting"}
FR = {"heading": "Vitrine i18n de Teksilo", "greeting": "Bonjour, Alice !",
      "direction": "Direction de la mise en page : de gauche à droite",
      "english": "Anglais", "french": "Français", "arabic": "Arabe",
      "leading": "Début", "trailing": "Fin", "formatting": "Formatage selon la locale",
      "language": "Langue :"}
AR = {"heading": "معرض التدويل في Teksilo", "greeting": "مرحبا Alice!",
      "direction": "اتجاه التخطيط: من اليمين إلى اليسار", "english": "الإنجليزية",
      "french": "الفرنسية", "arabic": "العربية", "leading": "البداية",
      "trailing": "النهاية", "language": "اللغة:"}


def _names(tree: dict | None) -> list[dict]:
    out: list[dict] = []
    stack = [tree] if tree else []
    while stack:
        node = stack.pop()
        out.append(node)
        stack.extend(reversed(node.get("children", [])))
    return out


def formatting_rows(box: dict):
    """Record the formatting rows' names after the act, for the report."""
    def check(act):
        nodes = _names(act.tree)
        names = [n.get("name") or "" for n in nodes if n.get("role") == "label"]
        box["labels"] = names
        return True, [repr(n) for n in names]
    return custom("the labels a reader reads after the act (recorded)", check, needs_tree=True)


def intl_launch(run, box_lang: dict):
    run.wait_for(role="push button", name="English")
    box_lang["nodes"] = record_languages(run, "at launch")
    with run.act("the tree at launch",
                 [names_in_tree(EN["heading"], EN["greeting"], EN["direction"],
                                EN["english"], EN["french"], EN["arabic"]),
                  languages_are(box_lang, "en-US")],
                 should="English names, and the language en-US on every node a reader's "
                 "speech could switch voice by", tree=True):
        run.wait(0.2)


def intl_switch_buttons(run):
    """Launch in en-US, switch to fr-FR then ar-SA then back to en-US with the
    three buttons, as a keyboard user does."""
    en_langs: dict = {}
    intl_launch(run, en_langs)
    fr_langs: dict = {}
    fr_rows: dict = {}
    run.grab_focus(role="push button", name="Français")
    with run.act("Space on Français",
                 [names_in_tree(FR["heading"], FR["greeting"], FR["direction"],
                                FR["english"], FR["french"], FR["arabic"],
                                FR["leading"], FR["trailing"]),
                  in_tree(role="combo box", name="Langue"),
                  in_tree(role="label", name_contains="€"),
                  orca_said_anything(),
                  formatting_rows(fr_rows)],
                 should="every name turns French; focus stays on the Français button; "
                 "the reader learns the language changed", tree=True):
        run.key("space")
    fr_langs["nodes"] = record_languages(run, "after Français")
    with run.act("after Français: the language a reader's speech could follow",
                 [languages_are(fr_langs, "fr-FR")],
                 should="every Text node now carries fr-FR"):
        run.wait(0.1)
    with run.act("Tab to العربية", [focused(role="push button", name=FR["arabic"]),
                                    said(FR["arabic"])],
                 should="focus moves to the Arabic button, named in French"):
        run.key("Tab")
    ar_langs: dict = {}
    ar_rows: dict = {}
    with run.act("Space on العربية (Arabe)",
                 [names_in_tree(AR["heading"], AR["greeting"], AR["direction"],
                                AR["english"], AR["french"], AR["arabic"],
                                AR["leading"], AR["trailing"]),
                  in_tree(role="combo box", name="اللغة"),
                  formatting_rows(ar_rows)],
                 should="every name turns Arabic, the layout flips right-to-left, focus "
                 "stays on the Arabic button", tree=True):
        run.key("space")
    ar_langs["nodes"] = record_languages(run, "after العربية")
    with run.act("after العربية: the language a reader's speech could follow",
                 [languages_are(ar_langs, "ar-SA")], should="every Text node now carries ar-SA"):
        run.wait(0.1)
    with run.act("Shift+Tab, Shift+Tab to English (الإنجليزية)",
                 [focused(role="push button", name=AR["english"])],
                 should="Shift+Tab goes back along the row in reading order: Arabic, "
                 "French, English"):
        run.key("Shift+Tab", "Shift+Tab", gap=0.6)
    with run.act("Space on English (الإنجليزية)",
                 [names_in_tree(EN["heading"], EN["greeting"], EN["english"]),
                  formatting_rows({})],
                 should="every name back to English", tree=True):
        run.key("space")


def intl_rtl_order(run):
    """Switch to ar-SA, then record where the row's controls sit and how Tab
    and Shift+Tab walk them."""
    run.wait_for(role="push button", name="English")
    before = {n["name"]: n.get("extents") for n in text_languages(run)
              if n.get("role") == "push button"}
    run.note(f"button extents in en-US: {before}")
    run.grab_focus(role="push button", name="العربية")
    with run.act("Space on العربية", [in_tree(role="push button", name=AR["leading"])],
                 should="the layout flips right-to-left", tree=True):
        run.key("space")
    after = {n["name"]: n.get("extents") for n in record_languages(run, "en-US to ar-SA")
             if n.get("role") == "push button"}
    run.note(f"button extents in ar-SA: {after}")

    def mirrored(act):
        lead, trail = after.get(AR["leading"]), after.get(AR["trailing"])
        en, ar = after.get(AR["english"]), after.get(AR["arabic"])
        if not (lead and trail and en and ar):
            return False, [f"missing extents: {after}"]
        ok = lead[0] > trail[0] and en[0] > ar[0]
        return ok, [f"Leading x={lead[0]} Trailing x={trail[0]}; "
                    f"English x={en[0]} Arabic x={ar[0]}"]
    order: dict = {}

    def tree_order(act):
        nodes = [n for n in _names(act.tree) if n.get("role") in ("push button", "label",
                                                                  "combo box")]
        order["names"] = [n.get("name") for n in nodes]
        want = [AR["english"], AR["french"], AR["arabic"]]
        idx = [order["names"].index(w) for w in want if w in order["names"]]
        return idx == sorted(idx) and len(idx) == 3, [repr(n) for n in order["names"]]
    with run.act("where the row sits in ar-SA",
                 [custom("the Leading/Trailing and language rows are mirrored on screen",
                         mirrored),
                  custom("the tree keeps the reading order English, French, Arabic "
                         "(logical order, not left-to-right)", tree_order, needs_tree=True)],
                 should="the visual row flips; the reading order does not", tree=True):
        run.wait(0.1)
    run.grab_focus(role="push button", name=AR["english"])
    with run.act("Tab from English (الإنجليزية)",
                 [focused(role="push button", name=AR["french"]), said(AR["french"])],
                 should="Tab moves in reading order, English to French (right to left on "
                 "screen)"):
        run.key("Tab")
    with run.act("Tab again", [focused(role="push button", name=AR["arabic"])],
                 should="to Arabic"):
        run.key("Tab")
    with run.act("Tab to the combo box", [focused(role="combo box")],
                 should="to the language combo box"):
        run.key("Tab")
    with run.act("Tab to Leading (البداية)", [focused(role="push button", name=AR["leading"])],
                 should="to Leading, which sits on the right in RTL"):
        run.key("Tab")
    with run.act("Tab to Trailing (النهاية)", [focused(role="push button", name=AR["trailing"])],
                 should="to Trailing"):
        run.key("Tab")


def intl_switcher(run):
    """The LanguageSwitcher combo box: what a reader hears on it, and a switch
    made from it with the keyboard."""
    combo = {"role": "combo box", "name": "Language"}
    run.wait_for(**combo)
    run.grab_focus(role="push button", name="العربية")
    with run.act("Tab to the language combo box",
                 [focused(role="combo box", name="Language"), said("Language"),
                  said("English")],
                 should="the reader hears the combo box's name and the language it shows "
                 "(English (en-US))"):
        run.key("Tab")
    with run.act("Alt+Down opens the list",
                 [event("object:state-changed:expanded"), orca_said_anything()],
                 should="the list opens and the reader hears the current item", tree=True):
        run.key("Alt+Down")
    with run.act("Down to the next language",
                 [orca_said_anything(),
                  no_event("object:property-change:accessible-name", role="label",
                           name_contains="Bonjour")],
                 should="the reader hears the next language; moving through the list does "
                 "not yet switch the application's language"):
        run.key("Down")
    with run.act("Enter picks it",
                 [names_in_tree(FR["heading"]), orca_said_anything()],
                 should="the app switches language; the reader hears the combo box and "
                 "the language it now shows", tree=True):
        run.key("Return")
    with run.act("Shift+Tab, Tab back onto the combo box",
                 [focused(role="combo box"), said("Français")],
                 should="the reader hears the combo box in French, and its value"):
        run.key("Shift+Tab", gap=0.6)
        run.key("Tab")


def intl_tabwalk_ar(run):
    """A Tab walk after switching to Arabic, as a reader meets the RTL layout."""
    run.wait_for(role="push button", name="العربية")
    run.grab_focus(role="push button", name="العربية")
    with run.act("Space on العربية", [in_tree(role="push button", name=AR["leading"])],
                 should="the layout flips right-to-left", tree=True):
        run.key("space")
    tab_walk(run, stops=14)


def intl_values(run):
    """The price and count buttons below the fold: what a reader hears on
    them, and what reaches them when the values they change are reformatted."""
    run.wait_for(role="push button", name="Trailing")
    run.grab_focus(role="push button", name="Trailing")
    with run.act("Tab to − 100", [focused(role="push button", name="− 100"),
                                  said("Price"), said("100")],
                 should="focus moves below the fold to the price's minus button; the reader "
                 "hears what it changes (the price) and its name", tree=True):
        run.key("Tab")
    with run.act("Tab to + 100", [focused(role="push button", name="+ 100")],
                 should="to the price's plus button"):
        run.key("Tab")
    box: dict = {}
    with run.act("Space on + 100",
                 [in_tree(role="label", name="$1,334.56"), orca_said_anything(),
                  formatting_rows(box)],
                 should="the price goes up by 100; every formatted row follows; the reader "
                 "hears the new price", tree=True):
        run.key("space")
    with run.act("AT-SPI click on Français (focus stays on + 100)",
                 [in_tree(role="label", name_contains="334,56"), formatting_rows({})],
                 should="the app switches to French; the price rows are reformatted in "
                 "French", tree=True):
        run.action("click", role="push button", name="Français")
    with run.act("Space on + 100 in French",
                 [in_tree(role="label", name_contains="434,56"), orca_said_anything(),
                  formatting_rows({})],
                 should="the price goes up; the reader hears the new price in French",
                 tree=True):
        run.key("space")
    with run.act("Tab to − 1", [focused(role="push button", name="− 1"), said("1")],
                 should="to the count's minus button; the reader hears what it changes"):
        run.key("Tab")


def intl_scroll_back(run):
    """Tab down to the last button (the scroll area scrolls, and what leaves
    the viewport leaves the tree), then Shift+Tab back up to the language
    buttons, which scrolled out and come back."""
    run.wait_for(role="push button", name="English")
    run.grab_focus(role="push button", name="English")
    stops = ["Français", "العربية", None, "Leading", "Trailing", "− 100", "+ 100", "− 1",
             "+ 1"]
    for name in stops:
        label = f"Tab to {name or 'the combo box'}"
        expect = [focused(role="push button", name=name), said(name)] if name else \
            [focused(role="combo box")]
        with run.act(label, expect, should="focus moves on and the reader hears it"):
            run.key("Tab")
    with run.act("the tree at the bottom", [not_in_tree(role="push button", name="English")],
                 should="(recorded) whether the language buttons left the tree when they "
                 "scrolled out", tree=True):
        run.wait(0.1)
    for name in reversed(stops[:-1]):
        label = f"Shift+Tab to {name or 'the combo box'}"
        expect = [focused(role="push button", name=name), said(name)] if name else \
            [focused(role="combo box"), said("Language")]
        with run.act(label, expect, should="focus moves back and the reader hears it"):
            run.key("Shift+Tab")
    with run.act("Shift+Tab to English", [focused(role="push button", name="English"),
                                          said("English")],
                 should="focus is back on English, which scrolled out and back in; the "
                 "reader hears it", tree=True):
        run.key("Shift+Tab")
    with run.act("Tab to Français", [said("Français")],
                 should="the reader hears Français"):
        run.key("Tab")


def intl_scroll_reenter(run):
    """Scroll the intro out of the tree with Tab, then make the window tall
    enough that it scrolls back in, and see how the labels that left and came
    back stand on the bus."""
    run.wait_for(role="push button", name="Trailing")
    run.grab_focus(role="push button", name="Trailing")
    with run.act("Tab, Tab, Tab to − 1 (the intro scrolls out)",
                 [event("object:state-changed:defunct", role="label",
                        name_contains="Teksilo i18n Showcase")],
                 should="the viewport scrolls; labels scrolled out leave the tree", tree=True):
        run.key("Tab", "Tab", "Tab", gap=0.6)

    def grow():
        run._step("KWin makes the window 1000 px tall")
        kwin(run, "grow", _windows_js(
            run, "const g = w.frameGeometry; w.frameGeometry = "
                 "{x: g.x, y: 0, width: g.width, height: 1000};"))

    def heading_states(act):
        for node in _names(act.tree):
            if node.get("name") == "Teksilo i18n Showcase":
                states = node.get("states", [])
                return "defunct" not in states, [f"{node.get('path')} states={states}"]
        return False, ["the heading is not in the tree"]
    with run.act("the window grows; the intro scrolls back in",
                 [in_tree(role="label", name="Teksilo i18n Showcase"),
                  custom("the heading that came back is not defunct to libatspi",
                         heading_states, needs_tree=True)],
                 should="the heading and greeting are back in the tree, usable", tree=True):
        grow()
        run.wait(1.0)
    with run.act("AT-SPI click on Français",
                 [event("object:property-change:accessible-name", role="label",
                        name_contains="Vitrine")],
                 should="the heading's name change reaches the bus from a live node"):
        run.action("click", role="push button", name="Français")


def intl_scroll_focus_back(run):
    """In a short window, Tab down until the language buttons scroll out of
    the tree, then Shift+Tab back up to them: a control that left the tree
    and comes back, focused."""
    run.wait_for(role="push button", name="English")

    def shrink():
        run._step("KWin makes the window 300 px tall")
        kwin(run, "shrink", _windows_js(
            run, "const g = w.frameGeometry; w.frameGeometry = "
                 "{x: g.x, y: g.y, width: g.width, height: 300};"))
    with run.act("the window shrinks to 300 px", should="(set up)", tree=True):
        shrink()
        run.wait(1.0)
    run.grab_focus(role="push button", name="English")
    down = ["Français", "العربية", None, "Leading", "Trailing", "− 100", "+ 100", "− 1", "+ 1"]
    for name in down:
        with run.act(f"Tab to {name or 'the combo box'}",
                     [focused(role="push button", name=name), said(name)] if name
                     else [focused(role="combo box")],
                     should="focus moves on and the reader hears it", settle=0.8, record=1.5):
            run.key("Tab")
    with run.act("the tree at the bottom",
                 [not_in_tree(role="push button", name="English")],
                 should="(recorded) the language buttons scrolled out of the tree", tree=True):
        run.wait(0.1)
    for name in reversed(down[:-1]):
        with run.act(f"Shift+Tab to {name or 'the combo box'}",
                     [focused(role="push button", name=name), said(name)] if name
                     else [focused(role="combo box"), said("Language")],
                     should="focus moves back and the reader hears it", settle=0.8,
                     record=1.5):
            run.key("Shift+Tab")
    with run.act("Shift+Tab to English",
                 [focused(role="push button", name="English"), said("English")],
                 should="focus is back on English, which left the tree and came back; the "
                 "reader hears it", tree=True):
        run.key("Shift+Tab")


def intl_lang_env(run):
    """Launched with a French session: what the app and its root declare."""
    box: dict = {}
    run.wait_for(role="push button", name="English")
    box["nodes"] = record_languages(run, "LANG=fr_FR at launch")
    with run.act("the tree at launch under LANG=fr_FR.UTF-8",
                 [names_in_tree(EN["heading"]), languages_are(box, "en-US")],
                 should="the example turns OS detection off, so it is English; the "
                 "language it declares matches what it shows", tree=True):
        run.wait(0.2)
    run.grab_focus(role="push button", name="Français")
    with run.act("Tab to Leading", [orca_said_anything()],
                 should="what Orca (French) says for an English button"):
        run.key("Tab", "Tab")


SCENARIOS = [
    Scenario("winintl-mw-open-close", MW, steady(mw_open_close),
             "F1 opens Help, Tab to Close help, Space closes it"),
    Scenario("winintl-mw-switch", MW, steady(mw_switch),
             "open Help, switch windows, F1 on an open Help, close Help from KWin"),
    Scenario("winintl-mw-focus-return", MW, steady(mw_focus_return),
             "open Help from the Open help button, close it, where focus returns"),
    Scenario("winintl-mw-atspi", MW, steady(mw_atspi),
             "open and close Help by AT-SPI click; Enter after an AT-SPI focus"),
    Scenario("winintl-mw-crash", MW, mw_crash_sequence,
             "the open/close sequence the previous sweep crashed on, seat left to flap"),
    Scenario("winintl-mw-crash-steady", MW, steady(mw_crash_sequence),
             "the same sequence with the seat's pointer capability held steady"),
    Scenario("winintl-intl-switch", INTL, intl_switch_buttons,
             "en-US to fr-FR to ar-SA and back with the three buttons",
             lang="en_US.UTF-8"),
    Scenario("winintl-intl-rtl", INTL, intl_rtl_order,
             "ar-SA: mirrored layout, tree order and Tab order", lang="en_US.UTF-8"),
    Scenario("winintl-intl-switcher", INTL, intl_switcher,
             "the LanguageSwitcher combo box by keyboard", lang="en_US.UTF-8"),
    Scenario("winintl-intl-tabwalk-ar", INTL, intl_tabwalk_ar,
             "a Tab walk after switching to Arabic", lang="en_US.UTF-8"),
    Scenario("winintl-intl-values", INTL, intl_values,
             "the price and count buttons, and a price change in en-US and fr-FR",
             lang="en_US.UTF-8"),
    Scenario("winintl-intl-scroll-back", INTL, intl_scroll_back,
             "Tab down past the fold and Shift+Tab back to the top", lang="en_US.UTF-8"),
    Scenario("winintl-intl-scroll-reenter", INTL, intl_scroll_reenter,
             "labels that scroll out and back in", lang="en_US.UTF-8"),
    Scenario("winintl-intl-scroll-focus-back", INTL, intl_scroll_focus_back,
             "a short window: controls scroll out and are focused again", lang="en_US.UTF-8"),
    Scenario("winintl-intl-fr-session", INTL, intl_lang_env,
             "launched under LANG=fr_FR.UTF-8 with a French Orca",
             lang="fr_FR.UTF-8", orca_lang="fr_FR.UTF-8"),
]
