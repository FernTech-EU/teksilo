# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The JSON-RPC conversation and the payload unwrapping."""

from __future__ import annotations

import base64
import json
import os
import subprocess
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from teksilo_probe import session as sess  # noqa: E402
from teksilo_probe import shot  # noqa: E402
from teksilo_probe.bridge import Bridge  # noqa: E402

FAKE = str(Path(__file__).resolve().parent / "fake_mcp.py")


def start(script: dict | None = None, **extra: str) -> sess.Session:
    env = dict(os.environ, FAKE_MCP_SCRIPT=json.dumps(script or {}), **extra)
    proc = subprocess.Popen(
        [sys.executable, FAKE],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1, env=env,
    )
    return sess.Session(proc, timeout=15.0)


class UnwrapTests(unittest.TestCase):
    def test_structured_content_wins(self):
        result = {"structuredContent": {"nodes": [1]},
                  "content": [{"type": "text", "text": '{"nodes": []}'}]}
        self.assertEqual(sess.unwrap(result), {"nodes": [1]})

    def test_falls_back_to_text_blocks(self):
        result = {"content": [{"type": "text", "text": '{"a": 1}'}]}
        self.assertEqual(sess.unwrap(result), {"a": 1})

    def test_text_blocks_are_concatenated(self):
        result = {"content": [{"type": "text", "text": '{"a":'},
                              {"type": "text", "text": ' 1}'}]}
        self.assertEqual(sess.unwrap(result), {"a": 1})

    def test_array_payload(self):
        """A bare list is a payload shape too — nothing may assume a dict."""
        result = {"content": [{"type": "text", "text": '[{"id": 1}, {"id": 2}]'}]}
        self.assertEqual(sess.unwrap(result), [{"id": 1}, {"id": 2}])

    def test_structured_array_payload(self):
        self.assertEqual(sess.unwrap({"structuredContent": [1, 2]}), [1, 2])

    def test_non_json_text_is_not_an_error(self):
        result = {"content": [{"type": "text", "text": "everything is fine"}]}
        self.assertEqual(sess.unwrap(result), {"_text": "everything is fine"})

    def test_image_blocks_are_not_read_as_payload(self):
        result = {"content": [{"type": "image", "data": "AAA", "mimeType": "image/png"}]}
        self.assertEqual(sess.unwrap(result), {"_text": ""})

    def test_explicit_null_structured_content_falls_through(self):
        result = {"structuredContent": None,
                  "content": [{"type": "text", "text": '{"a": 2}'}]}
        self.assertEqual(sess.unwrap(result), {"a": 2})


class ConversationTests(unittest.TestCase):
    def test_handshake_and_call(self):
        with start({"snapshot_tree": {"structured": {"nodes": [{"id": 7}]}}}) as s:
            s.initialize("unit-test")
            self.assertEqual(s.call("snapshot_tree"), {"nodes": [{"id": 7}]})

    def test_arguments_reach_the_wire_verbatim(self):
        with start() as s:
            s.initialize()
            echoed = s.call("inject_pointer", x=1.0, y=2.0, action="click")
            self.assertEqual(echoed["echo"], {"x": 1.0, "y": 2.0, "action": "click"})

    def test_tool_error_raises_with_the_code(self):
        script = {"find_node": {"error": {"code": "NOT_FOUND", "message": "no such node"}}}
        with start(script) as s:
            s.initialize()
            with self.assertRaises(sess.ToolError) as caught:
                s.call("find_node", label="nope")
            self.assertEqual(caught.exception.code, "NOT_FOUND")
            self.assertIn("no such node", str(caught.exception))

    def test_try_call_reports_an_error_without_raising(self):
        script = {"screenshot": {"error": {"code": "GPU_UNAVAILABLE", "message": "no adapter"}}}
        with start(script) as s:
            s.initialize()
            result = s.try_call("screenshot")
            self.assertFalse(result)
            self.assertEqual(result.code, "GPU_UNAVAILABLE")

    def test_replies_are_matched_by_id(self):
        """Two calls in a row must not cross their replies."""
        with start() as s:
            s.initialize()
            self.assertEqual(s.call("a", n=1)["echo"], {"n": 1})
            self.assertEqual(s.call("b", n=2)["echo"], {"n": 2})

    def test_a_silent_tool_times_out_with_the_stderr_tail(self):
        with start({"settle": {"silent": True}}) as s:
            s.initialize()
            with self.assertRaises(sess.ProbeError) as caught:
                s.call("settle", _timeout=0.6)
            self.assertIn("no MCP reply", str(caught.exception))

    def test_generated_wrappers_drop_omitted_arguments(self):
        with start() as s:
            s.initialize()
            echoed = s.tools.snapshot_tree()
            self.assertEqual(echoed["echo"], {})
            echoed = s.tools.snapshot_tree(max_depth=3)
            self.assertEqual(echoed["echo"], {"max_depth": 3})

    def test_unknown_tool_on_the_proxy_names_the_known_ones(self):
        with start() as s:
            s.initialize()
            with self.assertRaises(AttributeError) as caught:
                s.tools.definitely_not_a_tool
            self.assertIn("snapshot_tree", str(caught.exception))

    def test_close_is_idempotent(self):
        s = start()
        s.initialize()
        s.close()
        s.close()
        self.assertIsNotNone(s.proc.poll())


