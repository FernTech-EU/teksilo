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

## Selection contrast (resolved)

The default theme's `surface_selected` is a subtle pale wash that does not, on
its own, reach WCAG SC 1.4.11's 3:1 non-text contrast as a flat fill. This is
now addressed on two levels: (1) selected `StandardListItem` / `StandardTreeItem`
rows draw a thin `BorderRole::Focused` boundary (accent teal, >= 3:1) even
without keyboard focus — a non-color-alone cue satisfying SC 1.4.1 / 1.4.11; and
(2) the opt-in high-contrast theme (`ColorTokens::for_high_contrast`) also uses
a stronger `surface_selected` fill.

Residual: the `TableView` / `TreeTableView` selection *band* is painted through a
separate `TableStyle` path, not the `StandardItemStyle` boundary above, so it
does not yet carry the same boundary cue — a follow-up for those two widgets.

## WCAG 1.4.12 Text Spacing — Not Applicable (WCAG2ICT)

SC 1.4.12 is a **web** criterion: its premise is that content survives a
*user-injected* text-spacing override (a browser user stylesheet / bookmarklet
that forces line-height ≥ 1.5, letter-spacing ≥ 0.12em, etc.). Native desktop
software exposes **no such injection mechanism**, so — per W3C **WCAG2ICT**,
which EN 301 549 defers to for applying WCAG to non-web software — the criterion
**does not apply** where the software provides no text-spacing-override facility.

Bastyde already ships the primary readability adjustment as a first-class
control — the global **text scale** (`TextScaleControl`, 80–200 %, applied
app-wide through `effective_theme`) — and its reflow/shrink layout prevents
clipping or overlap when text enlarges. An *independent* line/letter-spacing
control (a "reading-comfort" slider, chiefly of benefit to dyslexic readers)
remains a possible **product feature**, not a conformance requirement; if built
later it needs a `text-typeset` shaping hook (letter-spacing per glyph +
line-height multiplier), the tokens threaded through `TypographyTokens` →
`WidgetTree`, and a settings-bound control parallel to `TextScaleControl`.

**Conformance position:** 1.4.12 → **N/A** for the toolkit (no user
spacing-override mechanism); text enlargement is covered by the text-scale
control. Defensible under EN 301 549 / RGAA per WCAG2ICT.

_Last reviewed: 2026-07-02._
