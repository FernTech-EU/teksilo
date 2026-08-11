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
