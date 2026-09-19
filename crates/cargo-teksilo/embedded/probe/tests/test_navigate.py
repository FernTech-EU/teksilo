# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Scroll-direction discovery, the re-find discipline, and the keyboard route.

Driven by a fake virtualized view: a list of row labels with a viewport that
only *realises* the rows inside it, which is the behaviour every one of these
helpers exists to cope with. A probe that passes against this fake is a probe
that will not re-learn the four lessons in `navigate`'s docstring.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from teksilo_probe import navigate, tree  # noqa: E402

ROW_HEIGHT = 24.0
VIEWPORT_ROWS = 5


class FakeView:
    """A virtualized list, faithful in the three ways that matter.

    * Only the rows inside the viewport exist as nodes at all.
    * A row realised again after scrolling away gets a **fresh id**.
    * The scroll clamps at both ends, so a wheel past the end changes nothing.
    """

    def __init__(self, labels, first=0):
        self.labels = list(labels)
        self.first = first
        self.next_id = 100
        self.ids: dict[str, int] = {}
        self.scrolls: list[float] = []
        self.keys: list[str] = []
        self.pointer: list[tuple] = []
        self.settles = 0

    # -- the view itself ---------------------------------------------------

    def visible(self):
        return self.labels[self.first:self.first + VIEWPORT_ROWS]

    def snapshot(self):
        rows = [{"id": 1, "role": "Tree", "label": "The List",
                 "bounds": {"x": 0, "y": 0, "width": 200,
                            "height": ROW_HEIGHT * VIEWPORT_ROWS}}]
        for index, label in enumerate(self.visible()):
            # A fresh id every time a row is realised — a virtualizer rebuilds
            # the widget, and ids are stable for a widget's lifetime only.
            self.next_id += 1
            self.ids[label] = self.next_id
            rows.append({
                "id": self.next_id, "role": "TreeItem", "label": label,
                "bounds": {"x": 0, "y": index * ROW_HEIGHT, "width": 200,
                           "height": ROW_HEIGHT},
                "actions": ["click", "focus"],
            })
        return {"nodes": rows}

    def scroll(self, dy):
        self.scrolls.append(dy)
        step = int(round(-dy / ROW_HEIGHT))
        self.first = max(0, min(len(self.labels) - VIEWPORT_ROWS, self.first + step))

    # -- the session surface ----------------------------------------------

    def call(self, tool, **args):
        if tool == "snapshot_tree":
            return self.snapshot()
        if tool == "scroll":
            self.scroll(args.get("dy", 0.0))
            return {}
        if tool == "inject_key":
            self.keys.append(args["key"])
            return {}
        if tool == "inject_pointer":
            self.pointer.append((args.get("x"), args.get("y"), args.get("action")))
            return {}
        if tool == "invoke_action":
            self.pointer.append(("action", args["action"], args["node"]))
            return {}
        if tool in ("settle", "expand"):
            return {}
        raise AssertionError(f"unexpected tool {tool}")

    def settle(self, **kw):
        self.settles += 1
        return {}


ALPHABET = [f"Row {i:02d}" for i in range(20)]

#: One viewport of this fake, in pixels.
#:
#: `navigate.SCROLL_STEP` is one viewport of a *real* teksilo list, because a
#: notch smaller than the realization buffer reveals nothing and reads as the
#: end of the list. This fake's viewport is 120 px, so the tests pass the
#: equivalent rather than inheriting a constant scaled for real UI — inheriting
#: it would clamp to the end on the first notch and test nothing.
FAKE_STEP = ROW_HEIGHT * VIEWPORT_ROWS


class SignatureTests(unittest.TestCase):
    def test_labels_not_ids(self):
        """Ids churn on every rebuild the scroll itself causes, so an id-based
        signature reports 'something changed' on a scroll that moved nothing."""
        view = FakeView(ALPHABET)
        first = navigate.visible_signature(tree.find_all(view, role="TreeItem"))
        second = navigate.visible_signature(tree.find_all(view, role="TreeItem"))
        self.assertEqual(first, second)

    def test_a_moved_view_has_a_different_signature(self):
        view = FakeView(ALPHABET)
        before = navigate.visible_signature(tree.find_all(view, role="TreeItem"))
        view.scroll(-ROW_HEIGHT * 3)
        after = navigate.visible_signature(tree.find_all(view, role="TreeItem"))
        self.assertNotEqual(before, after)


