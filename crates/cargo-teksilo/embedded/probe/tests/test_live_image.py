# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Aiming at a live picture's source pixels, and cutting it out of a screenshot."""

from __future__ import annotations

import base64
import json
import os
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from teksilo_probe import live_image  # noqa: E402
from teksilo_probe import session as sess  # noqa: E402

FAKE = str(Path(__file__).resolve().parent / "fake_mcp.py")


def start(script: dict | None = None) -> sess.Session:
    env = dict(os.environ, FAKE_MCP_SCRIPT=json.dumps(script or {}))
    proc = subprocess.Popen(
        [sys.executable, FAKE],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1, env=env,
    )
    return sess.Session(proc, timeout=15.0)


def pattern(width: int, height: int) -> bytes:
    """Every pixel distinct: `(x, y, x ^ y, 255 - x)`."""
    return bytes(
        v
        for y in range(height)
        for x in range(width)
        for v in (x * 7 % 256, y * 11 % 256, (x ^ y) % 256, (255 - x) % 256)
    )


def filtered_png(width: int, height: int, rgba: bytes, kinds: list[int],
                 colour: int = 6) -> bytes:
    """A PNG whose row `r` is written with filter `kinds[r % len(kinds)]`, as an
    encoder choosing filters adaptively would."""
    bpp = 4 if colour == 6 else 3
    if colour == 2:
        rgba = b"".join(rgba[i:i + 3] for i in range(0, len(rgba), 4))
    stride = width * bpp
    rows = [rgba[r * stride:(r + 1) * stride] for r in range(height)]
    out = b""
    previous = bytes(stride)
    for r, row in enumerate(rows):
        kind = kinds[r % len(kinds)]
        line = bytearray()
        for i, value in enumerate(row):
            left = row[i - bpp] if i >= bpp else 0
            up = previous[i]
            corner = previous[i - bpp] if i >= bpp else 0
            if kind == 0:
                predicted = 0
            elif kind == 1:
                predicted = left
            elif kind == 2:
                predicted = up
            elif kind == 3:
                predicted = (left + up) // 2
            else:
                estimate = left + up - corner
                pa, pb, pc = abs(estimate - left), abs(estimate - up), abs(estimate - corner)
                predicted = left if pa <= pb and pa <= pc else up if pb <= pc else corner
            line.append((value - predicted) & 0xFF)
        out += bytes([kind]) + bytes(line)
        previous = row

    def chunk(kind: bytes, body: bytes) -> bytes:
        crc = zlib.crc32(kind + body) & 0xFFFFFFFF
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc)

    header = struct.pack(">IIBBBBB", width, height, 8, colour, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(out)) + chunk(b"IEND", b""))


class PngTests(unittest.TestCase):
    def test_round_trip(self):
        rgba = pattern(5, 3)
        self.assertEqual(live_image.decode_png(live_image.encode_png(5, 3, rgba)),
                         (5, 3, rgba))

    def test_every_filter_type_decodes(self):
        rgba = pattern(9, 10)
        for kinds in ([0], [1], [2], [3], [4], [0, 1, 2, 3, 4]):
            with self.subTest(kinds=kinds):
                self.assertEqual(live_image.decode_png(filtered_png(9, 10, rgba, kinds)),
                                 (9, 10, rgba))

    def test_rgb_comes_back_opaque(self):
        rgba = bytes(v if i % 4 != 3 else 255 for i, v in enumerate(pattern(4, 4)))
        self.assertEqual(live_image.decode_png(filtered_png(4, 4, rgba, [4], colour=2)),
                         (4, 4, rgba))

    def test_what_it_cannot_read_is_refused(self):
        png = bytearray(live_image.encode_png(2, 2, pattern(2, 2)))
        with self.assertRaises(ValueError):
            live_image.decode_png(b"GIF89a")
        corrupt = bytearray(png)
        corrupt[-20] ^= 0xFF
        with self.assertRaises(ValueError):
            live_image.decode_png(bytes(corrupt))
        sixteen = bytearray(png)
        # IHDR's bit depth, and its checksum recomputed: well formed, not 8-bit.
        sixteen[24] = 16
        crc = zlib.crc32(bytes(sixteen[12:29])) & 0xFFFFFFFF
        sixteen[29:33] = struct.pack(">I", crc)
        with self.assertRaises(ValueError):
            live_image.decode_png(bytes(sixteen))


class AimingTests(unittest.TestCase):
    def test_click_source_aims_at_the_source_pixel(self):
        s = start()
        try:
            echoed = live_image.click_source(s, {"id": 9, "role": "Image"}, 10, 20,
                                             command=True)
            self.assertEqual(echoed["echo"], {"node": 9, "source": [10, 20],
                                              "action": "click", "command": True})
        finally:
            s.close()

    def test_map_queries_and_their_answers(self):
        s = start({"live_image_map": {"structured": {
            "source_point": [12.5, 40.25],
            "source_window_rect": {"x": 1.0, "y": 2.0, "width": 3.0, "height": 4.0},
            "pixel": [3, 4],
        }}})
        try:
            self.assertEqual(live_image.point_of(s, 9, 1, 2), (12.5, 40.25))
            self.assertEqual(live_image.rect_of(s, 9, (0, 0, 2, 2)),
                             {"x": 1.0, "y": 2.0, "width": 3.0, "height": 4.0})
            self.assertEqual(live_image.source_of(s, 9, (5.0, 6.0)), (3, 4))
        finally:
            s.close()

    def test_a_point_on_the_letterbox_is_no_pixel(self):
        s = start({"live_image_map": {"structured": {"pixel": None}}})
        try:
            self.assertIsNone(live_image.source_of(s, 9, (0.0, 0.0)))
        finally:
            s.close()

    def test_a_refusal_is_raised(self):
        s = start({"live_image_map": {"error": {"code": "NO_GEOMETRY",
                                                "message": "not laid out"}}})
        try:
            with self.assertRaises(sess.ToolError) as caught:
                live_image.source_of(s, 9, (0.0, 0.0))
            self.assertEqual(caught.exception.code, "NO_GEOMETRY")
        finally:
            s.close()


