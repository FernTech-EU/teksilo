<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# kde_golden: reference data from a real KDE Plasma

This directory produces the reference ("oracle") vectors that the KDE Breeze
preset (`teksilo-kde-scheme`, `teksilo-theme-kde`) is tested against. The
vectors come from a real KDE Plasma 6.8 / Frameworks 6.31 installation running
in Docker, driven by a small C++ program that calls only public Qt 6 and KDE
Frameworks 6 API, plus the `plasma-apply-colorscheme` command-line tool.

- **Nothing here runs in CI.** The vectors are generated on a maintainer
  machine with `run.sh` and committed.
- **Nothing here is packaged.** `tools/` sits outside every crate directory,
  so no `cargo package` can include it (the planned package-size guard also
  asserts that tools and vectors are absent from the `.crate` files).
- **No KDE source and no `.colors` file is in this directory.** The image has
  the installed files; the driver and the scripts read them at run time. The
  vectors embed the *parsed key/value pairs* of each input scheme so a test
  never needs the file. Those values come from files under their own licences:
  `BreezeLight.colors` and `BreezeDark.colors` carry
  `LGPL-2.0-or-later` headers; `BreezeClassic.colors` (package
  `kde-style-breeze`) and the Oxygen files (package `kde-style-oxygen`) carry
  no SPDX header, see their Debian copyright files in the image.

## Files

| File | What |
|---|---|
| `Dockerfile` | the image: KDE neon pinned by digest, upgraded to Plasma 6.8 / KF 6.31, build deps, the driver built in |
| `driver/` | the MPL-2.0 C++ driver (`kde_golden`), CMake project |
| `run.sh` | on the host: builds the image, runs itself inside it, writes `vectors/` |
| `accent_oracle.sh` | inside the container: `plasma-apply-colorscheme -a` cases into `vectors/accent.json` |
| `vectors/math.json` | `KColorUtils` and static `KColorScheme::shade` over a lattice, plus the Qt colour behaviour they rest on |
| `vectors/resolve.json` | `KColorScheme` and `createApplicationPalette` over 13 schemes |
| `vectors/sizes.json` | Breeze `QStyle` metrics, `sizeFromContents`, sub-rects, widget `sizeHint` at 6 font sizes |
| `vectors/pixels.json` | Breeze-painted controls sampled along fixed lines, Light and Dark |
| `vectors/accent.json` | `kdeglobals` written by `plasma-apply-colorscheme` for 40 scheme x accent cases |
| `vectors/versions.txt` | `dpkg-query` package/version list from the image |
| `vectors/fonts.sha256` | sha256 of the Noto Sans and Hack faces the image measured with |

## Provenance

**Base image.** `invent-registry.kde.org/neon/docker-images/plasma:user`,
pulled 2026-10-16, pinned by digest:

```
invent-registry.kde.org/neon/docker-images/plasma@sha256:f58f9b97f218ee976cf928f6fe00abb929a644aded8c9dfb02f77a9b2a6e1732
```

That tag (image created 2026-09-10) ships KDE neon User Edition on Ubuntu
24.04 with Plasma 6.7.4, Frameworks 6.29 and Qt 6.11.1. No published neon image
had Plasma 6.8 yet, so the Dockerfile runs `apt-get full-upgrade` against
neon's user archive (`http://archive.neon.kde.org/user`, suite `noble`), which
on 2026-10-16 carried Plasma 6.8.0 and Frameworks 6.31.0.

**Versions obtained** (also in `vectors/versions.txt` and in every file's
`meta.packages`):

| Package | Version |
|---|---|
| plasma-workspace | 4:6.8.0-0zneon+24.04+noble+release+build99 |
| kde-style-breeze | 4:6.8.0-0zneon+24.04+noble+release+build80 |
| breeze | 4:6.8.0-0zneon+24.04+noble+release+build80 |
| kde-style-oxygen | 4:6.8.0-0zneon+24.04+noble+release+build59 |
| kf6-kcolorscheme | 6.31.0-0zneon+24.04+noble+release+build47 |
| kf6-kguiaddons | 6.31.0-0zneon+24.04+noble+release+build59 |
| kf6-kconfig | 6.31.0-0zneon+24.04+noble+release+build36 |
| qt6-base | 6.11.1-0zneon+24.04+noble+release+build162 |
| fonts-noto-core | 20201225-2 (Ubuntu) |
| fonts-hack | 3.003-3 (Ubuntu) |

