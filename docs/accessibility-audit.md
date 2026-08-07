<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Teksilo Accessibility Conformance Audit — Post-Remediation

**Standards basis:** WCAG 2.1 Level A/AA · EN 301 549 v3.2.1 (§9 WCAG mapping + §11 Software) · WCAG2ICT (non-web applicability guidance)
**Date:** 2026-07-02
**Status:** This document **supersedes** the prior audit, which found "Partial conformance, active blockers" across all three regimes. This revision is based on a line-level, source-verified re-check of every claimed fix, not on commit messages or prior audit text.

---

## 1. Executive Summary

Teksilo shipped a 14-commit accessibility remediation branch (`a11y/wcag-en301549-conformance`, `e1681ddc..bae92c18`) closing **16 of 17 tracked framework-level gaps**, plus a defensible **N/A reclassification of WCAG 1.4.12 Text Spacing** per WCAG2ICT. This audit independently re-verified all 16 fixes against live `.rs` source (file:line), re-ran the regression tests that gate them, and additionally scanned adjacent WCAG 2.1 criteria not covered by the original 16-gap list.

**Honest bottom line:**

- **Framework-level remediation is functionally complete for the 16 tracked gaps.** Every one is verified present, correctly wired end-to-end (not just declared), and covered by a passing test. No claimed fix was found to be a stub, a dead code path, or silently broken.
- **One tracked residual remains open and is honestly self-disclosed by the framework's own docs**, not overclaimed: TableView/TreeTableView selection rows still paint a flat, color-only fill (`recipe_table_style.rs::make_row_background`) — no boundary stroke — unlike ListView/TreeView rows, which now draw a real non-color selection boundary.
- **This audit surfaces a small number of new, narrow findings** the original 16-gap remediation did not target: WCAG 2.2.2 (Pause/Stop/Hide — `Cycle` widget has no pause API), WCAG 2.4.11 (Focus Not Obscured — no framework-level detection), WCAG 2.5.3 (Label in Name — `access_label` can silently diverge from visible text with no debug warning), an EN 301 549 §11.5.2.15 reactivity gap in `access_disabled` (plain `bool`, not `Prop<bool>`), and two stale doc comments (`icon_button.rs:98,582`) contradicting the corrected 24px Compact size.
- **A conformance claim is always an application-level artifact, never a toolkit-level one.** Framework correctness is necessary but not sufficient — an app built on Teksilo still owns: actual alt-text content, information architecture (Multiple Ways), the assembled ACR/VPAT/déclaration, and its own use of the `access_*` override escape hatches without breaking what the framework got right.

**Per-regime posture (see §4 for detail):**

| Regime | Verdict | Change from prior audit |
|---|---|---|
| **EAA / EN 301 549** | Partial — narrow, named residuals only | Upgraded from "active blockers affecting every app" to a short, tracked residual list |
| **Section 508 (US)** | Effectively clean at framework level | Upgraded — WCAG 2.0-only scope excludes most of the still-open 2.1-only items |
| **RGAA 4.1 (France)** | Partial — same criteria set as EAA, but the defect class RGAA's live-AT-testing methodology is best at catching (functionally inert despite correct static markup) is exactly what this remediation closed | Upgraded — same technical gap set as EAA/EN 301 549 |

---

## 2. What Changed Since the Prior Audit

