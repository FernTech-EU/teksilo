<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Known accessibility limitations

This file records accessibility gaps that Teksilo **cannot** close from within
its own code because they depend on an external component. Everything
achievable inside the framework is implemented; the items below are the
residual, documented for anyone assembling a conformance statement (EN 301 549
ACR, US Section 508 VPAT, RGAA déclaration de conformité).

## WCAG 1.3.5 Identify Input Purpose — full autofill tokens (AccessKit-blocked)

**Status: partially met. The remainder is blocked upstream in AccessKit; no
Teksilo-side action is possible or planned.**

Teksilo exposes a field's semantic purpose to assistive technology via
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
underlying platform-API limitation, not a Teksilo omission: there is nothing in
the accessibility protocol to write the token into.

Consequence for conformance: an app can satisfy the *role/kind* half of 1.3.5
today; the *autofill-token* half is unavailable on this toolkit until AccessKit
grows such a field. Note the specialised input roles Teksilo does set are
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

Teksilo already ships the primary readability adjustment as a first-class
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

## `described_by` reaches no assistive technology (AccessKit-blocked)

**Status: not met upstream. There is nothing for Teksilo to write.**

AccessKit 0.25 carries a `described_by` relation on the node, but nothing
resolves it: `accesskit_consumer::Node::description()` reads only the node's own
`description` property, and none of the three adapters exports the relation —
macOS maps `AXHelp` from `description()`, Windows maps
`UIA_FullDescriptionPropertyId` from `description`, and the AT-SPI relation set
carries `controls` alone. A node described *only* through the relation therefore
describes itself to nobody.

Teksilo consequently copies the description string onto the described node
(`set_description`) wherever a description must actually be heard — the plain
tooltip tier does this — and keeps the relation for the day upstream resolves
it. Note also that VoiceOver exposes a description as `AXHelp`, a *hint*: it is
read after a delay or on VO-Shift-H, not in the focus utterance, so the ARIA
`aria-describedby` reading behaviour is an NVDA / Orca one on AccessKit 0.25.

## Text-run colour and per-run language (AccessKit-shaped, Teksilo-side work)

A text run carries weight, italic, underline and strikethrough but no
foreground colour and no per-run language, so a syntax-highlighted editor
exposes runs whose colours assistive technology cannot see, and a quoted
sentence in another language is read in the document's voice. AccessKit *has*
the properties (`set_foreground_color`, `set_background_color`, `set_language`);
Teksilo does not yet emit them. Tracked as §5.9 of the internal audit — a
Teksilo omission, not an upstream block.

_Last reviewed: 2026-09-06._
## Two AT actions deliberately not advertised (decisions, not gaps)

An AccessKit client acts on what a node **advertises**: `accesskit_consumer`
filters by `supports_action`, and VoiceOver's rotor and Narrator's scan build
their verb lists from it. Advertising an action the widget does not service is
therefore worse for a user than not advertising it — the verb appears and does
nothing when chosen. Two omissions were raised as possible defects and are
recorded here as decisions.

**`TextInputField` does not advertise `Action::ScrollIntoView`.** Its
`handle_access_action` (`primitives/text_input_field.rs`) answers `Ignored` to
that action, and nothing else would pick it up: `ScrollIntoView` is inert in the
top-level event router and reaches a container only through the
clipping-ancestor walk from `EventContext::ensure_visible`, never through the AT
action path. Revealing a field is the enclosing `ScrollArea`'s job and already
works. What the field does advertise — `Focus`, and, when it can service them,
`SetValue`, `ReplaceSelectedText` and `SetTextSelection` — is pinned by
`the_field_advertises_the_actions_an_assistive_client_may_invoke` and
`a_read_only_or_protected_field_withdraws_the_actions_it_cannot_service` in
`text_input/tests.rs`.

`RichTextEditor` does advertise `ScrollIntoView` and services it, by revealing
its caret (`rich_text.rs`, the `(Action::ScrollIntoView, _)` arm).

**`TextWidget` advertises no actions at all.** It is a leaf label: it has no
`on_access_action` handler, takes no focus, and holds no value a client could
set. Every action AccessKit defines for it would be unserviced. Its
accessibility contribution is its name and its `Role::TextRun` children (see
the text-ranges work), which is what a screen reader reviews it by; a client
that wants it brought on screen goes through the scroll container around it, as
above.

## `CodeEditor` and `LogView` advertise `ScrollIntoView` and do not service it

**Status: open defect, not a limit.** `code_editor/a11y.rs`'s `finish` adds
`Action::ScrollIntoView` to the node for **both** wrappers, but the shared
`handle_access_action` in the same file has no arm for it — it falls through to
`_ => EventResponse::Ignored`. So a screen reader offers the verb on a code
editor and on a log view, and choosing it does nothing. This is the same
mistake as advertising an unserviced action anywhere else, and it is the
opposite of the `TextInputField` decision above.

Fixing the `CodeEditor` half is small: `keyboard::ensure_caret_visible(state)`
already exists and does exactly what `RichTextEditor`'s arm does. The `LogView`
half needs a decision first — a log view follows its tail, so "reveal the
caret" may fight `follow_tail`, and it is not obvious whether an AT reveal
should suspend tail-following the way a user scroll does. Left unfixed rather
than guessed at, because a log that silently stops following its tail is a
worse bug than a verb that does nothing.

_Added 2026-09-08 alongside the touch programme's scrollables review._

_Last reviewed: 2026-07-02._
