# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Descriptor parsing, the announce fallback, and the wait loop."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from teksilo_probe import bridge  # noqa: E402


def descriptor(pid: int = 1234, **overrides) -> dict:
    data = {
        "version": 1,
        "pid": pid,
        "transport": "unix",
        "address": "/run/user/1000/teksilo-automation/1234.sock",
        "token": "11111111-2222-3333-4444-555555555555",
        "app": "my-app",
        "started_unix_ms": 1770000000000,
    }
    data.update(overrides)
    return data


class DescriptorTests(unittest.TestCase):
    def test_parses_the_flattened_endpoint(self):
        """`endpoint` is `#[serde(flatten)]`-ed: transport/address sit at the top."""
        got = bridge.parse_descriptor(json.dumps(descriptor()))
        self.assertEqual(got.pid, 1234)
        self.assertEqual(got.transport, "unix")
        self.assertEqual(got.endpoint, "/run/user/1000/teksilo-automation/1234.sock")
        self.assertEqual(got.token, "11111111-2222-3333-4444-555555555555")
        self.assertEqual(got.app, "my-app")

    def test_named_pipe_descriptor(self):
        got = bridge.parse_descriptor(json.dumps(descriptor(
            transport="named_pipe", address=r"\\.\pipe\teksilo-automation-1234")))
        self.assertEqual(got.transport, "named_pipe")
        self.assertTrue(got.endpoint.startswith("\\\\"))

    def test_bytes_are_accepted(self):
        got = bridge.parse_descriptor(json.dumps(descriptor()).encode("utf-8"))
        self.assertEqual(got.pid, 1234)

    def test_a_descriptor_without_a_token_is_refused(self):
        body = descriptor()
        del body["token"]
        with self.assertRaises(ValueError):
            bridge.parse_descriptor(json.dumps(body))

    def test_optional_fields_may_be_absent(self):
        body = descriptor()
        del body["app"]
        del body["started_unix_ms"]
        got = bridge.parse_descriptor(json.dumps(body))
        self.assertIsNone(got.app)

    def test_half_written_file_reads_as_absent_not_as_an_error(self):
        """This polls a path another process is creating; a torn read is normal."""
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "1234.json"
            path.write_text('{"version": 1, "pid": 1234, "addr')
            self.assertIsNone(bridge.read_descriptor(path))

    def test_missing_file_reads_as_absent(self):
        self.assertIsNone(bridge.read_descriptor("/definitely/not/here.json"))

    def test_pid_must_match_the_file_name(self):
        """A mismatch means the file was replaced by a pid-recycled process."""
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "1234.json"
            path.write_text(json.dumps(descriptor(pid=9999)))
            self.assertIsNone(bridge.read_descriptor(path))

    def test_read_descriptor_records_its_own_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "1234.json"
            path.write_text(json.dumps(descriptor()))
            got = bridge.read_descriptor(path)
            self.assertEqual(got.descriptor, str(path))


