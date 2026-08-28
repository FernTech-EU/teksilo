<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Teksilo Accessibility — Internal Engineering Assessment

> **This is not an official document.** It is an internal, machine-drafted working
> assessment of Teksilo's accessibility posture, produced by reading source and
> writing down what was found. It is **not** an Accessibility Conformance Report,
> **not** a VPAT, **not** an RGAA déclaration de conformité, and **not** a
> conformance claim of any kind. Nothing here has been verified by a third party,
> by an accessibility auditor, or by a live assistive-technology test session.
>
> Treat every verdict below as an **engineering hypothesis with a source citation
> attached** — useful for deciding what to fix next, not for telling anyone what
> Teksilo conforms to. Where a verdict is uncertain, it says so. Where a previous
> revision of this file was confidently wrong, §7 says that too.
>
> This page is deliberately **not** listed in [`docs/SUMMARY.md`](SUMMARY.md), so it
> is not published as a chapter of the documentation site. That is intentional.

**Assessed against:** commit `7b15f57d`, 2026-08-28
**Previous revision:** 2026-07-02 (commit `771fa3e0`), superseded — see §7
**Standards referenced:** WCAG 2.1 A/AA · WCAG 2.2 delta · EN 301 549 v3.2.1 (§5, §9, §11) · WCAG2ICT

---

## 1. Executive summary

The 2026-07-02 revision of this file claimed a 16-of-17-gap remediation and scored
most of WCAG 2.1 as met. Re-checking it against source at `7b15f57d` — 311 commits
later — produces three findings that matter more than any individual criterion:

1. **The 2026-07-02 remediation itself held up.** All sixteen tracked fixes are
   still present, none regressed, and all ten regression tests it names still exist
   under their original names and pass. What rotted is the *citations*: roughly half
   the `file:line` references in the previous revision now point at unrelated code,
   and one cited a symbol (`Tree::update_and_process_changes`) that has never existed
   in this workspace.

2. **Several of its ✅ verdicts were wrong when written, and were disproved by
   later bug fixes rather than by this review.** Between July and August, commits
   fixed: six stock widgets advertising `Action::Click` to assistive technology with
   no handler behind it (`893768f7`), the *entire* menu system being AT-inoperable
   (`d7485ad6`), list and tree rows exposing no name and no actions (`8d2ef1aa`),
   context menus never being advertised (`696ab3d8`), rich-text reporting a frozen
   caret to AT (`2f305370`), and plain tooltips having no screen-reader path at all
   (`55eb6de4`). The previous revision scored 4.1.2 and 2.1.1 as ✅ *supported* while
   all of that was true. The reason is structural and is discussed in §7: it checked
   whether semantics were **declared**, never whether the declared action **executed**.

3. **The widget catalog has grown faster than this assessment tracked it.** Terminal,
   WebView, CommandPalette, CodeEditor/LogView, GridView, DockingLayout, Splitter,
   SegmentedControl, ColumnFlow, Toast, charts, and three new theme presets all
   post-date the previous revision and appear nowhere in it. Three of them carry
   live Level-A defects.

**Newly identified, ranked by severity:**

| # | Finding | Criterion | Severity |
|---|---|---|---|
| 1 | `Terminal` is an unconditional keyboard trap — Tab, Shift+Tab and Escape are encoded to the PTY and every `KeyDown` returns `Handled`, with no escape chord | 2.1.2 (A) | **Fail** (opt-in feature) |
| 2 | Chart series are distinguished by colour alone — no dash, marker, or fill-pattern channel exists | 1.4.1 (A) | **Fail** |
| 3 | `CommandPalette` exposes an unnamed `Role::Dialog`; its arrow-key highlight is invisible to AT and is colour-only on screen | 4.1.2 (A), 1.4.1 (A) | **Fail** |
| 4 | `WebView` is never `.focusable()` — embedded page content is outside the Tab cycle entirely | 2.1.1 (A) | **Fail** |
| 5 | `Toast` auto-dismisses actionable controls after 10 s, pausable by pointer hover only — never by keyboard focus | 2.2.1 (A), 2.2.2 (A) | Partial |
| 6 | No keyboard route to any context menu exists (no `Key::ContextMenu`, no Shift+F10) | 2.1.1 (A) | Partial |
| 7 | `teksilo-theme-material3` ships with zero contrast assertions | 1.4.3, 1.4.11 (AA) | Untested |
| 8 | `for_high_contrast()` substitutes IntUI teal into whichever preset is active | EN 11.7(a) | Partial |
| 9 | No per-run language: `set_language` is emitted once, on the root node | 3.1.2 (AA) | **Fail** |

**Bottom line.** Teksilo's accessibility *architecture* is strong and, in several
places, better than most desktop toolkits — the AccessKit bridge is real on all
three platforms, roles and names are declared at the trait level, keyboard
alternatives exist for the primary drag interactions, and three of four theme
presets self-gate their contrast in CI. Its accessibility *coverage* is uneven, and
the unevenness tracks widget age: everything audited in July is in good shape, and
everything shipped since July is unassessed until now. A conformance claim remains an
application-level artifact regardless; nothing in this file supports one.

### 1.1 Scope and method — read this before quoting anything below

**What was examined:** the `crates/` tree at `7b15f57d`, by reading source. Every
`file:line` in this document was opened and checked at the time of writing.

**What was *not* done, and therefore what this document cannot tell you:**

- **No live assistive-technology testing.** Nothing here was tried with VoiceOver,
  NVDA, JAWS, Orca, or Narrator. Finding #2 in §1 exists precisely because the
  previous revision made the same omission and scored a broken AT-activation path
  as supported. Static reading cannot detect an advertised action that does nothing.
- **No compilation or execution of most claims.** A subset of named regression tests
  were run; the rest of the assessment is a source read.
- **No colorimetric verification on real displays.** Contrast ratios are computed
  from token values, not measured from rendered output, and translucent tokens
  compare imprecisely except where a preset's own test uses an alpha-compositing
  helper (Fluent does; the IntUI gate does not).
- **No coverage of application-level obligations** — alt-text content, information
  architecture, consistent navigation, or the assembly of any conformance artifact.
- **Not exhaustive over the catalog.** `FontPicker`, `RadioTileGroup`, `InputDialog`,
  `FocusScope`, the `teksilo-automation` AT-driving tools, and the Tier-3 style
  implementations in the Fluent and macOS presets were not assessed.
- **EN 301 549 clause numbers below §11.5.2 are cited by topic, not by number,**
  where the standard text was not to hand. Anyone building an ACR should pull the
  clause list from the standard rather than from this file or from code comments.

---

## 2. Status of the 2026-07-02 remediation (G1–G17)

All sixteen are still in place. Citations below are re-pinned to `7b15f57d`.
The rightmost column records what the previous revision got wrong about each.