| G-id | Criterion | What was fixed | Verified evidence |
|---|---|---|---|
| G1 | 4.1.2 / AT reactivity | Rebuild now dirties the AT snapshot (`a11y_dirty`) so screen readers see post-rebuild state | `crates/teksilo-core/src/widget_tree/layout_impl.rs:203`; test `rebuild_dirties_accessibility_tree` passes |
| G2 | 1.4.3 Contrast (Minimum) | Correct WCAG relative-luminance contrast formula; default themes enforced ≥4.5:1 text | `crates/teksilo-tokens/src/color.rs:266-277`; `crates/teksilo-tokens/src/theme.rs:784-826` (CI-gated test) |
| G3 | 1.4.11 Non-text Contrast | `border_focused`/`focus_ring` retuned to `#0C8294` (4.53:1) after raw accent measured only 2.47:1 | `crates/teksilo-tokens/src/theme.rs:286-289,342-345` |
| G4 | 4.1.3 Status Messages | `ProgressBar` determinate value now bound `AccessibilityOnly` + unconditional `Live::Polite` | `crates/teksilo-widgets/src/progress_bar.rs:250-254,292-301`; test `accessibility_values` |
| G5 | 3.3.1 Error Identification | Field↔error association via `access_described_by`, targeting the real inner editable node across TextInput/PasswordField/DateTimeEdit/DateRangeEdit | `crates/teksilo-widgets/src/text_input.rs:732`; `password_field.rs:578`; `date_time_edit.rs:726-727`; `date_range_edit.rs:578-579` |
| G6 | 2.5.7 Dragging Movements | Scene items gain an Alt+Arrow (Shift = ×10) keyboard nudge equivalent to pointer group-drag | `crates/teksilo-scene/src/view/gestures_impl.rs:671-701`; test `alt_arrow_nudges_all_selected_items` |
| G7 | EN 301 549 11.5.2.9 | Bold/italic/underline/strikethrough now reach AT `TextRun` nodes via `TextRunAttributes`, sourced from real `text-document` `TextFormat` | `crates/teksilo-core/src/accessibility.rs:156-165,937-988`; `crates/teksilo-widgets/src/rich_text.rs:2302-2358` |
| G8 | 3.3.2 Labels or Instructions | `FormLayout` wires `access_labelled_by(field, label)` per row | `crates/teksilo-widgets/src/primitives/form_layout.rs:210-219`; test `line_wires_field_labelled_by_label` |
| G9 | 1.3.1 / 3.3.2 | `DateEdit` no longer double-labels a redundant middle `GenericContainer` node — collapses to a clean 2-node AT tree | `crates/teksilo-widgets/src/date_edit.rs:716-756`; test `date_edit_collapses_middle_container_node` |
| G10 | 1.3.5 Identify Input Purpose | `InputPurpose` enum → specialised AT roles (Email/Phone/Url/Number/Search), correct precedence under `PasswordInput` | `crates/teksilo-widgets/src/primitives/text_input_field.rs:118-148,1406-1438`; test `input_purpose_sets_specialised_at_role` |
| G12 | 1.4.13 Content on Hover or Focus | Tooltip no longer dismisses on anchor-leave; hoverable via 100ms grace + overlay-bounds check; sticky tooltips keep Escape-dismissibility | `crates/teksilo-core/src/widget_tree/overlay_impl.rs:753-781`; `widget_tree.rs:900-969`; test `tooltip_dismissed_on_pointer_leave` |
| G13 | 1.4.1/1.4.11 + high-contrast | `for_high_contrast()` theme variant (≥7:1 text) + live OS-pref re-query on window focus + non-color selection boundary on ListView/TreeView rows | `crates/teksilo-tokens/src/theme.rs:203-234`; `crates/teksilo-app/src/window_manager.rs:1300-1313`; `crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs:132-154` |
| G14 | 3.2.1 On Focus | Debug-only warning when an `on_focus` handler synchronously calls `open_window`/`focus_window` | `crates/teksilo-core/src/widget_tree/focus_impl.rs:39-46`; `crates/teksilo-core/src/widget/event_context.rs:392-408` |
| G15 | 4.1.2 reactivity | `access_label`/`access_description`/`access_value` now registered at `BindingLevel::AccessibilityOnly` (previously only `access_hidden`) | `crates/teksilo-core/src/widget_tree.rs:1947-1965`; test `bound_access_label_change_dirties_accessibility_tree` |
| G16 | 2.3.3 Animation from Interactions | Overlay/tooltip fades snap instantly under `prefers_reduced_motion` instead of tweening | `crates/teksilo-core/src/widget_tree/overlay_impl.rs` (`attach_overlay_fade` ~950-979, `process_tooltips_impl` ~157-161) |
| G17 | 1.3.2 Meaningful Sequence | `Widget::accessibility_children()` lets AT reading order diverge from paint order; TableView/TreeTableView expose header-before-body to AT while keeping body-before-header for correct z-stacking | `crates/teksilo-core/src/widget.rs:410`; `widget_tree/accessibility_impl.rs:413-419`; `table_view.rs:2104-2135`; test `accessibility_children_overrides_at_reading_order` |
| — | 2.5.8 Target Size (sub-fix bundled with G13) | `IconButtonSize::Compact` raised 22px→24px | `crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs:32-33` |