The driver reports the same at run time: `qt_runtime` 6.11.1,
`kcolorscheme_build` and `kguiaddons_build` 6.31.0.

**Reproducibility.** Two consecutive `run.sh` runs produce byte-identical
vectors. A rebuild later may not reach the same packages: neon keeps only the
newest build of each package, so the upgrade step cannot be pinned more tightly
than the Dockerfile's check (Plasma 6.8.x, Frameworks 6.31.x; the build fails
otherwise). Compare `vectors/versions.txt` before and after a regeneration.

**Run environment** (set by `run.sh --inside`):

- `QT_QPA_PLATFORM=offscreen`; no platform theme (`QT_QPA_PLATFORMTHEME`,
  `QT_STYLE_OVERRIDE`, `KDE_FULL_SESSION`, `DISPLAY` unset); `LC_ALL=C.UTF-8`.
- Every step gets fresh scratch `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`,
  `XDG_CACHE_HOME`, `XDG_RUNTIME_DIR`, and an **empty** `XDG_CONFIG_DIRS`, so
  `/etc/xdg` is never read. `XDG_DATA_DIRS=/usr/local/share:/usr/share` (where
  the installed schemes live). No `breezerc` exists, so the Breeze style runs on
  its defaults.
- The offscreen screen reports logical DPI 96, physical DPI 100, device pixel
  ratio 1.
- Fonts: Noto Sans and Hack from the Ubuntu packages above (sha256 in
  `vectors/fonts.sha256`). Teksilo's bundled faces may be newer releases.

## Regenerating

```bash
tools/kde_golden/run.sh
```

It needs docker without sudo and network access to the two registries. It
never touches the host's KDE configuration: everything runs in the container
as the invoking uid, with scratch XDG directories, and writes only
`tools/kde_golden/vectors/`. `accent_oracle.sh` additionally refuses to run
unless `XDG_CONFIG_HOME` and `XDG_DATA_HOME` are set, absolute, and each either
empty or marked with a `.kde_golden_scratch` file, because
`plasma-apply-colorscheme` rewrites `kdeglobals`. Never run either script
outside the container.

## Encodings shared by every file

**Input colour** (a string): `"#rrggbbaa"`, 8 bits per channel. The driver
builds it with `QColor::fromRgb(r, g, b, a)`, so its 16-bit value is exactly
`v * 257`.

**Output colour** (an object):

```json
{"hex": "#rrggbbaa", "u16": [r, g, b, a]}
```

- `u16`: the QColor's own 16-bit channels (`QColor::rgba64()`), 0..65535. This
  is exact: QColor stores RGB colours as 16-bit integers, so any floating
  result KColorUtils or KColorScheme computes has already been quantised to 16
  bits by QColor before it reaches the file.
- `hex`: Qt's 8-bit accessors (`red()`, `green()`, `blue()`, `alpha()`), which
  round the 16-bit value divided by 257 to the nearest integer
  (`floor((q + 128) / 257)`; checked on all 65536 values).
- `spec` and `f` (four exact doubles) appear only when the colour is not stored
  as `QColor::Rgb`. Only `math.json`'s `qt_colorspace` and `qt_extended`
  members contain such colours (`ExtendedRgb`, held in half precision).
- `{"valid": false}` is an invalid QColor (for example an empty value in a
  scheme file).

**Scalars** (luma, contrast ratios, HCY components, amounts) are JSON numbers
written by `QJsonDocument` with the shortest decimal that round-trips the
double, so parsing them as `f64` gives back the exact value. `1.0` may appear
as `1`.

**Rectangles and sizes**: `[x, y, width, height]` and `[width, height]`, in
logical pixels (device pixel ratio 1).

**Line layout.** Each file is one JSON object. Arrays that hold records are
written one record per line, so a regeneration diffs line by line. Object keys
inside a record are sorted.