| G-id | Criterion | Current location | Correction |
|---|---|---|---|
| G1 | 4.1.2 AT reactivity — rebuild dirties the AT snapshot | `a11y_dirty = true` at [layout_impl.rs:256](../crates/teksilo-core/src/widget_tree/layout_impl.rs), in `process_pending_rebuilds`. Test `rebuild_dirties_accessibility_tree` at [event_dispatch_impl.rs:2237](../crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs) | Was cited at `layout_impl.rs:203` (now unrelated code). Two sibling dirty-sites now exist — `:61` (AccessibilityOnly bindings) and `:100` (dormancy transitions) — and a plain relayout deliberately no longer re-walks AT |
| G2 | 1.4.3 Contrast — WCAG luminance formula, CI-gated themes | `relative_luminance` [color.rs:261-270](../crates/teksilo-tokens/src/color.rs), `contrast_ratio` `:278-283`. Test `default_themes_meet_wcag_contrast_minimums` at [theme.rs:844-887](../crates/teksilo-tokens/src/theme.rs) | Was cited at `color.rs:266-277` / `theme.rs:784-826`. **Scope was overstated:** the gate iterates only `light_default()`/`dark_default()` — the IntUI preset — and never asserts `text_primary`. See §5.4 |
| G3 | 1.4.11 Non-text contrast — focus indicators retuned | IntUI light `border_focused`/`focus_ring` = `#0C8294` at [theme.rs:319, :378](../crates/teksilo-tokens/src/theme.rs) (4.53:1 on `surface_content`, 4.26:1 on `surface_main`); IntUI dark = `#19BDD4` at `:456, :520` (7.28:1 / 6.10:1) | Was cited at `theme.rs:286-289,342-345`. The claim is IntUI-specific and does not generalise: Fluent binds focus to `focus_stroke_outer` (deliberately never the accent), macOS to the raw accent, Material 3 to `m.primary` |
| G4 | 4.1.3 Status messages — ProgressBar value + `Live::Polite` | [progress_bar.rs:250-254, :292-301](../crates/teksilo-widgets/src/progress_bar.rs); test `accessibility_values` at `:480` | **Accurate as written** — the only G-row whose citation survived intact |
| G5 | 3.3.1 Error identification — field↔error `access_described_by` | [text_input.rs:738](../crates/teksilo-widgets/src/text_input.rs), `password_field.rs`, `date_time_edit.rs`, `date_range_edit.rs` | Line drift on three of four; behaviour intact on all four |
| G6 | 2.5.7 Dragging movements — Alt+Arrow scene nudge | [gestures_impl.rs](../crates/teksilo-scene/src/view/gestures_impl.rs); test `alt_arrow_nudges_all_selected_items` at [view/tests.rs:1020](../crates/teksilo-scene/src/view/tests.rs) | Line drift only. Now joined by two further keyboard drag-alternatives — Splitter and scene magnetism (§3.2) |
| G7 | EN 11.5.2.9 — text run attributes reach AT | `TextRunAttributes` at [accessibility.rs:178-187](../crates/teksilo-core/src/accessibility.rs); rich-text walk in [rich_text.rs](../crates/teksilo-widgets/src/rich_text.rs) | Line drift (~+1900 lines in `rich_text.rs`). The struct gained `font_weight` since; still **no colour and no language** — see §5.9 |
| G8 | 3.3.2 Labels — `FormLayout` wires `access_labelled_by` | [form_layout.rs:210-219](../crates/teksilo-widgets/src/primitives/form_layout.rs); test `line_wires_field_labelled_by_label` | **Accurate as written** |
| G9 | 1.3.1 / 3.3.2 — DateEdit collapses its redundant middle node | [date_edit.rs](../crates/teksilo-widgets/src/date_edit.rs); test `date_edit_collapses_middle_container_node` | Line drift only |
| G10 | 1.3.5 Input purpose → specialised AT roles | `InputPurpose` at [text_input_field.rs:120-149](../crates/teksilo-widgets/src/primitives/text_input_field.rs) | Line drift, **and the stated blocker is wrong** — see §5.8 |
| G12 | 1.4.13 Content on hover or focus | No-dismiss-on-anchor-leave at [overlay_impl.rs:1615-1645](../crates/teksilo-core/src/widget_tree/overlay_impl.rs); 100 ms grace at `:486-494`; bounds check at [widget_tree.rs:1029-1055](../crates/teksilo-core/src/widget_tree.rs) | **The verdict was wrong when written.** G12 closed *hoverable*; *dismissible* and *persistent* were both still broken and were fixed a month later by `55eb6de4` — see §7 |
| G13 | 1.4.1 / 1.4.11 + high contrast | `for_high_contrast()` at [theme.rs:230-259](../crates/teksilo-tokens/src/theme.rs); OS re-query in [window_manager.rs](../crates/teksilo-app/src/window_manager.rs); list/tree selection boundary at [recipe_standard_item_style.rs:133-166](../crates/teksilo-widgets/src/styles/recipe_standard_item_style.rs) | Line drift. **New problem:** `for_high_contrast()` hardcodes IntUI hexes and is applied to whatever preset is active — see §5.6 |
| G14 | 3.2.1 On focus — debug warning for context change | [focus_impl.rs](../crates/teksilo-core/src/widget_tree/focus_impl.rs), [event_context.rs](../crates/teksilo-core/src/widget/event_context.rs) | Line drift only |
| G15 | 4.1.2 — bound `access_*` props at `AccessibilityOnly` | `register_access_prop_bindings` at [widget_tree.rs:2298-2314](../crates/teksilo-core/src/widget_tree.rs); test `bound_access_label_change_dirties_accessibility_tree` at [event_dispatch_impl.rs:2264](../crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs) | Line drift. G15 covered *override* props only — the editors' own caret signals had the identical defect and were fixed later (`2f305370`) |
| G16 | 2.3.3 — overlay fades snap under reduced motion | [overlay_impl.rs](../crates/teksilo-core/src/widget_tree/overlay_impl.rs) (`attach_overlay_fade`, `process_tooltips_impl`) | Previous revision cited a file `process_tooltips_impl.rs` that does not exist — it is a function inside `overlay_impl.rs` |
| G17 | 1.3.2 Meaningful sequence — `accessibility_children()` | Trait method at [widget.rs:439](../crates/teksilo-core/src/widget.rs), honoured at [accessibility_impl.rs:499](../crates/teksilo-core/src/widget_tree/accessibility_impl.rs), overridden at [table_view.rs:2713](../crates/teksilo-widgets/src/table_view.rs) and [tree_table_view.rs:2673](../crates/teksilo-widgets/src/tree_table_view.rs) | Line drift only. The "exactly one walker call site" claim re-verified and holds |
| — | 2.5.8 Target size — Compact icon button 22→24 dp | `ICON_BUTTON_SIZE_COMPACT = 24.0` at [recipe_icon_button_style.rs:32](../crates/teksilo-widgets/src/styles/recipe_icon_button_style.rs) | **The macOS preset overrides it to 18 dp** ([metrics.rs:170-172](../crates/teksilo-theme-macos/src/styles/metrics.rs)), reopening 2.5.8 for macOS-themed apps |

There is no G11 row and never was one in this revision: G11 was the label for the
1.4.12 Text Spacing N/A reclassification, which the 2026-07-02 rewrite carried as
unnumbered prose. That reclassification now needs qualifying — see §3.1.

---

## 3. Conformance matrices

Legend: ✅ supported · 🟡 partial · ❌ not supported · ➖ not applicable · ❔ unassessed.
"Responsibility" distinguishes what the framework owns from what an application author owns.

### 3.1 WCAG 2.1 — Perceivable

