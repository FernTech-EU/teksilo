# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Fixture file for teksilo-i18n's own tr! macro tests. Applications have
# their own locales/en-US.ftl; this one only contains keys referenced by
# teksilo-i18n/tests/tr_macro.rs.

greeting = Hello, World!
welcome = Hello, { $name }!
count-items = You have { $count } items.
farewell = Goodbye!

# Format-integration test fixtures. Used by tests/format_integration.rs.
price-display = The price is { NUMBER($v) }
cart-total = Total: { NUMBER($price, style: "currency", currency: "USD") }
percent-done = { NUMBER($ratio, style: "percent") } complete
last-saved = Last saved on { DATETIME($ts, dateStyle: "long") }
cart-summary = { $count } items at { NUMBER($price) } each

# A plural and a term reference: messages the macro cannot put back together
# from their parts, for the fallback tests in tests/tr_macro.rs.
-app-name = Teksilo
items-selected =
    { $count ->
        [0] No item selected
        [one] 1 item selected
       *[other] { $count } items selected
    }
about-app = About { -app-name }
