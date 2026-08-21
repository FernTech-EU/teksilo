<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Changelog

All notable changes to Teksilo are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) —
though pre-1.0, so breaking changes can land in a minor bump. `release.toml`
keeps every workspace crate on one shared version; entries below are grouped
by crate for clarity, not because crates version independently.

## [Unreleased]

### Fixed — `teksilo-widgets`: caret motion follows the platform's own layout

Word-jump, the line edge and the document edge do not merely sit on a different
modifier on macOS — they are laid out differently. Windows and Linux put word
on `Ctrl+←/→` and the line edges on bare `Home`/`End`; macOS spreads the same
motions across three modifiers on the arrows themselves: `⌥←/→` word, `⌘←/→`
line edge, `⌘↑/↓` document, `⌥↑/↓` paragraph.

Every text surface read a single "is the accelerator held?" flag, so on macOS
`⌘←` jumped a word (it should reach the start of the line), `⌘↑` moved one line
(it should reach the top of the document), and `⌥←` did nothing at all. Reading
the chords through the new `common::text_nav` fixes `RichTextEditor`,
`CodeEditor`, and `TextInput` / `SearchField` / `PasswordField` / `SpinBox` /
`DateEdit` through the shared `TextInputField`. Word-delete moves with them —
`⌥⌫` on macOS, `Ctrl+⌫` elsewhere. `LogView` is untouched: it scrolls a
read-only view and has no caret to move.

Windows and Linux are unchanged. `Alt+↑/↓` keeps move-line in the code editor
on every platform (the binding every code editor ships, macOS included), so
only the rich-text editor reads `⌥↑/↓` as a paragraph motion. `⌘⌫` means
delete-to-line-start on macOS, which is not implemented; it falls through to a
plain single-character delete rather than removing more than was asked for.

`⌘←/→` folds through the caret's text direction the same way character and word
steps already do, so all three horizontal motions agree about which way "left"
is inside an Arabic paragraph.


### Changed — `teksilo-core`: a declared `Ctrl` shortcut now fires on ⌘ on macOS

**Behaviour change on macOS.** A `Shortcut` whose declared default is a `Ctrl`
chord is now read as *the platform's primary accelerator* and resolves to ⌘
there — the convention Qt spells `Qt::CTRL`, and the one Teksilo's native menu
bar has always applied when building `NSMenuItem` key equivalents. So
`Shortcut::new("editor.find").primary(KeyStroke::ctrl(Key::F))` fires on ⌘F on
macOS and on Ctrl+F everywhere else, from one declaration.

Until now the registry matched chords by exact modifier equality while the
native menu row advertised ⌘, so on macOS a command that appeared on a menu
worked (AppKit dispatched the key equivalent) and an identical command that did
not appear on one silently required physical ⌃. Three places in the framework
answered the Ctrl-vs-Command question independently and the one that dispatches
keystrokes was the one that did not.

The corollary is that a declared `Ctrl` chord **no longer fires on physical ⌃**
on macOS, where Control belongs to the text system and to the secondary click.

**If you have a chord that really is Control on every platform** — Ctrl+Tab
cycles tabs on macOS too, and the ⌘ form of Space, H or Q is taken by the
system — declare it with the new opt-out:

```rust
Shortcut::new("view.next_tab")
    .literal_modifiers()
    .primary(KeyStroke::ctrl(Key::Tab))
    .build()
```

Worth auditing your declarations for that shape; nothing else needs changing.
User overrides are deliberately left literal, so a chord captured in a settings
UI still means exactly the keys that were pressed and physical ⌃F stays
bindable on macOS.

New API: `Modifiers::COMMAND` / `Modifiers::command()` (the platform's primary
accelerator, for widgets testing a live `Modifiers`), `KeyStroke::command()` /
`command_shift()` (the same intent stated outright, for chords built outside the
registry), `KeyStroke::with_command_convention()`, `Shortcut::declared_keystrokes()`,
`ShortcutBuilder::literal_modifiers()`.