| Criterion | Lvl | Resp. | Status | Evidence / note |
|---|---|---|---|---|
| 1.1.1 Non-text Content | A | author | 🟡 | Mechanism supplied (`access_label`/`access_description`, `tile_a11y_label`). Charts are the exception where the framework supplies the alternative itself — per-datum name+value nodes at [hit.rs:237-252](../crates/teksilo-charts/src/hit.rs). Rich-text inline images dropped their `alt` entirely until `ea290adb` fixed it — a mechanism defect the previous revision's "content is author-supplied" framing concealed |
| 1.2.1–1.2.5 Time-based media | A/AA | — | ➖ | No audio subsystem and no media/player widget exist in the workspace. **Caveat:** an app embedding `WebView` inherits these as live obligations for the page it hosts |
| 1.3.1 Info and Relationships | A | framework | ✅ | G5, G7, G8, G9. Substantially strengthened since: rows fill a nameless `TreeItem`/`ListBoxOption` from their first named descendant ([accessibility_impl.rs:268](../crates/teksilo-core/src/widget_tree/accessibility_impl.rs)); `StandardItem::label_slot` decouples visual rendering from accessible name; annotations expose an ARIA-`details` relation ([accessibility.rs:323, :331](../crates/teksilo-core/src/accessibility.rs)); docking sides emit `Role::Complementary` landmarks |
| 1.3.2 Meaningful Sequence | A | framework | ✅ | G17. Second independent proof point: `ColumnFlow` re-partitions across a width-dependent column count but keeps children as contiguous source-order runs, so visual order == AT order == focus order at every count ([column_flow.rs:47-53](../crates/teksilo-widgets/src/primitives/column_flow.rs)) |
| 1.3.3 Sensory Characteristics | A | framework-enabled | ✅ | No shipped instructional copy relies on shape or position alone |
| 1.3.4 Orientation | AA | — | ➖ | Desktop, freely resizable windows; no orientation lock |
| 1.3.5 Identify Input Purpose | AA | framework | 🟡 | Role half done ([text_input_field.rs:120-149](../crates/teksilo-widgets/src/primitives/text_input_field.rs)). The autofill-token half is unreachable — but **not for the reason previously given**: AccessKit 0.24.1 *does* have an `AutoComplete` field; it carries the ARIA `aria-autocomplete` behaviour vocabulary (`Inline`/`List`/`Both`), not the HTML autofill tokens. Only the previous revision of *this* file garbled that — [`docs/a11y/a11y_issues.md`](a11y/a11y_issues.md) and the source comment at `text_input_field.rs:113-118` both draw the distinction correctly |
| 1.4.1 Use of Color | A | framework | ❌ | **Three colour-only sites.** (a) Chart series carry no non-colour channel — `ChartSeries` is `{name, color, visible, points}` ([chart_model.rs:87-92](../crates/teksilo-data/src/chart_model.rs)) and the legend maps by swatch; the palette also wraps modulo its length, so series 9 repeats series 1. (b) `CommandPalette`'s highlight is a background fill with no border or marker ([command_palette.rs:556-560](../crates/teksilo-widgets/src/command_palette.rs)). (c) TableView/TreeTableView selection band remains a flat fill (§5.3). List/tree rows *were* fixed (G13) |
| 1.4.2 Audio Control | A | — | ➖ | No auto-playing audio. The terminal bell is visual-only (`BellStyle::{Visual,None}`) — but see 2.3.1 |
| 1.4.3 Contrast (Minimum) | AA | framework | 🟡 | Formula at [color.rs:261-283](../crates/teksilo-tokens/src/color.rs). CI-gated for IntUI ([theme.rs:844-887](../crates/teksilo-tokens/src/theme.rs)); Fluent self-gates with 14 assertions using an alpha-compositing-aware helper ([color.rs:259-264, :334-402](../crates/teksilo-theme-fluent/src/color.rs)); macOS self-gates with ~43. **Material 3 has zero contrast assertions** — its tokens measure fine today but nothing holds them there |
| 1.4.4 Resize Text | AA | framework | ✅ | Global text scale 80–200 % applied through `effective_theme` at relayout. The editor family moved from post-layout page zoom to a true `font_size_scale` since (`dfe23340`), so enlarged text now shapes at a larger ppem rather than stretching a raster. `SegmentedControl`'s 24 dp height was a *ceiling* that clipped labels at raised scale until `7f212749` made it a floor |
| 1.4.5 Images of Text | AA | framework | ✅ | All chrome renders through the glyph-atlas pipeline; no stock widget bakes label text into a raster |
| 1.4.10 Reflow | AA | — | ➖ | Viewport-specific criterion; desktop constraint-negotiation layout is the analogue, not a literal evaluation |
| 1.4.11 Non-text Contrast | AA | framework | 🟡 | Focus indicators fixed for IntUI (G3) and independently gated in Fluent and macOS. Same three colour-only sites as 1.4.1 lack a ≥3:1 boundary. macOS deliberately overrides Apple's own 1.25:1 `separatorColor` hairline for this reason |
| 1.4.12 Text Spacing | AA | framework | 🟡 | **Reclassification qualified.** The N/A rested on "no spacing-override mechanism exists anywhere in the toolkit (exhaustive grep confirms this)". That is now false: `EditorTypographyDefaults` carries `line_height`, `first_line_indent`, `paragraph_spacing_before`/`_after` ([typography_defaults.rs:28-47](../crates/teksilo-text/src/typography_defaults.rs)) and is settable at runtime via `RichTextEditor::set_typography_defaults`. It is host-set rather than end-user-set, and letter/word spacing remain unexposed — so N/A still holds for *chrome* (`TypographyTokens::scaled()` multiplies `size` alone), but editor content is now testable and has not been tested |
| 1.4.13 Content on Hover or Focus | AA | framework | ✅ | *As of `55eb6de4` (2026-08-03), not as of the previous revision.* G12 delivered only *hoverable*. *Dismissible* was broken (Escape scanned `stack.last()` only, so no key dismissed a hover tooltip); *persistent* was broken (a shown tooltip could hang forever). Both fixed at [overlay.rs:896-922](../crates/teksilo-core/src/overlay.rs). Separately, plain-tooltip text reached no AT at all until `1b253c77` gave tooltips a description *owner* ([accessibility_impl.rs:598-623, :718-745](../crates/teksilo-core/src/widget_tree/accessibility_impl.rs)). **Chart tooltips are painted directly and bypass this pipeline entirely** — the ✅ does not extend to them |

### 3.2 WCAG 2.1 — Operable