class ScrollDiscoveryTests(unittest.TestCase):
    def test_finds_a_row_below_the_fold(self):
        view = FakeView(ALPHABET)
        got = navigate.scroll_until_found(view, 1, role="TreeItem", label="Row 18",
                                          row_role="TreeItem", step=FAKE_STEP, pause=0.0)
        self.assertIsNotNone(got)
        self.assertEqual(got["label"], "Row 18")

    def test_a_visible_row_costs_no_scroll_at_all(self):
        view = FakeView(ALPHABET)
        got = navigate.scroll_until_found(view, 1, role="TreeItem", label="Row 02",
                                          row_role="TreeItem", step=FAKE_STEP, pause=0.0)
        self.assertEqual(got["label"], "Row 02")
        self.assertEqual(view.scrolls, [])

    def test_direction_flips_for_a_row_above_the_start(self):
        """A fixed 'wheel down until found' can never reach a row above the
        starting position, and the result is indistinguishable from absence."""
        view = FakeView(ALPHABET, first=len(ALPHABET) - VIEWPORT_ROWS)
        got = navigate.scroll_until_found(view, 1, role="TreeItem", label="Row 00",
                                          row_role="TreeItem", step=FAKE_STEP, pause=0.0)
        self.assertIsNotNone(got)
        self.assertEqual(got["label"], "Row 00")
        self.assertTrue(any(d > 0 for d in view.scrolls),
                        "never tried the other direction")

    def test_both_ends_reached_reports_absent_without_burning_the_limit(self):
        view = FakeView(ALPHABET)
        got = navigate.scroll_until_found(view, 1, role="TreeItem",
                                          label="Row 99", row_role="TreeItem",
                                          limit=24, step=FAKE_STEP, pause=0.0)
        self.assertIsNone(got)
        # It stops as soon as both ends have been reached, rather than wheeling
        # 24 times into a clamped viewport.
        self.assertLess(len(view.scrolls), 24)

    def test_a_short_list_that_never_scrolls_is_still_answered(self):
        view = FakeView(["only", "a", "few"])
        self.assertIsNone(navigate.scroll_until_found(
            view, 1, role="TreeItem", label="missing", row_role="TreeItem",
            step=FAKE_STEP, pause=0.0))
        self.assertEqual(navigate.scroll_until_found(
            view, 1, role="TreeItem", label="a", row_role="TreeItem",
            step=FAKE_STEP, pause=0.0)["label"], "a")

    def test_the_container_may_be_given_as_a_node(self):
        view = FakeView(ALPHABET)
        container = tree.find(view, role="Tree")
        got = navigate.scroll_until_found(view, container, role="TreeItem",
                                          label="Row 10", row_role="TreeItem",
                                          step=FAKE_STEP, pause=0.0)
        self.assertIsNotNone(got)

    def test_a_custom_predicate_is_honoured(self):
        view = FakeView(ALPHABET)
        got = navigate.scroll_until(
            view, 1,
            lambda rows: next((r for r in rows if r.get("label") == "Row 15"), None),
            role="TreeItem", pause=0.0)
        self.assertEqual(got["label"], "Row 15")


class RevealTests(unittest.TestCase):
    def test_the_node_is_re_found_between_the_scroll_and_the_click(self):
        """`scroll_into_view` rebuilds the row; the pre-scroll id is dead."""
        view = FakeView(ALPHABET)
        seen: list[int] = []

        def refind():
            hit = tree.find(view, role="TreeItem", label="Row 02")
            if hit:
                seen.append(hit["id"])
            return hit

        # Advertise `scroll_into_view` so the reveal branch is exercised.
        original = view.snapshot

        def with_reveal():
            payload = original()
            for row in payload["nodes"]:
                if row["role"] == "TreeItem":
                    row["actions"] = ["click", "scroll_into_view"]
            return payload

        view.snapshot = with_reveal
        clicked = navigate.reveal_and_click(view, refind, pause=0.0)
        self.assertIsNotNone(clicked)
        self.assertEqual(len(seen), 2, "refind was not called again after the scroll")
        self.assertNotEqual(seen[0], seen[1], "the fake did not rebuild the row")
        self.assertEqual(clicked["id"], seen[1], "clicked the pre-scroll id")

    def test_a_refind_that_comes_up_empty_is_none_not_a_crash(self):
        view = FakeView(ALPHABET)
        self.assertIsNone(navigate.reveal_and_click(view, lambda: None, pause=0.0))

    def test_click_prefers_the_at_action_over_coordinates(self):
        view = FakeView(ALPHABET)
        row = tree.find(view, role="TreeItem", label="Row 01")
        navigate.click(view, row)
        self.assertEqual(view.pointer[0][:2], ("action", "click"))

    def test_click_falls_back_to_a_pointer_when_no_action_is_advertised(self):
        view = FakeView(ALPHABET)
        row = dict(tree.find(view, role="TreeItem", label="Row 01"), actions=[])
        navigate.click(view, row)
        self.assertEqual(view.pointer[0][2], "click")