### Fixed — `teksilo-widgets`: accelerator chords across the widget catalog

The same gap, widget-side. Select-all, the discontiguous-selection click, the
marquee's additive modifier and Ctrl+Home / Ctrl+End were all testing physical
Control, so on macOS ⌘A did not select all and ⌘-click did not extend a
selection — while ⌃-click, which macOS treats as the secondary click, did.
`ListView`, `TreeView`, `TableView`, `TreeTableView`, `GridView`, `Calendar` and
the colour picker's swatch grid now test `Modifiers::command()` for those.

The text surfaces (`RichTextEditor`, `TextInputField` and everything built on
it, `CodeEditor`, `LogView`) tested `ctrl() || super_key()`, which worked on
macOS but also made the Win key act as Ctrl on Windows and Linux; they now test
the accelerator too. The Explorer-style cursor pair (Ctrl+Space / Ctrl+Arrow),
Ctrl+Tab's focus escape, Ctrl+Space forced completion and the terminal's control
codes deliberately stay on literal Control — each says so at its site.

Context-menu labels are fixed with them: the built-in Cut / Copy / Paste /
Select All rows built their accelerator text from hard-coded `Ctrl` literals, so
on macOS a right-click in any text field read "Copy ⌃C" while the menu bar
showed ⌘C for the same command.

### Added — `teksilo-widgets`: `Settings…` in the macOS App menu

`StandardMenu::settings_intent("app.settings")` puts a **Settings…** row in the
application menu, under About, on ⌘, — placement and chord that an ordinary
`MenuEntry` cannot reach, since the App menu is filled in by the platform.
Pair it with `.settings(tr!(settings()))` for the label.

Unlike Quit there is no system fallback — no platform opens an arbitrary app's
settings on its own — so leaving `settings_intent` unset omits the row rather
than rendering a dead one.


### Added — `teksilo-automation`: `scroll` carries modifiers

The `scroll` op hardcoded `Modifiers::NONE`, so it could only ever deliver a
bare wheel. It now takes `ctrl` / `shift` / `alt` / `meta` alongside `dx` / `dy`,
mirroring `inject_key`; all default to false, so every existing caller is
unchanged.

A modifier-held wheel is a distinct gesture, not a decorated one —
`WidgetEvent::Scroll` carries a `modifiers` field precisely so an app can
implement Ctrl-wheel-to-zoom, and its own doc comment says as much. Until now
that was the one input the bridge could *describe* but not *perform*: a probe
could assert that a plain wheel scrolls and never that Ctrl+wheel does anything
else, which is exactly backwards for a feature whose whole risk is the two being
confused. Two tests pin it — that each modifier reaches the widget as asked, and
that the no-modifier default is still a plain wheel.

### Added — `teksilo-theme-macos`: the macOS (Aqua / Dark Aqua) preset

`teksilo-theme-macos` was a stub returning an IntUI-shaped baseline behind a
wall of `TODO(macos)` markers. It is now a complete design language, opt-in via
the umbrella crate's `theme-macos` feature and reachable as
`teksilo::prelude::macos::{light, dark}`.