**Comparing from Rust.** Colours: compare `u16` exactly, or allow 1 unit per
channel where an implementation quantises at a different step, and require
`hex` to be equal. Scalars: within 1e-6.

Every file starts with `meta`:

| Field | Meaning |
|---|---|
| `kind` | `math`, `resolve`, `sizes`, `pixels` or `accent` |
| `generator` | what wrote it |
| `image` | base image reference with digest |
| `packages` | package name to version, from `/opt/kde_golden/versions.txt` |
| `qt_runtime`, `qt_build` | Qt version at run time and at build time (driver files only) |
| `kcolorscheme_build`, `kguiaddons_build` | KF versions the driver was built against (driver files only) |
| `readme` | pointer to this file |

## `math.json`

Calls `KColorUtils::{luma, hue, chroma, getHcy, hcyColor, contrastRatio,
lighten, darken, shade, tint, mix, overlayColors}` and the static
`KColorScheme::shade(color, role, contrast, chromaAdjust)`.

| Member | Content |
|---|---|
| `colors` | the 62 input colours (black, white, greys around the ends and around 0x7f/0x80, primaries, secondaries, half and pastel variants, near-grey and near-black/white values, colours from the Breeze schemes, the Plasma accent swatches, 4 translucent colours) |
| `pair_colors` | the 17-colour subset used for two-colour operations |
| `single` | one record per `colors` entry: `in`, `luma`, `hue`, `chroma`, `hcy` = `[h, c, y, a]` from `getHcy`, `roundtrip` = colour from `hcyColor(h, c, y, a)` |
| `contrast_ratio` | `a` (each of `pair_colors`) x `b` (each of `colors`): `out` = `contrastRatio(a, b)` |
| `lighten` | `in` x `amount` in {-0.5, 0, 0.1, 0.25, 0.5, 0.75, 0.9, 1, 1.5} x `gain` in {0, 0.5, 1, 2}: `out` = `lighten(in, amount, gain)` (gain is `chromaInverseGain`) |
| `darken` | same lattice: `out` = `darken(in, amount, gain)` (gain is `chromaGain`) |
| `shade` | `in` x `luma` in {-1.5, -1, -0.5, -0.1, 0, 0.1, 0.5, 1, 1.5} x `chroma` in {-1, 0, 0.5, 1}: `out` = `shade(in, luma, chroma)` |
| `tint` | `base` x `color` (both `pair_colors`) x `amount` in {-0.5, 0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.7, 1, 1.5}: `out` = `tint(base, color, amount)` |
| `mix` | `c1` x `c2` (both `pair_colors`) x `bias` in {-0.5, 0, 0.1, 0.25, 0.5, 0.75, 0.9, 1, 1.5}: `out` = `mix(c1, c2, bias)` |
| `overlay` | `base` x `paint` (both `pair_colors`) x `mode` in {`SourceOver`, `Multiply`, `Screen`}: `out` = `overlayColors(base, paint, mode)` |
| `hcy_color` | `h` in {-0.25, 0, 0.1, 1/6, 0.5, 0.9, 1, 1.25} x `c` in {-0.5, 0, 0.25, 0.5, 1, 1.5} x `y` in {-0.1, 0, 0.1, 0.5, 0.9, 1, 1.1} x `a` in {1, 0.5}: `out` = `hcyColor(h, c, y, a)` |
| `scheme_shade` | `in` x `role` (`Light`, `Midlight`, `Mid`, `Dark`, `Shadow`) x `contrast` in {-1, 0, 0.4, 0.7, 1, 1.5} x `chroma` in {0, 0.5}: `out` = `KColorScheme::shade(in, role, contrast, chroma)` |
| `qt_lighter_darker` | `in` (each of `colors`) x `factor` in {-10, 0, 1, 50, 99, 100, 101, 102, 110, 120, 150, 200, 300, 1000} x `op` in {`lighter`, `darker`}: `out` = `QColor::lighter(factor)` / `darker(factor)` |
| `qt_colorspace` | `in` = each of `colors`, the 256 greys and the reds `#010000` to `#ff0000`: `to_linear` and `from_linear` = Qt's `QColorTransform` from `QColorSpace::SRgb` to `SRgbLinear` and back, applied to the `QColor` |
| `qt_extended` | `in` = three floats, some outside 0..1: `stored` = `QColor::fromRgbF(in)` (an `ExtendedRgb` colour, with its half-precision values in `f`), `to_rgb` = its `toRgb()` |
| `qt_color_text` | `in` = a colour string: `out` = `QColor::fromString(in)` (hex forms of 3, 6, 8, 9 and 12 digits, invalid lengths, names in several cases, `transparent`, unknown names, an empty string) |