class AnnounceFallbackTests(unittest.TestCase):
    """0.9.3 renamed `bridge socket = ` to `bridge endpoint = `.

    Every probe's regex missed it, waited out a 60 s timeout, and reported "no
    bridge socket" for an app whose bridge was up the whole time. Both spellings
    are accepted so that cannot happen again in either direction.
    """

    CURRENT = (
        "teksilo-automation: bridge endpoint = /run/user/1000/tk/9.sock\n"
        "teksilo-automation: descriptor = /run/user/1000/tk/9.json\n"
        "TEKSILO_AUTOMATION_TOKEN=abc-123\n"
    )
    LEGACY = (
        "teksilo-automation: bridge socket = /run/user/1000/tk/9.sock\n"
        "TEKSILO_AUTOMATION_TOKEN=abc-123\n"
    )

    def test_accepts_endpoint(self):
        got = bridge.parse_announce(self.CURRENT)
        self.assertEqual(got.endpoint, "/run/user/1000/tk/9.sock")
        self.assertEqual(got.token, "abc-123")
        self.assertEqual(got.descriptor, "/run/user/1000/tk/9.json")

    def test_accepts_the_legacy_socket_spelling(self):
        got = bridge.parse_announce(self.LEGACY)
        self.assertEqual(got.endpoint, "/run/user/1000/tk/9.sock")
        self.assertEqual(got.token, "abc-123")
        self.assertIsNone(got.descriptor)

    def test_endpoint_without_a_token_is_not_yet_a_bridge(self):
        self.assertIsNone(bridge.parse_announce(
            "teksilo-automation: bridge endpoint = /run/x.sock\n"))

    def test_nothing_announced(self):
        self.assertIsNone(bridge.parse_announce("starting up\n"))

    def test_a_windows_pipe_announce_is_recognised_as_such(self):
        text = ("teksilo-automation: bridge endpoint = \\\\.\\pipe\\tk-9\n"
                "TEKSILO_AUTOMATION_TOKEN=abc-123\n")
        self.assertEqual(bridge.parse_announce(text).transport, "named_pipe")

    def test_the_announce_is_found_among_other_log_lines(self):
        noisy = "wgpu: picked adapter\n" + self.CURRENT + "window opened\n"
        self.assertEqual(bridge.parse_announce(noisy).token, "abc-123")


class RuntimeDirTests(unittest.TestCase):
    def test_linux_prefers_xdg_runtime_dir(self):
        with mock.patch.object(bridge.sys, "platform", "linux"), \
                mock.patch.dict(os.environ, {"XDG_RUNTIME_DIR": "/run/user/1000"}):
            self.assertEqual(bridge.runtime_dir(), Path("/run/user/1000"))

    def test_macos_uses_tmpdir_not_xdg(self):
        """macOS never sets XDG_RUNTIME_DIR; honouring it would land in /tmp."""
        with mock.patch.object(bridge.sys, "platform", "darwin"), \
                mock.patch.dict(os.environ, {"TMPDIR": "/var/folders/ab/T/"}), \
                mock.patch.object(bridge.tempfile, "gettempdir",
                                  return_value="/var/folders/ab/T"):
            self.assertEqual(bridge.runtime_dir(), Path("/var/folders/ab/T"))

    def test_windows_uses_localappdata_teksilo(self):
        with mock.patch.object(bridge.sys, "platform", "win32"), \
                mock.patch.dict(os.environ, {"LOCALAPPDATA": r"C:\Users\x\AppData\Local"}):
            self.assertEqual(bridge.runtime_dir().name, "Teksilo")

    def test_descriptor_path_is_named_after_the_pid(self):
        self.assertEqual(bridge.descriptor_path(77).name, "77.json")
        self.assertEqual(bridge.descriptor_path(77).parent.name, "teksilo-automation")


