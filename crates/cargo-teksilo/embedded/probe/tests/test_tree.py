# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Searching the accessibility tree, and the geometry helpers."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from teksilo_probe import tree  # noqa: E402


def node(nid, role="Button", label=None, **extra):
    out = {"id": nid, "role": role}
    if label is not None:
        out["label"] = label
    out.update(extra)
    return out


SNAPSHOT = {
    "nodes": [
        node(1, "Window", "My App", children=[2, 5]),
        node(2, "Tab", "Binder", bounds={"x": 4, "y": 40, "width": 40, "height": 40},
             children=[3, 4]),
        node(3, "TreeItem", "Chapter 1", bounds={"x": 60, "y": 40, "width": 200,
                                                 "height": 24}),
        node(4, "TreeItem", "Chapter 2", bounds={"x": 60, "y": 64, "width": 200,
                                                 "height": 24}),
        node(5, "Button", "Settings", bounds={"x": 4, "y": 900, "width": 40,
                                              "height": 40}, actions=["click"]),
    ]
}


class FindTests(unittest.TestCase):
    def test_finds_by_role_and_label(self):
        hit = tree.find(SNAPSHOT, role="Button", label="Settings")
        self.assertEqual(hit["id"], 5)

    def test_role_and_label_matching_is_case_insensitive(self):
        """A role is a Debug name; a probe that must match capitalisation
        breaks on a rename that changed nothing."""
        self.assertEqual(tree.find(SNAPSHOT, role="button", label="  settings ")["id"], 5)

    def test_label_contains(self):
        self.assertEqual(tree.find(SNAPSHOT, label_contains="chapter")["id"], 3)

    def test_find_all_keeps_at_order(self):
        got = tree.find_all(SNAPSHOT, role="TreeItem")
        self.assertEqual([n["id"] for n in got], [3, 4])

    def test_a_predicate_composes_with_the_criteria(self):
        got = tree.find(SNAPSHOT, role="TreeItem",
                        pred=lambda n: n["bounds"]["y"] > 50)
        self.assertEqual(got["id"], 4)

    def test_absence_is_none_not_an_exception(self):
        self.assertIsNone(tree.find(SNAPSHOT, label="Nothing Here"))

    def test_a_raw_node_list_is_a_valid_source(self):
        self.assertEqual(tree.find(SNAPSHOT["nodes"], label="Binder")["id"], 2)

    def test_labels_lists_what_is_on_screen(self):
        self.assertIn("Settings", tree.labels(SNAPSHOT))
        self.assertEqual(tree.labels(SNAPSHOT, role="TreeItem"),
                         ["Chapter 1", "Chapter 2"])


class HierarchyTests(unittest.TestCase):
    def test_children_of(self):
        got = tree.children_of(SNAPSHOT, 2)
        self.assertEqual([n["id"] for n in got], [3, 4])

    def test_descendants_are_depth_first_and_exclude_the_root(self):
        got = tree.descendants(SNAPSHOT, 1)
        self.assertEqual([n["id"] for n in got], [2, 3, 4, 5])

    def test_a_cycle_cannot_loop_forever(self):
        looped = {"nodes": [node(1, children=[2]), node(2, children=[1])]}
        self.assertEqual([n["id"] for n in tree.descendants(looped, 1)], [2, 1])


class RegionTests(unittest.TestCase):
    def test_a_rail_node_is_in_the_rail(self):
        rail = tree.find(SNAPSHOT, label="Settings")
        self.assertTrue(tree.in_region(rail, max_x=56))

    def test_a_content_node_is_not(self):
        row = tree.find(SNAPSHOT, label="Chapter 1")
        self.assertFalse(tree.in_region(row, max_x=56))

    def test_a_node_wider_than_the_strip_is_still_in_it(self):
        """The origin, not the whole rectangle — otherwise a row whose bounds
        run past a narrow rail would be dropped, which is the very node being
        looked for."""
        wide = node(9, bounds={"x": 4, "y": 10, "width": 400, "height": 20})
        self.assertTrue(tree.in_region(wide, max_x=56))

    def test_vertical_bands(self):
        rail = tree.find(SNAPSHOT, label="Settings")
        self.assertTrue(tree.in_region(rail, min_y=800))
        self.assertFalse(tree.in_region(rail, max_y=100))

    def test_all_four_bounds_together(self):
        row = tree.find(SNAPSHOT, label="Chapter 2")
        self.assertTrue(tree.in_region(row, min_x=50, max_x=100, min_y=60, max_y=70))
        self.assertFalse(tree.in_region(row, min_x=50, max_x=100, min_y=60, max_y=63))

    def test_a_node_with_no_bounds_is_in_no_region(self):
        self.assertFalse(tree.in_region(node(9), max_x=56))
        self.assertFalse(tree.in_region({}, max_x=56))

    def test_no_constraints_accepts_anything_with_bounds(self):
        self.assertTrue(tree.in_region(tree.find(SNAPSHOT, label="Settings")))


class GeometryTests(unittest.TestCase):
    def test_center(self):
        self.assertEqual(tree.center(tree.find(SNAPSHOT, label="Settings")), (24.0, 920.0))

    def test_center_of_a_node_with_no_bounds_says_so(self):
        with self.assertRaises(ValueError):
            tree.center(node(9))

    def test_bounds_of_nothing_is_empty(self):
        self.assertEqual(tree.bounds(None), {})

    def test_text_of_joins_value_and_label(self):
        self.assertEqual(tree.text_of({"value": "Hi", "label": "Greeting"}), "hi greeting")


class FakeSession:
    """Answers `snapshot_tree` from a scripted list of snapshots."""

    def __init__(self, snapshots):
        self.snapshots = list(snapshots)
        self.calls = 0

    def call(self, tool, **args):
        assert tool == "snapshot_tree", tool
        self.calls += 1
        return self.snapshots[min(self.calls - 1, len(self.snapshots) - 1)]

    def settle(self, **kw):
        return {}


class WaitTests(unittest.TestCase):
    def test_wait_for_polls_until_the_marker_appears(self):
        """One snapshot straight after connect is not enough: the bridge answers
        before the app has opened a document."""
        session = FakeSession([{"nodes": []}, {"nodes": []},
                               {"nodes": [node(1, label="Untitled")]}])
        self.assertTrue(tree.wait_for(session, "untitled", timeout=5.0, interval=0.0))
        self.assertEqual(session.calls, 3)

    def test_wait_for_gives_up_and_says_so(self):
        session = FakeSession([{"nodes": []}])
        self.assertFalse(tree.wait_for(session, "never", timeout=0.05, interval=0.0))

    def test_wait_for_accepts_several_markers(self):
        session = FakeSession([{"nodes": [node(1, label="Général")]}])
        self.assertTrue(tree.wait_for(session, ("General", "Général"),
                                      timeout=1.0, interval=0.0))

    def test_wait_for_node_returns_the_node(self):
        session = FakeSession([{"nodes": []},
                               {"nodes": [node(3, "Button", "Save")]}])
        got = tree.wait_for_node(session, role="Button", label="Save",
                                 timeout=5.0, interval=0.0)
        self.assertEqual(got["id"], 3)


if __name__ == "__main__":
    unittest.main()
