<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# FontPicker

A drop-in font-family selector, in the tradition of Qt's `QFontComboBox`,
GTK's `FontChooser`, and UIKit's `UIFontPickerViewController`. It lists every
installed font family, previews each one, and binds the choice to a
`Signal<Option<String>>` (the family name) that plugs straight into
`TextStyle.family` / `RichTextEditor::set_font_family`.

```rust
use teksilo::prelude::*;
use teksilo::widgets::FontPicker;

let family: Signal<Option<String>> = Signal::new(None);
VStack::new()
    .child(TextWidget::new(tr!(font())).style(TextStyleRole::BodyBold))
    .child(
        FontPicker::new(family.clone())
            .on_select(|name, _ctx| editor.set_font_family(name)),
    );
```

Source: [crates/teksilo-widgets/src/font_picker.rs](../crates/teksilo-widgets/src/font_picker.rs).
Demo: `cargo run -p font-picker`. Also on the widget catalog's **Rich text** tab
(`cargo run -p widget-catalog`).

## What it does

- **Self-populates** from the app's shared typesetter — no font list is
  passed in. (Reads `ctx.app_state::<SharedTypesetter>()` at build time, the
  same path `SpinBox` uses for text measurement.)
- **Previews each font.** By default every row shows the family name in a
  legible UI font next to a tiny **sample rendered in that font**; the sample
  text is chosen for the font's writing system (a Cyrillic font previews
  Cyrillic, an Arabic font previews Arabic, …). The closed control shows the
  selected family in its own typeface.
- **Searchable** (type to filter hundreds of fonts) and **filterable** by
  spacing (monospaced / proportional) and by writing system.
- Built on `ComboBox`, so it inherits complete keyboard navigation,
  type-ahead, virtualization (the dropdown never materializes hundreds of
  rows), and AccessKit wiring (`Role::ComboBox` + `HasPopup::Listbox`,
  `Role::ListBox`/`ListBoxOption` rows, `set_value`/`set_expanded`).

## API

```rust
FontPicker::new(selected: Signal<Option<String>>) -> Self   // family name = source of truth

// Item source
.families(impl IntoIterator<Item = impl Into<String>>)          // override enumeration (names only)
.families_with_meta(Vec<(String, FontMeta)>)                    // + inject monospaced/writing systems

// Filtering (accept a static value or a Signal for a reactive toolbar)
.spacing_filter(impl Into<Prop<FontSpacingFilter>>)   // Any | Monospaced | Proportional
.writing_system(impl Into<Prop<Option<WritingSystem>>>)         // restrict to a script

// Preview
.preview_mode(FontPreviewMode)   // NameThenSample (default) | NameInOwnFont | NameInSystemFont
.preview_in_own_font(bool)                            // false ⇒ NameInSystemFont
.sample_text(impl Into<String>)                       // global sample override
.sample_text_for(WritingSystem, impl Into<String>)    // per-script sample (Qt setSampleTextForSystem)
.sample_text_for_family(family, text)                 // per-font sample  (Qt setSampleTextForFont)
.show_selected_in_own_font(bool)                      // trigger in the font's own face (default true)

// ComboBox passthrough
.placeholder(..) / .label(..) / .enabled(bool) / .variant(ComboBoxVariant) / .style(impl ComboBoxStyle)
.max_visible_items(usize) / .searchable(bool) [default true] / .search_query(Signal<String>)
.on_select(impl Fn(&str, &mut EventContext))          // apply hook
.tooltip(..) / .rich_tooltip(..) / .rich_tooltip_content(..) / .composite_tooltip(..)
```

The bound value is the **family name string**, matching how fonts are named
everywhere in the stack, so it drops straight into `TextStyle { family, .. }`
or `RichTextEditor::set_font_family(name)`. There is no ambient "app font"
(unlike theme/locale), so `on_select` is where you apply the choice and the
signal is the source of truth.

### Filters are programmatic

