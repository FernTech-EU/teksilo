<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Native theme

An app can let the desktop decide how it looks. `ThemeMode` says how:

| Mode | The theme is | Set with |
| --- | --- | --- |
| `Manual` (default) | the one the app picked | `.theme(intui::light())` |
| `FollowSystem` | Int UI light or dark, as the desktop's light or dark preference says | `.theme_mode(ThemeMode::FollowSystem)` |
| `Native` | Int UI in the desktop's own colours, or a theme of the app's own making | `.theme_mode(ThemeMode::Native)` |

At runtime a handler switches to `Native` with `ctx.follow_system_theme()`, which
is what the "System" entry of a [`ThemeSwitcher`](widgets/theme_switcher.md)
calls, and back to `Manual` with `ctx.set_theme(..)`.

This page is about `Native`: what it reads on each platform, how an app gives it
shapes as well as colours, and what it does when the desktop changes.

## What Native mode reads

`teksilo_platform::os_theme::query_native_desktop()` answers with a
`NativeDesktop`: `os`, the colour report (`OsThemeColors`: light or dark
preference, accent, window, button, selection and tooltip colours, the
content-view colours, links and status colours), and `kde`, the KDE desktop
itself on a KDE Plasma session (`None` elsewhere). Every colour is an
`Option`; a colour the desktop does not state is left to the Int UI default.

| Platform | What the report holds |
| --- | --- |
| Linux, KDE Plasma | everything below |
| Linux, GNOME | light or dark and the accent colour from the desktop portal (the GNOME named accent as a fallback), and the surface, selection and tooltip colours of the active GTK theme's CSS |
| Linux, Cinnamon | the accent inferred from a Mint-Y theme's name, and the colours of the GTK theme |
| macOS, Windows | only a light or dark state, from the window system (`window.theme()`); no colours |

Where the desktop states no light or dark preference of its own (macOS,
Windows), the window system's state decides, and a window with none reads as
light. Where it states one, the desktop wins.

### The KDE reader

A session is a KDE session when `XDG_CURRENT_DESKTOP` lists `KDE` or
`KDE_FULL_SESSION` is `true`. The reader is `teksilo-kde-scheme`: it does no
I/O itself, and `teksilo-platform` hands it the environment and the contents of
the files it asks for. It reads:

- the `kdeglobals` cascade (system layers, `kdedefaults`, the user's file),
  with KDE's immutable (Kiosk) entries honoured;
- the **colour scheme in effect**: the `[Colors:*]` groups of `kdeglobals`,
  which already carry any accent the user chose; when there are none, the
  look-and-feel package's colours, else the scheme named by
  `[General] ColorScheme`, else Breeze Light, with `[General] AccentColor`
  applied to it as Plasma applies it;
- the five desktop **fonts**, `[KDE] AnimationDurationFactor` and
  `[KDE] widgetStyle`;
- the **`breezerc`** cascade: every Breeze widget-style key, carried as
  `BreezeStyleConfig` (the preset paints from the menu opacity, the window
  shadow and the busy progress bar).

The light or dark preference is KDE's own judgement of the scheme (the window
background's grey value), the rule KDE reports to applications. On a session
whose `kdeglobals` cannot be read at all, the report falls back to the
desktop portal's preference and accent and there is no `kde` value.

An animation speed of 0 (`AnimationDurationFactor`) also turns on the
reduced-motion preference. `forceFontDPI` is not read: no current Plasma
component honours it.

## From a report to a theme

When Native mode needs a theme (a window opening, the desktop changing, a
switch to Native mode) it builds a `NativeThemeRequest { desktop, dark,
density }` and calls `resolve_native_theme(factory, &request)`:

1. if the app installed a factory with `TeksiloAppBuilder::native_theme` and it
   answers `Some(theme)`, that is the theme;
2. otherwise it is `presets::intui::from_os(&desktop.os, dark)`: Int UI
   with its colours replaced by the desktop's.

`NativeThemeRequest` is `#[non_exhaustive]`; build one with
`NativeThemeRequest::new(&desktop, dark, density)`.

`resolve_native_theme` sets no id and applies no density. The window manager
does both for every theme that follows the OS (Native and FollowSystem alike):
the theme gets the id `system`, so a `ThemeSwitcher` shows "System" as selected,
and is projected onto the density of the window it is for.

A factory is for an app that wants the desktop's shapes too:

```rust,ignore
TeksiloAppBuilder::new()
    .theme_mode(ThemeMode::Native)
    .native_theme(|request| {
        // `None` keeps Int UI in the desktop's colours.
        let kde = request.desktop.kde.as_ref()?;
        my_theme_for(kde, request.dark, request.density)
    })
    .run();
```

`configured_native_theme()` returns the installed factory, for an install hook
or a test.

### `native_theme_kde()`

With the `theme-kde` feature the prelude has `TeksiloAppBuilderKdeExt`, and

```rust,ignore
TeksiloAppBuilder::new()
    .theme_mode(ThemeMode::Native)
    .native_theme_kde()
    .initial_window(WindowConfig::new().root(|tree, _state| tree.add(MyRoot::new())))
    .run();
```

installs `kde_native_theme` as the factory. It answers
`kde::native_theme(desktop, density)`: the desktop's colour scheme with the
user's accent, its fonts, its animation speed and its `breezerc` settings, in
Breeze's geometry and widget chrome. It answers **only for a Breeze desktop**:
`kdeglobals [KDE] widgetStyle` absent, empty or `Breeze`. For a KDE desktop
styled with Fusion, Oxygen or Kvantum, for a KDE desktop with no settings and
for every other desktop it answers `None`, and Native mode keeps Int UI in the
desktop's colours, because a Teksilo app in Breeze's shapes would not match the
other applications there.

Setting a factory does not select Native mode; combine it with `.theme_mode`.
`ThemeMode::FollowSystem` never asks the factory.

### Native mode and the KDE preset are two things

- **Native mode adopts the live desktop colours.** With no factory it works on
  GNOME, Cinnamon and KDE alike, and the shapes are Int UI's.
- **The KDE preset is Breeze.** `kde::light()` and `kde::dark()` are the stock
  Breeze Light and Dark in Breeze's shapes, with no desktop involved, on any
  platform. `kde::from_scheme`, `kde::from_desktop` and
  `kde::native_theme` build the theme of a given scheme or desktop. See
  [the styling system](styling-system.md#built-in-presets).

`native_theme_kde()` is the two together.

## Following the desktop while the app runs

Under `Native` and `FollowSystem`, the window manager starts an
`OsThemeWatch` (Linux; elsewhere `OsThemeWatch::new` answers `None` and the
window system's own light or dark notification is all there is). It listens on
the D-Bus session bus, on a thread of its own, for:

- the desktop portal's `SettingChanged` for the appearance namespace (colour
  scheme, accent colour, contrast, reduced motion) and for KDE's
  `org.kde.kdeglobals.*` namespaces;
- KDE's `org.kde.KGlobalSettings.notifyChange`;
- KDE's `org.kde.kconfig.notify.ConfigChanged` for `kdeglobals` and `breezerc`.

A change that arrives as several signals within a few milliseconds is reported
once, 150 ms after the first. The window manager then reads the desktop again
and gives each window the theme it now resolves to; a re-read that changes
nothing a theme was resolved from sets no theme. The reduced-motion preference is
refreshed from the same signal. On a session with no bus, or a signal that
never came, a change to `kdeglobals` or `breezerc` is found by modification time
the next time a window takes focus. Under `Manual` nothing is watched.

## Known limitation: switching re-tints, the shapes stay

A widget's chrome (which `Tier-3` style paints it) is chosen when the widget is
built, from the theme the tree has then. Changing the theme later is a colour
swap, with no rebuild, which is why it keeps focus and scroll position. So:

- a window built while the app is in Native mode asks the factory, and shows
  Breeze's shapes on a Breeze desktop;
- switching to Native mode at runtime, picking "System" in a `ThemeSwitcher`, or
  the desktop changing its colours re-tints the windows that are open, but keeps
  the shapes they were built with. If the desktop moves from Breeze to another
  widget style while the app runs, the open windows keep Breeze's shapes until
  they are built again;
- the same holds between presets: picking `kde-dark` in the title-bar switcher
  of `widget-catalog` over a tree built as Int UI changes the colours only.
  `cargo run -p widget-catalog -- --theme kde-dark` builds the tree in Breeze's
  chrome, and `--theme system` builds it in Native mode.

See [the reactive theme](reactive-theme.md) for why a theme swap never rebuilds.

## Testing

`NativeDesktop::new(OsThemeColors::new(..))` builds a report with no KDE
settings, and `KdeDesktop::new(KdeColorScheme::breeze_dark())` a KDE desktop at
Plasma's defaults, so a test can call `resolve_native_theme` or a factory
without a desktop. `teksilo-kde-scheme` takes the environment and the files as
arguments for the same reason.

### The Breeze preset in the running catalog

`tools/reader/kde_visual_check.sh` checks the KDE preset as it renders in the
real `widget-catalog`, and in Native mode on a KDE desktop. It builds the debug
catalog and `teksilo-automation-mcp`, then runs everything in a private session
(`tools/reader/private_session.sh`: its own D-Bus bus, `kwin_wayland --virtual`
and configuration home), so neither the catalog nor Plasma's tools reach the
desktop or `~/.config`. For `--theme kde-light`, `kde-dark` and `system` in
turn, it gives the private configuration a Breeze scheme with
`plasma-apply-colorscheme`, starts the catalog as on a KDE session, and measures
over the automation bridge the push button, line edit, combo box, spin box,
check box row, list and tree rows, table header and row, menu row, tab and
scroll bars. It prints each height beside Plasma 6.8's own
(`tools/kde_golden/vectors/sizes.json`), the specification's and the preset's,
with the documented deviations, and fails on a control more than 1 px from the
preset's height or missing from the catalog. In `system` mode it then switches
the desktop to Breeze Dark and to another accent while the catalog runs, and
checks that the window repaints in the new colours with the same widget tree.
It needs `kwin_wayland` and Plasma's `plasma-apply-colorscheme`, so it is not
part of `cargo test`.
