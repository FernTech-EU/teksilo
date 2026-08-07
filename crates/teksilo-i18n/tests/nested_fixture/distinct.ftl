# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Distinctness fixture — four keys that all would have collided to
# `foo-bar-baz` under the old dash-joining scheme. Under option B
# each maps to its own key:
#
#   tr!(foo_bar_baz())       → foo-bar-baz       (flat)
#   tr!(foo::bar_baz())      → foo__bar-baz      (two seg)
#   tr!(foo_bar::baz())      → foo-bar__baz      (two seg)
#   tr!(foo::bar::baz())     → foo__bar__baz     (three seg)

foo-bar-baz = flat
foo__bar-baz = two-seg-a
foo-bar__baz = two-seg-b
foo__bar__baz = three-seg
