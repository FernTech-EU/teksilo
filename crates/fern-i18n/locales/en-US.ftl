# Fixture file for fern-i18n's own tr! macro tests. Applications have
# their own locales/en-US.ftl; this one only contains keys referenced by
# fern-i18n/tests/tr_macro.rs.

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
