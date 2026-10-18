<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# KDE colour model and Breeze metrics: clean-room specification

This document is the authoritative specification from which Teksilo
re-implements the KDE Plasma colour model (crate `teksilo-kde-scheme`) and
transcribes the Breeze design language (crate `teksilo-theme-kde`). It
overrides the implementation plan and the synthesised design wherever they
disagree.

It is written in prose and mathematics only. It contains no upstream code,
no pseudo-code that mirrors upstream function structure, and no upstream
internal identifiers. Values are facts; behaviour is described as inputs,
outputs, order of operations and rounding. The closing ledger (section o)
records, for every number, where it was observed upstream. The ledger is a
list of references, never an excerpt.

Who may read what:

- **Role A** (the author of this file) read the upstream sources listed in
  section a.
- **Role B** (the implementer of `math.rs`, `effects.rs`, `palette.rs` and
  `accent.rs`) reads only this file, Qt's public documentation, WCAG 2.x,
  Björn Ottosson's OKLab article and the existing Teksilo crates. Role B
  never reads upstream KDE sources, research notes, or the ledger's target
  files.

Normative words: **must**, **must not**, **should**, **may** have their RFC
2119 meanings. A statement tagged **[divergence]** is a deliberate,
documented difference between Teksilo and KDE. A statement tagged
**[pin]** is behaviour of the Qt 6 runtime that this document describes as
precisely as it can but that the black-box golden vectors in
`tools/kde_golden` must confirm; where a golden disagrees with a **[pin]**
statement, the golden wins and this document is corrected in the same
commit. Every former **[pin]** has now been checked against the goldens
and is tagged [measured-qt] (or [qt] where Qt documents it); none is open.

Provenance tags used in tables (the same tags the preset uses at each
assignment):