class ConnectTests(unittest.TestCase):
    def test_a_missing_client_says_how_to_install_it(self):
        """Not retryable: retrying for 30 s would bury the one useful sentence."""
        bridge = Bridge(pid=None, endpoint="/nope", token="tok")
        with self.assertRaises(sess.ProbeError) as caught:
            sess.connect(bridge, mcp=sys.executable + "-does-not-exist",
                         connect_timeout=5.0)
        self.assertIn("cargo install teksilo-automation-mcp", str(caught.exception))

    def test_a_client_that_exits_at_once_is_retried_then_reported(self):
        bridge = Bridge(pid=4321, endpoint="/nope", token="tok")
        with self.assertRaises(sess.ProbeError) as caught:
            # `python --attach-pid 4321` exits immediately, which is exactly the
            # shape of a client that cannot reach the app.
            sess.connect(bridge, mcp=sys.executable, connect_timeout=0.2)
        self.assertIn("could not attach", str(caught.exception))

    def test_the_token_travels_in_the_environment_not_the_command_line(self):
        """A command line is world-readable through /proc/<pid>/cmdline."""
        bridge = Bridge(pid=4321, endpoint="/run/x.sock", token="secret-token")
        env = sess._child_env(bridge)
        self.assertEqual(env["TEKSILO_AUTOMATION_TOKEN"], "secret-token")
        from teksilo_probe.bridge import mcp_argv

        argv = mcp_argv(bridge, "mcp")
        self.assertEqual(argv, ["mcp", "--attach-pid", "4321"])
        self.assertNotIn("secret-token", " ".join(argv))


class ScreenshotTests(unittest.TestCase):
    def test_image_block_is_decoded_to_disk(self):
        import tempfile

        png = b"\x89PNG\r\n\x1a\n-not-really"
        script = {"screenshot": {"image": base64.b64encode(png).decode(),
                                 "meta": {"width": 80, "height": 60, "scale": 2.0}}}
        with start(script) as s:
            s.initialize()
            with tempfile.TemporaryDirectory() as tmp:
                target = Path(tmp) / "sub" / "shot.png"
                meta = shot.save(s, target)
                self.assertEqual(target.read_bytes(), png)
                self.assertEqual(meta["scale"], 2.0)

    def test_gpu_unavailable_is_survivable(self):
        script = {"screenshot": {"error": {"code": "GPU_UNAVAILABLE", "message": "none"}}}
        with start(script) as s:
            s.initialize()
            self.assertIsNone(shot.try_save(s, "/tmp/never-written.png"))


if __name__ == "__main__":
    unittest.main()