class WaitTests(unittest.TestCase):
    def test_descriptor_is_preferred_over_the_announce(self):
        with tempfile.TemporaryDirectory() as tmp:
            socket = Path(tmp) / "9.sock"
            socket.touch()
            path = Path(tmp) / "9.json"
            path.write_text(json.dumps(descriptor(pid=9, address=str(socket),
                                                  token="from-descriptor")))
            log = Path(tmp) / "app.log"
            log.write_text("teksilo-automation: bridge endpoint = /elsewhere.sock\n"
                           "TEKSILO_AUTOMATION_TOKEN=from-announce\n")
            with mock.patch.object(bridge, "descriptor_path", return_value=path):
                got = bridge.wait_for_bridge(9, timeout=1.0, log=str(log))
            self.assertEqual(got.token, "from-descriptor")

    def test_falls_back_to_the_announce_when_no_descriptor_exists(self):
        with tempfile.TemporaryDirectory() as tmp:
            socket = Path(tmp) / "9.sock"
            socket.touch()
            log = Path(tmp) / "app.log"
            log.write_text(f"teksilo-automation: bridge socket = {socket}\n"
                           "TEKSILO_AUTOMATION_TOKEN=abc\n")
            missing = Path(tmp) / "absent.json"
            with mock.patch.object(bridge, "descriptor_path", return_value=missing):
                got = bridge.wait_for_bridge(9, timeout=1.0, log=str(log))
            self.assertEqual(got.token, "abc")
            self.assertEqual(got.pid, 9)

    def test_announced_before_bound_is_reported_as_the_race_it_is(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "app.log"
            log.write_text("teksilo-automation: bridge endpoint = /nope/never.sock\n"
                           "TEKSILO_AUTOMATION_TOKEN=abc\n")
            with mock.patch.object(bridge, "descriptor_path",
                                   return_value=Path(tmp) / "absent.json"):
                with self.assertRaises(RuntimeError) as caught:
                    bridge.wait_for_bridge(9, timeout=1.0, log=str(log))
            self.assertIn("announce-before-bind", str(caught.exception))

    def test_a_foreign_token_is_refused(self):
        """We pinned a token; a different one means a pid-recycled process."""
        with tempfile.TemporaryDirectory() as tmp:
            socket = Path(tmp) / "9.sock"
            socket.touch()
            path = Path(tmp) / "9.json"
            path.write_text(json.dumps(descriptor(pid=9, address=str(socket),
                                                  token="theirs")))
            with mock.patch.object(bridge, "descriptor_path", return_value=path):
                with self.assertRaises(RuntimeError) as caught:
                    bridge.wait_for_bridge(9, timeout=1.0, token="ours")
            self.assertIn("another process", str(caught.exception))

    def test_a_dead_process_fails_immediately_with_the_log_tail(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "app.log"
            log.write_text("thread 'main' panicked at src/main.rs:1\n")
            proc = subprocess.Popen([sys.executable, "-c", "raise SystemExit(7)"])
            proc.wait()
            with mock.patch.object(bridge, "descriptor_path",
                                   return_value=Path(tmp) / "absent.json"):
                with self.assertRaises(RuntimeError) as caught:
                    bridge.wait_for_bridge(proc.pid, timeout=30.0, log=str(log),
                                           proc=proc)
            message = str(caught.exception)
            self.assertIn("exited (code 7)", message)
            self.assertIn("panicked", message)

    def test_timeout_names_the_descriptor_it_looked_for(self):
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.object(bridge, "descriptor_path",
                                   return_value=Path(tmp) / "absent.json"):
                with self.assertRaises(RuntimeError) as caught:
                    bridge.wait_for_bridge(9, timeout=0.2)
            self.assertIn("absent.json", str(caught.exception))
            self.assertIn("debug_assertions", str(caught.exception))


class LaunchTests(unittest.TestCase):
    def test_the_token_is_pinned_before_the_process_exists(self):
        """The app honours $TEKSILO_AUTOMATION_TOKEN, so nothing need be scraped."""
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "out.log"
            app = bridge.launch(
                [sys.executable, "-c",
                 "import os,sys; sys.stderr.write(os.environ['TEKSILO_AUTOMATION_TOKEN'])"],
                token="pinned-token", log=str(log))
            app.proc.wait(timeout=30)
            self.assertEqual(app.token, "pinned-token")
            self.assertIn("pinned-token", log.read_text())

    def test_a_token_is_generated_when_none_is_given(self):
        with tempfile.TemporaryDirectory() as tmp:
            app = bridge.launch([sys.executable, "-c", "pass"],
                                log=str(Path(tmp) / "out.log"))
            app.proc.wait(timeout=30)
            self.assertTrue(app.token)


class McpArgvTests(unittest.TestCase):
    def test_attach_pid_is_preferred(self):
        got = bridge.mcp_argv(bridge.Bridge(pid=42, endpoint="/s", token="t"), "mcp")
        self.assertEqual(got, ["mcp", "--attach-pid", "42"])

    def test_connect_is_the_fallback_when_the_pid_is_unknown(self):
        got = bridge.mcp_argv(bridge.Bridge(pid=None, endpoint="/s", token="t"), "mcp")
        self.assertEqual(got, ["mcp", "--connect", "/s", "--token", "t"])


if __name__ == "__main__":
    unittest.main()