| Criterion | Lvl | Resp. | Status | Evidence / note |
|---|---|---|---|---|
| 2.1.1 Keyboard | A | framework | 🟡 | Real DFS Tab order + roving tabindex. Strong coverage: Splitter (arrows/Home/End/Enter, [handle.rs:489, :519-528](../crates/teksilo-widgets/src/splitter/handle.rs)), GridView (full 2D nav + Alt+Arrow reorder), SegmentedControl, CommandPalette, table cell navigation (added `15837b69`/`79b916f3`), scene magnetism connect flow. **Three gaps:** no `Key::ContextMenu`/Shift+F10 exists anywhere, so every `.context_menu(..)` is pointer- or AT-only; docking's `split_into_tab`/`stack_into_tab` drop zones have no menu equivalent; `WebView` is never `.focusable()`. Charts are also unreachable — no `focusable`/`on_key` outside the legend |
| 2.1.2 No Keyboard Trap | A | framework | ❌ | Modal and overlay scopes still default to `EscapeOrClickOutside`. But `Terminal` sets `keyboard_capture(true)` ([terminal.rs:633](../crates/teksilo-terminal/src/terminal.rs)) and returns `EventResponse::Handled` from **every** `KeyDown` arm — including the read-only path — after encoding Tab as `\t` and Shift+Tab as CSI Z. Core dispatches Tab to the focused widget first and only cycles focus when unhandled ([event_dispatch_impl.rs:476-492](../crates/teksilo-core/src/widget_tree/event_dispatch_impl.rs)), so neither escapes. No escape chord exists. The editable `CodeEditor` has a milder version: its Tab-indent arm lacks the `tab_escape = ctrl` guard `RichTextEditor` has. Behind the off-by-default `terminal` feature |
| 2.1.4 Character Key Shortcuts | A | framework | ✅ | Type-ahead and mnemonics are active only while the owning component holds focus. Scene magnetism's single-character `m` trigger meets the criterion by both available exceptions — remappable via `connect_key(..)` and active-on-focus-only. `4402128c` fixed hidden menu rows claiming a mnemonic and being activatable through it |
| 2.2.1 Timing Adjustable | A | framework | 🟡 | **Previously scored ➖ n/a on the grounds that "no time-limited interactions exist" — that is false.** `Toast` auto-dismisses after a default 10 s ([toast.rs:65, :443](../crates/teksilo-widgets/src/toast.rs)) and can carry Link/Button actions. Adjustable only by the app author (`auto_dismiss_after`/`persistent`), never by the end user. Partially mitigated: dismissed toasts persist in the notification archive |
| 2.2.2 Pause, Stop, Hide | A | framework | 🟡 | No auto-updating surface ships a pause affordance. `Cycle` (3 s default) has zero `pause`/`paused` occurrences. Joined since by `LogView` tail-following, `Terminal` scroll-on-output, and streaming charts — the chart demo hand-rolls its own pause signal, i.e. the framework supplies none. `Toast` pauses on pointer hover only, never on keyboard focus |
| 2.3.1 Three Flashes | A | framework | 🟡 | The animation wrappers are safe (`Shake` is spatial; `Pulse` is ~1.1 Hz opacity). **The terminal visual bell is not assessed as safe:** `BellStyle::Visual` is the default and every BEL restamps a full-bounds 0.25-alpha flash over 150 ms with no rate limit, no minimum interval and no reduced-motion gate ([terminal.rs:505-516, :816-826](../crates/teksilo-terminal/src/terminal.rs)). A process emitting BEL faster than ~3/s produces a full-viewport sawtooth. Whether the luminance delta crosses the general-flash threshold was not measured; the rate and area clearly qualify for assessment |
| 2.4.1 Bypass Blocks | A | framework-enabled | ✅ | Previously ➖ n/a. Docking sides now emit `Role::Complementary` landmarks ([panel.rs:682](../crates/teksilo-widgets/src/docking/panel.rs)), which a screen reader's landmark rotor consumes — the desktop bypass mechanism. Roving tabindex additionally collapses composite groups to one Tab stop |
| 2.4.2 Page Titled | A | framework | ➖ | Every window is titled — but `WindowConfig::title` is an *optional* builder field defaulting to the literal `"Teksilo"` ([config.rs:134, :276](../crates/teksilo-core/src/window/config.rs)), not "a required, always-supplied parameter" as previously stated. A non-descriptive default is reachable |
| 2.4.3 Focus Order | A | framework | ✅ | Tab order is decoupled from G17's AT-only override. Four post-audit fixes were load-bearing: focus survives a rebuild instead of dropping to `None`; pointer-origin focus no longer triggers scroll-into-view; `Widget::focus_reveal_rect` reveals a tall editor's caret line rather than its whole bounds; plain windows honour `initial_focus_hint` (previously only modals set initial focus, so an ordinary window's first keystroke went nowhere) |
| 2.4.4 Link Purpose (In Context) | A | author | ✅ | `Link` requires a text label at construction; no icon-only variant |
| 2.4.5 Multiple Ways | AA | author | ➖ | Application information-architecture decision |
| 2.4.6 Headings and Labels | AA | framework-enabled | ❔ | Mechanism only. There is no framework code site that enforces every control having a label, and none was verified. Previously scored ✅ without evidence |
| 2.4.7 Focus Visible | AA | framework | ✅ | Focus-visible heuristic plus the shared `focus_ring_width = 2.0` token consumed across the widget styles |
| 2.5.1 Pointer Gestures | A | framework | ✅ | Multi-point and path gestures (scene pan/zoom, marquee) all carry single-pointer or keyboard equivalents |
| 2.5.2 Pointer Cancellation | A | framework | ✅ | Recognizer-based activation on release, not on raw pointer-down |
| 2.5.3 Label in Name | A | framework-enabled | 🟡 | `access_label` can fully replace a control's visible text with no automated check that the accessible name still contains it — not even a debug warning, despite G14 establishing exactly that pattern for a different criterion. Zero label-in-name checks exist anywhere |
| 2.5.4 Motion Actuation | A | — | ➖ | No device-motion, tilt, or camera-gesture input surface |

### 3.3 WCAG 2.1 — Understandable & Robust

| Criterion | Lvl | Resp. | Status | Evidence / note |
|---|---|---|---|---|
| 3.1.1 Language of Page | A | framework | 🟡 | **Previously absent from the matrix.** The walker tags the root `Role::Window` with the app's BCP-47 locale and AccessKit inherits `language` down the tree ([accessibility_impl.rs:198-207](../crates/teksilo-core/src/widget_tree/accessibility_impl.rs)). Gated on `locale_signal` being `Some`, which defaults to `None` — an app not using `teksilo-i18n` emits no language at all and screen readers fall back to a default voice. Until `896d05c6` the OS-locale partial match was dead code, so the tag was wrong for every partial-match user |
| 3.1.2 Language of Parts | AA | framework | ❌ | **Previously absent.** `set_language` has exactly one occurrence workspace-wide and it is the root node. `TextRunAttributes` has no language field, so a mixed-language document cannot be marked. Same root cause as EN 11.5.2.9 and §5.9 |
| 3.2.1 On Focus | A | framework | ✅ | G14's debug-only warning for `open_window`/`focus_window` inside `on_focus`. Correctly self-scoped; does not cover other context-change side effects |
| 3.2.2 On Input | A | author | ✅ | No stock widget triggers a context change purely on value change without explicit activation |
| 3.2.3 Consistent Navigation | AA | author | ➖ | **Previously absent.** Set-of-screens criterion, author-scope like 2.4.5. Framework enablers exist (one `MenuModel` drives both the in-window and native menu bars) but do not enforce it |
| 3.2.4 Consistent Identification | AA | author | ➖ | **Previously absent.** Same footing as 3.2.3 |
| 3.3.1 Error Identification | A | framework-enabled | ✅ | G5 — `described_by` wiring across all validated stock inputs |
| 3.3.2 Labels or Instructions | A | framework-enabled | ✅ | G8, G9. Plus the tooltip description-owner fix (`1b253c77`), without which a hint sat on an unnamed box beside the control it described, for roughly two dozen widgets |
| 3.3.3 Error Suggestion | AA | framework-enabled | ✅ | **Previously absent.** `ValidationStrip` announces `Invalid` at `Live::Assertive` and `Corrected` at `Live::Polite` under `Role::Status` ([validation_strip.rs:4-15, :154-163](../crates/teksilo-widgets/src/primitives/validation_strip.rs)); `InputDialog` adds live validation. Suggestion *text* is author-supplied |
| 3.3.4 Error Prevention | AA | framework-enabled | ✅ | **Previously absent.** `WindowConfig::on_close_requested` → `CloseResponse::Veto` plus `MessageBox`/`Dialog` give the confirm-before-destructive pattern. Which actions are guarded is author-scope |
| 4.1.1 Parsing | A | — | ➖ | No markup surface; removed outright in WCAG 2.2 |
| 4.1.2 Name, Role, Value | A | framework | 🟡 | G1, G15, G17, and — since the previous revision — the fixes listed in §7 that made the declared semantics actually executable. Every post-audit widget family was spot-checked and declares role, name and value: Terminal, WebView, DockingLayout, GridView, SegmentedControl, ColumnFlow, Splitter, CodeEditor/LogView, Toast. **One exception: `CommandPalette`** emits only a bare `Role::Dialog` ([command_palette.rs:487-489](../crates/teksilo-widgets/src/command_palette.rs)) with no name, and its inner `ListView` is built with no `SelectionModel` and no `active_descendant`, so the arrow-key highlight is announced to nobody. `LogView` is partial by design (windowed tree, §5.5). Note: change notification is delegated to `accesskit_winit::Adapter::update_if_active` ([window.rs:415](../crates/teksilo-platform/src/window.rs)) — the previously cited `Tree::update_and_process_changes` does not exist in this workspace |
| 4.1.3 Status Messages | AA | framework | ✅ | G4 (ProgressBar). Now joined by `Toast`'s severity×priority mapping — `Role::Alert` for Error and high-priority Warning, `Role::Status` otherwise, matching `Live` and `live_atomic` so title and body announce as one unit ([toast/surface.rs:117-135, :368-385](../crates/teksilo-widgets/src/toast/surface.rs)) — and by the Terminal's delta-only `Role::Status` announcer. `StatusBar`, `LogView` and `NotificationLog` opt in or deliberately opt out, consistently |

### 3.4 WCAG 2.2 delta — forward look

EN 301 549 v3.2.1 is WCAG 2.1-based; the v4 draft moves to WCAG 2.2. The previous
revision silently scored four 2.2 criteria inside a matrix declared as 2.1, and
mislabelled 2.4.13 as Level AA when it is AAA. They are separated here.

| Criterion | Lvl | Status | Note |
|---|---|---|---|
| 2.4.11 Focus Not Obscured (Min) | AA | 🟡 | A framework-level focus-*reveal* engine does exist — `scroll_focused_into_view` runs on every focus change ([focus_impl.rs:110-133](../crates/teksilo-core/src/widget_tree/focus_impl.rs)), backed by `scroll_rect_into_view` and the public `EventContext::ensure_visible*` family. The previous revision's claim that "`ensure_visible` exists only per-widget" was wrong. But that engine walks `clips_children` scroll ancestors only — it solves *scrolled out of view*, never *covered by content*. Overlays now self-dismiss when focus leaves them (`4152a4c2`), which narrows the exposure. **Zero rect-intersection tests exist** between a focused node and any sticky, pinned, docked or floating rect. Exposure surfaces: corner-anchored toasts, GridView sticky headers, TableView pinned columns, docking side panels |
| 2.4.12 Focus Not Obscured (Enhanced) | AAA | ❌ | Follows from the above |
| 2.4.13 Focus Appearance | AAA | ✅ | 2.0 dp stroke at ≥3:1 (G3). **Level corrected from AA** |
| 2.5.7 Dragging Movements | AA | 🟡 | G6 (scene Alt+Arrow nudge), plus two further alternatives since: Splitter keyboard resize, and the scene magnetism keyboard connect flow ([magnetism.rs:172-240](../crates/teksilo-scene/src/view/magnetism.rs)). GridView's Alt+Arrow reorder is a validated same-view drop. **Docking drag-to-dock is the gap:** its only non-drag route is a context menu, which no keyboard chord can open |
| 2.5.8 Target Size (Minimum) | AA | 🟡 | 24 dp in the default recipe and under Fluent/Material 3; **18 dp under the macOS preset** ([metrics.rs:170-172](../crates/teksilo-theme-macos/src/styles/metrics.rs)). No test asserts a ≥24 floor — the only guard is `size_compact < size_default`. One stale "Compact 22 dp" doc comment remains at [icon_button.rs:98](../crates/teksilo-widgets/src/icon_button.rs) |
| 3.2.6 Consistent Help | A | ➖ | Author-scope |
| 3.3.7 Redundant Entry | A | author | ➖ | Framework hooks exist (`Wizard`/`Stepper` for multi-step processes; `MruList`/`SettingsStore` for re-entry avoidance) but the criterion is satisfied at application level |
| 3.3.8 Accessible Authentication (Min) | AA | ✅ | **`PasswordField` gets the asymmetry right, which is worth stating explicitly.** `copy_allowed()` is consulted at exactly two sites — `clipboard_copy` and `clipboard_cut` ([keyboard.rs:445, :460](../crates/teksilo-widgets/src/primitives/text_input_field/keyboard.rs)). `clipboard_paste` has no such guard, and the context-menu Paste row is the only one of the four built without an `.enabled(..)` clause. So a password manager's paste into a masked field works, while plaintext cannot be copied out. The reveal toggle is an additional transcription aid |
| 3.3.9 Accessible Authentication (Enh.) | AAA | ✅ | Follows from 3.3.8 — no cognitive function test is imposed |

### 3.5 EN 301 549 v3.2.1 — Chapter 5, generic requirements

Absent from the previous revision, whose scope statement named only §9 and §11.
Chapter 5 applies to all ICT including software and is not subsumed by either.

| Clause | Topic | Status | Note |
|---|---|---|---|
| 5.1 | Closed functionality | ➖ | Open functionality — AT attaches through `accesskit_winit` on all three desktop platforms |
| 5.2 | Activation of accessibility features | ✅ | `AccessibilityPreferences::query()` reads OS high-contrast, reduced-motion and text-scale; the framework never suppresses a platform accessibility feature |
| 5.3 | Biometrics | ➖ | No biometric input path |
| 5.4 | Preservation of accessibility info during conversion | ❔ | **Unassessed, with a concrete probe available.** `RichTextEditor`'s clipboard path serialises to `text/html` and parses back. `ea290adb` established that inline images carry `alt` through the model to the AT walk; whether that `alt` survives the HTML round-trip is exactly the 5.4 question, and the serializer lives in the external `text-document` crate |
| 5.5 / 5.6 | Operable parts / locking controls | ➖ | Hardware-scoped |
| 5.7 | Key repeat | ✅ | Delegated to the OS — no repeat or debounce logic in the platform event translation. **Caveat:** `StepButton` implements its own pointer hold-to-repeat with four hardcoded constants (400 ms delay, 120 ms start interval accelerating ×0.88 to a 45 ms floor ≈ 22 Hz), not adjustable, not disableable, and not consulting any preference ([step_button.rs:43-52](../crates/teksilo-widgets/src/spin_box/step_button.rs)). Not a strict 5.7 failure — 5.7 is scoped to key repeat, and SpinBox always offers typed entry and arrow keys — but the same hazard class |
| 5.8 | Double-strike key acceptance | ✅ | Delegated to the OS |
| 5.9 | Simultaneous user actions | ✅ | Every modifier-dependent selection has a single-action alternative — bare `Space` toggles selection in ListView, GridView and TableView alongside Ctrl+Space and Shift+click |

### 3.6 EN 301 549 v3.2.1 — Chapter 11, software

| Clause | Topic | Status | Note |
|---|---|---|---|
| 11.5.2.5 | Relationships | ✅ | `described_by`, `labelled_by`, `controls`, and — new since — `details` for the ARIA annotations pattern ([accessibility.rs:323, :331](../crates/teksilo-core/src/accessibility.rs)) |
| 11.5.2.9 | Text attributes | 🟡 | Bold/italic/underline/strikethrough reach AT (G7); `font_weight` added since. **Colour and per-run language do not:** `set_foreground_color` and `set_background_color` have zero occurrences workspace-wide, and `accessibility.rs:189-203` documents the omission explicitly, hardcoding decorations to opaque black. Now applies to three text stacks, not one — `CodeEditor` exposes syntax-highlighted runs whose colours AT cannot see |
| — | Rows, columns, headers | ✅ | `Role::Table`/`TreeGrid`/`Grid` with per-cell position. Cell navigation was *not* AT-followable until `15837b69`/`79b916f3` added `active_descendant` for the keyboard-focused cell; column reorder also silently relabelled focused cells onto whatever column took the old position |
| — | List of available actions | 🟡 | Advertisement is now derived for context menus ([accessibility_impl.rs:754-790](../crates/teksilo-core/src/widget_tree/accessibility_impl.rs)) and for list/tree rows. `LogView` advertises no scroll actions despite having a windowed tree (§5.5) |
| — | Execution of available actions | ✅ | *As of the July–August fixes, not before.* See §7 — this is the subclause the previous revision had no row for, and the one that was framework-wide broken while it scored 4.1.2 ✅ |
| 11.5.2.14 / .16 / .17 | Change notification | ✅ | Delegated to `accesskit_winit::Adapter::update_if_active` ([window.rs:415](../crates/teksilo-platform/src/window.rs)); G1 and G15 ensure the tree it diffs is current |
| 11.5.2.15 | State exposed to AT | 🟡 | Core reactive state is correct. Two residuals: `WidgetBuilder::access_disabled` takes a plain `bool`, not `impl Into<Prop<bool>>`, unlike every sibling override ([widget_builder.rs:1170, :1771](../crates/teksilo-core/src/widget_builder.rs)); and until `ee3d406f`/`4769528f` the *visual* half was broken framework-wide — nine form fields and every ancestor-disabled widget rendered pixel-identical to enabled while correctly reporting `disabled` to AT |
| 11.5.2.x | AT platform-API binding | ✅ | Real `accesskit_winit` bridges on AT-SPI, NSAccessibility and UIA. `accesskit_winit` 0.33.2, `accesskit` 0.24.1 |
| 11.6.2 | Accessible end-user documentation | ➖ | Applies to shipped products, not the toolkit |
| 11.7(a) | Contrast preference | 🟡 | Applied at both main and overlay paint passes — but `for_high_contrast()` substitutes literal IntUI hexes into whatever preset is active (§5.6) |
| 11.7(b) | Reduced motion preference | 🟡 | G16, and the unified `AccessibilityPreferences` pipeline. **The terminal visual bell is a motion path this pipeline does not reach** (2.3.1) |
| 11.7(c) | Text scaling preference | ✅ | Applied at launch and on every focus-triggered refresh |
| 11.7 | Re-query granularity | 🟡 | Focus-transition-triggered, not an OS push subscription — a deliberate zero-idle-cost choice. A user toggling "increase contrast" while the app holds focus continuously sees no change until a focus transition |

---

## 4. Regime notes

These are notes on *scope*, not verdicts. No conformance position is asserted.

**EAA / EN 301 549 v3.2.1.** The widest of the three regimes: §5 + §9 (all of WCAG 2.1
A/AA) + §11. Consequently it carries every open item in §5 below. The required
artifact is an ACR in the EN 301 549 ITI template, prepared by whoever ships the
product — never by the toolkit. The v4 draft moves §9/§11 to WCAG 2.2, which is why
§3.4 exists.

**US Section 508 (Revised).** References WCAG **2.0** A/AA plus the Chapter 3
functional performance criteria. Most open items here (1.4.10, 1.4.11, 1.4.12, 1.3.5,
2.5.x) are 2.1-or-later and sit outside its literal scope. The 2.0 AA criteria that do
bind — 1.4.3 and 4.1.2 — are in the state described above, meaning 4.1.2's
CommandPalette exception and 1.4.3's Material 3 gap are the two in-scope items.

**RGAA 4.1 (France).** Same criteria set as EAA, but audited through RGAA's 106 test
criteria and conducted manually with real screen readers. That methodology is
precisely the one that catches the defect class §7 describes — correct static markup
that is functionally inert — and precisely the one this document cannot substitute
for. A live manual audit is mandatory regardless of anything written here.

---

## 5. Open findings

Ranked by severity. Each is tagged with who can act on it.

### 5.1 `Terminal` is an unconditional keyboard trap — WCAG 2.1.2 (A) · framework

`Terminal` sets `keyboard_capture(true)` and returns `EventResponse::Handled` from
every `KeyDown` arm, including the `read_only` early return. Tab is encoded to `\t`
and Shift+Tab to CSI Z; core only falls back to `cycle_focus` when the focused widget
did not handle the key. No escape chord exists anywhere in the crate.

The framework already has the correct pattern in three other widgets: `GridView`'s
`Key::Tab` handler returns `None` at either grid boundary, which becomes
`EventResponse::Ignored` and lets core's `cycle_focus` take over.

Treat `keyboard_capture(bool)` itself as the hazard, not just Terminal. It is a
general primitive with no contract requiring an escape, and its doc comment at
[widget_builder.rs:616-624](../crates/teksilo-core/src/widget_builder.rs) currently
promises that "Escape and overlay back-navigation still run first, so an open overlay
can still be closed" — true only while an overlay *is* open. That sentence is what a
future capture-surface author will rely on, and it should be corrected in source.

**Recommendation:** define a framework-level escape chord for capture surfaces
(Ctrl+Tab / Ctrl+Shift+Tab, returning `Ignored`), document it as part of the
`keyboard_capture` contract, and implement it in Terminal. Mitigated today only by
the `terminal` feature being off by default.

### 5.2 Chart series are distinguished by colour alone — WCAG 1.4.1 (A) · framework

`ChartSeries<T>` is `{name, color, visible, points}` with no dash-pattern, marker-shape
or fill-pattern field, and `LineChart` strokes every series at the same width. The
legend maps series to plot by swatch. `ChartPalette::color_for` wraps modulo the
palette length, so a tenth series repeats the second's colour exactly.

The default palette is Okabe–Ito, which is a genuine and deliberate CVD-safe choice
worth crediting — but it addresses whether the colours are *distinguishable*, not the
requirement that colour not be the *only* visual means of conveying information.

**Recommendation:** add a per-series non-colour channel (dash pattern for lines,
marker shape for points, hatch for areas) and render it in the legend swatch.

### 5.3 Colour-only selection: TableView/TreeTableView, and now CommandPalette — WCAG 1.4.1 / 1.4.11 · framework

`RecipeTableStyle::make_row_background`
([recipe_table_style.rs:182-233](../crates/teksilo-widgets/src/styles/recipe_table_style.rs))
still ends in a single `RectWidget::new().background(ColorProp::DynamicSurfaceRole(role))`
with no border call. It gained window-active/view-focus awareness since (`Selected` vs
`SelectedInactive`), which changes the code but not the finding.

**A correction to the previous revision's recommendation:** the stock views also paint
the band directly via `canvas.fill_rect` at
[table_view.rs:2465-2491](../crates/teksilo-widgets/src/table_view.rs), so a fix must
touch **both** the recipe style and the widget's own paint path — porting
`border_color(BorderRole::Focused).border_width(selection_edge_width)` from
`recipe_standard_item_style.rs:133-166` into `make_row_background` alone is not enough.

`CommandPalette` is a second site with the same shape (§5.4).

### 5.4 `CommandPalette` is inert to assistive technology — WCAG 4.1.2 (A) · framework

Four defects in one widget: the root emits a bare, unnamed `Role::Dialog` with no
combobox/listbox relationship and no `active_descendant`; the inner `ListView` is
constructed with no `SelectionModel`, so every row reports `set_selected(false)`; the
highlight is a colour-only background; and arrow keys move `state.selected` while focus
stays in the `SearchField`, with nothing published to AT. Its test module contains no
accessibility assertions.

This is a fresh instance of exactly the defect class §7 describes, shipped after the
remediation that was supposed to close it.

**Recommendation:** give the `ListView` a `SelectionModel` bound to `state.selected`,
point `active_descendant` at the highlighted row, name the dialog, and add a non-colour
highlight cue. `SegmentedControl` is the house reference for getting this right.

### 5.5 Content reachable on screen but not through AT — 4.1.2 / 1.3.1 · framework

Three surfaces hide content from AT that is visually present or reachable, and they
solve it to three different standards:

- **`SegmentedControl`** — correct. Overflowed segments become real `Role::MenuItemRadio`
  rows behind a `HasPopup::Menu` chevron. This is the house pattern.
- **`LogView`** — partial and defensible. The AT walk covers only the visible window
  ([a11y.rs:128-141](../crates/teksilo-widgets/src/code_editor/a11y.rs)), numbered by
  global line index so a reader still hears "line 41002 of 128449". The alternative is
  a 128k-node tree per walk. Keyboard scrolling moves the window and re-walks, so it is
  not a 1.3.1 failure — but AT-action-driven traversal stops at the window edge, because
  `ScrollUp`/`ScrollDown` are not advertised. **Recommendation:** advertise and service
  them, as Terminal does.
- **`CommandPalette`** — not solved. See §5.4.

### 5.6 High contrast overwrites the active preset's palette — EN 11.7(a) · framework

`ColorTokens::for_high_contrast()`
([theme.rs:230-259](../crates/teksilo-tokens/src/theme.rs)) branches only on
`surface_main` luminance and substitutes literal IntUI hexes (`#4FCCE0`, `#0A7B8B`, …)
for the accent family, focus indicators and selection surface. The paint walker applies
it to whatever theme is active. So a macOS-Aqua or Fluent app whose user turns on the OS
high-contrast preference gets IntUI teal — discarding a hand-tuned, individually
WCAG-verified accent family in favour of values verified against a different token set.
No preset crate overrides it, and no test covers high-contrast contrast for those three.

**Recommendation:** make it a per-preset hook (a style slot or theme extension), or
derive the high-contrast values from the live tokens rather than hardcoding one preset's.

### 5.7 `teksilo-theme-material3` has no contrast assertion of any kind — 1.4.3 / 1.4.11 · framework

Fluent ships 14 contrast assertions using an alpha-compositing-aware helper; macOS ships
roughly 43; IntUI is gated by `default_themes_meet_wcag_contrast_minimums`. Material 3
has none — its tests check appearance flags, hex values, typography sizes and elevation
ordering, never a ratio. Hand-measured, its baseline tokens pass today (on-accent text
6.44:1, secondary text 8.88:1, focus ring 6.12:1), but nothing holds them there.

There is also no cross-preset gate: a fourth preset can be added tomorrow with zero
contrast coverage and CI will not notice.

**Recommendation:** port Fluent's `contrast_on` suite to Material 3, and consider a
shared contrast-conformance test helper the preset crates each invoke.

### 5.8 No keyboard route to any context menu — 2.1.1 (A) · framework

The `Key` enum has no `ContextMenu`/Menu-key variant, and `Action::ShowContextMenu` is
serviced only from an AT request or a secondary-button press. Every `.context_menu(..)`
factory in the framework is therefore pointer- or AT-only. This matters most where a
context menu is the *only* non-drag route to a command — docking's "Move to ▸", and any
data-view row whose actions live on hover-only buttons.

**Recommendation:** add `Key::ContextMenu` and a Shift+F10 binding in dispatch.

### 5.9 Text-run colour and per-run language never reach AT — EN 11.5.2.9, WCAG 3.1.2 · framework

`TextRunAttributes`
([accessibility.rs:178-187](../crates/teksilo-core/src/accessibility.rs)) carries only
`font_weight`, `bold`, `italic`, `underline`, `strikethrough`. Workspace-wide,
`set_foreground_color` and `set_background_color` have zero occurrences;
`set_language` has exactly one, on the root `Role::Window` node — document-level
language, not per-run. `accessibility.rs:189-203` documents the colour omission
explicitly and hardcodes decorations to opaque black.

This now spans three text stacks: `RichTextEditor`, `TextInputField`, and `CodeEditor` —
where a syntax-highlighted editor exposes runs whose colours AT cannot see at all.

**Recommendation:** extend `TextRunAttributes` with colour and language fields,
following the pattern G7 established, and emit them from all three walks.

### 5.10 `WebView` is outside the Tab cycle — 2.1.1 / 2.4.3 (A) · framework + author

`WebView::accessibility` emits one node and deliberately does not mirror the page. The
widget declares no `.focusable(true)` and installs no `HandlerSet` — the file contains
zero occurrences of `focus`. The backend trait exposes `WebViewHandle::set_focus()`
([backend.rs:128](../crates/teksilo-webview/src/backend.rs)) but the widget never calls
it. A keyboard-only user cannot reach page content.

Note the structural consequence for anyone assembling a conformance artifact: a
WebView-embedding application has **two disjoint focus rings and two AT trees** —
AccessKit's and the engine's platform tree. It cannot inherit the toolkit's 2.1.1 or
4.1.2 posture; it must scope the embedded page separately.

### 5.11 Toast timing, and pause affordances generally — 2.2.1 / 2.2.2 (A) · framework

`Toast` auto-dismisses after 10 s by default and can carry actionable controls; the only
pause input is a pointer hover count. Keyboard focus does not pause it — `focusable(true)`
is attached only when `closable_on_escape`, and only to bind Escape. A keyboard or
screen-reader user has a hard 10 s window to reach and operate a toast action.

More broadly, no auto-updating surface in the toolkit ships a pause affordance: `Cycle`
has none, `LogView`'s tail-follow is build-time only, `Terminal` scrolls on output, and
the streaming-chart demo hand-rolls its own pause because the framework supplies none.

**Recommendation:** pause the toast group on `focus_within` as well as hover, and adopt a
shared `.paused(impl Into<Prop<bool>>)` convention across the four auto-updating surfaces.

### 5.12 Terminal visual bell is unbounded — 2.3.1 (A) · framework

`BellStyle::Visual` is the default; every BEL restamps a full-bounds 0.25-alpha flash
fading over 150 ms, with no rate limit, no minimum interval between bells, and no
`prefers_reduced_motion` gate. A build-error loop or `yes $'\a'` produces a full-viewport
sawtooth at the bell rate.

**Recommendation:** ignore a restamp while a flash is in flight (capping at ~2 Hz), gate
the path on reduced motion, and measure the luminance delta against a real colour scheme
before claiming it is under threshold.

### 5.13 Smaller framework items

- **2.5.3 Label in Name** — no check, not even a debug warning, that `access_label`
  preserves a control's visible text. G14 established the pattern for a different criterion.
- **`access_disabled` is the only non-reactive `access_*` override** — plain `bool` on both
  the `WidgetBuilder` trait method and the `WidgetWithHandlers` twin. Worth also noting at
  [docs/accessibility-overrides.md:276](accessibility-overrides.md), where it is documented
  without that caveat (and where `access_hidden` is documented as `bool` when it is
  actually `impl Into<Prop<bool>>`).
- **2.5.8 under the macOS preset** — `size_compact: 18.0` and no test asserting a ≥24 floor.
- **One stale doc comment** — `icon_button.rs:98` still says "Compact 22 dp". The second
  occurrence the previous revision cited at `:582` is gone.
- **`StepButton` repeat timing** — hardcoded, non-adjustable, not preference-aware (§3.5, 5.7).
- **`NotificationCenterButton`** — the unread count reaches AT only as a bare badge text
  node, not folded into the bell's accessible name.
- **3.1.1 default** — an app that never calls `set_locale` emits no root language at all.
  Consider deriving a default from the OS locale.
- **2.4.2 default title** — consider a debug-only warning when a window ships with the
  literal default `"Teksilo"`, in G14's spirit.

### 5.14 Upstream and platform

- **WCAG 1.3.5 autofill tokens** remain unreachable. AccessKit is still 0.24.1 (only
  `accesskit_winit` and `accesskit_consumer` were bumped). Its `AutoComplete` enum is the
  ARIA popup-behaviour vocabulary, not the HTML autofill tokens; [`docs/a11y/a11y_issues.md`](a11y/a11y_issues.md)
  and the source comment already state this correctly, and need no change.
- **AT-bridge coverage** varies only by OS backend maturity, not by Teksilo's code.
  `accesskit_unix`, `accesskit_macos` and `accesskit_windows` are all real, non-stubbed
  bindings. X11 gained a custom title bar with its own window menu since, removing a
  platform-specific keyboard dead end.
- **EN 5.4** round-trip preservation is unassessed and depends on the external
  `text-document` serializer.

### 5.15 Author responsibility, always

Alt-text content, 2.4.5 Multiple Ways, 3.2.3/3.2.4 consistency across screens, which
destructive actions get a confirmation, and the assembly of any ACR, VPAT or déclaration.
Add to that list: an app calling `TitleBar::controls_visible(false)` owes the user another
visible way out of fullscreen, and an app embedding a `WebView` owes the page its own
separate conformance scope.

---

## 6. What is genuinely strong

Stated as observations with citations, not as claims of conformance.

- **The contrast discipline in the preset crates is unusually rigorous.** Fluent's own
  suite uses an alpha-compositing-aware `contrast_on(fg, bg) = over(fg, bg).contrast_ratio(bg)`
  helper — methodologically better than the raw ratio the IntUI gate uses, which is
  imprecise for translucent tokens. And the macOS preset documents **four places where it
  deliberately departs from Apple's published colours because Apple's do not meet WCAG**,
  each measured and tested at its assignment: `secondaryLabelColor` measures 3.98:1 against
  1.4.3's 4.5:1 floor; `separatorColor`'s hairline measures 1.25:1 against 1.4.11's 3:1;
  the Default system colours are swapped for Apple's Accessible variants (`systemRed`
  3.55:1 → 5.39:1); and `findHighlightColor`'s pure yellow would put white Dark-Aqua text
  at 1.05:1. A native-look preset that inherits the platform's contrast failures is the
  norm; refusing to is not.
