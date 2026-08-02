<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# RichTextEditor

Rich text editor and viewer widget.

Two construction presets share the same implementation: `RichTextEditor::editor`
provides a full editing surface (blinking caret, keyboard commands, clipboard,
undo/redo, `Role::MultilineTextInput`) and `RichTextEditor::read_only` is a
view-only surface (hidden caret, mutations rejected, `Role::Document`). Both
bind to an external `TextDocument`
via `on_change` subscriptions, so any number of editors and viewers can share
one document and observe each other's edits live.

The widget owns a per-widget `RichTextEngine` (typesetter), and drives its own
scroll bars independently of `ScrollArea` to avoid the wrap/scrollbar circular
measurement dependency. Use `RichTextEditor::min_lines` /
`RichTextEditor::max_lines` to switch from greedy sizing to intrinsic
(messenger-composer) sizing. A detachable `EditorHandle` lets toolbars and
palette panels issue formatting commands from closures that cannot borrow the
editor directly.

```ignore
use bastyde_text::text_document::TextDocument;
let doc = TextDocument::new();
let editor = RichTextEditor::editor(doc)
    .min_lines(3)
    .max_lines(8)
    .wrap_mode(WrapMode::Word);
```

## Builder methods at a glance

`read_only`, `editor`, `style`, `content_padding`, `content_padding_symmetric`, `content_padding_each`, `content_padding_top`, `content_padding_right`, `content_padding_bottom`, `content_padding_left`, `wrap_mode`, `show_highlights`, `annotation_spans`, `set_highlight_mask`, `typography_defaults`, `background`, `selection_color`, `caret_color`, `text_color`, `v_scroll_policy`, `h_scroll_policy`, `window_to_clip`, `scroll_policy`, `follow_caret_in_page`, `typewriter`, `overscroll_behavior`, `min_lines`, `max_lines`, `follow_text_scale`, `font_size_scale`, `context_menu`, `default_context_menu`, `font_registrar`, `on_change`, `document_version`, `cursor_position`, `cursor_anchor`, `is_composing`, `cursor_position_signal`, `cursor_anchor_signal`, `has_selection`, `can_undo`, `can_redo`, `caret_char_format`, `scroll_y`, `scroll_x`, `context_target_at`, `selected_text`, `select_all`, `deselect`, `insert_text`, `insert_html`, `insert_djot`, `insert_block`, `insert_image`, `delete_selection`, `select_word`, `select_line`, `set_caret_position`, `focused_signal`, `select_range`, `reveal_range`, `set_bold`, `set_italic`, `set_underline`, `set_strikethrough`, `set_font_size`, `set_font_family`, `toggle_bold`, `toggle_italic`, `toggle_underline`, `toggle_strikethrough`, `set_superscript`, `set_subscript`, `set_vertical_alignment`, `get_vertical_alignment`, `is_superscript`, `is_subscript`, `toggle_superscript`, `toggle_subscript`, `apply_block_format`, `apply_text_format`, `set_alignment`, `clear_direction`, `set_direction`, `set_heading_level`, `insert_list`, `create_list`, `indent`, `outdent`, `remove_from_list`, `is_in_blockquote`, `selection_spans_multiple_frames`, `toggle_blockquote`, `increase_blockquote_depth`, `decrease_blockquote_depth`, `insert_table`, `remove_current_table`, `insert_row_above`, `insert_row_below`, `insert_column_before`, `insert_column_after`, `remove_current_row`, `remove_current_column`, `is_in_table`, `is_bold`, `is_italic`, `is_underline`, `is_strikethrough`, `get_heading_level`, `get_alignment`, `get_direction`, `undo`, `redo`, `begin_edit_block`, `end_edit_block`, `edit_block`, `set_default_language`, `default_language`, `handle`, `copy`, `cut`, `paste`, `paste_unformatted`, `can_paste`, `set_font_size_scale`, `get_font_size_scale`, `set_typography_defaults`, `get_typography_defaults`, `set_typewriter`, `get_typewriter`, `set_caret_highlight`, `get_caret_highlight`, `caret_window_rect`, `format_version`, `document_loaded_count`, `on_link_activated`, `on_image_activated`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/rich_text/index.html)

## `pub enum ScrollPolicy`

Scroll bar visibility policy for `RichTextEditor`, applied independently per axis.

```rust
pub enum ScrollPolicy { /* variants */ }
```

### Variants

- **`Auto`** — Show the scroll bar only when content overflows the visible area (default).
- **`AlwaysOn`** — Always show the scroll bar, reserving gutter space even when content fits.
- **`AlwaysOff`** — Never show the scroll bar; useful when embedding the editor inside an outer `ScrollArea` or in headless tests.

## `pub struct RichTextEditor`

```rust
pub struct RichTextEditor { /* fields */ }
```

### Methods

#### `pub fn read_only(document: TextDocument) -> Self`

Construct a read-only rich text viewer bound to `document`. The
document can also back an editable `RichTextEditor::editor` in
another part of the UI — both widgets receive document events
independently via `on_change` subscriptions.

#### `pub fn editor(document: TextDocument) -> Self`

Construct an editable rich text editor bound to `document`.
Uses the full editor preset: every command accepted, caret
blinks, `MultilineTextInput` accessibility role, full clipboard
support. Multiple editors on the same document share live edits
via per-widget `on_change` subscriptions.

#### `pub fn style(mut self, style: impl RichTextEditorStyle) -> Self`

Per-call style override for the editor chrome (border, padding,
focus ring). Replaces the theme-wide
`style_slots.rich_text_editor` and the IntUI default
`RecipeRichTextEditorStyle` for just this editor.

#### `pub fn content_padding(mut self, amount: f32) -> Self`

Set a uniform padding (logical pixels) between the text content
and the editor's chrome. Replaces the style's default insets
(TextInput-style for editable, none for read-only). Use
`content_padding_symmetric` or
`content_padding_each` for
per-axis / per-edge control.

#### `pub fn content_padding_symmetric(mut self, vertical: f32, horizontal: f32) -> Self`

Set vertical and horizontal padding (logical pixels) between the
text content and the editor's chrome. Replaces the style's
default insets.

#### `pub fn content_padding_each(mut self, top: f32, right: f32, bottom: f32, left: f32) -> Self`

Set per-edge padding `(top, right, bottom, left)` between the
text content and the editor's chrome. Replaces the style's
default insets.

#### `pub fn content_padding_top(mut self, top: f32) -> Self`

Set just the top inset between the text and the chrome. Leaves
the other edges at their previously-set values, defaulting to
`0.0` for any edge never touched.

#### `pub fn content_padding_right(mut self, right: f32) -> Self`

Set just the right inset between the text and the chrome.

#### `pub fn content_padding_bottom(mut self, bottom: f32) -> Self`

Set just the bottom inset between the text and the chrome.

#### `pub fn content_padding_left(mut self, left: f32) -> Self`

Set just the left inset between the text and the chrome.

#### `pub fn wrap_mode(self, mode: WrapMode) -> Self`

Set the line-wrap mode. `WrapMode::Word` (the default) wraps at word
boundaries; `WrapMode::None` allows horizontal overflow — pair with
`.h_scroll_policy(ScrollPolicy::Auto)` to expose a scroll bar.

#### `pub fn show_highlights(self, show: bool) -> Self`

Whether this view applies the document's syntax / search / spell
highlighting. `editor` defaults to `true`; `read_only` defaults to
`false` (a bare preview). A highlights-off view pulls a *clean*
snapshot (no highlights at all, even metric ones like keyword bold) and
ignores paint-only highlight events entirely, so it does zero work when
the shared document's search/spell highlights change.