**1.4.12 Text Spacing → reclassified N/A** per WCAG2ICT: no user-injectable line-height/letter-spacing/paragraph-spacing override mechanism exists anywhere in the toolkit (exhaustive grep confirms this), so the criterion's premise never arises for native desktop software. `TypographyTokens::scaled()` deliberately preserves `line_height`/`letter_spacing` unchanged and multiplies only `size`. The separately-verified `TextScaleControl` + wrap-reflow layout model correctly covers the adjacent, genuinely-applicable "enlarge text without clipping" concern (WCAG 1.4.4) without being conflated with 1.4.12 itself. Documented at `docs/a11y/a11y_issues.md:50-73`.

---

## 3. Conformance Matrix

### 3.1 WCAG 2.1 — Principle 1: Perceivable

| Criterion | Level | Responsibility | Status | Evidence |
|---|---|---|---|---|
| 1.1.1 Non-text Content | A | author | 🟡 partial | Framework supplies the mechanism (`access_label`/`access_description` on any widget); alt-text *content* for app icons/images is author-supplied |
| 1.3.1 Info and Relationships | A | framework | ✅ supported | G5, G8, G9, G7 — `text_input.rs:732`, `form_layout.rs:210-219`, `date_edit.rs:716-756`, `accessibility.rs:156-165` |
| 1.3.2 Meaningful Sequence | A | framework | ✅ supported | G17 — `widget.rs:410`, `accessibility_impl.rs:413-419` |
| 1.3.3 Sensory Characteristics | A | framework-enabled | ✅ supported | No shipped instructional copy relies on shape/position alone; framework affordances carry semantic AT roles + state |
| 1.3.4 Orientation | AA | n/a | ➖ n/a | Desktop, freely-resizable windows; no orientation lock exists |
| 1.3.5 Identify Input Purpose | AA | framework | 🟡 partial | G10 — role half done (`text_input_field.rs:118-148`); autocomplete-token vocabulary blocked upstream (AccessKit 0.24.1 has no such field) |
| 1.4.1 Use of Color | A | framework | 🟡 partial | G13 fixed for List/Tree (`recipe_standard_item_style.rs:132-154`); TableView/TreeTableView selection remains color-only fill (`recipe_table_style.rs:171-222`, grep-confirmed no border) |
| 1.4.3 Contrast (Minimum) | AA | framework | ✅ supported | G2 — `color.rs:266-277`, `theme.rs:784-826` (CI-enforced) |
| 1.4.4 Resize Text | AA | framework | ✅ supported | `text_scale_control.rs:63-260`; `widget_tree.rs:1482-1495,1520-1528`; wrap-reflow tests pass |
| 1.4.5 Images of Text | AA | framework | ✅ supported | All chrome renders via the GPU glyph-atlas pipeline; no stock widget bakes label text into a raster |
| 1.4.10 Reflow | AA | n/a | ➖ n/a | Web-viewport-specific criterion; desktop constraint-negotiation layout is the analogue, not literally evaluated |
| 1.4.11 Non-text Contrast | AA | framework | 🟡 partial | G3 fixed for focus rings (`theme.rs:286-289`, 4.53:1); same TableView/TreeTableView selection-boundary absence as 1.4.1 |
| 1.4.12 Text Spacing | AA | n/a | ➖ n/a | Reclassified N/A per WCAG2ICT — no spacing-injection mechanism exists; see §2 |
| 1.4.13 Content on Hover or Focus | AA | framework | ✅ supported | G12 — `overlay_impl.rs:753-781,900-969`; test `tooltip_dismissed_on_pointer_leave` |

### 3.2 WCAG 2.1 — Principle 2: Operable