Like Qt / GTK / UIKit, the spacing and writing-system filters are set in code,
not exposed as in-widget chrome. Bind them to `Signal`s and drive them from
your own controls next to the picker — the demo pairs the picker with a
"Monospace only" checkbox and a writing-system dropdown. The in-dropdown search
field (type-to-filter) is the one filter that lives inside the widget.

Reactive filtering re-runs even while the dropdown is open: a filter change
recomputes the visible names and pushes them into the picker's backing
`ListModel` (`replace_all`), which rebuilds only the dropdown list, not the
whole control. The currently-selected family is always kept in the list so a
filter change never silently clears your choice.

## Writing-system detection is off-thread

A font's writing systems (Latin, Cyrillic, CJK, …) are read from its OS/2
table's `ulUnicodeRange` (script coverage) and `ulCodePageRange` (the
CJK-language + Vietnamese distinction, which shares codepoints and so can't be
told apart from Unicode coverage alone — the same heuristic Qt uses), with a
cmap sample-codepoint cross-check for fonts whose OS/2 ranges are absent or
wrong. This lives in `text-typeset`
(`text_typeset::font::writing_system::writing_systems_for_face`, built on
`ttf-parser`).

Classifying a font means reading its bytes, so doing it for a whole system is
hundreds of file reads — far too much for the UI thread. The picker therefore
builds the coverage index on a **background thread** the first time it mounts
and polls readiness on the frame tick. Until the index is ready the
writing-system filter shows the unfiltered list and samples fall back to a
Latin default; the list narrows (and samples upgrade to their real scripts)
once it completes. **The UI never blocks.** Spacing filtering is instant — it
uses only font metadata, no bytes.

`WritingSystem` mirrors Qt's `QFontDatabase::WritingSystem` set (Latin, Greek,
Cyrillic, …, Simplified/Traditional Chinese, Japanese, Korean, Vietnamese,
Symbol, Ogham, Runic, N'Ko). It is re-exported at `teksilo::text::WritingSystem`.

## Accessibility

The control inherits `ComboBox`'s complete AccessKit surface: `Role::ComboBox`
with `HasPopup::Listbox`, the selected family announced via `set_value` (the
placeholder via `set_placeholder` when empty), `set_expanded`, `aria-controls`
to the open listbox, and `AutoComplete::List` in searchable mode; the popup is
`Role::ListBox` and each row `Role::ListBoxOption` with
`set_selected`/`position_in_set`/`size_of_set`. The in-font sample on each row
is decorative and hidden from assistive technology — the row's accessible name
is the plain family string, so a screen reader reads "DejaVu Sans", never the
sample text or a tofu glyph.

## Testing

The `.families(...)` / `.families_with_meta(...)` overrides make the picker
fully testable headlessly (no font backend): the latter injects synthetic
monospaced + writing-system metadata so the spacing and writing-system filter
predicates are exercised deterministically. See the tests in
[font_picker.rs](../crates/teksilo-widgets/src/font_picker.rs).

## Scope

Family selection only, exactly like Qt's `QFontComboBox`. Face / weight / style
/ size selection is a larger control — Qt splits it into `QFontDialog` — and is
out of scope here. Simplified vs Traditional Chinese is a best-effort heuristic
from OS/2 code-page bits (the codepoints are shared; the difference is
glyph-variant, undecidable from coverage alone).

## The text-typeset additions

The picker needed two capabilities surfaced from the external `text-typeset`
crate (both cheap plumbing over data the font stack already had):

- **Enumeration** — `TextFontService::families()` / `family_names()` /
  `family_is_monospaced()` (fontdb metadata; no bytes loaded).
- **Writing-system coverage** — `TextFontService::writing_system_index_builder()`
  returns a `Send` snapshot whose `build()` computes the per-family coverage
  map off-thread, plus the `WritingSystem` / `WritingSystemSet` types. Reads
  OS/2 via a new `ttf-parser` dependency (already present transitively through
  fontdb, so no newly-compiled crate).

Both are re-exported through `teksilo-text` and reachable from a widget's
`build()` via `ctx.app_state::<SharedTypesetter>()`.