class KeyboardTests(unittest.TestCase):
    def test_already_there_presses_nothing(self):
        """A view that remembers its last selection can start on the target, and
        an unconditional anchor click would leave it."""
        view = FakeView(ALPHABET)
        anchor = tree.find(view, role="TreeItem", label="Row 00")
        got = navigate.select_via_keyboard(view, lambda: "arrived", anchor=anchor,
                                           pause=0.0)
        self.assertEqual(got, "arrived")
        self.assertEqual(view.keys, [])
        self.assertEqual(view.pointer, [])

    def test_steps_down_then_up(self):
        view = FakeView(ALPHABET)
        anchor = tree.find(view, role="TreeItem", label="Row 00")
        state = {"n": 0}

        def reached():
            state["n"] += 1
            return "there" if state["n"] > 4 else None

        got = navigate.select_via_keyboard(view, reached, anchor=anchor, steps=10,
                                           pause=0.0)
        self.assertEqual(got, "there")
        self.assertEqual(view.keys, ["Down"] * 3)

    def test_both_directions_are_tried(self):
        """Walking one direction only reports a reachable target as absent."""
        view = FakeView(ALPHABET)
        anchor = tree.find(view, role="TreeItem", label="Row 02")
        got = navigate.select_via_keyboard(view, lambda: None, anchor=anchor,
                                           steps=2, pause=0.0)
        self.assertIsNone(got)
        self.assertEqual(view.keys, ["Down", "Down", "Up", "Up"])

    def test_no_anchor_is_none_rather_than_a_blind_walk(self):
        view = FakeView(ALPHABET)
        self.assertIsNone(navigate.select_via_keyboard(view, lambda: None, anchor=None))
        self.assertEqual(view.keys, [])


class FallbackTests(unittest.TestCase):
    def test_the_first_truthy_strategy_wins(self):
        tried: list[str] = []

        def make(name, result):
            def strategy():
                tried.append(name)
                return result
            return strategy

        got = navigate.first_available(make("at", None), make("overflow", "cell"),
                                       make("keyboard", "never"))
        self.assertEqual(got, "cell")
        self.assertEqual(tried, ["at", "overflow"])

    def test_all_empty_is_none(self):
        self.assertIsNone(navigate.first_available(lambda: None, lambda: []))

    def test_a_broken_strategy_is_not_swallowed(self):
        """A broken strategy and an inapplicable one are different things."""
        def boom():
            raise RuntimeError("the overflow menu never opened")

        with self.assertRaises(RuntimeError):
            navigate.first_available(lambda: None, boom, lambda: "unreached")


class ExpandAllTests(unittest.TestCase):
    def test_expands_until_nothing_more_opens(self):
        class Tree:
            def __init__(self):
                self.opened: list[int] = []
                self.depth = 0

            def call(self, tool, **args):
                if tool == "snapshot_tree":
                    if self.depth >= 2:
                        return {"nodes": [{"id": 9, "role": "TreeItem",
                                           "expanded": True, "actions": ["collapse"]}]}
                    return {"nodes": [{"id": self.depth, "role": "TreeItem",
                                       "expanded": False, "actions": ["expand"]}]}
                if tool == "expand":
                    self.opened.append(args["node"])
                    self.depth += 1
                    return {}
                raise AssertionError(tool)

            def settle(self, **kw):
                return {}

        fake = Tree()
        self.assertEqual(navigate.expand_all(fake), 2)
        self.assertEqual(fake.opened, [0, 1])


if __name__ == "__main__":
    unittest.main()