**Tokens, with their provenance stated.** Apple attaches a standing disclaimer
to every colour value it publishes ("the actual color values will fluctuate from
release to release"), and publishes no table at all for corner radii, control
heights, focus-ring geometry or animation durations bar one. So every literal in
the crate is tagged at its definition: **`[HIG]`** (published — the 13-hue system
colour table and the whole typography ramp), **`[measured]`** (a community
capture of the private `NSColor` enumeration, or a screen measurement), or
**`[derived]`** (computed from one of the first two, with the rule given). The
full AppKit vocabulary — four label grades, two independent selection families,
the bezel description, the eight System Settings accents — is exposed through
the **`MacOsPalette`** theme extension.

Geometry is the two measured radii (6 dp in-page, 10 dp floating, with a menu at
its own measured 9 dp) and a **22 dp** control height — a third shorter than
Fluent's 32, which is most of why a macOS window fits more. Typography is the
published text-style ramp (Body 13/16, Callout 12/15, Subheadline 11/14) with
Apple's **signed tracking**: −0.08 at 13 pt, exactly 0 at 12, +0.06 at 11. It is
the only Teksilo preset that tracks non-uniformly, and the only one whose
tracking changes sign. Motion is Core Animation's default 0.25 s on
`kCAMediaTimingFunctionEaseInEaseOut`, `cubic-bezier(0.42, 0, 0.58, 1)` —
symmetric, where Fluent's curve is decelerate-only.

**Chrome for 28 style slots.** Eight are real `impl FooStyle` blocks, for the
controls where AppKit is structurally its own thing:

- the push button's **bezel** — a soft shadow, a faint top-to-bottom face
  gradient, a hairline, and (in Dark Aqua) a catch-light along the top edge.
  Pressing it darkens the face and drops the shadow, so the control settles into
  the surface. The *default* button deliberately gets none of it: a flat accent
  fill, because doubling the elevation cues is what makes a macOS default button
  look wrong;
- a focus ring that **is the accent** — not Fluent's high-contrast neutral
  outline. Drawn naively that fails WCAG SC 1.4.11 (the accent at the ~50 %
  alpha the halo appears to have measures 1.86:1 on `windowBackgroundColor`), so
  it is built as two bands: a 2 dp solid one at full accent that carries the
  contrast, and a translucent halo outside it that supplies the macOS softness;
- the `NSSwitch`'s **18 dp knob in a 22 dp track** — a knob that nearly fills its
  corridor, wearing the same bezel the push button does, where Fluent's is a
  12 dp dot in a 20 dp track;
- the 14 dp checkbox and radio (half the size of Fluent's 20 dp box), bezelled
  while unchecked, with AppKit's forward-leaning tick and its small fixed pip;
- the field's **accent focus halo** — the whole announcement, where Fluent grows
  its bottom edge and Material 3 thickens its outline;
- the slider's plain round bezelled knob, which unlike Fluent's does not resize
  under the pointer;
- the menu row's **accent fill with a white label**, where Fluent uses a neutral
  wash;
- the list row's **selection capsule** — Big Sur's inset rounded rectangle,
  accent-filled with `alternateSelectedControlTextColor` on top.

The other twenty are the shipped `Recipe*Style` constructed with AppKit metrics,
not reimplementations.

**Accent.** `controlAccentColor` is whatever the user picked in System Settings.
`light()` / `dark()` resolve it against the out-of-box `systemBlue`;
**`light_with_accent(Color)` / `dark_with_accent(Color)`** rebuild the accent
family around any other seed, and **`SystemAccent`** carries the eight swatches
macOS itself offers. `linkColor` deliberately does *not* follow — AppKit keeps
links a fixed blue whatever accent is chosen. Note that an accent-filled control
paints `selectedContentBackgroundColor` (a *darkened* accent) rather than the raw
one: white on the raw `#007AFF` is only 4.02:1, so painting it would have shipped
an inaccessible default button.

**Four documented deviations from Apple's own numbers**, each with the
measurement that forced it and a test that pins the premise:

- **The label grades are lifted, and the lift is computed rather than
  guessed.** `secondaryLabelColor` is 50 % black in Aqua (3.98:1 on
  `textBackgroundColor`) and 55 % white in Dark Aqua; neither clears WCAG SC
  1.4.3's 4.5:1 floor on every surface the preset paints. Rather than hand-pick
  a number per appearance, the projection raises the alpha until it clears the
  floor on each surface **including the hover and press washes over it** — a
  press wash moves a surface *toward* the label, and that is where Apple's own
  value first stops passing. Apple's numbers stay on `MacOsPalette`; the lifted
  ones are what `ColorTokens` carries.
- A 45 % `border_strong` where AppKit's control hairline measures 1.25:1
  against WCAG 1.4.11's 3:1 floor for a control boundary.
- The *Accessible* variant of each system colour for status foregrounds rather
  than the Default one (`systemRed` Default is 3.55:1 on white).
- The IntUI amber kept for the search highlight, because `findHighlightColor` is
  pure `#FFFF00` in both appearances and AppKit special-cases the text on it to
  black.

A discrete slider's **tick marks** paint `border_strong` rather than
`tertiaryLabelColor` for the same reason a checkbox outline does — a tick marks
a value the slider can actually take, and AppKit's 25 % tertiary label measures
1.8:1 on the window in Aqua. The slider's *rail* is deliberately left faint:
there the knob is the control and the rail is the groove behind it, which AppKit
also keeps subtle. Both are commented at the paint site so the asymmetry is not
mistaken for an oversight.

**Known limitations, stated rather than deferred.** The OS accent is not read —
Teksilo's platform layer returns only the light/dark preference on macOS, so
there is no live `NSColor.controlAccentColor` to bind to. Vibrancy uses each
material's opaque fallback, which is what macOS itself shows with "Reduce
transparency" on. The `TableView` / `GridView` selection band is an accent *wash*
rather than the capsule, because those views paint the shared `surface_selected`
token behind app-supplied cell widgets this preset cannot retint. San Francisco
cannot be bundled, so the optional `system-fonts` feature *names* the macOS faces
and the default build keeps the metric-neutral bundled Inter.

### Added — `teksilo-core`: two defaulted label-role style hooks

`StandardItemStyle::selected_label_role` and
`MenuItemStyle::highlighted_label_role`, both defaulting to `None`.

A row builds its label before any style's `make_body` runs, so a design language
whose selection is a **solid** fill could not recolour the text on top of it —
macOS's accent capsule would have left `labelColor` at roughly 3.5:1. These
declare the role instead, and the widget composes it into the label's (and the
menu row's shortcut's) colour signal, gated on the row actually being emphasised
so an unemphasised or window-inactive row keeps its normal label. Same shape as
the existing `ButtonStyle::label_text_role`, and defaulted for the same reason:
IntUI and Fluent use pale washes, need no flip, and are unchanged.

### Fixed — `teksilo-widgets`: a tree row's chevron ignored its row's colour

`TwistArrow` painted a hardcoded `TextRole::Secondary` with no way to override
it. That was invisible while every theme's selection was a pale wash, and wrong
the moment one is not: under a style that flips a selected row's label (see
above), the label turned white and the chevron stayed a grey smudge on the
accent capsule — roughly 2.5:1, under WCAG SC 1.4.11's 3:1 floor. `MenuItem`'s
own chevron was already wired to its row's text role; `StandardTreeItem`'s was
not.