#### `pub fn annotation_spans(self, spans: Vec<TextAnnotationSpan>) -> Self`

Declare the annotations (comment threads) covering ranges of this
document, for the **accessibility tree only**.

Each span becomes a `Role::Comment` node, and every `Role::TextRun` it
covers points at it through AccessKit's `details` relation — the W3C
annotations pattern, and the reason a screen reader can say "has comment"
and let the user navigate in rather than reciting the thread every time the
caret crosses the span.

Painting is a separate concern: a highlight session draws the underline. A
highlight carries no text and this carries no colour, so neither is
derivable from the other and both are supplied independently.

#### `pub fn set_highlight_mask(&self, mask: bastyde_text::text_document::HighlightMask)`

Set which highlight sessions **this view** renders, at runtime.

`HighlightMask::all` shows every
session on the document (the default);
`HighlightMask::only` shows a
chosen set — which is how a per-editor find banner
keeps one pane's find highlighting out of another pane over the same document.
`show_highlights(false)` still overrides this to nothing.

Forces a re-pull on the next tick so the change is visible immediately.

#### `pub fn typography_defaults(self, defaults: EditorTypographyDefaults) -> Self`

Set the initial non-destructive default typography (font family / line
height / first-line indent) applied to runs and blocks that carry no
explicit override. Applied before the first layout. These are display
defaults — they never mutate the bound document (no undo entry, no
`modified`); use `set_typography_defaults`
or `EditorHandle::set_typography_defaults` to change them after mount.
Preferred text size is `font_size_scale`.

#### `pub fn background(self, color: impl Into<ColorProp>) -> Self`

Override the editor background fill. Accepts a `Color`, a theme role
(`SurfaceRole::Content`, …), or a `Signal`. Threaded into the active
`RichTextEditorStyle`'s `make_body`, so the common case ("give the
editor a surface") needs no custom style. `None` uses the style's
default surface.

#### `pub fn selection_color(self, color: impl Into<ColorProp>) -> Self`

Override the selection-highlight color. Accepts a `Color`, theme role,
or `Signal`. Resolved against the active theme on every paint; `None`
uses the engine/theme default.

#### `pub fn caret_color(self, color: impl Into<ColorProp>) -> Self`

Override the caret / insertion-point color. Accepts a `Color`, theme
role, or `Signal`. Resolved against the active theme on every paint;
`None` tracks the theme's `editor_caret` role.

#### `pub fn text_color(self, color: impl Into<ColorProp>) -> Self`

Override the default text color. Accepts a `Color`, theme role, or
`Signal`. Resolved against the active theme on every paint; `None`
tracks the theme's `editor_fg` role (so dark / light swaps follow
automatically). A role or `Signal` stays reactive; a bare `Color` pins
it.

#### `pub fn v_scroll_policy(mut self, policy: ScrollPolicy) -> Self`

Set the vertical scroll-bar visibility policy.

#### `pub fn h_scroll_policy(mut self, policy: ScrollPolicy) -> Self`

Set the horizontal scroll-bar visibility policy.

#### `pub fn window_to_clip(self, on: bool) -> Self`

Window paint-time culling to the accumulated ancestor clip rather than
this editor's own bounds.

Enable this **only** for an editor deliberately laid out at its full
document height inside an outer `ScrollArea`
(`v_scroll_policy(ScrollPolicy::AlwaysOff)`, no `max_lines`) — "dubious
mode". Such an editor's own viewport spans the whole document, so the
viewport-derived render cull keeps nothing; this makes it cull to the
visible clip band instead, so a huge document only rasterizes the rows on
screen. Correct under nested ScrollAreas (the clip is the intersection of
all clipping ancestors), and positioning / hit-testing are unaffected.

A normal self-scrolling editor already culls correctly from its own scroll
offset and doesn't need this — leave it **off** (the default). (The window
is computed relative to the editor's own scroll offset as well, so enabling
it on a self-scroller degrades to a correct-but-redundant cull rather than
rendering the wrong rows.)

#### `pub fn scroll_policy(mut self, policy: ScrollPolicy) -> Self`

Set the same scroll-bar visibility policy on both axes.

#### `pub fn follow_caret_in_page(self, follow: bool) -> Self`

Whether moving the caret also scrolls any *enclosing* scroll area to
keep the caret on screen — the standard editor "caret stays visible as
you type / navigate" behaviour. **On by default.**

It fires only on a caret *move*, never on a plain wheel / scrollbar
scroll, so the reader can still scroll freely away from the caret and the
view holds until the caret next moves. This is what makes an editor that
**grows** to its content with its own scroll suppressed (a flowing page
inside an outer `ScrollArea`) track the caret at all — there the editor's
internal caret-visibility is a no-op, so the enclosing-page follow is the
only mechanism that reveals the caret. Pass `false` for the rare layout
where a caret change must never move the surrounding page.

#### `pub fn typewriter(self, anchor: Option<f32>) -> Self`

**Typewriter scrolling**: pin the caret's line at `fraction` of the way
down the enclosing scroll area — `0.0` at the top, `0.5` centred, `1.0`
at the bottom — and let the document scroll under it. `None` (the
default) leaves the ordinary minimal-reveal follow in charge.

Unlike that follow, which only acts once the caret would leave the
viewport, a pin re-asserts on every caret move, so the line being written
holds a constant height on screen. The classic writing-app feature.

Three behaviours come with it, each of them the consensus answer among
the editors that ship this well:

- **The pointer stands the pin down.** A click places the caret without
  scrolling, and that position becomes the new resting place; a
  drag-selection is never interrupted. The next keystroke resumes
  pinning. Editors that re-centre on pointer input instead have open bugs
  about the view fighting the mouse and about drag-selection becoming
  unusable.
- **The rendered row is pinned, not the paragraph.** Under soft wrap a
  long paragraph spans several visual rows; pinning the logical line
  would leave the caret far from the mark.
- **Typing snaps, page jumps glide.** Animating a pin that updates on
  every keystroke is what produces the "screen bouncing" complaint other
  implementations attract.

Requires `follow_caret_in_page` (on by
default). `fraction` is clamped to `0.0..=1.0`.

Near the start of the document the pin gives way to the scroll range —
the caret rides above its line until there is room — and near the end it
would do the same, which is usually not what you want: pair this with
`ScrollArea::scroll_past_end(1.0 - fraction)` so the last line can still
reach the pin.

Takes a plain value, like `typography_defaults`;
to follow a setting live, push changes onto the handle with
`EditorHandle::set_typewriter`.

#### `pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self`

Set the wheel scroll-chaining behavior at the editor's boundary
(default `OverscrollBehavior::Chain`). With `Chain`, a wheel event the
editor can no longer absorb (already at the top/bottom, or content that
fits so there is nothing to scroll) is declined so it bubbles to an
ancestor scrollable — an editor embedded in a scrolling form/page lets
the page scroll once the editor reaches its edge.
`OverscrollBehavior::Contain` keeps the event at the editor instead.
Mirrors the identical knob on `ScrollArea` / `ListView` / `TableView` /
`GridView`.

#### `pub fn min_lines(mut self, n: u32) -> Self`

Set a minimum height (in lines of text) for the editor's
**intrinsic** size.

Setting either `min_lines` or `max_lines`
switches the editor from greedy sizing (consume the
proposal) to intrinsic sizing: `size_that_fits` returns
`clamp(content_height, min_lines × line_height, max_lines × line_height)`
for the dimension the parent leaves unspecified. A parent
like `VStack` proposes unbounded height to non-Expand
children, so the editor lands at its intrinsic height —
exactly the messenger-composer / chat-input pattern.

