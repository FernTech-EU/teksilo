<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SeriesPattern

`SeriesPattern` — the non-colour channel that identifies a chart series.

## Public types

| Kind | Name |
| ---: | :--- |
| `enum` | [`SeriesPattern`](#seriespattern) — The non-colour visual channel identifying one chart series |
| `enum` | [`SeriesMarker`](#seriesmarker) — The marker glyph drawn at a line chart's data points, and next to a series in a legend |
| `enum` | [`SeriesHatch`](#serieshatch) — How a filled region (a bar, an area, a pie slice) carries its series' pattern |

## Public functions

### `SeriesPattern`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`for_index(index: usize)`](#seriespattern-for_index) |
| | **Methods** |
| `Option<(f32, f32)>` | [`dash(line_width: f32)`](#seriespattern-dash) |
| `SeriesMarker` | [`marker()`](#seriespattern-marker) |
| `SeriesHatch` | [`hatch()`](#seriespattern-hatch) |
| | **Constants and types** |
| `[SeriesPattern` | [`ALL`](#seriespattern-all) |

## Detailed description

A chart that tells its series apart by colour and nothing else fails WCAG
1.4.1 (Use of Color), *whatever* palette it uses. A CVD-safe palette like
Okabe–Ito answers a different question — whether the colours are
distinguishable from one another — and does not answer this one: a reader
with monochrome vision, a monochrome printout, a display in bright sun, or a
forced-colours setting has no colour channel at all. It also does not answer
the wrap-around problem, where a ninth series repeats the first's colour
exactly.

So every series carries a second, orthogonal identity: a **pattern**. One
value drives all three renderings a chart needs, so a series looks like
*itself* whether it is drawn as a line, a bar, a slice, or a legend swatch:

| | line | marker | filled area |
| --- | --- | --- | --- |
| `Solid` | solid | circle | plain |
| `Dashed` | long dash | square | 45° hatch |
| `Dotted` | dotted | triangle | back-hatch |
| `DashDot` | dash-dot | diamond | cross-hatch |
| `ShortDash` | short dash | cross | horizontal |
| `WideDash` | wide dash | plus | vertical |

Six patterns against the theme palette's eight colours means the pair
`(colour, pattern)` does not repeat until the 24th series — where colour
alone repeated at the 9th.

A series with no explicit pattern is assigned one from its position by
`SeriesPattern::for_index`, so the channel exists without any application
code. Whether a chart *draws* it is the chart's decision (the stock charts
draw it once more than one series is visible, since a single-series chart
has nothing to disambiguate).

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/series_pattern/index.html)

<a id="seriespattern"></a>

## `pub enum SeriesPattern`

The non-colour visual channel identifying one chart series.

See the `module docs` for the rendering table and the reasoning.

```rust
pub enum SeriesPattern { /* variants */ }
```

### Variants

- **`Solid`** — Unbroken line, round marker, plain fill.
- **`Dashed`** — Long dash, square marker, forward (45°) hatch.
- **`Dotted`** — Dotted line, triangular marker, back (135°) hatch.
- **`DashDot`** — Dash-dot line, diamond marker, cross-hatch.
- **`ShortDash`** — Short dash, ×-shaped marker, horizontal hatch.
- **`WideDash`** — Wide-spaced dash, +-shaped marker, vertical hatch.

### Methods

<a id="seriespattern-all"></a>

#### `pub const ALL: `SeriesPattern;`

Every pattern, in assignment order. The order is the cycle
[`for_index`` walks.

<a id="seriespattern-for_index"></a>

#### `pub fn for_index(index: usize) -> Self`

The pattern a series at `index` gets when it declares none.

Wraps, like `ChartPalette::color_for` does
with colours — but at a different period (6 against the theme palette's
8), so the wrap points do not coincide and `(colour, pattern)` stays
unique far longer than either channel alone.

<a id="seriespattern-dash"></a>

#### `pub fn dash(self, line_width: f32) -> Option<(f32, f32)>`

The dash pattern for a stroked line, as `(dash, gap)` in logical
pixels, or `None` for an unbroken line.

Scaled by `line_width` so a 1 dp line and a 4 dp line read as the same
pattern rather than the thick one looking almost solid.

<a id="seriespattern-marker"></a>

#### `pub fn marker(self) -> SeriesMarker`

The marker glyph for this pattern.

<a id="seriespattern-hatch"></a>

#### `pub fn hatch(self) -> SeriesHatch`

The hatch for a filled region carrying this pattern.

<a id="seriesmarker"></a>

## `pub enum SeriesMarker`

The marker glyph drawn at a line chart's data points, and next to a series
in a legend. Shape, not colour — that is the whole point.

```rust
pub enum SeriesMarker { /* variants */ }
```

### Variants

- **`Circle`**
- **`Square`**
- **`Triangle`**
- **`Diamond`**
- **`Cross`**
- **`Plus`**

<a id="serieshatch"></a>

## `pub enum SeriesHatch`

How a filled region (a bar, an area, a pie slice) carries its series'
pattern. `None` is a plain fill; the rest are line hatches at the named
angle, drawn in a contrasting tone over the fill.

```rust
pub enum SeriesHatch { /* variants */ }
```

### Variants

- **`None`** — No hatch — a plain fill.
- **`Forward`** — Parallel lines rising to the right (45°).
- **`Backward`** — Parallel lines falling to the right (135°).
- **`Cross`** — Both diagonals.
- **`Horizontal`** — Parallel horizontal lines.
- **`Vertical`** — Parallel vertical lines.
