<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CodeEditor, PlainTextEditor, and LogView

Three multi-line text surfaces over one core
([`crates/teksilo-widgets/src/code_editor/`](../crates/teksilo-widgets/src/code_editor.rs)):

- [`CodeEditor`](../crates/teksilo-widgets/src/code_editor/widget.rs) — a source
  editor: a line-number gutter, a current-line band, indentation, bracket
  handling, multiple carets, and completion.
- [`PlainTextEditor`](../crates/teksilo-widgets/src/code_editor/widget.rs) — the
  same core with the code affordances off and wrapping on: a notes field, a
  commit message, a description box.
- [`LogView`](../crates/teksilo-widgets/src/code_editor/log_view.rs) — a
  read-only, append-only, tail-following streaming view that scales to 100 000+
  lines. Its own page: [Log view](log-view.md).

They are one implementation because they differ in *configuration*, not in kind
— all three are a run of lines with a caret in it. A widget per face would
triplicate the caret, selection, IME, clipboard, scrolling, and accessibility and
let them drift.

## Why not RichTextEditor

[`RichTextEditor`](../crates/teksilo-widgets/src/rich_text.rs) already edits
multi-line text, and this deliberately does not build on it. Its command
vocabulary is tables, lists, blockquotes, and bold — reusing it would put
Tab-navigates-a-table-cell and Ctrl+B-emboldens into a source file, where the
first is wrong and the second is meaningless. Its state carries a table-aware
Ctrl+A ladder and a rich clipboard fragment; this one carries an indent policy
and a caret vector. What the two genuinely share — the caret blink clock, the
debounce window, the scroll arithmetic — lives in the crate-internal
[`common::editor_runtime`](../crates/teksilo-widgets/src/common/editor_runtime.rs),
used by both, so the overlap is factored, not copied.

## Language-agnostic by construction

There is no `Language` enum anywhere in this module. Comment tokens, bracket
pairs, indent width, and completion candidates are
[`CodeConfig`](../crates/teksilo-widgets/src/code_editor/config.rs) values the
application supplies: the editor knows how to toggle a line comment, not that
Rust uses `//`. Guessing would be worse than not knowing — inserting `//` into a
Python file corrupts it silently — so the defaults do only what needs no language
knowledge (indent, auto-indent) and leave comment toggling and bracket handling
**off** until the application says what the tokens are.

```rust
use teksilo::widgets::{CodeEditor, COMMON_BRACKETS};
use teksilo::text_document::TextDocument;

let doc = TextDocument::new();
doc.set_plain_text(source).unwrap();

let editor = CodeEditor::new(doc)
    .font_family("monospace")           // a code editor wants a monospace family
    .line_comment("//")                 // enables Ctrl+/
    .bracket_pairs(COMMON_BRACKETS.to_vec())
    .auto_close_brackets(true)          // typing '(' inserts ')'
    .bracket_matching(true)             // the caret's bracket + its match wash
    .completion_provider(|ctx| complete(ctx.prefix));
let handle = editor.handle();           // drive it from a toolbar / status bar
```