- **The terminal's AT model is the best live-region design in the framework**, whatever
  its keyboard problem. It publishes a genuine reviewable tree — one `Role::Paragraph` per
  visible row with `Role::TextRun` children, trailing blanks trimmed, wide-glyph spacers
  skipped, the VT cursor mapped to the AT caret — and announces new output through a
  separate zero-size `Role::Status` child carrying only the last completed line, rather
  than re-announcing the screen.
- **`SegmentedControl` is the reference answer to "content that overflows out of view".**
  Overflowed segments become real `Role::MenuItemRadio` rows behind a `HasPopup::Menu`
  chevron rather than disappearing from the tree. Its module header states the constraint
  explicitly, which is why it got it right.
- **AT reading order is decoupled from paint order by design.** `Widget::accessibility_children()`
  is consulted at exactly one point in the walker — re-verified — and never leaks into
  Tab order or geometry.
- **Change notification is correctly delegated** to `accesskit_winit`'s own diff machinery
  rather than hand-rolled, and G1/G15 ensure the tree it diffs against is current.
- **High contrast, reduced motion and text scale share one preference pipeline**, so the
  three cannot drift apart — with the two exceptions named in §5.6 and §5.12.
- **`PasswordField` gets the clipboard asymmetry right** (§3.4, 3.3.8): plaintext cannot
  leave the field, and paste into it is never blocked. Most password implementations get
  this backwards, and getting it backwards is a WCAG 2.2 AA failure.
