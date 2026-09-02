<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CommandPalette

CommandPalette — type-to-run access to every command an app has registered.

The palette is **application-agnostic**: it holds no list of its own and knows
nothing about any particular app. Its content is the tree's
`ShortcutRegistry`, which already
carries everything a palette row needs — a localized
`name`, an optional `category` to group by, an
optional `description`, the effective keystroke (user rebinds merged in), and a
live `enabled` verdict. Activating a row sends the command's intent, which is the
same path a menu row or the chord itself takes.

That has a consequence worth stating plainly, because it is the whole design:
**a command does not need a keystroke to appear here.** `iter_effective()` yields
every registered entry, bound or not, so an app makes a command searchable by
registering it with a name and no chord:

```ignore
// Reachable from the palette, and rebindable by the user later, without
// occupying a keystroke today.
ctx.register_shortcut_global(
    Shortcut::new("document.export")
        .name("Export…")
        .category("File")
        .build(),
);
```

# Presenting it

`CommandPalette::present` shows it centered, dismissed by Escape or a click
outside:

```ignore
ctx.register_action_global(Action::new("app.command_palette").on_invoke(|_, ctx| {
    CommandPalette::new().present(ctx);
}));
```

Presenting it as a **window-level** modal is deliberate, not incidental: a palette
is routinely opened from a menu, and a menu is itself a transient overlay.
Anchoring to the invoking widget would render the palette inside the menu that
opened it, positioned against a surface that is about to disappear.

# Matching

Typing filters by subsequence, not substring, so `ndw` finds "New Window" and
`expdoc` finds "Export document". Matches score higher when the typed letters land
consecutively and on word starts, so the most literal reading of a query sorts
first. An empty query lists everything in the registry's own deterministic
`(category, id)` order. The category takes part in matching, so `file new` finds
the New command filed under File.

# Keyboard

Focus stays in the search field throughout — that is what makes a palette feel
like one. Arrow keys are not editing keys for the field, so they bubble to the
palette's own handler, which moves the highlight and scrolls it into view. Enter
runs the highlighted command; Escape dismisses.

## Builder methods at a glance

`placeholder`, `empty_text`, `include`, `on_dismiss`, `show_disabled`, `query_signal`, `present`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/command_palette/index.html)

## `pub struct PaletteCommand`

One command as the palette sees it.

A read-only projection of a registered shortcut, handed to
`CommandPalette::include` so an app can decide what belongs in its palette
without the widget growing knowledge of any app's command names. Deliberately
*not* the `Shortcut` itself: that type carries the activation closure and the
rebinding machinery, neither of which a filter predicate has any business
reaching.

```rust
pub struct PaletteCommand { /* fields */ }
```

## `pub struct CommandPalette`

Type-to-run access to every registered command. See the `module docs`.

```rust
pub struct CommandPalette { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

A palette over every command in the tree's registry.

#### `pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self`

Replace the search field's placeholder text.

#### `pub fn empty_text(mut self, text: impl Into<LocalizedString>) -> Self`

Replace the text shown when nothing matches the query.

#### `pub fn include(mut self, f: impl Fn(&PaletteCommand) -> bool + 'static) -> Self`

Keep only the commands this predicate accepts.

The usual reasons are to hide the command that opens the palette itself, and
to drop registry entries that are key bindings rather than commands a person
would look for by name.

#### `pub fn on_dismiss(self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Run this after a command is activated, and when Escape is pressed.

`present` installs its own, so this is for callers embedding
the palette in a surface they manage themselves.

#### `pub fn show_disabled(mut self, show: bool) -> Self`

Also list commands whose `enabled_when` predicate currently says no, greyed
out and inert. Off by default: a palette answers "what can I do now", and a
row that cannot run is a row that has to be explained.

#### `pub fn query_signal(&self) -> Signal<String>`

The query signal, so a caller can seed or observe what was typed.

#### `pub fn present(self, ctx: &mut EventContext)`

Show the palette centered in the window, dismissed by Escape or a click
outside. See the `module docs` on why this is window-level.
