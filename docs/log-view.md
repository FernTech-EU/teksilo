<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# LogView — a scalable streaming log

[`LogView`](../crates/bastyde-widgets/src/code_editor/log_view.rs) is the code
editor's core turned inside out: instead of a bounded document a person edits,
it is an unbounded one the *program* appends to and the person only reads,
scrolls, selects, and copies. It reuses
[`CodeEditorState`](../crates/bastyde-widgets/src/code_editor/state.rs) — so
selection, copy, theming, and accessibility come for free and cannot drift from
the editors' — but owns its own frame step
([`log_stream.rs`](../crates/bastyde-widgets/src/code_editor/log_stream.rs)) and
paint body, because content arrives faster than a person types, forever, and
neither the editor's full relayout nor its event handling can carry that load.

```rust
use bastyde::widgets::{LogView, LogViewHandle};
use bastyde::tokens::Color;

let log = LogView::new()
    .scrollback_limit(50_000)                 // bound the retained lines
    .severity_highlighter(|line| {            // colour a line by what it is
        if line.contains(" ERROR ") { Some(Color::new(0.92, 0.36, 0.36, 1.0)) }
        else if line.contains(" WARN ") { Some(Color::new(0.92, 0.72, 0.28, 1.0)) }
        else { None }
    })
    .font_family("monospace");
let handle: LogViewHandle = log.handle();     // append from anywhere on the UI thread
```

Feed it through the handle: `append("line")`, `append_line`, `append_lines(iter)`
(splitting on `\n`; a single trailing newline is a terminator, not a blank line),
`clear()`, and `scroll_to_bottom()`. `handle.line_count()` is a reactive
`Signal<usize>` of the retained count for a status bar.

## Two costs, bounded

A naive multi-line view over a growing document has two costs that grow without
bound. `LogView` answers each; the numbers below are from
[`text-typeset/docs/streaming-baseline.md`](../../text-typeset/docs/streaming-baseline.md)
(a 65-char log line, no-wrap, 16 px).

### Appending one line

Because a block-count change invalidates a full layout, a consumer with no
tail-append entry point is forced to re-lay-out the whole document on every
appended line — O(N):

| Lines | Full relayout (per line) | Windowed append (per line) | speedup |
|---:|---:|---:|---:|
| 1 000 | 10.9 ms | ~10 µs | 1 046× |
| 10 000 | 113 ms | ~10 µs | 11 655× |
| 100 000 | **1.167 s** | ~10 µs | **117 007×** |

The `LogView` never re-lays-out the whole buffer: `drain_events`, told the state
is streaming, sets a re-window flag instead of forcing a relayout, and a frame's
arrivals are batched into one `append_lines`.

### Holding a large buffer

A resident *shaped* line costs ≈ 6.5 KB, so laying out the whole document is the
real memory sink:

| Lines | Fully resident | Windowed (viewport only) |
|---:|---:|---:|
| 1 000 | 10.8 MB | 3.7 MB |
| 10 000 | 68.1 MB | 3.7 MB |
| 100 000 | **622.9 MB** | **3.7 MB** (168× less) |

`LogView` shapes only the rows the viewport can show, via
[`RichTextEngine::layout_window_from_snapshots`](../crates/bastyde-text/src/rich_text_engine.rs),
placing each row arithmetically at `y = index × row_height`; the scrollbar spans
the whole document even though almost none of it is shaped. Render already culls
to the viewport, so shaping the rest only ever cost memory. The document's raw
text (a rope, ≈ 65 B a line) is cheap by comparison — ~6.5 MB at 100 k.

### The text-stack additions

The windowed path is additive to the sibling crates, so `RichTextEditor`'s paths
are byte-for-byte unchanged:

- **text-typeset** — `DocumentFlow::{layout_window, set_uniform_extent, add_block,
  remove_leading, block_params_for}`.
- **bastyde-text** — `RichTextEngine::{layout_window, layout_window_from_snapshots,
  set_uniform_extent, append_block, remove_leading, block_visual_info}`.
- **text-document** — `TextDocument::{append_line, append_lines, truncate_front}`
  (undoable-false, all-or-nothing), whose events now also reach `on_change`
  subscribers, not only the poll path.

## Windowing internals

The visible window `(first, count)` is computed from the scroll offset and a
learned uniform row height (all three of the render window, the a11y window, and
the a11y-change check share one `window_bounds` helper so they cannot disagree).
Rows are located by **chaining character positions through the rope**
(`snapshot_block_at_position`) — each row is one O(log n) snapshot, not the O(n)
block walk `TextBlock::next` would be — forward from a cached `(row, position)`
anchor that the tail-following hot path advances a few rows at a time. Windowing
is therefore O(window · log n) in steady state, with a cold O(n) locate only on a
far scrollbar jump or after an eviction drops the anchor.

The document's `block_count` stat does not count its initial empty block, so the
view keeps an authoritative line count instead and fills that initial block with
the first line — a fresh log opens on real content, not a blank line.

## Following the tail

Following is **derived from scroll position**, not a mode flag that fights the
user: the view sticks to the bottom only while it is already at the bottom
(`scroll_y ≥ max_scroll_y − ε`). Scroll up to read history and it pauses; scroll
back (or `scroll_to_bottom()`) and it resumes — the behaviour of every terminal,
and the one that composes correctly with a stray key or click nudging the
viewport. Set `follow_tail(false)` to hold position as the buffer grows.

## Scrollback

`scrollback_limit(n)` evicts the oldest lines past `n` from the front. Eviction
is amortised over a slack band that scales down with the cap (so a small cap is
still honoured tightly), and `truncate_front` shifts the cursors automatically, so
a live selection stays glued to surviving text. Unset (the default) keeps every
line: memory stays flat in the line count (only the window is shaped), but the
raw text accumulates in the rope and each append stays linear in the document
size — so a genuinely unbounded, sustained high-rate producer should set a cap.

## Accessibility

`Role::Document` (not `Role::Log` — that role is excluded from `accesskit`'s
text-range support, so a reader could not track a caret through it), read-only,
with the same paragraph/run walk as the editor
([`a11y.rs`](../crates/bastyde-widgets/src/code_editor/a11y.rs)) — but **windowed**:
only the visible lines are emitted as paragraphs (numbered by global line, "line
41 002 of 128 449"), so an append re-walks O(window), not O(document). The tree
re-walks on the log's own `a11y_version`, bumped only when the visible window
changes — a scroll crossing a row, a following-tail append, an eviction — not on a
sub-row pixel scroll or a tail append arriving while the reader is scrolled away.
Opt into a `Live::Polite` region for new lines with `announce_appends(true)` — off
by default, because a live region is right for a handful of meaningful events and
hostile for a build log at fifty lines a second.

## Threading

The handle is UI-thread (`Rc`). Feeding a log from a background thread (a PTY
reader, a tracing layer) means marshalling the lines to the UI thread first —
through the app's async executor, or a channel drained in a handler. Each append
wakes the view, which otherwise stops asking for frames when idle.

## Demo

```bash
cargo run -p log_view      # a synthetic ~40-lines/frame producer, severity
                           # colour, follow-tail, a 50k scrollback cap, and a
                           # "Burst 10k" button to watch the windowing hold
```