Out-of-range amounts are deliberate: they pin the clamping behaviour.

## `resolve.json`

| Member | Content |
|---|---|
| `missing_installed` | installed schemes the image lacked (empty in this run) |
| `schemes` | one record per scheme, one per line |

Each scheme record:

| Field | Meaning |
|---|---|
| `name` | scheme name |
| `origin` | `installed` (read from `/usr/share/color-schemes/`) or `synthetic` (written by the driver) |
| `source` | installed only: the file path in the image |
| `purpose`, `source_text` | synthetic only: what the scheme isolates, and its complete file text (MPL-2.0, original values) |
| `sha256` | sha256 of the scheme file as read; for the Breeze files it identifies the upstream file to vendor as a fixture |
| `input` | the parsed scheme: group name to `{key: value}`. Nested groups are spelled as in the file, `"Colors:Header][Inactive"`. Values are KConfig's parsed strings (surrounding whitespace removed; for a duplicated key or group, what KConfig kept). Localised keys (`Name[de]`) are reduced to the untranslated value |
| `contrastF` | `KColorScheme::contrastF(config)` |
| `frameContrast` | `KColorScheme::frameContrast(config)` |
| `set_supported` | set name to `KColorScheme::isColorSetSupported(config, set)` |
| `sets` | set name (`View`, `Window`, `Button`, `Selection`, `Tooltip`, `Complementary`, `Header`) to state (`Active`, `Inactive`, `Disabled`) to the four role groups below |
| `palette` | state to QPalette role name to colour, from `KColorScheme::createApplicationPalette(config)`; roles are every `QPalette::ColorRole` below `NColorRoles` except `NoRole` (Qt 6.11 includes `Accent`) |

Role groups inside `sets.<set>.<state>`, each from
`KColorScheme(state, set, config)`:

| Group | Keys | Call |
|---|---|---|
| `background` | `Normal`, `Alternate`, `Active`, `Link`, `Visited`, `Negative`, `Neutral`, `Positive` | `background(role).color()` |
| `foreground` | `Normal`, `Inactive`, `Active`, `Link`, `Visited`, `Negative`, `Neutral`, `Positive` | `foreground(role).color()` |
| `decoration` | `Focus`, `Hover` | `decoration(role).color()` |
| `shade` | `Light`, `Midlight`, `Mid`, `Dark`, `Shadow` | `shade(role)` |

Config objects are opened with `KSharedConfig::openConfig(path,
KConfig::SimpleConfig)`, so nothing cascades into them.

Schemes, in file order: `BreezeLight`, `BreezeDark`, `BreezeClassic`,
`Oxygen`, `OxygenCold`, `OxygenDark` (installed), then the synthetic
`SyntheticNoHeader` (no Header group, no effect groups), 
`SyntheticInactiveEffects` (dark, Header without an Inactive subgroup, Inactive
effects enabled with the selection change, every effect kind with non-zero
amounts, `contrast=2`, `frameContrast=0.35`), `SyntheticInactiveSubgroups`
(Inactive effects without the selection change, explicit Inactive subgroups for
Header, View and a partial one for Window), `SyntheticAccentKeys` (the
`[General]` accent keys and a `[WM]` group, few sets), `SyntheticEmpty` (an
empty file), `SyntheticMalformed` (hex, alpha, spaces, a named colour, too
few components, out-of-range components, an empty value, `contrast=15`, an
out-of-range effect id, a duplicated key and a duplicated group) and
`SyntheticValueForms` (an empty boolean, `[ColorEffects:Inactive] Enable=`;
a signed component; 3-, 9- and 12-digit hex; names in other cases;
`transparent`; a trailing comma; a hex component; the unknown CSS name
`rebeccapurple`).

