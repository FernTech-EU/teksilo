# English (source language). The `tr!` proc macro validates every
# invocation in this crate against this file at compile time.

window-title = Internationalization Demo
-brand = FernUI
heading = { -brand } i18n Showcase
greeting = Hello, { $name }!
body-paragraph = Pick a language from the list below. Switching to Arabic flips the layout direction — leading and trailing swap, so the row at the bottom visibly reverses its children. English and French are both left-to-right, so the row stays in the same order between them.
direction-note-ltr = Layout direction: Left to Right
direction-note-rtl = Layout direction: Right to Left
language-label = Language:
lang-english = English
lang-french = Français
lang-arabic = العربية
leading-button = Leading
trailing-button = Trailing
status-en = Showing English
status-fr = Showing French
status-ar = Showing Arabic

# Locale-aware formatting showcase. Demonstrates the four pieces wired
# by `fern-i18n`: bundle-side `NUMBER()` / `DATETIME()` (this file's
# `bundle-currency-row` and `bundle-date-row`), the Signal-side
# `NumberFormatter` / `FernDateTimeFormatter` (no .ftl keys needed),
# and `tr_signal!` (this file's `cart-summary`).
formatting-heading = Locale-aware formatting
bundle-currency-row = Total (bundle): { NUMBER($price, style: "currency", currency: "USD") }
bundle-date-row = Today (bundle): { DATETIME($ts, dateStyle: "long") }
cart-summary = { $count } items at { NUMBER($price) } each
price-label = Price:
count-label = Count:
