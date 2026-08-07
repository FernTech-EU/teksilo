<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# LanguageSwitcher

LanguageSwitcher — a drop-in UI-language picker for settings screens.

A thin `ComboBox` preset that lists the application's supported
locales and switches the active locale on selection. Each entry is
shown as its **endonym** — the language's own name — followed by the
BCP-47 tag, e.g. `français (fr-FR)`, `Deutsch (de-DE)`,
`العربية (ar-SA)`. Showing endonyms (not "French", "German", "Arabic")
means a speaker of each language can always find their own in the list.

Zero-config: drop it into a settings panel and it

- self-populates from the installed `I18nManager`
  (`teksilo_i18n::current_supported_locales()`),
- shows the active locale as the current selection
  (`teksilo_i18n::current_locale()`),
- switches the app locale on selection via `EventContext::set_locale`,
  which the window manager fans out to every window (re-translating
  text and flipping layout direction for RTL locales like Arabic),
- and keeps its selection in sync if the locale is changed elsewhere.

```ignore
// In a settings panel's build():
VStack::new()
    .child(TextWidget::new(tr!(ui_language())).style(TextStyleRole::BodyBold))
    .child(LanguageSwitcher::new())
```

Endonyms come from ICU4X CLDR data via
`teksilo_i18n::language_endonym`; an unknown tag falls back to the
raw BCP-47 tag. When no `I18nManager` is configured the switcher
renders an empty, placeholder ComboBox.

## Builder methods at a glance

`variant`, `label`, `locales`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/language_switcher/index.html)

## `pub struct LanguageSwitcher`

A UI-language picker built on `ComboBox`. See the module docs.

```rust
pub struct LanguageSwitcher { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a switcher that auto-discovers the supported locales from
the active `I18nManager`.

#### `pub fn variant(mut self, variant: ComboBoxVariant) -> Self`

Pick the inner ComboBox's design-language variant.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Set the accessible / control label (defaults to `"Language"`).
Pass a `tr!(...)` to localize it.

#### `pub fn locales(mut self, locales: Vec<LanguageIdentifier>) -> Self`

Override the locale list instead of auto-discovering it from the
active `I18nManager`. Useful in previews / tests, or to restrict
the offered set.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain tooltip, forwarded to the inner `ComboBox`.
Mutually exclusive with the rich / composite variants — last
call wins.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip resolved from the app-wide registry,
forwarded to the inner `ComboBox`. Overrides any previously
set tooltip.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip driven by inline
`TooltipContent`, forwarded to
the inner `ComboBox`. Overrides any previously set tooltip.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip hosting an arbitrary widget tree,
forwarded to the inner `ComboBox`. Overrides any previously
set tooltip.