- **Keyboard alternatives to dragging keep being added rather than deferred** — scene
  Alt+Arrow nudge, Splitter arrows/Home/End, GridView Alt+Arrow reorder as a validated
  same-view drop, and the scene magnetism connect flow.

---

## 7. Why the previous revision's verdicts did not hold — and what to change about the method

This section exists because the failure mode is more useful than the findings.

The 2026-07-02 revision scored **4.1.2 Name, Role, Value** and **2.1.1 Keyboard** as
✅ *supported*. In the eight weeks that followed, ordinary bug fixes established that at
the moment it was written:

- six stock widgets — `SegmentedControl`, `ComboBox`, `SplitButton`, `Calendar` day cells,
  `GridView` tiles and `TitleBar` window controls — advertised `Action::Click` to assistive
  technology with **no handler behind it**, so VoiceOver offered a press that did nothing.
  A screen-reader user could not close a window through custom chrome (`893768f7`);
- **the entire menu system was AT-inoperable** — menu bar triggers advertised no action at
  all, and menu items advertised `Click` and never handled it (`d7485ad6`);
- list and tree rows were **two unjoined AT nodes**, one with the role and one with the
  name, so no client could match both, and `invoke_action` on a row was a silent no-op
  (`8d2ef1aa`);
- context menus were **never advertised**, making them simultaneously reachable and
  undiscoverable (`696ab3d8`);