Behaviours these record, for orientation (the data is the authority):
a duplicated key keeps the last value; a duplicated group merges, later values
winning; an empty colour value resolves to an invalid colour; an empty
boolean reads as true; `contrastF` is
not clamped (1.5 for `contrast=15`); the `[General]` accent keys do not reach
KColorScheme; Header falls back to Window when the scheme has no Header group.

## `sizes.json`

All values come from the Breeze style (`QStyleFactory::create("Breeze")`,
installed as the application style) under the offscreen platform, with the
default `kdeglobals` set to BreezeLight. Every measured widget except a `QMenu`
is created hidden with a plain `QWidget` parent (Breeze answers some metrics
differently for a top-level window) and polished before it is measured.

| Member | Content |
|---|---|
| `environment` | `style`, `platform`, `logical_dpi`, `physical_dpi`, `device_pixel_ratio`, the Qt default font before the driver overrides it, and a note on parenting |
| `pixel_metrics` | every `QStyle::PixelMetric` name to `pixelMetric(pm, nullptr, nullptr)` |
| `widget_pixel_metrics` | widget class to the pixel metrics whose value changes when the style is given that widget and an option `initFrom` it (only the differing ones; `{}` means none differ). Key example: `QLineEdit.PM_DefaultFrameWidth` is 6 while the widget-less answer is 0 |
| `style_hints` | every `QStyle::StyleHint` name to `styleHint(sh, nullptr, nullptr, nullptr)` |
| `sub_rects` | sub-control and sub-element rectangles at the 10 pt font, keyed by control; each object has the input `rect` plus one rectangle per sub-control, and extra inputs (`range`, `page_step`, `value`) where relevant. Controls: combo box (normal and editable, 150 x 32), spin box (100 x 32), vertical scroll bar at values 0, 50, 100 (`PM_ScrollBarExtent` x 200, range 0..100, page 20), horizontal slider with ticks 0 (none), 2 (below), 3 (both) at 200 wide and value 30, check box and radio (at their size hint), push button, line edit, progress bar (200 wide), item-view item (200 x 24, check + icon + text), tab (80 x 30) |
| `size_from_contents` | records `{id, option, contents, out}`: `out` = `sizeFromContents(type, option, contents, widget)` for content sizes [0,0], [20,16], [60,18], [100,24], [100,32]. `id` is the contents type plus a variant; `option` lists the option fields set beyond `initFrom(widget)` (enum values are Qt's integers: `type` is `QStyleOptionMenuItem::MenuItemType`, `check_type` is `CheckType`, `shape` is `QTabBar::Shape`, `orientation` is `Qt::Orientation`, `ticks` is `QSlider::TickPosition`) |
| `monospace_fonts` | font records for Hack at 9, 10, 11 pt |
| `fonts` | font records for Noto Sans at 10, 8, 9, 11, 12, 14 pt (the order the driver measured them) |
| `widgets` | widget records, one per widget per font |
| `legacy_font_weights` | `legacy` 0..99 in field 4 of a Qt 5 font string (`Noto Sans,10,-1,5,W,0,0,0,0,0`): `weight` = the OpenType weight of `QFont::fromString` |
| `tooltip` | the label `QToolTip::showText` shows for "Tooltip text" at Noto Sans 10 pt: its class, `margin`, `contents_margins`, `size_hint` and the text's `QFontMetrics::size` |

A **font record**: `requested_family`, `requested_point_size`,
`resolved_family`, `resolved_style`, `resolved_point_size`,
`resolved_pixel_size`, `exact_match` (from `QFontInfo`), `to_string`
(`QFont::toString`), `metrics` (integer `QFontMetrics`: `height`, `ascent`,
`descent`, `leading`, `line_spacing`, `x_height`, `cap_height`,
`average_char_width`, `max_width`), `metrics_f` (the same from
`QFontMetricsF`), `advance` (string to `[QFontMetrics::horizontalAdvance,
QFontMetricsF::horizontalAdvance]`). At 96 DPI Noto Sans 10 pt resolves to a
13 px face (`resolved_point_size` 9.75, `exact_match` false) whose
`QFontMetrics::height` is 18.

A **widget record**:

| Field | Meaning |
|---|---|
| `id` | what was measured; a suffix names the variant (`push_button.default`, `tool_button.auto_raise.22`, ...). Several records can share an `id` when only `config` differs (the three `push_button` texts) |
| `class` | the Qt class |
| `font` | `"Noto Sans <pt>"`, the application font at measurement |
| `config` | the properties set on the widget (text, icon, flags; enum values as Qt integers: `style` is `Qt::ToolButtonStyle`, `popup_mode` is `QToolButton::ToolButtonPopupMode` or -1 for no menu, `shape` is `QTabBar::Shape`) |
| `size_hint`, `minimum_size_hint` | `QWidget::sizeHint()` and `minimumSizeHint()`; `[-1, -1]` is Qt's invalid size |
| `rows` | menus only: one entry per action, `{kind, text, icon, geometry}`, `kind` in `item`, `shortcut`, `separator`, `section`, `checkable`, `submenu`, `geometry` = `QMenu::actionGeometry` |
| `tab_rects` | tab bars only: `QTabBar::tabRect(i)` after `resize(sizeHint())`, tabs "Tab", "Second tab", "Third", `expanding` off |
| `section_size_hint`, `default_section_size`, `minimum_section_size` | headers: `QHeaderView` values for sections 0 and 1 of a two-column model ("Name", "Size") |
| `size_hint_for_row` | item views: `QAbstractItemView::sizeHintForRow(0)` for a two-row model, with a 16 px icon (`.icon`) or a check box (`.check`) |
| `indentation`, `header_height` | tree views: `QTreeView::indentation()` and its header's size hint height |

Icons are a solid 64 x 64 pixmap scaled by Qt to the requested icon size.

## `pixels.json`

Controls painted by the Breeze style with a hand-built `QStyleOption` and **no
widget** (so no animation engine runs and window activation is explicit), into
a `QImage::Format_ARGB32` image of the given size that is first filled opaque
with the palette's background colour. No text, icon or label is painted:
these vectors pin chrome, not glyph rasterisation.

The palette is `KColorScheme::createApplicationPalette` of the process's
`kdeglobals`, which `run.sh` sets to the scheme being sampled; the Breeze style
also reads its hover and focus colours from that `kdeglobals`. The font is
Noto Sans 10 pt.

| Member | Content |
|---|---|
| `schemes` | `BreezeLight` and `BreezeDark`, each `{environment, records}` |
| `schemes.<s>.environment` | `scheme`, `kdeglobals_color_scheme` (what the style saw), `style`, `platform`, `image_format` |
| `schemes.<s>.records` | one record per painted control state, one per line |

A record:

| Field | Meaning |
|---|---|
| `id` | control and state, `push_button.hover` |
| `control` | the QStyle element painted (`CE_PushButtonBevel`, `CC_ComboBox`, `PE_PanelItemViewItem`, ...) |
| `state` | `name`; `flags`, the extra `QStyle::State` bits as an integer (`State_Enabled` is added unless `group` is `Disabled`, `State_Active` unless `group` is `Inactive`); `group`, the palette colour group (`Active`, `Inactive`, `Disabled`); and per-control inputs (`features` = `QStyleOptionButton::ButtonFeatures`, `editable`, `value`, `handle_rect`, `slider_rect`, `active_sub_control`, `type`, `document_mode`, `line_width`) |
| `size` | `[width, height]` of the image, which is also the option rectangle |
| `background` | the fill colour painted first (`Window`, or `Base` for header and item-view rows, `ToolTipBase` for the tooltip) |
| `rows` | y (as a string) to the run-length-encoded row of pixels at that y |
| `cols` | x (as a string) to the run-length-encoded column at that x |

Run-length encoding: a list of `[start, length, "#rrggbbaa"]`, covering the
whole line in order. All pixels are opaque.

Sampled lines per control are chosen to cross the outline, the fill and the
indicator: the outer two or three rows and columns, the centre, and for
sliders and scroll bars the line through the handle (its rectangle is in
`state`). Controls: push button (normal, hover, focus, pressed, default,
disabled, inactive window, flat, flat hover; 84 x 34), auto-raise tool button
(normal, hover, pressed, checked, framed; 28 x 28), line edit panel (normal,
hover, focus, disabled; 150 x 32, frame width from a real `QLineEdit`), check
box indicator (off, on, partial, hover, focus, pressed, disabled), radio
indicator (off, on, hover, focus, disabled), combo box (normal, hover, focus,
editable), spin box (normal, hover), slider (normal, handle hover, focus,
disabled; value 30), vertical scroll bar (normal, hover, slider hover, slider
pressed; value 0, page 20), progress bar at 50 %, menu panel, menu item
(normal, selected, separator), tab shape (selected, unselected, hover, and
document mode), header section (normal, hover, sunken), item-view row
(normal, hover, selected, selected and hovered, selected in an inactive
window, focus), tooltip panel, styled frame, group box frame.

## `accent.json`

What `plasma-apply-colorscheme` writes to `kdeglobals`. Per case, in its own
scratch config directory and its own `dbus-run-session`:

1. `kdeglobals` is seeded with only `[General] ColorScheme=KdeGoldenNone`, a
   name no installed scheme has: the tool refuses to apply the scheme that is
   already current, and an unset `ColorScheme` reads as `BreezeLight`;
2. `plasma-apply-colorscheme SCHEME` applies the scheme (snapshot `base`);
3. `plasma-apply-colorscheme -a ACCENT` applies the accent. With `-a` the tool
   ignores a scheme argument and re-applies the *current* scheme with the
   accent.

Cases: the 10 Plasma swatches (`#e93a9a #e93d58 #e9643a #e8cb2d #3dd425
#00d3b8 #3daee9 #b875dc #926ee4 #686b6f`) x 4 schemes: `BreezeLight`,
`BreezeDark`, `KdeGoldenTintLight` (BreezeLight plus `[General]
TintFactor=0.25`) and `KdeGoldenTitlebarDark` (BreezeDark plus `[General]
TitlebarIsAccentColored=true`). The two derived schemes are built in the
scratch data directory with `kwriteconfig6`, which also sets their `Name` and
`ColorScheme` to the derived name.

| Member | Content |
|---|---|
| `meta` | `kind`, `generator`, `image`, `packages`, `procedure`, `readme` |
| `schemes` | scheme name to its record, one per line |
| `cases` | one record per case, one per line |

A scheme record:

| Field | Meaning |
|---|---|
| `origin` | `installed` or `derived` |
| `source` | the installed file path, or `derived (scratch data dir)` |
| `derived_from`, `changes` | derived only: the installed scheme and the `[General]` keys `kwriteconfig6` set |
| `sha1` | sha1 of the scheme file; the tool writes it to `[General] ColorSchemeHash` |
| `sha256` | sha256 of the scheme file |
| `input` | the scheme file parsed as group to `{key: raw value}` (localised `Name[xx]` keys dropped, `[$x]` flags kept in the key) |
| `base` | `kdeglobals` after step 2 (no accent), same parsing |
| `base_sha256` | sha256 of that file |
| `base_differs_between_cases` | present (true) only if step 2 ever produced a different file for the same scheme; absent in this run |

A case record:

| Field | Meaning |
|---|---|
| `scheme`, `accent` | the inputs; `accent` exactly as passed to `-a` |
| `commands` | the two invocations |
| `stdout` | the tool's message for step 3 |
| `kdeglobals` | `kdeglobals` after step 3, parsed as group to `{key: raw value}`; this is the byte-for-byte target for `with_accent` |
| `kdeglobals_sha256` | sha256 of that file |
| `config_files` | every file the tool left in the config directory (only `kdeglobals` in this run) |

Behaviours these record, for orientation: the tool writes no `AccentColor`
key; `[General] ColorScheme` is removed when it equals the default
(`BreezeLight`); `[General] ColorSchemeHash` is the sha1 of the scheme file.