| Criterion | Level | Responsibility | Status | Evidence |
|---|---|---|---|---|
| 2.1.1 Keyboard | A | framework-enabled | ✅ supported | Real DFS Tab order + roving tabindex; G6 closes the last named keyboard-only gap (scene item drag) |
| 2.1.2 No Keyboard Trap | A | framework | ✅ supported | Modal scope always paired with `EscapeOrClickOutside` default |
| 2.1.4 Character Key Shortcuts | A | framework | ✅ supported | Type-ahead / mnemonics only active while the owning component holds focus |
| 2.2.1 Timing Adjustable | A | n/a | ➖ n/a | No session timeouts or time-limited interactions exist |
| 2.2.2 Pause, Stop, Hide | A | framework | 🟡 partial | **New finding**: `Cycle` (rotating-content widget, 3s default) honours `prefers_reduced_motion` but has no `.paused(Signal<bool>)` or built-in pause affordance — `crates/teksilo-widgets/src/animations/cycle.rs:42-160`, no `pause` API found |
| 2.3.1 Three Flashes or Below Threshold | A | framework | ✅ supported | Only `Shake` (spatial) and `Pulse` (~1.1Hz opacity) oscillate; neither is a flash hazard |
| 2.4.1 Bypass Blocks | A | n/a | ➖ n/a | No web-style repeated-block navigation; roving tabindex already collapses composite groups to one Tab stop |
| 2.4.2 Page Titled | A | n/a | ➖ n/a | `WindowConfig::title` is a required, always-supplied parameter |
| 2.4.3 Focus Order | A | framework | ✅ supported | Tab order (`node.children`, paint order) unaffected by G17's AT-only override; the two are correctly decoupled |
| 2.4.4 Link Purpose (In Context) | A | author | ✅ supported | `Link` requires a text label at construction; no icon-only variant |
| 2.4.5 Multiple Ways | AA | author | ➖ n/a | App-level information-architecture decision |
| 2.4.6 Headings and Labels | AA | framework-enabled | ✅ supported | G8/G9 reinforce correct, non-redundant labelling; enforcement that every control *has* a label stays app-author responsibility |
| 2.4.7 Focus Visible | AA | framework | ✅ supported | Real focus-visible heuristic; G3 strengthens the shared `focus_ring_width=2.0` token consumed by 15+ widget styles |
| 2.4.11 Focus Not Obscured (Minimum) | AA | framework | 🟡 partial | **New finding**: no `WidgetTree`-level mechanism detects/corrects a focused element hidden behind sticky/floating content (`ensure_visible` exists only per-widget for scroll-into-view, e.g. GridView/Scene) — not assessed in either audit pass until now |
| 2.4.13 Focus Appearance | AA | framework | ✅ supported | 2.0dp stroke + ≥3:1 contrast (G3) both independently verified |
| 2.5.1 Pointer Gestures | A | framework | ✅ supported | All multi-point/path gestures (Scene pan/zoom, marquee) have single-pointer keyboard equivalents; G6 closes the last one |
| 2.5.2 Pointer Cancellation | A | framework | ✅ supported | Recognizer-based activation (tap-on-release), not raw pointer-down |
| 2.5.3 Label in Name | A | framework-enabled | 🟡 partial | **New finding**: `access_label` can fully replace a widget's visible text with no automated check the accessible name still contains it — no debug warning exists (unlike the precedent set by G14) |
| 2.5.4 Motion Actuation | A | n/a | ➖ n/a | No device-motion/tilt/camera-gesture input surface exists |
| 2.5.7 Dragging Movements | AA | framework | ✅ supported | G6 — `gestures_impl.rs:671-701`; test `alt_arrow_nudges_all_selected_items` |
| 2.5.8 Target Size (Minimum) | AA | framework | 🟡 partial | Compact IconButton raised to 24px, but two doc comments (`icon_button.rs:98,582`) remain stale, and no regression test numerically gates the constant |

### 3.3 WCAG 2.1 — Principle 3: Understandable / Principle 4: Robust

| Criterion | Level | Responsibility | Status | Evidence |
|---|---|---|---|---|
| 3.2.1 On Focus | A | framework (debug-only aid) | ✅ supported | G14 — debug-only warning for `open_window`/`focus_window` inside `on_focus`; correctly self-scoped, does not cover other context-change side effects |
| 3.2.2 On Input | A | author | ✅ supported | No stock widget triggers a context change purely on value-change without explicit activation |
| 3.3.1 Error Identification | A | framework-enabled | ✅ supported | G5 — described_by wiring across all validated stock inputs |
| 3.3.2 Labels or Instructions | A | framework-enabled | ✅ supported | G8, G9 |
| 4.1.2 Name, Role, Value | A | framework | ✅ supported | G1, G15, G17; AT diff/notify delegated correctly to AccessKit's own `Tree::update_and_process_changes` |
| 4.1.3 Status Messages | AA | framework | ✅ supported | G4 (ProgressBar); StatusBar's opt-in `announce_changes` is a deliberate, defensible design choice, not a gap |