- `RichTextEditor` reported a **frozen caret at character 0** to any screen reader arrowing
  through it, because caret signals were bound at `RepaintOnly` (`2f305370`);
- **plain tooltips had no screen-reader path at all**, and rich ones put their description
  on an unnamed chrome node beside the control (`55eb6de4`, `1b253c77`).

None of that was a regression. All of it was true on 2026-07-02, and the audit did not see
any of it. The structural reason is visible in the shape of the old matrix: it had a row for
**11.5.2.15, "is the state exposed"**, and no row for **"does the advertised action
execute"** — the previous §3.4 collapsed most of the `11.5.2` subclause set into a single
`11.5.2.x` catch-all. Four subclauses went unexamined because there was no cell to write
them in, and the audit's method — read the source, confirm the semantics are declared —
could not have caught the difference anyway.

Three method changes follow, and they are the actionable part of this document:

1. **Never score an action-bearing criterion from declaration alone.** For 4.1.2 and 2.1.1,
   the question is whether an AT-invoked action *executes*, which requires driving it. The
   `teksilo-automation` MCP tooling can invoke AT actions in-process against a live tree;
   that is the cheapest available approximation of an AT test and it was not used.
2. **Keep a per-subclause EN 301 549 matrix.** Delete catch-all rows — a catch-all is where
   unexamined clauses hide.