A parent that *forces* the height (e.g. `FixedSize`) wins
regardless. This is intentional and matches Bastyde's
general layout discipline: parents always have the final
say on the dimensions they pin.

`min_lines` measures the *visible text area*, not the outer
widget — `min_lines(1)` reports a height equal to one line
of text at the typesetter's default font + size, even
before the document has any content.

#### `pub fn max_lines(mut self, n: u32) -> Self`

Set a maximum height (in lines of text) for the editor's
intrinsic size. Past this cap the vertical scroll bar
absorbs further content growth.

See `min_lines` for the intrinsic-mode
switch and the parent-proposal interaction. `max_lines`
measures the visible text area, not the outer widget.

#### `pub fn follow_text_scale(self, follow: bool) -> Self`

Whether this editor's text grows with the global accessibility text
scale (`ctx.text_scale`). Defaults to `true` — like every other text
surface, the editor magnifies when the user raises the app-wide text
size. Pass `false` for an editor whose font sizes are **document
content** (a WYSIWYG / print-layout editor) that must stay at its true
point size regardless of the reader's UI accessibility setting.

Composed with `font_size_scale`:  
`engine.font_scale = (follow ? text_scale : 1.0) × font_size_scale`.

#### `pub fn font_size_scale(self, scale: f32) -> Self`

Per-editor logical font-size multiplier (`1.0` = 100 %). Applied
*before* shaping (same channel as accessibility text scale), so text
grows, re-wraps, and stays sharp — the knob for a "Text size"
preference. Composed as
`(follow_text_scale ? ctx.text_scale : 1.0) × font_size_scale`.
Clamped to `[0.1, 10.0]`. Use `set_font_size_scale`
after mount.

#### `pub fn context_menu( mut self, factory: impl Fn( bastyde_canvas::Point, &mut bastyde_core::widget::EventContext, ) -> Option<Box<dyn bastyde_core::widget::Widget>> + 'static, ) -> Self`

Replace the built-in right-click context menu with a
user-provided factory. Same shape as the framework's
closure receives the click position (widget-local) and a full
`EventContext`, and returns
`Some(menu_widget)` to mount or `None` to decline (falling
through to the next ancestor with a factory).

Taking this branch disables the default menu unconditionally.
The framework's
`show_context_menu_for` handles
the overlay lifecycle (open at pointer, dismiss on
click-outside / Escape, focus-restore on dismiss), so the
factory only needs to build the menu content.

This is an **inherent method**: it shadows the blanket
`WidgetBuilder::context_menu`
trait method so the user can chain it directly on the editor.
Internally, the factory is installed on the editor's arena
node via the same `HandlerSet::context_menu` plumbing.

#### `pub fn default_context_menu(mut self, enabled: bool) -> Self`

Enable (default) or disable the widget's built-in right-click
context menu (Cut / Copy / Paste / Paste Unformatted / Select
All). When disabled, right-click bubbles past the widget
unhandled and
`context_target_at` stays
available for applications that render their own menu.

Note: if a user factory is installed via
`context_menu`, that factory wins
regardless of this flag — this setter only governs the
*default* menu.

#### `pub fn font_registrar(self, registrar: &dyn FontRegistrar) -> Self`

Install a custom font registrar for the fallback private
engine. Only has effect when the editor is built outside a
windowed bastyde-app — once `build()` sees a `SharedTypesetter`
in `app_state`, the private engine is replaced with one that
shares the app's typesetter and this registrar is ignored.

#### `pub fn on_change(self, f: impl Fn() + 'static) -> Self`

Install a callback fired once per batch of genuine **user content
edits** (typing, paste, cut, delete) — and *not* on a programmatic
`set_djot` / `set_markdown` / `set_html` load or a document reset, and
*not* while an IME composition (CJK/Kana candidate preview, dead-key
accent) is still in progress — only the settled result of a commit
fires it. The callback runs on the UI thread during the editor's frame
drain, so it may touch `Signal`s directly — e.g. flip a "dirty" flag or
kick a debounced autosave. Replaces any prior change callback on this
editor.

For a reactive change *token* (which also bumps on loads/format-only
changes, and on intermediate IME composition steps), observe
`document_version` instead.

#### `pub fn document_version(&self) -> Signal<u64>`

Reactive counter that bumps on every document change (content edits,
format changes, load events). Starts at `0`. Use as a change token to
invalidate external caches.

#### `pub fn cursor_position(&self) -> usize`

Current cursor position in the document, in character units.
Exposed for tests and for applications that need to mirror the
caret position externally (status bar, outline panel, etc.).

#### `pub fn cursor_anchor(&self) -> usize`

Current selection anchor (equal to `cursor_position` when there
is no selection).

#### `pub fn is_composing(&self) -> bool`

`true` while an IME composition (CJK/Kana candidate preview, dead-key
accent) is actively in progress — i.e. `on_change`
is currently suppressed for this editor. Exposed so a caller doing its
own while-typing scanning (e.g. an autocorrect feature) can gate its
own trigger logic the same way, as defense-in-depth alongside
`on_change`'s own gate.

#### `pub fn cursor_position_signal(&self) -> Signal<usize>`

Reactive cursor position signal. Observers fire whenever the
cursor moves (arrow keys, click, Home/End, …). Useful for
status bars and tests.

#### `pub fn cursor_anchor_signal(&self) -> Signal<usize>`

Reactive selection anchor signal.

#### `pub fn has_selection(&self) -> Signal<bool>`

Reactive signal — `true` whenever the editor has a non-empty
selection. Updates synchronously after every cursor mutation.

#### `pub fn can_undo(&self) -> Signal<bool>`

Reactive undo-availability signal, suitable for toolbar button
enable-state. Updated through the frame loop's debounce drain
so toolbars don't flicker during rapid editing.

#### `pub fn can_redo(&self) -> Signal<bool>`

Reactive redo-availability signal.

#### `pub fn caret_char_format(&self) -> TextFormat`

Read the current character format at the widget's caret —
the right source for toolbars that mirror bold/italic/underline
state.

