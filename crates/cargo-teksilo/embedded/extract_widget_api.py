#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""
Extract public API and inline documentation from a Teksilo crate's source files.

Every crate under `crates/` with a meaningful public Rust API is queryable
through this tool, selected with `--crate` (default `widgets`) and listed in
`CRATE_SPECS`. Each entry carries a `catalog: bool` that splits the table into
two tiers:

  * `catalog=True` — the crate ALSO gets a generated mdBook catalog (one
    Markdown page per public type, plus an index), written by `--md-dir` /
    `--catalog-all` under `docs/<md_subdir>/`. Exactly four crates are
    cataloged:

        widgets   teksilo-widgets   -> docs/widgets/           "Widget Catalog"
        data      teksilo-data      -> docs/data-collections/  "Data Collections"
        settings  teksilo-settings  -> docs/settings/          "Settings"
        scene     teksilo-scene     -> docs/scene/             "Scene"

    `--catalog-all` iterates ONLY these four, so `docs/` output stays
    byte-identical no matter how many more crates join the table below;
    `--md-dir` against a `catalog=False` crate refuses outright rather than
    silently writing pages nobody wired into the book.

  * `catalog=False` — every other entry (the framework core, tokens, canvas,
    the sibling theme presets, i18n, telemetry, the analytics adapters, the
    teksu! tooling, …). Fully queryable — `--crate <key>`, `--list`, `--all`,
    and a plain `<Name>` lookup all behave exactly like a cataloged crate —
    it just has no mdBook pages yet. Flipping the flag is the only step a
    later change needs to give one of these its own catalog.

Only `widgets` gets the widget-specific behaviour (the impl-Widget entry filter
and the `widgets-overview.md` categories); every other crate surfaces its
re-exported public types and groups by directory.

A hand-maintained `UMBRELLA_REEXPORTS` table additionally lets a bare `<Name>`
lookup that isn't in the active `--crate` fall through to the crate that
actually owns it, when that name is reachable through `teksilo::prelude::*`
(e.g. `Theme` lives in teksilo-core, not the default `widgets` crate, but
`teksilo::prelude::Theme` is how an app actually sees it).

Walks the selected crate's `src/` recursively (top-level files plus submodule
directories like `notification/`, `tab_widget/`, `primitives/`, `animations/`),
looks up the file for each requested name, and emits:

  - The file's `//!` module header doc
  - Every `pub struct` / `pub enum` / `pub type` / `pub const` with its `///` doc
  - Every `pub fn` inside inherent `impl Foo { ... }` blocks with its `///` doc
  - Enum variants with their own `///` docs

Trait impls like `impl Widget for Foo` are skipped — they are internal plumbing,
not part of the widget's builder API.

`--list` shows only the widget(s) per file, not every exported type. A genuine
widget is a pub type that both `impl Widget` and is re-exported from the crate
root (`lib.rs`). Config enums (e.g. `IconLocation`) and internal helpers (e.g.
`HeaderCell`) are therefore filtered out of the listing — but every type remains
addressable by name (`extract_widget_api.py IconLocation` still works). The
flat, one-widget-per-file dirs (top-level, `primitives/`, `animations/`) keep a
lenient fallback so no conventional module is ever hidden.

Usage:
    python tools/extract_widget_api.py Button HStack
    python tools/extract_widget_api.py --all
    python tools/extract_widget_api.py --list
    python tools/extract_widget_api.py Button --format json
    python tools/extract_widget_api.py Button -o out.md
    python tools/extract_widget_api.py --crate scene --list
    python tools/extract_widget_api.py --crate core Theme      # queryable, not cataloged
    python tools/extract_widget_api.py --md-dir docs/widgets   # one crate
    python tools/extract_widget_api.py --catalog-all --api-dir target/doc

`--md-dir DIR` regenerates the active crate's mdBook catalog: one Markdown page
per type (deep-linking to its rustdoc module page), a grouped `index.md`, and an
in-place patch of that crate's `<!-- BEGIN/END GENERATED <DIR> -->` region of
`docs/SUMMARY.md`. Refused when the active crate has `catalog=False`.
`--catalog-all` does the same for all four `catalog=True` crates, each into its
own default directory, and ignores every `catalog=False` entry. This is what
the docs workflow runs.

The generated pages ARE committed, unlike `book/` — see the note in
`.gitignore` — so the book builds with its sidebar and pictures on any
checkout. Regenerate and commit them when a documented item changes.

Two things depend on artifacts this script does not produce:

  * `--api-dir DIR` (the built rustdoc tree, e.g. `target/doc`) makes each
    page's API link fall back to the nearest module that actually has a page,
    so a private or cfg-gated module does not 404. Run `cargo doc` first, or
    the deep links are emitted unverified.
  * A page opens with `![... preview](img/<slug>.png)` only if that file
    exists. The images come from
    `cargo run -p teksilo-widgets-previewer -- --export-docs`, which needs a
    GPU adapter and so is not part of CI; a missing image degrades to a page
    without a picture. A denser pass of the same command
    (`--density=touch`) writes `img/<slug>-touch.png` beside it, and the
    page then also carries a "Density" section showing it. The suffix table
    is `DENSITY_IMAGE_SUFFIXES` below, mirroring
    `teksilo_preview::PreviewPass::image_suffix` on the Rust side.

Each page's API deep link is emitted as the crate's **docs.rs** URL, because
the pages are committed and have to resolve for someone reading the Markdown on
GitHub, where no rustdoc tree exists. `tools/fix_book_links.py` rewrites those
back to the book-relative `../api/...` at build time, so the rendered site links
into its own rustdoc tree instead of leaving for docs.rs. `--api-base ../api`
skips the round trip and emits the book form directly.

