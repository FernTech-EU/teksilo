<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# The platform text input system

Text does not only reach a field from the keyboard. The operating system's
text input system also inserts it: an IME commits a composition, macOS
Dictation speaks into the field, the emoji viewer drops in a symbol, Writing
Tools rewrites a selection. This page covers what Teksilo gives a text field
and a custom text widget for those routes.

The stock text widgets (`TextInput`, `SearchField`, `PasswordField`,
`RichTextEditor`, and `TextInputField` underneath them) need nothing from you.
Read on to tell where text came from, to turn Writing Tools off, or to make a
custom text widget take part.

## 1. `ImeComposition`

While the user composes (a CJK reading, a dead-key accent, macOS dictation's
words in progress), the focused widget receives each state of the
composition as `WidgetEvent::ImeComposition`:

```rust
ImeComposition {
    text: String,
    cursor: Option<std::ops::Range<usize>>,
    replacement: Option<std::ops::Range<usize>>,
}
```

- `text` is the composition as the input method shows it now. Each update
  replaces the previous one. An empty `text` ends the composition: a commit
  usually follows (winit sends an empty update right before every commit),
  and when none does, the composition was cancelled.
- `cursor` is the caret or selection inside `text`, in byte offsets of `text`.
- `replacement` is set on the first update only, when the input method
  re-marks text already in the field: the range of the field's text, in
  characters, that the composition takes the place of (Korean Hanja
  conversion of the syllable just typed, on macOS). The field takes that text
  out while the composition shows, and puts it back if the composition is
  cancelled. `None` shows the composition at the selection.

`WidgetEvent::ime_composition(text, cursor)` builds an update with no
`replacement`.

The stock widgets show the composition inside their text, underlined, with
a thick underline under the segment the input method is converting (a
`cursor` range with an extent) and a thin one under the rest; a `cursor`
range with no extent puts the caret there inside the composition. They
record none of its updates in the undo history: one undo after a commit
takes the committed text out. An edit the application makes while a
composition shows (writing the text, say) takes the composition out of the
text; the input method's next update shows it again at the caret. A custom
widget built on `text-document` does the same with its
transient edits (`TextCursor::insert_text_transient`,
`remove_selected_text_transient` and `commit_transient_text`,
`TextDocument::revert_transient_edits`; text-document 1.15 or later).

The composition belongs to the view the writer is typing in. Every other
view of the same document, a `RichTextEditor` or a `CodeEditor`, editable or
read-only, shows the text as recorded, without the composition, and changes
once it is committed or cancelled. A host that makes transient edits of its
own and wants them shown turns on `show_transient_edits` on the views that
should show them. A custom view does the same with
`TextDocument::snapshot_recorded_flow_masked` and its siblings, and places
its cursor with `TextCursor::recorded_position` and `recorded_anchor`.

## 2. `ImeCommit`

A widget receives the text as `WidgetEvent::ImeCommit`:

```rust
ImeCommit {
    text: String,
    origin: ImeOrigin,
    replacement: Option<std::ops::Range<usize>>,
}
```

- `origin` says which part of the input system committed the text.
  `ImeOrigin` is `#[non_exhaustive]`; match it with a wildcard arm.

  | `ImeOrigin` | The text is |
  | --- | --- |
  | `Composition` | the end of a composition the user typed (a CJK candidate, a dead-key accent). The default. |
  | `Dictation` | speech the platform recognised (macOS Dictation, Windows voice typing) |
  | `WritingTools` | text macOS Writing Tools produced to replace the user's |
  | `Insertion` | text inserted outside a key press and outside a composition: the emoji viewer, an accent picked with the mouse |

- `replacement` is the range of the field's text the commit replaces, in
  **characters** (Unicode scalar values), when the input system named one: a
  dictation correction, a Writing Tools rewrite. `None` replaces the selection,
  or inserts at the caret when nothing is selected.

`WidgetEvent::ime_commit(text)` builds the plain case (a composition, no
range), which is what a test or a synthetic event wants.

