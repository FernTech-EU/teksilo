# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""A minimal MCP-over-stdio server, run as a real subprocess by the tests.

Exercises the framing the way the real client does — line-delimited JSON-RPC on
stdin/stdout — so `Session` is tested against a process, not a mock. Which
matters: the reader is a thread over a pipe, and a mock that hands back objects
would never notice a pipe that was never flushed.

Scripted through `$FAKE_MCP_SCRIPT`, a JSON object::

    {"tool_name": {"structured": {...}} | {"text": "..."} |
                  {"error": {"code": "...", "message": "..."}} |
                  {"image": "<base64>", "meta": {...}} |
                  {"silent": true}}
"""

from __future__ import annotations

import json
import os
import sys


def main() -> int:
    script = json.loads(os.environ.get("FAKE_MCP_SCRIPT") or "{}")
    if os.environ.get("FAKE_MCP_DIE_AT_ONCE"):
        return 3
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        method = message.get("method")
        if method == "initialize":
            if os.environ.get("FAKE_MCP_NO_HANDSHAKE"):
                continue
            _reply(message["id"], {"protocolVersion": "2024-11-05",
                                   "serverInfo": {"name": "fake", "version": "0"}})
        elif method == "notifications/initialized":
            continue
        elif method == "tools/call":
            params = message.get("params") or {}
            _tool(message["id"], params.get("name", ""), params.get("arguments") or {},
                  script)
        elif "id" in message:
            _reply(message["id"], {})
    return 0


def _tool(rpc_id, name, arguments, script) -> None:
    spec = script.get(name)
    if spec is None:
        # The default: echo the arguments back, so a test can assert on exactly
        # what reached the wire.
        _reply(rpc_id, {"content": [{"type": "text", "text": json.dumps(arguments)}],
                        "structuredContent": {"echo": arguments}})
        return
    if spec.get("silent"):
        return
    if "error" in spec:
        body = json.dumps(spec["error"])
        _reply(rpc_id, {"content": [{"type": "text", "text": body}],
                        "structuredContent": spec["error"], "isError": True})
        return
    if "image" in spec:
        _reply(rpc_id, {"content": [{"type": "image", "data": spec["image"],
                                     "mimeType": "image/png"},
                                    {"type": "text",
                                     "text": json.dumps(spec.get("meta") or {})}],
                        "structuredContent": spec.get("meta") or {}})
        return
    if "structured" in spec:
        _reply(rpc_id, {"content": [{"type": "text",
                                     "text": json.dumps(spec["structured"])}],
                        "structuredContent": spec["structured"]})
        return
    _reply(rpc_id, {"content": [{"type": "text", "text": spec.get("text", "")}]})


def _reply(rpc_id, result) -> None:
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": rpc_id, "result": result}) + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    raise SystemExit(main())