When a selection is active, the format is read from
`selection_start()`
rather than `position()`.
Rationale (matches godot-rich-text's `query_char_format`):
`position()` lands at the **end** of the selection and may fall
on a run with different formatting (or past the last character,
on an empty virtual element) — a toolbar observing that value
would flicker or lie. `selection_start()` always points at the
first character of the selected range, so the reading is
stable and matches what a user would expect from "tell me the
format of what I have selected."

#### `pub fn scroll_y(&self) -> Signal<f32>`

Reactive vertical scroll offset in logical pixels. Bind to a
scroll bar or observe for scroll-position persistence.

#### `pub fn scroll_x(&self) -> Signal<f32>`

Reactive horizontal scroll offset in logical pixels. Non-zero
only when `wrap_mode` is `WrapMode::None`.

#### `pub fn context_target_at(&self, point: Point) -> Option<hit_test::ContextTarget>`

Classify what is under `point` in the widget's local coordinates
(origin at the widget's top-left, scroll offset handled
internally by the typesetter), for applications building an
external context menu. Returns `None` if the point does not
land on any hit region.

#### `pub fn selected_text(&self) -> String`

Currently selected text, or an empty string if nothing is selected.

#### `pub fn select_all(&self)`

Select the entire document programmatically. Equivalent to
the final step of the Ctrl+A ladder; resets the ladder state
so a subsequent Ctrl+A starts fresh at level 1.

#### `pub fn deselect(&self)`

Clear any current selection.

#### `pub fn insert_text(&self, text: &str)`

Insert plain text at the widget's caret. Replaces any selection.

#### `pub fn insert_html(&self, html: &str)`

Insert a fragment parsed from HTML at the widget's caret.
Replaces any selection. Uses text-document's
`TextCursor::insert_html`,
which parses the HTML into a `DocumentFragment` and inserts it.

#### `pub fn insert_djot(&self, djot: &str)`

Insert a fragment parsed from djot at the widget's caret.
Replaces any selection. Uses text-document's
`TextCursor::insert_djot`,
which parses the djot into a `DocumentFragment` and inserts it — so
unlike `insert_text`, block-level source really
does produce new blocks rather than literal newlines in one paragraph.

#### `pub fn insert_block(&self)`

Split the current block at the widget's caret, as pressing Enter does.

#### `pub fn insert_image(&self, name: &str, width: u32, height: u32)`

Insert an inline image by logical resource name. `width` and
`height` are in logical pixels.

#### `pub fn delete_selection(&self)`

Delete the current selection. No-op when nothing is selected.

#### `pub fn select_word(&self)`

Select the word under the widget's caret.

#### `pub fn select_line(&self)`

Select the paragraph / block under the widget's caret.

#### `pub fn set_caret_position(&self, position: usize)`

Move the caret to an absolute character position. Collapses any
existing selection (passes `MoveMode::MoveAnchor`). Resets
`CursorAffinity` to `Downstream` — programmatic placement
can't know whether the caller wanted the upstream side of a
wrap boundary, so we default to the same placement that
existed before affinity was introduced.

#### `pub fn focused_signal(&self) -> Signal<bool>`

Reactive signal — `true` while **this** editor holds keyboard focus.

A per-editor find banner (Ctrl+F) targets whichever editor is focused, and the split
view has two of them; `focused_side` only names the Primary/Secondary *pane*, not which
editor. This is the per-editor answer, mirroring `has_selection`.

#### `pub fn select_range(&self, start: usize, end: usize)`

Select the character range ``start, end)`, **without** collapsing — unlike
[`set_caret_position``, which always moves both ends together.

The anchor lands at `start` and the caret (focus) at `end`, so the standard selection
highlight marks the range and a subsequent replace acts on it. Used to select a search
match. (The non-collapsing two-call shape is the same one the AccessKit
`SetTextSelection` handler uses.)

#### `pub fn reveal_range( &self, ctx: &mut bastyde_core::widget::EventContext, start: usize, end: usize, )`

Scroll the character range ``start, end)` into view within the enclosing scroll area.

Reveals an **arbitrary** offset range — the current search match — rather than the live
caret the follow-into-view path tracks, and works whether or not the editor is focused.
A no-op until the editor has a full layout.

Under [`typewriter`` scrolling the range is *pinned* to
the anchor rather than merely revealed, so a search walks matches to the
same height the caret writes at instead of leaving them wherever they
happened to fall. Because a search jump is a deliberate, screen-sized
move, it glides.

#### `pub fn set_bold(&self, enabled: bool)`

Apply **bold** to the current selection (or set the typing bold
state when no selection is active). Pairs with
`is_bold` and `toggle_bold`.

#### `pub fn set_italic(&self, enabled: bool)`

Apply *italic* to the current selection.

#### `pub fn set_underline(&self, enabled: bool)`

Apply underline to the current selection.

#### `pub fn set_strikethrough(&self, enabled: bool)`

Apply strikethrough to the current selection.

#### `pub fn set_font_size(&self, size: u32)`

Set the font size (in points) for the current selection.

#### `pub fn set_font_family(&self, family: impl Into<String>)`

Set the font family for the current selection. `family` must be
a name resolvable by the shared typesetter's font registrar.

#### `pub fn toggle_bold(&self)`

Toggle bold on the current selection, reading the current state
via `caret_char_format`. Matches the
Ctrl+B keyboard shortcut's behaviour.

#### `pub fn toggle_italic(&self)`

Toggle italic; see `toggle_bold`.

#### `pub fn toggle_underline(&self)`

Toggle underline; see `toggle_bold`.

#### `pub fn toggle_strikethrough(&self)`

Toggle strikethrough; see `toggle_bold`.

#### `pub fn set_superscript(&self, enabled: bool)`

Raise the selection to superscript, or drop it back to the baseline.

#### `pub fn set_subscript(&self, enabled: bool)`

Lower the selection to subscript, or drop it back to the baseline.

#### `pub fn set_vertical_alignment(&self, alignment: CharVerticalAlignment)`

Set the selection's vertical alignment directly. `Normal` is the
baseline; `Middle` exists in the model but has no toolbar affordance.

#### `pub fn get_vertical_alignment(&self) -> CharVerticalAlignment`

The caret's vertical alignment, `Normal` when unset.

#### `pub fn is_superscript(&self) -> bool`

True while the caret sits in superscript text.

#### `pub fn is_subscript(&self) -> bool`

True while the caret sits in subscript text.

#### `pub fn toggle_superscript(&self)`

Flip superscript on the selection. Turning it on replaces subscript.

#### `pub fn toggle_subscript(&self)`

Flip subscript on the selection. Turning it on replaces superscript.

#### `pub fn apply_block_format(&self, fmt: BlockFormat)`

Set an arbitrary `BlockFormat` on the caret's current block.
The higher-level helpers `set_alignment`
and `set_heading_level` go through
this method. Exposed so apps that need less common fields
(`indent`, `left_margin`, `line_height`, …) don't have to
reach through `TextDocument::cursor()` and lose the widget's
caret continuity.

#### `pub fn apply_text_format(&self, fmt: TextFormat)`

Set an arbitrary `TextFormat` on the current selection.
Public counterpart of the private `apply_char_format` helper,
for apps that need fields beyond the dedicated
`set_bold` / `set_italic` / … setters (e.g. `letter_spacing`,
`foreground_color`).

#### `pub fn set_alignment(&self, alignment: Alignment)`

Set the paragraph alignment for the current block (or the block
containing the selection anchor).

#### `pub fn clear_direction(&self)`

Unset the block's direction, handing the paragraph back to
automatic detection.

Not the same as setting left-to-right. An explicit direction
*pins* the paragraph and overrides the bidi algorithm, so
"clearing" a direction by writing `LeftToRight` would force
Arabic and Hebrew prose to lay out backwards. Only an unset
direction lets the text speak for itself.

#### `pub fn set_direction(&self, direction: TextDirection)`

Set the base reading direction of the current block.

This is the *paragraph* direction, not a character property: it
decides which edge unaligned text sits against and, more
importantly, overrides the bidi algorithm's first-strong-character
guess — which misreads an Arabic paragraph opening with a Latin
acronym as left-to-right.

#### `pub fn set_heading_level(&self, level: u8)`

Set the heading level of the current block. `0` = plain
paragraph; `1..=6` follow the HTML `<h1>..<h6>` convention.

#### `pub fn insert_list(&self, ordered: bool)`

Create a list at the current selection. `ordered = true` uses
decimal numbering; `ordered = false` uses a bullet disc.
Choose a specific style with `create_list`.

#### `pub fn create_list(&self, style: ListStyle)`

Create a list with an explicit `ListStyle`. Exposed for
applications that want e.g. lowercase Roman numerals or circle
bullets.

#### `pub fn indent(&self)`

Increase the nesting depth of the caret's current list item by
one. No-op when the caret is not inside a list. Equivalent to
pressing Tab while the caret is on a list item — same behaviour,
same `nest_current_list_item` codepath, exposed for toolbar
buttons that do not want to synthesise key events.

#### `pub fn outdent(&self)`

Decrease the nesting depth of the caret's current list item by
one. No-op at depth 0 (use `Backspace` at block-start to exit
the list entirely). Toolbar counterpart of Shift+Tab.

#### `pub fn remove_from_list(&self)`