| Tag | Meaning |
|---|---|
| [M] | Breeze widget-style metrics header (breeze, kstyle) |
| [S] | Breeze widget-style sizing and drawing code (breeze, kstyle) |
| [Hp] | Breeze widget-style painting helper (breeze, kstyle) |
| [Kc] | Breeze style configuration schema, `breeze.kcfg` |
| [K] | Kirigami (frameworks/kirigami) |
| [Qd] | qqc2-desktop-style (frameworks/qqc2-desktop-style) |
| [CS] | KDE colour-scheme library (frameworks/kcolorscheme) |
| [CU] | KDE colour utilities and HCY space (frameworks/kguiaddons) |
| [KC] | KDE configuration library (frameworks/kconfig) |
| [AP] | Plasma accent application (plasma-workspace, kcms/colors) |
| [PI] | Plasma Qt platform integration (plasma/plasma-integration) |
| [PK] | xdg-desktop-portal-kde |
| [PW] | other plasma-workspace files (startup, fonts KCM) |
| [qt] | Qt's public documentation or documented Qt behaviour |
| [measured-qt] | measured in the C0 golden image (`tools/kde_golden/vectors`, Qt 6.11.1, offscreen) |
| [derived] | a rule stated at the assignment, not an upstream value |
| [wcag] | a WCAG lift (Teksilo's, never upstream) |
| [unpublished] | no upstream source; Teksilo's choice |

---

## a. Scope and pinned versions

### a.1 What is specified

1. The KConfig text grammar and the cascade KDE uses to read `kdeglobals`
   and colour-scheme files (section b).
2. The KDE colour-scheme model: sets, roles, defaults, effects, contrast
   (section c).
3. The colour mathematics: the HCY colour space, luma, contrast, blends,
   tint, shade roles, HSV lighter and darker, 8-bit alpha blending, the
   OKLab tint (section d).
4. State effects for the Disabled and Inactive colour groups (section e).
5. The computed backgrounds and the QPalette-equivalent roles (section f).
6. Plasma's accent application, every key it writes (section g).
7. The ten accent swatches (section h).
8. The light/dark predicate (section i).
9. Desktop settings: fonts, animation factor, font DPI, widget style,
   `breezerc` (section j).
10. Breeze geometry and painting rules for every control the preset styles
    (section k).
11. Typography (section l) and motion (section m).
12. The answers to the C0 open items (section n) and the ledger (section o).

Out of scope, documented elsewhere in the plan: Union, Ocean, the Breeze
window decoration (KWin draws it under server-side decorations), Plasma
shell translucency, Kirigami heading levels as rendered styles.

### a.2 Pinned upstream versions

| Repository | Tag | Commit |
|---|---|---|
| plasma/breeze | v6.8.0 | cb1de02b5d4fa81d309e33a3f8cd8795ff975336 |
| frameworks/qqc2-desktop-style | v6.31.0 | 2efe38d55ebcfa97599643daada20c157fa1a8a5 |
| frameworks/kirigami | v6.31.0 | eecf183a956fdbfbf331c0ef61fd6e35220d4275 |
| plasma/xdg-desktop-portal-kde | v6.8.0 | d0b8eb0467369a570f57f7fe9242c1bfbf34faf2 |
| frameworks/kconfig | v6.31.0 | a25acdfb1f51c9d8af3426f03380a9de5c88817a |
| plasma/plasma-integration | v6.8.0 | 354bc2ab5bd52715d5a01358ecabfaeb8366152d |
| plasma/plasma-workspace | v6.8.0 | 9d82cf039a17a300c696c87ad32d3113507ed7f9 |
| frameworks/kguiaddons | v6.31.0 | 92ebbc758fa58bb89e6d73fdd24e50a296259bd3 |
| frameworks/kcolorscheme | v6.31.0 | b01fde3096de99584786aa412e0130661c97f7e8 |
| qt/qtbase (the runtime, d.11, d.15) | v6.11.1 | 59c81a3c2247b821b9b84b4eb8d939b77e07e276 |

The runtime the behaviour was observed against is Qt 6 (Plasma 6.8 ships on
Qt 6.10 or later). Every statement about Qt numeric behaviour refers to Qt 6,
where colour channels are stored as 16-bit integers and the floating-point
channel accessors are single precision.

Fonts: Noto Sans v2.015 (notofonts latin-greek-cyrillic release
NotoSans-v2.015) and Hack v3.003 (source-foundry). Hashes are in the ledger.

Fixtures: the three v6.8.0 Breeze colour files (`BreezeLight.colors`,
`BreezeDark.colors`, `BreezeClassic.colors`) are vendored verbatim under
`crates/teksilo-kde-scheme/tests/fixtures/breeze-v6.8.0/` with their LGPL
headers and excluded from the package. The copies in kcolorscheme v6.31.0
differ from the breeze v6.8.0 files only in translated `Name[...]` lines and
in SPDX header lines; every colour, effect, `[KDE]` and `[WM]` value is
identical.

### a.3 Representation conventions (normative for every section)

These conventions are what makes 8-bit equality with KDE possible.

- **Stored colour.** A colour value is four 16-bit unsigned channels
  (red, green, blue, alpha), each an integer `q` in 0..65535.
- **From 8 bits.** A channel given as an 8-bit integer `v` (0..255) is
  stored as `q = 257 v`. Every colour read from a configuration file in the
  `r,g,b[,a]` form enters the model this way; `#rrggbb` and the other hex
  forms are covered in section b.9.
- **Reading a channel as a real number.** The real value of a stored
  channel is `F(q) = q / 65535` computed in single precision (IEEE 754
  binary32, round to nearest), then widened to double precision for all
  further arithmetic. Call this a *channel read*.
- **Storing a real value.** When an operation produces a real channel value
  `v` in [0, 1], it is stored as `Q16(v)`: first round `v` to single
  precision, then form the single-precision product `p = v x 65535`
  (rounded to single precision), then `q = floor(p + 1/2)`. Alpha is stored
  the same way.
- **Quantisation points.** Every operation in section d that returns a
  colour stores its result with `Q16`. The next operation reads the stored
  channels with `F`. Intermediate real values inside one operation are never
  quantised unless this document says so.
- **To 8 bits.** The 8-bit value of a stored channel `q` is
  `E8(q) = floor((q + 128) / 257)`, in integer arithmetic. This is exactly
  `round(q / 257)` (no tie can occur), for example `E8(7067) = 27`.
  [measured-qt]: the Qt 6.11 runtime's 8-bit channel accessors and its
  packed 8-bit colour value both agree with this rule on all 65536 values of
  `q`, and every `hex` field of the goldens equals `E8` of its `u16` field.
  Every colour written to a configuration file, every value compared "8-bit
  equal" in the goldens and every 8-bit read (the dark predicate) uses
  `E8`.
- **Arithmetic.** All other real arithmetic is double precision (binary64),
  evaluated in the order written. Where this document says *single
  precision*, the named product or sum is rounded to binary32 at that step.
  `F32(x)` names that rounding explicitly, and `R(x) = floor(x + 1/2)` is
  the conversion of a non-negative single-precision value to an integer
  used inside Q16.
- **Clamp to the unit interval.** `N(x)` maps x to x if 0 < x < 1, to 0 if
  x <= 0, and to 1 if x >= 1 **or if x is NaN**. (A NaN input maps to 1, not
  to 0.)
- **Interpolation.** `L(a, b, t) = a + (b - a) t` (exactly this form, not
  `(1 - t) a + t b`), unless a step says otherwise.

Teksilo's own `Color` type holds four `f32` in 0..1. The kde-scheme crate
must keep the 16-bit channel integers internally (or an `f32` that is
exactly `q / 65535` and is converted back by `q = round(65535 c)`), and must
convert to Teksilo's `Color` only at the crate boundary.

---

## b. KConfig text grammar and cascade

### b.1 Bytes and lines

1. A file is read as bytes. An empty file contributes nothing.
2. The file is split into lines at byte 0x0A only. Each line is then
   trimmed of leading and trailing ASCII whitespace (space, tab, CR, LF,
   VT, FF). A CR before the LF is therefore removed; a file that uses lone
   CR separators is a single line.
3. A line longer than 1.5 MiB without a newline: KDE warns and parses it
   as consecutive 1.5 MiB pieces, each treated as a line. Teksilo skips such
   a line and records a diagnostic **[divergence]** (an error case only).
4. A UTF-8 byte order mark is **not** recognised. A BOM at the start of the
   file is part of the first line, so if the first line is a group header
   it is not recognised as one (it does not start with `[`); it is treated
   as an entry line, has no `=`, and is rejected as an error. Entries that
   follow it belong to the root group until the next valid header. Teksilo
   must reproduce this and should emit a diagnostic naming the BOM. The
   plan's grammar list mentions the BOM without saying what happens; this
   rule is the decision.
5. Bytes are decoded as UTF-8 only when a value, key or group name is used
   as text; invalid sequences decode to U+FFFD.

### b.2 Comments and blank lines

A trimmed line that is empty, or whose first byte is `#`, is ignored. A `#`
anywhere else is literal text. A `;` is never a comment.

### b.3 Group headers

1. A trimmed line whose first byte is `[` is a group header.
2. The header is a sequence of bracketed segments `[a][b]...`. The first
   segment starts after the first `[`; each segment ends at the next `]`. A
   further segment follows only if the byte after a `]` is `[`. Anything
   after the last `]` that does not start a new segment is ignored.
3. If a segment has no closing `]`, the header is invalid: the line is
   skipped **and the current group does not change**, so following entries
   go to the previous group.
4. Segments are joined into one hierarchical group name. Nested groups such
   as `[Colors:Header][Inactive]` name the group `Inactive` inside
   `Colors:Header`. A nested group is a separate group: its entries are not
   inherited from, and do not fall back to, the parent group.
5. The segment `[$i]` is an option, not a name, when it is exactly `$i`
   and it is the last segment of the line (the line ends right after its
   `]`). If it is the only segment on the line, the **file** becomes
   immutable from this point (b.6). If it follows a name, the **group** is
   immutable. A `[$i]` segment in any other position is part of the name.
6. Escape sequences (b.5) are decoded inside each name segment.
7. Entries that appear before any header belong to the root group. After a
   bare `[$i]` line, entries before the next named header belong to a group
   whose name is empty.

### b.4 Entry lines

1. A non-header line is an entry. The key is the text before the first
   `=`, trimmed; the value is the text after it, trimmed.
2. A line with an empty key is ignored (not counted as an error).
3. Trailing bracketed suffixes on the key are processed from the right:
   - `[$...]` is an option list: each letter `i` (immutable entry), `e`
     (environment expansion allowed), `d` (deleted entry) applies; any other
     letter is ignored.
   - Any other bracket `[xx]` is a locale tag. A second locale tag on one
     key is an error and the line is skipped.
   - A `[` without a later `]` is an error and the line is skipped.
   - `Key[de][$i]` and `Key[$i][de]` are both valid.
4. A line with no `=` is an error and is skipped, unless its options contain
   `d`.
5. `[$d]`: the entry is recorded as deleted, its value (if any) is ignored.
   A deleted entry hides the same key from every lower layer; a higher
   layer that sets the key again makes it present again.
6. Locale-tagged entries: KDE uses them only when the tag equals the
   current locale or its language part. Teksilo must ignore every
   locale-tagged entry. None of the keys this specification reads is
   localised in practice (only `Name` in colour files is).
7. Errors that are counted: missing `]`, second locale, missing `=`,
   invalid escape. After the 100th counted error the rest of the file is
   not read; entries read so far are kept.

### b.5 Escapes

Escape decoding applies to keys, group-name segments and values, and only
when the text contains a backslash. The text is trimmed again first. Then,
left to right:

| Sequence | Result |
|---|---|
| `\s` | a space |
| `\t` | a tab |
| `\n` | a line feed |
| `\r` | a carriage return |
| `\\` | one backslash |
| `\xHH` | the byte with hexadecimal value HH (two hex digits, either case) |
| `\;` | the two characters backslash and semicolon (kept) |
| `\,` | the two characters backslash and comma (kept) |
| a backslash at the very end | kept as one backslash |

- `\x` with fewer than two following characters yields the character `x`
  and ends decoding. `\x` followed by two characters of which one is not a
  hex digit yields `x` and consumes both characters.
- Any other escape is invalid. KDE then keeps a partially decoded value of
  the original length. Teksilo keeps the backslash and the following
  character verbatim, continues decoding, and records a diagnostic
  **[divergence]** (an error case only; no KDE-written file contains it).

Values never contain unescaped leading or trailing spaces because writers
escape them as `\s`.

### b.6 Layers, precedence, immutability

A *cascade* is an ordered list of layers, lowest priority first. Each layer
is one file. Each layer is either a *default layer* (every system file, and
the user's own `system.kdeglobals`) or the *user layer* (the user's own
`kdeglobals`, the only file a KDE program writes).

The entries of all layers are merged into one map from (group, key) to
value, applying the layers in order. For each entry of the layer being
applied:

1. If the map already holds an **immutable** entry for that (group, key),
   the new entry is ignored.
2. Else if the map holds no entry for that (group, key) and the **group is
   marked immutable** by a lower layer, the new entry is ignored.
3. Else, if the layer is the user layer and the group was already marked
   immutable when this layer's header for that group was read, every entry
   of that group in this layer is ignored, including keys that lower layers
   set without immutability.
4. Otherwise the new entry replaces the old one (or is added). It becomes
   immutable if it carries `[$i]`, or if its group header in this layer
   carried `[$i]`, or if the file became immutable before its group header.

Consequences that tests must pin:

- **Duplicate keys.** Within one file the **last** occurrence wins, and a
  higher layer wins over a lower one, except that an occurrence that is
  immutable freezes the value: later occurrences in the same file and in
  higher layers are ignored. Inside a group whose header carries `[$i]`,
  and in every group after a bare `[$i]` line, every occurrence is
  immutable, so the **first** occurrence wins.
- **Group immutability** is set after the whole file has been read, so a
  file can re-open its own immutable group later and add to it. It applies
  to that exact group only, not to nested groups.
- **File immutability.** When a layer contains a bare `[$i]` line, no
  higher layer is read at all.

A group **exists** when it, or any group nested inside it, holds at least
one entry that is not deleted. This is the test used for "the cascade has
`[Colors:View]`" and for "the set has an `[Inactive]` subgroup".

### b.7 The `kdeglobals` cascade

Let `CH` be `$XDG_CONFIG_HOME` (default `$HOME/.config`) and `CD1..CDn` the
entries of `$XDG_CONFIG_DIRS` in the order listed (default `/etc/xdg`).
Empty and relative entries are ignored [qt]. Only files that exist count.
The layers, lowest priority first:

1. `/etc/kde5rc`, if readable, unless the environment variable
   `KDE_SKIP_KDERC` is set.
2. `CDn/system.kdeglobals`, ..., `CD1/system.kdeglobals` (last listed
   directory first).
3. `CDn/kdeglobals`, ..., `CD1/kdeglobals`.
4. `CH/system.kdeglobals` (a default layer).
5. `CH/kdeglobals` (the user layer).

All system `system.kdeglobals` files rank below all system `kdeglobals`
files. A file-immutable layer in steps 1 to 3 suppresses steps 4 and 5.

**kdedefaults.** A Plasma session start prepends `CH/kdedefaults` to
`XDG_CONFIG_DIRS` before any program starts (using `/etc/xdg` as the rest
when the variable was empty). A program started in a Plasma session
therefore sees `CH/kdedefaults/kdeglobals` as the highest-priority system
layer, below the user's `CH/kdeglobals`. Teksilo must honour
`XDG_CONFIG_DIRS` exactly. When the session is KDE (b.10) and
`CH/kdedefaults` is not already one of the entries, Teksilo should prepend
it, reproducing the session start **[derived]**; this is what makes a
program launched outside the session environment agree with one launched
inside it.

### b.8 Locating a colour-scheme file

Let `DH` be `$XDG_DATA_HOME` (default `$HOME/.local/share`) and `DD1..DDn`
the entries of `$XDG_DATA_DIRS` (default `/usr/local/share` then
`/usr/share`). A relative data path `P` resolves to the first existing file
among `DH/P`, `DD1/P`, ..., `DDn/P`.

When the `kdeglobals` cascade has no group `[Colors:View]` (b.6, exists),
KDE's platform integration builds the palette from, in order of preference:

1. `plasma/look-and-feel/<pkg>/contents/colors`, where `<pkg>` is
   `[KDE] LookAndFeelPackage` (default `org.kde.breeze.desktop`);
2. `color-schemes/<name>.colors`, where `<name>` is `[General] ColorScheme`
   (default `BreezeLight`);
3. `color-schemes/BreezeLight.colors`.

That file is read as a full configuration with the `kdeglobals` cascade
below it, so a key the scheme file lacks (for example `[KDE] contrast`)
falls back to `kdeglobals`. **KDE does not apply `[General] AccentColor` in
this fallback** (see n.13; Teksilo applies it there, by decision).

**`KDE_COLOR_SCHEME_PATH` is not an environment variable.** It is an
in-process application property that a KDE program sets on itself when it
chooses its own scheme; KDE reads no environment variable of that name.
Teksilo must not read such an environment variable and claim KDE
behaviour. If Teksilo offers a per-application scheme override, it is an
explicit API parameter that replaces the scheme file of step 2. This
corrects the plan (n.14).

### b.9 Value syntaxes

All values are trimmed and escape-decoded (b.5) before interpretation.

**Strings** are used as they are, except when the entry carries `[$e]`
(b.11).

**Integers and reals** use the C locale. An unparseable value yields the
reader's default. Booleans: a value equal, ignoring ASCII case, to `false`,
`no`, `off` or `0` is false; **every other value is true, including an
empty value** (confirmed: `resolve.json` scheme `SyntheticValueForms`
has `[ColorEffects:Inactive] Enable=` and resolves with its Inactive
effects on). An empty value is present (not absent): it does not
fall back to the reader's default, except for integers and reals, whose
parse fails and so gives the default.

**Colours** (the same rules for every colour key):

| Stored text | Result in KDE | Teksilo |
|---|---|---|
| key absent or deleted | the reader's default | the compiled default (c.3) |
| empty, or exactly `invalid` | an *invalid colour* (not the default) | treated as absent **[divergence]** |
| starts with `#` | Qt's hex forms: `#RGB`, `#RRGGBB`, `#AARRGGBB`, `#RRRGGGBBB`, `#RRRRGGGGBBBB` [qt]; anything else is an invalid colour | hex forms parsed as Qt documents [qt]: the 3- and 6-digit forms give 8-bit channels (a single digit d means 17 d, stored as `257 x 17 d`), `#AARRGGBB` adds an 8-bit alpha; a 9-digit form gives 12-bit channels v stored as `q = 16 v + floor(v / 256)`, a 12-digit form gives the 16-bit channels as written [measured-qt]; any other digit count, or a non-hex digit, is treated as absent **[divergence]** |
| contains no comma and no leading `#` | an SVG 1.1 colour keyword (case-insensitive) or `transparent` [qt]; unknown names give an invalid colour (the CSS-only keyword `rebeccapurple` is unknown [measured-qt]) | the SVG 1.1 keyword table [qt]; unknown treated as absent **[divergence]** |
| contains commas | exactly 3 or 4 components, each a base-10 integer (optional sign, optional surrounding ASCII whitespace; both confirmed by `resolve.json`) in 0..255; red, green, blue, then alpha; anything else (wrong count, not an integer, out of range) gives the reader's default | same; a rejected value is treated as absent |

An invalid colour in KDE behaves like opaque black in arithmetic but is
not a colour a user can intend; the plan's decision to treat it as absent
stands, and it is the only colour divergence.

When KDE **writes** a colour, it writes `r,g,b` (decimal, no spaces) when
the 8-bit alpha `E8(qa)` is 255, `r,g,b,a` otherwise, and `invalid` for an
invalid colour. Each component is `E8` of the stored channel.

When KDE writes a real number it writes the shortest decimal text that
reads back to the same double (for example `0.2`, `0.15`). Integers are
plain decimal. When KDE *copies* an entry from a scheme file it copies the
**text** verbatim (so `#3daee9` stays `#3daee9`); when it reads a colour and
writes it back it re-formats it as `r,g,b`. Section g says which keys are
copied and which are re-formatted.

### b.10 KDE session detection

A session is KDE when `XDG_CURRENT_DESKTOP`, split at `:`, contains `KDE`,
or when `KDE_FULL_SESSION` equals `true`. (The Plasma session start sets
both, plus `KDE_SESSION_VERSION=6`.) Teksilo compares the desktop names
ignoring ASCII case **[derived]**: the session start writes `KDE`, and
Teksilo's desktop detection has always ignored case.

### b.11 `[$e]` expansion

Expansion applies only when the entry carries `[$e]` **and** the value is
read as a plain string. Values read as colours, numbers, booleans or fonts
are never expanded. Expansion scans left to right for `$`:

1. `$$` becomes a single `$`; scanning resumes after it.
2. `${NAME}` (up to the next `}`, or to the end of the value if there is
   none) and `$NAME` (the longest run of letters, digits and `_` following
   the `$`; letters and digits in the Unicode sense) are replaced by the
   value of environment variable `NAME`. An unset or empty variable is
   replaced by nothing, except that `QT_DATA_HOME`, `QT_CONFIG_HOME` and
   `QT_CACHE_HOME`, when unset, are replaced by the XDG data, config and
   cache home directories.
3. A `$` followed by anything else, including `(`, removes just the `$`.
   `$(command)` therefore becomes `(command)`. **No command is ever
   executed**, and a test must pin this. An empty `${}` is removed
   entirely.
4. A trailing lone `$` is kept.

The injected environment of `KdeConfigLocations::from_env` is the only
source of variable values.

---

## c. The colour-scheme model

### c.1 Sets, roles, states

There are seven colour sets, each stored in its own group:

| Set | Group |
|---|---|
| View | `[Colors:View]` |
| Window | `[Colors:Window]` |
| Button | `[Colors:Button]` |
| Selection | `[Colors:Selection]` |
| Tooltip | `[Colors:Tooltip]` |
| Complementary | `[Colors:Complementary]` |
| Header | `[Colors:Header]` |

Each set has twelve stored roles (the keys): `BackgroundNormal`,
`BackgroundAlternate`, `ForegroundNormal`, `ForegroundInactive`,
`ForegroundActive`, `ForegroundLink`, `ForegroundVisited`,
`ForegroundNegative`, `ForegroundNeutral`, `ForegroundPositive`,
`DecorationFocus`, `DecorationHover`.

There are three states (colour groups): Active, Inactive, Disabled.

After resolution (sections e and f) each (state, set) has 8 foregrounds
(Normal, Inactive, Active, Link, Visited, Negative, Neutral, Positive), 8
backgrounds (Normal, Alternate and the six computed ones), 2 decorations
(Focus, Hover) and 5 shade roles (Light, Midlight, Mid, Dark, Shadow).

### c.2 Reading a set for a state

For a given (state, set):

1. Choose the **source group** and the **default table**:
   - View, Window, Button, Tooltip, Complementary: their own group and their
     own table (c.3).
   - Header: group `[Colors:Header]`; the default for each key is the value
     of the **same key in `[Colors:Window]`** if present there, else the
     Header table value. This is a per-key fallback, not a wholesale one: a
     Header group that sets only some keys takes the others from Window,
     and a scheme without a Header group gets Window's keys for all twelve.
     The two decoration keys follow the same rule against the decoration
     defaults.
   - Selection, Active state: `[Colors:Selection]` with the Selection
     table.
   - Selection, Inactive state, when **ChangeSelectionColor** (c.5) is
     true: group `[Colors:Window]` with the **Window** table, and a *tint
     colour* T equal to `[Colors:Selection] BackgroundNormal` (top-level
     group, default 61,174,233).
   - Selection, Inactive state, when ChangeSelectionColor is false:
     `[Colors:Selection]` with the Selection table.
   - Selection, Disabled state: `[Colors:Window]` with the Window table and
     no tint colour.
2. **Inactive subgroup.** For the Inactive state only: if the source group
   has a nested group `[Inactive]` that exists (b.6), that nested group
   becomes the source group. Its missing keys take the defaults chosen in
   step 1 (for Header: Window's top-level key, else the Header table); they
   do **not** fall back to the parent group's keys. For Selection with
   ChangeSelectionColor the nested group looked at is
   `[Colors:Window][Inactive]`.
3. Read each of the twelve keys from the source group, falling back to the
   default. A colour that is treated as absent (b.9) takes the default.
4. **Selection tint.** If a tint colour T was chosen in step 1, replace
   BackgroundNormal by `tint(BackgroundNormal, T, 0.4)` and
   BackgroundAlternate by `tint(BackgroundAlternate, T, 0.4)` (d.9). The
   foregrounds and decorations stay the Window values.
5. Apply the state effects (section e) for every state other than Active.
   **This includes the Inactive state when an `[Inactive]` subgroup
   supplied the colours**: the subgroup replaces the source values, and the
   Inactive effects still run on top of them (they are no-ops for Breeze,
   whose Inactive effects are disabled).
6. Compute the six derived backgrounds (section f.1).

The Tooltip set used for palettes is always the Active state's (f.2).

### c.3 Compiled defaults for absent keys

These are the values used when a key is absent. They equal Breeze Light except for
`DecorationHover`.
`DecorationFocus` default for every set is 61,174,233; `DecorationHover`
default is **147,206,233** (note: not the 61,174,233 the Breeze files
write).

| Key | View | Window | Button | Selection | Tooltip | Complementary | Header |
|---|---|---|---|---|---|---|---|
| BackgroundNormal | 255,255,255 | 239,240,241 | 252,252,252 | 61,174,233 | 247,247,247 | 42,46,50 | 222,224,226 |
| BackgroundAlternate | 247,247,247 | 227,229,231 | 163,212,250 | 163,212,250 | 239,240,241 | 27,30,32 | 239,240,241 |
| ForegroundNormal | 35,38,41 | 35,38,41 | 35,38,41 | 255,255,255 | 35,38,41 | 252,252,252 | 35,38,41 |
| ForegroundInactive | 112,125,138 | 112,125,138 | 112,125,138 | 112,125,138 | 112,125,138 | 161,169,177 | 112,125,138 |
| ForegroundActive | 61,174,233 | 61,174,233 | 61,174,233 | 255,255,255 | 61,174,233 | 61,174,233 | 61,174,233 |
| ForegroundLink | 41,128,185 | 41,128,185 | 41,128,185 | 253,188,75 | 41,128,185 | 29,153,243 | 41,128,185 |
| ForegroundVisited | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 |
| ForegroundNegative | 218,68,83 | 218,68,83 | 218,68,83 | 176,55,69 | 218,68,83 | 218,68,83 | 218,68,83 |
| ForegroundNeutral | 246,116,0 | 246,116,0 | 246,116,0 | 198,92,0 | 246,116,0 | 246,116,0 | 246,116,0 |
| ForegroundPositive | 39,174,96 | 39,174,96 | 39,174,96 | 23,104,57 | 39,174,96 | 39,174,96 | 39,174,96 |

(The Header column applies only where `[Colors:Window]` lacks the key.)

### c.4 The stock Breeze schemes (v6.8.0), for the Rust constants

These are the fixture values; the constants must be tested equal to the
fixture files.

**Breeze Light** (`ColorScheme=BreezeLight`, `Name=Breeze Light`). Every set
has DecorationFocus 61,174,233 and DecorationHover 61,174,233.

| Key | View | Window | Button | Selection | Tooltip | Complementary | Header | Header [Inactive] |
|---|---|---|---|---|---|---|---|---|
| BackgroundNormal | 255,255,255 | 239,240,241 | 252,252,252 | 61,174,233 | 247,247,247 | 42,46,50 | 222,224,226 | 239,240,241 |
| BackgroundAlternate | 247,247,247 | 227,229,231 | 163,212,250 | 163,212,250 | 239,240,241 | 27,30,32 | 239,240,241 | 227,229,231 |
| ForegroundNormal | 35,38,41 | 35,38,41 | 35,38,41 | 255,255,255 | 35,38,41 | 252,252,252 | 35,38,41 | 35,38,41 |
| ForegroundInactive | 112,125,138 | 112,125,138 | 112,125,138 | 112,125,138 | 112,125,138 | 161,169,177 | 112,125,138 | 112,125,138 |
| ForegroundActive | 61,174,233 | 61,174,233 | 61,174,233 | 255,255,255 | 61,174,233 | 61,174,233 | 61,174,233 | 61,174,233 |
| ForegroundLink | 41,128,185 | 41,128,185 | 41,128,185 | 253,188,75 | 41,128,185 | 29,153,243 | 41,128,185 | 41,128,185 |
| ForegroundVisited | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 |
| ForegroundNegative | 218,68,83 | 218,68,83 | 218,68,83 | 176,55,69 | 218,68,83 | 218,68,83 | 218,68,83 | 218,68,83 |
| ForegroundNeutral | 246,116,0 | 246,116,0 | 246,116,0 | 198,92,0 | 246,116,0 | 246,116,0 | 246,116,0 | 246,116,0 |
| ForegroundPositive | 39,174,96 | 39,174,96 | 39,174,96 | 23,104,57 | 39,174,96 | 39,174,96 | 39,174,96 | 39,174,96 |

`[KDE] contrast=4` (no `frameContrast`). `[WM]`: activeBackground
227,229,231; activeBlend 227,229,231; activeForeground 35,38,41;
inactiveBackground 239,240,241; inactiveBlend 239,240,241;
inactiveForeground 112,125,138. `[General] shadeSortColumn=true`.

**Breeze Dark** (`ColorScheme=BreezeDark`, `Name=Breeze Dark`). Every set
has DecorationFocus 61,174,233 and DecorationHover 61,174,233.

| Key | View | Window | Button | Selection | Tooltip | Complementary | Header | Header [Inactive] |
|---|---|---|---|---|---|---|---|---|
| BackgroundNormal | 20,22,24 | 32,35,38 | 41,44,48 | 61,174,233 | 41,44,48 | 32,35,38 | 41,44,48 | 32,35,38 |
| BackgroundAlternate | 29,31,34 | 41,44,48 | 30,87,116 | 30,87,116 | 32,35,38 | 30,87,116 | 32,35,38 | 41,44,48 |
| ForegroundNormal | 252,252,252 | 252,252,252 | 252,252,252 | 252,252,252 | 252,252,252 | 252,252,252 | 252,252,252 | 252,252,252 |
| ForegroundInactive | 161,169,177 | 161,169,177 | 161,169,177 | 161,169,177 | 161,169,177 | 161,169,177 | 161,169,177 | 161,169,177 |
| ForegroundActive | 61,174,233 | 61,174,233 | 61,174,233 | 252,252,252 | 61,174,233 | 61,174,233 | 61,174,233 | 61,174,233 |
| ForegroundLink | 29,153,243 | 29,153,243 | 29,153,243 | 253,188,75 | 29,153,243 | 29,153,243 | 29,153,243 | 29,153,243 |
| ForegroundVisited | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 | 155,89,182 |
| ForegroundNegative | 218,68,83 | 218,68,83 | 218,68,83 | 176,55,69 | 218,68,83 | 218,68,83 | 218,68,83 | 218,68,83 |
| ForegroundNeutral | 246,116,0 | 246,116,0 | 246,116,0 | 198,92,0 | 246,116,0 | 246,116,0 | 246,116,0 | 246,116,0 |
| ForegroundPositive | 39,174,96 | 39,174,96 | 39,174,96 | 23,104,57 | 39,174,96 | 39,174,96 | 39,174,96 | 39,174,96 |

`[KDE] contrast=4`. `[WM]`: activeBackground 39,44,49; activeBlend
252,252,252; activeForeground 252,252,252; inactiveBackground 32,36,40;
inactiveBlend 161,169,177; inactiveForeground 161,169,177.
`[General] shadeSortColumn=true`.

**Breeze Classic** (`ColorScheme=BreezeClassic`, `Name=Breeze Classic`). No
`[Colors:Header]` group (Header resolves to Window per key). Decorations:
DecorationFocus 61,174,233 and DecorationHover 147,206,233 in View, Window,
Button, Selection, Tooltip; Complementary has DecorationFocus 30,146,255 and
DecorationHover 61,174,230.

| Key | View | Window | Button | Selection | Tooltip | Complementary |
|---|---|---|---|---|---|---|
| BackgroundNormal | 252,252,252 | 239,240,241 | 239,240,241 | 61,174,233 | 35,38,39 | 49,54,59 |
| BackgroundAlternate | 239,240,241 | 189,195,199 | 189,195,199 | 29,153,243 | 77,77,77 | 59,64,69 |
| ForegroundNormal | 35,38,39 | 35,38,39 | 35,38,39 | 252,252,252 | 252,252,252 | 239,240,241 |
| ForegroundInactive | 127,140,141 | 127,140,141 | 127,140,141 | 239,240,241 | 189,195,199 | 175,176,179 |
| ForegroundActive | 61,174,233 | 61,174,233 | 61,174,233 | 252,252,252 | 61,174,233 | 147,206,233 |
| ForegroundLink | 41,128,185 | 41,128,185 | 41,128,185 | 253,188,75 | 41,128,185 | 61,174,230 |
| ForegroundVisited | 127,140,141 | 127,140,141 | 127,140,141 | 189,195,199 | 127,140,141 | 61,174,230 |
| ForegroundNegative | 218,68,83 | 218,68,83 | 218,68,83 | 176,55,69 | 218,68,83 | 231,76,60 |
| ForegroundNeutral | 246,116,0 | 246,116,0 | 198,92,0 | 246,116,0 | 246,116,0 | 253,188,75 |
| ForegroundPositive | 39,174,96 | 39,174,96 | 39,174,96 | 23,104,57 | 39,174,96 | 46,204,113 |

`[General]` also has `accentActiveTitlebar=true`, `accentInactiveTitlebar=false`,
`shadeSortColumn=true`. `[KDE] contrast=4`. `[WM]`: activeBackground
71,80,87; activeBlend 252,252,252; activeForeground 252,252,252;
inactiveBackground 239,240,241; inactiveBlend 75,71,67; inactiveForeground
189,195,199.

**Effects in all three files** (identical):

`[ColorEffects:Disabled]`: Color 56,56,56; ColorAmount 0; ColorEffect 0;
ContrastAmount 0.65; ContrastEffect 1; IntensityAmount 0.1;
IntensityEffect 2. (No `Enable` key, so enabled by default.)

`[ColorEffects:Inactive]`: ChangeSelectionColor true; Color 112,111,110;
ColorAmount 0.025; ColorEffect 2; ContrastAmount 0.1; ContrastEffect 2;
**Enable false**; IntensityAmount 0; IntensityEffect 0.

### c.5 `[ColorEffects:Disabled]` and `[ColorEffects:Inactive]`

Keys, read from the configuration being resolved (top-level groups):

| Key | Type | Meaning |
|---|---|---|
| Enable | bool | whether the state's effects run |
| IntensityEffect | int | 0 none, 1 shade, 2 darken, 3 lighten; any other value: none |
| IntensityAmount | real | the intensity amount |
| ColorEffect | int | 0 none, 1 desaturate, 2 fade, 3 tint; any other value: none |
| ColorAmount | real | the colour amount |
| Color | colour | the effect colour, read only when ColorEffect is greater than 0 |
| ContrastEffect | int | 0 none, 1 fade, 2 tint; any other value: none |
| ContrastAmount | real | the contrast amount |
| ChangeSelectionColor | bool | Inactive group only; see c.2 |

Defaults when a key is absent:

| Key | Disabled | Inactive |
|---|---|---|
| Enable | **true** | **false** |
| IntensityEffect | 2 (darken) | 0 |
| IntensityAmount | 0.10 | 0.0 |
| ColorEffect | 0 | 1 (desaturate) |
| ColorAmount | 0.0 | -0.9 |
| Color | 56,56,56 | 112,111,110 |
| ContrastEffect | 1 (fade) | 2 (tint) |
| ContrastAmount | 0.65 | 0.25 |

`ChangeSelectionColor` defaults to the value of `Enable` in
`[ColorEffects:Inactive]` **with a default of true** for that lookup. So a
configuration with no `[ColorEffects:Inactive]` group at all has Inactive
effects off and ChangeSelectionColor on.

When Enable is false, all three effect kinds are "none" and the amounts are
irrelevant; the effect pass returns its inputs **unchanged and
unquantised** (no colour operation runs).

### c.6 `[KDE] contrast` and `[KDE] frameContrast`

- `contrast` is an integer, default 7. The **contrast value** used by the
  shade roles is `k = 0.1 x contrast`. It is not clamped when read; the
  shade-role computation clamps it (d.10). An unparseable value gives 7.
  When KDE has no configuration at all (the system-palette mode used under
  a platform high-contrast preference with no scheme) the contrast value is
  0.7.
- `frameContrast` is a real, default 0.2, clamped to [0, 1]. An
  unparseable value gives 0.2. It is not used by the colour model itself;
  Breeze uses it as the frame intensity bias `fc` (section k).

### c.7 `[WM]`

Six colour keys describe title bars: `activeBackground`,
`activeForeground`, `inactiveBackground`, `inactiveForeground`,
`activeBlend`, `inactiveBlend`. The accent application always writes all
six (g.5, g.7). The widget style itself reads only the first four (for its MDI
title bars), defaulting to the Active Highlight, Active HighlightedText,
Disabled Highlight and Disabled HighlightedText of the application palette
when absent.

### c.8 `[General]` keys

| Key | Where | Meaning |
|---|---|---|
| ColorScheme | kdeglobals, scheme | the scheme name; default `BreezeLight` |
| Name | scheme | display name (localised variants ignored) |
| AccentColor | kdeglobals | the user's accent; consulted by the accent application (g.1) |
| TintFactor | scheme | presence turns on accent tinting (g.3, g.7); value default 0.15 |
| TitlebarIsAccentColored | scheme | accent-coloured title bar; falls back to `accentActiveTitlebar`, then false |
| accentActiveTitlebar | scheme | legacy name of the previous key |
| accentInactiveTitlebar | scheme | legacy; not read by the accent application |
| ColorSchemeHash | kdeglobals | lower-case hex SHA-1 of the applied scheme file's bytes; written by the accent application |
| shadeSortColumn | scheme | item-view sort-column shading; not used by this specification |

---

## d. Colour mathematics

All operations here take and return stored colours (a.3) unless they return
a real number. Operation names in this section are this document's own.

### d.1 Constants

- Gamma exponent: 2.2 (forward), 1/2.2 (inverse).
- Luma weights (ITU-R BT.709): `wr = 0.2126`, `wg = 0.7152`,
  `wb = 0.0722`.

### d.2 Linearisation

For a channel read `x` (a.3): `lin(x) = N(x) ^ 2.2` and
`delin(x) = N(x) ^ (1/2.2)`, using the real power function in double
precision.

### d.3 Luma

`luma(C) = lin(R) wr + lin(G) wg + lin(B) wb`, evaluated left to right,
where R, G, B are the channel reads of C. Alpha is ignored. Range 0 to 1.

This is not WCAG relative luminance: it uses a pure 2.2 power instead of
the sRGB transfer curve.

### d.4 HCY forward conversion

Input: a stored colour. Output: four reals (h, c, y, a).

1. `r = lin(R)`, `g = lin(G)`, `b = lin(B)`; `a` is the channel read of
   alpha.
2. `y = r wr + g wg + b wb` (as d.3).
3. `p = max(r, g, b)`, `n = min(r, g, b)`, `d = 6 (p - n)`.
4. Hue, tested in this order:
   - if `n = p`: `h = 0`;
   - else if `r = p`: `h = (g - b) / d`;
   - else if `g = p`: `h = (b - r) / d + 1/3`;
   - else: `h = (r - g) / d + 2/3`.

   `h` is **not wrapped**: when red is the maximum and blue exceeds green,
   `h` lies in (-1/6, 0). It stays negative inside the operations of d.6
   and is wrapped only by the inverse conversion. (Where a hue is reported
   to a caller as a number in [0, 1), add 1 when negative.)
5. Chroma: if `r = g = b`: `c = 0`; else
   `c = max((y - n) / y, (p - y) / (1 - y))`.

Equality tests in steps 4 and 5 are exact floating-point comparisons.

### d.5 HCY inverse conversion

Input: (h, c, y, a). Output: a stored colour.

1. `h1 = wrap(h)`, where `wrap(h)` is the remainder of h divided by 1 with
   the sign of h (C `fmod`), plus 1 if that remainder is negative; a zero
   or NaN remainder gives 0. `c1 = N(c)`, `y1 = N(y)`.
2. `s = 6 h1`. Choose the sector and compute `t` and `m`:

| Sector | Condition | t | m |
|---|---|---|---|
| 0 | s < 1 | s | wr + wg t |
| 1 | s < 2 | 2 - s | wg + wr t |
| 2 | s < 3 | s - 2 | wg + wb t |
| 3 | s < 4 | 4 - s | wb + wg t |
| 4 | s < 5 | s - 4 | wb + wr t |
| 5 | otherwise | 6 - s | wr + wb t |

3. Compute the three linear components (top `P`, middle `O`, bottom `B0`):
   - if `m >= y1`:
     `P = y1 + y1 c1 (1 - m) / m`,
     `O = y1 + y1 c1 (t - m) / m`,
     `B0 = y1 - y1 c1`;
   - else:
     `P = y1 + (1 - y1) c1`,
     `O = y1 + (1 - y1) c1 (t - m) / (1 - m)`,
     `B0 = y1 - (1 - y1) c1 m / (1 - m)`.

   Products and quotients are evaluated left to right as written (for
   example `y1 c1 (1 - m) / m` is `((y1 x c1) x (1 - m)) / m`).
4. Assign to (red, green, blue) by sector:

| Sector | red | green | blue |
|---|---|---|---|
| 0 | P | O | B0 |
| 1 | O | P | B0 |
| 2 | B0 | P | O |
| 3 | B0 | O | P |
| 4 | O | B0 | P |
| 5 | P | B0 | O |

5. Each channel is `delin` of its component, stored with Q16. Alpha `a` is
   stored with Q16.

Round trip: forward then inverse must reproduce any 8-bit colour exactly
(a proptest over 8-bit inputs). Example: 61,174,233 has
h = 0.58336232, c = 0.88592616, y = 0.37683496 and returns to 61,174,233.

### d.6 Luma operations

Each converts the input to HCY (d.4), changes y and c, keeps h and a, and
converts back (d.5).

- **Luma lighten** with amount `ky` (default 0.5) and chroma factor `kc`
  (default 1): `y' = 1 - N((1 - y)(1 - ky))`, `c' = 1 - N((1 - c) kc)`.
  Amount 1 gives white, 0.5 halves the distance to white, 0 keeps the
  luma.
- **Luma darken** with amount `ky` (default 0.5) and chroma factor `kc`
  (default 1): `y' = N(y (1 - ky))`, `c' = N(c kc)`.
- **Luma shade** with luma amount `ky` and chroma amount `kc` (default 0):
  `y' = N(y + ky)`, `c' = N(c + kc)`.

Even with amounts that leave y and c unchanged, the round trip re-quantises
the colour. (The effect pass avoids this by not calling an operation when
the effect is "none", c.5.)

### d.7 Premultiplied mix

**Mix**(C1, C2, t):

1. If `t <= 0`, return C1 unchanged. Else if `t >= 1`, return C2 unchanged.
   Else if t is NaN, return C1 unchanged. (Tested in this order.)
2. `a = L(a1, a2, t)` from the alpha channel reads. If `a <= 0`, return
   (0, 0, 0, 0).
3. For each colour channel: form the premultiplied values `p1 = x1 a1` and
   `p2 = x2 a2` **in single precision** (x the channel reads), then
   `x = N'(L(p1, p2, t)) / a`, where `N'` clamps to [0, 1] (no NaN case
   arises).
4. Store the three channels and `a` with Q16.

### d.8 Luma contrast ratio

`ratio(Y1, Y2) = (Y1 + 0.05) / (Y2 + 0.05)` if `Y1 > Y2`, otherwise
`(Y2 + 0.05) / (Y1 + 0.05)`, for two lumas from d.3. Between colours:
`ratio(luma(C1), luma(C2))`. It has WCAG's shape but uses d.3's luma, so
its values differ from WCAG contrast; Teksilo's WCAG checks must use
Teksilo's own WCAG helpers, never this.

### d.9 Luma-preserving tint

**Tint**(base B, colour C, amount t), default t = 0.3:

1. If `t <= 0` return B unchanged; else if `t >= 1` return C unchanged;
   else if t is NaN return B unchanged.
2. `Yb = luma(B)`; `ri = ratio(Yb, luma(C))`;
   `rg = 1 + ((ri + 1) x t x t x t)` (left to right).
3. Bisection on `s` in [0, 1] with `l = 0`, `u = 1`, exactly **12**
   iterations. In each iteration:
   - `s = (l + u) / 2`;
   - `M = Mix(B, C, s ^ 0.3)` (a stored colour);
   - `Ys = L(Yb, luma(M), s)`;
   - if `ratio(Yb, Ys) > rg` then `u = s`, else `l = s`.
4. The result uses the **last midpoint `s` computed in the loop** (the
   twelfth midpoint), not the final `l` or `u`: take `M = Mix(B, C, s ^ 0.3)`,
   convert M to HCY, replace its y by `L(Yb, y_M, s)`, keep h, c and alpha,
   and convert back (d.5).

The exponent 0.3 and the cube in rg are exact constants. `s ^ 0.3` uses the
real power function.

### d.10 Shade roles

**ShadeRole**(colour C, role, contrast value k), with chroma amount 0:

1. Clamp k: if `k >= 1` or k is NaN, k = 1; else if `k <= -1`, k = -1.
2. `y = luma(C)`, `yi = 1 - y`.
3. If `y < 0.006` (very dark): Light = shade(C, 0.05 + 0.95 k);
   Mid = shade(C, 0.01 + 0.20 k); Dark = shade(C, 0.02 + 0.40 k);
   Midlight and Shadow = shade(C, 0.03 + 0.60 k).
4. Else if `y > 0.93` (very light): Midlight = shade(C, -0.02 - 0.20 k);
   Dark = shade(C, -0.06 - 0.60 k); Shadow = shade(C, -0.10 - 0.90 k);
   Light and Mid = shade(C, -0.04 - 0.40 k).
5. Otherwise, with `la = (0.05 + 0.55 y)(0.25 + 0.75 k)` and
   `da = (-y)(0.55 + 0.35 k)`:
   - Light = shade(C, la);
   - Midlight = shade(C, (0.15 + 0.35 yi) la);
   - Mid = shade(C, (0.35 + 0.15 y) da);
   - Dark = shade(C, da);
   - Shadow = darken(shade(C, da), 0.5 + 0.3 y) with chroma factor 1.

"shade" and "darken" are d.6 (shade with chroma amount 0). Example: Breeze
Light Window background 239,240,241 with k = 0.4 gives Light 255,255,255,
Midlight 246,247,247, Mid 196,200,204, Dark 136,142,147, Shadow 71,74,76
(computed from this specification; the goldens are authoritative).

### d.11 HSV lighter and darker (Qt semantics)

These reproduce Qt's documented `lighter(factor)` and `darker(factor)`
colour functions [qt]. The factor is an integer percentage.

- factor <= 0: return the colour unchanged.
- lighter with factor < 100 is darker with factor `floor(10000 / factor)`;
  darker with factor < 100 is lighter with factor `floor(10000 / factor)`.
- Convert to HSV with 16-bit value and saturation (0..65535) and hue in
  hundredths of a degree (0..36000, or "undefined" for an achromatic
  colour), as below.
- lighter (factor >= 100): `v' = floor(factor x v / 100)` (integers). If v'
  exceeds 65535, the excess is removed from saturation:
  `s' = max(0, s - (v' - 65535))` and `v' = 65535`.
- darker (factor >= 100): `v' = floor(v x 100 / factor)` (integers); s
  unchanged.
- Convert back to RGB and store. Alpha is kept as stored.

**RGB to HSV**, on the stored 16-bit integers r, g, b [measured-qt]:

1. `max` and `min` of the three integers; `v = max`.
2. If `max = min` (exact integer equality): hue undefined, `s = 0`.
3. Otherwise `k = max - min` as a single-precision value;
   `s = R(F32(k / v) x 65535)`, where the quotient and the product are each
   rounded to single precision and `R(x) = floor(x + 1/2)` turns the
   single-precision value into an integer.
4. Hue position `p` (single precision): if `v = r`, `p = (g - b) / k`, plus 6
   when negative; else if `v = g`, `p = 2 + (b - r) / k`; else
   `p = 4 + (r - g) / k`. The integer differences are exact; each quotient
   and sum is rounded to single precision. The stored hue is
   `R(F32(p x 6000))`, which can be 36000.

**HSV to RGB** [measured-qt]:

1. If `s = 0` or the hue is undefined, all three channels are `v`.
2. Otherwise, in single precision: `h = 0` when the hue is 36000, else
   `h = hue / 6000`; `S = s / 65535`; `V = v / 65535`; `i = trunc(h)`,
   `f = h - i`; `p = V (1 - S)`; for odd i, `q = V (1 - S f)`; for even i,
   `t = V (1 - S (1 - f))` (each product and difference rounded to single
   precision, innermost first).
3. Channels (red, green, blue) by i: 0: (V, t, p); 1: (q, V, p); 2: (p, V,
   t); 3: (p, q, V); 4: (t, p, V); 5: (V, p, q). Each is stored as
   `R(F32(x x 65535))` (the Q16 of a.3).

`math.json` member `qt_lighter_darker` pins these rules: 62 colours x 14
factors (including -10, 0, 1, 50, 99, 100, 102, 110, 120, 150 and 1000),
both directions, all reproduced exactly.

### d.12 8-bit alpha blend

**Blend8**(foreground F with alpha, opaque or translucent background G)
operates on 8-bit channel values (E8 of the stored channels) and returns a
colour built from 8-bit integers:

1. `af` = the alpha channel read of F (single precision value `qa / 65535`).
2. If `af = 0`, return G unchanged.
3. `ai = 1 - af` in double precision (af widened).
4. If G's alpha read is exactly 1: each channel is
   `trunc(P + ai x g8)`, where `P` is the **single-precision** product
   `af x f8` and the sum is double precision; `trunc` discards the fraction
   (all values are non-negative). Alpha is 255. The result is stored from
   8-bit values (`q = 257 v`).
5. Otherwise (translucent background; never the case for Breeze): with
   `ab` = G's alpha read, `aib = ab x ai`, each channel is
   `trunc(P + aib x g8)` and the **alpha is the integer part of
   `af + aib`**, which is 0 unless the sum is exactly 1, and then 1 (out of
   255). The colour channels are not un-premultiplied. KDE writes this
   nearly transparent result; Teksilo must reproduce it for byte equality.

Alpha quantisation that feeds Blend8 (setting an alpha as a fraction
uses Q16):

| Fraction | Stored alpha q | Effective af |
|---|---|---|
| 0.7 | 45875 | 0.70000761747 (single precision) |
| 0.6 | 39321 | 0.60000002384 (single precision) |
| 1.0 | 65535 | 1.0 |

Example (why the precision rules matter): white at alpha 0.6 over 255
gives `trunc(153.0 + 0.39999997616 x 255) = trunc(254.99999392) = 254`,
not 255.

### d.13 Black-or-white foreground rule

**OnColour**(background G, active flag):

1. Start from white. If `ratio(luma(G), luma(white)) < 4.5` (d.8, so
   KDE's luma ratio, not WCAG), choose black; otherwise white.
   Equivalently, black exactly when `luma(G) > 1.05 / 4.5 - 0.05`
   (about 0.183333).
2. Give the chosen colour alpha 1.0 when active, 0.6 when inactive.
3. Return `Blend8(chosen, G)`.

Active results are pure black or pure white. Inactive results are the
chosen colour at 0.6 over G with the truncation of d.12.

### d.14 Accent background blend

**AccentBg**(accent A, background G) = `Blend8(A with alpha 0.7, G)`.
Only the 8-bit values of A are used. Example: 61,174,233 over 255,255,255
gives 119,198,239; over 20,22,24 gives 48,128,170.

### d.15 OKLab tint

**OklabTint**(base B, colour C, factor f) is the accent tint of g.3 and g.7. It
follows Björn Ottosson's OKLab definition ("A perceptual color space for
image processing", 2020, MIT licence), using the matrices **as Ottosson
published them to ten decimal places**:

- linear sRGB to LMS: l = 0.4122214708 r + 0.5363325363 g + 0.0514459929 b;
  m = 0.2119034982 r + 0.6806995451 g + 0.1073969566 b;
  s = 0.0883024619 r + 0.2817188376 g + 0.6299787005 b;
- cube roots `l^, m^, s^` (real power 1/3);
- L = 0.2104542553 l^ + 0.7936177850 m^ - 0.0040720468 s^;
  a = 1.9779984951 l^ - 2.4285922050 m^ + 0.4505937099 s^;
  b = 0.0259040371 l^ + 0.7827717662 m^ - 0.8086757660 s^;
- inverse: l^ = L + 0.3963377774 a + 0.2158037573 b;
  m^ = L - 0.1055613458 a - 0.0638541728 b;
  s^ = L - 0.0894841775 a - 1.2914855480 b; cube each;
  r = 4.0767416621 l - 3.3077115913 m + 0.2309699292 s;
  g = -1.2684380046 l + 2.6097574011 m - 0.3413193965 s;
  b = -0.0041960863 l - 0.7034186147 m + 1.7076147010 s.

**sRGB to linear (SL) and linear to sRGB (LS)** are Qt's colour-space
transforms between its named sRGB and linear sRGB spaces, applied to a
stored colour [measured-qt]. Everything below is single precision (each
operation rounded to binary32), and the single-precision power is the
correctly rounded binary32 power (computing it in double precision and
rounding once reproduces every golden).

- Constants: `A = F32(1 / F32(1.055))`, `B = F32(F32(0.055) / F32(1.055))`,
  `K = F32(1 / F32(12.92))`, `D = F32(0.04045)`, `G = F32(2.4)`; for the
  inverse curve `D' = F32(K D)`, `K' = F32(1 / K)`,
  `A' = F32(F32(1 / A) ^ G)`, `E' = F32(-B / A)`, `G' = F32(1 / G)`.
- Forward curve T(x), x a channel read: with `u = |x|`, `T = K u` when
  `u < D`, else `T = (A u + B) ^ G`; the sign of x is restored.
- Inverse curve T'(y): with `u = |y|`, `T' = K' u` when `u < D'`, else
  `T' = (A' u) ^ G' + E'`; the sign of y is restored.
- Matrix M (applied in both directions; it is the identity up to
  single-precision rounding, because Qt forms it as the inverse of the sRGB
  to XYZ matrix times that matrix). With input channels (r, g, b), each
  output channel is `(r m1 + g m2) + b m3`, products and sums in single
  precision, left to right, with (m1, m2, m3) =
  red: (`0x1.fffffep-1` = 0.99999994039535522, `0x1.4p-25` =
  3.7252902984619141e-08, `-0x1p-25` = -2.9802322387695312e-08);
  green: (`0x1.928p-26` = 2.3428583517670631e-08, 1, 0);
  blue: (0, 0, `0x1.fffffcp-1` = 0.99999988079071045).
  The values are binary32 numbers written in hexadecimal floating point.
- SL(X): read X's channels, apply T to each, then M; the linear space's own
  curve is the identity. LS(X): read X's channels (identity curve), apply
  M, then T' to each.
- **Storing a float colour** (used by SL, LS and step 4): if all three
  channels lie in [0, 1], store each with Q16; otherwise the colour is held
  in **half precision**: each channel (and the alpha, 1) is rounded to the
  nearest binary16 value (ties to even). A channel read of such a colour is
  its binary16 value. Converting it to 16-bit channels (step 4, and the 8-bit
  write of a.3) clamps each binary16 value to [0, 1] and stores it with Q16.
  M can push a channel of an in-gamut colour a few units in the last place
  outside [0, 1] (for example the red channel of pure blue becomes
  -2.98e-08), and such a colour is then held in half precision.

Steps:

1. `Bl = SL(B)`, `Cl = SL(C)` (stored as above).
2. Read the channels of Bl and Cl and convert each to OKLab in double
   precision (cube roots by the real power 1/3).
3. Keep B's L. Replace B's a by `Lf(aB, aC, f)` and b by `Lf(bB, bC, f)`,
   where `Lf(x, y, f) = x (1 - f) + y f` is evaluated with x, y and f first
   rounded to single precision, `(1 - f)` and `x (1 - f)` in double, `y f`
   in single, the sum in double, and **the result rounded to single
   precision**.
4. Convert back to linear sRGB in double precision, round each channel to
   single precision, store the float colour as above (the range test is on
   the single-precision values), and convert it to 16-bit channels.
5. The result is `LS` of that colour, stored as above. Alpha is opaque.

The written value is the E8 projection of step 5 (clamped first when it is
held in half precision). Goldens: `math.json` members `qt_colorspace` (SL
and LS of 573 colours; 6 of the 1146 results are held in half precision) and
`qt_extended` (half-precision storage and its conversion) pin the
conversions exactly; `accent.json` (the `KdeGoldenTintLight` cases, whose
tinted Button background 232,255,255 leaves the gamut) pins the whole
tint. A rule that skips M, the half-precision storage, or the
single-precision curve constants (using `((x + 0.055) / 1.055) ^ 2.4`
instead) still matches those 40 oracle cases but differs from Plasma in
about 1 tint in 1400 over random inputs, so the details above are
normative.

---

## e. State effects (Disabled and Inactive)

### e.1 When they run

For every set and for the Disabled and Inactive states (never Active),
after c.2 steps 1 to 4, using the effect parameters of that state (c.5).
The Tooltip set is resolved for the palette in the Active state only (f.2),
but its Inactive and Disabled resolutions follow the same rules when a
caller asks for them.

### e.2 Order of application within one (state, set)

Let `BG0` be the set's BackgroundNormal **before** any effect (after the
selection tint of c.2 step 4, if any).

1. **Foregrounds and decorations first**, each against `BG0`: for each of
   the 8 foregrounds and the 2 decorations, apply the foreground chain
   (e.3) with background `BG0`.
2. **Then the two stored backgrounds**: BackgroundNormal and
   BackgroundAlternate each get the background chain (e.3).
3. Only after both steps are the computed backgrounds derived (f.1), from
   the post-effect BackgroundNormal and the post-effect foregrounds.

### e.3 The chains

**Background chain** on a colour X:

1. Intensity: shade gives `shade(X, IntensityAmount)` (chroma amount 0);
   darken gives `darken(X, IntensityAmount)` (chroma factor 1); lighten
   gives `lighten(X, IntensityAmount)` (chroma factor 1); none leaves X.
2. Colour, on the result: desaturate gives `darken(X, 0, 1 - ColorAmount)`
   (luma amount 0, chroma factor `1 - ColorAmount`); fade gives
   `Mix(X, Color, ColorAmount)`; tint gives `Tint(X, Color, ColorAmount)`;
   none leaves it.

**Foreground chain** on a colour X against background G:

1. Contrast: fade gives `Mix(X, G, ContrastAmount)`; tint gives
   `Tint(X, G, ContrastAmount)`; none leaves X.
2. Then the background chain (intensity, then colour) on the result.

Decorations use the foreground chain.

### e.4 Breeze consequences

- **Disabled** (all three Breeze files): every foreground and decoration
  becomes `darken(Mix(X, BG0, 0.65), 0.1)`; each background becomes
  `darken(X, 0.1)`. Example (Breeze Light Window): ForegroundNormal
  35,38,41 becomes 160,161,163; BackgroundNormal 239,240,241 becomes
  227,229,231 (computed from this specification).
- **Inactive**: Enable is false, so foregrounds, backgrounds and
  decorations pass through exactly. ChangeSelectionColor is true, so the
  Inactive **Selection** set is the Window set (or `[Colors:Window][Inactive]`
  if present) with both backgrounds tinted 0.4 toward the Selection
  BackgroundNormal, and with the Window set's foregrounds. Example (Breeze
  Light): Inactive Selection BackgroundNormal is
  `Tint(239,240,241, 61,174,233, 0.4)` = 194,224,245, BackgroundAlternate
  `Tint(227,229,231, 61,174,233, 0.4)` = 176,216,243, ForegroundNormal
  35,38,41 (computed from this specification).
- **Disabled Selection** is the Window set with the Disabled effects and no
  tint.
- **Header, Inactive**: Breeze has `[Colors:Header][Inactive]`, which
  supplies the Inactive Header colours (no effects change them).

A scheme that enables Inactive effects (Oxygen-style schemes) gets the
full chains; Teksilo must honour them.

---

## f. Computed backgrounds and palette roles

### f.1 Computed backgrounds

For each (state, set), from the post-effect values:

| Background | Value |
|---|---|
| ActiveBackground | Tint(BackgroundNormal, ForegroundActive, 0.3) |
| LinkBackground | Tint(BackgroundNormal, ForegroundLink, 0.3) |
| VisitedBackground | Tint(BackgroundNormal, ForegroundVisited, 0.3) |
| NegativeBackground | Tint(BackgroundNormal, ForegroundNegative, 0.3) |
| NeutralBackground | Tint(BackgroundNormal, ForegroundNeutral, 0.3) |
| PositiveBackground | Tint(BackgroundNormal, ForegroundPositive, 0.3) |

Examples (Breeze Light View, Active): ActiveBackground 236,246,252;
NegativeBackground 250,236,237; NeutralBackground 254,241,234;
PositiveBackground 221,251,232 (computed from this specification).

### f.2 Palette roles (the QPalette-equivalent table)

For each state (Active, Inactive, Disabled):

| Palette role | Source |
|---|---|
| Window | Window BackgroundNormal |
| WindowText | Window ForegroundNormal |
| Base | View BackgroundNormal |
| Text | View ForegroundNormal |
| AlternateBase | View BackgroundAlternate |
| Link | View ForegroundLink |
| LinkVisited | View ForegroundVisited |
| PlaceholderText | View ForegroundInactive |
| Button | Button BackgroundNormal |
| ButtonText | Button ForegroundNormal |
| Highlight | Selection BackgroundNormal |
| HighlightedText | Selection ForegroundNormal |
| Accent | Selection BackgroundNormal (same as Highlight) |
| ToolTipBase | Tooltip BackgroundNormal **of the Active state**, in every state |
| ToolTipText | Tooltip ForegroundNormal **of the Active state**, in every state |
| Light | ShadeRole(Window BackgroundNormal of this state, Light, k) |
| Midlight | ShadeRole(..., Midlight, k) |
| Mid | ShadeRole(..., Mid, k) |
| Dark | ShadeRole(..., Dark, k) |
| Shadow | ShadeRole(..., Shadow, k) |

`k` is the contrast value of c.6. The Header and Complementary sets have no
palette role; the preset reads them from the resolved palette directly.
KDE does not set BrightText: it keeps Qt's default, opaque white
255,255,255, in every state (`resolve.json` records it for every
scheme) [measured-qt].

Breeze Light with the default k (contrast 4, so k = 0.4) gives the Light to
Shadow values of d.10.

---

## g. Accent application

This section describes, completely, what Plasma does when an accent colour
is applied to a scheme (the colour settings page, and the command-line tool
with its accent option). Teksilo's `with_accent` must produce, for every key
listed, the same value that KDE writes into `kdeglobals`, compared after
parsing (b.9) and, for copied entries, as text.

### g.1 Inputs

- The **scheme file** S (read on its own, with no cascade underneath; its
  `[Colors:Header]` presence is S's own).
- The **accent** A. When given explicitly (the command-line accent option,
  or Teksilo's `with_accent(a)`), that colour. Otherwise
  `[General] AccentColor` read from the output configuration (the
  `kdeglobals` cascade). The command-line option accepts SVG colour names
  and hex forms [qt].
- **ACC** (accent present) is false when A is fully transparent black (all four 8-bit
  channels 0). Otherwise it is true when A was given explicitly, or when
  the output configuration has an `AccentColor` key. (An `AccentColor`
  present but unparseable gives an invalid accent with ACC true in
  KDE; Teksilo treats an unparseable accent as no accent **[divergence]**.)
- From S's `[General]`: **TB** (accent title bar) = `TitlebarIsAccentColored`,
  else `accentActiveTitlebar`, else false; **TINT** (tinting on) = S has a
  `TintFactor` key; **factor** = `TintFactor` as a real, default 0.15 (also
  used when unparseable).

### g.2 Bookkeeping keys

1. `[General] ColorSchemeHash` = lower-case hexadecimal SHA-1 of S's bytes
   (written whenever S can be read). Teksilo's `to_kconfig_text` may omit
   it; byte comparisons exclude it unless the test supplies the file bytes.
2. The command-line accent option does **not** write `[General] AccentColor`
   and does not change `[General] ColorScheme`. (The settings page writes
   `AccentColor` itself, outside this procedure.)

### g.3 The seven colour groups

The groups are processed in this order: View, Window, Button, Selection,
Tooltip, Complementary, Header. For each group G:

1. The output group G is deleted, together with its nested groups.
2. If G is Header and S has no `[Colors:Header]` group, nothing is written
   for Header (no group, no `[Inactive]`, no header accent); go to the next
   group. A scheme without Header colours must stay without them.
3. For each of the twelve keys, in the order BackgroundNormal,
   BackgroundAlternate, ForegroundNormal, ForegroundInactive,
   ForegroundActive, ForegroundLink, ForegroundVisited, ForegroundNegative,
   ForegroundNeutral, ForegroundPositive, DecorationFocus, DecorationHover:
   - If ACC and the key is one of **ForegroundActive,
     ForegroundLink, DecorationFocus, DecorationHover** (the *accent keys*):
     - ForegroundLink gets the **readable accent** (g.4), computed against
       S's own `BackgroundNormal` of this group.
     - The other three accent keys get A (written `r,g,b`).
   - Else if ACC and TINT: the key gets
     `OklabTint(S value, A, factor)` (d.15), written `r,g,b`. A key absent
     from S is not written by Teksilo **[divergence]** (KDE writes the tint
     of an undefined colour).
   - Else: the key is **copied as text** from S if S has it; nothing is
     written if S lacks it.
4. If G is **Selection** and ACC: let `Bsel = AccentBg(A, S's
   [Colors:View] BackgroundNormal)` (d.14). Write BackgroundNormal and
   BackgroundAlternate = `Bsel`; ForegroundNormal and ForegroundInactive =
   `OnColour(Bsel, active)` (d.13). These overwrite step 3's values.
   ForegroundActive, ForegroundLink and the decorations keep step 3's accent
   values (so Selection ForegroundActive becomes A, and Selection
   ForegroundLink is A made readable against S's Selection background).
5. If G is **Button** and ACC: write BackgroundAlternate =
   `AccentBg(A, S's [Colors:Button] BackgroundNormal)`.
6. **Inactive subgroup.** If S's group G has a nested `[Inactive]` group:
   for each of the twelve keys, in order:
   - if TINT (whether or not ACC): write
     `OklabTint(S's [G][Inactive] value, A, factor)` (a key absent from
     `[G][Inactive]` is not written, the same **[divergence]** as step 3);
   - else if G is Header: **copy as text** the key from S's top-level
     `[Colors:Window]` group (not from `[Colors:Header][Inactive]`) into the
     output `[Colors:Header][Inactive]`, if Window has it;
   - else copy as text from S's `[G][Inactive]` if present.

   Note the accent keys are **not** set to A inside `[Inactive]`. For the
   Breeze schemes this means the output `[Colors:Header][Inactive]` equals
   S's `[Colors:Window]` text, including Window's own DecorationFocus.
7. **Header accent.** If G is Header and ACC: let `W = S's
   [Colors:Window] BackgroundNormal`, `Wa = AccentBg(A, W)` and
   `Wt = OklabTint(W, A, factor)` (factor is 0.15 when S has no TintFactor).
   If TB: write `[Colors:Header] BackgroundNormal = Wa`,
   `[Colors:Header] ForegroundNormal = OnColour(Wa, active)`,
   `[Colors:Header][Inactive] BackgroundNormal = Wt`,
   `[Colors:Header][Inactive] ForegroundNormal = OnColour(Wt, inactive)`.
   Otherwise write nothing in this step.

### g.4 Readable link accent

Given the accent A and a background X (S's BackgroundNormal of the current
group; when S lacks it, X is treated as black, luma 0):

1. If `ratio(luma(X), luma(A)) >= 5.0`, the link is A.
2. Otherwise, if `luma(A) > luma(X)`, the link is `lighten(A)` (luma amount
   0.5, chroma factor 1); else it is `darken(A)` (luma amount 0.5, chroma
   factor 1).

Exactly one step is taken; the result is not re-checked. Example: A =
61,174,233 on 255,255,255 has ratio 2.46, so the link is darken(A) =
45,127,170 (computed from this specification).

### g.5 `[WM]`

For each key, in the order activeBackground, activeForeground,
inactiveBackground, inactiveForeground, activeBlend, inactiveBlend, write S's
`[WM]` value if S has it (read as a colour and re-formatted `r,g,b`),
otherwise the default:

| Key | Default |
|---|---|
| activeBackground | Header BackgroundNormal resolved from S for the Active state (c.2 and e) |
| activeForeground | Header ForegroundNormal, Active |
| inactiveBackground | Header BackgroundNormal, Inactive |
| inactiveForeground | Header ForegroundNormal, Inactive |
| activeBlend | Header BackgroundNormal, Active |
| inactiveBlend | Header BackgroundNormal, Inactive |

These defaults use S **before** the accent is applied.

### g.6 `[KDE]`

- `frameContrast` = S's `[KDE] frameContrast` as a real if present
  (written as read, without clamping), otherwise 0.2 (the clamped default of
  c.6 applied to S, which has no value). Written as the shortest decimal.
- `contrast` = S's `[KDE] contrast` as an integer, default 7.

### g.7 Title bar override in `[WM]`

If ACC and (TINT or TB), with `W` = S's
`[Colors:Window] BackgroundNormal`:

1. If TINT: `Wt = OklabTint(W, A, factor)`. If not TB:
   write `activeBackground = Wt` and `activeForeground = OnColour(Wt,
   active)`. In both cases write `inactiveBackground = Wt` and
   `inactiveForeground = OnColour(Wt, inactive)`.
2. If TB: `Wa = AccentBg(A, W)`; write `activeBackground = Wa`
   and `activeForeground = OnColour(Wa, active)`.

These overwrite g.5.

### g.8 Effects

For `[ColorEffects:Inactive]` then `[ColorEffects:Disabled]`, for each of the
nine keys Enable, ChangeSelectionColor, IntensityEffect, IntensityAmount,
ColorEffect, ColorAmount, Color, ContrastEffect, ContrastAmount: write S's
value as text, **writing an empty value when S lacks the key**. For Breeze,
the output `[ColorEffects:Disabled]` therefore contains `Enable=` and
`ChangeSelectionColor=` with empty values. An empty boolean reads as true
(b.9), so `Enable=` keeps the Disabled effects on, matching the absent-key
default.

### g.9 Quantisation points (summary)

- Every written colour is the E8 projection of a stored colour.
- `AccentBg` and `OnColour` work on 8-bit integer channels with the
  single/double precision split of d.12.
- `lighten`/`darken` for the link round-trip through HCY with Q16.
- `OklabTint` quantises after each of its conversions (d.15).
- Copied entries are text and are never re-quantised.

### g.10 Worked examples (Breeze Light, accent 61,174,233)

Computed from this specification; the accent oracle is authoritative.

- Selection: BackgroundNormal = BackgroundAlternate = 119,198,239;
  ForegroundNormal = ForegroundInactive = 0,0,0; ForegroundActive,
  DecorationFocus, DecorationHover = 61,174,233.
- View, Window, Button, Tooltip, Header ForegroundLink = 45,127,170 (their
  light backgrounds give a ratio below 5 and the accent is darker, so the
  accent is darkened). Selection ForegroundLink is also 45,127,170: it is
  checked against S's own Selection background 61,174,233 (ratio 1, equal
  lumas, so darkened). Complementary ForegroundLink = 61,174,233 (ratio 5.88
  against 42,46,50).
- Button BackgroundAlternate = AccentBg(A, 252,252,252) = 118,197,238.
- Breeze Dark, same accent: Selection BackgroundNormal 48,128,170 with
  ForegroundNormal 0,0,0; View, Window, Button, Tooltip and Header
  ForegroundLink = 61,174,233 (ratios 7.84, 6.84, 6.04, 6.04, 6.04); Button
  BackgroundAlternate 55,135,177.

`light_with_accent(61,174,233)` therefore differs from `light()`
(Selection background and text change), which is Plasma's behaviour and is
pinned.

### g.11 The ten swatches through the selection rule

AccentBg over the Breeze Light View (255,255,255) and Breeze Dark View
(20,22,24), with OnColour (active), computed from this specification:

| Swatch | Light selection | Text | Dark selection | Text |
|---|---|---|---|---|
| #e93a9a | 239,117,184 | black | 169,47,115 | white |
| #e93d58 | 239,119,138 | black | 169,49,68 | white |
| #e9643a | 239,146,117 | black | 169,76,47 | white |
| #e8cb2d | 238,218,107 | black | 168,148,38 | black |
| #3dd425 | 119,224,102 | black | 48,155,33 | black |
| #00d3b8 | 76,224,205 | black | 5,154,136 | black |
| #3daee9 | 119,198,239 | black | 48,128,170 | black |
| #b875dc | 205,158,230 | black | 134,88,161 | white |
| #926ee4 | 178,153,236 | black | 108,83,166 | white |
| #686b6f | 149,151,154 | black | 78,81,84 | white |

---

## h. The ten accent swatches

The colour settings page offers exactly these ten preset swatches, in this
order, with no names, tooltips or translatable labels:

| Index | Hex | 8-bit | Suggested id **[derived]** |
|---|---|---|---|
| 0 | #e93a9a | 233,58,154 | pink |
| 1 | #e93d58 | 233,61,88 | red |
| 2 | #e9643a | 233,100,58 | orange |
| 3 | #e8cb2d | 232,203,45 | yellow |
| 4 | #3dd425 | 61,212,37 | green |
| 5 | #00d3b8 | 0,211,184 | teal |
| 6 | #3daee9 | 61,174,233 | blue |
| 7 | #b875dc | 184,117,220 | purple |
| 8 | #926ee4 | 146,110,228 | violet |
| 9 | #686b6f | 104,107,111 | gray |

KDE ships no names; the ids are Teksilo's, chosen by hue, stable, and used
only for persistence (`as_str` / `from_str`). The first swatch is also the
page's fallback custom colour when no custom colour was ever saved.

---

## i. Dark predicate

**CONFIRMED.** Both xdg-desktop-portal-kde (for
`org.freedesktop.appearance color-scheme`) and the Plasma Qt platform
integration (for the application's colour-scheme hint) use the same rule:

1. Take the Window palette colour of the **Active** state (f.2, which is
   `[Colors:Window] BackgroundNormal` after resolution; effects never apply
   to Active) as 8-bit channels (E8).
2. `gray = floor((11 r + 16 g + 5 b) / 32)` (Qt's documented gray value,
   integer division) [qt].
3. Dark when `gray < 192`, light otherwise.

The portal never answers "no preference": it returns 1 (prefer dark) or 2
(prefer light). Breeze Light's Window 239,240,241 gives 239 (light); Breeze
Dark's 32,35,38 gives 34 (dark).

`is_dark()` must implement exactly this. The plan's fallback (luma
comparison) is not needed.

Other portal facts used by Teksilo's reader:

- `org.freedesktop.appearance accent-color` is the Active Highlight
  (Selection BackgroundNormal) as three doubles in 0..1 (the channel reads,
  widened).
- `org.freedesktop.appearance reduced-motion` is 1 when `[KDE]
  AnimationDurationFactor` (real, default 1.0, read from `kdeglobals`)
  equals 0 exactly, else 0.
- Namespaces served: `org.freedesktop.appearance`,
  `org.kde.kdeglobals.<Group>` for every top-level `kdeglobals` group
  (`General`, `KDE`, `Icons`, `Colors:View`, `Toolbar style`, ...; nested
  groups are not exposed), plus `org.kde.VirtualKeyboard` and
  `org.kde.TabletMode`. A read is routed to the first namespace whose name
  is a prefix of the requested group; anything else fails with "Namespace is
  not supported", and an unknown key with "Property doesn't exist". Values
  in `org.kde.kdeglobals.*` are the raw strings (expanded when `[$e]`).
- Change signals: the portal emits `SettingChanged` for
  `org.kde.kdeglobals.General ColorScheme` on a palette change (any colour
  change, not only a scheme switch), `org.kde.kdeglobals.General font` on a
  font change, `org.kde.kdeglobals.KDE widgetStyle` on a style change, and
  `org.freedesktop.appearance` `color-scheme` and `accent-color` whenever its
  own palette changes, and `reduced-motion` when `[KDE]
  AnimationDurationFactor` changes.

---
## j. Desktop settings

### j.1 Fonts

Font keys, read from the `kdeglobals` cascade as **strings** (so `[$e]`
expansion applies, b.11). When the program runs in a sandbox the platform
integration first asks the portal (`org.kde.kdeglobals.<Group>`, the key
name) and uses a non-empty answer.

| Group | Key | Default |
|---|---|---|
| General | font | Noto Sans, 10 pt, normal weight, style name Regular |
| General | fixed | Hack, 10 pt, Regular, monospace hint |
| General | smallestReadableFont | Noto Sans, 8 pt, Regular |
| General | toolBarFont | Noto Sans, 10 pt, Regular |
| General | menuFont | Noto Sans, 10 pt, Regular |
| General | taskbarFont | Noto Sans, 10 pt, Regular |
| WM | activeFont | Noto Sans, 10 pt, Regular (window titles) |

An absent or empty value gives the default. A value that does not parse
leaves the default in place.

**Font string layout** [qt]. A comma-separated list (no escaping of commas
inside the family name is done; KDE writes the family as is):

| Field | Meaning | Qt 5 form | Qt 6 form |
|---|---|---|---|
| 0 | family | yes | yes |
| 1 | point size (real; a value <= 0 means unset) | yes | yes |
| 2 | pixel size (integer; -1 or <= 0 means unset) | yes | yes |
| 3 | style hint (integer) | yes | yes |
| 4 | weight | legacy scale 0..99 | OpenType scale 1..1000 |
| 5 | style: 0 normal, 1 italic, 2 oblique | yes | yes |
| 6 | underline 0 or 1 | yes | yes |
| 7 | strike out 0 or 1 | yes | yes |
| 8 | fixed pitch 0 or 1 | yes | yes |
| 9 | always 0 | yes | yes |
| 10 | capitalisation (integer) | style name, if 11 fields | yes |
| 11 | letter-spacing type (integer) | | yes |
| 12 | letter spacing (real) | | yes |
| 13 | word spacing (real) | | yes |
| 14 | stretch (integer, 0 = any) | | yes |
| 15 | style strategy (integer; 1 is the default) | | yes |
| 16 | style name, if 17 fields | | yes |

- Accepted field counts: 2 (family and point size), 10 or 11 (Qt 5), 16 or
  17 (Qt 6). Qt 6.11 and later append further fields (font features,
  variable axes); KDE cuts the string to its first 17 fields when running on
  an older Qt, and Teksilo must do the same (read fields 0 to 16, ignore the
  rest). Any other count is invalid and leaves the default.
- The weight field is on the OpenType scale when the string has 16 or more
  fields, and on the legacy scale when it has 10 or 11.
- **Legacy weight mapping** [qt]: map a legacy weight w to the OpenType
  weight of the **nearest** legacy named weight in this table, ties going to
  the lighter one: 0 to 100, 12 to 200, 25 to 300, 50 to 400, 57 to 500,
  63 to 600, 75 to 700, 81 to 800, 87 to 900. (So 50 is 400, 60 is 500,
  70 is 700.)
- A typical Plasma 6 value is
  `Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1` (16 fields, OpenType weight
  400, no style name). A Plasma 5 value is
  `Noto Sans,10,-1,5,50,0,0,0,0,0` (legacy weight 50).
- Pixel size: Teksilo converts a point size to logical pixels as
  `px = pt x 96 / 72` (the logical DPI at scale 1). A positive pixel-size
  field is used as is.
- When KDE writes a font whose weight is normal and whose style name is
  Regular, Normal, Book or Roman, it drops the style name.

### j.2 `AnimationDurationFactor`

`[KDE] AnimationDurationFactor` in `kdeglobals`, a real. Absent means 1.0.
Three upstream consumers, each with its own rule:

1. **Breeze widget style** (QStyle animations). Only when the key is
   **present**: `d = trunc(factor x 100)`, the product formed in single
   precision. If `d > 0`, style animations are on with duration `d` ms;
   otherwise they are off. When the key is absent, `breezerc [Style]
   AnimationsDuration` (100) and `AnimationsEnabled` (true) apply. The
   progress-bar busy animation is **not** scaled (its duration is
   `ProgressBarBusyStepDuration`, 800 ms, and it is switched by
   `ProgressBarAnimated`).
2. **Kirigami and the desktop QtQuick style** (Kirigami units). Let
   `f = max(0, factor)` and `base = plasmarc [Units] longDuration`
   (default 200). Then `long = max(1, round(base x f))` (round half up),
   `short = floor(long / 2)`, `veryShort = floor(long / 4)`,
   `veryLong = 2 long`. The tooltip delay (700 ms) and the "human moment"
   (2000 ms) are not scaled.
3. **Reduced motion.** The portal reports reduced motion when the factor
   equals 0 exactly; the platform integration (on Qt 6.12 or later) when
   its absolute value is at most 1e-5 (Qt's documented single-precision
   near-zero test [qt], applied by the platform integration [PI]).

Teksilo's rule for `from_desktop` **[derived]**, following consumer 2,
which drives the controls whose timing the preset reproduces:
`duration_normal = max(1, round(200 f))`,
`duration_fast = floor(duration_normal / 2)`,
`duration_slow = 2 duration_normal`,
`duration_collapse = duration_normal`,
`duration_indeterminate_sweep = 800` (unscaled, consumer 1), tooltip
delays unscaled. A factor of 0 is reported as reduced motion (and gives
normal 1, fast 0, which the preset treats as "no animation"). This differs
from the plan's "scale every non-zero duration" in two places: the busy
sweep is not scaled, and fast is derived from normal by halving.

### j.3 `forceFontDPI`

Plasma 6 removed font DPI forcing. The 6.0 configuration update deletes
`forceFontDPIWayland` from `kcmfonts [General]`, the fonts settings page has
no DPI setting any more (scaling is global, set by the display scale), and
no plasma-workspace 6.8 component reads `forceFontDPI` either: the Plasma 5
X resource loader that applied it no longer exists in the 6.8 tree.
Teksilo must read neither key on either windowing system (see n.8).

### j.4 `widgetStyle`

`[KDE] widgetStyle` in `kdeglobals`, a string. Absent or empty means
Breeze. Style names are matched ignoring ASCII case. `native_theme` returns
a Breeze theme only when the value is absent, empty, or equal to `breeze`
ignoring case; any other value (`fusion`, `oxygen`, `kvantum`, ...) keeps
Teksilo's generic geometry with the KDE colours.

### j.5 Other `kdeglobals [KDE]` and related keys (informative)

| Key | Type | Default | Use |
|---|---|---|---|
| [KDE] contrast | int | 7 | c.6 |
| [KDE] frameContrast | real | 0.2 | c.6, k |
| [KDE] AnimationDurationFactor | real | 1.0 | j.2 |
| [KDE] widgetStyle | string | breeze | j.4 |
| [KDE] LookAndFeelPackage | string | org.kde.breeze.desktop | b.8 |
| [KDE] SingleClick | bool | false | item activation |
| [KDE] ShowIconsOnPushButtons | bool | true | button icons |
| [KDE] CursorBlinkRate | int ms | 1000 | 0 means no blink, otherwise bounded to 200..2000 |
| [KDE] DoubleClickInterval | int ms | 400 | |
| [KDE] StartDragDist | int px | 10 | |
| [KDE] StartDragTime | int ms | 500 | |
| [KDE] WheelScrollLines | int | 3 | |
| [Icons] Theme | string | breeze | icon theme |
| [Toolbar style] ToolButtonStyle | string | TextBesideIcon | |

### j.6 `breezerc`

Read from the `breezerc` cascade (b.6 rules; system layers are
`CDn/breezerc` ... `CD1/breezerc`, the user layer `CH/breezerc`).

| Group | Key | Type | Default | Notes |
|---|---|---|---|---|
| Common | ShadowStrength | int | 255 | range 25..255; strength = value / 255 |
| Common | ShadowSize | enum | ShadowLarge | ShadowNone, ShadowSmall, ShadowMedium, ShadowLarge, ShadowVeryLarge (stored as their names or indices) |
| Common | ShadowColor | colour | 0,0,0 | |
| Common | OutlineCloseButton | bool | false | decoration only |
| Style | AnimationsEnabled | bool | true | |
| Style | AnimationSteps | int | 10 | animation progress is quantised to multiples of 1 / steps |
| Style | AnimationsDuration | int ms | 100 | overridden by j.2 rule 1 |
| Style | StackedWidgetTransitionsEnabled | bool | false | |
| Style | ProgressBarAnimated | bool | true | busy animation |
| Style | ProgressBarBusyStepDuration | int ms | 800 | |
| Style | ScrollBarAddLineButtons | int | 0 | 0 none, 1 single, any other value double |
| Style | ScrollBarSubLineButtons | int | 0 | same |
| Style | MnemonicsMode | enum | MN_AUTO | MN_NEVER, MN_AUTO, MN_ALWAYS |
| Style | ToolBarDrawItemSeparator | bool | true | |
| Style | ViewDrawFocusIndicator | bool | true | item-view focus rectangle |
| Style | SliderDrawTickMarks | bool | true | |
| Style | ViewDrawTreeBranchLines | bool | true | |
| Style | ViewInvertSortIndicator | bool | true | |
| Style | TabBarDrawCenteredTabs | bool | false | |
| Style | DockWidgetDrawFrame | bool | false | |
| Style | SidePanelDrawFrame | bool | false | |
| Style | MenuItemDrawStrongFocus | bool | true | k.5.13 |
| Style | WindowDragMode | enum | WD_MINIMAL | WD_NONE, WD_MINIMAL, WD_FULL |
| Style | WindowDragWhiteList | string list | empty | |
| Style | WindowDragBlackList | string list | empty | |
| Style | SplitterProxyEnabled | bool | true | |
| Style | SplitterProxyWidth | int px | 12 | |
| Style | WidgetExplorerEnabled | bool | false | |
| Style | DrawWidgetRects | bool | false | |
| Style | MenuOpacity | int percent | 100 | k.5.13 |

### j.7 Kirigami units (KF 6.31)

| Unit | Value |
|---|---|
| gridUnit | 18, a **constant** in KF 6.31 (the desktop units do not derive it from the font) |
| smallSpacing | 4 |
| mediumSpacing | 6 |
| largeSpacing | 8 |
| cornerRadius | 5 |
| veryShortDuration / shortDuration / longDuration / veryLongDuration | 50 / 100 / 200 / 400 at factor 1 (j.2) |
| toolTipDelay | 700 |
| humanMoment | 2000 |
| icon sizes small / smallMedium / medium / large | 16 / 22 / 32 / 48 |

---

## k. Breeze geometry and painting

### k.1 Metric constants

All in logical pixels unless stated. [M]

| Constant | Value |
|---|---|
| ArrowSize | 10 |
| SmallArrowSize | 5 |
| Frame width (frame margin) | 2 |
| Frame radius | 5 |
| Layout top-level margin | 10 |
| Layout child margin | 6 |
| Layout default spacing | 6 |
| Line edit frame width | 6 |
| Menu frame width | 0 |
| Menu item margin width | 4 |
| Menu item highlight gap (menu margin) | 4 |
| Menu item extra left margin | 4 |
| Menu item margin height | 4 |
| Menu item item spacing | 2 |
| Menu item accelerator space | 16 |
| Menu item text left margin | 8 |
| Combo box frame width | 6 |
| Combo box item margin width | 3 |
| Spin box frame width | 6 |
| Spin box arrow button width | 20 |
| Group box title margin | 4 |
| Button minimum width | 80 |
| Button margin width | 6 |
| Button item spacing | 4 |
| Tool button margin width | 6 |
| Tool button item spacing | 4 |
| Tool button inline indicator width | 12 |
| Menu button indicator width | 20 |
| Check box size (indicator) | 20 |
| Check box focus margin width | 2 |
| Check box item spacing | 4 |
| Check box radius | 4 (frame radius minus 1) |
| Menu bar item margin width / height | 10 / 6 |
| Scroll bar extent | 21 |
| Scroll bar slider (handle) width | 8 |
| Scroll bar minimum slider length | 20 |
| Scroll bar button heights: none / single / double | 3 / 21 / 42 |
| Tool bar frame width | 0 |
| Tool bar handle extent / handle width | 10 / 6 |
| Tool bar separator width | 8 |
| Tool bar extension width | 20 |
| Tool bar item margin / item spacing | 6 / 0 |
| Tool bar separator vertical margin | 2 |
| Progress bar busy indicator size | 14 |
| Progress bar thickness | 6 |
| Progress bar item spacing | 4 |
| MDI title bar margin width | 4 |
| Slider tick length | 8 |
| Slider tick margin width | 2 |
| Slider groove thickness | 6 |
| Slider control thickness | 20 |
| Tab margin height / width | 4 / 8 |
| Tab minimum width | 80 |
| Tab minimum height / static-tab minimum height (static: k.5.14) | 30 / 34 |
| Tab item spacing | 8 |
| Tab overlap / tab-bar base overlap | 1 / 2 |
| Tab active-effect (indicator) size | 3 |
| Tab widget margin width | 3 |
| Tool box tab min width / item spacing / margin width | 80 / 4 / 8 |
| Tooltip frame width | 3 |
| Header margin width / item spacing / arrow size | 6 / 4 / 10 |
| Item view arrow size | 10 |
| Item view item margin left / right | 2 / 2 |
| Item view item margin top / bottom | 1 / 1 |
| Item view first item top margin | 2 |
| Item view item padding width / height | 2 / 2 |
| Side panel item margin width | 4 |
| Splitter width | 1 |
| Shadow overlap | 2 |
| Blend value (hover and press washes) | 0.3 |
| Focus lighten factor | 110 (percent, d.11) |
| Pen width for symbols, frames and shadows | 1.001 |

Pen width 1.001 is an antialiasing workaround; Teksilo uses 1. Where a
radius is computed from a pen width, the pen width is truncated to the
integer 1 first.

Style hints [S]: submenu popup delay 150 ms; menus scrollable; sloppy
submenus on; dialog buttons have icons; form layout labels right-aligned,
fields grow; tab bar alignment leading (centred when
`TabBarDrawCenteredTabs`); focus frame drawn above the widget.

### k.2 Painting conventions

- **Stroked rectangle:** a rectangle inset by half the pen width on every
  side (0.5 for the 1 px pen), so a 1 px outline lies inside the bounds.
- **Frame radius for a stroked frame:** 5 - 0.5 = **4.5**.
- **Shadowed rectangle:** the bounds inset by 1 on every side (room for a
  1 px shadow).
- **Composite over:** "X at alpha a over Y" means X with its alpha
  multiplied by a, source-over composited on opaque Y.
- **With alpha a** means the alpha is **replaced** by a (absolute); **alpha
  times a** means multiplied (applied only for 0 <= a < 1).
- **Current group:** painters use the widget's own colour group (Active in
  an active window, Inactive in an inactive window, Disabled when disabled)
  unless a rule says "Active Highlight", which ignores window activity.

### k.3 Shared colours

With `fc` the frame contrast (c.6, default 0.2) and palette roles from f.2
of the current group:

| Name | Value |
|---|---|
| frame outline | Mix(Window, WindowText, fc) |
| separator | Mix(Window, WindowText, fc) |
| frame background | Mix(Window, Base, 0.3) |
| focus | View DecorationFocus of the current group |
| hover | View DecorationHover of the current group |
| focus outline | Mix(focus, WindowText, 0.15) |
| hover outline | Mix(hover, WindowText, 0.15) |
| arrow colour for a text role | Mix(that text role, its background role, 0.15); for example Mix(WindowText, Window, 0.15) |
| button outline | Mix(Button, ButtonText, fc) |
| slider outline | Mix(Button, ButtonText, fc) |
| shadow | black with alpha 0.125 |

Input-frame outline state: focus wins over hover; focused uses focus,
hovered uses hover, otherwise the frame outline. Transitions between states
are animated (k.8).

### k.4 Computed sizes

Let `H` be the font line height in pixels (l.2; 18 for Noto Sans 10 pt) and
`I` the small icon size (16). [S] unless tagged.

| Control | Size rule | H = 18 |
|---|---|---|
| Push button, text | height `H + 2x6 + 2x2 = H + 16`; width `max(textW [+ icon 16 + 4] [+ menu 20 + 4] + 12, 80) + 4` | 34 high, at least 84 wide |
| Push button, icon only | `I + 16` high, no minimum width; 31 wide with a 16 px icon [measured-qt] | 32 |
| Tool button, auto-raise | contents + 12 (contents = icon) | 28 (16 px icon), 34 (22 px) |
| Tool button, framed | contents + 16 | 32 (16 px), 38 (22 px) |
| Line edit | `max(H, 14) + 2` (Qt's inner height [qt]) `+ 2x6` | 32 |
| Combo box | `max(H, 20) + 2x6`; width `textW + 12 + 20 + 2 + 4` | 32 |
| Spin box | inner edit `max(H, 14) + 2`, `+ 2x6`, at least 20; width `+ 20` | 32 |
| Check box / radio row | `max(H + 2x2, 20)`; width `20 + textW + 4` (`+ 4` with an icon) | 22 |
| Check box / radio indicator | 20 x 20, label at x = 24 | |
| Switch (desktop QtQuick style) | 36 x 18 (k.5.9) [Qd] | |
| Slider | thickness 20; tick marks add 3 per side (Qt's built-in 5 minus Breeze's 2) [measured-qt] | 20 (23 one side, 26 both) |
| Scroll bar | extent 21, minimum handle 20 | |
| Progress bar | groove 6; Qt's hint height `H + 8` [qt] [measured-qt] | 26 |
| Menu item | `max(H, 20, iconW) + 1 + 2x4` | 29 |
| Menu separator | 9 x 5 (`1 + 2x4` by `1 + 4`) | 5 high |
| Menu section (separator with text) | `max(H_bold, 20, iconW [, 20 if checkable]) + 4 + 4` | 28 |
| Menu bar item | `H + 2x6`, width `textW + 2x10` | 30 |
| Tab | `max(H + 2x4, 30)` (34 for static tabs); width at least 80 with text | 30 |
| Header section | `max(H, 10 with a sort arrow, 22 with an icon) + 2x6` | 30 |
| Item view row (list, tree) | text only: `H + 1 + 1 + 2x2`; with an icon: `max(H, icon) + 2 + 1 + 1 + 2x2` (Qt adds 2 when the item has a decoration) [measured-qt]; with a check box: `max(H, 20) + 1 + 1 + 2x2` [measured-qt]; first row `+ 1` (top margin 2), minus 1 when the view has a frame | 24 (first 25); 26 with a 16 px icon or a check box |
| Table row | Qt base height `+ 2 x (1 + 1)`, width `+ 2 x (2 + 2)`; the table view adds 1 for its grid line [measured-qt] | 22 (23 per row with the grid) |
| Tooltip | content inset `3 + 1 = 4` on every side (3 from the style, 1 added by Qt) [measured-qt]; height `H + 8` | 26 for one line |
| Splitter | 1 px visual, 12 px hit proxy when enabled (j.6) | |
| Group box title margin | 4 | |
| Dialog margin | 10 (top-level layout margin); child margin 6; spacing 6 | |

Menu item width: `textW + 2x4` plus the icon column (`iconW + 2`) when any
item has an icon and icons are shown, the check column (`20 + 2`) when any
item is checkable, the right column (`20 + 2`), and the accelerator gap 16
when the item has a shortcut (the shortcut's own width is added by Qt).

**Other font sizes** [measured-qt]. The rules above are exact at the
default 10 pt (`H = 18`). At other sizes Qt's text box, which several
controls measure instead of the font height, can exceed `H` by 1 to 3 px
(the combo box at 11 pt), so the `H`-based rules can be short by that
much. `sizes.json` records the
measured heights; Teksilo keeps the deterministic `H` rules and documents
the difference **[divergence]**:

| pt | H (l.2) | push | line edit | combo | spin | check | menu item | menu section | tab | header | list row | list row, icon | table row | progress |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 8 | 15 | 32 | 29 | 32 | 29 | 20 | 29 | 28 | 30 | 27 | 21 | 24 | 20 | 23 |
| 9 | 17 | 34 | 31 | 32 | 31 | 21 | 29 | 28 | 30 | 29 | 23 | 25 | 22 | 25 |
| 10 | 18 | 34 | 32 | 32 | 32 | 22 | 29 | 28 | 30 | 30 | 24 | 26 | 23 | 26 |
| 11 | 20 | 38 | 34 | 35 | 34 | 25 | 30 | 28 | 30 | 33 | 27 | 29 | 26 | 28 |
| 12 | 22 | 38 | 36 | 36 | 36 | 26 | 31 | 30 | 30 | 34 | 28 | 30 | 27 | 30 |
| 14 | 26 | 42 | 40 | 40 | 40 | 30 | 35 | 34 | 34 | 38 | 32 | 34 | 31 | 34 |

(Table rows include the 1 px grid line.)

Teksilo lifts (planned): table row 22 to 24 (D7) and check/radio row 22 to
a 24 node with unchanged paint (D8) [wcag].

### k.5 Painting rules per control

Each rule names the fills and pens; geometry uses k.2.

**k.5.1 Push button (normal).** Bounds inset by 1 (shadowed rectangle),
then stroked; radius 4.5; 1 px pen.

In k.5.1 and k.5.2, **Active Highlight** means the Active group's
Highlight for an enabled button and the Disabled group's Highlight for a
disabled one, whatever the window state (an inactive window's muted
Highlight is never used here).

- Fill: Button. Pen: button outline.
- Hovered, keyboard-focused (focus made visible by keyboard) or pressed,
  when enabled: pen becomes Active Highlight.
- Pressed (enabled): fill Mix(Button, Active Highlight, 0.3).
- Checked: fill Mix(Button, ButtonText, 0.125); pen button outline.
- Default button, only in an active window: fill
  Mix(Button, Active Highlight, 0.2); pen
  Mix(Active Highlight, button outline, 0.5).
- Shadow: only in an active window, only when the button is enabled and
  neither flat, pressed nor checked: a 1 px outline of the shadow colour
  (black alpha 0.125) around the shadowed rectangle, moved down so that it
  shows as a 1 px line below the button (radius 4.5). Equivalent to a
  shadow offset (0, 1), blur 0.
- Label: ButtonText (WindowText for flat buttons); icon 16 + 4 spacing;
  menu arrow 20 + 4.
- Transitions: the fill animates toward the pressed fill and the pen toward
  Highlight over the style animation duration (k.8).

**k.5.2 Flat button (tool button with auto-raise, flat push button).**

- Idle (not hovered, pressed, checked or keyboard-focused): nothing is
  painted.
- Hovered or keyboard-focused: no fill (transparent), pen Active Highlight.
- Pressed: fill Active Highlight at alpha 0.3.
- Checked: fill ButtonText at alpha 0.125, pen button outline.
- Default (active window): fill Active Highlight at alpha 0.125, pen
  Mix(Active Highlight, button outline, 0.5).
- Never a shadow.

**k.5.3 Line edit.** Fill Base; pen the input-frame outline (k.3); stroked
frame radius 4.5. When the rectangle is shorter than `2x6 + H`, only the
Base fill is painted (no frame). Text inset 6 when there is room.

**k.5.4 Combo box.** Non-editable: the push-button frame of k.5.1 with
"pressed" also meaning "popup open". Editable: the line-edit frame. Arrow:
a 10 px chevron (k.5.17) centred in a 20 x 20 box at the trailing edge
inside the 2 px frame margin; colour arrow colour of ButtonText
(non-editable) or WindowText (editable), hover colour when its sub-control
is hovered (editable), WindowText when focused and not hovered
(non-editable), Disabled colours when empty or disabled.

**k.5.5 Spin box.** Line-edit frame. Arrow column 20 wide at the trailing
edge inside the 2 px frame margin; arrow height `min(inner height, 20)`
centred, split into two halves of `floor(h/2)` (10 and 10 at the default
size). Chevrons in arrow colour of Text; hover colour on the hovered half;
Disabled arrow colour at the limit. No button chrome in the arrow column.

**k.5.6 Check box.** Indicator 20 x 20.

- Box: the indicator inset by 2 (16 x 16), stroked (15 x 15 at offset 2.5),
  radius 4. Pen: separator colour when off; Highlight (current group) when
  on or partial. Fill: Button (Button darkened 110 percent, d.11, while
  pressed).
- On or partial: the same rounded box filled again with Highlight at alpha
  0.3 over it. [measured-qt] Qt's raster painter composites that wash over
  the opaque Button fill in 8-bit sRGB, rounding each channel
  (`(t a + b (255 - a) + 127) / 255` with the wash's alpha read as a byte),
  so it lands as one opaque fill: Plasma's samples are #c2e4f6 for Breeze
  Light and #2f5368 for Breeze Dark (`check_box.on` in the golden pixels).
  A compositor that blends in linear light gives another shade of the same
  colour (#d9e8f7 for Breeze Light). [measured-qt] The second fill is painted
  with the outline pen still set, so the outline is laid twice: the
  antialiased edge of a marked box is denser than one stroke gives (the
  corner pixels of Plasma's checked box fit two strokes to within 15 levels,
  one stroke no closer than 33).
- Check mark (on): a polyline through, in the 16 x 16 box's coordinates
  (box at (2,2) inside the indicator, centre (10,10) of the indicator):
  (left + 4, centre y), (centre x - 1, bottom - 5), (centre x + 4.5,
  top + 5.5), where left = top = 2, bottom = 18 and the centre is (10, 10):
  so (6, 10), (9, 13), (14.5, 7.5) in indicator coordinates; pen 2 px, Text
  colour (View text), mitre joins, [measured-qt] square caps (Qt's default
  pen cap), so the mark runs half the pen width, 1 px, past both end points
  (`check_box.on` covers (5, 10), the pixel before the first point, at 0.83).
- Partial: three 2 x 2 squares in Text colour, centred on the box centre and
  4 px to each side.
- Hover (enabled): an extra 1 px ring in the focus colour on the box
  outline (same 15 x 15 rounded rectangle, radius 4).
- Toggle animation: the mark is drawn progressively (dash length grows with
  the animation progress); partial dots appear at 1/3, 2/3 and 3/3.

**k.5.7 Radio button.** Indicator 20 x 20.

- Disc: the indicator inset by 2 and then by 0.5 (15 x 15 circle at 2.5).
  Pen: separator colour when off, Highlight when on. Fill Button (darkened
  110 percent while pressed). On: filled again with Highlight at alpha 0.3,
  [measured-qt] composited over the face as one opaque fill and with the
  outline laid twice, as for the check box (k.5.6; `radio_button.on`).
- Dot (on): a filled circle of radius `floor((18 - 12) / 2) = 3` (6 px
  diameter) at the centre, Text colour.
- Hover: a 1 px ring in the focus colour on the 15 x 15 circle.
- Animation: the dot radius grows with the animation progress.

**k.5.8 Focus frame (keyboard focus on buttons, fields, checks, sliders).**
An annulus filled with Highlight alpha times 0.3: inner edge the widget
rectangle inset by 1 with radius 5, outer edge that inner rectangle grown
by 2 with radius 7 (for check boxes and radio buttons, the inner edge is
the indicator box; for radio buttons and dials both edges are circles). So
the ring runs from 1 px inside to 1 px outside the control, 2 px wide.
This is what the preset's focus-ring tokens transcribe (width 2, offset -1).

**k.5.9 Switch (desktop QtQuick style, as used by Kirigami apps)** [Qd].
Uses the **Button** colour set.

- Indicator: height = Kirigami gridUnit (18), width = 2 x height (36).
- Track: the indicator inset vertically by `floor(height / 6) = 3` top and
  bottom (36 x 12), radius `round(12 / 2) = 6`. Fill: ButtonText at 10
  percent over Button background (that is, Button background at alpha 0.9
  composited over ButtonText). Border (1 px): Highlight when checked,
  otherwise the handle border colour.
- Checked fill: a rectangle from the track's leading edge to the handle's
  trailing edge, same radius, Highlight at alpha 0.5.
- Handle: a circle of the full height (18 x 18), radius `floor(18 / 2) = 9`,
  fill Button background, 1 px border: the Button set's DecorationHover when
  hovered or keyboard-focused, otherwise Mix(Button background, ButtonText,
  fc).
- Handle position: `x = clamp(p x 36 - 9, 0, 18)` where p is the visual
  position (0 off, 1 on; mirrored in right-to-left layouts).
- Label: spacing 4 (smallSpacing), padding 1; disabled label opacity 0.6.
- Motion in k.8.

**k.5.10 Slider.** Thickness 20. Handle box 20 x 20, centred across the
groove, positioned along it by the value over `length - 20`.

- Groove: 6 thick, centred, full length, drawn on the rectangle inset by
  0.5, radius 3. Background part (whole groove): pen WindowText with alpha
  fc, fill WindowText at alpha 0.2 x 0.7 = 0.14 over Window. Fill part
  (from the start to the handle centre; mirrored in right-to-left; for
  vertical sliders from the top unless inverted): pen Highlight with alpha
  fc, fill Highlight at alpha 0.7 over Window. Disabled: only the background
  part.
- Handle: the box inset by 1 (18), stroked (17 px circle at 1.5); fill
  Button; pen slider outline, hover colour when the handle is hovered, focus
  colour when focused; a shadow ellipse of the shadow colour (black 0.125),
  1 px, offset toward the bottom right, drawn unless pressed.
- Ticks (when enabled in breezerc and requested): lines 8 long, 2 px away
  from the groove, Highlight for ticks at or below the value, separator
  colour above it.

**k.5.11 Scroll bar.** Extent 21 including a 1 px separator line on the
leading edge (top for horizontal bars, left in left-to-right vertical bars,
right in right-to-left), painted in **Text** (View text) with alpha fc.
**No track or groove is painted.** Buttons none by default (j.6).

- Handle: 8 wide, centred in the remaining 20 px (so 6 px from the
  separator side), stroked (painted 7 wide), radius
  `0.5 x min(width, height, 8) = 4`, positioned and sized as Qt's
  proportional thumb (length `space x page / (range + page)`, at least 20).
- Handle colour C: WindowText with alpha times 0.5 when idle; hover colour
  when the handle is hovered; focus colour when the scroll area has focus
  (hover wins). With animations on, C's alpha is multiplied by
  `0.7 + 0.3 g`, where g goes from 0 to 1 while the pointer is anywhere over
  the scroll bar.
- Painted fill: C at alpha times 0.5 over Window. Pen: C **with alpha fc**.
  So the idle, not-hovered thumb is WindowText at effective alpha
  `0.5 x 0.7 x 0.5 = 0.175` over Window with a WindowText outline at alpha
  fc; with the pointer over the bar, 0.25; a hovered handle is the hover
  colour at 0.5 over Window.

**k.5.12 Progress bar.** Groove 6 thick, centred, radius 3, drawn on the
rectangle inset by 0.5 (inset horizontally by the frame width, normally 0).

- Groove: pen WindowText with alpha fc; fill WindowText at alpha 0.2 x 0.7 =
  0.14 over Window.
- Contents (determinate): from the start, length `floor(position x
  grooveLength)`; pen Highlight with alpha fc; fill Highlight at alpha 0.7
  over Window (HighlightedText instead of Highlight when the bar is in a
  selected item). A contents shorter than 6 is drawn 6 long and clipped.
  Right-to-left and inverted bars fill from the other end.
- Busy (minimum = maximum = 0): the groove rectangle filled with a repeating
  stripe pattern of period 28 px: 14 px of Highlight and 14 px of
  Mix(Highlight, Window, 0.7), offset by the animation value, radius 3. The
  offset runs linearly from 0 to 28 over 800 ms and loops
  (ProgressBarBusyStepDuration).
- Text (when visible, horizontal only): Text colour (HighlightedText when
  selected), to the trailing side of the groove with 4 px spacing.

**k.5.13 Menus (popup panel, items, separators, sections).**

- Panel: fill frame background, pen frame outline. With compositing
  (translucent popup windows) the panel is a rounded rectangle, stroked,
  radius 4.5; without, a plain rectangle. Menu frame width 0; items sit 4 px
  inside the panel (menu margins). When `MenuOpacity` is below 100 the fill
  has alpha `MenuOpacity / 100` and the pen is WindowText with alpha 0.25.
- Window shadow: k.6.
- Item highlight with `MenuItemDrawStrongFocus` true (the default): for the
  hovered or keyboard-selected item, the item rectangle (inset 4 from the
  panel) is filled with the focus colour at alpha 0.3 and outlined with the
  focus outline (1 px, stroked, radius 4.5). While pressed: fill the focus
  colour (opaque) and outline the focus colour. Item text: WindowText;
  HighlightedText while pressed.
- With `MenuItemDrawStrongFocus` false: no fill; a 1 px line under the text
  (2 px below the text's bottom) in the hover colour when selected, the
  focus colour when pressed.
- Item layout: contents rectangle = item inset 4 horizontally and 4
  vertically, then 8 more on the leading side. Check column: a 20 x 20
  check box or radio indicator (k.5.6, k.5.7) at the contents' leading edge
  minus 8, vertically centred; the text then starts 8 after the indicator.
  Icon column: `iconW` wide, then 2 + 1 spacing. Without an icon column the
  text moves 4 further (extra left margin). Submenu arrow: 20 x 20 at the
  trailing edge, arrow colour of WindowText (HighlightedText when pressed),
  text ending 3 before it. Shortcut text: right-aligned, at opacity 0.7.
- Separator: a 1 px line in the separator colour (WindowText with alpha fc
  when `MenuOpacity` is below 100), vertically centred in the 5 px item,
  inset 4 on each side.
- Section (separator with text): the text in bold, WindowText at alpha 0.7,
  leading-aligned with 4 px extra top padding; the line fills the remaining
  width starting `4 + 4 - 1 = 7` px after the text.

**k.5.14 Tabs (regular tab bars).**

- Selected tab: fill frame background (Window in document mode outside
  QtQuick); pen Mix(fill, WindowText, fc); the rectangle stroked, with the
  two outer corners rounded (radius 4.5). A **3 px Highlight bar** along the
  outer edge (top for tabs on the north, bottom for south, the outer side
  for west and east), its outer corners rounded with radius 5.
- Unselected tab: fill `Window darkened 120 percent` (d.11), corners on the
  outer edge rounded radius 5 only at the ends of the row; hovered: the hover
  colour at alpha 0.2 (animated by mixing).
- Tabs overlap by 1; padding 8 horizontal, 4 vertical; spacing 8 between
  icon, text and buttons; minimum width 80, minimum height 30.
- Static tab bars (document mode, not closable, not movable, expanding or
  vertical): minimum height 34. Selected: Window fill, then (document mode)
  Highlight with alpha fc as the background, or frame background otherwise,
  pen Mix(background, WindowText, fc), 3 px Highlight bar. Unselected: Base
  darkened 102 percent; hover colour with alpha fc on hover. Between
  unselected tabs a 1 px separator in Mix(Base darkened 102, WindowText, fc),
  inset 8 at both ends.

**k.5.15 Header section (table and tree headers).** Fill Button; hovered
Mix(Button, hover, 0.2); pressed or sorted-down Mix(Button, focus, 0.2).
A bottom line (horizontal headers) or trailing line (vertical headers) and
separators between sections, all WindowText with alpha fc, 1 px, not
antialiased. Sort indicator: a 10 px chevron, spacing 4. Margins 6.

**k.5.16 Item-view selection (lists, trees).** The item rectangle minus its
margins (2 left, 2 right, 1 top, 1 bottom; 2 top on the first row; one less
on each framed side; in row-selection views the inner columns drop their
side margins so the pill spans the whole row). Colour group: Active when the
window is active, Inactive when not, Disabled when disabled.

- Nothing is painted unless the item is hovered, selected, has a custom
  background, or is an alternate row.
- Alternate rows: AlternateBase over the full, un-inset item rectangle,
  square.
- Fill: Highlight of that colour group. Hovered and not selected: Highlight
  with alpha 0.3. Hovered and selected: Highlight lightened 110 percent
  (d.11).
- Outline: the fill colour with alpha 1.0 when selected, 0.8 otherwise; 1 px,
  on the stroked rectangle with radius 5 [measured-qt] (the control radius;
  this section gave 4.5, the radius less half the pen, as for the push button,
  but Plasma's painted rows fit 5 to within 23 levels at every corner pixel
  where 4.5 leaves up to 75: `item_view_row.selected` and `.hover` in the
  golden pixels), sides chosen by the item's
  position in the row (only the row's two ends are rounded and outlined at
  the sides).
- Tables: square cells, no rounding.
- Keyboard focus without selection (when `ViewDrawFocusIndicator`):
  an outline only, in Highlight lightened 110 percent, radius 4.5, on the
  item rectangle minus the margins.

**k.5.17 Arrows (chevrons).** A polyline in a square of side
`s = min(width, height, 10)`, centred; apex at the centre line; the two
ends at a quarter and three quarters of s; 1 px pen, square caps, mitre
joins; nudged by -0.25, +0.5 or +0.25 px when `s mod 4` is 1, 2 or 3 so the
apex lands on a pixel. [measured-qt] The nudge is along the way the arrow
points: Plasma's spin box puts the ends of its up arrow half a pixel above
the centre of its half and those of its down arrow half a pixel below.

**k.5.18 Tooltip.** Fill ToolTipBase; pen Mix(ToolTipBase, ToolTipText, fc);
with compositing a rounded rectangle, stroked, radius 4.5. Text ToolTipText.
Padding 4. Window shadow (k.6). Tooltip colours are the Active state's in
every state (f.2).

**k.5.19 Group box.** Fill frame background, pen frame outline, stroked
radius 4.5; flat group boxes have no frame. Title margin 4.

**k.5.20 Generic frames and scroll areas.** Fill Base, pen the input-frame
outline, stroked radius 4.5 (frame width 2) when the frame is shown; side
panels use a single line in the Inactive Highlight (Active Highlight when
focused).

**k.5.21 Splitter.** A 1 px handle; when `SplitterProxyEnabled`, a 12 px
invisible hit proxy.

### k.6 Shadows

**Window shadows** (menus, tooltips, combo-box popups). Each size is two
layers. The *size offset* moves both layers; the layer offset is added to
it. Radii are CSS box-shadow **blur radii** (n.4). Colour `ShadowColor`
(default black) with alpha `opacity x ShadowStrength / 255`.

| ShadowSize | Size offset | Layer 1: offset, blur, opacity | Layer 2: offset, blur, opacity | Effective layers (offset y, blur, opacity) |
|---|---|---|---|---|
| None | | | | no shadow |
| Small | (0, 3) | (0, 0), 12, 0.26 | (0, -2), 6, 0.16 | (3, 12, 0.26) and (1, 6, 0.16) |
| Medium | (0, 4) | (0, 0), 16, 0.24 | (0, -2), 8, 0.14 | (4, 16, 0.24) and (2, 8, 0.14) |
| Large (default) | (0, 5) | (0, 0), 20, 0.22 | (0, -3), 10, 0.12 | (5, 20, 0.22) and (2, 10, 0.12) |
| Very large | (0, 6) | (0, 0), 24, 0.20 | (0, -3), 12, 0.10 | (6, 24, 0.20) and (3, 12, 0.10) |

The shadow shape is the window's rounded rectangle (radius 5); the window
itself covers the shadow except where the shadow extends beyond it, with a
2 px overlap. An unknown ShadowSize value means Large.

**Control shadows.** Push button: k.5.1 (offset (0, 1), blur 0, black
0.125). Slider handle: a 1 px ellipse outline in the same colour, rotated
45 degrees so it shows at the bottom right.

Mapping to Teksilo shadow tokens: `blur` = the radius in the table, offset
as in the last column, colour black with the layer opacity; `spread` 0.
The preset's `shadow_sm`, `shadow_md`, `shadow_lg` are Small, Medium and
Large, first layer in the tokens; the styles paint both layers.
`shadow_xs` is the button shadow.

### k.7 Layout tokens

Layout spacing 6 (`control_gap`), child margin 6, top-level (dialog) margin
10, Kirigami largeSpacing 8 (`section_gap`) [K]. Radii: controls and popups
5 (painted 4.5 when stroked), check box 4, progress and slider grooves 3,
scroll handle 4, focus ring outer radius 7.

### k.8 Control motion (see m for tokens)

- Style animations (buttons, frames, check boxes, tabs, headers, scroll
  bars, combo and spin arrows): duration 100 ms (j.2), linear progress
  quantised down to multiples of 1/10 (AnimationSteps), so a transition is
  10 discrete steps.
- Switch handle position: a smoothed animation (Qt's `SmoothedAnimation`,
  documented as an ease-in-out quadratic curve [qt]) with duration
  Kirigami shortDuration (100 ms) **and Qt's default velocity of 200 px/s**;
  Qt uses whichever gives the shorter duration [qt], so the full 18 px
  travel takes 90 ms at the default factor. Not animated while the switch
  is being pressed or dragged, nor when shortDuration is 0.
- Switch handle colour: a colour animation with an ease-in cubic curve over
  shortDuration [Qd].
- Busy progress: linear, 800 ms per 28 px, looping.

---

## l. Typography

### l.1 Roles

| Teksilo style | Font | Size | Weight | Source |
|---|---|---|---|---|
| body | Noto Sans | 10 pt = 13 px face | 400 | j.1 `font` |
| body_bold | Noto Sans | 13 px face | 600 (DemiBold) | Kirigami primary heading weight [K] |
| small, small_bold | Noto Sans | 8 pt = 11 px face | 400 / 600 | j.1 `smallestReadableFont` |
| tiny | as small | | | **[derived]**: KDE has no smaller role |
| mono | Hack | 10 pt = 13 px face | 400 | j.1 `fixed` |

Letter spacing (tracking) is 0 everywhere. `from_desktop` uses the
desktop's own families and sizes (j.1).

Kirigami heading factors (documented, not rendered as separate styles):
level 1 x 1.35, level 2 x 1.20, level 3 x 1.15, level 4 x 1.10, level 5 and
higher x 1, applied to the default point size; weight DemiBold for the
primary type, normal otherwise; the secondary type at opacity 0.75 [K].

### l.2 Line height

**Facts.** Noto Sans v2.015 has units per em 1000, hhea ascender 1069,
descender -293, line gap 0 (the OS/2 typographic metrics are identical and
flagged for use; the Windows metrics 1124 and 395 are not used), so its
natural line height is 1.362 em. Hack v3.003: units per em 2048, hhea
ascender 1901, descender -483, line gap 0 (the OS/2 typographic values
1556, -492, 410 are not flagged for use; the Windows metrics equal hhea);
monospaced with an advance of 1233 units (0.60205 em).

**How Qt measures it** [measured-qt] (`sizes.json`, Noto Sans at 8, 9, 10,
11, 12 and 14 pt, Hack at 9, 10 and 11 pt, all reproduced by the rule):

1. The face is resolved at the integer pixel size `P = round(pt x 96 / 72)`
   (round half up): 10 pt is a 13 px face (resolved point size 9.75), 8 pt
   is 11 px.
2. Ascent and descent are `a = floor(64 P asc / upm) / 64` and
   `d = floor(64 P desc / upm) / 64`, with `asc` and `desc` the hhea
   ascender and the absolute descender (Noto Sans at 13 px: a = 13.890625,
   d = 3.796875).
3. The integer line height is `H = round(a) + round(d)`, each rounded half
   up separately (not `round(a + d)`, which is 1 px short at 9 and 14 pt).

Noto Sans: 8 pt 15, 9 pt 17, 10 pt 18, 11 pt 20, 12 pt 22, 14 pt 26. Hack:
9 pt 14, 10 pt 15, 11 pt 18.

**Text roles (normative).** `H` is computed by steps 1 to 3 with the
bundled face's metrics (Noto Sans for text roles, Hack for mono): 18 for
body and body_bold (10 pt), 15 for small, small_bold and tiny (8 pt), 15
for mono (Hack 10 pt). The line-height ratio given to Teksilo's text style
is `H / px` with `px` the role's size from l.1, so the line box is the
integer `H`. `from_desktop` applies the same steps to the desktop's fonts
(no font measurement): exact for Noto Sans and Hack, and, for any other
family, `H = round(P x 1.362)` **[derived]**, a documented +/-1 px
approximation. Without either font feature (l.3) the same Noto Sans and
Hack numbers are used for the fallback families.

The glyph size is the integer face size `P` from step 1 **[measured-qt]**:
13 px for the 10 pt roles and 11 px for the 8 pt roles. Qt resolves a whole
pixel size and draws that face, so Teksilo uses the same size rather than
the unrounded `pt x 96 / 72` (40/3 px), which would make every label 0.33 px
larger than in a Qt application next to it. The line-height ratio given to
Teksilo's text style is therefore `H / P` (18/13 for body, 15/11 for small).
`from_desktop` applies the same rule to the desktop's fonts.

This corrects the plan's `round(px x 1.35)` (14 instead of 15 at 8 pt) and
the earlier `round(px x 1.362)` (16 instead of 17 at 9 pt and 25 instead
of 26 at 14 pt).

**Kirigami gridUnit** is the constant 18 (j.7); it coincides with H at the
default font but does not follow the font.

### l.3 Families without the font features

With neither `bundled-fonts` nor `system-fonts`, the preset falls back to
Inter and JetBrains Mono with the same sizes **[unpublished]**.

---

## m. Motion

| Teksilo token | ms at factor 1 | Upstream source |
|---|---|---|
| duration_fast | 100 | breezerc AnimationsDuration; Kirigami shortDuration |
| duration_normal | 200 | Kirigami longDuration |
| duration_slow | 400 | Kirigami veryLongDuration |
| duration_collapse | 200 | Kirigami longDuration |
| duration_indeterminate_sweep | 800 | breezerc ProgressBarBusyStepDuration (not scaled) |
| tooltip_delay, tooltip_delay_heavy | 700 | Kirigami toolTipDelay (not scaled) |
| submenu popup delay (informative) | 150 | Breeze style hint |

Easing facts:

- QStyle (Breeze) animations are linear, quantised to 10 steps over
  100 ms (k.8). At 60 Hz that is about 6 frames, so the quantisation is
  invisible and `easing_standard` = Linear.
- The switch handle uses Qt's smoothed animation, documented as an ease-in-
  out quadratic; the preset approximates it by
  `CubicBezier(0.455, 0.03, 0.515, 0.955)` **[derived]** with a test
  bounding the maximum deviation from the quadratic ease-in-out at 0.02.
- The switch handle colour uses an ease-in cubic, `t` cubed. The preset
  writes it exactly as `CubicBezier(1/3, 0, 2/3, 0)` **[derived]**: with
  its control points at one and two thirds along and none of the way up,
  a cubic Bezier's horizontal coordinate is its parameter and its vertical
  one the parameter cubed. The common approximation
  `CubicBezier(0.55, 0.055, 0.675, 0.19)` strays 0.03 from it and is not
  used.
- An ease-out cubic appears in the QtQuick **Breeze** style (a different
  style package) for its switch border colour; the desktop style that
  Kirigami desktop apps use does not use it. Teksilo does not use
  ease-out cubic for Breeze controls.
- `from_desktop` scaling: j.2. A factor of 0 is reduced motion.

---
## n. C0 settles: answers

Each item of the plan's C0 "Settles" list, then the plan's fallback table,
then further findings. **CONFIRMED** means settled from the pinned upstream
source (cited in the ledger); **FALLBACK** means not settled, with the
plan's fallback in force and the item tagged `[unverified]` until settled.

**n.1 `expandSize`. CONFIRMED.** Expanding a size by a margin adds twice the
margin on each axis: `(w + 2 mx, h + 2 my)`; the one-argument form uses the
same margin for both axes. Every size in k.4 uses this.

**n.2 `scrollBarButtonHeight`. CONFIRMED.** No buttons: 3; single: 21 (the
scroll bar extent); double: 42. breezerc `ScrollBarAddLineButtons` and
`ScrollBarSubLineButtons` default to 0 (no buttons); 1 is single and any
other value double.

**n.3 qqc2-desktop-style SwitchIndicator. CONFIRMED.** 36 x 18 (height =
Kirigami gridUnit, width twice the height), track inset `floor(h / 6) = 3`
top and bottom, track radius 6, handle 18 x 18 with radius 9, colours and
motion in k.5.9 and k.8. The plan's fallback values (from the QtQuick
Breeze style) match the size but not the colours, borders and checked fill,
which k.5.9 now specifies.

**n.4 Shadow blur mapping. CONFIRMED.** Breeze's shadow "radius" is a CSS
box-shadow blur radius: the renderer turns it into a Gaussian of standard
deviation `radius / 2` (the CSS Backgrounds definition) and approximates
that Gaussian with three successive box blurs whose total extent is
`max(2, floor(sigma x 3 sqrt(2 pi) / 4 x 1.5 + 0.5))` px (the SVG
filter approximation). The Teksilo token `blur` therefore equals the Breeze
radius, the same convention every sibling preset uses for vendor blur
values, which is also the plan's fallback. Offsets are the combined
offsets of k.6 (size offset plus layer offset). Note: Teksilo's renderer
draws a blur `b` as a smoothstep edge with `sigma = b / 2.5`, which is
somewhat sharper than CSS; that is a renderer-wide property, not part of
this mapping.

**n.5 Portal dark predicate and namespaces. CONFIRMED.** Section i.

**n.6 Duplicate-key precedence. CONFIRMED.** Last occurrence wins within a
file and higher layers win, except that an immutable occurrence freezes the
value, so inside an immutable group or file the first occurrence wins
(b.6).

**n.7 Hack metrics. CONFIRMED.** Units per em 2048, hhea 1901 / -483 / 0
(natural ratio 1.1640625); Qt's integer line height for Hack 10 pt is 15
(l.2, [measured-qt]). The synthesised design's "about 1.2" was an
estimate.

**n.8 `forceFontDPI` on Wayland. CONFIRMED: not read.** Plasma 6.0 deletes
`forceFontDPIWayland`. In addition, **on X11 it is not read either**: the
6.8 fonts settings page has no DPI setting and no 6.8 plasma-workspace
component reads `forceFontDPI` (j.3). The plan's "text scale follows
kcmfonts forceFontDPI on X11" must be dropped (plan correction).

**n.9 `MenuItemDrawStrongFocus`. CONFIRMED.** breezerc `[Style]`, default
true; behaviour in k.5.13.

**n.10 Progress hint height. CONFIRMED for Breeze, [measured-qt] for Qt's
part.** Breeze makes a horizontal progress bar at least 6 high, and at
least the font height when its text is visible; it adds nothing else. Qt's
progress-bar size hint supplies `font height + 8` as the contents height
[qt], so the hint is `H + 8 = 26`. The C0 harness confirms 26.

**n.11 `ThemeExtension` bounds. CONFIRMED (Teksilo code).** There is no
`ThemeExtension` trait. `Theme` carries a `ThemeExtensions` registry keyed
by type; `with_extension` and `extension` require `T: Any + Send + Sync`
(which implies `'static`), and the registry already wraps each value in an
`Arc`. The registry's equality compares only which types are registered,
never their contents, so `assert_same_theme` must compare `KdePalette` by
content through `theme.extension::<KdePalette>()`, and `KdePalette` must
implement `PartialEq` itself. An inner `Arc<ResolvedPalette>` is optional.

Plan fallback table:

| Item | Status | Value in force |
|---|---|---|
| Switch geometry | CONFIRMED | n.3 (qqc2-desktop-style) |
| Dark predicate | CONFIRMED | gray < 192 (i), not the luma fallback |
| Shadow blur mapping | CONFIRMED | blur = radius (n.4), equal to the fallback |
| forceFontDPI on Wayland | CONFIRMED | not read; nor on X11 (n.8) |

Further items the synthesised design listed for C0:

- **Busy period. CONFIRMED.** 28 px of stripe offset per 800 ms, linear,
  looping; not scaled by the animation factor.
- **Flat-button hover. CONFIRMED.** Outline only, in Active Highlight, no
  fill (k.5.2).
- **Groove fill. CONFIRMED.** Slider and progress grooves: WindowText at
  0.14 over Window with a WindowText outline at alpha fc; filled parts
  Highlight at 0.7 over Window with a Highlight outline at alpha fc.
- **Wallpaper accent written to `AccentColor`. FALLBACK (not settled,
  irrelevant).** The settings-page code that writes `AccentColor` was not
  read. Teksilo reads the accent-applied `[Colors:*]` groups, so the
  question does not affect any value.
- **Window decoration metrics.** Out of scope (a.1).

Plan corrections and clarifications found while writing this specification
(each overrides the plan):

**n.12 Accent precedence, reading side.** `QPalette` Accent and Highlight
are both Selection BackgroundNormal (CONFIRMED); the portal's
`accent-color` is the same colour. `OsThemeColors.accent` = Selection
BackgroundNormal and `focus` = View DecorationFocus stand.

**n.13 `[General] AccentColor` in the no-colours fallback. DECIDED by the
user (2026-10-17): apply it.** KDE's own behaviour, recorded here as a fact:
when `kdeglobals` has no `[Colors:View]`, the platform integration builds the
palette from the look-and-feel package's colour file, else the named scheme,
else Breeze Light, and does **not** apply `AccentColor` (b.8). Teksilo
deliberately differs: it uses the same trigger (no `[Colors:View]` group) and
the same file order (look-and-feel colours, then `[General] ColorScheme`,
then Breeze Light), and then applies `[General] AccentColor`, when it is
present and not transparent, through the accent application of section g.
Reason: the accent the user chose is honoured even on a profile whose colour
groups were never written. This is a documented deviation from Qt
applications in that edge case only; with `[Colors:View]` present, the
colour groups already carry the accent and nothing is re-applied.

**n.14 `KDE_COLOR_SCHEME_PATH`.** It is an in-process application property,
not an environment variable (b.8). `color_scheme_file(name)` must not
honour an environment variable of that name.

**n.15 `[Inactive]` subgroups do not stop the Inactive effects.** The
subgroup replaces the set's source values (missing keys take the compiled
defaults, or Window's keys for Header), and the Inactive effects then still
apply (c.2 step 5). For Breeze they are disabled, so nothing changes.

**n.16 Header falls back to Window per key**, not as a whole set (c.2).

**n.17 Invalid and empty colours.** KDE yields an invalid colour, not the
default; Teksilo treats them as absent (b.9) **[divergence]**, as the plan
decided.

**n.18 Byte order mark.** KDE does not strip it (b.1); Teksilo reproduces
that and emits a diagnostic.

**n.19 Line height.** `H` follows Qt's measurement (l.2: integer pixel
size, ascent and descent rounded separately), not `round(px x 1.35)`: the
small role's line box is 15, not 14, and the rule is exact at every
measured size.

**n.20 Scroll-bar thumb.** The idle thumb fill is WindowText at effective
alpha 0.175 over Window (0.25 while the pointer is over the bar), with a
1 px WindowText outline at alpha fc; the hovered thumb is the hover colour
at 0.5 over Window. Breeze paints **no** track; the only alpha-fc element is
the separator line, in View Text. The synthesised design's
`scrollbar_thumb = window_text at 0.5 x 0.7` and `scrollbar_track_hover =
window_text at alpha fc` do not match Breeze (k.5.11); the preset's tokens
should be re-derived from k.5.11 before the WCAG lift D9.

**n.21 Progress contents.** The determinate fill is Highlight at 0.7 over
Window with an outline, not solid Highlight (k.5.12).

**n.22 Animation factor.** Busy sweep not scaled; fast derived from normal
by halving (j.2).

**n.23 Default button** blend applies only in an active window; its pen is
Mix(Active Highlight, button outline, 0.5) (k.5.1).

**n.24 Accent oracle procedure.** The command-line accent option applies
the accent to the scheme currently named in `kdeglobals` and does not check
whether that scheme is already in use; only the scheme-name argument has
that check. It also does not write `AccentColor`. The accent run
therefore only needs `[General] ColorScheme` in the scratch `kdeglobals`
to name the scheme under test; `accent_oracle.sh` gets there by applying
the scheme by name first (from a placeholder name, because the scheme-name
path refuses the scheme already in use), which also records the no-accent
`kdeglobals` as each scheme's `base`.

**n.25 16-bit storage.** All colour arithmetic stores 16-bit channels and
converts to 8 bits with `E8(q) = floor((q + 128) / 257)`, which is
`round(q / 257)` (a.3, [measured-qt] on all 65536 values). The goldens'
"8-bit equal" comparison uses `E8`; their `hex` fields are `E8` of `u16`.

**n.26 Breeze `contrast`.** All three Breeze schemes set `[KDE] contrast=4`,
so their shade roles use k = 0.4; the default 7 applies only when the key is
absent.

**n.27 kdedefaults.** It reaches programs through `XDG_CONFIG_DIRS`, which
the Plasma session start prepends; b.7 gives the rule for programs started
outside the session environment.

**n.28 Confirmed as planned** (no change): push button 34 high and at least
84 wide, radius 4.5 stroked, 1 px pen, shadow only in an active window,
Highlight outline on hover and keyboard focus; tool buttons 28 / 34 auto-raise
and 32 / 38 framed (contents taken as the icon; `sizes.json` confirms all
four);
line edit, combo and spin 32, combo arrow 20, spin column 20 split 10 + 10;
check and radio indicator 20, box 15 at 2.5, radius 4, check pen 2 with
mitre joins, radio dot 6, label at 24; slider 20 / groove 6 radius 3 /
handle 17 painted, ticks 8 with a 2 px gap, mirrored in right-to-left;
scroll bar 21 with a 1 px separator, handle 8 painted 7 radius 4, minimum
20; progress groove 6 radius 3; menu item 29, separator 5, section 28,
margin 4, text margin 8 (+4 without icons), highlight focus at 0.3 with an
outline, radius 4.5, shortcut opacity 0.7; tab 30 (34 static), minimum
width 80, padding 8, 3 px Highlight bar, unselected Window darkened 120,
hover at 0.2 (of the hover colour); header 30; item row 24 (26 with a
16 px icon or a check box, k.4) with margins 2 / 1 and radius 4.5, hover Highlight at 0.3, outline alpha 1.0 selected and 0.8
otherwise; tooltip padding 4, radius 5 (4.5 stroked); focus ring 2 px of
Highlight at 0.3 from 1 px inside to 1 px outside; `control_gap` 6,
`section_gap` 8, dialog margin 10; durations 100 / 200 / 400 / 200 / 800 /
700; ChangeSelectionColor makes the Inactive Highlight the Window colour
tinted 0.4 toward the selection colour, with the Window foreground as its
text.

---

## o. Ledger

References only. Paths are relative to the repository root at the pinned
tag (a.2); URLs have the form
`https://invent.kde.org/<group>/<repo>/-/raw/<tag>/<path>`. Line numbers
are at that tag. "Qt docs" means Qt 6 public documentation of the named
class or function.

### o.1 Files read beyond the fetch manifest

Role A fetched these additional files at the same pinned tags (stored
outside every repository):

| Repository @ tag | Path | sha256 |
|---|---|---|
| plasma/plasma-workspace @ v6.8.0 | kcms/colors/ui/AccentColorUI.qml | c62b614f1fa6da2a385b2c9bc2b0f6998ddf59bed3e7ef1860ddf1bd080367aa |
| frameworks/kconfig @ v6.31.0 | src/gui/kconfiggroupgui.cpp | 484abe215a1ef648b0bd7967b6f4c13b13dbcca9584a9030d8d2953279af92e7 |
| frameworks/kconfig @ v6.31.0 | src/core/kconfig.cpp | 679a6d335ff4bbeee349d7adf32df89031f4be6470ab18e0f4ac61605c00d825 |
| frameworks/kconfig @ v6.31.0 | src/core/kconfigdata.cpp | 42882ba136c16a8aa6fb76cf1f41407bf059ddd1b8ef2dae6b1c5043c876a15b |
| frameworks/kconfig @ v6.31.0 | src/core/kconfigdata_p.h | 87b8ed5500a825080517a84bcc8f31b615296b416a12e1325f6748e71a61585b |
| plasma/breeze @ v6.8.0 | libbreezecommon/breezeboxshadowrenderer.cpp | b52ea0760c82c3d23dccdef1407283b68049fac605ea3264306d978c619c84cd |
| plasma/breeze @ v6.8.0 | libbreezecommon/breezeboxshadowrenderer.h | 31af761f1f7f8411abddc2deaba8f4bfede466c85cb0f7288ea1a3792ce68801 |
| plasma/breeze @ v6.8.0 | kstyle/breeze.h | 8646f4b6a72eca865bc469017f2dbdc54bb348ce185a8ae0613cb7bbcc4cfd0e |
| plasma/breeze @ v6.8.0 | kstyle/animations/breezeanimations.cpp | 934931079849b8be5a4083b699759a8927cfedd3d2fb879aac77ce7e0b617427 |
| plasma/breeze @ v6.8.0 | kstyle/animations/breezeanimationdata.cpp | 97d0b393a5207abc4321ab0f5b000f9a5db9a1423e43406eec3aa179ed09ab29 |
| plasma/breeze @ v6.8.0 | kstyle/animations/breezeanimationdata.h | f7c40f0e967f1c98ed9153e8d19ee61b1e00a6f79ba99b0ee1f1cf78d44074c7 |
| plasma/breeze @ v6.8.0 | kstyle/animations/breezebusyindicatorengine.cpp | 4b25b1d871831befe8ae4ef626b1685ec2ee5325bd28286fb2a14447c89c706a |
| frameworks/qqc2-desktop-style @ v6.31.0 | kirigami-plasmadesktop-integration/plasmadesktopunits.cpp | aeca22cf2e8c4b37bde99fae4de0edbd1331595150b6d5c0bf0c1df5f43a0d1a |
| frameworks/qqc2-desktop-style @ v6.31.0 | kirigami-plasmadesktop-integration/animationspeedprovider.cpp | c3aaf8c9c221785cca717a854481d47046372bd0f9d0f5e0e9fe4531a660662a |
| qt/qtbase @ v6.11.1 | src/gui/painting/qcolor.cpp | 1f9b37229fab0eac532d4c8ef52da4fcddd3e501a8a45a78ef982da0f2fc9216 |
| qt/qtbase @ v6.11.1 | src/gui/painting/qcolorspace.cpp | 2b4a575b0cf6483106a162df7d5677b7b62faba3ad4e502a556b5bc62d141579 |
| qt/qtbase @ v6.11.1 | src/gui/painting/qcolormatrix_p.h | de9757a4012d27503773f1e8e53534e2510b603f95ace3d3bf18f96d25f823a0 |
| qt/qtbase @ v6.11.1 | src/gui/painting/qcolortransferfunction_p.h | 8cbc4d00fd53e3014a7cd4eb2e42c4eec33a0ea5d55339171e16cae8856cc8d8 |
| qt/qtbase @ v6.11.1 | src/gui/painting/qcolortransform.cpp | 75442b871d3e66a46b5da6dd839bb85b17cc9ee7e08627544a85887fa9ef10ae |
| qt/qtbase @ v6.11.1 | src/gui/painting/qcolortrc_p.h | 9ca6f329514171013d136f74e6c70b6ce1c877caa356ee2249d459fa95a6ab5a |
| qt/qtbase @ v6.11.1 | src/gui/painting/qdrawhelper_p.h | 58539076c7ef0e4d4b0df785ab24af13002f6673e126e1faffbf5b8c2857a5b9 |

The plasma-workspace v6.8.0 tree listing of `kcms/` (GitLab repository API)
was consulted to establish that no X resource loader exists (n.8).

The qtbase files come from Qt's GitHub mirror,
`https://raw.githubusercontent.com/qt/qtbase/v6.11.1/<path>`. Qt is
LGPL-3.0 or GPL; like the KDE files, these are described in prose here and
never copied. Each behaviour read from them was also confirmed black-box in
the golden image (o.12).

### o.2 Grammar and cascade (frameworks/kconfig @ v6.31.0)

| Value or rule | File | Lines |
|---|---|---|
| line trimming, blank and `#` lines | src/core/kconfigini.cpp | 92-98 |
| header parsing, nested segments, invalid header keeps group | src/core/kconfigini.cpp | 100-150 |
| `[$i]` file and group immutability | src/core/kconfigini.cpp | 116-128, 145-149, 278-283 |
| immutable group skips user-layer entries | src/core/kconfigini.cpp | 140-155 |
| key and value trimming, empty key | src/core/kconfigini.cpp | 157-171 |
| key options i, e, d; locale tags; errors | src/core/kconfigini.cpp | 178-229 |
| locale filtering | src/core/kconfigini.cpp | 231-243 |
| 100-error limit, 1.5 MiB line limit | src/core/kconfigini.cpp | 74, 78, 86, 271-274 |
| escape decoding table, `\x` rules, invalid escape | src/core/kconfigini.cpp | 706-801 |
| escape encoding (leading/trailing space as `\s`) | src/core/kconfigini.cpp | 615-704 |
| immutable entry and immutable group block writes; deletion | src/core/kconfigdata.cpp | 71-140 |
| deleted entries read as absent | src/core/kconfigdata.cpp | 221-234 |
| `$` expansion rules, QT_*_HOME fallbacks, `$$` | src/core/kconfig.cpp | 192-250 |
| group exists: non-deleted entry in group or subgroup | src/core/kconfig.cpp | 140-145, 300-303, 373-378, 1094-1101 |
| global files order: kde5rc, system.kdeglobals, kdeglobals, user files | src/core/kconfig.cpp | 110-130, 735-768, 786-850 |
| system files before user files, immutable stops user files | src/core/kconfig.cpp | 711-731 |
| typed reads: absent gives default; bool negatives | src/core/kconfiggroup.cpp | 229-269, 686-701 |
| plain-string reads expand `[$e]` | src/core/kconfiggroup.cpp | 645-662 |
| colour read: empty and `invalid`, `#` forms, names, comma form bounds | src/gui/kconfiggroupgui.cpp | 25-95 |
| colour write: `r,g,b`, `,a` when alpha not 255, `invalid` | src/gui/kconfiggroupgui.cpp | 131-152 |
| font write drops Regular/Normal/Book/Roman style names | src/gui/kconfiggroupgui.cpp | 153-171 |
| list serialisation with `,` | src/core/kconfiggroup.cpp | 133-158 |

### o.3 Session and locations (plasma/plasma-workspace @ v6.8.0)

| Value or rule | File | Lines |
|---|---|---|
| KDE_FULL_SESSION, KDE_SESSION_VERSION, XDG_CURRENT_DESKTOP | startkde/startplasma.cpp | 410-415 |
| kdedefaults prepended to XDG_CONFIG_DIRS, `/etc/xdg` default | startkde/startplasma.cpp | 421-428 |
| default ColorScheme BreezeLight; ColorSchemeHash SHA-1 | startkde/startplasma.cpp | 446-466 |
| forceFontDPIWayland deleted | kcms/fonts/kconf_update/plasma6.0-remove-dpi-settings.cpp | 13-29 |
| font defaults (kdeglobals) | kcms/fonts/fontssettings.kcfg | 7-58 |

### o.4 Colour model (frameworks/kcolorscheme @ v6.31.0)

| Value or rule | File | Lines |
|---|---|---|
| effect groups, Enable defaults, effect defaults and Color defaults | src/kcolorscheme.cpp | 22-52 |
| background chain (intensity then colour) | src/kcolorscheme.cpp | 54-80 |
| foreground chain (contrast then the background chain) | src/kcolorscheme.cpp | 82-97 |
| effect enumerations | src/kcolorschemehelpers_p.h | 50-78 |
| compiled default tables, decoration defaults 61,174,233 and 147,206,233 | src/kcolorscheme.cpp | 120-217 |
| twelve keys and their names | src/kcolorscheme.cpp | 251-278 |
| set to group mapping, Selection Inactive and Disabled use Window, tint source | src/kcolorscheme.cpp | 289-342 |
| ChangeSelectionColor defaults to Enable, default true | src/kcolorscheme.cpp | 305-307 |
| Header defaults from Window per key | src/kcolorscheme.cpp | 330-334 |
| `[Inactive]` subgroup selection | src/kcolorscheme.cpp | 344-352 |
| selection tint 0.4 on both backgrounds | src/kcolorscheme.cpp | 374-378 |
| effects for every non-Active state; foregrounds against pre-effect background | src/kcolorscheme.cpp | 380-391 |
| computed backgrounds, tint default 0.3 | src/kcolorscheme.cpp | 393-405 |
| contrast value 0.1 x contrast, default 7; 0.7 without config | src/kcolorscheme.cpp | 519-527 |
| frameContrast default 0.2, clamp 0..1 | src/kcolorscheme.cpp | 529-539 |
| shade-role formulas and thresholds 0.006, 0.93 | src/kcolorscheme.cpp | 566-616 |
| shade chroma amount default 0 | src/kcolorscheme.h | 351 |
| palette roles; Tooltip Active in all states; Accent = Selection background | src/kcolorscheme.cpp | 656-695 |
| app-chosen scheme path is an application property | src/kcolorschemehelpers_p.h (and src/kcolorschememanager.cpp 55-66) | 20-36 |

### o.5 Colour mathematics (frameworks/kguiaddons @ v6.31.0)

| Value or rule | File | Lines |
|---|---|---|
| clamp with NaN to 1 | src/colors/kguiaddons_colorhelpers_p.h | 13-16 |
| hue wrap | src/colors/kcolorspaces.cpp | 16-20 |
| Rec.709 weights 0.2126, 0.7152, 0.0722 | src/colors/kcolorspaces.cpp | 25-32 |
| gamma 2.2 and 1/2.2 | src/colors/kcolorspaces.cpp | 34-42 |
| forward HCY: luma, hue order, chroma | src/colors/kcolorspaces.cpp | 57-87 |
| inverse HCY: sectors, top/middle/bottom, assignment | src/colors/kcolorspaces.cpp | 89-148 |
| luma | src/colors/kcolorspaces.cpp | 160-163 |
| contrast ratio with +0.05 | src/colors/kcolorutils.cpp | 59-71 |
| lighten, darken, shade formulas | src/colors/kcolorutils.cpp | 73-95 |
| default amounts (lighten 0.5 / 1.0, darken 0.5 / 1.0, tint 0.3, mix 0.5) | src/colors/kcolorutils.h | 121, 140, 156, 172, 189 |
| tint: exponent 0.3, rg with cube, 12 bisection steps, last midpoint | src/colors/kcolorutils.cpp | 97-142 |
| mix: bounds, NaN, premultiplied, transparent result | src/colors/kcolorutils.cpp | 144-166 |
| interpolation form a + (b - a) t | src/colors/kcolorutils.cpp | 19-22 |

### o.6 Accent application (plasma/plasma-workspace @ v6.8.0)

| Value or rule | File | Lines |
|---|---|---|
| OKLab matrices (Ottosson), single-precision lerp | kcms/colors/colorsapplicator.cpp | 23-103 |
| sRGB to linear and back via the colour-space transform | kcms/colors/colorsapplicator.cpp | 105-116 |
| accent source, transparent means none, accent-present rule | kcms/colors/colorsapplicator.cpp | 125-137 |
| ColorSchemeHash | kcms/colors/colorsapplicator.cpp | 139-146 |
| TitlebarIsAccentColored / accentActiveTitlebar; TintFactor presence and value | kcms/colors/colorsapplicator.cpp | 150-155 |
| default tint factor 0.15 | kcms/colors/colorsapplicator.h | 86-88 |
| group order, key order, accent keys | kcms/colors/colorsapplicator.cpp | 157-181 |
| delete group, skip Header without Header | kcms/colors/colorsapplicator.cpp | 183-192 |
| link readability: ratio 5.0, one lighten or darken | kcms/colors/colorsapplicator.cpp | 197-218 |
| tint or copy of other keys | kcms/colors/colorsapplicator.cpp | 219-227 |
| Selection backgrounds and foregrounds | kcms/colors/colorsapplicator.cpp | 230-238 |
| Button BackgroundAlternate | kcms/colors/colorsapplicator.cpp | 240-245 |
| `[Inactive]` handling, Header copies Window | kcms/colors/colorsapplicator.cpp | 247-267 |
| Header title-bar accent | kcms/colors/colorsapplicator.cpp | 269-284 |
| `[WM]` keys and defaults from the Header set | kcms/colors/colorsapplicator.cpp | 287-309 |
| `[KDE]` frameContrast and contrast | kcms/colors/colorsapplicator.cpp | 311-318 |
| `[WM]` title-bar override | kcms/colors/colorsapplicator.cpp | 320-338 |
| effects copied, empty when absent | kcms/colors/colorsapplicator.cpp | 340-359 |
| 8-bit blend with truncation, translucent branch | kcms/colors/colorsapplicator.h | 18-43 |
| accent background alpha 0.7 | kcms/colors/colorsapplicator.h | 45-50 |
| black-or-white threshold 4.5, inactive alpha 0.6 | kcms/colors/colorsapplicator.h | 52-72 |
| accent option: current scheme, no AccentColor write, no "already set" check | kcms/colors/plasma-apply-colorscheme.cpp | 131-155 |
| scheme-name path refuses the scheme in use | kcms/colors/plasma-apply-colorscheme.cpp | 64-80 |
| ten swatches and their order | kcms/colors/ui/AccentColorUI.qml | 119-130 |
| fallback custom colour is the first swatch | kcms/colors/ui/AccentColorUI.qml | 148 |

### o.7 Platform and portal

| Value or rule | Repository @ tag | File | Lines |
|---|---|---|---|
| dark when gray of Window < 192, values 1 / 2 | plasma/xdg-desktop-portal-kde @ v6.8.0 | src/settings.cpp | 259-274 |
| accent-color is Highlight as doubles | plasma/xdg-desktop-portal-kde @ v6.8.0 | src/settings.cpp | 276-284 |
| reduced-motion is factor == 0 | plasma/xdg-desktop-portal-kde @ v6.8.0 | src/settings.cpp | 286-292 |
| namespaces and errors | plasma/xdg-desktop-portal-kde @ v6.8.0 | src/settings.cpp | 29-49, 87, 151, 228, 378-392, 502-545 |
| change signals | plasma/xdg-desktop-portal-kde @ v6.8.0 | src/settings.cpp | 294-307, 412-468 |
| palette from kdeglobals iff Colors:View; LnF colors, ColorScheme, BreezeLight fallbacks | plasma/plasma-integration @ v6.8.0 | qt6/src/platformtheme/khintssettings.cpp | 427-473 |
| same gray < 192 rule in the platform theme | plasma/plasma-integration @ v6.8.0 | qt6/src/platformtheme/khintssettings.cpp | 475-490 |
| reduced motion fuzzy zero | plasma/plasma-integration @ v6.8.0 | qt6/src/platformtheme/khintssettings.cpp | 492-497 |
| widgetStyle lookup and default breeze | plasma/plasma-integration @ v6.8.0 | qt6/src/platformtheme/khintssettings.cpp | 97-113, 304-322 |
| [KDE] hint keys and defaults | plasma/plasma-integration @ v6.8.0 | qt6/src/platformtheme/khintssettings.cpp | 72-92, 383-403 |
| font keys and defaults; Qt < 6.11 truncation to 17 fields; portal first in sandbox | plasma/plasma-integration @ v6.8.0 | qt6/src/platformtheme/kfontsettingsdata.cpp | 43-97, 144-168 |

### o.8 Breeze widget style (plasma/breeze @ v6.8.0)

| Value or rule | File | Lines |
|---|---|---|
| every metric constant of k.1 | kstyle/breezemetrics.h | 14-179 |
| pen width 1.001 | kstyle/breezemetrics.h | 22-28 |
| breezerc keys and defaults | kstyle/breeze.kcfg | whole file |
| size expansion by margins, rectangle inset and centring helpers | kstyle/breezestyle.h | 397-447 |
| scroll-bar button heights | kstyle/breezestyle.h | 366-376 |
| pixel metrics (menu margins 4, frame widths, layout margins, spacing, indicator sizes) | kstyle/breezestyle.cpp | 645-883 |
| style hints (submenu delay 150 and others) | kstyle/breezestyle.cpp | 885-983 |
| check box size | kstyle/breezestyle.cpp | 3304-3334 |
| line edit, combo, spin sizes | kstyle/breezestyle.cpp | 3337-3405 |
| slider tick length and size | kstyle/breezestyle.cpp | 3408-3489 |
| push-button size, minimum width, frame margin | kstyle/breezestyle.cpp | 3491-3565 |
| tool-button size | kstyle/breezestyle.cpp | 3568-3593 |
| menu-bar item size | kstyle/breezestyle.cpp | 3596-3599 |
| menu item, separator and section sizes | kstyle/breezestyle.cpp | 3602-3711 |
| progress-bar size | kstyle/breezestyle.cpp | 3714-3743 |
| tab size, static tab 34 | kstyle/breezestyle.cpp | 3801-3853 |
| header size | kstyle/breezestyle.cpp | 3856-3899 |
| item-view row size | kstyle/breezestyle.cpp | 3902-3916 |
| frames and line edits, neutral and focus outlines | kstyle/breezestyle.cpp | 3919-4085 |
| focus rectangle in views, Highlight lightened 110 | kstyle/breezestyle.cpp | 4087-4167 |
| menu frame for toolbars | kstyle/breezestyle.cpp | 4169-4192 |
| push and tool button frames, states | kstyle/breezestyle.cpp | 4511-4619 |
| menu panel, MenuOpacity, outline alpha 0.25 | kstyle/breezestyle.cpp | 4695-4727 |
| tooltip panel | kstyle/breezestyle.cpp | 4729-4744 |
| item-view selection, alpha 0.3, lighter 110, outline 1.0 / 0.8 | kstyle/breezestyle.cpp | 4746-4897 |
| check box and radio drawing | kstyle/breezestyle.cpp | 4900-4985 |
| menu item drawing, strong focus, separator, section, shortcut 0.7 | kstyle/breezestyle.cpp | 5951-6188 |
| progress bar, busy colours, groove WindowText 0.2 | kstyle/breezestyle.cpp | 6191-6306 |
| scroll-bar handle, alpha 0.7 + 0.3 | kstyle/breezestyle.cpp | 6339-6407 |
| focus frame annulus, radii 5 and 7, Highlight 0.3 | kstyle/breezestyle.cpp | 6614-6781 |
| header section drawing | kstyle/breezestyle.cpp | 6803-6894 |
| tab shapes | kstyle/breezestyle.cpp | 7175-7378 |
| combo box drawing | kstyle/breezestyle.cpp | 7813-7931 |
| spin box drawing and arrows | kstyle/breezestyle.cpp | 7933-7969, 8363-8409 |
| slider drawing | kstyle/breezestyle.cpp | 7971-8137 |
| scroll-bar separator, View Text at alpha fc | kstyle/breezestyle.cpp | 8213-8233 |
| scroll-bar, combo and spin sub-control rectangles | kstyle/breezestyle.cpp | 2970-3212 |
| slider sub-control rectangles | kstyle/breezestyle.cpp | 3253-3301 |
| progress groove and contents rectangles | kstyle/breezestyle.cpp | 2255-2339 |
| AnimationDurationFactor x 100 for QStyle animations | kstyle/breezestyle.cpp | 2070-2087 |
| scroll-bar button mapping 0 / 1 / other | kstyle/breezestyle.cpp | 2148-2213 |
| arrow shade 0.15, highlight alpha 0.3, sunken darkening 110 | kstyle/breezehelper.cpp | 38-42 |
| frame outline, focus and hover outlines, frame background, arrow colours | kstyle/breezehelper.cpp | 142-246 |
| slider outline, scroll handle colour, check-box mark colour, separator | kstyle/breezehelper.cpp | 248-332 |
| alpha multiply rule | kstyle/breezehelper.cpp | 349-356 |
| focus rectangle (stroked, radius 4.5, sides) | kstyle/breezehelper.cpp | 369-421 |
| frame rendering, menu frame (radius 5 with compositing) | kstyle/breezehelper.cpp | 475-611 |
| button frame: fills, pens, default button, shadow | kstyle/breezehelper.cpp | 647-740 |
| separator line | kstyle/breezehelper.cpp | 814-829 |
| check-box background and mark geometry | kstyle/breezehelper.cpp | 831-980 |
| radio background and dot | kstyle/breezehelper.cpp | 983-1078 |
| slider groove and handle | kstyle/breezehelper.cpp | 1081-1200 |
| progress groove and busy stripes | kstyle/breezehelper.cpp | 1204-1271 |
| scroll-bar handle radius, pen alpha fc, fill alpha 0.5 | kstyle/breezehelper.cpp | 1274-1296 |
| static tab and tab shapes, darker 120, hover alpha 0.2, 3 px bar | kstyle/breezehelper.cpp | 1299-1508 |
| arrow geometry | kstyle/breezehelper.cpp | 1511-1578 |
| button and ellipse shadows | kstyle/breezehelper.cpp | 1646-1690 |
| view item position sides | kstyle/breezehelper.cpp | 1692-1737 |
| stroked rectangle 0.5 inset | kstyle/breezehelper.cpp | 1747-1757 |
| item-view margins, first row, framed views, row selection | kstyle/breezehelper.cpp | 1759-1799 |
| frame radius 5 - pen / 2 with integer pen; shadow colour 0.125; frame bias = frameContrast | kstyle/breezehelper.h | 64-108, 340-363 |
| window shadow parameter table, fallback Large, strength / 255 | kstyle/breezeshadowhelper.cpp | 30-70, 175-237 |
| CSS blur semantics, box-blur extent formula | libbreezecommon/breezeboxshadowrenderer.cpp | 27-44, 57-92, 241-284 |
| animation steps quantisation | kstyle/animations/breezeanimationdata.h | 73-80 |
| engines use AnimationsDuration; busy uses ProgressBarBusyStepDuration | kstyle/animations/breezeanimations.cpp | 48-82 |
| busy offset 0 to 28, looping | kstyle/animations/breezebusyindicatorengine.cpp | 85-104 |
| stock schemes | colors/BreezeLight.colors, colors/BreezeDark.colors, colors/BreezeClassic.colors | whole files |

### o.9 QtQuick style and Kirigami

| Value or rule | Repository @ tag | File | Lines |
|---|---|---|---|
| switch indicator size, track inset, radii, colours, motion | frameworks/qqc2-desktop-style @ v6.31.0 | org.kde.desktop/private/SwitchIndicator.qml | 12-94 |
| switch padding 1, spacing smallSpacing, label opacity 0.6 | frameworks/qqc2-desktop-style @ v6.31.0 | org.kde.desktop/Switch.qml | 25-26, 64-83 |
| slider handle size from the style | frameworks/qqc2-desktop-style @ v6.31.0 | org.kde.desktop/private/DefaultSliderHandle.qml | 16-48 |
| desktop durations from longDuration x factor, halving | frameworks/qqc2-desktop-style @ v6.31.0 | kirigami-plasmadesktop-integration/plasmadesktopunits.cpp | 18-55 |
| factor = max(0, AnimationDurationFactor), default 1 | frameworks/qqc2-desktop-style @ v6.31.0 | kirigami-plasmadesktop-integration/animationspeedprovider.cpp | 34-48 |
| gridUnit 18, spacings 4 / 6 / 8, durations, tooltip 700, corner 5 | frameworks/kirigami @ v6.31.0 | src/platform/units.cpp | 33-49 |
| icon sizes 16 / 22 / 32 / 48 | frameworks/kirigami @ v6.31.0 | src/platform/units.cpp | 297-345 |
| heading factors 1.35 / 1.20 / 1.15 / 1.10, DemiBold, 0.75 | frameworks/kirigami @ v6.31.0 | src/controls/Heading.qml | 14-37 |

### o.10 Fonts (font files)

| Value | Source | Location |
|---|---|---|
| Noto Sans upem 1000, hhea 1069 / -293 / 0, typo-metrics flag, Windows 1124 / 395 | NotoSans-v2.015, `NotoSans[wdth,wght].ttf` (full and unhinted) | head, hhea, OS/2 tables |
| Hack upem 2048, hhea 1901 / -483 / 0, OS/2 typo 1556 / -492 / 410, advance 1233 | Hack v3.003, `Hack-Regular.ttf`, `Hack-Bold.ttf` | head, hhea, OS/2, hmtx tables |

Font archive hashes: `Hack-v3.003-ttf.zip`
0c2604631b1f055041c68a0e09ae4801acab6c5072ba2db6a822f53c3f8290ac;
`NotoSans-v2.015.zip`
0c34df072a3fa7efbb7cbf34950e1f971a4447cffe365d3a359e2d4089b958f5;
`Hack-Regular.ttf`
15f55cc0c85a2988d2b4b3a8cdb5d77fdfbaf319e1bb5309d725db9818fb7125;
`NotoSans[wdth,wght].ttf` (full)
ae80a5e79e4afbf493e9f4684ec4988e6a861b3598102ee48950b4b7a754cdff.

### o.11 Fixture hashes (vendored, LGPL-2.0-or-later)

| File (breeze @ v6.8.0) | sha256 |
|---|---|
| colors/BreezeLight.colors | c46470b75e7df3e1ebff344748b92347f8d5adfa306aef9c91c4d2edae2cd6df |
| colors/BreezeDark.colors | 11fce1d8ab97b529162e50eb200d1a93fab65c7612f9bebd0230a6a348d0cc21 |
| colors/BreezeClassic.colors | 19dcbd1883c03d2d9e0176a39e3ec9df6c18d6e22f0474e6c886aa20aff76508 |

### o.12 Qt behaviour relied on

| Behaviour | Reference | Status |
|---|---|---|
| 16-bit channel storage, single-precision float accessors, Q16 on store | Qt 6 docs, the colour class (channel accessors, float API); qtbase src/gui/painting/qcolor.cpp 1315-1338 | [measured-qt]: every `u16` of `math.json` and `resolve.json` reproduced |
| 8-bit accessors round `q / 257` (a.3) | qtbase src/gui/painting/qdrawhelper_p.h 886-887, qcolor.cpp 1526-1530 | [measured-qt]: all 65536 values, and every `hex` field |
| hex, SVG-name and `transparent` colour parsing; 9- and 12-digit forms | Qt 6 docs, the colour class, parsing a colour from a string | [qt]; widths [measured-qt] (`math.json` `qt_color_text`) |
| lighter / darker via HSV value, factor rules, HSV conversions (d.11) | Qt 6 docs, the colour class, lighter and darker; qtbase src/gui/painting/qcolor.cpp 2040-2110, 2181-2225, 2805-2862 | [measured-qt] (`math.json` `qt_lighter_darker`, 1736 records exact) |
| gray value (11 r + 16 g + 5 b) / 32 | Qt docs, qGray | [qt] |
| font string field layout and weight scales | Qt docs, QFont::toString, QFont::fromString, QFont::Weight | [qt] |
| legacy weight nearest-neighbour mapping, ties to the lighter weight | Qt docs, QFont::Weight (Qt 5 and Qt 6 values) | [qt]; all 100 legacy values [measured-qt] (`sizes.json` `legacy_font_weights`) |
| font line height: integer pixel size, ascent and descent rounded separately (l.2) | `sizes.json` font records | [measured-qt] |
| sRGB curve constants, inverse curve, matrix M, float storage (d.15) | Qt 6 docs, the colour-space class (sRGB and linear sRGB) and its transform; qtbase src/gui/painting/qcolortransferfunction_p.h 54-112, qcolorspace.cpp 40-45, 89-106, 353-365, 483-493, qcolormatrix_p.h 32-76, 232-270, 288-312, qcolortransform.cpp 241-272, 1781-1812, qcolortrc_p.h 67-108 | [measured-qt] (`math.json` `qt_colorspace`, 1146 results exact) |
| out-of-range float colours held in half precision, clamped on conversion | Qt 6 docs, the colour class, extended RGB specification; qcolor.cpp 1315-1332 | [measured-qt] (`math.json` `qt_extended`) |
| smoothed animation curve, default velocity 200, shorter of velocity and duration | Qt docs, SmoothedAnimation | [qt] |
| default animation easing linear | Qt docs, QVariantAnimation | [qt] |
| a translucent fill over an opaque one is composited in 8-bit sRGB, each channel rounded; a compositor that blends in linear light gives another shade (k.5.6) | Qt 6 docs, the painter class, composition modes (source over) | [measured-qt] (`pixels.json` `check_box.on`, `radio_button.on`: #c2e4f6 and #2f5368) |
| a fill painted with the pen still set strokes the outline again (k.5.6, k.5.7) | Qt 6 docs, the painter class (drawing a shape fills it with the brush and strokes it with the pen) | [measured-qt] (`pixels.json` `check_box.on`, `radio_button.on` corner pixels) |
| the default pen cap is square (k.5.6 check mark) | Qt 6 docs, the pen class, default cap style | [qt]; [measured-qt] (`check_box.on`, 0.83 before the first point) |
| an item-view selection is stroked and filled at radius 5, not 4.5 (k.5.16) | Qt 6 docs, the painter class (rounded rectangles); Plasma's painted rows | [measured-qt] (`pixels.json` `item_view_row.selected`, `.hover`) |
| tooltip label margin = 1 + frame width | Qt widgets tooltip behaviour | [measured-qt] (`sizes.json` `tooltip`: margin 4, hint height 26 for an 18 px line) |
| line edit inner height max(fm height, 14) + 2 | Qt widgets line edit size hint | [measured-qt] (`sizes.json`: framed 32, frameless 20 at 10 pt) |
| progress bar hint height fm height + 8 | Qt widgets progress bar size hint | [measured-qt] (`sizes.json`: 26 at 10 pt, 23 at 8 pt) |
| XDG_*_DIRS defaults, relative entries ignored | Qt docs, QStandardPaths; XDG Base Directory specification | [qt] |