The site's `/api/` tree is assembled by the workflow with
`cp -r target/doc book/api`. A bare local `mdbook build` does not do that, so
copy it yourself if you want those links live in a local preview.
"""

from __future__ import annotations

import argparse
import difflib
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent


@dataclass(frozen=True)
class CrateSpec:
    """A crate this tool can extract public API from.

    `is_widget` selects the widget-specific behaviour (impl-Widget entry filter +
    widgets-overview.md categories); other crates surface their re-exported public
    types and group by directory.

    `catalog` gates mdBook catalog generation ONLY (`--md-dir` / `--catalog-all`).
    It has no bearing on `--crate` / `--list` / `--all` / a positional `<Name>`
    lookup, which work identically for every entry regardless of this flag —
    see the module docstring's two-tier explanation.
    """

    crate: str  # cargo package name, e.g. "teksilo-widgets"
    rustdoc: str  # rustdoc crate dir, e.g. "teksilo_widgets"
    md_subdir: str  # output dir under docs/, e.g. "widgets"
    title: str  # SUMMARY part / index title, e.g. "Widget Catalog"
    group: str  # top-level group label in the index for non-widget crates
    is_widget: bool
    catalog: bool  # True: also gets a committed mdBook catalog under docs/<md_subdir>/

    @property
    def src(self) -> Path:
        return REPO_ROOT / "crates" / self.crate / "src"

    @property
    def docs_rs(self) -> str:
        """Base for this crate's published rustdoc.

        The catalog pages are committed, so their API deep link has to resolve
        for someone reading the Markdown on GitHub, where no rustdoc tree
        exists. `fix_book_links.py` points it back at the book's own `/api/`
        tree at build time, so the rendered site links locally.
        """
        return f"https://docs.rs/{self.crate}/latest"

    @property
    def marker_begin(self) -> str:
        return f"<!-- BEGIN GENERATED {self.md_subdir.upper()} -->"

    @property
    def marker_end(self) -> str:
        return f"<!-- END GENERATED {self.md_subdir.upper()} -->"


# Crates under crates/ deliberately left OUT of CRATE_SPECS below. This tool
# parses inherent impls and top-level pub items out of a library's src/ tree,
# so a crate whose public surface is a binary or a proc-macro yields nothing
# useful. Each exclusion here was verified against the crate's own Cargo.toml
# (`[[bin]]` / `proc-macro = true` / `publish = false`), not guessed from its
# name.
#
# Binaries — `[[bin]]` in Cargo.toml and no src/lib.rs, so there is no library
# API to walk:
#   cargo-teksilo, cargo-teksilo-fmt, cargo-teksilo-telemetry-lint,
#   teksilo-automation-mcp, teksilo-fmt-lsp, teksilo-widgets-previewer
#
# Proc-macro crates — `[lib]` / `proc-macro = true` in Cargo.toml. Their public
# surface is macro-invocation syntax (documented in prose elsewhere), not
# struct/enum/fn declarations this tool's regexes target:
#   teksilo-i18n-macros, teksilo-macros, teksilo-telemetry-codegen
#   teksilo-resources — despite its name suggesting a plain library, its
#   Cargo.toml declares `[lib]\nproc-macro = true` (it is the `res!` macro
#   crate); verified, not assumed from the name.
#
# Internal build-time gates — `publish = false`, and each Cargo.toml says
# outright it exists only to fail this workspace's own build (drift guards /
# fixture lists), so there is no public surface meant for a downstream reader:
#   teksilo-teksu-guard, teksilo-target-conformance
#
# Architectural limitation, verified per-crate — `SKIP_FILES` (below) treats
# every crate's src/lib.rs as a pure aggregator (mod declarations + `pub use`
# re-exports) and never scans it for locally-defined pub items; that rule is
# applied uniformly, including to the four cataloged crates. teksilo-tokio and
# teksilo-async-std define their ENTIRE public surface (`TokioHandle`,
# `TeksiloAppBuilderTokioExt`, `TeksiloAppBuilderAsyncStdExt`) directly inside
# lib.rs, and neither has any other .rs file in src/ — so for these two
# specifically, nothing is left to discover:
#   teksilo-async-std, teksilo-tokio
#
# Explicitly out of scope for this step — another agent is creating it
# concurrently and it is pure data, not a documentable API surface:
#   teksilo-corpus
CRATE_SPECS: dict[str, CrateSpec] = {
    # --- Cataloged (catalog=True): also get committed mdBook pages under
    #     docs/<md_subdir>/ via --md-dir / --catalog-all. Unchanged from
    #     before this table grew — --catalog-all only ever visits these four.
    "widgets": CrateSpec("teksilo-widgets", "teksilo_widgets", "widgets", "Widget Catalog", "Widgets", True, True),
    "data": CrateSpec("teksilo-data", "teksilo_data", "data-collections", "Data Collections", "Models", False, True),
    "settings": CrateSpec("teksilo-settings", "teksilo_settings", "settings", "Settings", "Stores & services", False, True),
    "scene": CrateSpec("teksilo-scene", "teksilo_scene", "scene", "Scene", "Scene", False, True),

    # --- Queryable only (catalog=False): --crate / --list / --all / <Name> all
    #     work; --md-dir refuses (see main()). title/md_subdir/group below are
    #     placeholders — correct if a later step flips catalog=True, unused
    #     until then.

    # Framework core & foundational layers
    "core": CrateSpec("teksilo-core", "teksilo_core", "core", "Core Framework", "Framework", False, False),
    "tokens": CrateSpec("teksilo-tokens", "teksilo_tokens", "tokens", "Design Tokens", "Tokens", False, False),
    "canvas": CrateSpec("teksilo-canvas", "teksilo_canvas", "canvas", "Canvas API", "Canvas", False, False),
    "render": CrateSpec("teksilo-render", "teksilo_render", "render", "Renderer", "Rendering", False, False),
    "platform": CrateSpec("teksilo-platform", "teksilo_platform", "platform", "Platform Integration", "Platform", False, False),
    "app": CrateSpec("teksilo-app", "teksilo_app", "app", "App Runtime", "App", False, False),
    "text": CrateSpec("teksilo-text", "teksilo_text", "text", "Text Backend", "Text", False, False),

    # Feature crates built on the core
    "charts": CrateSpec("teksilo-charts", "teksilo_charts", "charts", "Charts", "Charts", False, False),
    "i18n": CrateSpec("teksilo-i18n", "teksilo_i18n", "i18n", "Internationalization", "I18n", False, False),
    "telemetry": CrateSpec("teksilo-telemetry", "teksilo_telemetry", "telemetry", "Telemetry", "Telemetry", False, False),
    "automation": CrateSpec("teksilo-automation", "teksilo_automation", "automation", "Automation Toolkit", "Automation", False, False),
    "webview": CrateSpec("teksilo-webview", "teksilo_webview", "webview", "WebView", "WebView", False, False),
    "terminal": CrateSpec("teksilo-terminal", "teksilo_terminal", "terminal", "Terminal", "Terminal", False, False),
    "inspector": CrateSpec("teksilo-inspector", "teksilo_inspector", "inspector", "Inspector", "Tooling", False, False),
    "async": CrateSpec("teksilo-async", "teksilo_async", "async", "Async Executor", "Async", False, False),

    # Analytics adapters (teksilo-telemetry consumers)
    "analytics-native": CrateSpec("teksilo-analytics-native", "teksilo_analytics_native", "analytics-native", "Analytics: Native", "Analytics", False, False),
    "analytics-otlp": CrateSpec("teksilo-analytics-otlp", "teksilo_analytics_otlp", "analytics-otlp", "Analytics: OTLP", "Analytics", False, False),
    "analytics-plausible": CrateSpec("teksilo-analytics-plausible", "teksilo_analytics_plausible", "analytics-plausible", "Analytics: Plausible", "Analytics", False, False),

    # Sibling design-language presets (tokens + Tier-3 chrome)
    "theme-fluent": CrateSpec("teksilo-theme-fluent", "teksilo_theme_fluent", "theme-fluent", "Fluent Theme", "Themes", False, False),
    "theme-macos": CrateSpec("teksilo-theme-macos", "teksilo_theme_macos", "theme-macos", "macOS Theme", "Themes", False, False),
    "theme-material3": CrateSpec("teksilo-theme-material3", "teksilo_theme_material3", "theme-material3", "Material 3 Theme", "Themes", False, False),

    # Storybook-equivalent previewer: trait/registry crate + reusable GUI library
    "preview": CrateSpec("teksilo-preview", "teksilo_preview", "preview", "Widget Previewer Core", "Preview", False, False),
    "preview-ui": CrateSpec("teksilo-preview-ui", "teksilo_preview_ui", "preview-ui", "Previewer UI", "Preview", False, False),

    # teksu! DSL tooling shared by the proc macro and the formatter
    "fmt": CrateSpec("teksilo-fmt", "teksilo_fmt", "fmt", "teksu! Formatter", "Tooling", False, False),
    "parse": CrateSpec("teksilo-parse", "teksilo_parse", "parse", "teksu! Parser", "Tooling", False, False),

    # The umbrella crate's OWN files (its toast/webview install-hook extension
    # traits — automation_install.rs only re-exports teksilo-app's, so it does
    # not add anything new here) — distinct from the crates it merely
    # re-exports, each of which already has its own entry above.
    "teksilo": CrateSpec("teksilo", "teksilo", "umbrella", "Umbrella Crate", "Umbrella", False, False),

    # --- The two external siblings.
    #
    # Not teksilo crates and not in this workspace: `text-document` and
    # `text-typeset` are ordinary crates.io dependencies, maintained alongside
    # Teksilo and released on their own cadence. They are here because the
    # question this tool answers is "what can the app I am writing reach?", and
    # the answer includes them: `teksilo-text` re-exports `text_document`
    # wholesale (so a consumer writes `teksilo::text_document::TextDocument`)
    # plus ~20 `text_typeset` types by name (`CursorAffinity`, `HitTestResult`,
    # `RenderFrame`, `FontFaceId`, …). Before these entries, `symbol
    # TextDocument` answered "unknown type … Did you mean textwidget?" about a
    # type that is public, documented and load-bearing for every text surface
    # in the framework — the confidently-wrong answer this tool exists to stop.
    #
    # `src` resolves to `crates/<name>/src`, which does NOT exist in a
    # framework checkout — these two live outside it. That is not a problem to
    # fix but the normal case: `cargo teksilo symbol` stages the consumer's
    # *resolved* sources into exactly that layout, so the entries are live
    # where the question gets asked and inert (via `MissingCrateSource`) where
    # it does not. `--list` / `--all` / the corpus build skip them in-repo for
    # the same reason.
    "text-document": CrateSpec("text-document", "text_document", "text-document", "Rich Text Document", "Text", False, False),
    "text-typeset": CrateSpec("text-typeset", "text_typeset", "text-typeset", "Typesetter", "Text", False, False),
}

# The active crate, set by `main()` from `--crate` (default: widgets).
SPEC: CrateSpec = CRATE_SPECS["widgets"]

# Back-compat aliases for the widget crate (used by widget-only helpers).
WIDGETS_SRC = CRATE_SPECS["widgets"].src
PRIMITIVES_DIR = WIDGETS_SRC / "primitives"
ANIMATIONS_DIR = WIDGETS_SRC / "animations"

# Aggregator files we never treat as a catalog entry.
SKIP_FILES = {"lib.rs", "primitives.rs", "animations.rs", "layout_integration_tests.rs", "mod.rs"}

# `lib.rs` is in SKIP_FILES because it is normally a pure aggregator — `mod`
# declarations and `pub use` re-exports — and scanning it would list every
# re-exported name a second time under the wrong file.
#
# For 9 of 39 crates that is simply false: they define public types directly in
# `lib.rs`. teksilo-webview's `WebView` — the widget the whole crate exists for
# — is one, and so are teksilo-fmt's `FmtConfig`/`FmtError`, both analytics
# adapters, and teksilo-inspector's install extension. Skipped unconditionally,
# those 14 types answered `unknown type`, which reads to an agent exactly like
# "does not exist": the failure this tool is built against.
#
# So the rule checks itself rather than being asserted. A `lib.rs` that defines
# nothing is still skipped, and the hijack the blanket skip was guarding
# against cannot happen anyway: `build_registry`'s `type_re` matches `pub
# struct|enum|type|trait` DECLARATIONS only, never `pub use`, so a scanned
# `lib.rs` claims a name only when it is the file that declares it.
LIB_RS_DEFINES_TYPES_RE = re.compile(
    r"^\s*pub\s+(?:struct|enum|trait|union)\s+[A-Z]", re.MULTILINE
)


def _lib_rs_is_pure_aggregator(path: Path) -> bool:
    """True when `path` (a crate's `lib.rs`) declares no public types of its
    own, and so is the aggregator `SKIP_FILES` assumes it to be."""
    try:
        return LIB_RS_DEFINES_TYPES_RE.search(path.read_text(errors="ignore")) is None
    except OSError:
        return True


def _is_scannable(p: Path) -> bool:
    """Whether `build_registry` should read `p` for type declarations."""
    if _is_test_file(p):
        return False
    if p.name == "lib.rs":
        return not _lib_rs_is_pure_aggregator(p)
    return p.name not in SKIP_FILES


# ----------------------------------------------------------------------------
# Umbrella re-export map
# ----------------------------------------------------------------------------
#
# `_parse_public_exports()` (below) only reads the ACTIVE crate's own lib.rs
# `pub use ...;` statements — no glob resolution, no transitive chains. So a
# name an app actually sees via `use teksilo::prelude::*;` (e.g. `Button`,
# `Theme`, `ListModel`) does not resolve as such: `Button` only works today
# because the default `--crate` happens to be `widgets`, and `Theme` (owned by
# teksilo-core) fails outright against that default with "unknown widget".
#
# This table is the fix: name -> the CRATE_SPECS key that actually owns it.
# When a positional lookup misses in the active crate, main() consults this
# map and retries against the owning crate's own registry (see
# `_resolve_across_crates`). It is HAND-MAINTAINED, built by reading
# `crates/teksilo/src/lib.rs`'s `pub mod prelude { ... }` block name by name —
# not derived programmatically, because that would require the very glob/
# transitive-`pub use` resolution `_parse_public_exports()` deliberately
# doesn't do.
#
# What breaks if this goes stale: nothing crashes. A name added to (or moved
# within) `teksilo::prelude` later, and not mirrored here, simply isn't
# redirected — a plain `<Name>` lookup against an unrelated default crate
# reports "unknown widget" for a name that is in fact real and reachable
# through the umbrella, same as before this table existed. `run_self_tests()`
# only guards the OTHER direction: every value here must still be a live
# CRATE_SPECS key, so a renamed/removed table row is caught immediately rather
# than failing silently later.
#
# Pointing at the right CRATE_SPECS key is necessary but not always
# sufficient: a small number of prelude names are re-exports of a *different*
# crate than the one that ships them to the umbrella (`ToastPriority` and
# `WebViewStyle` are Tier-3 style types actually declared in teksilo-core, not
# in teksilo-widgets / teksilo-webview which merely forward them — see those
# entries below), and `WebView` itself is declared directly in
# teksilo-webview's own lib.rs, which this tool never scans for content (the
# same SKIP_FILES rule documented above CRATE_SPECS) — so it maps to the
# right crate but the underlying lookup still fails there, exactly as it
# would for any other type defined straight in a crate's lib.rs.
#
# Two names from `teksilo::prelude` are intentionally left OUT: `TokioHandle`
# and `TeksiloAppBuilderTokioExt` (owned by teksilo-tokio) and
# `TeksiloAppBuilderAsyncStdExt` (owned by teksilo-async-std) — both source
# crates are excluded from CRATE_SPECS (see the comment above the table: their
# entire public surface lives directly in lib.rs, which this tool never scans
# for content), so there is no crate key to redirect to.
UMBRELLA_REEXPORTS: dict[str, str] = {
    # teksilo-core — widget trait, events, layout, actions/intents/shortcuts,
    # theming types, multi-window API (teksilo::prelude's two `teksilo_core::{…}`
    # blocks + the standalone dim_when_inactive / color_prop / window re-exports).
    "AccessNodeBuilder": "core",
    "AccessSubtreeMode": "core",
    "AccessibilityOverrides": "core",
    "Action": "core",
    "AnimationSpec": "core",
    "BuildContext": "core",
    "ButtonMask": "core",
    "CloseResponse": "core",
    "ColorProp": "core",
    "CursorIcon": "core",
    "DecorationsMode": "core",
    "DimWhenInactive": "core",
    "EventContext": "core",
    "EventResponse": "core",
    "ImeContext": "core",
    "ImePurpose": "core",
    "Intent": "core",
    "IntentKind": "core",
    "IntentResponse": "core",
    "IntoTeksiChild": "core",
    "IntoTeksiCondition": "core",
    "Key": "core",
    "KeyStroke": "core",
    "LayoutContext": "core",
    "LayoutResponse": "core",
    "ModalCloseBehavior": "core",
    "ModalConfig": "core",
    "ModalPresentation": "core",
    "Modifiers": "core",
    "OverscrollBehavior": "core",
    "PaintContext": "core",
    "PointerButton": "core",
    "Politeness": "core",
    "Prop": "core",
    "Shortcut": "core",
    "ShortcutRegistry": "core",
    "ShortcutScope": "core",
    "Signal": "core",
    "SizeToContent": "core",
    "TapEvent": "core",
    "TeksiBranch": "core",
    "TeksiBranch3": "core",
    "TeksiBranch4": "core",
    "TeksiloWindowId": "core",
    "TextStyleProp": "core",
    "Theme": "core",
    "ThemeAppearance": "core",
    "ThemeExtensions": "core",
    "ThemeId": "core",
    "TraversalScopePolicy": "core",
    "UserAttentionKind": "core",
    "Widget": "core",
    "WidgetBuilder": "core",
    "WidgetEvent": "core",
    "WidgetId": "core",
    "WindowCommand": "core",
    "WindowConfig": "core",
    "WindowPlacement": "core",
    "WindowRemovedCallback": "core",
    "WindowRemovedEvent": "core",
    "WindowState": "core",

    # teksilo-canvas — geometry + Canvas API
    "Canvas": "canvas",
    "EllipsisMode": "canvas",
    "Paint": "canvas",
    "Path": "canvas",
    "Point": "canvas",
    "Rect": "canvas",
    "RenderFrame": "canvas",
    "Size": "canvas",
    "SizeProposal": "canvas",
    "TextOverflow": "canvas",
    "Vec2": "canvas",

    # teksilo-tokens — color/typography tokens
    "BorderRole": "tokens",
    "Color": "tokens",
    "CornerRadius": "tokens",
    "SurfaceRole": "tokens",
    "TextRole": "tokens",
    "TextStyleRole": "tokens",

    # teksilo-app
    "TeksiloAppBuilder": "app",
    "ThemeMode": "app",

    # teksilo-settings
    "AppPaths": "settings",
    "MruEntry": "settings",
    "MruList": "settings",
    "PerWindowState": "settings",
    "SettingsBundle": "settings",
    "SettingsExt": "settings",
    "SettingsFile": "settings",
    "SettingsKey": "settings",
    "SettingsStore": "settings",
    "TEXT_SCALE_KEY": "settings",
    "WindowStateService": "settings",

    # teksilo-i18n
    "I18nConfig": "i18n",
    "LocalizedString": "i18n",
    # `LanguageIdentifier` is deliberately absent: it is `pub use
    # unic_langid::LanguageIdentifier;` — an external crate's type, not one
    # any teksilo-* crate defines, so there is no CRATE_SPECS key to point at.

    # teksilo-inspector
    "TeksiloAppBuilderInspectorExt": "inspector",

    # The umbrella crate's OWN files (toast_install.rs / webview_install.rs) —
    # these two extension traits are teksilo's own code, not a re-export of
    # another crate. `TeksiloAppBuilderAutomationExt`, by contrast, is genuinely
    # defined in teksilo-app (automation_bridge.rs) and merely re-exported
    # through automation_install.rs — see the "teksilo-app" group below.
    "TeksiloAppBuilderToastExt": "teksilo",
    "TeksiloAppBuilderWebViewExt": "teksilo",

    # teksilo-widgets — the toast/notification surface re-exported in prelude
    "EventContextToastExt": "widgets",
    "NotificationArchive": "widgets",
    "NotificationArchiveModel": "widgets",
    "NotificationCenterButton": "widgets",
    "NotificationEntry": "widgets",
    "NotificationLog": "widgets",
    "NotificationLogDialog": "widgets",
    "Toast": "widgets",
    "ToastAction": "widgets",
    "ToastActionStyle": "widgets",
    "ToastAudience": "widgets",
    "ToastDismissCause": "widgets",
    "ToastHandle": "widgets",
    "ToastHost": "widgets",
    "ToastInstallOptions": "widgets",
    "ToastRegistry": "widgets",
    "ToastRoute": "widgets",
    # `ToastPriority` is a Tier-3 style-config enum actually declared in
    # teksilo-core (styles/toast_style.rs) and merely re-exported through
    # teksilo-widgets — see the "teksilo-core" group above.
    "ToastPriority": "core",
    # `ToastSeverity` is deliberately absent: teksilo-widgets defines it as
    # `pub use teksilo_core::styles::BannerSeverity as ToastSeverity;` — a
    # renamed re-export. Neither crate's registry has a declaration under the
    # name `ToastSeverity` (`_parse_public_exports`/`type_re` don't resolve
    # `pub use X as Y` renames), so no crate key here would actually resolve;
    # the real, extractable name is `BannerSeverity` in teksilo-core.

    # teksilo-app — includes automation_bridge.rs's extension trait, which the
    # umbrella only forwards through automation_install.rs (see above).
    "TeksiloAppBuilderAutomationExt": "app",

    # teksilo-webview
    "WebSource": "webview",
    "WebView": "webview",
    "WebViewBackend": "webview",
    "WebViewEvent": "webview",
    "WebViewHandle": "webview",
    "WebViewId": "webview",
    "WebViewRegistry": "webview",
    # `WebViewStyle` is the Tier-3 style trait, declared in teksilo-core
    # (styles/web_view_style.rs) like every other `*Style` protocol, and
    # merely re-exported through teksilo-webview — see "teksilo-core" above.
    "WebViewStyle": "core",

    # teksilo-terminal
    "BellStyle": "terminal",
    "ColorScheme": "terminal",
    "CursorStyle": "terminal",
    "Terminal": "terminal",
    "TerminalClosePolicy": "terminal",
    "TerminalCommand": "terminal",
    "TerminalController": "terminal",
    "TerminalStyle": "terminal",

    # teksilo-platform (file_dialog submodule)
    "EventContextFileDialogExt": "platform",
    "FileDialogHandle": "platform",
    "FileDialogRequest": "platform",
    "FileDialogResult": "platform",

    # teksilo-async
    "AsyncRuntimeHandle": "async",
    "BlockingError": "async",
    "EventContextAsyncExt": "async",
    "TaskHandle": "async",
    "TeksiloAppBuilderAsyncExt": "async",
}


# ----------------------------------------------------------------------------
# Data model
# ----------------------------------------------------------------------------


@dataclass
class EnumVariant:
    name: str
    signature: str
    doc: str


@dataclass
class Item:
    kind: str  # 'struct' | 'enum' | 'type' | 'const' | 'fn' | 'external'
    name: str
    signature: str
    doc: str
    hidden: bool = False
    cfg: list[str] = field(default_factory=list)
    variants: list[EnumVariant] = field(default_factory=list)
    methods: list["Item"] = field(default_factory=list)


@dataclass
class ParsedFile:
    module_name: str
    file_path: Path
    header_doc: str
    cfg: list[str]
    items: list[Item]


# ----------------------------------------------------------------------------
# Line cleanup — blank out string/char literals and comments so we can count
# braces / parens without being fooled by characters inside literals.
# ----------------------------------------------------------------------------


class BlockCommentTracker:
    """Removes /* ... */ content (possibly multi-line), keeping line length.

    Does not try to handle nested block comments correctly for depth > 1;
    teksilo-widgets code does not use them.
    """

    def __init__(self) -> None:
        self.in_block = False

    def process(self, line: str) -> str:
        out: list[str] = []
        i = 0
        n = len(line)
        while i < n:
            if self.in_block:
                j = line.find("*/", i)
                if j < 0:
                    out.append(" " * (n - i))
                    return "".join(out)
                out.append(" " * (j + 2 - i))
                i = j + 2
                self.in_block = False
            else:
                j = line.find("/*", i)
                if j < 0:
                    out.append(line[i:])
                    return "".join(out)
                out.append(line[i:j])
                out.append("  ")
                i = j + 2
                self.in_block = True
        return "".join(out)


def strip_line_literals(line: str) -> str:
    """Return a line with string/char literals and line-comment content replaced
    by spaces, so brace/paren counting is safe."""
    out: list[str] = []
    i = 0
    n = len(line)
    while i < n:
        c = line[i]
        if c == "/" and i + 1 < n and line[i + 1] == "/":
            # Line comment — preserve leading `//` so we can still recognize
            # `///` and `//!`, but blank the rest.
            out.append("//")
            out.append(" " * (n - i - 2))
            return "".join(out)
        if c == '"':
            out.append(" ")
            i += 1
            while i < n:
                if line[i] == "\\" and i + 1 < n:
                    out.append("  ")
                    i += 2
                elif line[i] == '"':
                    out.append(" ")
                    i += 1
                    break
                else:
                    out.append(" ")
                    i += 1
            continue
        if c == "'":
            # Char literal or lifetime?  Char literals: 'x', '\n', '\u{...}'.
            m = re.match(r"'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^'\\])'", line[i:])
            if m:
                out.append(" " * len(m.group(0)))
                i += len(m.group(0))
                continue
            out.append(c)
            i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def preprocess(raw_lines: list[str]) -> list[str]:
    bc = BlockCommentTracker()
    return [strip_line_literals(bc.process(line)) for line in raw_lines]


# ----------------------------------------------------------------------------
# Regex helpers
# ----------------------------------------------------------------------------

DOC_OUTER = re.compile(r"^\s*///(.*)$")
DOC_INNER = re.compile(r"^\s*//!(.*)$")
ATTR_PREFIX = re.compile(r"^\s*#\[")
IMPL_PREFIX = re.compile(r"^\s*impl\b")
PUB_STRUCT = re.compile(r"^\s*pub\s+struct\s+([A-Za-z_]\w*)")
PUB_ENUM = re.compile(r"^\s*pub\s+enum\s+([A-Za-z_]\w*)")
PUB_TYPE = re.compile(r"^\s*pub\s+type\s+([A-Za-z_]\w*)")
PUB_CONST = re.compile(r"^\s*pub\s+(?:const|static)\s+([A-Za-z_]\w*)")
PUB_FN = re.compile(
    r"^\s*pub"  # only fully-public; `pub(crate)` etc. are excluded
    r"(?:\s+(?:async|const|unsafe|extern(?:\s+\"[^\"]*\")?))*"
    r"\s+fn\s+([A-Za-z_]\w*)"
)
# `impl ... Widget for X` — allows a path-qualified trait (`widget::Widget`),
# leading generics (`impl<T> Widget for Foo<T>`), and captures the target type
# name. `\bWidget\s+for` won't match `WidgetBuilder for` (no whitespace after
# `Widget`).
WIDGET_IMPL_RE = re.compile(r"impl\b[^{]*?\bWidget\s+for\s+([A-Za-z_]\w*)")


def _doc_text(m: re.Match[str]) -> str:
    """Strip one optional leading space from a /// or //! capture."""
    s = m.group(1)
    if s.startswith(" "):
        s = s[1:]
    return s


# ----------------------------------------------------------------------------
# Low-level scanners — all operate on (raw, cleaned) parallel line lists
# ----------------------------------------------------------------------------


def find_matching_brace(
    cleaned: list[str], start_line: int, start_col: int
) -> tuple[int, int]:
    """Given that cleaned[start_line][start_col] == '{', return (line, col) of
    the matching '}'."""
    depth = 0
    for li in range(start_line, len(cleaned)):
        line = cleaned[li]
        start = start_col if li == start_line else 0
        for ci in range(start, len(line)):
            c = line[ci]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    return li, ci
    return len(cleaned) - 1, 0


def consume_attribute(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int]:
    """Parse a possibly multi-line #[ ... ] attribute starting at line i.
    Return (attribute_text_joined_on_one_line, end_line_idx)."""
    depth = 0
    started = False
    for li in range(i, len(cleaned)):
        line = cleaned[li]
        for col, c in enumerate(line):
            if c == "[":
                depth += 1
                started = True
            elif c == "]":
                depth -= 1
                if started and depth == 0:
                    if li == i:
                        return raw[i][: col + 1].strip(), i
                    parts = (
                        [raw[i].strip()]
                        + [raw[j].strip() for j in range(i + 1, li)]
                        + [raw[li][: col + 1].strip()]
                    )
                    return " ".join(p for p in parts if p), li
    # Malformed — consume a single line.
    return raw[i].strip(), i


def _join_signature(raw: list[str], start: int, end_line: int, end_col: int) -> str:
    if end_line == start:
        return raw[start][:end_col].rstrip()
    parts = [raw[start]]
    parts.extend(raw[j] for j in range(start + 1, end_line))
    parts.append(raw[end_line][:end_col])
    return "\n".join(parts).rstrip()


def consume_stmt(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int]:
    """Consume a statement ending with `;` at brace-depth 0. Returns
    (signature_including_semicolon, end_line_idx)."""
    depth = 0
    for li in range(i, len(cleaned)):
        for col, c in enumerate(cleaned[li]):
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
            elif depth == 0 and c == ";":
                return _join_signature(raw, i, li, col + 1), li
    return raw[i].rstrip(), i


def consume_item_signature(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int, tuple[int, int] | None]:
    """Parse `pub struct X ...;` or `pub struct X ... { ... }` or
    `pub enum X { ... }`. Returns (signature_without_body, end_line,
    brace_open_pos_or_None). If the item is unit/tuple (ends with ;), returns
    (sig_with_trailing_semi, line_of_semi, None). Otherwise returns
    (sig_up_to_but_not_including_brace, line_of_closing_brace, (open_line,
    open_col))."""
    paren = 0
    for li in range(i, len(cleaned)):
        for col, c in enumerate(cleaned[li]):
            if c == "(":
                paren += 1
            elif c == ")":
                paren -= 1
            elif paren == 0 and c == ";":
                return _join_signature(raw, i, li, col + 1), li, None
            elif paren == 0 and c == "{":
                sig = _join_signature(raw, i, li, col)
                close_line, _ = find_matching_brace(cleaned, li, col)
                return sig, close_line, (li, col)
    return raw[i].rstrip(), i, None


def consume_fn_signature(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int, bool]:
    """Parse a `fn name(...) -> ... { body }` or `fn name(...) -> ... ;`
    starting at line i. Returns (signature_without_brace_or_semi, end_line,
    had_body). For had_body==True, end_line is the line of the matching `}`."""
    paren = 0
    started_paren = False
    for li in range(i, len(cleaned)):
        for col, c in enumerate(cleaned[li]):
            if c == "(":
                paren += 1
                started_paren = True
            elif c == ")":
                paren -= 1
            elif paren == 0 and started_paren and c == ";":
                return _join_signature(raw, i, li, col), li, False
            elif paren == 0 and started_paren and c == "{":
                sig = _join_signature(raw, i, li, col)
                close_line, _ = find_matching_brace(cleaned, li, col)
                return sig, close_line, True
    return raw[i].rstrip(), i, False


def consume_impl_header(
    raw: list[str], cleaned: list[str], i: int
) -> tuple[str, int, int] | None:
    """Find the `{` that opens the impl block. Return (header_text,
    open_line, open_col) or None if malformed."""
    for li in range(i, len(cleaned)):
        for col, c in enumerate(cleaned[li]):
            if c == "{":
                header = _join_signature(raw, i, li, col)
                return header, li, col
    return None


# ----------------------------------------------------------------------------
# impl block analysis
# ----------------------------------------------------------------------------


_HRTB_RE = re.compile(r"for\s*<[^>]*>")


def _normalize_impl_header(header: str) -> str:
    """Strip higher-ranked trait bound patterns like `for<'a>` so we can
    detect the trait-impl `for` keyword cleanly."""
    return _HRTB_RE.sub(" ", header)


def is_trait_impl(header: str) -> bool:
    return re.search(r"\bfor\b", _normalize_impl_header(header)) is not None


def extract_impl_target(header: str) -> str:
    """Return the target type name of an `impl ...` header (the thing the
    methods attach to)."""
    normalized = _normalize_impl_header(header)
    # Drop the leading `impl` keyword.
    m = re.match(r"\s*impl\b", normalized)
    if not m:
        return ""
    body = normalized[m.end() :].strip()

    # Strip leading generics <...>.
    if body.startswith("<"):
        depth = 0
        j = 0
        while j < len(body):
            if body[j] == "<":
                depth += 1
            elif body[j] == ">":
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            j += 1
        body = body[j:].strip()

    if re.search(r"\bfor\b", body):
        # `Trait for Target [where ...]`
        _, _, after = body.partition(" for ")
        if not after:
            # Fallback for cases where there's no space around `for`.
            after = re.split(r"\bfor\b", body, maxsplit=1)[1]
        target_part = after.strip()
    else:
        target_part = body

    target_part = re.split(r"\bwhere\b", target_part, maxsplit=1)[0].strip()
    target_part = target_part.rstrip("{").strip()
    m2 = re.match(r"([A-Za-z_][A-Za-z0-9_]*)", target_part)
    return m2.group(1) if m2 else ""


# ----------------------------------------------------------------------------
# Per-file parser
# ----------------------------------------------------------------------------


def _is_hidden(attrs: list[str]) -> bool:
    return any(
        re.search(r"#\[\s*doc\s*\(\s*hidden\s*\)\s*\]", a) for a in attrs
    )


def _extract_cfgs(attrs: list[str]) -> list[str]:
    return [a for a in attrs if re.search(r"#\[\s*cfg\s*\(", a)]


def _parse_enum_variants(
    raw: list[str], cleaned: list[str], start: int, end: int
) -> list[EnumVariant]:
    """Parse variants from cleaned[start..=end] which is the interior of the
    enum body (between `{` and `}`)."""
    variants: list[EnumVariant] = []
    v_doc: list[str] = []
    i = start
    while i <= end:
        line = raw[i]
        cln = cleaned[i]
        stripped = line.strip()

        m = DOC_OUTER.match(line)
        if m:
            v_doc.append(_doc_text(m))
            i += 1
            continue
        if stripped.startswith("#["):
            _, end_i = consume_attribute(raw, cleaned, i)
            i = end_i + 1
            continue
        if not stripped:
            i += 1
            continue

        # Start of a variant. Collect until `,` at brace/paren-depth 0, or end.
        paren = 0
        brace = 0
        end_line = None
        end_col = None
        for li in range(i, end + 1):
            for col, c in enumerate(cleaned[li]):
                if c == "(":
                    paren += 1
                elif c == ")":
                    paren -= 1
                elif c == "{":
                    brace += 1
                elif c == "}":
                    brace -= 1
                elif paren == 0 and brace == 0 and c == ",":
                    end_line, end_col = li, col
                    break
            if end_line is not None:
                break
        if end_line is None:
            end_line = end
            end_col = len(cleaned[end])

        sig = _join_signature(raw, i, end_line, end_col).strip()
        name_m = re.match(r"(\w+)", sig)
        name = name_m.group(1) if name_m else "<?>"
        variants.append(
            EnumVariant(
                name=name,
                signature=sig,
                doc="\n".join(v_doc).rstrip(),
            )
        )
        v_doc = []
        i = end_line + 1
    return variants


def _parse_impl_body(
    raw: list[str], cleaned: list[str], start: int, end: int
) -> list[Item]:
    """Parse inside an inherent impl block body (between `{` and `}`). Emit
    only public associated items: `pub fn`, `pub const`, `pub type`."""
    methods: list[Item] = []
    doc_buf: list[str] = []
    attr_buf: list[str] = []

    def clear() -> None:
        doc_buf.clear()
        attr_buf.clear()

    i = start
    while i <= end:
        line = raw[i]
        cln = cleaned[i]
        stripped = line.strip()

        m = DOC_OUTER.match(line)
        if m:
            doc_buf.append(_doc_text(m))
            i += 1
            continue
        if stripped.startswith("#["):
            attr, end_i = consume_attribute(raw, cleaned, i)
            attr_buf.append(attr)
            i = end_i + 1
            continue
        if not stripped:
            # Blank line between /// block and item would break attachment in
            # Rust. Detect by peeking ahead.
            if doc_buf or attr_buf:
                j = i + 1
                while j <= end and not raw[j].strip():
                    j += 1
                if j > end:
                    clear()
                else:
                    nxt = raw[j]
                    if not (
                        DOC_OUTER.match(nxt) or nxt.strip().startswith("#[")
                    ):
                        clear()
            i += 1
            continue

        if PUB_FN.match(line):
            name = PUB_FN.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line, had_body = consume_fn_signature(raw, cleaned, i)
            methods.append(
                Item(
                    kind="fn",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        if PUB_CONST.match(line):
            name = PUB_CONST.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line = consume_stmt(raw, cleaned, i)
            methods.append(
                Item(
                    kind="const",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        if PUB_TYPE.match(line):
            name = PUB_TYPE.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line = consume_stmt(raw, cleaned, i)
            methods.append(
                Item(
                    kind="type",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        # Non-pub item inside impl — skip, count as body. For simplicity we
        # advance one line; brace balancing is handled by the outer caller
        # which computed `end`.
        clear()
        i += 1
    return methods


def parse_file(path: Path, module_name: str, cfg: list[str]) -> ParsedFile:
    raw = path.read_text(encoding="utf-8").splitlines()
    cleaned = preprocess(raw)
    n = len(raw)

    # --- Module header: the //! block near the top. Every file opens with two
    # `// SPDX-...` license comments (and occasionally a `#![...]` inner attr)
    # BEFORE the //! header, so skip those leading lines until the //! block
    # starts; once it starts, stop at the first non-//! non-blank line.
    header_lines: list[str] = []
    started = False
    i = 0
    while i < n:
        stripped = raw[i].strip()
        if not stripped:
            if started:
                header_lines.append("")
            i += 1
            continue
        m = DOC_INNER.match(raw[i])
        if m:
            started = True
            header_lines.append(_doc_text(m))
            i += 1
            continue
        if not started and (stripped.startswith("//") or stripped.startswith("#![")):
            # Leading license comment / inner attribute before the header.
            i += 1
            continue
        # Stop at the first real (non-//!) line once we've passed the preamble.
        break
    while header_lines and not header_lines[-1].strip():
        header_lines.pop()
    header_doc = "\n".join(header_lines)

    items: list[Item] = []
    item_by_name: dict[str, Item] = {}

    doc_buf: list[str] = []
    attr_buf: list[str] = []

    def clear() -> None:
        doc_buf.clear()
        attr_buf.clear()

    def ensure_item(name: str) -> Item:
        it = item_by_name.get(name)
        if it is not None:
            return it
        placeholder = Item(
            kind="external", name=name, signature="", doc=""
        )
        items.append(placeholder)
        item_by_name[name] = placeholder
        return placeholder

    i = 0
    while i < n:
        line = raw[i]
        stripped = line.strip()

        # Skip //! anywhere (module header already captured).
        if DOC_INNER.match(line):
            i += 1
            continue

        m = DOC_OUTER.match(line)
        if m:
            doc_buf.append(_doc_text(m))
            i += 1
            continue

        if stripped.startswith("#["):
            attr, end_i = consume_attribute(raw, cleaned, i)
            attr_buf.append(attr)
            i = end_i + 1
            continue

        if not stripped:
            # Blank line between pending docs and an unrelated item breaks
            # attachment; detect and clear.
            if doc_buf or attr_buf:
                j = i + 1
                while j < n and not raw[j].strip():
                    j += 1
                if j >= n:
                    clear()
                else:
                    nxt = raw[j]
                    if not (DOC_OUTER.match(nxt) or nxt.strip().startswith("#[")):
                        clear()
            i += 1
            continue

        # impl block
        if IMPL_PREFIX.match(line):
            parsed = consume_impl_header(raw, cleaned, i)
            if parsed is None:
                clear()
                i += 1
                continue
            header, open_line, open_col = parsed
            close_line, _ = find_matching_brace(cleaned, open_line, open_col)

            if is_trait_impl(header):
                # Skip the whole block — internal plumbing.
                clear()
                i = close_line + 1
                continue

            target = extract_impl_target(header)
            methods = _parse_impl_body(
                raw, cleaned, open_line + 1, close_line - 1
            )
            parent = ensure_item(target) if target else None
            if parent is not None:
                parent.methods.extend(methods)
            clear()
            i = close_line + 1
            continue

        if PUB_STRUCT.match(line):
            name = PUB_STRUCT.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line, _open = consume_item_signature(raw, cleaned, i)
            item = Item(
                kind="struct",
                name=name,
                signature=sig.strip(),
                doc="\n".join(doc_buf).rstrip(),
                hidden=_is_hidden(attr_buf),
                cfg=_extract_cfgs(attr_buf),
            )
            items.append(item)
            item_by_name[name] = item
            clear()
            i = end_line + 1
            continue

        if PUB_ENUM.match(line):
            name = PUB_ENUM.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line, open_pos = consume_item_signature(raw, cleaned, i)
            variants: list[EnumVariant] = []
            if open_pos is not None:
                open_line, _oc = open_pos
                variants = _parse_enum_variants(
                    raw, cleaned, open_line + 1, end_line - 1
                )
            item = Item(
                kind="enum",
                name=name,
                signature=sig.strip(),
                doc="\n".join(doc_buf).rstrip(),
                hidden=_is_hidden(attr_buf),
                cfg=_extract_cfgs(attr_buf),
                variants=variants,
            )
            items.append(item)
            item_by_name[name] = item
            clear()
            i = end_line + 1
            continue

        if PUB_TYPE.match(line):
            name = PUB_TYPE.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line = consume_stmt(raw, cleaned, i)
            items.append(
                Item(
                    kind="type",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        if PUB_CONST.match(line):
            name = PUB_CONST.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line = consume_stmt(raw, cleaned, i)
            items.append(
                Item(
                    kind="const",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        if PUB_FN.match(line):
            name = PUB_FN.match(line).group(1)  # type: ignore[union-attr]
            sig, end_line, _had_body = consume_fn_signature(raw, cleaned, i)
            items.append(
                Item(
                    kind="fn",
                    name=name,
                    signature=sig.strip(),
                    doc="\n".join(doc_buf).rstrip(),
                    hidden=_is_hidden(attr_buf),
                    cfg=_extract_cfgs(attr_buf),
                )
            )
            clear()
            i = end_line + 1
            continue

        # Anything else (use, mod, private items…) — drop pending metadata.
        clear()
        i += 1

    # Drop `external` placeholder items that never received any methods —
    # they're types defined elsewhere that we don't care about.
    items = [
        it
        for it in items
        if it.kind != "external" or it.methods
    ]
    return ParsedFile(
        module_name=module_name,
        file_path=path,
        header_doc=header_doc,
        cfg=cfg,
        items=items,
    )


# ----------------------------------------------------------------------------
# Widget registry — discover files, build name lookup
# ----------------------------------------------------------------------------


@dataclass
class Registry:
    files: list[Path]
    cfg_by_file: dict[Path, list[str]]
    type_to_file: dict[str, Path]  # lowercased type name -> file
    module_to_file: dict[str, Path]  # lowercased file stem -> file
    type_display: dict[Path, list[str]]  # file -> all exported type names
    widget_display: dict[Path, list[str]]  # file -> just the widget type(s)
    exported: set[str] = field(default_factory=set)  # names re-exported from lib.rs


def _stem_to_camel(stem: str) -> str:
    """`date_edit` -> `DateEdit`, `button` -> `Button`."""
    return "".join(part[:1].upper() + part[1:] for part in stem.split("_") if part)


def _is_test_file(p: Path) -> bool:
    return p.name == "tests.rs" or p.name.endswith("_tests.rs")


_EXPORT_TOKEN_RE = re.compile(r"[A-Z][A-Za-z0-9_]*")


def _parse_public_exports(lib_rs: Path) -> set[str]:
    """Return every type-ish name re-exported via `pub use ...;` in lib.rs.

    The crate root lists its public surface explicitly (no `::*` globs), so the
    CamelCase / SCREAMING tokens inside each `pub use` statement are exactly the
    publicly reachable type and const names. snake_case path segments are
    lowercase and so excluded by the leading-uppercase requirement.
    """
    if not lib_rs.exists():
        return set()
    text = lib_rs.read_text(encoding="utf-8")
    exported: set[str] = set()
    for m in re.finditer(r"\bpub\s+use\b(.*?);", text, re.DOTALL):
        exported.update(_EXPORT_TOKEN_RE.findall(m.group(1)))
    return exported


def _pick_widget_names(
    stem: str,
    pub_names: list[str],
    widget_impls: set[str],
    exported: set[str],
    nested: bool,
    is_widget: bool = True,
) -> list[str]:
    """Choose which names to surface for a file in `--list` / the catalog.

    For non-widget crates a catalog entry is simply a file's re-exported public
    type(s), preferring the one matching the file stem; a file with no re-exported
    type gets no page (this drops impl-split modules like `view/gestures_impl.rs`
    and internal helpers).

    For the widget crate a genuine widget is a pub type that both `impl Widget`
    and is re-exported. Nested submodule files have no fallback (keeps helpers
    like `HeaderCell` out); top-level files keep the lenient historical fallback.
    """
    camel = _stem_to_camel(stem)
    if not is_widget:
        candidates = [n for n in pub_names if n in exported]
        if camel in candidates:
            return [camel]
        return candidates

    candidates = [n for n in pub_names if n in widget_impls and n in exported]
    if camel in candidates:
        return [camel]
    if candidates:
        return candidates
    if nested:
        return []

    # Top-level fallback — preserve historical behaviour.
    widget_pub = [n for n in pub_names if n in widget_impls]
    if camel in widget_pub:
        return [camel]
    if widget_pub:
        return widget_pub
    if camel in pub_names:
        return [camel]
    return pub_names


def _collect_cfgs_from_aggregator(aggregator: Path, base: Path) -> dict[Path, list[str]]:
    """Parse an aggregator file (lib.rs, primitives.rs) and return
    file-path -> list of #[cfg(...)] attrs attached to its `pub mod` line."""
    out: dict[Path, list[str]] = {}
    if not aggregator.exists():
        return out
    raw = aggregator.read_text(encoding="utf-8").splitlines()
    pending: list[str] = []
    for line in raw:
        s = line.strip()
        if not s or s.startswith("//"):
            continue
        if s.startswith("#["):
            if re.search(r"#\[\s*cfg\s*\(", s):
                pending.append(s)
            continue
        m = re.match(r"(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+(\w+)\s*;", s)
        if m:
            name = m.group(1)
            target = base / f"{name}.rs"
            if target.exists():
                out[target.resolve()] = list(pending)
            pending = []
            continue
        # Any other statement resets pending cfgs.
        pending = []
    return out


def build_registry() -> Registry:
    src = SPEC.src
    if not src.exists():
        raise SystemExit(f"{SPEC.crate} src not found at {src}")

    # Recursive discovery so types defined in submodule directories are found
    # too. Shallow paths sort first so the conventional top-level file wins any
    # name/module collision.
    files = sorted(
        (
            p
            for p in src.rglob("*.rs")
            if _is_scannable(p)
        ),
        key=lambda p: (len(p.relative_to(src).parts), str(p)),
    )

    cfg_by_file: dict[Path, list[str]] = {}
    cfg_by_file.update(_collect_cfgs_from_aggregator(src / "lib.rs", src))
    if SPEC.is_widget:
        cfg_by_file.update(
            _collect_cfgs_from_aggregator(src / "primitives.rs", src / "primitives")
        )
        cfg_by_file.update(
            _collect_cfgs_from_aggregator(src / "animations.rs", src / "animations")
        )

    # Nested files inherit the cfg of their top-level ancestor module
    # (e.g. color_picker/swatch.rs inherits color_picker.rs's rich-text gate).
    for fp in files:
        rel = fp.relative_to(src)
        if len(rel.parts) > 1 and fp.resolve() not in cfg_by_file:
            ancestor = (src / f"{rel.parts[0]}.rs").resolve()
            inherited = cfg_by_file.get(ancestor)
            if inherited:
                cfg_by_file[fp.resolve()] = list(inherited)

    exported = _parse_public_exports(src / "lib.rs")

    type_to_file: dict[str, Path] = {}
    module_to_file: dict[str, Path] = {}
    type_display: dict[Path, list[str]] = {}
    widget_display: dict[Path, list[str]] = {}
    type_re = re.compile(r"^\s*pub\s+(?:struct|enum|type|trait)\s+([A-Za-z_]\w*)", re.MULTILINE)

    for fp in files:
        module_to_file.setdefault(fp.stem.lower(), fp)
        text = fp.read_text(encoding="utf-8")
        names = [m.group(1) for m in type_re.finditer(text)]
        type_display[fp] = names
        widget_impls = (
            {m.group(1) for m in WIDGET_IMPL_RE.finditer(text)} if SPEC.is_widget else set()
        )
        # `primitives/` and `animations/` are flat one-type-per-file collections,
        # like the top-level dir, so they keep the lenient fallback. Per-widget
        # submodule dirs use strict re-export filtering to drop internal helpers.
        rel_parts = fp.relative_to(src).parts
        lenient = len(rel_parts) == 1 or (
            len(rel_parts) == 2 and rel_parts[0] in ("primitives", "animations")
        )
        widget_display[fp] = _pick_widget_names(
            fp.stem, names, widget_impls, exported, nested=not lenient,
            is_widget=SPEC.is_widget,
        )
        for name in names:
            type_to_file.setdefault(name.lower(), fp)

    return Registry(
        files=files,
        cfg_by_file=cfg_by_file,
        type_to_file=type_to_file,
        module_to_file=module_to_file,
        type_display=type_display,
        widget_display=widget_display,
        exported=exported,
    )


def resolve_name(reg: Registry, name: str) -> Path | None:
    key = name.lower()
    if key in reg.type_to_file:
        return reg.type_to_file[key]
    if key in reg.module_to_file:
        return reg.module_to_file[key]
    return None


class MissingCrateSource(Exception):
    """A `CRATE_SPECS` entry whose `src/` is not on disk in this checkout.

    Not every entry exists everywhere. The two external siblings are the
    standing case — `text-document` / `text-typeset` are crates.io packages
    that a consumer resolves and `cargo teksilo symbol` stages beside the
    teksilo crates, while a framework checkout has no `crates/text-document/`
    at all — and a partial or feature-gated checkout is the same shape.

    This is a plain `Exception` on purpose. `build_registry()` raises
    `SystemExit` for the *active* crate, which is right when the user named it
    with `--crate`; but `SystemExit` derives from `BaseException`, so the
    `except Exception` in `_crate_owners` never caught it and one absent crate
    took down every cross-crate lookup — exactly the case its comment claimed
    to tolerate.
    """


_registry_cache: dict[str, Registry] = {}


def _registry_for(key: str) -> Registry:
    """Build (and memoize) the `Registry` for a `CRATE_SPECS` key without
    disturbing the caller's module-global `SPEC`.

    `build_registry()` reads `SPEC` for the crate's `src/` path, its
    `is_widget` flag and its aggregator files, so building a second crate's
    registry means swapping `SPEC` in and back out again — the same
    save/restore dance `run_self_tests()` already does around the `data`
    crate check.
    """
    if key in _registry_cache:
        return _registry_cache[key]
    if not CRATE_SPECS[key].src.exists():
        raise MissingCrateSource(key)
    global SPEC
    prev = SPEC
    try:
        SPEC = CRATE_SPECS[key]
        reg = build_registry()
    finally:
        SPEC = prev
    _registry_cache[key] = reg
    return reg


def _crate_owners(name: str) -> list[str]:
    """Every `CRATE_SPECS` key whose registry resolves `name`, in declaration
    order.

    Used for the last-resort sweep in `_resolve_across_crates` and to report
    the alternatives when more than one crate defines the name. Building 30
    registries is only worth it on a miss, so every caller is on an error or
    a cross-crate path; `_registry_for` memoizes, so the sweep is paid once.
    """
    owners: list[str] = []
    for key in CRATE_SPECS:
        try:
            if resolve_name(_registry_for(key), name) is not None:
                owners.append(key)
        except Exception:
            # A crate whose source is absent (a partial checkout, a
            # feature-gated path) must not take the whole lookup down: it
            # just cannot own the name.
            continue
    return owners


def _cross_crate_hints(name: str, active_key: str, n: int = 5) -> list[str]:
    """Close matches for `name` across *every* queryable crate, each tagged
    with the `--crate` flag that reaches it.

    Drawing hints from the active crate alone is how `symbol ListModel` used
    to answer "Did you mean: splittermodel, model, list_source?" — three
    teksilo-widgets names, none of them the teksilo-data type the user asked
    for, and no mention that `--crate data` exists. A name that resolves
    exactly in another crate never reaches here (the sweep in
    `_resolve_across_crates` already returned it), so these really are typo
    candidates.
    """
    pool: dict[str, str] = {}
    for key in CRATE_SPECS:
        try:
            r = _registry_for(key)
        except Exception:
            continue
        for candidate in set(r.type_to_file) | set(r.module_to_file):
            pool.setdefault(candidate, key)
    return [
        m if pool[m] == active_key else f"{m} (--crate {pool[m]})"
        for m in difflib.get_close_matches(name.lower(), list(pool), n=n)
    ]


def _resolve_across_crates(
    reg: Registry, current_key: str, name: str
) -> tuple[Path | None, str, Registry]:
    """Resolve `name` against the active crate's registry first; on a miss,
    consult `UMBRELLA_REEXPORTS` for a name reachable through
    `teksilo::prelude`; on a second miss, sweep every crate in `CRATE_SPECS`.

    Returns `(file, crate_key_used, registry_used)`. The caller needs
    `registry_used` (not just `reg`) to look up the resolved file's cfg gates
    — a file resolved in another crate's registry has its `cfg_by_file` entry
    recorded there, not in `reg`.

    The sweep is the third step rather than the first because
    `UMBRELLA_REEXPORTS` is *authoritative* where the two disagree: 81 names
    (types and modules; 11 of them types)
    are defined in more than one crate, and for those the hand-maintained
    table names the one an app actually sees. `Theme` lives in teksilo-core,
    teksilo-tokens and teksilo-inspector; only the table knows that
    `teksilo::prelude::Theme` is teksilo-core's.

    The sweep exists because the table's premise — "reachable through
    `teksilo::prelude`" — is narrower than what a consumer can reach.
    `teksilo` also re-exports its peer crates as modules (`pub use
    teksilo_data as data;`), so `teksilo::data::ListModel` is perfectly
    reachable while `ListModel` is in no prelude and so was in no table.
    Asking for it printed `unknown widget 'ListModel'` with three unrelated
    teksilo-widgets suggestions — for an agent, indistinguishable from "this
    type does not exist", which is the failure mode this whole tool is built
    against. The skill has always advertised `cargo teksilo symbol ListModel`;
    the sweep is what makes that true, for 2247 names across 30 crates rather
    than for the ~100 hand-listed ones.
    """
    fp = resolve_name(reg, name)
    if fp is not None:
        return fp, current_key, reg
    owner = UMBRELLA_REEXPORTS.get(name)
    if owner is not None and owner != current_key:
        other_reg = _registry_for(owner)
        fp = resolve_name(other_reg, name)
        if fp is not None:
            return fp, owner, other_reg
    for key in _crate_owners(name):
        if key == current_key:
            continue
        other_reg = _registry_for(key)
        fp = resolve_name(other_reg, name)
        if fp is not None:
            return fp, key, other_reg
    return None, current_key, reg


# ----------------------------------------------------------------------------
# Formatters
# ----------------------------------------------------------------------------


def _fmt_cfg(cfg: list[str]) -> str:
    return " ".join(cfg)


def format_markdown(pf: ParsedFile) -> str:
    out: list[str] = []
    rel = pf.file_path.relative_to(REPO_ROOT) if REPO_ROOT in pf.file_path.parents else pf.file_path
    out.append(f"# `{pf.module_name}.rs`")
    out.append("")
    out.append(f"> Source: [{rel}]({rel})")
    if pf.cfg:
        out.append(f"> cfg: `{_fmt_cfg(pf.cfg)}`")
    out.append("")

    if pf.header_doc:
        out.append(pf.header_doc.rstrip())
        out.append("")

    _emit_items_md(pf.items, out)

    return "\n".join(out).rstrip() + "\n"


def _emit_items_md(items: list[Item], out: list[str]) -> None:
    """Render the struct/enum/type/const/fn items of a file as Markdown.

    Shared by `format_markdown` (the `--format md` output) and
    `format_catalog_markdown` (the mdBook catalog pages) so both render the API
    surface identically.
    """
    for it in items:
        if it.kind == "external":
            # Type defined elsewhere, but has methods in this file.
            out.append(f"## `impl {it.name}`  *(methods defined in this file)*")
            out.append("")
            _emit_methods_md(it, out)
            continue

        flags: list[str] = []
        if it.hidden:
            flags.append("hidden")
        if it.cfg:
            flags.append(_fmt_cfg(it.cfg))
        flags_str = f"  *({'; '.join(flags)})*" if flags else ""

        if it.kind == "struct":
            out.append(f"## `pub struct {it.name}`{flags_str}")
        elif it.kind == "enum":
            out.append(f"## `pub enum {it.name}`{flags_str}")
        elif it.kind == "type":
            out.append(f"## `pub type {it.name}`{flags_str}")
        elif it.kind == "const":
            out.append(f"## `pub const {it.name}`{flags_str}")
        elif it.kind == "fn":
            out.append(f"## `pub fn {it.name}(...)`{flags_str}")
        out.append("")

        if it.doc:
            out.append(it.doc.rstrip())
            out.append("")

        if it.kind == "struct":
            out.append("```rust")
            out.append(f"{it.signature} {{ /* fields */ }}"
                       if "{" not in it.signature and not it.signature.rstrip().endswith(";")
                       else it.signature)
            out.append("```")
            out.append("")
        elif it.kind == "enum":
            out.append("```rust")
            out.append(f"{it.signature} {{ /* variants */ }}")
            out.append("```")
            out.append("")
        elif it.kind in ("type", "const", "fn"):
            out.append("```rust")
            sig = it.signature
            if it.kind == "fn" and not sig.rstrip().endswith(";"):
                sig = sig + ";"
            out.append(sig)
            out.append("```")
            out.append("")

        if it.kind == "enum" and it.variants:
            out.append("### Variants")
            out.append("")
            for v in it.variants:
                doc = v.doc.replace("\n", " ").strip()
                if doc:
                    out.append(f"- **`{v.name}`** — {doc}")
                else:
                    out.append(f"- **`{v.name}`**")
            out.append("")

        if it.methods:
            _emit_methods_md(it, out)


def _emit_methods_md(it: Item, out: list[str]) -> None:
    out.append("### Methods")
    out.append("")
    for m in it.methods:
        flags: list[str] = []
        if m.hidden:
            flags.append("hidden")
        if m.cfg:
            flags.append(_fmt_cfg(m.cfg))
        flags_str = f"  *({'; '.join(flags)})*" if flags else ""
        # Collapse multi-line signatures onto one line: a `#### `...`` heading is
        # an inline-code span, so a wrapped signature (e.g. a long `composite_tooltip(
        # …, impl Widget + 'static)`) would leave the span unclosed and break the
        # heading's rendering.
        sig = " ".join(m.signature.split())
        out.append(f"#### `{sig}`{flags_str}")
        out.append("")
        if m.doc:
            out.append(m.doc.rstrip())
            out.append("")


def format_text(pf: ParsedFile) -> str:
    out: list[str] = []
    out.append(f"=== {pf.module_name}.rs ===")
    out.append(f"Path: {pf.file_path}")
    if pf.cfg:
        out.append(f"cfg: {_fmt_cfg(pf.cfg)}")
    out.append("")
    if pf.header_doc:
        out.append(pf.header_doc.rstrip())
        out.append("")

    for it in pf.items:
        if it.kind == "external":
            out.append(f"--- impl {it.name} (methods defined in this file) ---")
        else:
            tag = it.kind
            flags = []
            if it.hidden:
                flags.append("hidden")
            if it.cfg:
                flags.append(_fmt_cfg(it.cfg))
            fs = f" [{'; '.join(flags)}]" if flags else ""
            out.append(f"--- {tag} {it.name}{fs} ---")
            if it.doc:
                out.append(_indent(it.doc, "  "))
            if it.signature:
                out.append(f"  {it.signature.strip()}")

        if it.variants:
            out.append("  variants:")
            for v in it.variants:
                if v.doc:
                    out.append(f"    - {v.name}: {v.doc.splitlines()[0]}")
                else:
                    out.append(f"    - {v.name}")

        if it.methods:
            out.append("  methods:")
            for m in it.methods:
                flags = []
                if m.hidden:
                    flags.append("hidden")
                if m.cfg:
                    flags.append(_fmt_cfg(m.cfg))
                fs = f" [{'; '.join(flags)}]" if flags else ""
                out.append(f"    • {m.signature.strip()}{fs}")
                if m.doc:
                    out.append(_indent(m.doc, "      "))
        out.append("")

    return "\n".join(out).rstrip() + "\n"


def _indent(s: str, prefix: str) -> str:
    return "\n".join(prefix + line for line in s.splitlines())


def format_json(pfs: list[ParsedFile]) -> str:
    def item_to_dict(it: Item) -> dict:
        return {
            "kind": it.kind,
            "name": it.name,
            "signature": it.signature,
            "doc": it.doc,
            "hidden": it.hidden,
            "cfg": it.cfg,
            "variants": [v.__dict__ for v in it.variants],
            "methods": [item_to_dict(m) for m in it.methods],
        }

    payload = [
        {
            "module": pf.module_name,
            "file": str(pf.file_path),
            "cfg": pf.cfg,
            "header_doc": pf.header_doc,
            "items": [item_to_dict(it) for it in pf.items],
        }
        for pf in pfs
    ]
    return json.dumps(payload, indent=2)


# ----------------------------------------------------------------------------
# mdBook catalog generator
#
# Emits one Markdown page per widget into a book sub-directory (default
# `docs/widgets/`), a grouped `index.md`, and patches the auto-generated region
# of `docs/SUMMARY.md`. Each page deep-links to the widget's rustdoc module page
# so the mdBook "discovery" layer and the rustdoc "API reference" layer compose.
# ----------------------------------------------------------------------------


SUMMARY_BEGIN = "<!-- BEGIN GENERATED WIDGETS -->"
SUMMARY_END = "<!-- END GENERATED WIDGETS -->"

# Non-canonical preview densities, as `suffix -> heading label`.
#
# `Compact` is canonical and carries no suffix, so `img/<slug>.png` never
# moves; every other density is additive and lands beside it. This table is
# the Python half of `teksilo_preview::PreviewPass::image_suffix` — the two
# sides never call each other, they only have to agree on the filename, so a
# change to one is a change to both.
DENSITY_IMAGE_SUFFIXES: list[tuple[str, str]] = [
    ("-comfortable", "Comfortable"),
    ("-touch", "Touch"),
]


def _density_image_stem(stem: str) -> str:
    """`button-touch` -> `button`; a stem with no density suffix is itself.

    Used to tell a legitimate per-density variant apart from an orphan when
    reporting stale images.
    """
    for suffix, _ in DENSITY_IMAGE_SUFFIXES:
        if stem.endswith(suffix):
            return stem[: -len(suffix)]
    return stem

# Prepended to every generated page so they satisfy the SPDX pre-commit hook
# (the catalog Markdown is committed). Matches the repo's `.md` header style.
_MD_SPDX_HEADER = [
    "<!-- SPDX-License-Identifier: MPL-2.0 -->",
    "<!-- SPDX-FileCopyrightText: 2026 FernTech -->",
    "",
]

_OVERVIEW = REPO_ROOT / "docs" / "widgets-overview.md"
_OV_SECTION_RE = re.compile(r"^#{2,3}\s+(.+?)(?:\s+[—-].*)?$")
_OV_LINK_RE = re.compile(r"\]\([^)]*crates/teksilo-widgets/src/([^)\s]+?\.rs)[^)]*\)")


def _overview_category_map() -> "tuple[dict[str, str], list[str]]":
    """Parse docs/widgets-overview.md into {src-relative-path -> section} plus the
    section order. This is the single source of truth for catalog grouping, so the
    catalog index matches the hand-maintained overview (data-collection views land
    under "Data-driven widgets", buttons under "Buttons", etc.)."""
    mapping: dict[str, str] = {}
    order: list[str] = []
    if not _OVERVIEW.exists():
        return mapping, order
    cur: str | None = None
    for ln in _OVERVIEW.read_text(encoding="utf-8").splitlines():
        s = ln.strip()
        if s.startswith("#"):
            m = _OV_SECTION_RE.match(s)
            if m:
                name = m.group(1).strip()
                if name.lower() in ("cross-references", "styling status"):
                    cur = None
                else:
                    cur = name
                    if cur not in order:
                        order.append(cur)
            continue
        if cur and s.startswith("- "):
            for lm in _OV_LINK_RE.finditer(ln):
                mapping[lm.group(1)] = cur
    return mapping, order


def _catalog_title(reg: "Registry", pf: ParsedFile) -> str:
    """Human-facing title for a widget page (its primary widget type name)."""
    names = reg.widget_display.get(pf.file_path) or []
    if names:
        return names[0]
    return _stem_to_camel(pf.module_name)


def _catalog_category(fp: Path, overview: "dict[str, str] | None" = None) -> str:
    """Group label for a catalog file. The widget crate prefers the section it
    appears under in widgets-overview.md; other crates group by directory."""
    rel = fp.relative_to(SPEC.src)
    if SPEC.is_widget:
        if overview:
            hit = overview.get(rel.as_posix())
            if hit:
                return hit
        if len(rel.parts) == 1:
            return "Other"  # top-level file not in the overview
        top = rel.parts[0]
        return {
            "primitives": "Layout primitives",
            "animations": "Animations",
        }.get(top, f"{_stem_to_camel(top)} (submodule)")
    # Non-widget crate: top-level files share one group; submodules group by dir.
    if len(rel.parts) == 1:
        return SPEC.group
    return _stem_to_camel(rel.parts[0])


def _build_slugs(parsed: list[ParsedFile]) -> dict[Path, str]:
    """Stable, collision-free page slugs. Top-level files keep their clean stem
    (`button`, `list_model`); a genuine stem collision across directories falls
    back to the dir-prefixed path (`notification_log`)."""
    slugs: dict[Path, str] = {}
    used: set[str] = {"index"}  # reserved for the catalog landing page
    for pf in parsed:
        slug = pf.module_name
        if slug in used:
            rel = pf.file_path.relative_to(SPEC.src).with_suffix("")
            slug = "_".join(rel.parts)
        while slug in used:  # still collides (e.g. a top-level index.rs)
            slug = f"{slug}_"
        used.add(slug)
        slugs[pf.file_path] = slug
    return slugs


def _rustdoc_module_url(
    api_base: "str | None", fp: Path, api_dir: "Path | None" = None
) -> str:
    """rustdoc module-index URL for a catalog file, e.g.
    `button.rs` -> `<base>/teksilo_widgets/button/index.html`.

    `api_base` of `None` means the active crate's docs.rs base, which is the
    committed form; pass `--api-base ../api` to emit book-relative links
    directly instead.

    With a built rustdoc tree (`api_dir`) the URL falls back to the nearest
    ancestor module that actually has a page — covering private `mod`s and
    cfg-gated modules that rustdoc omits. Without it, nested files (other than the
    widget crate's public `primitives/` & `animations/`) link to their top-level
    module as a best-effort guess.
    """
    rel = fp.relative_to(SPEC.src).with_suffix("")
    parts = list(rel.parts)

    base = SPEC.docs_rs if api_base is None else api_base

    def url(ps: list[str]) -> str:
        tail = "/".join([SPEC.rustdoc, *ps, "index.html"])
        return f"{base.rstrip('/')}/{tail}"

    if api_dir is not None:
        cand = list(parts)
        while cand:
            disk = Path(api_dir) / SPEC.rustdoc / Path(*cand) / "index.html"
            if disk.exists():
                return url(cand)
            cand = cand[:-1]
        return url([])

    if len(parts) >= 2 and not (SPEC.is_widget and parts[0] in ("primitives", "animations")):
        parts = parts[:1]
    return url(parts)


def _first_sentence(text: str, limit: int = 160) -> str:
    """First sentence of a module header, for the catalog index brief."""
    for line in text.splitlines():
        s = line.strip()
        if not s or s.startswith("#") or s.startswith("```"):
            continue
        # Stop at the first sentence boundary (". " followed by a capital).
        m = re.search(r"\.(?:\s|$)", s)
        sentence = s[: m.start()] if m else s
        sentence = sentence.strip()
        if len(sentence) > limit:
            sentence = sentence[: limit - 1].rstrip() + "…"
        return sentence
    return ""


# Doc text swept from Rust source carries link targets that don't resolve inside
# the mdBook catalog: rustdoc intra-doc links (`[`X`](crate::..)`, `[`X`](Self::..)`,
# `[`X`](self)`, bare `[`X`](TreeView)`, and the reference form `[`X`]` + `[`X`]:
# crate::X`) and repo-relative file links (`[x](../crates/..)`, `[x](locales/..)`).
# A catalog page is prose plus the single rustdoc-API link we add ourselves, so we
# KEEP only web URLs, in-page anchors, and our own `../api/...` link, and reduce
# every other link to plain inline code.
_INLINE_LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
_REF_DEF_RE = re.compile(r"^(\s*)\[([^\]]+)\]:\s*(\S+).*$")


def _as_code(label: str) -> str:
    label = label.strip()
    if label.startswith("`") and label.endswith("`"):
        return label
    return f"`{label}`"


def _catalog_keep_target(target: str) -> bool:
    """A link target that resolves inside the catalog as-is."""
    t = target.strip()
    return (
        t.startswith("#")
        or t.startswith("../api/")
        # The page's own preview image, written next to it by
        # `teksilo-widgets-previewer --export-docs`.
        or t.startswith("img/")
        or t.startswith("mailto:")
        or "://" in t
    )


def _clean_catalog_links(md: str) -> str:
    """Reduce non-resolvable links in catalog doc text to plain inline code."""
    # Pass 1: drop reference DEFINITIONS we can't keep; remember their labels.
    dropped: set[str] = set()
    kept: list[str] = []
    for ln in md.split("\n"):
        m = _REF_DEF_RE.match(ln)
        if m and not _catalog_keep_target(m.group(3)):
            dropped.add(m.group(2).strip())
            continue
        kept.append(ln)
    md = "\n".join(kept)

    # Pass 2: inline links — keep web/anchor/api, strip the rest to code.
    def repl(m: "re.Match[str]") -> str:
        if _catalog_keep_target(m.group(2)):
            return m.group(0)
        return _as_code(m.group(1))

    md = _INLINE_LINK_RE.sub(repl, md)

    # Pass 3: shortcut / collapsed usages of the dropped labels -> code
    # (but not `[label](...)` inline or `[label][ref]` full-reference forms).
    for lbl in dropped:
        code = _as_code(lbl)
        md = md.replace(f"[{lbl}][]", code)
        md = re.sub(re.escape(f"[{lbl}]") + r"(?![\(\[])", code, md)
    # Pass 4: any remaining rustdoc shortcut link `[`X`]` (a backticked label with
    # no inline target and no reference definition) -> plain code, so the brackets
    # don't leak into the rendered book.
    md = re.sub(r"\[(`[^`\]]+`)\](?![\(\[:])", r"\1", md)
    return md


def _catalog_abilities(pf: ParsedFile, title: str) -> str:
    """A scannable inline list of the primary widget's builder methods."""
    primary = None
    for it in pf.items:
        if it.kind in ("struct", "external") and it.name == title and it.methods:
            primary = it
            break
    if primary is None:
        for it in pf.items:
            if it.methods:
                primary = it
                break
    if primary is None:
        return ""
    names = [
        f"`{m.name}`"
        for m in primary.methods
        if not m.hidden and m.name not in ("new", "default")
    ]
    return ", ".join(names)


def format_catalog_markdown(
    pf: ParsedFile,
    *,
    title: str,
    slug: str,
    api_base: "str | None",
    img_dir: Path,
    api_dir: "Path | None" = None,
) -> str:
    """Render one widget's mdBook catalog page."""
    out: list[str] = list(_MD_SPDX_HEADER)
    out.append(f"# {title}")
    out.append("")

    if (img_dir / f"{slug}.png").exists():
        out.append(f"![{title} preview](img/{slug}.png)")
        out.append("")

    if pf.cfg:
        out.append(f"> Available under: `{_fmt_cfg(pf.cfg)}`")
        out.append("")

    if pf.header_doc:
        out.append(pf.header_doc.rstrip())
        out.append("")

    density_images = [
        (suffix, label)
        for suffix, label in DENSITY_IMAGE_SUFFIXES
        if (img_dir / f"{slug}{suffix}.png").exists()
    ]
    if density_images:
        out.append("## Density")
        out.append("")
        out.append(
            "The picture above is the widget at `TargetDensity::Compact`, the "
            "mouse-and-keyboard ladder. Below is the same subject on the same "
            "canvas with only the ladder changed, so what moves is the density "
            "and nothing else — where the subject no longer fits, that is what "
            "the denser targets cost it at that size. "
            "See `docs/density-and-targets.md`."
        )
        out.append("")
        for suffix, label in density_images:
            out.append(f"**{label}**")
            out.append("")
            out.append(f"![{title} at {label} density](img/{slug}{suffix}.png)")
            out.append("")

    abilities = _catalog_abilities(pf, title)
    if abilities:
        out.append("## Builder methods at a glance")
        out.append("")
        out.append(abilities)
        out.append("")

    out.append("## API reference")
    out.append("")
    out.append(
        f"📖 [Full rustdoc API for this module]"
        f"({_rustdoc_module_url(api_base, pf.file_path, api_dir)})"
    )
    out.append("")
    _emit_items_md(pf.items, out)

    # The module header + item docs were swept from rustdoc-style source, so they
    # carry links that don't resolve in the book — reduce them to plain code.
    return _clean_catalog_links("\n".join(out).rstrip()) + "\n"


def format_catalog_index(
    reg: "Registry", parsed: list[ParsedFile], slugs: dict[Path, str]
) -> str:
    """The catalog landing page: every widget grouped by category, with a brief."""
    overview, ov_order = _overview_category_map()
    groups: dict[str, list[tuple[str, str, str]]] = {}
    for pf in parsed:
        cat = _catalog_category(pf.file_path, overview)
        title = _catalog_title(reg, pf)
        brief = _clean_catalog_links(_first_sentence(pf.header_doc))
        groups.setdefault(cat, []).append((title, slugs[pf.file_path], brief))

    # Order by the overview's section order, then any remaining groups (submodule
    # helpers, uncategorised) alphabetically.
    ordered = [c for c in ov_order if c in groups]
    ordered += sorted(c for c in groups if c not in ordered)

    out: list[str] = list(_MD_SPDX_HEADER)
    out.append(f"# {SPEC.title}")
    out.append("")
    noun = "widget" if SPEC.is_widget else "type"
    out.append(
        f"Every public {noun} in `{SPEC.crate}`, grouped by category. Each page "
        "links to its full rustdoc API reference."
    )
    out.append("")
    for cat in ordered:
        out.append(f"## {cat}")
        out.append("")
        for title, slug, brief in sorted(groups[cat], key=lambda t: t[0].lower()):
            line = f"- [{title}]({slug}.md)"
            if brief:
                line += f" — {brief}"
            out.append(line)
        out.append("")
    return "\n".join(out).rstrip() + "\n"


def _summary_block(
    reg: "Registry", parsed: list[ParsedFile], slugs: dict[Path, str], md_subdir: str
) -> str:
    """The auto-generated `docs/SUMMARY.md` region for the active crate: an
    `Overview` link followed by one alphabetically-sorted chapter per type (flat,
    so the static `# <title>` part header in SUMMARY.md groups them)."""
    lines = [SPEC.marker_begin, f"- [Overview]({md_subdir}/index.md)"]
    for pf in sorted(parsed, key=lambda p: _catalog_title(reg, p).lower()):
        title = _catalog_title(reg, pf)
        lines.append(f"- [{title}]({md_subdir}/{slugs[pf.file_path]}.md)")
    lines.append(SPEC.marker_end)
    return "\n".join(lines)


def book_subdir(out_dir: Path, book_src: Path) -> "str | None":
    """`out_dir` as a book-relative posix path, or `None` when it is outside
    the book source.

    `SUMMARY.md` is the book's table of contents and every link in it resolves
    against the book source root (`docs/`, per `book.toml`'s `src`). A run that
    writes its pages anywhere else — a scratch directory used to diff a
    regeneration, say — has produced nothing the book contains, so patching
    `SUMMARY.md` from it can only point chapters at a directory that is not
    there. `mdbook build` is a pre-commit gate, so the next person to run it
    fails for a reason that has nothing to do with their change.

    This replaced an `out_dir.name` fallback, which made that silent and
    plausible: a run into `/tmp/gen` rewrote a whole generated region to
    `- [Overview](gen/index.md)` and twenty-one sibling links, all valid-looking
    and all dead.

    Resolves both sides first, so a symlinked scratch path or a relative
    `--md-dir` is judged by where it actually lands.
    """
    try:
        return out_dir.resolve().relative_to(book_src.resolve()).as_posix()
    except (ValueError, OSError):
        return None


def patch_summary(summary_path: Path, block: str, begin: str, end: str) -> bool:
    """Replace the `begin`..`end` marked region of SUMMARY.md with `block`.
    Returns False if the markers are absent (caller then prints guidance)."""
    text = summary_path.read_text(encoding="utf-8")
    if begin not in text or end not in text:
        return False
    pre = text[: text.index(begin)]
    post = text[text.index(end) + len(end):]
    summary_path.write_text(pre + block + post, encoding="utf-8")
    return True


def merge_submodule_items(reg: "Registry", pf: ParsedFile) -> ParsedFile:
    """Fold a widget's submodule types into its own page.

    A widget laid out as `foo.rs` + `foo/` keeps helpers in the directory, and
    those files get no page of their own (they carry no `impl Widget`). But some
    of what lives there is genuinely public API re-exported from `lib.rs` —
    `segmented_control/id.rs`'s `SegmentId`, `tab_widget/id.rs`'s `TabId`. Those
    belong on the parent widget's page rather than nowhere at all.

    Only re-exported names are merged, so internal helpers stay out, and a type
    the parent file already documents is never duplicated.
    """
    sub_dir = pf.file_path.parent / pf.file_path.stem
    if not sub_dir.is_dir():
        return pf

    have = {item.name for item in pf.items}
    extra: list[Item] = []
    for sub in sorted(sub_dir.glob("*.rs")):
        if sub.name in SKIP_FILES or _is_test_file(sub):
            continue
        # A submodule that earns its own page (it declares a re-exported
        # `impl Widget` type, like `tab_widget/bar.rs`'s `TabBar`) is
        # documented there — merging it here too would duplicate it.
        if reg.widget_display.get(sub):
            continue
        sub_pf = parse_file(sub, sub.stem, reg.cfg_by_file.get(sub.resolve(), []))
        for item in sub_pf.items:
            if item.name in have or item.name not in reg.exported:
                continue
            have.add(item.name)
            extra.append(item)

    if not extra:
        return pf
    return ParsedFile(
        module_name=pf.module_name,
        file_path=pf.file_path,
        header_doc=pf.header_doc,
        cfg=pf.cfg,
        items=pf.items + extra,
    )


# Every generated page carries this line; `index.md` does not, and neither does
# a hand-written guide. `_prune_stale_pages` requires it before deleting, so
# pointing `--md-dir` at a directory of notes cannot destroy them.
_PAGE_SIGNATURE = "Full rustdoc API for this module"


def _prune_stale_pages(out_dir: Path, written: "set[str]") -> "list[str]":
    """Delete catalog pages whose source module no longer exists.

    The generator rewrites every page on every run, so a `*.md` it did not just
    write documents something that has gone away. `popover.md` outlived the
    split of `popover.rs` into three modules by a month, unreferenced by
    `SUMMARY.md` and by `index.md`, still carrying a deep link to a rustdoc
    module that no longer exists.

    Deliberately narrow. It globs `*.md` at the top level of `out_dir` only:
    NOT recursive, and no other suffix, so the preview images under
    `out_dir/img/` are never traversed and never removed. Those are produced by
    `teksilo-widgets-previewer --export-docs`, cost a GPU to rebuild, and are
    not this tool's to delete; stale ones are reported instead.
    """
    removed = []
    for md in sorted(out_dir.glob("*.md")):
        if md.name in written:
            continue
        try:
            if _PAGE_SIGNATURE not in md.read_text(encoding="utf-8"):
                continue
        except (OSError, UnicodeDecodeError):
            continue
        md.unlink()
        removed.append(md.name)
    return removed


def cmd_md_dir(
    reg: "Registry",
    md_dir: str,
    api_base: "str | None",
    api_dir: "Path | None" = None,
) -> int:
    out_dir = Path(md_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    img_dir = out_dir / "img"

    # Only the curated set (same files `--list` shows: a non-empty `widget_display`
    # — i.e. files carrying a public widget / re-exported type). Internal helpers
    # and impl-split modules carry none, so they get no catalog page.
    target = [fp for fp in reg.files if reg.widget_display.get(fp)]
    parsed = [
        merge_submodule_items(
            reg, parse_file(fp, fp.stem, reg.cfg_by_file.get(fp.resolve(), []))
        )
        for fp in target
    ]
    slugs = _build_slugs(parsed)

    for pf in parsed:
        slug = slugs[pf.file_path]
        page = format_catalog_markdown(
            pf,
            title=_catalog_title(reg, pf),
            slug=slug,
            api_base=api_base,
            img_dir=img_dir,
            api_dir=api_dir,
        )
        (out_dir / f"{slug}.md").write_text(page, encoding="utf-8")
    (out_dir / "index.md").write_text(
        format_catalog_index(reg, parsed, slugs), encoding="utf-8"
    )

    removed = _prune_stale_pages(
        out_dir, {f"{s}.md" for s in slugs.values()} | {"index.md"}
    )
    # Reported, never deleted: `img/` belongs to the previewer's --export-docs.
    live_slugs = set(slugs.values())
    stale_img = (
        sorted(
            q.name
            for q in img_dir.glob("*.png")
            if _density_image_stem(q.stem) not in live_slugs
        )
        if img_dir.is_dir()
        else []
    )

    book_src = REPO_ROOT / "docs"
    md_subdir = book_subdir(out_dir, book_src)
    summary = book_src / "SUMMARY.md"
    if md_subdir is None:
        # Outside the book: the pages are not chapters, so the table of
        # contents is left exactly as it was. See `book_subdir`.
        note = f"SUMMARY.md untouched — {out_dir} is outside the book source ({book_src})"
    elif summary.exists() and patch_summary(
        summary, _summary_block(reg, parsed, slugs, md_subdir), SPEC.marker_begin, SPEC.marker_end
    ):
        note = "patched docs/SUMMARY.md"
    else:
        note = (
            f"SUMMARY markers not found — add this region to docs/SUMMARY.md:\n"
            f"{SPEC.marker_begin}\n{SPEC.marker_end}"
        )
    print(
        f"Wrote {len(parsed)} catalog pages + index.md to {out_dir} ({note})",
        file=sys.stderr,
    )
    if removed:
        print(
            f"  pruned {len(removed)} stale page(s): {', '.join(removed)}",
            file=sys.stderr,
        )
    if stale_img:
        print(
            f"  {len(stale_img)} image(s) with no page, left in place for "
            f"--export-docs to reconcile: {', '.join(stale_img)}",
            file=sys.stderr,
        )
    return 0


def _test_prune() -> None:
    """`_prune_stale_pages` removes dead pages and nothing else."""
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        img = d / "img"
        img.mkdir()
        page = f"# X\n\n{_PAGE_SIGNATURE} link\n"
        (d / "button.md").write_text(page)      # written this run -> keep
        (d / "popover.md").write_text(page)     # generated, orphaned -> remove
        (d / "index.md").write_text("# Index")  # always rewritten -> keep
        (d / "notes.md").write_text("hand-written, no signature")
        (img / "button.png").write_bytes(b"\x89PNG")
        (img / "popover.png").write_bytes(b"\x89PNG")
        (d / "stray.txt").write_text("not markdown")
        # A signature-bearing .md one level down. Only a recursive walk would
        # reach it, so this is what pins the glob as top-level-only.
        (img / "nested.md").write_text(page)

        removed = _prune_stale_pages(d, {"button.md", "index.md"})

        assert removed == ["popover.md"], removed
        assert not (d / "popover.md").exists()
        # Everything else survives: a written page, the index, a file with no
        # generated signature, a non-.md file, and BOTH images. img/ is not
        # traversed at all, so even the image of the pruned page is untouched.
        for name in ("button.md", "index.md", "notes.md", "stray.txt"):
            assert (d / name).exists(), name
        assert (img / "button.png").exists()
        assert (img / "popover.png").exists(), "prune must never reach into img/"
        assert (img / "nested.md").exists(), "prune must not recurse below out_dir"


def _test_density_images() -> None:
    """A Compact image opens the page; a denser one adds a Density section.

    And a per-density variant is never mistaken for an orphan — that is the
    difference between `--export-docs --density=touch` producing artifacts
    the next catalog run reports as stale, and it just working.
    """
    import tempfile

    reg = build_registry()
    fp = reg.module_to_file["button"]
    pf = parse_file(fp, fp.stem, reg.cfg_by_file.get(fp.resolve(), []))

    with tempfile.TemporaryDirectory() as td:
        img = Path(td)
        (img / "button.png").write_bytes(b"\x89PNG")

        page = format_catalog_markdown(
            pf, title="Button", slug="button", api_base="../api", img_dir=img
        )
        assert "![Button preview](img/button.png)" in page
        assert "## Density" not in page, "no denser image exists yet"

        (img / "button-touch.png").write_bytes(b"\x89PNG")
        page = format_catalog_markdown(
            pf, title="Button", slug="button", api_base="../api", img_dir=img
        )
        assert "## Density" in page, "a -touch image must earn a Density section"
        assert "![Button at Touch density](img/button-touch.png)" in page
        # The canonical image still opens the page, unmoved.
        assert "![Button preview](img/button.png)" in page

    # The orphan rule, at the level the stale report applies it.
    assert _density_image_stem("button-touch") == "button"
    assert _density_image_stem("button-comfortable") == "button"
    assert _density_image_stem("button") == "button"
    # A slug that merely ends in a density word is not a variant of anything.
    assert _density_image_stem("touch_target") == "touch_target"


def _test_summary_scope() -> None:
    """`--md-dir` patches `SUMMARY.md` only for an output directory inside the
    book, and leaves the repo's own table of contents alone otherwise."""
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        book = Path(td) / "docs"
        (book / "scene").mkdir(parents=True)
        (book / "widgets" / "extra").mkdir(parents=True)
        assert book_subdir(book / "scene", book) == "scene", "in-book dir rejected"
        assert book_subdir(book / "widgets" / "extra", book) == "widgets/extra", "nested dir"
        assert book_subdir(Path(td) / "gen", book) is None, "scratch dir accepted"
        assert book_subdir(Path(td), book) is None, "the book's own parent accepted"

    # End to end, against the real tree: a regeneration into a scratch
    # directory must leave `docs/SUMMARY.md` byte-identical. This is the
    # failure the rule exists for — a diff run that quietly rewrote the
    # generated region to links the book does not contain, and an `mdbook
    # build` gate that then failed for someone else's change.
    reg = build_registry()
    summary = REPO_ROOT / "docs" / "SUMMARY.md"
    before = summary.read_text(encoding="utf-8")
    with tempfile.TemporaryDirectory() as td:
        cmd_md_dir(reg, str(Path(td) / "gen"), None, None)
    assert summary.read_text(encoding="utf-8") == before, (
        "a --md-dir run outside docs/ rewrote docs/SUMMARY.md"
    )


def run_self_tests() -> int:
    """Smoke tests for the catalog generator (`--test`). stdlib-only, runs
    against the live source tree."""
    import tempfile

    _test_prune()
    _test_density_images()
    _test_summary_scope()
    reg = build_registry()
    fp = reg.module_to_file["button"]
    pf = parse_file(fp, fp.stem, reg.cfg_by_file.get(fp.resolve(), []))

    page = format_catalog_markdown(
        pf, title="Button", slug="button", api_base="../api", img_dir=Path("/nonexistent")
    )
    assert page.startswith("<!-- SPDX-License-Identifier"), page[:40]
    assert "\n# Button\n" in page, page[:80]
    assert "## API reference" in page, "missing API reference section"
    assert "Full rustdoc API" in page, "missing rustdoc deep link"
    assert "teksilo_widgets/button/index.html" in page, "wrong rustdoc url"

    slugs = _build_slugs([pf])
    idx = format_catalog_index(reg, [pf], slugs)
    assert "\n# Widget Catalog\n" in idx, idx[:80]
    assert "[Button](button.md)" in idx, "index missing button link"

    # SUMMARY marker round-trip.
    with tempfile.TemporaryDirectory() as td:
        sp = Path(td) / "SUMMARY.md"
        sp.write_text(
            f"# Index\n\n{SUMMARY_BEGIN}\nstale\n{SUMMARY_END}\n\n# Tail\n",
            encoding="utf-8",
        )
        block = _summary_block(reg, [pf], slugs, "widgets")
        assert patch_summary(sp, block, SUMMARY_BEGIN, SUMMARY_END), "patch returned False"
        result = sp.read_text(encoding="utf-8")
        assert "stale" not in result, "stale content survived"
        assert "[Overview](widgets/index.md)" in result, "catalog overview missing"
        assert "[Button](widgets/button.md)" in result, "widget chapter missing"
        assert "# Index" in result and "# Tail" in result, "surrounding text clobbered"

    # Slug collision fallback.
    # Default base is docs.rs, the form that resolves on GitHub; fix_book_links
    # rewrites it to ../api/ for the book.
    assert _rustdoc_module_url(None, WIDGETS_SRC / "button.rs") == (
        "https://docs.rs/teksilo-widgets/latest/teksilo_widgets/button/index.html"
    ), _rustdoc_module_url(None, WIDGETS_SRC / "button.rs")
    assert _rustdoc_module_url("../api", PRIMITIVES_DIR / "hstack.rs").endswith(
        "teksilo_widgets/primitives/hstack/index.html"
    ), "rustdoc url for nested module wrong"
    # Nested per-widget submodule -> public parent module.
    assert _rustdoc_module_url("../api", WIDGETS_SRC / "tab_widget" / "bar.rs").endswith(
        "teksilo_widgets/tab_widget/index.html"
    ), "rustdoc url for private submodule should fall back to parent"

    # Catalog link cleaning: keep web/anchor/api, strip rustdoc + file links to code.
    nz = _clean_catalog_links(
        "[`HStack`](crate::primitives::HStack), [a](Self::alignment), [b](self), "
        "[c](TreeView), [d](../crates/x.rs), [api](../api/x.html), [web](https://x.io)\n"
        "see [`Ref`].\n\n[`Ref`]: crate::Ref"
    )
    for bad in ("](crate::", "](Self::", "](self)", "](TreeView)", "](../crates/", "[`Ref`]:"):
        assert bad not in nz, f"{bad} survived: {nz}"
    assert "`HStack`" in nz and "see `Ref`." in nz, nz
    assert "](../api/x.html)" in nz and "](https://x.io)" in nz, nz

    # Non-widget crate generalization (teksilo-data): re-exported types become
    # catalog entries, rustdoc links target the crate's own rustdoc dir.
    global SPEC
    _prev = SPEC
    try:
        SPEC = CRATE_SPECS["data"]
        dreg = build_registry()
        dfp = dreg.module_to_file["list_model"]
        assert dreg.widget_display.get(dfp), "list_model.rs should be a catalog entry"
        dpf = parse_file(dfp, dfp.stem, dreg.cfg_by_file.get(dfp.resolve(), []))
        dpage = format_catalog_markdown(
            dpf, title="ListModel", slug="list_model", api_base="../api",
            img_dir=Path("/nonexistent"),
        )
        assert "teksilo_data/list_model/index.html" in dpage, dpage[:400]
        didx = format_catalog_index(dreg, [dpf], _build_slugs([dpf]))
        assert "\n# Data Collections\n" in didx, didx[:80]
    finally:
        SPEC = _prev

    # Step 9: a queryable-but-not-cataloged crate, end to end. `core` has
    # catalog=False (it is not one of the four mdBook-catalogued crates) but
    # must still resolve/build/extract exactly like a cataloged one.
    _prev = SPEC
    try:
        SPEC = CRATE_SPECS["core"]
        assert not SPEC.catalog, "'core' must stay catalog=False for this to test the right thing"
        creg = build_registry()
        cfp = creg.type_to_file.get("theme")
        assert cfp is not None, "Theme should be resolvable by name in teksilo-core"
        cpf = parse_file(cfp, cfp.stem, creg.cfg_by_file.get(cfp.resolve(), []))
        assert any(item.name == "Theme" for item in cpf.items), (
            "Theme struct not extracted from teksilo-core"
        )
    finally:
        SPEC = _prev

    # `--catalog-all` must keep iterating exactly the four originally-cataloged
    # crates — every crate added to CRATE_SPECS above must never grow docs/ output.
    catalog_keys = {k for k, s in CRATE_SPECS.items() if s.catalog}
    assert catalog_keys == {"widgets", "data", "settings", "scene"}, (
        f"--catalog-all must visit exactly the four cataloged crates, got {catalog_keys}"
    )

    # UMBRELLA_REEXPORTS must only ever point at a live CRATE_SPECS key — a
    # renamed/removed table row would otherwise silently stop redirecting.
    for _name, _key in UMBRELLA_REEXPORTS.items():
        assert _key in CRATE_SPECS, f"UMBRELLA_REEXPORTS[{_name!r}] -> unknown crate key {_key!r}"

    # End-to-end umbrella redirect: `Theme` isn't in teksilo-widgets, but is
    # reachable via `teksilo::prelude::Theme` (owned by teksilo-core). A
    # lookup against the default 'widgets' registry should fall through.
    assert "theme" not in reg.type_to_file, "test assumption: Theme is not itself in teksilo-widgets"
    redirected_fp, used_key, _ = _resolve_across_crates(reg, "widgets", "Theme")
    assert used_key == "core" and redirected_fp is not None, (
        "umbrella redirect for 'Theme' should land on teksilo-core"
    )

    # The umbrella table stays AUTHORITATIVE over the crate sweep. 'Theme' is
    # defined in teksilo-core, teksilo-tokens and teksilo-inspector; only the
    # table knows which one `teksilo::prelude::Theme` is. If the sweep ever
    # runs first, this lands on whichever crate CRATE_SPECS declares earliest.
    assert len(_crate_owners("Theme")) > 1, (
        "test assumption: 'Theme' is defined in more than one crate"
    )
    assert UMBRELLA_REEXPORTS.get("Theme") == "core", (
        "test assumption: the table pins Theme to teksilo-core"
    )

    # Cross-crate sweep: a peer-crate type reachable as `teksilo::data::…` but
    # present in NO prelude, and therefore in no UMBRELLA_REEXPORTS row. The
    # skill advertises `cargo teksilo symbol ListModel`; before the sweep that
    # answered "unknown widget 'ListModel'" with three teksilo-widgets
    # suggestions, which reads to an agent as "this type does not exist".
    assert "ListModel" not in UMBRELLA_REEXPORTS, (
        "test assumption: ListModel is not a prelude re-export"
    )
    swept_fp, swept_key, swept_reg = _resolve_across_crates(reg, "widgets", "ListModel")
    assert swept_key == "data" and swept_fp is not None, (
        f"crate sweep for 'ListModel' should land on teksilo-data, got {swept_key!r}"
    )
    assert swept_reg is not reg, "sweep must return the OWNING crate's registry, not the caller's"

    # The other peer-crate types the skill names in the same breath.
    for _name, _expect in (
        ("TreeSlice", "data"),
        ("SelectionModel", "data"),
        ("MruList", "settings"),
        ("SceneCard", "scene"),
    ):
        _fp, _key, _ = _resolve_across_crates(reg, "widgets", _name)
        assert _fp is not None and _key == _expect, (
            f"{_name} should resolve to --crate {_expect}, got {_key!r}"
        )

    # A genuine typo draws its hints from every crate, tagged with the flag
    # that reaches each — the whole point is that the hint names `--crate`.
    _hints = _cross_crate_hints("LstModel", "widgets")
    assert any("--crate data" in h for h in _hints), (
        f"typo hints should reach teksilo-data, got {_hints}"
    )

    # A name in no crate at all still fails, and fails loudly.
    _none_fp, _, _ = _resolve_across_crates(reg, "widgets", "NoSuchTypeAnywhere")
    assert _none_fp is None, "the sweep must not invent a resolution"

    print("extract_widget_api.py self-tests passed.", file=sys.stderr)
    return 0


# ----------------------------------------------------------------------------
# CLI
# ----------------------------------------------------------------------------


def cmd_list(reg: Registry) -> int:
    # Show exported type names grouped by file.
    rows: list[tuple[str, str]] = []
    for fp, names in sorted(reg.widget_display.items(), key=lambda kv: kv[0].name):
        if not names:
            continue
        rel = fp.relative_to(REPO_ROOT) if REPO_ROOT in fp.parents else fp
        cfg = reg.cfg_by_file.get(fp.resolve(), [])
        cfg_s = f"  {{{_fmt_cfg(cfg)}}}" if cfg else ""
        rows.append((fp.stem, f"  {', '.join(names)}  ({rel}){cfg_s}"))

    if not rows:
        print("No catalog files found.", file=sys.stderr)
        return 1

    print(f"{len(rows)} files under {SPEC.src.relative_to(REPO_ROOT)}:\n")
    for stem, body in sorted(rows):
        print(f"{stem}:")
        print(body)
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Extract public API + docs from teksilo-widgets source files."
        ),
    )
    parser.add_argument(
        "widgets",
        nargs="*",
        help="Widget names (type or module name, case-insensitive). "
        "e.g. Button HStack Dialog",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="Extract every widget file.",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="List available widgets and exit.",
    )
    parser.add_argument(
        "--format",
        "-f",
        choices=("md", "text", "json"),
        default="md",
        help="Output format (default: md).",
    )
    parser.add_argument(
        "--output",
        "-o",
        help="Write output to this file instead of stdout.",
    )
    parser.add_argument(
        "--md-dir",
        metavar="DIR",
        help="Generate one mdBook catalog page per widget into DIR "
        "(e.g. docs/widgets), plus index.md, and patch the generated region of "
        "docs/SUMMARY.md — only when DIR is inside docs/, so a scratch-directory "
        "run leaves the book's table of contents alone. Ignores positional widget "
        "names (always emits all).",
    )
    parser.add_argument(
        "--api-base",
        default=None,
        help="Base URL/path for the rustdoc API links in catalog pages. "
        "Default: the crate's docs.rs base, so a committed page resolves when "
        "read on GitHub; fix_book_links.py repoints it at the book's own "
        "/api/ tree at build time. Pass ../api to emit that directly.",
    )
    parser.add_argument(
        "--api-dir",
        metavar="DIR",
        help="Path to the built rustdoc tree (e.g. target/doc). When given, each "
        "catalog page's API link falls back to the nearest module that actually "
        "has a page, so private/cfg-gated modules don't 404.",
    )
    parser.add_argument(
        "--crate",
        choices=list(CRATE_SPECS),
        default="widgets",
        help="Which crate to extract / catalog (default: widgets).",
    )
    parser.add_argument(
        "--catalog-all",
        action="store_true",
        help="Generate the mdBook catalog for ALL crates into their default "
        "docs/<dir> and patch each SUMMARY region.",
    )
    parser.add_argument(
        "--test",
        action="store_true",
        help=argparse.SUPPRESS,  # run the catalog-generator smoke tests and exit
    )
    args = parser.parse_args(argv)

    if args.test:
        return run_self_tests()

    global SPEC

    def _api_dir() -> "Path | None":
        # Auto-use the built rustdoc tree if present, so a plain run still
        # resolves deep-links for private / cfg-gated modules instead of 404ing.
        if args.api_dir:
            return Path(args.api_dir)
        _doc = REPO_ROOT / "target" / "doc"
        return _doc if (_doc / SPEC.rustdoc).exists() else None

    if args.catalog_all:
        rc = 0
        for key, spec in CRATE_SPECS.items():
            if not spec.catalog:
                continue
            SPEC = spec
            reg = build_registry()
            out = REPO_ROOT / "docs" / spec.md_subdir
            rc |= cmd_md_dir(reg, str(out), args.api_base, _api_dir())
        return rc

    SPEC = CRATE_SPECS[args.crate]
    reg = build_registry()

    if args.list:
        return cmd_list(reg)

    if args.md_dir:
        if not SPEC.catalog:
            print(
                f"error: '{args.crate}' ({SPEC.crate}) is queryable via "
                "--list / --all / <Name> but is not part of the mdBook "
                "catalog — CRATE_SPECS marks it catalog=False. Refusing to "
                "write pages for it; flip `catalog=True` in CRATE_SPECS "
                "first if this crate should join the book.",
                file=sys.stderr,
            )
            return 2
        return cmd_md_dir(reg, args.md_dir, args.api_base, _api_dir())

    file_cfg: dict[Path, list[str]] = {}
    if args.all:
        target_files = list(reg.files)
        for fp in target_files:
            file_cfg[fp] = reg.cfg_by_file.get(fp.resolve(), [])
    elif args.widgets:
        target_files = []
        seen: set[Path] = set()
        for name in args.widgets:
            fp, used_key, used_reg = _resolve_across_crates(reg, args.crate, name)
            if fp is None:
                hints = _cross_crate_hints(name, args.crate)
                hint_str = (
                    f" Did you mean: {', '.join(hints)}?" if hints else ""
                )
                print(
                    f"error: unknown type '{name}' in any teksilo crate."
                    f"{hint_str}",
                    file=sys.stderr,
                )
                return 2
            if used_key != args.crate:
                via = (
                    "the teksilo umbrella prelude"
                    if UMBRELLA_REEXPORTS.get(name) == used_key
                    else "a crate sweep"
                )
                note = (
                    f"note: '{name}' isn't in {SPEC.crate}; resolved via "
                    f"{via} to {CRATE_SPECS[used_key].crate} "
                    f"(--crate {used_key})."
                )
                others = [
                    k
                    for k in _crate_owners(name)
                    if k not in (used_key, args.crate)
                ]
                if others:
                    note += " Also defined in: " + ", ".join(
                        f"{CRATE_SPECS[k].crate} (--crate {k})" for k in others
                    ) + "."
                print(note, file=sys.stderr)
            if fp not in seen:
                seen.add(fp)
                target_files.append(fp)
                file_cfg[fp] = used_reg.cfg_by_file.get(fp.resolve(), [])
    else:
        parser.print_help(sys.stderr)
        print(
            "\nPass one or more widget names, --all, or --list.",
            file=sys.stderr,
        )
        return 2

    parsed: list[ParsedFile] = []
    for fp in target_files:
        pf = parse_file(fp, fp.stem, file_cfg.get(fp, []))
        parsed.append(pf)

    if args.format == "json":
        rendered = format_json(parsed) + "\n"
    elif args.format == "text":
        rendered = "\n".join(format_text(pf) for pf in parsed)
    else:
        rendered = "\n---\n\n".join(format_markdown(pf) for pf in parsed)

    if args.output:
        Path(args.output).write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