Take the caret's block out of its list entirely, leaving a plain
paragraph. No-op when the caret is not inside a list.

`outdent` deliberately stops at depth 0 — Shift+Tab
should not silently destroy the list — so a toolbar that offers
"remove list formatting" needs this instead. Backspace at block-start
reaches the same codepath from the keyboard.

#### `pub fn is_in_blockquote(&self) -> bool`

True iff the caret currently sits inside a blockquote frame at
any nesting depth. Used by the toolbar to drive the toggle
button's pressed state and the context menu's label.

#### `pub fn selection_spans_multiple_frames(&self) -> bool`

True iff the current selection spans more than one frame. The
"Toggle blockquote" affordance is disabled in this case because
wrapping a cross-frame range has no well-defined semantics
(different blocks already belong to different containers).

#### `pub fn toggle_blockquote(&self)`

Wrap the current block (or selection) in a blockquote, or
unwrap the innermost enclosing blockquote if already inside one.
No-op (returns silently) when the selection spans multiple
frames.

#### `pub fn increase_blockquote_depth(&self)`

Equivalent to pressing Tab inside a blockquote — wraps the
current block in a deeper nested quote. No-op when the caret is
not in a quote.

#### `pub fn decrease_blockquote_depth(&self)`

Equivalent to pressing Shift+Tab inside a blockquote — pops one
nesting level. At depth 1 unwraps the block to a plain
paragraph. No-op when the caret is not in a quote.

#### `pub fn insert_table(&self, rows: usize, columns: usize)`

Insert a fresh `rows × columns` table at the caret. Any
existing selection is replaced.

#### `pub fn remove_current_table(&self)`

Remove the table containing the caret (if any). No-op when the
caret is not inside a table.

#### `pub fn insert_row_above(&self)`

Insert a row above the caret's current table row. No-op when
outside a table.

#### `pub fn insert_row_below(&self)`

Insert a row below the caret's current table row.

#### `pub fn insert_column_before(&self)`

Insert a column before the caret's current table column.

#### `pub fn insert_column_after(&self)`

Insert a column after the caret's current table column.

#### `pub fn remove_current_row(&self)`

Remove the caret's current table row.

#### `pub fn remove_current_column(&self)`

Remove the caret's current table column.

#### `pub fn is_in_table(&self) -> bool`

Whether the caret is currently inside a table cell.

#### `pub fn is_bold(&self) -> bool`

Whether the current selection / typing position is bold.

#### `pub fn is_italic(&self) -> bool`

Whether italic.

#### `pub fn is_underline(&self) -> bool`

Whether underline.

#### `pub fn is_strikethrough(&self) -> bool`

Whether strikethrough.

#### `pub fn get_heading_level(&self) -> u8`

Current heading level (0 = plain paragraph). Reads the caret's
current block format.

#### `pub fn get_alignment(&self) -> Alignment`

Current block alignment.

#### `pub fn get_direction(&self) -> Option<TextDirection>`

The block's explicitly-set reading direction, if it has one.
`None` means the bidi algorithm decides from the text.

#### `pub fn undo(&self)`

Undo the most recent edit. Mirrors Ctrl+Z. No-op when the undo
stack is empty.

#### `pub fn redo(&self)`

Redo the most recently undone edit. Mirrors Ctrl+Y /
Ctrl+Shift+Z. No-op when the redo stack is empty.

#### `pub fn begin_edit_block(&self)`

Begin grouping subsequent edits into a single undo entry.

Must be paired with `end_edit_block`. Prefer
`edit_block`, which pairs them for you.

#### `pub fn end_edit_block(&self)`

Close the group opened by `begin_edit_block`.

#### `pub fn edit_block<R>(&self, edits: impl FnOnce() -> R) -> R`

Run `edits` as one undo entry.

The scoped form of `begin_edit_block` — the
block is closed even if `edits` returns early, which hand-pairing gets
wrong eventually.

#### `pub fn set_default_language(&self, language: &str)`

Set the document-wide default language (ISO 639-1 code, e.g. "en",
"fr", "de"). Blocks that don't set their own language inherit it
for hyphenation. Forces a full re-layout so the change takes effect
on the next frame. No-op-safe if the document rejects the update.

#### `pub fn default_language(&self) -> String`

The document-wide default language (ISO 639-1 code). Defaults to
`"en"` when never set.

#### `pub fn handle(&self) -> EditorHandle`

Cheap clone-able handle for external toolbars / palettes — see
`EditorHandle`. The handle shares the editor's internal
state (same `Rc<RefCell<…>>`), so mutations through the handle
are immediately observable through the editor's reactive
signals (and vice versa).

Use this when the caller needs to invoke editor commands from
`on_activate_fn` / `ctx.effect` closures that outlive the
borrow of `&editor`: `RichTextEditor` itself is move-only
(the optional context-menu factory holds a `Box<dyn Fn>`,
which prevents `Clone`).

#### `pub fn copy(&self, ctx: &bastyde_core::widget::EventContext)`

Copy the current selection to the system clipboard (plain +
HTML payloads). No-op when there is no selection.

All clipboard methods take `&EventContext` because they only
need read access — the clipboard handle is looked up via
`ctx.app_state::<ClipboardHandle>()`. A call site that holds
`&mut EventContext` can pass `&ctx` directly; Rust reborrows
automatically.

#### `pub fn cut(&self, ctx: &bastyde_core::widget::EventContext)`

Cut the current selection: copy first, then remove.

#### `pub fn paste(&self, ctx: &bastyde_core::widget::EventContext)`

Paste from the system clipboard. Prefers an in-process fragment
over HTML over plain text — see
`rich_text/clipboard.rs`.

#### `pub fn paste_unformatted(&self, ctx: &bastyde_core::widget::EventContext)`

Paste plain text only, stripping any rich payload.

#### `pub fn can_paste(&self, ctx: &bastyde_core::widget::EventContext) -> bool`

Whether a paste would insert anything — `true` iff the system
clipboard carries text **or** an HTML payload (the shapes
`paste` can consume; an HTML-only clipboard pastes
fine, so probing plain text alone would under-report).

Clipboard contents are not reactively observable, so this is a
**point-in-time query** rather than a `Signal`: pass the active
`EventContext`. It probes
the clipboard (an X11 HTML probe can round-trip to the selection
owner), so a menu / toolbar builder should re-query when the menu
opens, not per frame. Returns `false` when no clipboard backend
is installed (headless or feature-off builds) — the same
"silently no-op" degradation the paste path itself uses.

#### `pub fn set_font_size_scale(&self, scale: f32)`

Set the per-editor logical font-size multiplier (`1.0` = 100 %).
Composed with accessibility text scale at paint; forces relayout.
See `font_size_scale`.

#### `pub fn get_font_size_scale(&self) -> f32`

Current per-editor font-size scale (`1.0` = 100 %).

#### `pub fn set_typography_defaults(&self, defaults: EditorTypographyDefaults)`

Set the non-destructive default typography at runtime. Re-lays out and
schedules a repaint. Never mutates the document.

#### `pub fn get_typography_defaults(&self) -> EditorTypographyDefaults`

Current default typography (see `typography_defaults`).

#### `pub fn set_typewriter(&self, anchor: Option<f32>)`

Set the typewriter-scrolling anchor at runtime — see
`typewriter`. `None` turns pinning off.

Takes effect on the next caret move rather than scrolling immediately: a
pin is a follow rule, and re-anchoring the page the instant a setting
changes would jump the view under a reader who is not even typing.

#### `pub fn get_typewriter(&self) -> Option<f32>`

Current typewriter anchor (see `typewriter`).