3. **State the coverage boundary.** The previous revision never said which widgets it
   examined, so a reader could not distinguish "assessed and passing" from "not assessed".
   Three of the widget families that shipped after it carry Level-A defects, and the
   document's silence about its own scope is what made that invisible. §1.1 is the fix.

A fourth, smaller lesson: **`file:line` citations rot fast.** Roughly half of the previous
revision's references were dead within eight weeks, and the bulk `bastyde` → `teksilo`
rename mechanically rewrote every cited path without re-verifying any of them. Prefer citing
a symbol name plus a file; cite a line only when the line is the point.

---

## 8. Companion documents

- [`docs/a11y/a11y_issues.md`](a11y/a11y_issues.md) — the externally-blocked gaps.
  Re-verified at `7b15f57d`: **all three of its items are still accurate**, which makes it
  the one accessibility document in the repo that has aged well. It needs only a new review date. Note its declared scope
  is narrow — gaps Teksilo *cannot* close from its own code — so it is not, and does not
  claim to be, a complete residual register. The framework-side residuals in §5 above appear
  in no other file.
- [`docs/accessibility-overrides.md`](accessibility-overrides.md) — API reference for the
  `.access_*` surface. Makes no conformance claim and does not contradict this document; see
  §5.13 for two small corrections it needs.
- [`docs/teksilo-scene-a11y.md`](teksilo-scene-a11y.md) — scene-specific AT model.