`CodeEditor::read_only(doc)` is the same, minus the caret: navigation, selection,
and copy only, `Role::Document`. Its right-click menu and its touch selection
toolbar therefore offer Copy and Select All and nothing that would change the
text — see [Pointer input](#pointer-input-two-devices).

## Builders

Shared by `CodeEditor` and `PlainTextEditor`:

| Builder | Effect |
| --- | --- |
| `wrap_mode(WrapMode)` | `CodeEditor` defaults to `None` (a wrapped source line breaks the gutter's one-number-per-line correspondence); `PlainTextEditor` defaults to `Word`. |
| `v_scroll_policy` / `h_scroll_policy` | `Auto` (default) / `AlwaysOn` / `AlwaysOff`. |
| `min_lines` / `max_lines` | Switch from greedy to intrinsic sizing — grow with content up to `max_lines`, then scroll (the composer pattern). |
| `font_family` / `zoom` / `follow_text_scale` | Typography. `follow_text_scale` (default on) grows text with the global accessibility scale. |
| `background` / `text_color` / `caret_color` / `selection_color` | `Color`, a theme role, or a `Signal`. |
| `on_change(Fn)` | Fired once per drain batch that contained a real edit. |
| `window_to_clip(bool)` | Cull the render to the visible clip band — only for an editor laid out at full document height inside an outer `ScrollArea`. |

`CodeEditor`-only:

| Builder | Effect |
| --- | --- |
| `gutter(bool)` (default on) | The line-number gutter. |
| `current_line_highlight(bool)` (default on) | A full-width band under the caret's line. |
| `indent_style` / `tab_width` / `use_soft_tabs` | Spaces of a width, or tabs rendered a width wide. |
| `auto_indent(bool)` (default on) | Enter carries the line's leading whitespace. |
| `line_comment(token)` | Enables `Ctrl+/`. Unset leaves it a no-op rather than guessing. |
| `bracket_pairs(pairs)` / `auto_close_brackets` / `bracket_matching` | Delimiter handling. Empty pairs (the default) disables both. |
| `completion_provider(Fn)` / `auto_complete(bool)` | See [Completion](#completion). |

## Code semantics

Every command in [`keyboard.rs`](../crates/teksilo-widgets/src/code_editor/keyboard.rs)
is driven by injected configuration and is a single atomic undo step:

- **Auto-indent on Enter** carries the previous line's indentation (and splits a
  `{}` pair onto its own indented line when the caret is between them).
- **Smart Tab / Shift+Tab** — soft or hard tabs; with a selection, indent /
  dedent every touched line.
- **Ctrl+/** toggles the configured line comment on the caret's line or selection.
- **Ctrl+D** duplicates the selection, or the caret's whole line when there is
  none; **Alt+↑ / Alt+↓** move the line.
- **Auto-close, type-over, and pair-backspace** for configured brackets;
  **bracket matching** publishes the caret's bracket and its partner as a
  reactive `Signal<Option<(usize, usize)>>` (via `handle.bracket_match()`) and
  washes both cells behind the text.
- **Multiple carets** — `Ctrl+Alt+↑/↓` add a caret above/below, Alt-click adds one
  at the pointer; typing goes to every caret at once, in one undo step. The
  accessibility tree reports only the primary caret.

### Caret motion follows the platform

Word-jump, the line edge and the document edge sit on different modifiers on
macOS than they do elsewhere, and the difference is not a simple substitution —
so the chords are read through
[`common::text_nav`](../crates/teksilo-widgets/src/common/text_nav.rs) rather
than from an "is the accelerator held?" flag:

| motion | Windows / Linux | macOS |
| --- | --- | --- |
| character | `←` `→` | `←` `→` |
| word | `Ctrl+←/→` | `⌥←/→` |
| line edge | `Home` `End` | `⌘←/→`, `Home` `End` |
| document edge | `Ctrl+Home/End` | `⌘↑/↓`, `⌘Home/End` |
| delete word | `Ctrl+⌫` `Ctrl+⌦` | `⌥⌫` `⌥⌦` |

`Alt+↑/↓` stays on move-line here on every platform, macOS included — that is
the binding every code editor ships, and it takes precedence over the
paragraph motion the rich-text editor puts there. `⌘⌫` means delete-to-line-start
on macOS, which is not implemented; it falls through to a plain single-character
delete rather than removing more than was asked for.

`Shift` extends the selection over any of them, and the policy filter is asked
about the motion that actually runs — a `MoveWordLeft` veto bites on `⌥←`
exactly as it bites on `Ctrl+←`.

## Pointer input: two devices

A **precise** pointer edits exactly as it always has, and that is a property this
package tests rather than assumes: a press places the caret at the press, a drag
selects what it crosses, `Alt`-click adds a caret, and a double or triple click
selects the word or the line. Nothing a mouse does reaches any of the machinery
below.

A **direct** pointer — a finger, a pen — commits nothing on the press. The same
contact is the opening sample of a *pan*, and a panning finger has to leave the
caret and the selection exactly as it found them; no release-time test can rescue
a caret already written on `PointerDown`, so the write itself moved to the
release. Two conditions gate it, and both are needed:

* the press was not claimed by a scrollable above
  (`release_completes_the_press`), and
* it travelled no further than a tap of its own kind may (18 dp for a finger).

The second exists because the framework's tap boundary for a coarse pointer is
the pressed node's **rectangle** — the right rule for a control, where the whole
node is one target and a finger covers it, and the wrong one for a text surface,
whose target is a character. An editor is also both the press's owner *and* the
pan's claimant, so nothing is ever "claimed elsewhere": without the radius check
a finger could pan a tall editor from the first line to the last and land a caret
wherever it stopped.

Two press-time commitments therefore have no direct-pointer form, each
deliberately:

| commitment | why not, and what replaces it |
| --- | --- |
| drag-select | a finger's drag pans; the range is chosen with the selection handles below |
| `Alt`-click's extra caret | there is no `Alt` on a touch screen. `Ctrl+Alt+↑/↓` stays the route, and a hold **collapses** a multi-caret set, because a touch selection is one range |

(The rich text editor gives up two more — picking selected text up to drag it, and
grabbing a picture's resize grip. A source document has neither.)

### Selecting with a finger

A **hold** selects the word under the contact and raises the touch chrome:
two 24 dp handles inside 44 dp targets, and a selection toolbar. Dragging a
handle adjusts that end; dragging one past the other grows the range on the far
side rather than collapsing it. Two details of the drag are worth knowing:

* The sample is translated onto the **caret the handle marks**, not taken raw.
  A handle's disc hangs off the line on purpose, so the fingertip does not cover
  the character it is pointing at — and hit-testing the fingertip would resolve
  onto the *next* line.
* Dragging into the viewport's edge band **auto-scrolls**, so a selection can
  grow past the visible text. The band is the shared
  [`common::drag_autoscroll`](../crates/teksilo-widgets/src/common/drag_autoscroll.rs)
  one every dragging view uses, which for a finger is 64 dp. It scrolls as the
  contact *moves* through the band; a finger held perfectly still there does not
  keep scrolling.

The **toolbar** is the right-click menu's own rows — one builder, so a command
cannot be offered by one surface and refused by the other. A command the surface
will not honour is **omitted** rather than greyed, which is what every platform's
touch toolbar does, and the offer also narrows with the *state*: Cut and Copy need
a selection, Paste needs an editable surface, and Select All is offered only while
nothing is selected. So a hold on an editable surface puts up Cut / Copy / Paste,
and a hold on a read-only one — a viewer, a `LogView` — puts up Copy alone. A tap
on a read-only surface raises **no** handle at all: there is no caret to place.

The chrome is retired by a cursor's press anywhere in the surface, by focus
leaving, and by a keystroke that moves the caret. The affordance band is exempt
from the framework's outside-press dismissal — every caret-moving tap is outside
a handle — so those are the surface's own job.

Full contract, including the magnifier and the accessibility of a handle:
[Touch text editing](text-touch-editing.md).

### The right-click menu

Cut / Copy / Paste / Select All, built fresh on each right-click so every row's
enabled state reflects the live selection and the live policy. All three faces
gained it in the same package that gave them a finger: they had **none** before,
so the only route to the clipboard was a chord — and `LogView`, where copying is
the only thing a reader can do at all, had only `Ctrl+C`.

Each row asks the predicate the surface's own chord asks, so the menu can neither
offer a Cut the editor refuses nor refuse a Copy it allows. Cut is offered with no
selection, because it takes the caret's whole line — the convention `Ctrl+X`
already followed here. Replace the menu with `context_menu(factory)` or suppress
it with `default_context_menu(false)`; neither touches the touch toolbar, which is
raised by a selection rather than by a button a finger does not have.

### The on-screen keyboard

Two things follow a caret placed by a **pointer**, and neither did before:

* The **IME candidate area** is reported, as it already was for every caret move
  made by a key. Without it the candidate list — and the suggestion strip an
  on-screen keyboard draws — stays beside wherever the caret was last typed to.
* A viewport that gets **smaller** re-reveals the caret. The keyboard rising under
  a focused editor is the case that makes this a correctness matter rather than a
  nicety. Growing the viewport deliberately does *not* chase the caret: growth
  reveals more text and never pushes it out, and a reader scrolled away would
  otherwise be yanked back every time a window edge moved. A `LogView` is exempt
  — it has no caret, and pulling its offset would fight its own follow-tail rule.

## Completion

Supply candidates with `completion_provider(Fn(&CompletionContext) -> Vec<CompletionItem>)`;
the editor filters them by the word before the caret, shows a caret-anchored
popup, and replaces the word on accept. Language-agnostic — the app knows the
candidates (keywords, in-scope names, an LSP reply), the editor knows the
mechanics. Without a provider there is no completion.

- [`CompletionContext`](../crates/teksilo-widgets/src/code_editor/completion.rs)
  carries the `prefix`, `line`, `column`, and document `position`.
- [`CompletionItem`](../crates/teksilo-widgets/src/code_editor/completion.rs) is
  `new(label).insert_text(..).detail(..).kind(CompletionKind)`.
- The editor owns the keys while the popup is open (Up/Down/PageUp-Down/Enter/Tab/
  Escape) — the popup is a detached overlay, not an ancestor, so keys cannot
  bubble to it. The ARIA listbox pattern (`HasPopup::Listbox` + `AutoComplete::List`
  + `active_descendant`) is on the editor node.
- `auto_complete(false)` restricts opening to `Ctrl+Space`.

## Accessibility

Both the editor and the log present their text to assistive technology as
`Role::TextRun` children of their own node — one run per visual line, or
several linked runs where a line is split by syntax highlighting or by the
255-character cap — built by the shared walk in
[`a11y.rs`](../crates/teksilo-widgets/src/code_editor/a11y.rs) on top of
`teksilo_core::accessibility::text_runs`. Each run carries the per-character
byte lengths, word starts, bounding box, reading direction and per-character
extents a screen reader needs to speak, navigate and route braille to, plus:

- **Same-line run linking** (`next_on_line` / `previous_on_line`) so a reader
  navigating by line does not stop at each syntax-highlight colour boundary.
- **A trailing newline** on each line's last run (AccessKit's line-break
  contract; the caret can never address it).
- **Chunking of runs over 255 characters** into linked ≤255-character runs —
  `word_starts` are character indices stored as `u8`, so a long line would
  otherwise lose word navigation past character 255.

The runs hang **directly** off the editor's node. They used to hang off a
`Role::Paragraph` per line, and that node was doing two kinds of damage:
`accesskit_consumer::common_filter` does not exclude Paragraph, so every line
was an object-navigation stop of its own; and a run's update routes to its
*filtered* parent, which was the Paragraph, which supports no text ranges — so
macOS, Windows and AT-SPI all dropped every text-change event the editor
produced. Removing it cost the "line 42 of 200" ordinal, which needed
`position_in_set` on those nodes. Line position is available through line
navigation on every platform, and in the gutter, which is hidden from AT.

Editable surfaces report `Role::MultilineTextInput` and advertise `SetValue` /
`ReplaceSelectedText` / `SetTextSelection`; read-only ones report `Role::Document`
(not `Role::Code`, which `accesskit_consumer` excludes from text-range support, so
a caret could not be tracked through it) and advertise, of those three,
`SetTextSelection` alone. Both also advertise `Focus` and `ScrollIntoView`, which
are not text edits and are offered whatever the policy — the contrast above is
about the actions that *write*.
An AT-initiated `SetTextSelection` resolves back to a document cursor position
through a per-run synthetic-node map. The editor walks the whole bounded document
(cached); the log walks only its visible window — see [Log view](log-view.md).

## Rendering & scale

The body paints via the shared
[`rich_text::paint::paint_frame`](../crates/teksilo-widgets/src/rich_text/paint.rs)
over the [`RichTextEngine`](../crates/teksilo-text/src/rich_text_engine.rs). For a
bounded document the standard full layout is right; for the unbounded streaming
case the `LogView` uses the windowed layout path — the text-stack additions that
make that possible (windowed `layout_window`, O(1) append, front-truncate) are
documented in [Log view](log-view.md), which also carries the before/after
benchmark table.

## Testing

The core is fully headless — no GPU, no display. Tests run against the private
engine's fixed metrics and verify the editor's own logic (viewport adoption,
caret bookkeeping, event classification, policy gating, the a11y walk), not
shaping, which is `text-typeset`'s own suite's job. See
[`code_editor/tests.rs`](../crates/teksilo-widgets/src/code_editor/tests.rs) and,
for the pointer / menu / touch behaviour above — both devices, all three faces —
[`code_editor/touch_tests.rs`](../crates/teksilo-widgets/src/code_editor/touch_tests.rs).

## Demos

```bash
cargo run -p code_editor    # gutter, brackets, comment toggle, multi-caret,
                            # an injected highlighter and completion
cargo run -p log_view       # the streaming face — see docs/log-view.md
```