#### `pub fn set_caret_highlight(&self, highlight: Option<caret_highlight::CaretHighlight>)`

Draw an ambient band behind the sentence — or paragraph — the caret is in.

`None` (the default) draws nothing and registers no session on the document. The band
shows only while **this** editor has focus, so two panes over one document never band
twice, and it disappears when focus leaves the editor entirely.

The band is registered below every other highlight layer, so a find match or a spell
squiggle always paints over it. Give it a paint-only `format` — a background colour —
or it will force a reshape on every caret move.

#### `pub fn get_caret_highlight(&self) -> Option<caret_highlight::CaretHighlight>`

What this editor's caret band is currently configured to draw.

#### `pub fn caret_window_rect(&self) -> Option<bastyde_canvas::Rect>`

The caret's rectangle in **absolute window (tree) coordinates**, or
`None` when the editor is unfocused or has not been laid out yet.

The same rect the OS-IME reporting and the caret follow use, exposed for
hosts that need to position something against the caret (and for tests
that need to assert where a pin actually put it).

#### `pub fn format_version(&self) -> Signal<u64>`

Signal that bumps on every format-only document event (bold /
italic / heading / alignment / list style changes …).
Distinct from `document_version`,
which also bumps on content changes. Useful for toolbar
observers that want to refresh button state on format changes
without flickering during plain typing.

#### `pub fn document_loaded_count(&self) -> Signal<u64>`

Signal that bumps once per document-loaded event (fires when
an async `set_html` / `set_markdown` import completes). Starts
at 0; observers see a new value each time a long import
finishes.

#### `pub fn on_link_activated( self, handler: impl Fn(&str, &mut bastyde_core::widget::EventContext) + 'static, ) -> Self`

Install a callback fired when the user Primary-clicks a link
(an element with an anchor `href`). The callback receives the
href string and the active `EventContext`.

The callback replaces any prior link-click callback on this
builder chain. To stop observing, reconstruct the editor
without the setter.

#### `pub fn on_image_activated( self, handler: impl Fn(&str, &mut bastyde_core::widget::EventContext) + 'static, ) -> Self`

Install a callback fired when the user Primary-clicks an inline
image. The callback receives the image's resource name and the
active `EventContext`.

## `pub struct EditorHandle`

A clone-able, `'static` handle to a `RichTextEditor`'s shared
state.

Use this when a toolbar, palette, command panel, or other external
widget needs to invoke editor commands from `on_activate_fn` /
`ctx.effect` closures that outlive the borrow of `&editor`.
`RichTextEditor` itself is move-only (the optional
`custom_context_menu` factory holds a `Box<dyn Fn>`, which prevents
`Clone`), so a closure cannot just capture `editor.clone()`.
Obtain a handle via `RichTextEditor::handle()` and clone it into
each closure that needs to issue commands.

`EditorHandle` mirrors the toolbar-relevant subset of the editor's
public API:

* Inline character formatting — `set_bold` /
  `toggle_bold` / `is_bold`
  and the italic / underline / strikethrough variants.
* Block-level formatting — `set_alignment`,
  `set_heading_level`,
  `apply_block_format`,
  `insert_list`,
  `indent` / `outdent`.
* Tables — `insert_table` and the per-row /
  per-column / remove operations, plus `is_in_table`
  for contextual UI enable state.
* History — `undo` / `redo`.
* Clipboard — `copy` / `cut` /
  `paste` /
  `paste_unformatted`, plus
  `can_paste` for Paste enable-state — so a
  context-menu factory (which can only capture a handle, never the
  editor that owns it) can rebuild Cut / Copy / Paste /
  Paste-Unformatted.
* Selection — `select_all` /
  `delete_selection`.
* Reactive signal accessors —
  `format_version`,
  `cursor_position_signal`,
  `cursor_anchor_signal`,
  `has_selection`,
  `can_undo` / `can_redo` — so
  callers that hold only an `EditorHandle` can derive bound signals
  without keeping a separate `RichTextEditor` reference.

Cloning is cheap (an `Rc` clone). All clones share the same
underlying state — mutations through any clone, through other
clones, or through the originating `RichTextEditor` are all
immediately observable through the same signals.

```rust
pub struct EditorHandle { /* fields */ }
```

### Methods

#### `pub fn to_djot(&self) -> String`

This editor's content as Djot.

The counterpart to `insert_djot`: a toolbar or command that can
write into an editor it did not build should be able to read it back the same way.
Without this the only route to the text is the host's own document bookkeeping,
which knows about the editors it *mounted* and not about the ones a list or a card
grid created — so a command ends up working on some surfaces and silently doing
nothing on others.

Empty string on a serialisation error, matching `TextDocument::to_djot`'s own
callers: a command reading an editor has no better answer than "nothing there", and
propagating a `Result` here would push that decision onto every call site.

#### `pub fn is_empty(&self) -> bool`

Whether this editor holds no text at all.

`character_count() == 0`, so a document of one empty paragraph is empty but one
holding only spaces is not — the distinction a caller usually wants is
`to_djot().trim().is_empty()`, and this is the cheap O(1) pre-check.

#### `pub fn focused_signal(&self) -> Signal<bool>`

Reactive signal — `true` while **this** editor holds keyboard focus.
See `RichTextEditor::focused_signal`.

#### `pub fn select_range(&self, start: usize, end: usize)`

Select the character range `[start, end)` without collapsing (anchor at
`start`, caret at `end`). See `RichTextEditor::select_range`.

#### `pub fn replace_range(&self, start: usize, end: usize, text: &str)`

Replace the character range ``start, end)` with `text`, leaving the caret
after the inserted text.

The counterpart to [`select_range`` for callers that
must *rewrite* a span rather than merely reveal it — a spell-check
correction picked from a context menu, an autocorrect, a
replace-this-occurrence action. It goes through the widget's **internal**
cursor, so the edit behaves exactly like typed text: it lands on the
editor's undo stack as one entry (the replacement is a single
insert-over-selection), fires the document's change notifications, and
leaves the caret where the user would expect it.

Offsets are **character** positions, the same space
`cursor_position` and `select_range` use. The
inserted text inherits the character format at `start`, so correcting a
word inside italic prose stays italic.

Reaching through `TextDocument::cursor`
instead would mutate the document behind the widget's back, leaving the
caret decoupled from the edit — use this.

#### `pub fn insert_text(&self, text: &str)`

Insert plain text at the caret, replacing any selection. The
`EditorHandle` counterpart of
`RichTextEditor::insert_text`, for callers
that hold only a handle — a toolbar button or a global menu command.

#### `pub fn insert_djot(&self, djot: &str)`

Insert a fragment parsed from djot at the caret, replacing any selection.

Unlike `insert_text`, which drops its bytes into the
current block verbatim (a `\n` becomes literal content, not a new
paragraph), this parses block-level djot into a `DocumentFragment`, so
inserting a standalone paragraph really does create one.

#### `pub fn insert_block(&self)`

Split the current block at the caret, as pressing Enter does.

#### `pub fn insert_paragraph(&self, text: &str) -> bool`

Insert `text` as a **paragraph of its own** at the caret: split here, fill
the new block, split again, so whatever followed the caret continues in a
third block.

Deliberately one call rather than three. Composing
`insert_block` + `insert_text` + `insert_block` from outside re-enters the
widget three times, and an application that rebuilds its editor in
response to the first change notification is left driving a handle that
no longer points at the mounted widget — the split lands and the text
silently does not. Doing the whole edit under a single borrow, with one
signal sync at the end, makes it atomic from the caller's side.
Returns `false` if any step failed, leaving the document as far as it
got. Steps are **not** attempted after a failure: filling and re-splitting
on top of a split that did not happen produces a mangled paragraph rather
than a partial one, and the caller has no way to tell.

