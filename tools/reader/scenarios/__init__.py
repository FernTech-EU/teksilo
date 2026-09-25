# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""One module per example; each lists its `SCENARIOS`.

A scenario states, act by act, what a screen reader user should get, as
checks from `reader_lib.checks`; the harness records what they get. See
`docs/a11y/reader-harness.md` for how to write one.

No module here may take the name of a standard-library module: Python run
from this directory would import it in the library's place.
"""