`TwistArrow::color(impl Into<ColorProp>)` now takes any colour, role or signal
(defaulting to the previous `Secondary`), and `StandardTreeItem` hands it the
same role its label uses, so the two always move together. Under IntUI and
Fluent nothing changes.

### Changed — `widget-catalog`: macOS in the theme switcher and `--theme`

The catalog's title-bar `ThemeSwitcher` gains macOS Light / macOS Dark, and
`--theme` accepts `macos-light` / `macos-dark`. Both are covered by the existing
persist/restore round-trip test, the failure mode where a new theme persists
happily and then silently reverts on the next launch.

### Fixed — `teksilo-widgets` SpinBox: the mouse wheel was inverted

Scrolling **down** over a `SpinBox` increased its value, and scrolling up
decreased it — the opposite of `QAbstractSpinBox`, `GtkSpinButton` and WinUI's
`NumberBox`, and the opposite of the widget's own ArrowDown key. Independent of
the theme.

`ScrollDelta` is not winit's raw wheel reading: `translate_mouse_wheel` negates
it so that **positive y increases a scroll offset** — which is why `ScrollArea`,
every data view and the `TabBar`'s wheel-to-horizontal remap all add it straight
to their scroll position. `SpinBox` was the one consumer mapping a wheel notch
to a *value* rather than an offset, and it read positive y as "the user scrolled
up". Now `y > 0` steps down. The wheel path had no test coverage at all; it has
three now, including one that pins the wheel against the arrow keys.

### Added — `teksilo-theme-fluent`: the Fluent (Windows 11 / WinUI 3) preset

`teksilo-theme-fluent` was a stub returning an IntUI-shaped baseline behind a
wall of `TODO(fluent)` markers. It is now a complete design language, opt-in via
the umbrella crate's `theme-fluent` feature and reachable as
`teksilo::prelude::fluent::{light, dark}`.

**Tokens, transcribed from primary source.** Every colour comes from WinUI's own
`Common_themeresources_any.xaml`, written in WinUI's `#AARRGGBB` notation so the
file diffs against the theme dictionary line by line. The full token set —
including the graded control fills, the on-accent strokes and the system fills
that Teksilo's `ColorTokens` has no slot for — is exposed through the
**`FluentPalette`** theme extension. Geometry is Fluent's two radii
(`ControlCornerRadius` 4 dp for in-page and bar elements, `OverlayCornerRadius`
8 dp for anything that floats, with the tooltip as the documented 4 dp
exception). Typography is the WinUI type ramp — Body 14/20, Caption 12/16 — at
**zero tracking** on every rung, the opposite of Material 3. Motion is the four
`Control*AnimationDuration` steps on `ControlFastOutSlowInKeySpline`,
`cubic-bezier(0, 0, 0, 1)`.

**Chrome for 25 style slots.** Eight are real `impl FooStyle` blocks, for the
controls where WinUI is structurally its own thing:

- the button's **elevation edge** — a heavier stroke along the bottom edge in
  light (a cast shadow) and the top edge in dark (a catch-light), dropped on
  press. WinUI produces it with a gradient border brush anchored to a fixed 3 dp
  band, flipped in the light dictionary only;
- a **two-tone, high-contrast focus ring** — 2 dp near-black (light) or white
  (dark) outside a 1 dp opposite-colour inner ring. Fluent never tints the focus
  indicator with the accent, so neither does this;
- the `ToggleSwitch`'s off-state outline, grey knob, and knob that grows on
  hover and squashes to 17 × 14 under the press;
- the filled unchecked checkbox and radio (an `ControlAltFill` box inside a
  `ControlStrongStroke` outline — IntUI's transparent-with-a-hairline box would
  fail WCAG 1.4.11 on a light Fluent surface);
- the field's **accent focus underline** — `TextControlBorderThemeThicknessFocused`
  is literally `1,1,1,2`;
- the slider's two-circle thumb, whose accent inner dot swells to 14 dp on hover
  and shrinks to 10 dp while dragging;
- the menu row's **neutral** hover (`SubtleFillColorSecondary`, not an accent
  tint);
- the list row's **selection pill** — the 3 × 16 dp accent bar on the leading
  edge that makes Windows 11's neutral selection wash legible.

The other seventeen are the shipped `Recipe*Style` constructed with Fluent
metrics, not reimplementations.

**Accent.** `AccentFillColor*` is not a literal in WinUI — it binds to the ramp
Windows generates from the user's accent, and the two appearances pull from
*opposite ends* of it (light fills with a darkened accent and white labels, dark
with a lightened one and black labels). `light()` / `dark()` resolve it against
the Windows out-of-box `#0078D4`; **`light_with_accent(Color)` /
`dark_with_accent(Color)`** rebuild the whole accent family around any other
seed, leaving every neutral token untouched.

**Known limitations, stated rather than deferred.** Mica and Acrylic are
compositor materials with no flat-fill equivalent; every surface uses the opaque
fallback WinUI itself falls back to when the material is unavailable. Segoe UI
Variable is proprietary and cannot be bundled, so the optional `system-fonts`
feature *names* the Windows faces for the text engine to resolve and the default
build keeps the metric-neutral bundled Inter.

### Changed — `widget-catalog`: Fluent in the theme switcher and `--theme`