#### `pub fn selection(&self) -> (usize, usize)`

The live selection as `(anchor, position)`, unordered — `anchor` is where the
selection started, `position` is where the caret is, so a backwards drag
reports `anchor > position`. Equal values mean no selection.

Both ends are read under a **single** borrow, so the pair cannot tear. That is
the reason to prefer this over pairing `cursor_position`
with `cursor_anchor_signal`: the former is a live
read of the cursor while the latter is a mirror refreshed on sync, so combining
them mixes two different moments in time and can invent — or miss — a selection
if the mirror lags. A caller deciding *"is there a selection, and over what"*
wants one consistent answer.

#### `pub fn range_rect(&self, start: usize, end: usize) -> Option<Rect>`

The **window-space** rectangle enclosing the character range ``start, end)`.

The inverse of [`offset_at_point``: that maps a point
to an offset, this maps offsets back to a point. It is what a decoration
drawn *outside* the editor — a margin annotation, a connector leader, a
bracket spanning a paragraph — needs in order to line itself up with the
text it refers to.

Coordinates match what the arena stores (`viewport_origin` + engine-local −
scroll), so the result can be compared with any other widget's bounds
directly, and it tracks scrolling for free.

`None` before the first full layout. Focus is **not** required — a margin
annotation must stay aligned whether or not the writer is typing.

#### `pub fn offset_rect(&self, offset: usize) -> Option<Rect>`