A custom text widget handles an `ImeCommit` like this:

```rust
WidgetEvent::ImeCommit { text, origin, replacement } => {
    // Replace `replacement` if it is named, else the selection.
    let range = replacement.clone().unwrap_or_else(|| self.selection_range());
    self.replace(range, text);
    // Tell the application which route it was, if the widget reports that.
    let source = match origin {
        ImeOrigin::Dictation => EditSource::Dictation,
        ImeOrigin::WritingTools => EditSource::WritingTools,
        _ => EditSource::Ime,
    };
    self.report_insertion(source, text.chars().count());
}
```

Ranges are character offsets in your document. The platforms count in UTF-16
code units; the conversion is already done by the time the event reaches you.

## 3. Ending a composition from the field

A native text view ends a composition when the user clicks elsewhere in it
or the field loses focus, and keeps the composition as committed text. The
stock widgets do the same, and also before an undo, redo, cut, paste or
format command runs (from a key, a menu or the native menu bar) and before
the command of a control that takes no focus (a toolbar button).

That last case is the framework's: a click that completes a tap on a control
built with `focus_on_click(false)`, or on one with nothing focusable from it
up to the root (a `focusable(false)` icon, a plain `on_tap` node), blurs
nothing, so before the control's handlers run the router calls the
`on_commit_edit` hook of the focused widget and of its ancestors, and the
stock widgets end their composition there. A screen reader's Click on such a
control does the same. Nothing is committed for a hover, a scroll, a press on
empty background (no command runs and focus stays), a press dragged off the
control or taken by a drag, nor for a click on scroll chrome (another view's
scroll bar, a tab strip's scroll arrows), which scrolls and runs no command,
nor for a click on a part of the focused field itself: its own subtree (its
scroll bar) and the popups attached to it (its completion or candidate list,
its context menu).

They call `ctx.finish_ime_composition()` first, while the composition still
shows. It tells the input method that the composition is over and returns
the text the input method committed for it, if it committed one. The field
keeps that text in place of what the composition showed, or what it showed
on `None`, as one undo step reported as `EditSource::Ime`. Whatever the
platform still delivers for the composition (an empty update, an empty
commit) arrives after the handler returns, and must find nothing to change.
A custom widget does the same, in that order:

```rust
let committed = ctx.finish_ime_composition();
// The input method's own form, under the rules a commit follows; else what
// the composition showed.
let text = committed.filter(|text| !text.is_empty()).unwrap_or(shown);
self.commit_composition(text); // one undo step; the composition is over
```

- **macOS** ends the view's marked text and has the input method discard its
  composition. The field keeps what the composition showed, as TextEdit does
  (Pinyin's "ni hao", with its space).
- **Windows** asks the input method to complete the composition, as Notepad's
  text control does, and the field keeps what it commits: Microsoft Pinyin
  shows `nihao` as "ni'hao" and commits "nihao", and keeps an apostrophe the
  writer typed ("xi'an"). winit's own copy of that commit, which it delivers
  after the handler, is dropped. An input method that commits nothing leaves
  the field what it showed.
- **Linux** has no such call in winit 0.30, so Teksilo switches the window's
  IME off and on again, and the field keeps what the composition showed.
  fcitx5 on KDE Plasma then sends the old composition again; Teksilo drops a
  repeat of the last composition it delivered until the input method sends
  something new.

## 4. `EditSource` and `on_text_inserted`

`EditSource` names the **channel** text arrived by: `Keyboard`, `Ime`,
`Dictation`, `WritingTools`, `Clipboard`, `Accessibility`, `Programmatic`. It
is not evidence of authorship. Typed text was typed one key at a time and
nothing more is claimed; what a channel means is for the application to decide.
`EditSource` lives at `teksilo_widgets::EditSource` (and
`teksilo::widgets::EditSource`) and is exhaustive, so a new channel is a
compile error where you count them.

`on_text_inserted(|source, chars| ..)` reports each insertion on a field. It is
on `TextInputField` and forwarded by `TextInput`, `PasswordField` and
`SearchField`; `RichTextEditor` reports the same sources. It fires for typing,
a finished composition, dictation, Writing Tools, a paste and an assistive
technology's `SetValue` or `ReplaceSelectedText`. It does not fire for the
application writing the bound `Signal<String>`, nor for undo or redo, nor for
an intermediate composition state. Emoji and other insertions report as
`EditSource::Ime`. The callback runs while the field is mid-edit, so it counts
characters rather than reading the field back.

`cargo run -p rich-text-editor` shows a count per source.

## 5. Writing Tools

macOS Writing Tools may read a field's text and replace it with what it
produces. It is on by default. For a field whose text must stay what the user
wrote:

```rust
TextInput::new(value).writing_tools(false)
```

The argument is `impl Into<Prop<bool>>`, so a `Signal<bool>` flips it live. It
is on `TextInputField`, `TextInput`, `SearchField` and `RichTextEditor`. A
secure field (`PasswordField`) never allows it. With it off, the Writing Tools
rows of the Edit menu are greyed out while that field has focus. Other
platforms have no such feature and the setting changes nothing there.

## 6. A custom text widget: `TextInputDocument`

Dictation, the accent popup and Writing Tools start by asking the window's
view where the selection is and what the text is. There is one native view
per window, so Teksilo answers from whichever text widget has focus. A widget
describes its document by implementing `teksilo_core::text_input_document::TextInputDocument`
and returning it from `TextSurface::input_document` on the surface it
registers with `BuildContext::register_text_surface`. The platform reaches it
with `TextSurfaces::with_focused_input_document`.

The trait answers, in characters and window-logical coordinates: the length,
the text of a range, the selection and caret, the composition range, where the
first line of a range is drawn, and the character nearest a point. A query
answers `None` when the widget cannot answer (no layout yet, busy, a secure
field). A widget that batches typed characters until its next frame must
answer as if they were already in the document. `Utf16Index` converts between
characters and the UTF-16 offsets the platform uses. A widget that does not
implement the trait still receives `ImeCommit`s; Dictation and Writing Tools
need the answers to start.

On Windows the same answers serve the input method's own requests: where a
character of the composition is drawn (its candidate window follows it) and
the text around the composition. Windows asks from
inside the window's procedure, where no application code may run, so the
bridge first asks `is_available`: whether the document can answer now
without any change. It is `false` while the widget is in the middle of its
own work, and while it holds batched typing, which a read would insert
first, running the change callbacks; the request is then answered
"nothing". It is `true` by default; a widget that batches typing or holds
its state while it works answers it.

A widget can also take the input system's writes, by returning a
`TextInputEditor` from `TextSurface::input_editor` (`None` while it takes no
text). The input system replaces ranges (`replace_text`), sets the selection,
and writes its composition into the document itself, then marks it
(`mark_composition`); while one is marked, its writes are not recorded and
the field's rules (filters, a length cap) wait for `end_composition`, which
records the final text as one undo step. Writes come between
`begin_input_batch` and `end_input_batch`; inside a batch the widget changes
only its own state and holds back what the application would see. Every
method has a default that refuses, so a widget that implements none of it
keeps the platform's other paths. The stock widgets implement it. For the
platform to report changes back, `revisions` and `text_changed_since` say
what changed since a revision and whether the input system caused it, and
`flush_pending_input` inserts batched typing at a moment the platform picks.

## 7. Platforms

A password field (`PasswordField`, or any field whose IME purpose is
`ImePurpose::Password`) takes no input method on Windows and X11: its text
comes from the keys alone, as in a native password field, and no
composition, emoji panel or Windows voice typing reaches it. On Wayland and macOS the
input method stays allowed and is told the purpose: on Wayland that keeps
the on-screen keyboard, and the input method types directly; on macOS a
Roman layout's dead keys compose through the input system. macOS secure
event input (`EnableSecureEventInput`, which a native secure text field
turns on) is not done yet, and winit passes the purpose to nothing there.

| Platform | What works | Notes |
| --- | --- | --- |
| macOS | Emoji & Symbols, Start Dictation, the press-and-hold accent popup, Writing Tools in its panel, Korean Hanja conversion of the syllable just typed. Keys the input method swallows (the accent popup open) do not type, a key it answers with a commit of its own (2-Set Korean's Space with nothing composed) types that commit once, and a key it types as another character (Kotoeri's full-width space, Pinyin's full-width punctuation) types that character. A switch to another input source in the middle of a composition commits it where it was, once, as the input method commits it (Pinyin's `ni hao` as `nihao`), whether the input method does so itself or not; another mode of the same input method (Kotoeri's Katakana after its Hiragana) leaves the composition going. | AutoFill is not supported. The rows come from the Edit menu: see [Rows macOS adds to the Edit menu](native-menu.md#rows-macos-adds-to-the-edit-menu). |
| Windows | The emoji panel and voice typing insert, as do compositions from the Microsoft Japanese, Pinyin and Korean input methods, Hanja conversion of the syllable being composed included. | The emoji panel and voice typing arrive as IMM results and report as `Ime`: the two cannot be told apart without a TSF text store, and without one voice typing shows no text while you speak. The window answers the input method's requests about where a character is drawn and the text around the composition. Reconversion of committed text (the Japanese conversion key) is not answered yet: it did not open reliably. Hanja conversion of a Korean syllable already committed reaches an IMM32 window as no request at all in the measurements so far, where Notepad, a TSF application, reconverts it. A design for a TSF text store is in `engineering/docs/tsf-text-store.md` in the repository. |
| Linux | Compositions, measured with fcitx5's Mozc, Pinyin and Hangul on KDE Plasma (Wayland). | Compositions arrive as `Composition`. winit 0.30 reports no surrounding text, so Korean Hanja conversion of text already committed does not work. |

On macOS, dictated text carries no public marker: it streams in as marked text
and is committed like a composition. Teksilo tells it apart through private
AppKit methods (`NSTextInputContext`'s dictation start and end notifications).
Where those methods are absent, dictated text reports as a `Composition` and
nothing else changes.

macOS behaviours to know when trying it (measured on macOS 27):

- Spoken editing commands ("delete that", "replace X with Y") belong to Voice
  Control, not to Dictation: Dictation types them as words, in Teksilo as in
  TextEdit.
- The dictation service can stop serving a process that has no bundle
  identifier, such as a bare `cargo run` binary, while it keeps serving
  bundled apps. If Dictation does not start in a demo, run it from a bundled
  copy (an `.app` with its own bundle identifier).
- In a process with no bundle identifier, the first Korean syllable after a
  switch to 2-Set Korean can come out as separate jamo (ㅎㅏㄴ for 한): the
  input method inserts the first jamo instead of composing it, though
  Teksilo answers it exactly as when it composes. A bundled copy composes,
  apart from a rare split on the first Korean activation of the process.
  Try Korean input from a bundled copy.
- The first activation of Kotoeri (Japanese) in a fresh process types plain
  Latin letters, in a native `NSTextView` too; selecting the input source
  again makes it compose.

## 8. Diagnosing

`TEKSILO_TEXT_INPUT_TRACE=1` makes the macOS and Windows hooks log every call
the text input system makes on a window to stderr. Run a demo with it set to
see what a given feature sends.

```text
TEKSILO_TEXT_INPUT_TRACE=1 cargo run -p rich-text-editor
```

On macOS, `TEKSILO_TEXT_INPUT_TRACE=stacks` also writes a short call stack on
the input method's calls inside a key press, which tells the input method's
own requests apart from AppKit's.

The automation tool `type_ime` takes an `origin`, so a headless test can
commit text as dictation or Writing Tools would; see
[Automation MCP](automation-mcp.md).