The catalog's title-bar `ThemeSwitcher` gains Fluent Light / Fluent Dark, and
`--theme` accepts `fluent-light` / `fluent-dark`. Theme restore-on-launch moved
from an inline `match` to a named `theme_from_id`, and the example gained its
first tests — including one that walks every offered preset through a
persist/restore round trip, the failure mode where a new theme persists happily
and then silently reverts on the next launch.

### Added — `teksilo-widgets` docking: rail actions + Strip bar slots

**`DockAction` — dockless command buttons in the activity rail.** A rail item
was previously always one activity (a tab with a panel behind it); there was no
way to put a plain command in that column. `DockRail::action(DockAction::new(…))`
adds one, rendered by the framework so it matches a real activity item — same
size modes, same selected-surface highlight, same side-placed tooltip, same
`Icon + Label` rotated caption.

An action is deliberately more restricted than an activity: never draggable,
never hidable, no "Move to" menu, and never overflow-parked (it is reserved
space). `DockActionPlacement::{Start, End, Pinned}` picks the cluster — `Pinned`
sits past the spacer at the rail's far edge, VS Code's Accounts / Manage
position, where a Settings gear belongs. `DockActionId::named("…")` is a
`const fn` (FNV-1a), so ids are module-scope `const` items and stay stable across
runs for the accessibility tree and the automation bridge.

Nothing about an action is persisted — it carries no user-mutable state, so it
is app config reconstructed each run, like the rail's slots.
**`DockLayoutState` is unchanged and needs no migration.** `DockPolicy` is
unchanged too: with hiding off the table there is nothing to gate, and a flag
that enforced nothing would lie.

`DockAction::toggled(signal)` is **reflect-only** — the rail never writes it, so
a derived signal is safe (unlike `IconButton::toggle`, which flips its signal on
click). `on_activate` owns every write.

**`DockRail::leading_slot` / `trailing_slot` — bar slots for a Strip side.**
`top_slot` / `bottom_slot` only ever reached the Rail presentation; a side
showing its in-side tab strip had no app-facing slot at all, even though
`TabWidget` has had `bar_leading_slot` / `bar_trailing_slot` all along. Two
things had to be fixed for that to be usable rather than a footgun:

- `TabWidget`'s bar slot is a single last-write-wins field, and `DockSidePanel`
  already spends the trailing one on its "hidden activities" hamburger — the
  only way back once every activity on a side is hidden. The two are now
  explicitly composed, so declaring a trailing slot can no longer silently drop
  the hamburger (or vice versa).
- A side with **zero** docks returned early, before its `TabWidget` was built,
  so a configured slot silently rendered nothing — a reachable state, not
  misuse. (The same bug Qt's `QTabWidget::setCornerWidget` still has: the corner
  widget only shows while at least one tab exists.) Slots now render there too.

These carry a weaker visibility contract than the rail slots and it is
documented as such: the activity bar is built whenever the side has a rail, so
`top_slot` / `bottom_slot` survive a collapsed side, while `leading_slot` /
`trailing_slot` live inside the collapsing content region and disappear with it.

### Fixed — `teksilo-widgets` docking: rail accessibility

**The activity rail was an invalid ARIA `tablist`.** `DockActivityBar` set
`Role::TabList` on its whole root, so `top_slot` / `bottom_slot` widgets and the
overflow trigger were non-`Role::Tab` children of a tablist — which ARIA's Tabs
pattern forbids (Required Owned Elements), and which real screen readers
navigate poorly. The role now sits on a `DockRailTabList` wrapping **only** the
items; the slots, the overflow trigger and the new action clusters are siblings,
and the rail root is a property-free `Role::GenericContainer` that the AT pass
prunes. Restricting the AT children instead (via `accessibility_children`) would
have deleted those controls from the tree rather than re-parenting them, turning
a spec violation into a WCAG 2.1.1 failure — hence the wrapper.

No app-facing API changed for this; a rail still reports one `Role::TabList` per
side with the same localized name.

### Changed — `teksilo-settings` (breaking)