### 3.4 EN 301 549 v3.2.1 — Chapter 11 (Software)

| Clause | Topic | Status | Evidence |
|---|---|---|---|
| 11.5.2.9 | Text attributes exposed to AT | 🟡 partial | Bold/italic/underline/strikethrough fixed (G7); **new finding**: text color, highlight color, and per-run language are available in `text-document`'s `TextFormat`/`Highlight` types but never forwarded to AccessKit's `set_foreground_color`/`set_background_color`/`set_language` — `accessibility.rs` has zero occurrences of these |
| 11.5.2.14 / .16 / .17 | Content/state/value change notifications | ✅ supported | Correctly delegated to `accesskit_winit::Adapter::update_if_active`'s own diff machinery; G1 + G15 ensure a genuinely different `TreeUpdate` is produced when it should be |
| 11.5.2.15 | State exposed to AT | 🟡 partial | Core reactive state (selected/expanded/invalid/toggled) correct; **new finding**: `WidgetBuilder::access_disabled` takes a plain `bool` (not `impl Into<Prop<bool>>`), unlike its `access_hidden`/`access_label` siblings — an app using this specific override gets a value frozen at build time. Narrow blast radius: the native `disabled: Prop<bool>` path stays fully reactive |
| 11.5.2.x | AT platform-API binding | ✅ supported | Real `accesskit_winit` bridges on all 3 desktop OSes (AT-SPI/Linux, NSAccessibility/macOS, UIA/Windows); unchanged by this branch, independently re-confirmed |
| 11.6.2 | Accessible end-user documentation | ➖ n/a | Applies to shipped end-user products, not the toolkit itself |
| 11.7(a) Contrast preference | User high-contrast pref honoured live | ✅ supported | G13; layered correctly at both main and overlay paint passes |
| 11.7(b) Reduced motion preference | ✅ supported | G16; consistent with the single `AccessibilityPreferences` plumbing |
| 11.7(c) Text scaling preference | ✅ supported | Applied at launch and on every focus-triggered refresh; `recompute_effective_theme()` dirty-checked |
| 11.7 (granularity) | Live re-query mechanism | 🟡 partial | Focus-transition-triggered, not a true OS push-subscription — a documented, deliberate zero-idle-cost design choice, not a regression |

---

## 4. Regime-Specific Analysis

### 4.1 European Accessibility Act (EAA) → EN 301 549 v3.2.1

- **Verdict:** Partial — narrow, named residuals only (upgraded from "active blockers affecting every app built on it").
- **In-scope set:** EN 301 549 §9 (= WCAG 2.1 A/AA in full) + §11 (software: AT interoperability, user preferences, authoring tools).
- **Required artifact:** An **Accessibility Conformance Report (ACR)** in the EN 301 549 ITI template, prepared by whoever ships the actual product built on Teksilo — never a toolkit-level document.
- **Residuals unique to this regime:** Because EAA pulls in the *full* WCAG 2.1 set (the widest of the three regimes), it retains the most residual exposure despite receiving the most fixes: TableView/TreeTableView selection-boundary gap (1.4.1/1.4.11), the autocomplete-token half of 1.3.5 (upstream-blocked), and the newly-surfaced 2.2.2/2.4.11/2.5.3 items in §5.

### 4.2 US Section 508 (Revised)

- **Verdict:** Effectively clean at the framework level.
- **In-scope set:** WCAG **2.0** A/AA by reference (not 2.1), plus Chapter 3 Functional Performance Criteria (302.x).
- **Required artifact:** A **VPAT 2.5 Rev (WCAG edition)**, scoped to 2.0 A/AA + the 302.x table — again an application-level document.
- **Residuals unique to this regime:** Most of the still-open items (1.4.10 Reflow, 1.4.11 as a *named* SC, 1.4.12, 2.5.7, 2.5.8, 1.3.5) are 2.1-only and sit outside 508's literal WCAG-2.0 scope entirely — yet Teksilo fixed several of them anyway, so they are moot as 508 findings regardless of scope. The two SC that *do* bind under 2.0 AA — 1.4.3 and 4.1.2 — are both closed and CI-hardened. Chapter 3 FPC concerns (302.7 limited manipulation, 302.8 limited reach) are covered by G6 and the IconButton fix respectively.

