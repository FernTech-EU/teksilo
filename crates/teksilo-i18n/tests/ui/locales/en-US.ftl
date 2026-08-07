# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Shared fixture for trybuild UI test cases. Every test case points at
# this file via the `TEKSILO_I18N_SOURCE_PATH` env var that the test
# runner exports before invoking trybuild.
#
# - Pass cases reference keys that exist with the expected signature.
# - Fail cases reference keys that don't exist, or use mismatched
#   arguments against the definitions here.

greeting = Hello
welcome = Hello, { $name }!
count-items = { $count } items