**Cross-process safety is now the default and only behaviour for every
persisted type in the crate**, not an opt-in mode. Previously,
`docs/settings.md` said — twice, deliberately — "multi-process is out of
scope; two app instances writing to the same file is last-write-wins," then
later grew an opt-in `SettingsFile::load_shared` escape hatch for callers
that needed better. Both of those are gone: `SettingsStore`,
`SettingsFile<T>`, and `PersistedListModel<T>` (and everything built on
them — `MruList<T>`, `WindowStateService`) now always merge a write against
the document read fresh under an exclusive lock, and a peer process's write
now arrives live, through the same `Signal`/`ListModel` a caller is already
bound to, via a new `notify`-based file watcher — with nothing for the
caller to remember to call, unlike Qt's `QSettings::sync()`. See
`docs/settings.md` for the full architecture (the patch/merge model, why a
lock alone isn't sufficient, the live-reload watcher, and the honest
performance trade-offs).

- **Removed:** `SettingsFile::load_shared`. `SettingsFile::load` /
  `load_strict` are unconditionally correct now — there is no longer a
  "which mode did I open this with" question to get wrong. `load` /
  `load_strict` also dropped their now-meaningless `delay: Duration`
  parameter (`SettingsFile<T>`'s writes are always synchronous).
- **Removed:** `MruList::toggle_pin`. Replaced by
  `MruList::set_pinned(key, bool)`. A toggle's effect depends on the
  *current* state at the moment it runs, which is exactly the kind of
  transient, position-dependent meaning a replayable, possibly-delayed
  write can no longer assume — `set_pinned` states the desired end state
  directly, so replaying it against a document a peer has already changed
  always lands on the same answer.
- **Removed:** `PersistedTreeModel<T>` (and its `collection::tree` module).
  Zero consumers anywhere in this workspace or in Skribisto, and it carried
  the exact whole-snapshot-clobber defect the rest of this crate was just
  hardened against. Reintroduce it ops-based, from scratch, if a real
  consumer needs a persisted tree — do not resurrect the deleted version.
- **Changed (source-compatible):** `Keyed` is a new, separate trait
  (`type Key` + `fn key(&self) -> Self::Key`, owned key) that `MruEntry`
  now requires alongside its existing pin/touch methods; existing
  `impl MruEntry` blocks need a companion `impl Keyed` (previously `Key`
  and `key()` lived directly on `MruEntry`).
- **Source-compatible, despite the headline:** `SettingsFile::mutate`,
  every `SettingsStore` signal (`signal`/`signal_for`/`.set()`), and
  `MruList::add`/`touch`/`remove`/`clear` keep their existing call-site
  shape. "Cross-process safe by default" sounds like it should cost
  callers something; it doesn't — the correctness moved into the write
  path and the new live-reload watcher, not into new ceremony at the
  call site.

### Changed — `teksilo-widgets` (breaking)

- `NotificationArchiveModel::remove(index: usize)` → `remove_by_id(id: u64)`.
  Same reasoning as `set_pinned` above: an index names a position in this
  process's *current* view of the list, which a peer's concurrent insert
  (or this crate's own debounce delay) can invalidate before the removal
  actually runs. An id is stable identity regardless of how many
  neighboring rows moved in the meantime.

### Added — `teksilo-data`

- `ListModel::reconcile_by_key(new_items, key_fn)`: diffs the live model
  against a new authoritative `Vec<T>` by key and emits the minimal
  granular `DataChange`s (coalesced removes/inserts, single-row moves only
  where something is actually out of place, updates where content
  differs) — and **never** a blanket `Reset`, which would otherwise clear
  a user's positional selection every time a peer's settings write landed.
  This is the primitive `PersistedListModel<T>`'s live-reload path is
  built on.
- `adjust_single_index_for_change` (in `data_change.rs`) and its use in
  `ListView`'s focused-index tracking, so a single-selection widget keeps
  its focus pointed at the right row across a reload-driven reconciliation,
  not just across ordinary user-driven inserts/removes.

## Earlier history

Entries before this file was introduced are not backfilled; see `git log`
for the full history.