### 4.3 RGAA 4.1 (France)

- **Verdict:** Partial — same criteria scope as EAA/EN 301 549 (RGAA 4.1 bridges non-web/native software through EN 301 549's WCAG 2.1 AA restatement).
- **In-scope set:** Identical criteria set to §4.1, but audited via RGAA's **106 test criteria**, methodologically distinct: conducted manually with real screen readers.
- **Required artifact:** A full **RGAA 4.1 déclaration de conformité** (schéma pluriannuel + plan annuel + a published accessibility statement page) — code-level conformance is necessary but not sufficient here; a live manual audit is mandatory regardless of this report.
- **Residuals unique to this regime:** Same technical gap set as EAA. Notably, RGAA's live-AT-testing methodology is precisely the modality best suited to catch the class of defect this remediation prioritized closing — "functionally inert despite correct static markup" (G1's stale AT tree, G4's ProgressBar value never reaching AT despite correct role tagging). Both are independently confirmed fixed, which lowers RGAA-specific practical risk more than the bare criteria-gap count would suggest.

---

## 5. Residuals & Recommendations

Ranked by impact; each item tagged by who owns the follow-up.

1. **[Framework, highest impact] TableView/TreeTableView selection band remains color-only.** `recipe_table_style.rs::make_row_background` (lines 171-222) paints selection exclusively via `.background(ColorProp::DynamicSurfaceRole(role))` — grep-confirmed zero `border_color`/`border_width` calls anywhere in the TableView/TreeTableView module tree. This is the direct counterpart of the fix already applied to `StandardListItem`/`StandardTreeItem` (`recipe_standard_item_style.rs:132-154`) and is honestly disclosed as an open follow-up in `docs/a11y/a11y_issues.md` — not misrepresented as fixed anywhere. **Recommendation:** port the same `border_color(BorderRole::Focused).border_width(selection_edge_width)` pattern to `make_row_background`.

2. **[Upstream/external, cannot fix in Teksilo] WCAG 1.3.5 full autofill-token vocabulary is blocked by AccessKit 0.24.1.** The crate has no field carrying HTML autocomplete tokens (`given-name`, `street-address`, `cc-number`, etc.) — only role identification is achievable today, and that half is done (G10). Honestly documented in `docs/a11y/a11y_issues.md:21-34`. **Recommendation:** track upstream AccessKit issue/feature request; no framework-side workaround exists.

3. **[Framework, medium impact] EN 301 549 11.5.2.9 — text color/highlight/language attributes not forwarded to AT.** `text-document`'s `TextFormat`/`Highlight` types already carry `foreground_color`/`background_color` data; AccessKit's `Node` supports `set_foreground_color`/`set_background_color`/`set_language` as real properties — neither side is missing, only the plumbing between them (`accessibility.rs` has zero occurrences of these setters). **Recommendation:** extend `TextRunAttributes` with color/language fields, following the exact pattern G7 already established for bold/italic/underline/strikethrough.

4. **[Framework, medium impact, new finding] WCAG 2.2.2 — `Cycle` widget has no pause affordance.** Auto-rotating content (default 3s period) presented in parallel with other content, honouring only OS-level `prefers_reduced_motion`, with zero `.paused(Signal<bool>)` API and zero production call sites layering their own pause UI. This was not caught by the original 16-gap remediation or its own audit trail. **Recommendation:** add a `.paused(impl Into<Prop<bool>>)` builder method, mirroring the pattern of `Pulse`/other animated wrappers.

5. **[Framework, medium impact, new finding] WCAG 2.4.11 — Focus Not Obscured has no framework-level mechanism.** `ensure_visible` exists only per-widget (GridView keyboard nav, Scene camera pan) for scrolling a focused item *into* view — there is no `WidgetTree`-level detection of a focused element being *covered* by sticky/floating content (a docked side panel, a sticky header, a non-modal Popover). Pre-existing architectural gap, unchanged by this branch, and absent from both audit passes until now. **Recommendation:** scope for a future pass; likely needs a z-order/bounds intersection check against the currently-focused node's screen rect.

6. **[Framework, lower impact, new finding] WCAG 2.5.3 — `access_label` can silently diverge from the visible label.** No automated check (not even a debug-mode warning, despite G14 establishing exactly that pattern for a different criterion) catches `.access_label(...)` fully replacing a text-bearing control's visible text — a real risk for voice-control users. **Recommendation:** add a debug-only warning analogous to G14's `warn_if_context_change_in_focus_dispatch` when `access_label` doesn't contain the widget's own rendered text.

7. **[Framework, low impact] EN 301 549 11.5.2.15 — `access_disabled` override is non-reactive.** `WidgetBuilder::access_disabled(bool)` and its `WidgetWithHandlers` twin take a plain `bool`, not `impl Into<Prop<bool>>`, unlike every sibling `access_*` override. Narrow blast radius — most apps drive disabled state via the widget's own native `disabled: Prop<bool>`, which stays fully reactive. **Recommendation:** widen the signature to `impl Into<Prop<bool>>` and register at `BindingLevel::AccessibilityOnly`, consistent with `register_access_prop_bindings`.

8. **[Framework, cosmetic] Stale doc comments contradict the corrected Compact icon-button size.** `icon_button.rs:98` and `:582` still say "Compact 22 dp" after the constant was raised to 24px (`recipe_icon_button_style.rs:32-33`) — no regression test gates the numeric floor either, so a future refactor could silently regress it. **Recommendation:** fix the two comments and add a `ICON_BUTTON_SIZE_COMPACT >= 24.0` assertion test.

9. **[Author responsibility, always] 1.1.1 alt-text content, 2.4.5 Multiple Ways, and all conformance-artifact assembly.** The framework provides the mechanism; content and information architecture are inherently per-application decisions the toolkit cannot make on an app author's behalf.

10. **[External/platform, expected and stable] AT-bridge coverage varies only by desktop-OS backend maturity, not by Teksilo's own code.** `accesskit_unix` (AT-SPI/D-Bus, identical across X11 and Wayland), `accesskit_macos` (NSAccessibility), `accesskit_windows` (UIA) are all real, non-stubbed bindings, unchanged by this remediation branch and independently re-confirmed. This is a platform-survey caveat, not a framework defect — it stays as-is regardless of any Teksilo-side work.

11. **[Documented, defensible] WCAG 1.4.12 Text Spacing — N/A.** No user-injectable spacing-override mechanism exists in native desktop software; the criterion's premise never arises. See §2 for the full rationale. This is a correct application of WCAG2ICT, not a gap.

---

## 6. Strengths

- **Contrast is now a CI-enforced invariant, not a one-time fix.** `default_themes_meet_wcag_contrast_minimums` (`theme.rs:784-826`) fails the build if either shipped preset regresses below 4.5:1 text / 3.0:1 focus-ring contrast — this class of defect cannot silently reappear.
- **The AT tree is genuinely reactive end-to-end.** G1 (rebuild dirties AT) and G15 (bound `access_label`/`description`/`value` reactivity) close exactly the defect class — "attribute present in code but functionally inert to a screen reader" — that a live AT audit (RGAA-style) is best positioned to catch, and both are independently verified correct, not just declared.
- **AT reading order is now decoupled from paint order by design**, not by accident: `Widget::accessibility_children()` is a narrowly-scoped override consulted at exactly one point in the walker, verified to never leak into Tab-order or geometry/paint code.
- **The framework correctly delegates change-notification plumbing to AccessKit's own diff machinery** (`Adapter::update_if_active`) rather than hand-rolling notifications — the architecturally sound choice, and it works because G1/G15 ensure the tree it diffs against is actually current.
- **High-contrast and reduced-motion are unified under one preference-query pipeline** (`AccessibilityPreferences`), applied consistently across contrast (11.7a), motion (11.7b), and text scale (11.7c) — a single source of truth rather than three independent mechanisms.
- **The framework is honest with itself.** `docs/a11y/a11y_issues.md` accurately discloses every remaining residual (TableView/TreeTableView selection, the AccessKit-blocked autofill-token gap, the 1.4.12 N/A rationale) — nothing found in this audit was misrepresented as fixed when it wasn't, which is itself a meaningful signal about the reliability of future claims from this codebase.
- **Global text scale (80%–200%) is wired app-wide with zero per-widget boilerplate**, combining a user factor with the live OS preference into one `effective_theme`, applied at relayout (not rebuild) so focus and scroll state survive.

---

*Prior audit (pre-remediation, "active blockers" posture) is preserved in git history at commit `bae92c18` and earlier.*
