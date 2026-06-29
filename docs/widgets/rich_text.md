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

`read_only`, `editor`, `style`, `content_padding`, `content_padding_symmetric`, `content_padding_each`, `content_padding_top`, `content_padding_right`, `content_padding_bottom`, `content_padding_left`, `wrap_mode`, `show_highlights`, `zoom`, `background`, `selection_color`, `caret_color`, `text_color`, `v_scroll_policy`, `h_scroll_policy`, `scroll_policy`, `min_lines`, `max_lines`, `follow_text_scale`, `context_menu`, `default_context_menu`, `font_registrar`, `document_version`, `cursor_position`, `cursor_anchor`, `cursor_position_signal`, `cursor_anchor_signal`, `has_selection`, `can_undo`, `can_redo`, `caret_char_format`, `scroll_y`, `scroll_x`, `context_target_at`, `selected_text`, `select_all`, `deselect`, `insert_text`, `insert_html`, `insert_image`, `delete_selection`, `select_word`, `select_line`, `set_caret_position`, `set_bold`, `set_italic`, `set_underline`, `set_strikethrough`, `set_font_size`, `set_font_family`, `toggle_bold`, `toggle_italic`, `toggle_underline`, `toggle_strikethrough`, `apply_block_format`, `apply_text_format`, `set_alignment`, `set_heading_level`, `insert_list`, `create_list`, `indent`, `outdent`, `is_in_blockquote`, `selection_spans_multiple_frames`, `toggle_blockquote`, `increase_blockquote_depth`, `decrease_blockquote_depth`, `insert_table`, `remove_current_table`, `insert_row_above`, `insert_row_below`, `insert_column_before`, `insert_column_after`, `remove_current_row`, `remove_current_column`, `is_in_table`, `is_bold`, `is_italic`, `is_underline`, `is_strikethrough`, `get_heading_level`, `get_alignment`, `undo`, `redo`, `set_default_language`, `default_language`, `handle`, `copy`, `cut`, `paste`, `paste_unformatted`, `set_zoom_level`, `get_zoom_level`, `format_version`, `document_loaded_count`, `on_link_activated`, `on_image_activated`

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

The main rich text widget. Construct via `RichTextEditor::read_only`
(view/select only) or `RichTextEditor::editor` (full editing).

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

#### `pub fn zoom(self, zoom: f32) -> Self`

Set the initial zoom factor (`1.0` = 100 %). Applied before the first
layout pass. Use `set_zoom_level` after the
widget is mounted.

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

#### `pub fn scroll_policy(mut self, policy: ScrollPolicy) -> Self`

Set the same scroll-bar visibility policy on both axes.

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
point size regardless of the reader's UI accessibility setting. The
programmatic zoom (`set_zoom`) is unaffected either way.

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
(origin at the widget's top-left, scroll offset and zoom handled
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

#### `pub fn undo(&self)`

Undo the most recent edit. Mirrors Ctrl+Z. No-op when the undo
stack is empty.

#### `pub fn redo(&self)`

Redo the most recently undone edit. Mirrors Ctrl+Y /
Ctrl+Shift+Z. No-op when the redo stack is empty.

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

#### `pub fn set_zoom_level(&self, zoom: f32)`

Set the editor's zoom level. Re-lays out immediately; triggers
a repaint via the engine's dirty tracking on the next frame.

#### `pub fn get_zoom_level(&self) -> f32`

Current zoom level.

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

#### `pub fn apply_block_format(&self, fmt: BlockFormat)`

Apply an arbitrary `BlockFormat` to the caret's block.

#### `pub fn set_alignment(&self, alignment: Alignment)`

Set paragraph alignment for the caret's block.

#### `pub fn set_heading_level(&self, level: u8)`

Set heading level for the caret's block. `0` = plain paragraph,
`1..=6` follow the HTML `<h1>..<h6>` convention.

#### `pub fn get_alignment(&self) -> Alignment`

Current block alignment.

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

#### `pub fn format_version(&self) -> Signal<u64>`

Bumps on every format-only document event (bold / italic /
heading / alignment / list-style changes). See
`RichTextEditor::format_version`.

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