class ArgumentTests(unittest.TestCase):
    """What each map query sends, against the generated wrapper's own
    signature: a scripted reply answers whatever was asked."""

    def test_the_map_queries_send_what_they_name(self):
        recorder = mock.create_autospec(live_image.tools.live_image_map, return_value={
            "source_point": [1.0, 2.0],
            "source_window_rect": {"x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0},
            "pixel": [3, 4],
        })
        with mock.patch.object(live_image.tools, "live_image_map", recorder):
            live_image.point_of("session", {"id": 9}, 1, 2, window_id=3)
            live_image.rect_of("session", 9, (1, 2, 3, 4))
            live_image.source_of("session", 9, (5.0, 6.0))
        self.assertEqual(recorder.call_args_list, [
            mock.call("session", 9, source=[1, 2], window_id=3),
            mock.call("session", 9, source_rect=[1, 2, 3, 4], window_id=None),
            mock.call("session", 9, window=[5.0, 6.0], window_id=None),
        ])


class CropTests(unittest.TestCase):
    RGBA = pattern(6, 5)
    PNG = filtered_png(6, 5, RGBA, [0, 1, 2, 3, 4])

    @staticmethod
    def meta(live_images):
        return {"width": 6, "height": 5, "scale": 2.0, "live_images": live_images}

    def check_cut(self, picture, x0, y0):
        for y in range(picture.height):
            for x in range(picture.width):
                i = ((y0 + y) * 6 + x0 + x) * 4
                self.assertEqual(picture.pixel(x, y), tuple(self.RGBA[i:i + 4]))

    def test_the_picture_is_the_rect_the_screenshot_recorded(self):
        meta = self.meta([
            {"node": 4, "generation": 1, "deferred": False, "rect": [0, 0, 1, 1]},
            {"node": 9, "generation": 17, "deferred": True, "rect": [2, 1, 3, 2]},
        ])
        picture = live_image.crop_visible(self.PNG, meta, {"id": 9})
        self.assertEqual((picture.width, picture.height), (3, 2))
        self.assertEqual((picture.generation, picture.deferred, picture.scale),
                         (17, True, 2.0))
        self.check_cut(picture, 2, 1)
        with self.assertRaises(IndexError):
            picture.pixel(3, 0)
        with tempfile.TemporaryDirectory() as tmp:
            saved = picture.save(Path(tmp) / "deep" / "vm.png")
            self.assertEqual(live_image.decode_png(saved.read_bytes()),
                             (3, 2, picture.rgba))

    def test_a_picture_the_screenshot_did_not_draw_is_none(self):
        meta = self.meta([{"node": 4, "generation": 1, "deferred": False,
                           "rect": [0, 0, 1, 1]}])
        self.assertIsNone(live_image.crop_visible(self.PNG, meta, 9))
        self.assertIsNone(live_image.crop_visible(self.PNG, self.meta([]), 9))

    def test_a_rect_past_the_image_is_refused(self):
        meta = self.meta([{"node": 9, "generation": 1, "deferred": False,
                           "rect": [4, 0, 3, 1]}])
        with self.assertRaises(ValueError):
            live_image.crop_visible(self.PNG, meta, 9)

    def test_capture_screenshots_the_node_and_cuts_it(self):
        meta = self.meta([{"node": 9, "generation": 5, "deferred": False,
                           "rect": [1, 2, 4, 3]}])
        s = start({"screenshot": {"image": base64.b64encode(self.PNG).decode(),
                                  "meta": meta}})
        try:
            picture = live_image.capture(s, 9)
        finally:
            s.close()
        self.assertEqual((picture.width, picture.height, picture.generation), (4, 3, 5))
        self.check_cut(picture, 1, 2)

    def test_capture_turns_a_bad_rect_into_a_bad_reply(self):
        meta = self.meta([{"node": 9, "generation": 1, "deferred": False,
                           "rect": [4, 0, 3, 1]}])
        s = start({"screenshot": {"image": base64.b64encode(self.PNG).decode(),
                                  "meta": meta}})
        try:
            with self.assertRaises(sess.ToolError) as caught:
                live_image.capture(s, 9)
            self.assertEqual(caught.exception.code, "BAD_REPLY")
        finally:
            s.close()

    def test_no_gpu_is_raised(self):
        s = start({"screenshot": {"error": {"code": "GPU_UNAVAILABLE",
                                            "message": "no adapter"}}})
        try:
            with self.assertRaises(sess.ToolError) as caught:
                live_image.capture(s, 9)
            self.assertEqual(caught.exception.code, "GPU_UNAVAILABLE")
        finally:
            s.close()


if __name__ == "__main__":
    unittest.main()
