# Known accessibility limitations

This file records accessibility gaps that Bastyde **cannot** close from within
its own code because they depend on an external component. Everything
achievable inside the framework is implemented; the items below are the
residual, documented for anyone assembling a conformance statement (EN 301 549
ACR, US Section 508 VPAT, RGAA déclaration de conformité).

## WCAG 1.3.5 Identify Input Purpose — full autofill tokens (AccessKit-blocked)

**Status: partially met. The remainder is blocked upstream in AccessKit; no
Bastyde-side action is possible or planned.**

Bastyde exposes a field's semantic purpose to assistive technology via
`TextInputField::input_purpose(..)` / `TextInput::input_purpose(..)`
(`InputPurpose::{Normal, Email, Phone, Url, Number, Search}`), which sets the
matching specialised AccessKit role (`Role::EmailInput`, `PhoneNumberInput`,
`UrlInput`, `NumberInput`, `SearchInput`). A screen reader therefore announces
the field kind ("email, edit text") rather than a generic "edit text".

What is **not** achievable: the full WCAG 1.3.5 success criterion also expects
the complete HTML `autocomplete`-token vocabulary — `given-name`,
`family-name`, `street-address`, `postal-code`, `cc-number`, `bday`, and the
rest — so the platform / an autofill agent can pre-fill known user data.
**AccessKit 0.24.1 has no node property that carries these tokens** (its
`AutoComplete { Inline, List, Both }` enum is the unrelated `aria-autocomplete`
popup-behaviour hint, not the field-purpose token). This is an AccessKit /
underlying platform-API limitation, not a Bastyde omission: there is nothing in
the accessibility protocol to write the token into.

Consequence for conformance: an app can satisfy the *role/kind* half of 1.3.5
today; the *autofill-token* half is unavailable on this toolkit until AccessKit
grows such a field. Note the specialised input roles Bastyde does set are
2.1-only concepts and are outside US Section 508's WCAG-2.0 scope.

## Selection contrast — default vs high-contrast (design decision)

The default theme's `surface_selected` is a deliberately subtle pale wash and
does not reach the 3:1 non-text contrast of WCAG SC 1.4.11 as a flat fill.
Selection is still conveyed non-visually (AccessKit `selected` state on the
row) and visually (the wash's lightness difference). Rather than force a
higher-contrast selection band or an accent edge bar into the default theme —
which would change the look of every list, tree, table, and menu app-wide, a
design decision the framework owner should make — Bastyde provides a **strong,
clearly-visible selection fill in the opt-in high-contrast theme**
(`ColorTokens::for_high_contrast`), which activates on the OS "increase
contrast" preference and serves the users who need it without altering the
default aesthetic. An app that wants a higher-contrast default selection can
override `surface_selected` (or install a `StandardItemStyle`) itself.

_Last reviewed: 2026-07-02._
