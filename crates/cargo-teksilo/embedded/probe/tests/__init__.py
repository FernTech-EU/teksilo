# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Unit tests for `teksilo_probe`.

Everything testable without a running app: the JSON-RPC conversation (against a
fake MCP server that is a real subprocess), descriptor parsing, the announce
fallback, the version comparison, the `Report` exit codes, geometry, and the
scroll-direction discovery against a fake session.

Run them with::

    python3 -m unittest discover -s crates/cargo-teksilo/embedded/probe -t \\
        crates/cargo-teksilo/embedded/probe
"""