The **window-space** caret rectangle at one offset — a zero-width
`range_rect`, and the anchor point for a marker drawn at
one end of a span (the triangle at a comment's tail).

#### `pub fn offset_at_point(&self, window_point: Point) -> Option<usize>`

Hit-test a point — **in window coordinates**, as a
`context_menu` factory receives it — to a
document character offset. `None` when the point resolves to no text
(past the last glyph on an empty line, outside the body, etc.).

Lets a custom context-menu factory resolve "the word under the pointer"
from the right-click position, since a bare right-click does not move the
caret on its own.

#### `pub fn reposition_caret_for_context_menu(&self, window_point: Point)`

Reposition the caret to a right-click point (**window coordinates**)
unless the click lands inside the current selection (then the selection
is preserved). Call this at the top of a custom
`context_menu` factory so the menu's Paste
— and any caret-relative action — operates where the user clicked, exactly
as the built-in menu and the single-line field do.

#### `pub fn reveal_range( &self, ctx: &mut bastyde_core::widget::EventContext, start: usize, end: usize, )`

Scroll the character range `[start, end)` into view. A no-op until the
editor has a full layout. See `RichTextEditor::reveal_range`.

#### `pub fn focus(&self, ctx: &mut bastyde_core::widget::EventContext)`

Move keyboard focus onto the editor. Lets a control built *above* the
editor — a find banner returning focus to the prose on Escape — put the
caret back where the user expects. A no-op until the editor has built at
least once (its wrapper id is stashed then).

#### `pub fn caret_char_format(&self) -> TextFormat`

Read the current character format at the caret. When a selection
is active, reads from `selection_start()` rather than
`position()` so toolbar bistate stays stable across selection
extension (same rule as
`RichTextEditor::caret_char_format`).

#### `pub fn set_bold(&self, enabled: bool)`

Apply **bold** to the current selection.

#### `pub fn set_italic(&self, enabled: bool)`

Apply *italic* to the current selection.

#### `pub fn set_underline(&self, enabled: bool)`

Apply underline to the current selection.

#### `pub fn set_strikethrough(&self, enabled: bool)`

Apply strikethrough to the current selection.

#### `pub fn set_font_family(&self, family: impl Into<String>)`

Set the font family for the current selection (a character-format
change applied over the selected range). Like the other char-format
setters (`set_bold`, …), this is a **no-op when there is no
selection** — the document model has no typing/pending format, so a
bare caret has no range to format. `family` must be a name resolvable
by the shared typesetter's font registrar — e.g. a value chosen from
a `FontPicker`.

#### `pub fn set_font_size(&self, size: u32)`

Set the font size (in points) for the current selection.

#### `pub fn set_typography_defaults(&self, defaults: EditorTypographyDefaults)`

Set the non-destructive default typography (font family / line height /
first-line indent) filled onto runs and blocks with no explicit
override. Unlike `set_font_family` /
`set_font_size` — which mutate the selected text —
this is a display-time default: it never touches the document, undo
stack, or `modified` flag. Schedules a relayout + repaint.

#### `pub fn get_typography_defaults(&self) -> EditorTypographyDefaults`

Current default typography.

#### `pub fn set_font_size_scale(&self, scale: f32)`

Set the per-editor logical font-size multiplier. See
`RichTextEditor::set_font_size_scale`.

#### `pub fn get_font_size_scale(&self) -> f32`

Current per-editor font-size scale (`1.0` = 100 %).

#### `pub fn set_typewriter(&self, anchor: Option<f32>)`

Set the typewriter-scrolling anchor — the `EditorHandle` counterpart of
`RichTextEditor::set_typewriter`. `None` turns pinning off.

This is the door a host uses to keep the pin following a live setting,
the same way `set_typography_defaults`
keeps typography following one.

#### `pub fn get_typewriter(&self) -> Option<f32>`

Current typewriter anchor.

#### `pub fn set_caret_highlight(&self, highlight: Option<caret_highlight::CaretHighlight>)`

Draw an ambient band behind the caret's sentence or paragraph — the `EditorHandle`
counterpart of `RichTextEditor::set_caret_highlight`, for hosts that re-push it from a
settings or theme effect after the editor is mounted.

#### `pub fn get_caret_highlight(&self) -> Option<caret_highlight::CaretHighlight>`

What this editor's caret band is currently configured to draw.

#### `pub fn caret_window_rect(&self) -> Option<bastyde_canvas::Rect>`

The caret's rectangle in **absolute window (tree) coordinates** — the
`EditorHandle` counterpart of `RichTextEditor::caret_window_rect`.
`None` when unfocused or not yet laid out.

#### `pub fn apply_text_format(&self, fmt: TextFormat)`

Apply an arbitrary `TextFormat` (escape hatch for fields not
covered by the dedicated setters: `letter_spacing`,
`foreground_color`, …).

#### `pub fn toggle_bold(&self)`

Toggle bold on the current selection.

#### `pub fn toggle_italic(&self)`

Toggle italic on the current selection.

#### `pub fn toggle_underline(&self)`

Toggle underline on the current selection.

#### `pub fn toggle_strikethrough(&self)`

Toggle strikethrough on the current selection.

#### `pub fn is_bold(&self) -> bool`

Whether the selection / typing position is bold.

#### `pub fn is_italic(&self) -> bool`

Whether italic.

#### `pub fn is_underline(&self) -> bool`

Whether underline.

#### `pub fn is_strikethrough(&self) -> bool`

Whether strikethrough.

#### `pub fn set_superscript(&self, enabled: bool)`

Raise the selection to superscript, or return it to the baseline.

#### `pub fn set_subscript(&self, enabled: bool)`

Lower the selection to subscript, or return it to the baseline.

#### `pub fn set_vertical_alignment(&self, alignment: CharVerticalAlignment)`

Set the selection's vertical alignment directly.

#### `pub fn get_vertical_alignment(&self) -> CharVerticalAlignment`

The caret's vertical alignment, `Normal` when unset.

#### `pub fn is_superscript(&self) -> bool`

True while the caret sits in superscript text.

#### `pub fn is_subscript(&self) -> bool`

True while the caret sits in subscript text.

#### `pub fn toggle_superscript(&self)`

Flip superscript on the selection. Turning it on replaces subscript.

#### `pub fn toggle_subscript(&self)`

Flip subscript on the selection. Turning it on replaces superscript.

#### `pub fn apply_block_format(&self, fmt: BlockFormat)`

Apply an arbitrary `BlockFormat` to the caret's block.

#### `pub fn set_alignment(&self, alignment: Alignment)`

Set paragraph alignment for the caret's block.

#### `pub fn clear_direction(&self)`

Unset the block's direction, handing the paragraph back to
automatic detection.

Not the same as setting left-to-right. An explicit direction
*pins* the paragraph and overrides the bidi algorithm, so
"clearing" a direction by writing `LeftToRight` would force
Arabic and Hebrew prose to lay out backwards. Only an unset
direction lets the text speak for itself.

#### `pub fn set_direction(&self, direction: TextDirection)`

Set the base reading direction of the caret's block. See
`RichTextEditor::set_direction`.

#### `pub fn set_heading_level(&self, level: u8)`

Set heading level for the caret's block. `0` = plain paragraph,
`1..=6` follow the HTML `<h1>..<h6>` convention.

#### `pub fn get_alignment(&self) -> Alignment`

Current block alignment.

#### `pub fn get_direction(&self) -> Option<TextDirection>`

The block's explicitly-set reading direction, if it has one.

`None` means the writer never chose — the bidi algorithm decides
from the text. That is a genuinely different state from an
explicit left-to-right, so it is reported rather than defaulted:
a toggle needs to show "auto" as its own setting.

#### `pub fn get_heading_level(&self) -> u8`

Current heading level (0 = plain paragraph).

#### `pub fn insert_list(&self, ordered: bool)`

Wrap the caret's block in a list. `ordered = true` uses decimal
numbering, `false` uses bullet discs.

#### `pub fn create_list(&self, style: ListStyle)`

Wrap the caret's block in a list with an explicit
`ListStyle`.

#### `pub fn indent(&self)`

Indent the caret's current list item by one nesting level.
No-op when the caret is not inside a list. Equivalent to Tab.

#### `pub fn outdent(&self)`

Outdent the caret's current list item by one nesting level.
No-op at depth 0. Equivalent to Shift+Tab.

#### `pub fn remove_from_list(&self)`

Take the caret's block out of its list entirely, leaving a plain
paragraph. No-op when the caret is not inside a list.

See `RichTextEditor::remove_from_list` for why this is separate from
`outdent`, which stops at depth 0 by design.

#### `pub fn is_in_blockquote(&self) -> bool`

True iff the caret currently sits inside a blockquote frame at
any nesting depth.

#### `pub fn selection_spans_multiple_frames(&self) -> bool`

True iff the selection spans more than one frame — the
"Toggle blockquote" affordance should be disabled in this case.

#### `pub fn toggle_blockquote(&self)`

Wrap the current block/selection in a blockquote, or unwrap the
innermost enclosing blockquote if already inside one. Toolbar
counterpart for a Ctrl+Shift+Q-style toggle.

#### `pub fn increase_blockquote_depth(&self)`

Wrap the current block in a deeper nested quote. Equivalent to
Tab inside a blockquote.

#### `pub fn decrease_blockquote_depth(&self)`

Pop the caret out of one blockquote nesting level. Equivalent to
Shift+Tab inside a blockquote.

#### `pub fn insert_table(&self, rows: usize, columns: usize)`

Insert a fresh `rows × columns` table at the caret.

#### `pub fn remove_current_table(&self)`

Remove the table containing the caret. No-op outside a table.

#### `pub fn insert_row_above(&self)`

Insert a row above the caret's current table row.

#### `pub fn insert_row_below(&self)`

Insert a row below the caret's current table row.

#### `pub fn insert_column_before(&self)`

Insert a column before the caret's current table column.

#### `pub fn insert_column_after(&self)`

Insert a column after the caret's current table column.

#### `pub fn remove_current_row(&self)`

Remove the caret's current table row.

#### `pub fn remove_current_column(&self)`

Remove the caret's current table column.

#### `pub fn is_in_table(&self) -> bool`

Whether the caret is currently inside a table cell.

#### `pub fn undo(&self)`

Undo the most recent edit. No-op when the undo stack is empty.

#### `pub fn redo(&self)`

Redo the most recently undone edit. No-op when the redo stack
is empty.

#### `pub fn begin_edit_block(&self)`

Begin grouping subsequent edits into a single undo entry. Pair with
`end_edit_block`, or prefer the scoped
`edit_block`.

#### `pub fn end_edit_block(&self)`

Close the group opened by `begin_edit_block`.

#### `pub fn edit_block<R>(&self, edits: impl FnOnce() -> R) -> R`

Run `edits` as one undo entry — the pairing-safe form.

#### `pub fn copy(&self, ctx: &bastyde_core::widget::EventContext)`

Copy the current selection to the system clipboard (plain + HTML
payloads). No-op when there is no selection. See
`RichTextEditor::copy`.

#### `pub fn cut(&self, ctx: &bastyde_core::widget::EventContext)`

Cut the current selection: copy first, then remove. See
`RichTextEditor::cut`.

#### `pub fn paste(&self, ctx: &bastyde_core::widget::EventContext)`

Paste from the system clipboard. Prefers an in-process fragment
over HTML over plain text. See `RichTextEditor::paste`.

#### `pub fn paste_unformatted(&self, ctx: &bastyde_core::widget::EventContext)`

Paste plain text only, stripping any rich payload. See
`RichTextEditor::paste_unformatted`.

#### `pub fn can_paste(&self, ctx: &bastyde_core::widget::EventContext) -> bool`

Whether a paste would insert anything — `true` iff the system
clipboard carries text **or** an HTML payload. A point-in-time
query (clipboard contents are not reactively observable), taking
the active `EventContext`.
Use it to drive a context-menu / toolbar Paste enable-state,
re-querying on menu-open. Mirrors `RichTextEditor::can_paste`.

#### `pub fn select_all(&self)`

Select the entire document programmatically. Resets the Ctrl+A
ladder so a subsequent Ctrl+A starts fresh at level 1. Mirrors
`RichTextEditor::select_all`.

#### `pub fn delete_selection(&self)`

Delete the current selection. No-op when nothing is selected.
Mirrors `RichTextEditor::delete_selection`.

#### `pub fn format_version(&self) -> Signal<u64>`

Bumps on every format-only document event (bold / italic /
heading / alignment / list-style changes). See
`RichTextEditor::format_version`.

#### `pub fn cursor_position(&self) -> usize`

The **live** caret offset — reads `cursor.position()` directly, unbatched. Unlike
`cursor_position_signal`, whose stored value lags one frame
behind a just-typed printable character (the insert is deferred to the frame loop and the
signal is only re-synced on the *next* caret event), this always reflects the true caret —
what a host that recomputes highlights on a frame tick must read. Mirrors
`RichTextEditor::cursor_position`.

#### `pub fn is_composing(&self) -> bool`

`true` while an IME composition is actively in progress. Mirrors
`RichTextEditor::is_composing`.

#### `pub fn cursor_position_signal(&self) -> Signal<usize>`

Reactive caret position signal.

#### `pub fn cursor_anchor_signal(&self) -> Signal<usize>`

Reactive selection anchor signal.

#### `pub fn has_selection(&self) -> Signal<bool>`

Reactive selection-non-empty signal.

#### `pub fn can_undo(&self) -> Signal<bool>`

Reactive undo-availability signal (toolbar enable-state source).

#### `pub fn can_redo(&self) -> Signal<bool>`

Reactive redo-availability signal.
