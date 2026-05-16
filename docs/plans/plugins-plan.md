# Plugin System Plan

A runtime-pluggable extension system for FernUI apps. Two interpreter
backends — **WASM component-model (sandboxed)** and **embedded CPython
via PyO3 (trusted)** — sitting under one common bundle format, manifest
schema, installer, manager UI, and contribution model. App authors
opt into one or both runtimes; plugin authors declare which they
target in the manifest. Most plugins never construct UI imperatively —
they ship declarative *contribution specs* the host renders.

## Context

FernUI today has no plugin system. Every app is a closed Rust binary:
extending it means forking, recompiling, redistributing. The
architecture is already plugin-friendly without realising it — the
`Widget` trait is dyn-compatible, intents and shortcuts are name-keyed
strings, style traits are `Rc<dyn FooStyle>`, the arena holds
`Box<dyn Widget>`, and `AppEventPoster::post_external` (built for file
dialogs) already gives us a thread-safe event-injection seam. What's
missing is the runtime, the boundary, and the host-side scaffolding
that turns "the framework could host plugins" into "this app hosts
plugins."

The target audience for this work is the FernTech-internal novelist
IDE — a Scrivener-class writing tool — but the framework surface is
designed so any FernUI app can opt in. The novelist IDE pulls writers,
hobbyist plugin authors, and small commercial extension shops; it
needs a plugin ecosystem that *can exist*, which constrains the
runtime choice as much as the security model does.

### Reference reading

- WebAssembly Component Model —
  [component-model.bytecodealliance.org](https://component-model.bytecodealliance.org).
  The wit/wit-bindgen toolchain we use for the sandboxed runtime.
- `wasmtime` crate —
  [docs.rs/wasmtime](https://docs.rs/wasmtime). Host embedding of
  the WASM component model.
- `pyo3` crate — [docs.rs/pyo3](https://docs.rs/pyo3). Rust ↔ CPython
  bridge for the trusted runtime.
- Blender Python API —
  [docs.blender.org/api](https://docs.blender.org/api/current/).
  The reference for "creative tool with embedded CPython, decade of
  ecosystem, signed marketplace, install-consent dialog."
- Sublime Text plugin API —
  [www.sublimetext.com/docs/api_reference.html](https://www.sublimetext.com/docs/api_reference.html).
  Same model, different domain — text-editing decorations and
  command palettes via Python.
- VSCode WebExtensions / WASM future direction — VSCode's
  contribution-point model and the in-progress shift to WASM
  components for untrusted extensions. The contribution-point shape
  (declarative manifest entries the host renders) is what our
  Tier-1 contributions emulate.
- Zed Extensions —
  [zed.dev/docs/extensions](https://zed.dev/docs/extensions).
  Sandboxed WASM-component plugin model in a production native app.

## Design targets

Settled through the design conversation. Listed flat so each is
independently checkable.

1. **Dual runtime, single common shell.** App authors enable one or
   both of the **WASM/sandboxed** and **Python/trusted** runtimes;
   plugin authors declare which they target in their manifest. The
   bundle format, installer, manifest schema, plugin manager UI,
   consent dialog, contribution registry, slot widgets, scope
   orchestration, settings namespacing, i18n bundle loading,
   intent dispatch, lifecycle, signature verification, and update
   mechanism are all **shared and runtime-agnostic**.
2. **Tier-1 declarative contributions cover the 80% case.** Plugin
   manifests declare contributions (status segments, settings pages,
   list panels, wizards) as typed data; the host renders them
   with full native theming / a11y / i18n / decoration support.
   No runtime UI calls needed for these. Cross-runtime by
   construction.
3. **Tier-2 freeform UI is the escape hatch.** Plugins that need to
   build novel widget trees (visualizations, custom layouts) opt
   in to runtime-specific imperative UI: a `wit` interface for
   WASM plugins, a Python builder DSL for Python plugins.
   Verbose but possible.
4. **Per-window vs shared scope, orthogonal to trust.** Same
   manifest field (`scope.instance = "per-window" | "shared"`)
   regardless of runtime. Default per-window. Shared plugins
   declare it explicitly and get louder consent ("this plugin
   sees data across all your projects").
5. **Plugin ID and slot ID are independent.** The plugin's `id`
   is reverse-DNS-named after its **creator** (e.g.
   `ai.mistral.grammar-helper`), not after the app it targets.
   Each contribution declares its own `slot` field as a local
   name (`dock.right`, `status.trailing`, `toolbar.trailing`),
   and the plugin's separate `target_app` field qualifies which
   app it's for. The framework checks `target_app` at install
   time to prevent a plugin built for app A from being installed
   in app B. Slot widgets on the app side declare just the local
   slot name; the framework qualifies routing internally.
6. **All-at-install capability prompts.** No runtime escalation.
   The user sees the full capability set at install time and
   either accepts or rejects. Plugin authors who need additional
   capabilities ship a new version, requiring re-consent. Same
   model as Android pre-M / browser-extension install.
7. **Capabilities are gates for sandboxed plugins, declarations
   for trusted plugins.** Same schema. For sandboxed: the runtime
   physically prevents undeclared actions. For trusted: declared
   capabilities are informational + auditable + inputs to
   best-effort OS sandboxing (macOS App Sandbox path scoping,
   Linux seccomp). The plugin manager UI marks this clearly
   (green padlock for sandboxed, yellow warning for trusted).
8. **Scope migration and trust-level migration are both treated
   as new plugins.** v1 ships sandboxed, v2 wants trusted →
   forced reinstall + fresh consent. v1 ships per-window, v2
   wants shared → same. No silent escalation. Same precedent as
   Android permission group changes.
9. **Additive-only minor versions of the wit interface.**
   Host supports any `0.x` plugin where `x ≤ host's x`. Breaking
   changes bump major and ship side-by-side support periods.
   Trusted-runtime Python API follows semver with the same rule.
10. **App-defined testing strategy, framework-provided hot-reload
    primitive.** Framework ships a dev-mode hot-reload path that
    skips consent dialogs and re-loads plugin artifacts on file
    change. Plugin authors choose their own mock/fixture
    strategy on top.
11. **Decoration system is out of scope, deferred to its own
    plan.** Editor decorations (squigglies, gutter marks, inline
    suggestions) require modifications to `fern-widgets`
    (`RichTextEditor::decoration_source`) and a separate
    contribution shape that doesn't fit slot widgets. Tracked
    separately so this plan stays focused on the widget /
    action / settings / panel contribution surface.
12. **Minimal touch to `fern-widgets` — one new widget.** With
    decorations deferred, the existing widget crate gains exactly
    one addition: a `CommandPalette` composition widget. It does
    not depend on `fern-plugins-*` — it queries the existing
    `ShortcutRegistry` directly, which carries both app and
    plugin commands (the latter through the plugin shortcut
    registration helpers in § 20). Apps without plugins still
    benefit from `CommandPalette` for their own command surface.
    All plugin contributions continue to flow through
    `fern-plugins-widgets` slot widgets; the dependency boundary
    is preserved.
13. **Off by default.** No runtime is compiled in or loaded unless
    the app builder calls `.install_plugins(...)` with at least
    one runtime registered. Pure-Rust apps that want sandboxed-
    only plugins skip the Python runtime and never bundle CPython.
14. **Plugins are multi-part.** A single plugin can contain
    multiple **UI parts** (one per declared contribution slot)
    plus an optional **always-alive core part** with no view.
    All parts within a plugin instance share runtime memory and
    communicate through a framework-provided **plugin bus** —
    publish-subscribe over plugin-private topics, runtime-agnostic.
    See § 6 (Multi-part plugins) and § 7 (Inter-part communication).
15. **Explicit hello/goodbye contract for every part.** Each
    part exports a lifecycle pair: `init` (called by host on
    load) and `shutdown` (called by host on unload, slot
    unmount, or window close). Core init runs before any UI
    init; UI shutdowns complete before core shutdown. The host
    enforces a shutdown timeout (default 5 s) before force-
    dropping the runtime instance. Clean unload is part of the
    plugin contract — leaking handles, open files, or live
    threads after `shutdown` is a plugin bug, reported in the
    manager UI.
16. **Framework integration is explicit, minimal, and named.**
    `fern-app` gains plugin lifecycle wiring in the
    `WindowManager` and `FernAppHandler` event loop;
    `fern-i18n` learns to load/unload plugin `.ftl` bundles
    under a namespaced key prefix; the existing
    `ShortcutRegistry` and `ShortcutSettings` learn to surface
    plugin-registered shortcuts as their own grouped sections
    with rebind support. No other framework crate is modified
    in v1. See § 20 (Framework integration changes).
17. **Reference example, test suite, and documentation are
    non-negotiable for v1.** The plan ships with
    `examples/plugins_demo/` — a small novelist-IDE-shaped app
    exercising the full matrix (WASM × Python × per-window ×
    shared × all four contribution shapes), a comprehensive
    test suite (unit + integration + E2E against the demo
    app), and a documentation set covering all contribution
    models and both runtime author SDKs. See § 21 (Reference
    example), § 22 (Testing strategy), § 23 (Documentation
    deliverables).
18. **The plugin system lives in a separate repository.** All
    six plugin crates (`fern-plugins-core`, `fern-plugins-wasm`,
    `fern-plugins-python`, `fern-plugin-sdk-wasm`,
    `fern-plugin-sdk-python`, `fern-plugins-widgets`) live in a
    standalone `fern-plugins` repository, depending on the main
    `fern-ui` workspace via git or registry. The
    `examples/plugins_demo/` reference app and bundled example
    plugins also live in `fern-plugins`. Reasons: dependency
    bloat (wasmtime, embedded CPython, HTTP + crypto) shouldn't
    burden apps that skip plugins; the plugin system's CI profile
    (WASM toolchain, cross-platform embedded CPython, sub-
    interpreter validation, network stubs) is fundamentally
    different from the framework's headless GUI test profile;
    the two SDK crates are third-party-facing and need strict
    SemVer hygiene that a separate repo enforces structurally;
    at the framework's current scale (~236k LoC across ~25
    crates), adding another ~30–50k LoC in-workspace crosses
    the "fits in one head" threshold and adds noticeable
    `cargo build` cost for every contributor. The integration
    points enumerated in § 20 (lifecycle hooks in `fern-app`,
    plugin bundle loading in `fern-i18n`, plugin shortcut
    helpers in `ShortcutRegistry`/`ShortcutSettings`, the new
    `CommandPalette` widget in `fern-widgets`) live in the main
    `fern-ui` repo and land first as a single bounded change.
    Plugin work proceeds in the `fern-plugins` repo afterwards.
    See § 20 "Repository split and integration sequencing" for
    the execution plan.

## 1. Architecture overview

The system splits cleanly into three layers:

```text
┌───────────────────────────────────────────────────────────────┐
│ App                                                            │
│   - DocumentProvider impl (bridges app domain to plugin API)   │
│   - Slot widget placement (PluginPanelSlot, PluginToolbarSlot) │
│   - PluginManagerWidget placement                              │
│   - .install_plugins(...) builder call                         │
│   - optional PluginRegistry (marketplace)                      │
├───────────────────────────────────────────────────────────────┤
│ fern-plugins-core (runtime-agnostic)                           │
│   - Bundle format + manifest parsing                           │
│   - Contribution registry (Tier-1 declarative + Tier-2 freeform)│
│   - Slot widgets (PluginPanelSlot, PluginToolbarSlot, …)       │
│   - Scope orchestration (per-window + shared host pools)       │
│   - Lifecycle traits (Runtime, RuntimeInstance)                │
│   - Capability schema + consent flow                           │
│   - Signature verification + installer + updater               │
│   - PluginManagerWidget, PluginConsentDialog, InstallWizard    │
│   - Settings/i18n integration (plugins.<id>.* namespacing)     │
│   - Intent dispatch (rides existing Action/Shortcut pipeline)  │
├───────────────────────────────────────────────────────────────┤
│ fern-plugins-wasm        │ fern-plugins-python                 │
│ (sandboxed runtime)      │ (trusted runtime)                   │
│ - wasmtime               │ - PyO3 + embedded CPython           │
│ - wit interface          │ - Python builder DSL                │
│ - Capability gates       │ - Capability declarations           │
│ - WASM bundle loading    │ - Python module loading             │
│ - WASI shim (scoped FS)  │ - OS sandbox best-effort            │
└───────────────────────────────────────────────────────────────┘
```

**The core decision is that the two runtimes implement the *same*
traits.** `fern-plugins-core` defines `Runtime`, `RuntimeInstance`,
`CapabilityGate`, `FreeformBuilder`; both runtime crates implement
them. Everything above (slot widgets, manager UI, scope orchestration,
contribution registry) sees only the trait, never the concrete
runtime.

## 2. Crate layout

**Repository.** Every crate listed below (`fern-plugins-*` and
`fern-plugin-sdk-*`) lives in the standalone `fern-plugins` repo,
not in the main `fern-ui` workspace (design target #18). The
sequencing — when the main repo gets touched vs when the plugins
repo is created and populated — is in § 20 "Repository split and
integration sequencing."

```text
crates/fern-plugins-core/
    src/
        lib.rs                  # public surface
        bundle.rs               # bundle archive format + extraction
        manifest.rs             # manifest schema + parser + validation
        signature.rs            # Ed25519 signature verification
        registry.rs             # ContributionRegistry, slot subscription
        slot/
            panel.rs            # PluginPanelSlot widget
            toolbar.rs          # PluginToolbarSlot widget
            menu_item.rs        # PluginMenuItemSlot widget
            status.rs           # PluginStatusSlot widget
            # Note: commands aren't slot-widget-based — they're
            # ShortcutRegistry entries surfaced by the new
            # `CommandPalette` widget in fern-widgets (see § 20).
        scope/
            per_window.rs       # PerWindowPluginHost (one per WindowState)
            shared.rs           # SharedPluginHost (singleton, dedicated thread)
            orchestrator.rs     # Lifecycle + ref-counting + idle timer
        runtime.rs              # Runtime / RuntimeInstance traits
        capability.rs           # Capability schema + grant tracking
        intent_bridge.rs        # Plugin intent → host Action dispatch
        registry/
            mod.rs              # PluginRegistry trait, PluginListing, RegistryError
            local_file.rs       # LocalFileRegistry (sideload)
            static_json.rs      # StaticJsonRegistry (HTTP JSON catalog)
        settings_bridge.rs      # plugins.<id>.* SettingsStore routing
        i18n_bridge.rs          # Plugin .ftl bundle loading
        installer.rs            # install / uninstall / update pipeline
        provider.rs             # DocumentProvider trait + NoopDocumentProvider
        bundle_ext.rs           # PluginsBundle builder for FernAppBuilder

crates/fern-plugins-wasm/
    src/
        lib.rs                  # public surface
        runtime.rs              # WasmRuntime: impl Runtime
        instance.rs             # WasmInstance: impl RuntimeInstance
        wit/                    # wit interface files
            world-per-window.wit
            world-shared.wit
            interface-ui.wit
            interface-document.wit
            interface-storage.wit
            interface-network.wit
            interface-notifications.wit
            interface-settings.wit
            interface-i18n.wit
            interface-contributions.wit
            interface-windows.wit
            interface-intents.wit
            interface-lifecycle.wit
        host_impl/              # host-side wit interface implementations
            ui.rs               # ui:: functions → widget resource handles
            document.rs
            storage.rs
            network.rs
            notifications.rs
            settings.rs
            i18n.rs
            contributions.rs
            windows.rs
            intents.rs
        widget_resource.rs      # Resource accounting for ui::widget handles
        signal_resource.rs      # signal-bool/-f32/-string resource impls
        capability_gate.rs      # CapabilityGate impl (enforced)
        wasi_shim.rs            # WASI filesystem shim scoped to plugin storage

crates/fern-plugins-python/
    src/
        lib.rs                  # public surface
        runtime.rs              # PythonRuntime: impl Runtime
        instance.rs             # PythonInstance: impl RuntimeInstance
        embed.rs                # PyO3 init + interpreter lifecycle
        builder_dsl.rs          # @panel / @command / @on_intent decorators
        host_module.rs          # `fern_host` Python module exposing host API
        sandbox.rs              # OS-sandbox best-effort (macOS / Linux / Windows)
        capability_decl.rs      # CapabilityGate impl (declarations only)
        gil.rs                  # GIL acquisition discipline + worker thread

crates/fern-plugin-sdk-wasm/    # what WASM plugin authors compile against
    src/
        lib.rs
        plugin_macro.rs         # fern_plugin! lookalike macro (Rust only)
        bindings/               # wit-bindgen-generated stubs
        helpers.rs              # ergonomic wrappers over raw wit calls

crates/fern-plugin-sdk-python/  # pip-installable for Python plugin authors
    pyproject.toml
    fern_plugin/
        __init__.py
        decorators.py           # @panel @command @on_intent @on_text_changed
        builders.py             # vstack / hstack / button context managers
        signals.py              # Signal wrappers
        host.py                 # raw host-call bindings (re-exported)

crates/fern-plugins-widgets/
    src/
        lib.rs
        manager.rs              # PluginManagerWidget
        consent_dialog.rs       # PluginConsentDialog
        install_wizard.rs       # PluginInstallWizard
        update_policy.rs        # PluginUpdatePolicyWidget
        crash_notification.rs   # PluginCrashNotification
        permissions_panel.rs    # per-plugin permissions inspector
        settings_host.rs        # PluginSettingsHost (wraps Tier-1 settings spec)
        registry_browser.rs     # if app's PluginRegistry exposes browse — optional
```

**Dependency flow:**
`core` is at the bottom. `wasm` and `python` depend on `core`. `widgets`
depends on `core` only (knows nothing about specific runtimes). The
two SDK crates are leaf — they ship to plugin authors, not into the
host. The host app composes:

```text
fern-plugins-widgets   ─┐
fern-plugins-wasm       ├─► fern-plugins-core
fern-plugins-python    ─┘
```

`fern-ui` does NOT re-export plugin crates — plugin support is
deliberately opt-in. Apps that want plugins add the dependencies they
need explicitly.

## 3. Bundle format

Plugins ship as a single archive file with an app-chosen extension
(e.g. `.novelplug` for the novelist IDE, `.consoleplug` for a console
app). Internally the archive is a flat tar/zip with a fixed layout:

```text
my-plugin.novelplug                  # tar.gz, app-chosen extension
├── manifest.toml                    # required
├── signature.bin                    # Ed25519 detached signature
├── runtime/
│   ├── plugin.wasm                  # sandboxed plugins
│   └── ...
│   OR
│   ├── plugin.py                    # trusted plugins
│   ├── requirements.txt             # pip-resolvable deps
│   └── vendored/                    # bundled wheels (optional)
├── i18n/
│   ├── en.ftl
│   └── fr.ftl
├── assets/
│   └── icon.svg
└── README.md                        # optional, shown in manager
```

The archive is verified against `signature.bin` using the manifest's
declared public key before any extraction. The internal manifest
version (separate from the plugin's user-facing version) tracks the
bundle format itself; v1 ships as `bundle_version = 1`.

**Extension is per-app, not framework-fixed.** `PluginsBundle` carries
a `bundle_extension(&str)` setter; the installer's file picker filters
by it, the registry advertises it. This sidesteps cross-app pollution
(an `.fernplug` for the novelist IDE shouldn't show up as installable
in a pentest console even if they share the framework).

## 4. Manifest schema

One TOML file, shared across both runtimes. The `runtime.kind`
field discriminates.

```toml
[plugin]
# Reverse-DNS named after the PLUGIN CREATOR, not the target app.
id = "ai.mistral.grammar-helper"
name = "Grammar Helper"
version = "1.2.0"                      # plugin user-facing version (semver)
author = "Jane Doe <jane@example.com>"
description = "LanguageTool-backed grammar and style checks."
homepage = "https://example.com/grammar-helper"
license = "MIT"
# The app this plugin targets — separate from the plugin id.
# Install fails if this doesn't match the host app's identifier.
target_app = "com.ferntech.novelist"
# Scope is plugin-level: every part of this plugin runs under it.
scope = "per-window"                   # or "shared"

[runtime]
kind = "sandboxed"                     # or "trusted"

# For sandboxed:
wasm_module = "runtime/plugin.wasm"
wit_interface = "fern.plugin/contributions@0.1"

# For trusted (mutually exclusive with sandboxed fields):
# python_entry = "grammar_helper"      # python module name (the package)
# python_requires = ">=3.11"

# Optional always-alive core part — runs for the lifetime of the
# plugin instance. Initialised before any UI part, shut down after.
# Plugins without a core (pure-UI plugins) omit this section entirely.
[core]
entry = "main_core"                    # WASM: exported callable id
                                       # Python: callable in python_entry module
init_priority = 100                    # higher init earlier; default 0

[capabilities]
# Common across both runtimes — sandboxed enforces, trusted declares.
document = ["read", "write"]
storage = "private"                    # "private" | "private+global"
notifications = true
plugin_bus = true                      # required for inter-part communication

[capabilities.network]
allowlist = ["api.languagetool.org"]

# Trusted-only fields. Install fails if kind != "trusted".
[capabilities.trusted_only]
filesystem = ["read", "write"]
subprocess = true
ffi = false                            # ctypes / cffi access

[contributions]
# Each entry is a UI part (or action). `slot` is a LOCAL name
# (no app-id prefix); routing is qualified by [plugin].target_app.

# Tier 1 — declarative, runtime-agnostic, host renders.

[[contributions.status_segments]]
slot = "status.trailing"
binding = "wordcount.current"          # plugin-maintained signal name
intent_on_click = "wordcount.open_goals"

[[contributions.settings_pages]]
slot = "settings"
title_tr = "grammar.settings_title"
form = [
    { kind = "spinbox", label_tr = "max_suggestions", binding = "max" },
    { kind = "toggle",  label_tr = "check_passive_voice", binding = "passive" },
]

# Tier 2 — runtime-specific UI part, plugin builds the tree.
[[contributions.freeform_panels]]
slot = "dock.right"
builder = "render_grammar_panel"       # wit builder-id OR python callable name
# Optional per-part settings (override defaults if needed).
# Lifecycle: this UI part's init runs after the [core] init;
# its shutdown runs before the [core] shutdown.

# Action-shaped, runtime-agnostic — command palette entry + shortcut.
[[contributions.commands]]
id = "check_now"                       # local id; framework qualifies as
                                       # plugin.<plugin_id>.<id> for routing
display_name_tr = "grammar.check_now"
intent = "check_now"                   # local intent name; framework
                                       # qualifies for plugin-internal dispatch
default_keystroke = "Ctrl+Alt+G"       # user-rebindable via ShortcutSettings

[bundle]
bundle_version = 1
ftl_files = ["i18n/en.ftl", "i18n/fr.ftl"]
icons = ["assets/icon.svg"]
public_key = "ed25519:..."             # for signature verification
```

**Validation rules** (enforced by the installer before consent):

- `[plugin].id` must be a syntactically valid reverse-DNS string,
  globally unique within the host app's installed plugin registry.
- `[plugin].target_app` must match the host app's `app_id`
  (configured in `PluginsBundle::app_id(...)`). Mismatch fails
  install with "this plugin is for `<target_app>`, not this app."
- `[plugin].scope` must be `"per-window"` or `"shared"`.
- `[runtime].kind` must be `"sandboxed"` or `"trusted"`. If the
  app didn't register that runtime, install fails with "this plugin
  requires the `<kind>` runtime, which is not enabled in this app."
- Fields under `[capabilities.trusted_only]` are rejected if
  `runtime.kind != "trusted"`.
- All slot names in `[[contributions.*]]` are local names (no
  colons, no DNS prefixes). Slots not present in any mounted
  `PluginXxxSlot` widget in the host app generate an install
  warning ("this plugin will have no visible effect for slot `X`")
  but do not fail the install — slots can be mounted later.
- `[runtime].wit_interface` must be additively compatible with
  the host's wit interface (`host.major == manifest.major &&
  host.minor >= manifest.minor`).
- `[runtime].python_requires` must be satisfiable against the
  embedded CPython version.
- The signature must verify against `[bundle].public_key`.

## 5. Lifecycle and scope

Two host pools, both backed by the same `RuntimeInstance` trait.
A `RuntimeInstance` holds **all parts of one plugin** (optional core
+ N UI builders) — parts are sub-objects of the instance, not
separate runtime objects.

### Per-window scope

`PerWindowPluginHost` lives inside each `WindowState`, alongside the
widget tree. One `RuntimeInstance` per (window × per-window-plugin).
All parts of the plugin (core + UIs) run in that one instance.

```text
window opens, plugin enabled for project
    ↓
Runtime::instantiate(manifest, scope=per_window(window_id))
    ↓
RuntimeInstance::init_core(InitInfo { plugin_id, host_version,
                                       granted_capabilities, window_id })
    ↓ — core's hello runs first
    ↓
contributions become live; UI builders registered, available for slot mount
    ↓ — each UI part's init_ui(builder_id) runs as its slot mounts
    ↓
... user interacts; plugin bus carries inter-part messages ...
    ↓
window closes (or plugin disabled, or app shutting down)
    ↓
RuntimeInstance::shutdown_ui(builder_id) called for each mounted UI part
    ↓ — UI goodbyes run in reverse-mount order
    ↓
RuntimeInstance::shutdown_core() — core's goodbye runs last
    ↓
instance dropped → memory freed
```

Aligns with the existing `WindowManager::close_window` purge hook
(same pattern as file dialogs — see § 20).

### Shared scope

`SharedPluginHost` is an app-level singleton on a dedicated thread.
One `RuntimeInstance` per shared plugin, multiplexed across windows.
Same multi-part structure: one core, N UI builders, all sharing the
instance's runtime memory.

```text
first window enables the plugin
    ↓
Runtime::instantiate(manifest, scope=shared{ initial_windows })
    ↓
RuntimeInstance::init_core(InitInfo { ..., initial_windows: [w1] })
    ↓
ref_count = 1; instance lives on shared host thread
    ↓
UI slots in w1 mount → init_ui(builder_id, window=w1) for each
    ↓
second window opens, enables same plugin
    ↓
RuntimeInstance::window_opened(w2)
    ↓
ref_count = 2
    ↓
UI slots in w2 mount → init_ui(builder_id, window=w2)
    ↓
window w1 closes
    ↓
UI slots in w1 unmount → shutdown_ui(builder_id, window=w1) for each
    ↓
RuntimeInstance::window_closed(w1)
    ↓
ref_count = 1
    ↓
... last window closes; all UI shut down ...
    ↓
ref_count = 0 → start idle timer (default 5 min)
    ↓
no new window in window → shutdown_core(); instance dropped
```

User disables a shared plugin in the manager → graceful shutdown
across all windows simultaneously (all UI parts in reverse order,
then core).

### Hello/goodbye contract

Every part has a lifecycle pair. The host calls them; plugins
implement them. Ordering is deterministic:

| Hook | Caller | When | Required? |
| --- | --- | --- | --- |
| `init_core(InitInfo)` | host | once per instance, before any UI | only if `[core]` declared |
| `init_ui(builder_id, BuildContext)` | host | once per slot mount per window | one per UI part |
| `shutdown_ui(builder_id, window_id)` | host | on slot unmount, window close, plugin disable | mirror of `init_ui` |
| `shutdown_core()` | host | after all `shutdown_ui` complete, instance teardown | mirror of `init_core` |

**Shutdown timeout.** The host gives every `shutdown_*` call a budget
(default 5 s, configurable per `PluginsBundle::shutdown_timeout(...)`).
If the plugin doesn't return within budget, the host logs a warning
and force-drops the runtime instance. For sandboxed runtimes this
is clean (WASM trap, memory freed). For trusted runtimes this is
best-effort — Python plugins that hold OS resources (open files,
sockets, child processes) past `shutdown_core` will leak them.
Surfaced in the manager UI as "this plugin didn't shut down cleanly."

**Hello may fail.** `init_core` returns `Result<(), PluginError>`. On
error, the host:
1. Marks the plugin as failed-to-init in the manager.
2. Does not invoke any `init_ui`.
3. Reports the error to the user (non-modal banner).
4. The plugin stays installed but disabled; user can attempt to
   re-enable from the manager (which retries `init_core`).

`init_ui` failure unmounts that UI part but keeps the rest of the
plugin alive. The host renders an error placeholder in the slot.

### Cross-runtime event routing

Both runtimes route plugin → host events through the existing
`AppEventPoster::post_external` plumbing (built for file dialogs).
The payload carries `(plugin_id, window_id, event)`; the
`FernAppHandler::AppEvent::External` arm downcasts and delivers to
the right window's `WidgetTree`.

For shared plugins, the `window_id` is set explicitly by the plugin
when firing events. For per-window plugins, it's bound at instance
creation and the runtime injects it transparently.

## 6. Multi-part plugins

A plugin is not a monolithic thing. It's a small structured app
with:

- **Zero or one core part** — long-lived background logic, no
  view. Declared as `[core]` in the manifest. Owns the plugin's
  domain state (open documents the plugin tracks, network
  client, ML model handles, scheduled timers). Common shape:
  a struct + an event loop processing inbound bus messages.
- **Zero or more UI parts** — one per `[[contributions.*]]`
  declaration. Each UI part has a `builder` (called per slot
  mount) and per-mount lifecycle. UIs typically subscribe to
  signals published by the core and re-render on update.

All parts of a plugin share the same `RuntimeInstance`. Same
WASM memory (for sandboxed) or same Python interpreter scope (for
trusted). Direct data sharing is possible inside a single
language — but the recommended decoupling is the **plugin bus**
(§ 7), which makes parts testable in isolation and lets the
framework provide cross-cutting concerns (logging, debug
inspection, replay).

### Why this shape

Three reasons.

**The grammar-checker case.** A grammar plugin holds an HTTP client,
a rate-limit budget, a per-document cache of last-checked text →
suggestions. That state needs to live longer than any single panel
view of it. If the user closes the right dock (which mounted the
grammar panel) and then re-opens it, the cached results should still
be there. The core owns that state; the UI part is a view.

**The character-database case.** A novelist plugin holds a list of
characters across a manuscript. It contributes (a) a list panel
showing all characters in the right dock, (b) a status segment
showing the currently-highlighted character's name as the writer's
caret moves over their name in the text, (c) a context-menu item
"Show character" that opens the panel. Three UI parts, all reading
from one shared character index. The core owns the index; UIs are
views.

**The AI-assistant case.** A chat-style assistant plugin holds a
conversation history per project, a streaming HTTP connection to
an LLM API, and a queue of pending requests. UI is a chat panel
that may not be visible at all times. The chat connection has to
stay alive while requests are in flight even if the user closes
the panel. Core owns the connection; UI is a view.

In all three, the alternative — packing the long-lived state into
the UI itself — means losing it whenever the slot unmounts. That's
wrong.

### Cardinality and scope

| Scope | Cardinality of core | Cardinality of each UI part |
| --- | --- | --- |
| per-window | one per (plugin × window) | one per (plugin × window × slot mount in that window) |
| shared | one per plugin (across all windows) | one per (plugin × window × slot mount per window) |

For shared plugins, the core is genuinely "always alive" for the
session. For per-window plugins, "always alive" means "for the
lifetime of the window."

A plugin **must declare a core** if its UI parts share state across
mounts (state that should survive a slot unmount/remount cycle).
Plugins where each UI part is fully self-contained (a wordcount
status segment that maintains a single counter) can skip the core.

### Multi-part lifecycle in detail

```text
plugin instance created
    ↓
init_core(InitInfo) — if [core] declared
    ↓ — core sets up its state, subscribes to bus topics,
        registers any dynamic contributions, returns OK
    ↓
slot A mounts → init_ui("render_panel", BuildContext { window_id })
    ↓ — UI subscribes to bus topics, builds its initial tree,
        returns the widget root
    ↓
slot B mounts → init_ui("render_status_seg", BuildContext { window_id })
    ↓
... user interaction; core publishes "wordcount.updated";
    UIs receive on the bus and re-render ...
    ↓
slot B unmounts (user collapses panel) →
        shutdown_ui("render_status_seg", window_id)
    ↓ — UI unsubscribes from bus topics, drops its widget tree,
        releases per-mount resources
    ↓
slot A unmounts → shutdown_ui("render_panel", window_id)
    ↓
all UI parts shut down
    ↓
shutdown_core() — last chance to flush state to plugin storage,
                  close HTTP connections, etc.
    ↓
instance dropped
```

UI parts can mount/unmount many times over the instance's life.
Core is single-shot: init once, shutdown once.

## 7. Inter-part communication

Plugin parts talk through a typed, plugin-private publish-subscribe
**plugin bus**. The framework provides it; plugins use it for all
intra-plugin communication.

### Why a bus, not direct calls

Inside a single language (Rust-WASM or Python), parts could share
state via plain references. But:

- **Cross-language parity.** Python and WASM plugins use the same
  surface. The bus is the same shape in both.
- **Testability.** A UI part testable against a mock bus doesn't
  need a working core. A core testable against a captured bus
  trace doesn't need real UIs.
- **Debuggability.** The framework can record bus traffic in
  dev-mode, surface it in the plugin manager, replay it for bug
  reproduction.
- **Same-shape across scope.** A per-window plugin's bus and a
  shared plugin's bus behave the same way. (Internally, both are
  in-process — the bus does NOT cross window boundaries or plugin
  boundaries. It's plugin-private and scope-local.)

### Bus surface

```wit
// fern.plugin/bus@0.1
interface plugin-bus {
    // Topic names are arbitrary strings, namespaced inside the
    // plugin. Cross-plugin pollution is impossible — the host
    // routes only within the same RuntimeInstance.
    publish:   func(topic: string, payload: list<u8>);
    subscribe: func(topic: string) -> stream<list<u8>>;
}
```

Payload is `list<u8>` because the framework doesn't constrain
serialization — plugins pick (JSON, MessagePack, plain UTF-8 text,
postcard, whatever). The plugin SDK provides ergonomic typed
wrappers on top:

```rust
// fern-plugin-sdk-wasm — Rust plugin author
use fern_plugin::bus::{publish, subscribe};

#[derive(serde::Serialize, serde::Deserialize)]
struct WordcountUpdated { count: u64 }

fn core_main() {
    // Core publishes
    publish("wordcount.updated", &WordcountUpdated { count: 42 });
}

fn render_status(ctx: BuildContext) -> Widget {
    // UI subscribes
    let count = subscribe::<WordcountUpdated>("wordcount.updated")
        .map(|m| m.count.to_string());
    text(label_source::bound(count), ...)
}
```

```python
# fern-plugin-sdk-python — Python plugin author
from fern_plugin.bus import publish, subscribe

# Core publishes
publish("wordcount.updated", {"count": 42})

# UI subscribes
@subscribe("wordcount.updated")
def on_wordcount(payload):
    status_signal.set(str(payload["count"]))
```

### Bus semantics

- **Async, fire-and-forget.** `publish` does not block. Subscribers
  receive in order per topic, but no delivery guarantee across
  process / instance crashes.
- **Topic strings are plugin-internal.** No collision with other
  plugins. The host's contribution registry topic ("contributions
  changed") is a separate channel, not the plugin bus.
- **Stream semantics.** `subscribe` returns a stream-shaped resource
  (in wit) or an iterator/generator (in Python). The plugin pulls
  from the stream at its own rate; the host buffers (bounded ring,
  default 256 messages) and drops oldest on overflow.
- **Per-topic isolation.** Subscribers to topic A don't see topic B.
  A late subscriber doesn't receive past messages (no retained
  state) — plugins that need "latest value" semantics publish on
  every change and have the subscriber maintain its own latest.
- **No cross-plugin and no cross-window bus.** Inter-plugin
  communication uses the host's existing intent system. Cross-
  window plugin communication is only possible for shared-scope
  plugins (where the core itself is cross-window) — the bus is
  local to one `RuntimeInstance`.

### Capability gate

`plugin_bus = true` in `[capabilities]` (typically declared in any
multi-part plugin's manifest). Without it, `publish` / `subscribe`
return `not-permitted`. The user sees "uses plugin bus" in the
consent dialog — informational only; the bus has no external effects.

## 8. Contribution model

Three contribution shapes, three integration patterns.

### Tier 1 — declarative contributions (runtime-agnostic, 80% case)

Plugin manifest declares contributions as typed data. Host renders
them with native widgets. No plugin runtime calls needed for the
UI itself; the plugin only maintains signals, handles intents, and
persists settings.

Shipped contribution shapes for v1:

| Shape | Renderer (in fern-plugins-widgets) | Use case |
| --- | --- | --- |
| `status_segments` | `StatusSegmentRenderer` | Wordcount, sync status |
| `settings_pages` | `SettingsPageRenderer` (form-builder over declared fields) | Plugin preferences |
| `list_panels` | `ListPanelRenderer` (binds a plugin-owned `ListModel` projection) | Character DB |
| `tree_panels` | `TreePanelRenderer` (binds a `TreeModel` projection) | Outline browser |
| `wizards` | `WizardRenderer` (step list + per-step form) | Export wizards |
| `commands` | (registered in `ShortcutRegistry`, surfaces in command palette) | Any action |
| `menu_items` | `MenuItemRenderer` (in `PluginMenuItemSlot`) | File menu extensions |

Each shape has a typed spec record in `fern-plugins-core::manifest`.
New shapes ship by adding a record + a renderer; existing plugins
keep working because spec validation is forward-tolerant (unknown
fields warn but don't fail).

**Cross-runtime, by construction.** A WASM plugin declaring a
`status_segment` and a Python plugin declaring a `status_segment`
write identical TOML, route through the same host renderer, look
identical to the user, fire intents through the same dispatch.

### Tier 2 — freeform UI (runtime-specific, escape hatch)

For plugins whose UI doesn't fit a declarative shape — scene-pacing
graphs, custom visualizations, novel interaction patterns. The plugin
opts in by listing a slot in `[[contributions.freeform_panels]]`,
and the runtime invokes a plugin-side builder when the slot widget
mounts.

**WASM:** plugin exports `widget-builder::build-widget(builder-id, ctx)`;
host instantiates a `widget` resource graph by calling back into
the plugin's wit module. The wit `ui` interface is the projected
widget surface (§ 10).

**Python:** plugin defines a builder function and decorates it with
`@panel(slot_id)`; host calls the Python callable, which uses
context managers and helpers from `fern_plugin.builders` to compose
the tree (§ 11). Builder calls flow through PyO3 into host-native
widget construction; no widget resource handles, because Python
holds Rust references directly.

**Both surfaces are deliberately verbose.** This is the escape hatch,
not the happy path. The plugin author is consciously paying DX cost
to get capability they couldn't get declaratively.

### Action-shaped contributions (runtime-agnostic)

Commands, intents, shortcuts. Registered through the existing
`Action` / `Shortcut` / `ShortcutRegistry` system. Plugins fire
intents via runtime-specific shims (`host::intents::send` in WASM,
`fern_host.send_intent` in Python); the existing dispatch picks
them up identically.

## 9. Slot widgets

The repeater-style pattern for widget-shaped contributions.
App composes slot widgets at the points where contributions should
appear. Slot identifiers are **local names** (no app-id prefix);
the framework qualifies routing internally using the app's
`app_id` from `PluginsBundle` and the plugin manifest's
`target_app`.

```rust
use fern_ui::prelude::*;
use fern_plugins_core::slot::*;

fern!(ctx =>
    HStack {
        spacing: 8.0
        Button("Save") { on_activate: AppIntent::Save }
        Button("Open") { on_activate: AppIntent::Open }
        Spacer
        PluginToolbarSlot { id: "toolbar.trailing" }   // local name
    }
)
```

Inside a menu:

```rust
MenuList::new()
    .item(MenuItem::new(tr!("file_new")))
    .item(MenuItem::new(tr!("file_open")))
    .separator()
    .child(PluginMenuItemSlot::new("menu.file.extensions"))
    .separator()
    .item(MenuItem::new(tr!("file_quit")))
```

As a side-dock panel:

```rust
SplitView::new()
    .child(main_editor)
    .child(PluginPanelSlot::new("dock.right"))
```

Each slot widget subscribes to `ContributionRegistry` filtered by
`(app_id, slot_id)` (the registry indexes by the qualified pair).
The registry's `Signal<Vec<Contribution>>` drives rebuilds when
plugins are enabled / disabled / installed / uninstalled. Slot
widgets support per-contribution wrapping (separator, frame) via
`Repeater`-style options.

**Routing.** Plugin manifests declare `target_app = "com.X.Y"` plus
local slot names per contribution. At install time, the framework
checks `target_app` against `PluginsBundle::app_id(...)` and
rejects mismatches with a clear error. At runtime, slot widgets
implicitly use the host app's `app_id`, so the slot ID space is
flat from the app developer's perspective. Plugin contributions
to slots not present in any mounted slot widget are tracked in
the registry but render nowhere; the install wizard warns about
this ("this plugin will have no visible toolbar contribution in
this app" or similar).

## 10. WIT interface (sandboxed runtime v1 surface)

The wit interface that WASM plugins link against. v1 ships **two worlds**
(per-window + shared) over **eight interfaces**. The interfaces are
defined in `crates/fern-plugins-wasm/src/wit/`.

### Worlds

```wit
package fern:plugin@0.1.0;

world per-window-plugin {
    import contributions;
    import document-window;
    import storage;
    import network;
    import notifications;
    import plugin-settings;
    import i18n;
    import intents-window;
    import plugin-bus;           // inter-part communication
    import ui;                   // tier-2 freeform only

    export lifecycle;            // unified: core + ui hooks
    export widget-builder;       // tier-2 freeform only
    export command-handler;
    export settings-page;        // optional; only if [[settings_pages]] declared
}

world shared-plugin {
    import contributions;
    import document-shared;
    import windows;
    import storage;
    import network;
    import notifications;
    import plugin-settings;
    import i18n;
    import intents-shared;
    import plugin-bus;
    import ui;

    export lifecycle;
    export widget-builder;
    export command-handler;
    export settings-page;
}
```

### Shared types

```wit
interface types {
    record window-id { value: u64 }
    record document-id { value: string }
    record scene-id { value: string }
    record text-range { start: u32, end: u32 }     // grapheme-cluster offsets
    record selection { range: text-range, anchor-at-start: bool }
    record text-change { range: text-range, replacement: string, revision: u64 }

    record scene {
        id: scene-id,
        title: string,
        text: string,
        metadata: list<tuple<string, string>>,
    }

    record manuscript {
        id: document-id,
        title: string,
        scenes: list<scene>,
    }

    variant intent-payload {
        unit(string),                              // intent-name only
        json(string),                              // intent-name + JSON payload
    }

    variant plugin-error {
        not-permitted(string),
        not-found(string),
        invalid-state(string),
        stale-revision,
        host-internal(string),
    }

    variant check-state { unchecked, checked, indeterminate }
}
```

### `ui` interface — v1 widget surface

Curated subset of ~22 widgets. Excluded for v1: `ListView` /
`TreeView` / `TableView` (data-source-driven — deferred to v2 with
pull-based resources), `RichTextEditor` (decoration concerns out of
scope), `SpinBox` / `Calendar` / `DateEdit` / `ColorPicker` (complex
state models), animations (per-frame cost across boundary), per-call
`.style(impl FooStyle)` overrides (style trait objects can't cross
the boundary — plugins get the host theme's active style via variant
enum only).

```wit
interface ui {
    use types.{intent-payload};

    resource widget { }
    resource signal-bool   { constructor(initial: bool);   get: func() -> bool;   set: func(v: bool); }
    resource signal-f32    { constructor(initial: f32);    get: func() -> f32;    set: func(v: f32); }
    resource signal-string { constructor(initial: string); get: func() -> string; set: func(v: string); }
    resource signal-check  { constructor(initial: check-state); get: func() -> check-state; set: func(v: check-state); }

    variant label-source   { static(string), bound(borrow<signal-string>) }
    variant h-alignment    { leading, center, trailing }
    variant v-alignment    { top, center, bottom }
    variant text-overflow  { wrap, single-line, ellipsis }
    variant button-variant { plain, filled, tinted, outlined, ghost, link, destructive }
    variant orientation    { horizontal, vertical }

    // Layout primitives
    vstack:   func(spacing: f32, alignment: h-alignment, children: list<borrow<widget>>) -> widget;
    hstack:   func(spacing: f32, alignment: v-alignment, children: list<borrow<widget>>) -> widget;
    zstack:   func(children: list<borrow<widget>>) -> widget;
    spacer:   func(min-length: f32) -> widget;
    expand:            func(flex: f32, child: borrow<widget>) -> widget;
    expand-horizontal: func(flex: f32, child: borrow<widget>) -> widget;
    expand-vertical:   func(flex: f32, child: borrow<widget>) -> widget;
    padding:           func(top: f32, right: f32, bottom: f32, left: f32, child: borrow<widget>) -> widget;
    padding-uniform:   func(amount: f32, child: borrow<widget>) -> widget;
    padding-symmetric: func(vertical: f32, horizontal: f32, child: borrow<widget>) -> widget;
    center:            func(child: borrow<widget>) -> widget;

    // Leaves
    text:         func(content: label-source, overflow: text-overflow) -> widget;
    text-styled:  func(content: label-source, role: string, overflow: text-overflow) -> widget;
    icon-svg:     func(svg: string, size: f32) -> widget;
    icon-png:     func(bytes: list<u8>, size: f32) -> widget;
    icon-builtin: func(name: string, size: f32) -> widget;  // "checkmark", "chevron-down", ...
    divider:      func(orient: orientation) -> widget;

    // Controls
    button: func(
        label: label-source,
        intent: intent-payload,
        variant: button-variant,
        enabled: bool,
        tooltip: option<string>,
    ) -> widget;
    icon-button: func(
        icon: borrow<widget>,
        intent: intent-payload,
        tooltip: string,                       // mandatory — AT name for icon-only
        enabled: bool,
    ) -> widget;
    checkbox:          func(value: borrow<signal-bool>,  label: option<string>, enabled: bool) -> widget;
    checkbox-tristate: func(value: borrow<signal-check>, label: option<string>, enabled: bool) -> widget;
    text-input: func(
        value: borrow<signal-string>,
        placeholder: string,
        submit-intent: option<intent-payload>,
        enabled: bool,
        read-only: bool,
    ) -> widget;
    slider:                     func(value: borrow<signal-f32>, min: f32, max: f32, step: f32, label: option<string>) -> widget;
    progress-bar-determinate:   func(value: borrow<signal-f32>, label: option<string>) -> widget;
    progress-bar-indeterminate: func(label: option<string>) -> widget;
    spinner: func(size: f32, label: option<string>) -> widget;
    link:    func(label: label-source, intent: intent-payload, tooltip: option<string>) -> widget;
    badge:   func(label: label-source) -> widget;

    // Containers
    panel:       func(child: borrow<widget>) -> widget;
    card:        func(header: option<borrow<widget>>, content: borrow<widget>, footer: option<borrow<widget>>) -> widget;
    scroll-area: func(child: borrow<widget>, smooth: bool) -> widget;
}
```

### Other interfaces

`document-window` / `document-shared` — read/write document state.
Implicit window context for per-window; explicit `window-id`
parameter for shared. Gated by `document.read` / `document.write`
capabilities.

`storage` — plugin-private K/V. Sandboxed FS shim routes to
`<project>/.fern/plugins/<id>/` (per-window) or
`<config>/plugins/<id>/` (shared).

`network` — HTTP fetch. Gated by `network.allowlist`; non-allowlisted
hosts return `not-permitted`.

`notifications` — toasts and banners.

`plugin-settings` — typed K/V under `plugins.<plugin-id>.*` in the
appropriate `SettingsStore` (per-project for per-window scope, global
for shared).

`i18n` — `tr(key, args) -> string`. Bundle preloaded from manifest.
Plugin must subscribe to `on-locale-changed` and rebuild its tree
on locale change (no automatic re-resolve across the boundary).

`contributions` — register widget builders and commands at runtime
(in addition to manifest declarations).

`intents-window` / `intents-shared` — fire intents into the host.

`lifecycle` — exported by plugin; host calls the multi-part hooks
in order:

```wit
interface lifecycle {
    use types.{window-id, plugin-error};

    record init-info {
        plugin-id: string,
        host-version: string,
        granted-capabilities: list<string>,
        scope: scope-kind,
        // For per-window: the window this instance is bound to.
        // For shared: the first window that triggered instantiation.
        initial-window: window-id,
    }
    variant scope-kind { per-window, shared }

    record build-context {
        window: window-id,        // which window the slot mounted in
        slot-id: string,          // local slot name
    }

    // Hello — core first if [core] declared, else this is a no-op.
    init-core: func(info: init-info) -> result<_, plugin-error>;

    // Hello — one per UI part per slot mount. May be called many
    // times over instance lifetime (slot mount / unmount cycles).
    init-ui: func(builder-id: string, ctx: build-context) -> result<_, plugin-error>;

    // For shared plugins only — fired when additional windows
    // enable the plugin or close while it's running.
    window-opened: func(window: window-id);
    window-closed: func(window: window-id);

    // Goodbye — mirror of init-ui. Called per slot unmount, window
    // close, plugin disable. UI part must release per-mount resources.
    shutdown-ui: func(builder-id: string, window: window-id) -> result<_, plugin-error>;

    // Goodbye — called once after all shutdown-ui complete. Core's
    // last chance to flush state, close connections, etc. After this
    // returns (or times out), the runtime instance is dropped.
    shutdown-core: func() -> result<_, plugin-error>;
}
```

`plugin-bus` — exported by host; imported by plugin. Plugin-private
pub/sub channel for inter-part communication (see § 7).

```wit
interface plugin-bus {
    // Topic strings are opaque to the host; routing is scoped to
    // this RuntimeInstance only — no cross-plugin leakage.
    publish:   func(topic: string, payload: list<u8>);
    subscribe: func(topic: string) -> stream<list<u8>>;
    // Returns an opaque subscription token; drop the stream to
    // unsubscribe. Subscriptions are auto-cleaned at instance
    // teardown.
}
```

`widget-builder` — exported by plugin; host calls
`build-widget(builder-id, build-context)` when a freeform slot mounts.
`build-context` carries the `window_id` so shared-plugin builders
know which window the slot belongs to.

`command-handler` — exported by plugin; host calls
`handle-command(command-id, intent-payload)` when a contributed
command fires.

`settings-page` — exported by plugin if declared in manifest.

### Version coordination across the repo boundary

The wit interface is the contract between the host-side runtime
(`fern-plugins-wasm`) and plugin authors building against
`fern-plugin-sdk-wasm`. Both ship from the same `fern-plugins`
repo but travel to users independently (the host inside an app
binary, the plugin as a `.<bundle_ext>` artefact), so the version
coordination story has to be mechanical, not cultural.

**The compatibility rule** (design target #9 restated for this
surface): host accepts a plugin if
`manifest.wit_interface.major == host.wit_interface.major
&& manifest.wit_interface.minor <= host.wit_interface.minor`.
Additive minor versions extend the interface; breaking changes
bump major and ship side-by-side support periods.

**The mechanics:**

1. **Single source of truth.** Wit files in
   `crates/fern-plugins-wasm/src/wit/` carry the version in the
   `package fern:plugin@X.Y.Z;` declaration. A build script in
   `fern-plugins-wasm` parses this and emits a `WIT_VERSION`
   constant for the runtime's installer check.
2. **SDK exposes the same constant.** `fern-plugin-sdk-wasm`
   re-exports `WIT_VERSION` (its build script reads the same
   wit files via a workspace path). Plugin authors can
   `assert_eq!(WIT_VERSION, "0.1");` in tests for compile-time
   pinning.
3. **Manifest field auto-populated.** `cargo fern-plugin pack`
   (the bundle-builder helper, listed in § 23) reads
   `WIT_VERSION` from the SDK crate at pack time and writes the
   manifest's `wit_interface = "fern.plugin/contributions@X.Y"`
   field. Plugin authors don't hand-edit it.
4. **Host-side installer check.** When the host loads a plugin,
   it compares `manifest.wit_interface` against its own
   `WIT_VERSION` per the rule above. Mismatch fails install
   with a clear message: "This plugin requires wit interface
   X.Y but the host supports X.Z. Please update the host (if
   Y > Z) or the plugin (if Y < Z)."
5. **SDK and runtime ship in lockstep.** Both
   `fern-plugin-sdk-wasm` and `fern-plugins-wasm` ship from the
   same git commit in the `fern-plugins` repo and carry the
   same crate version. Plugins built with SDK 0.4.0 always
   match host runtime 0.4.0's wit interface. SDK 0.4.0 plugins
   still work on host runtime 0.5.x (additive-minor rule).

**Why this matters specifically for the repo split.** With host
and plugin shipping as independent artefacts, plugin authors
would otherwise have to hand-sync their manifest's wit version
to what they linked against — error-prone, asymmetric (the
author sees their SDK version, not the host's), and a footgun
exactly when version skew matters most. Build-script
mechanisation makes the contract automatic: if the plugin
compiles, the manifest is correct.

The cross-repo concern that *does* require coordination is
host-app-version vs plugin-author-published-version — handled
end-to-end by the installer check in (4). The author publishes
once; users with newer hosts install fine via the additive-minor
rule; users with older hosts see a clear error pointing them at
the version mismatch.

### Known limitations of the WASM surface

Documented upfront so plugin authors don't hit them as surprises:

- **Closures across the boundary are impossible.** All handlers are
  intent-name dispatch. The behavior that lives inline in `fern!` /
  V2 builder closures sits in a centralized `command-handler` for
  plugins. Architecturally cleaner; ergonomically more ceremonial.
- **Per-call `.style(impl FooStyle)` overrides unreachable.** Style
  trait objects are host-side Rust. Plugins use the host theme's
  active style; design-language variants (`ButtonVariant::Filled`,
  etc.) still work.
- **Derived signals (`signal.map(...)`) unreachable.** Plugins maintain
  their own combinators on the plugin side and push results to a
  fresh signal handle.
- **Per-character validation / filtering unreachable.** `char_filter` /
  `validator` on TextInput are synchronous per-keystroke closures —
  cannot project across the boundary. Plugins do post-hoc validation
  via `on-text-changed` subscription and corrective edits.
- **Locale reactivity is opt-in rebuild.** Plugin subscribes to
  `i18n.on-locale-changed` and rebuilds its tree. No automatic
  re-resolve.
- **No `fern!` macro projection.** The Rust-side `fern!` macro is
  compile-time native. The `fern-plugin-sdk-wasm` ships a separate
  `fern_plugin!` macro (Rust plugin authors only) that desugars to
  wit-bindgen calls. Same surface shape, strictly narrower
  capabilities, different output.

## 11. Python builder DSL (trusted runtime v1)

For Python plugins, the host module `fern_host` is injected into the
plugin's namespace. The SDK adds decorators and context managers
that close the DX gap to native code. The same multi-part structure
applies — `@core` for the optional always-alive part, `@panel` /
`@status` / `@settings_page` / `@menu_item` for UI parts, plus
intent handlers and bus subscribers.

```python
from fern_plugin import core, panel, on_intent, on_shutdown
from fern_plugin.builders import vstack, hstack, text, button, icon_button, spacer
from fern_plugin.bus import publish, subscribe
from fern_host import tr, db, settings, network

# ──── Core part — always-alive, no view ────────────────────────────

@core
class CharacterDb:
    """Maintains the cross-scene character index for the plugin."""
    def __init__(self):
        self.index = {}              # name -> Character
        self.api = network.HttpClient(allowlist=["api.example.com"])

    def hello(self, info):           # called by host: init_core
        for char in db.iter_all_characters():
            self.index[char.name] = char
        # Subscribe to host text-changes to keep the index fresh.
        db.on_text_changed(self._refresh)

    def goodbye(self):               # called by host: shutdown_core
        self.api.close()
        # Persist working state — manager will report leak otherwise.

    def _refresh(self, change):
        # Recompute affected characters, then notify UI parts.
        affected = self._scan(change)
        publish("characters.changed", {"names": list(affected)})

# ──── UI part — per-window panel ───────────────────────────────────

@panel("dock.right")                 # local slot name
def character_panel(window):
    chars = subscribe("characters.changed")
    with vstack(spacing=12):
        text(tr("characters.title"), style="body_bold")
        for char in db.iter_all_characters():
            with hstack():
                text(char.name)
                spacer()
                icon_button("edit", intent=("edit", char.id))

# ──── UI part — per-window status segment ──────────────────────────

@status("status.trailing")
def char_count(window):
    chars = subscribe("characters.changed")
    return text(chars.map(lambda payload: f"{len(payload['names'])} chars"))

# ──── Action-shaped contributions ──────────────────────────────────

@on_intent("edit")                   # local intent name; framework
                                     # routes plugin-internal dispatch
def handle_edit(payload):
    char_id = payload[1]
    # ... open editor for char_id ...
```

**Lifecycle ordering** (enforced by the host):

1. The `@core`-decorated class is instantiated; its `hello(info)`
   runs first.
2. As UI slots mount, their builders (`@panel`, `@status`, etc.) are
   invoked; each one's return value becomes the slot's content.
3. On unmount, each UI builder's optional `goodbye` (if declared as
   a class with hello/goodbye) runs.
4. On instance teardown, the core's `goodbye()` runs last.

**Context managers** push/pop a builder stack; `vstack` / `hstack` /
`card` etc. return context-manager objects that, on `__exit__`, emit
a fully-built widget to the parent's child list. The
`fern_plugin.builders` module wraps PyO3 calls into host-side widget
construction directly — no resource handles, no marshalling overhead
beyond the GIL transition.

**Plugin bus** (§ 7) — `publish(topic, dict)` and `subscribe(topic)`
mirror the wit interface; payloads are JSON-encoded dicts by default,
with a `serializer=...` keyword to override (pickle, msgpack, raw
bytes). Subscribers return a `Signal`-like object that auto-updates
on new messages.

**Capability checks** happen inside the `fern_host` calls
(`db.characters()` requires `document.read`; `settings.set(...)`
requires `storage.private`). Violations raise
`fern_host.NotPermitted` — informational, since trusted plugins
have full Python access and could circumvent. The exception exists
to surface unintended capability use during development.

### Python runtime architecture: per-plugin sub-interpreter

The Python runtime does **not** share one CPython interpreter across
all plugins. Each Python plugin gets its own **sub-interpreter** with
its own GIL (PEP 684, Python 3.13+), running on its own dedicated
worker thread. This is the load-bearing design choice for trust-
runtime stability — without it, one misbehaving plugin can stall every
other Python plugin in the app.

**What this buys us:**

- **GIL isolation.** Plugin A's CPU-heavy loop never holds plugin B's
  GIL. Each sub-interpreter holds only its own.
- **Crash isolation between plugins.** A Python exception in plugin A
  is caught by A's worker thread and contained; B keeps running. (A
  hard interpreter crash from a misbehaving C extension still affects
  the whole process — that's the per-process model below, deferred to
  v2 if demand emerges.)
- **Per-call interruption.** Host can inject `KeyboardInterrupt` into
  a specific sub-interpreter via `PyThreadState_SetAsyncExc` without
  affecting others. This is the foundation for per-call timeouts.
- **Independent restart.** Plugin A crashes → A's sub-interpreter is
  re-created without touching B's state.

**Per-call timeouts.** Every host → plugin callback runs under a
budget (default **1 s for callbacks**, **5 s for shutdown**, both
configurable per `PluginsBundle::python_call_timeout(...)` and
`PluginsBundle::shutdown_timeout(...)`). On timeout, the host injects
`KeyboardInterrupt` into the plugin's sub-interpreter thread. Plugin
Python code is expected to surface that to the user (the host marks
the call as `PluginError::Timeout(...)`). If the plugin handles and
ignores `KeyboardInterrupt` (cooperative refusal), the host escalates:
drops the worker thread, recreates the sub-interpreter, counts a
crash against the budget.

**Long-running work pattern.** Plugin callbacks must return quickly
(under the budget). For work that doesn't fit — LLM streaming, ML
inference, large file I/O — plugins spawn a background task and
return immediately:

```python
from fern_plugin import async_task, on_intent
from fern_plugin.bus import publish

@on_intent("analyze")
def handle_analyze(payload):
    # Returns immediately; analysis runs on the plugin's
    # asyncio loop (separate from the callback worker).
    async_task(do_analysis(payload))

async def do_analysis(payload):
    result = await long_running_llm_call(payload["text"])
    publish("analysis.complete", {"result": result})
```

`async_task` schedules work on the plugin's own asyncio loop (per
sub-interpreter); results posted to the plugin bus reach UI parts via
the same subscriber mechanism as any other bus message. The host
doesn't time out async tasks — they're not blocking a callback.

**Worker thread model (v1).** One OS thread per active sub-interpreter
(simplest correct model). Idle plugins are not aggressively unloaded;
the worker stays parked between calls. ~8 MB stack overhead per
thread; ~10 MB per sub-interpreter for interpreter state and import
table. With 10 active Python plugins, expect ~150-200 MB total Python
memory footprint (on top of the ~50 MB embedded CPython base).

**Plugin bus across sub-interpreters.** No change to the API. The bus
payload is `list<u8>` (already designed that way in § 7), so cross-
sub-interpreter delivery is a byte-buffer hand-off — no Python object
serialization needed. Publisher in plugin A's sub-interpreter writes
bytes; subscriber in plugin B's sub-interpreter receives the same
bytes. (Bus is plugin-private; cross-plugin delivery doesn't happen
through the bus regardless — this is just about parts of the *same*
plugin if it lives in one sub-interpreter, which it always does.)

**C extension constraint.** Sub-interpreters require C extensions to
support **multi-phase init (PEP 489)**. Most modern wheels do (NumPy,
spaCy, pydantic, httpx, …); some legacy wheels don't (older versions
of certain ML libraries, some custom-built C extensions). Plugins
declaring a `requirements.txt` whose wheels don't support multi-phase
init fail at install with a clear error pointing to the offending
package. Plugin authors then either update to a newer wheel or
report a bug upstream.

**Python version requirement.** Embedded CPython is **3.13+** for
stable per-interpreter GIL semantics. Apps that enable the Python
runtime ship CPython 3.13 in their bundle. Plugin manifests declaring
`python_requires = ">=3.13"` is the common case; older requires
strings install successfully but with a one-time install-time warning.

**Known limitations of the Python surface:**

- **Sub-interpreter ecosystem maturity.** PyO3's sub-interpreter
  support is still maturing as of early 2026. Some PyO3 idioms work
  differently across sub-interpreters; the `fern-plugins-python`
  crate tracks PyO3 versioning carefully.
- **No host-side reactive signal composition.** Plugin-side signals
  use the SDK's own `Signal` class (a Python observer pattern). To
  bridge into host reactivity, plugins call `host.bind(host_signal,
  plugin_signal)` which sets up bidirectional sync.
- **Trusted = no enforced sandbox.** All capability checks are
  advisory. Plugins can `import os` and do anything.
- **Bundle size cost.** Apps that enable the Python runtime pay
  +30–50 MB for embedded CPython. Pure-Rust apps that don't enable
  it stay slim.
- **Per-plugin memory baseline.** ~10–18 MB per active plugin
  (sub-interpreter state + worker thread stack). Above ~30 active
  Python plugins per app, consider per-process plugins (deferred).
- **Hard interpreter crash still affects the process.** A segfault
  in a misbehaving C extension kills the host process. Per-plugin
  sub-interpreters isolate Python-level failures, not native-code
  failures. Apps that need C-extension crash isolation deploy
  per-process plugins (deferred to v2).

Handler decorators (`@on_intent`, `@on_text_changed`, etc.) register
into the host's existing dispatch tables. Capability checks happen
inside the `fern_host` calls (e.g. `db.characters()` requires
`document.read`; `settings.set(...)` requires `storage.private`).
Violations raise `fern_host.NotPermitted` — informational, since
trusted plugins have full Python access and could circumvent. The
exception exists to surface unintended capability use during
development.

## 12. Capability model

One schema, two enforcement semantics.

### Capabilities (v1)

| Capability | Schema | Sandboxed enforcement | Trusted enforcement |
| --- | --- | --- | --- |
| `document.read` | bool | wit calls return `not-permitted` | advisory raise |
| `document.write` | bool | wit calls return `not-permitted` | advisory raise |
| `storage = "private"` | enum | FS shim scopes to plugin dir | advisory; OS sandbox best-effort |
| `storage = "private+global"` | enum | FS shim adds global plugin dir | advisory |
| `network.allowlist = [...]` | list | non-allowlisted hosts return `not-permitted` | advisory; OS sandbox best-effort |
| `notifications` | bool | wit calls return `not-permitted` | advisory |
| `plugin_bus` | bool | publish/subscribe return `not-permitted` | advisory; recommended for any multi-part plugin |
| `filesystem` | list (read/write) | **rejected at install for sandboxed** | OS sandbox path scoping where available |
| `subprocess` | bool | **rejected at install for sandboxed** | OS sandbox subprocess restrictions where available |
| `ffi` | bool | **rejected at install for sandboxed** | advisory |

### Sandboxed enforcement mechanics

The WASM runtime instantiates the plugin with a host-side
`CapabilityGate` that wraps every interface implementation. The
`document::write_edit` wit binding, for example, checks the gate
before calling the host's actual write path; on denied capability,
the wit call returns a `not-permitted` `plugin-error` variant.

The WASI filesystem shim presents the plugin with a virtual root
mounted at the plugin's storage directory. Plugins cannot traverse
above it; absolute paths trigger `not-permitted`.

The network gate uses an HTTP client (host-side `reqwest` or similar)
that inspects every outbound URL against the allowlist before
dispatching. The plugin never gets a raw socket.

### Trusted enforcement mechanics

The Python runtime exposes the same `CapabilityGate` API, but its
implementation only **logs and raises advisory exceptions** —
nothing physically prevents a Python plugin from importing `os` and
reading anywhere. The gate exists to:

1. Surface unintended capability use during plugin development
   (when running with `dev_strict_mode = true`).
2. Provide an audit log of capability invocations (visible in the
   plugin manager UI).
3. Drive OS-level sandboxing where available:
   - **macOS**: App Sandbox with declared paths (via entitlements
     synthesized from the manifest at install time, if the host
     app is itself sandboxed and supports per-plugin entitlements
     — this is best-effort and may not work without significant
     host-app cooperation).
   - **Linux**: seccomp-bpf filters restricting syscall sets, plus
     namespaces for FS scoping where the host has `CAP_SYS_ADMIN`.
   - **Windows**: AppContainer / Job Object restrictions where
     plugin runs in a child process (rare for in-process Python).
4. Inform the user at install time about declared capabilities.

OS-sandboxing layers are best-effort and not promised by the
framework. The trusted-plugin contract is "this plugin runs with
the same privileges as the host app." Apps that need stricter
guarantees ship WASM-only.

### Trusted-plugin security mitigations

The trusted runtime gives plugins host-level privileges by design.
"Capabilities are advisory" is honest but it understates the risk:
a malicious or buggy trusted plugin can delete user files,
exfiltrate documents, install persistence, crash the app. The
framework cannot prevent this structurally without per-process
plugins (deferred to v2 — see § 24, § 26). What v1 ships is
**defence-in-depth** layered on top of the in-process model: every
mitigation below is mandatory or default-on; together they push
the threat model from "trust the plugin completely" to "trust the
plugin's author, with auditable behaviour and tight install gates."

**Mitigations (all v1):**

1. **Signed-by-default for trusted plugins.** `PluginsBundle`
   defaults to `trusted_signing(SigningPolicy::RequireSigned)`. An
   unsigned trusted-runtime plugin is **rejected at install** with
   a clear error. Apps that need to relax this for development
   ship `SigningPolicy::AllowUnsigned` explicitly — the relaxation
   is loud, tracked in the manager UI ("development mode"), and a
   manager-level banner reminds the user. The default for
   sandboxed plugins is `RecommendSigned` (signature shown if
   present, install allowed without one) since structural sandbox
   is the primary protection there.
2. **Capability audit log.** Every capability invocation is
   recorded:

   ```rust
   struct CapabilityInvocation {
       plugin_id: PluginId,
       capability: String,          // "document.read", "network.fetch", ...
       timestamp: SystemTime,
       detail: String,              // "GET https://api.languagetool.org/check"
       result: InvocationResult,    // Allowed | DeniedAdvisory | DeniedHard
   }
   ```

   Per-plugin ring buffer (default 10 000 entries, configurable),
   plus an on-disk audit trail for trusted plugins (rotated daily).
   Viewable in the plugin manager via a new "Activity log" tab per
   plugin. Includes: every host API call the plugin made, every
   capability denial (advisory or hard), every network destination
   contacted, every file path read/written.

   The user can spot a plugin that claimed it only needed
   `document.read` actually reaching out to non-allowlisted hosts,
   or a plugin that frequently triggers `NotPermitted` advisory
   raises (a sign of probing).
3. **Capability invocation rate-limiting.** Repeated `NotPermitted`
   or denied calls within a short window trigger a manager-UI
   warning. Default: > 50 denials/minute → "This plugin is making
   frequent unauthorised calls" banner; > 500/minute → automatic
   pause with user dialog.
4. **Network firewall (mandatory).** Plugin network calls go through
   a host-mediated HTTP client. The allowlist is enforced *by the
   host*, not by the plugin's own libraries. Plugins cannot bypass
   the allowlist through `urllib`, `httpx`, raw sockets, or any
   third-party library — the host intercepts at the socket layer
   on Linux/macOS (via `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES` for
   the worker thread) and via WinSock LSP on Windows where
   feasible. **Note:** this is the most fragile mitigation; if
   socket interception fails (statically-linked SSL, custom DNS
   resolvers), the host falls back to *audit-only* (logs the
   connection attempt but doesn't block). The audit log entry
   makes this visible.
5. **Best-effort per-platform OS sandboxing in-process** (full
   sandboxing requires per-process; that's v2):
   - **Linux:** per-worker-thread seccomp-bpf filter blocking
     `execve`, `fork`, `clone3` (subprocess creation),
     `ptrace`, raw socket syscalls. Installed when the Python
     worker thread spins up; cannot be lifted by the plugin.
   - **macOS:** if the host app is sandboxed (declared in
     entitlements at codesign time), the whole process is
     constrained; plugins inherit. We document the recommended
     entitlement set for apps shipping trusted plugins. **No
     per-plugin scoping in-process** — that's a per-process
     concern.
   - **Windows:** Job Object resource limits (memory, CPU);
     restricted token where the host can configure one. No
     per-thread sandbox primitive.
6. **Resource limits.** Per-plugin memory cap (default 512 MB,
   configurable), CPU quota via cgroups (Linux) / Job Objects
   (Windows) / fixed worker-thread time-slicing (macOS).
   Plugin exceeding limits gets a soft warning, then escalates
   to crash-on-next-allocation if persistent.
7. **Stronger consent dialog (§ 13).** Trusted-variant dialog
   uses an explicit "I trust this author" typed-confirmation
   step (the user types `INSTALL` to proceed) for plugins
   declaring `filesystem`, `subprocess`, or `ffi` capabilities.
   For trusted plugins without those escalation capabilities,
   the standard click-to-install dialog is used.
8. **App-side hard refusal.** `PluginsBundle::reject_in_process_trusted(true)`
   lets apps that care about security refuse to install any
   trusted plugin that isn't out-of-process. In v1, that means
   refusing to install trusted plugins entirely (since
   out-of-process is v2). The setter exists in v1 as the
   forward-looking commitment; v2 makes it meaningful.

**What this doesn't fix.** Be honest: even with all eight
mitigations, a sufficiently determined trusted plugin can still
delete `~/Documents`, exfiltrate the open manuscript over an
allowlisted host, or harvest the user's SSH keys. The mitigations
make malicious behaviour **auditable**, **slower**, and
**discoverable** — but not impossible. The structural fix is
per-process plugins (v2). v1's contract to users is: "trusted
plugins run with host privileges; install only from authors you
trust; the system records what they do."

**Per-process plugins (v2 commitment.)** v2 ships per-process
trusted plugins as the *default*. In-process becomes opt-in via
`[runtime].in_process = true` in the manifest, with extra-loud
consent at install time. Per-process plugins get:
- True OS sandboxing (XPC services on macOS, namespaces +
  seccomp on Linux, AppContainer on Windows)
- Crash isolation including C-extension segfaults
- Per-plugin resource quotas enforced by the OS
- The host's `PluginRegistry::reject_in_process_trusted(true)`
  becomes a *security guarantee*, not just a v1 forward-compat
  setter

The cost: ~50-100 ms IPC overhead per host call (fine for Tier-1
declarative + intents + bus; significant for high-frequency Tier-2
freeform UI rebuilds). The trade matches the security/perf
spectrum cleanly.

## 13. Consent UI

Same widget (`PluginConsentDialog` in `fern-plugins-widgets`), different
copy depending on trust level.

### Sandboxed consent dialog

```
┌─────────────────────────────────────────────────────────────┐
│  🔒 Install Grammar Checker?                                 │
│                                                              │
│  Author: Jane Doe                                            │
│  Version: 1.2.0                                              │
│  Signature: ✓ verified                                       │
│                                                              │
│  This plugin will be able to:                                │
│   • Read and edit text in this project                       │
│   • Send network requests to api.languagetool.org            │
│   • Show notifications                                       │
│                                                              │
│  Plugin contributions:                                       │
│   • Settings page                                            │
│   • Command: "Check grammar now" (Ctrl+Alt+G)                │
│   • Right-dock panel                                         │
│                                                              │
│  [Cancel]                                          [Install] │
└─────────────────────────────────────────────────────────────┘
```

### Trusted consent dialog

```
┌─────────────────────────────────────────────────────────────┐
│  ⚠️  Install Grammar Checker (Trusted)?                       │
│                                                              │
│  This plugin runs with FULL ACCESS TO YOUR COMPUTER.         │
│  It can read or modify any file you can, send any data to    │
│  the network, run other programs, and persist across         │
│  reboots. The system cannot prevent this.                    │
│                                                              │
│  Only install if you trust the author named below.           │
│                                                              │
│  Author:     Jane Doe <jane@example.com>                     │
│  Version:    1.2.0                                           │
│  Signature:  ✓ Ed25519, verified by ferntech.com             │
│  Registry:   plugins.ferntech.com/novelist (✓ trusted)       │
│                                                              │
│  Declared capabilities (recorded in activity log):           │
│   ⚠ Read and write your filesystem                           │
│   ⚠ Run other programs                                       │
│   • Send network requests to api.languagetool.org            │
│   • Read and edit text in this project                       │
│                                                              │
│  Plugin contributions:                                       │
│   • Settings page                                            │
│   • Command: "Check grammar now" (Ctrl+Alt+G)                │
│   • Right-dock panel                                         │
│                                                              │
│  Activity will be logged. View "Plugin Activity Log" in      │
│  Settings → Plugins after install.                           │
│                                                              │
│  To install, type INSTALL below:  [_________]  [Cancel] [Install]
└─────────────────────────────────────────────────────────────┘
```

The trusted dialog is loud by design. Layout, colours, language
all reinforce the security difference:

- Plain plain-English statement of what trusted means ("FULL
  ACCESS TO YOUR COMPUTER", "the system cannot prevent this").
- Plugin escalation capabilities (`filesystem`, `subprocess`, `ffi`,
  unrestricted network) are visually flagged with ⚠ to distinguish
  from scoped capabilities.
- The registry the plugin came from is shown — users with multiple
  registries configured can see which one curated this plugin.
- For plugins declaring escalation capabilities, install requires
  **typing the word `INSTALL`**, not just a click. Friction is
  deliberate. For trusted plugins with no escalation capabilities,
  the standard click-to-install applies.
- Activity logging is explicitly promised in the dialog — sets the
  expectation that the user can audit the plugin's behaviour after
  install.

The sandboxed dialog is correspondingly quieter: no typed
confirmation, no warning emoji, capabilities listed without ⚠
flags. The visual difference is part of the user's intuition.

## 14. Plugin manager UI

`PluginManagerWidget` lists installed plugins with one row per plugin.
Each row shows:

- Trust badge (🔒 green for sandboxed, ⚠ yellow for trusted)
- Scope badge (per-window / shared)
- Plugin name + version
- Enable / disable toggle
- "Configure" button (opens settings page if declared)
- "Permissions" link (opens permissions inspector)
- "Storage" link (opens storage size + clear)
- "Uninstall" button

Top of the widget: an "Install plugin..." button that opens the
file picker (filtered by the app's bundle extension), then routes
through the consent dialog.

**Per-plugin tabs** (when a row is expanded or clicked through):

- **Overview** — version, author, description, capabilities
  granted at install time.
- **Settings** — the plugin's contributed settings page, if any.
- **Activity log** — recent capability invocations from the audit
  log (see § 12). Filterable by capability kind, denial state,
  time range. For trusted plugins, this is the *primary* user-
  facing security surface — they spot anomalies here. Includes
  export-to-JSON for sharing with security reviewers.
- **Permissions** — what was granted at install time, with the
  ability to revoke selectively (revocation triggers plugin
  reload to apply).
- **Storage** — disk usage, "Clear data" action.

For per-window plugins, an additional per-project toggle: "Enabled
for: this project / all projects." For shared plugins, this is N/A
(they're always app-wide).

The widget is parameterised by `PluginScopeFilter` so an app can
embed two instances side-by-side (one for sandboxed, one for
trusted) or one combined view.

## 15. Plugin discovery and registry

The `PluginRegistry` trait is the discovery seam — it abstracts
"where can this app find installable plugins?" so apps can plug in
local-file sideload, a static JSON catalog hosted somewhere, an
HTTP marketplace API, or anything else with the same shape.

**v1 ships two built-in `PluginRegistry` implementations**, both
in `fern-plugins-core`:

1. **`LocalFileRegistry`** — sideload from filesystem. The minimal
   path; user picks a `.<bundle_ext>` file via the file dialog.
   No discovery, just install. Always available.
2. **`StaticJsonRegistry`** — fetches a single JSON catalog from a
   URL, lists the plugins inside, lets the user install them through
   the same consent flow. The smallest possible discovery primitive,
   designed so a project hosting `plugins.json` on GitHub Pages or
   any static-file CDN has a usable plugin ecosystem on day one.
   Optional, opt-in per app.

Anything richer (a real marketplace with search, ratings, comments,
curation pipeline, paid plugins, etc.) is deferred to its own plan
(§ 24). The `PluginRegistry` trait is stable enough that
marketplace-shaped registries are additive.

### `PluginRegistry` trait

```rust
pub trait PluginRegistry: 'static {
    /// All plugins this registry knows about. Returns a reactive
    /// signal so the manager UI updates when the registry refreshes
    /// (LocalFileRegistry never does; StaticJsonRegistry does on
    /// every refresh tick).
    fn list_available(&self) -> Signal<Vec<PluginListing>>;

    /// Resolve a listing to a downloadable bundle. For
    /// LocalFileRegistry this is a passthrough (the user has the
    /// file already); for StaticJsonRegistry this fetches the URL,
    /// verifies the SHA-256, and writes to a temporary file.
    fn fetch(&self, id: &PluginId) -> impl Future<Output = Result<PluginPackage, RegistryError>>;

    /// Optional update check. Default: no-op. StaticJsonRegistry
    /// returns the listing for any installed plugin whose version
    /// is greater than what's locally installed.
    fn check_updates(&self) -> impl Future<Output = Result<Vec<UpdateInfo>, RegistryError>> {
        async { Ok(vec![]) }
    }
}

pub struct PluginListing {
    pub id: PluginId,
    pub name: String,
    pub description: String,
    pub version: Version,
    pub author: String,
    pub license: String,
    pub homepage: Option<Url>,
    pub icon_url: Option<Url>,
    pub screenshots: Vec<Url>,
    pub target_app: String,
    pub runtime_kind: RuntimeKind,
    pub scope: ScopeKind,
    pub download_size: u64,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub min_host_version: Option<Version>,
}
```

### `StaticJsonRegistry`

Built-in implementation that fetches a single JSON document from a
configured URL and caches it locally.

```rust
PluginsBundle::new()
    .app_id("com.ferntech.novelist")
    .registry(StaticJsonRegistry::new(
        "https://plugins.ferntech.com/novelist/plugins.json",
    )
    .refresh_interval(Duration::from_secs(24 * 3600))   // default 24 h
    .timeout(Duration::from_secs(30))
    .trusted_signing_keys(vec![FERNTECH_KEY, COMMUNITY_KEY])
    .cache_dir(app_paths.cache_dir().join("plugin_registry")))
```

**Behaviour:**

1. **Fetch.** On first use and every `refresh_interval`, GET the
   configured URL with `If-Modified-Since` / `If-None-Match` headers.
   `304 Not Modified` is the happy path after the first fetch.
2. **Parse + validate.** Document must match the schema below; any
   listing failing schema validation is dropped with a log warning
   (the rest of the catalog continues to work — one malformed entry
   doesn't kill discovery).
3. **Cache.** Successful fetches written to the configured cache
   directory with timestamp + ETag. On startup, the cached document
   is loaded immediately so the manager UI has data while a fresh
   fetch runs in the background.
4. **Install flow.** When user clicks "Install" on a listing:
   - GET the `download_url`
   - Verify `download_sha256` matches the bytes received
   - GET `signature_url` (if declared); verify Ed25519 signature
     against one of the `trusted_signing_keys` configured on the
     registry
   - Pass the bytes to the installer pipeline (§ 17 sequence diagram)
5. **Updates.** On `check_updates`, compares installed plugin
   versions against the catalog's version field; returns
   `UpdateInfo` for any that have a newer version available.

**Offline behaviour.** If the network is unreachable, falls back to
the cached document. If the cache is empty (first run, offline),
returns an empty listing and surfaces "Couldn't reach the plugin
registry" in the manager UI.

**Multiple registries.** `PluginsBundle::registry(...)` can be
called multiple times to register more than one registry. The
manager UI shows entries from all registries merged (with the
registry's name shown next to each listing for transparency).
Common pattern: ship `StaticJsonRegistry` pointing at the
first-party catalog, plus `LocalFileRegistry` for sideload.

### JSON schema (v1)

A single file. Apps host it wherever; format is stable:

```json
{
  "schema_version": 1,
  "registry_name": "FernUI Novelist Plugin Registry",
  "registry_url": "https://plugins.ferntech.com/novelist/",
  "updated": "2026-05-16T12:00:00Z",
  "plugins": [
    {
      "id": "ai.mistral.grammar-helper",
      "name": "Grammar Helper",
      "description": "LanguageTool-backed grammar and style checks.",
      "version": "1.2.0",
      "author": "Jane Doe <jane@example.com>",
      "license": "MIT",
      "homepage": "https://example.com/grammar-helper",
      "icon_url": "https://plugins.ferntech.com/novelist/icons/grammar-helper.svg",
      "screenshots": [
        "https://plugins.ferntech.com/novelist/shots/grammar-helper-1.png",
        "https://plugins.ferntech.com/novelist/shots/grammar-helper-2.png"
      ],
      "target_app": "com.ferntech.novelist",
      "runtime": "sandboxed",
      "scope": "per-window",
      "download_url": "https://github.com/example/grammar-helper/releases/download/v1.2.0/grammar-helper-1.2.0.novelplug",
      "download_size": 245678,
      "download_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "signature_url": "https://github.com/example/grammar-helper/releases/download/v1.2.0/grammar-helper-1.2.0.novelplug.sig",
      "min_host_version": "0.1.0",
      "tags": ["editing", "grammar", "language-tool"],
      "category": "writing-aids"
    },
    {
      "id": "com.someone.scene-pacing",
      "name": "Scene Pacing Visualizer",
      "version": "0.3.1",
      "description": "Visualise pacing across your manuscript.",
      "author": "Alice Smith",
      "license": "Apache-2.0",
      "target_app": "com.ferntech.novelist",
      "runtime": "trusted",
      "scope": "shared",
      "download_url": "...",
      "download_size": 89421,
      "download_sha256": "...",
      "tags": ["analysis", "visualization"],
      "category": "analytics"
    }
  ]
}
```

**Required fields per listing**: `id`, `name`, `description`,
`version`, `author`, `license`, `target_app`, `runtime`, `scope`,
`download_url`, `download_size`, `download_sha256`. All others
optional.

**Validation rules:**

- `schema_version` must equal `1` (forward-incompatible bumps
  start a new major; the framework loads `2+` catalogs through a
  separate parser).
- `id` must be a syntactically valid reverse-DNS string.
- `target_app` must match the host app's configured `app_id`;
  non-matching listings are filtered out client-side so users only
  see plugins relevant to this app.
- `runtime` must be `"sandboxed"` or `"trusted"`. If the host app
  doesn't have that runtime enabled, the listing is shown but the
  Install button is disabled with a tooltip explaining why.
- `download_sha256` must be a 64-character hex string.
- `download_url` and `signature_url` (if present) must be HTTPS.

### Hosting

The expected hosting model is trivial: commit `plugins.json` to a
git repository, serve via GitHub Pages / Cloudflare Pages / any
static CDN, point `StaticJsonRegistry::new(...)` at the URL. The
plugin bundles themselves typically live as GitHub release assets;
the JSON just lists URLs. Total infrastructure cost: $0.

Apps that want something richer build it on top of `PluginRegistry`
and ship their own `HttpMarketplaceRegistry` (or whatever) — same
trait, different implementation. The framework doesn't care.

### Per-plugin trust and signing

Each registry carries its own `trusted_signing_keys` — the set of
Ed25519 public keys whose signatures are accepted for plugins
installed through this registry. Layered model:

- A plugin's bundle ships with a signature signed by the **plugin
  author's** key.
- The author's key is published in the registry document (per
  listing) or pre-configured in the app's
  `trusted_signing_keys(...)`.
- The registry document itself is **not** signed in v1 — that's a
  full-marketplace concern. Apps that need higher assurance can ship
  with the registry's expected public key pre-configured and verify
  the document's signature separately (a small wrapper around
  `StaticJsonRegistry`).

Plugin install rejects bundles whose signature doesn't match any
trusted key for the registry that surfaced the listing. Sideloaded
bundles (via `LocalFileRegistry`) verify against the app's
top-level `trusted_signing_keys(...)` plus the bundle's declared
`public_key` if `SignatureRequirement::FirstParty` is not set.

## 16. App-side surface

The irreducible minimum an app must provide. Listed exhaustively so
the framework / app boundary stays sharp.

```rust
use fern_ui::prelude::*;
use fern_plugins_core::{PluginsBundle, DocumentProvider};
use fern_plugins_wasm::WasmRuntime;
use fern_plugins_python::PythonRuntime;
use fern_plugins_widgets::PluginManagerWidget;

// 1. Implement DocumentProvider — bridges app domain types to plugin API.
struct NovelistDocs { /* ... */ }
impl DocumentProvider for NovelistDocs {
    fn current_selection(&self, window: WindowId) -> Option<String> { /* ... */ }
    fn current_scene(&self, window: WindowId) -> Signal<Option<plugin_types::Scene>> { /* ... */ }
    fn manuscript(&self, window: WindowId) -> Signal<Option<plugin_types::Manuscript>> { /* ... */ }
    fn on_text_changed(&self, window: WindowId) -> Signal<plugin_types::TextChange> { /* ... */ }
    fn apply_edit(&self, window: WindowId, edit: plugin_types::Edit) -> Result<(), DocError> { /* ... */ }
}

// 2. Install plugins in the app builder.
fn main() {
    FernAppBuilder::new()
        .theme(intui::light())
        .app_paths(AppPaths::new("com", "FernTech", "Novelist").unwrap())
        .settings(SettingsBundle::new().with_window_state(true))
        .install_plugins(
            PluginsBundle::new()
                // Identifies this app to plugins. Plugin manifests
                // declare `target_app = "..."` matching this string;
                // mismatch fails install. Reverse-DNS, app-controlled.
                .app_id("com.ferntech.novelist")
                // Bundle archive extension this app accepts.
                // App-chosen — picker filters by it, registry uses it.
                .bundle_extension("novelplug")
                .document_provider(NovelistDocs::new())
                .runtime(WasmRuntime::new())
                .runtime(PythonRuntime::embedded_cpython("python-3.12"))
                .registry(MarketplaceRegistry::new())   // optional
                .signature_requirement(SignatureRequirement::FirstParty)
                .shutdown_timeout(Duration::from_secs(5))
                .dev_hot_reload_in_debug(),             // skip-consent reload in dev builds
        )
        .initial_window(
            WindowConfig::new()
                .id("main")
                .title("Novelist")
                .size(1400, 900)
                .root(|tree, _state| tree.add(NovelistRoot::new())),
        )
        .run();
}

// 3. Compose slot widgets at contribution points.
//    Slot IDs are LOCAL names — the framework qualifies them with
//    `app_id` from PluginsBundle automatically.
impl Widget for NovelistRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        fern!(ctx =>
            VStack {
                TitleBar { /* ... */ }

                Toolbar {
                    // ...app's built-in toolbar items...
                    Spacer
                    PluginToolbarSlot { id: "toolbar.trailing" }
                }

                Expand {
                    HStack {
                        PluginPanelSlot { id: "dock.left" }
                        Expand { editor }
                        PluginPanelSlot { id: "dock.right" }
                    }
                }

                StatusBar {
                    // ...app's built-in status segments...
                    Spacer
                    PluginStatusSlot { id: "status.trailing" }
                }
            }
        )
    }
}

// 4. Place PluginManagerWidget in settings.
// (e.g. inside the app's "Plugins" settings tab)
ctx.add(PluginManagerWidget::new().filter(PluginScopeFilter::Both))
```

**That is the complete app-side surface.** Roughly 100–200 lines for
a real app. Everything else — runtime mechanics, capability
enforcement, lifecycle, manager UI, consent flow, manifest parsing,
i18n loading, settings namespacing, intent dispatch, slot rendering —
is framework code.

Optional add-ons:

- Implement `PluginRegistry` for marketplace browse/download.
- Add a custom `PluginUpdatePolicyWidget` placement if the default
  doesn't fit the app's settings layout.

## 17. Cross-runtime sequence diagrams

### Plugin install (sandboxed)

```
User clicks "Install plugin..."
    ↓
PluginManagerWidget invokes file picker (filtered by bundle ext)
    ↓
User picks plugin.novelplug
    ↓
Installer.verify_signature(bundle, manifest.public_key)
    ↓
Installer.parse_manifest(bundle)
    ↓
Installer.validate(manifest):
    runtime.kind == "sandboxed"
    WasmRuntime registered? → yes
    wit_interface compatible? → yes
    trusted_only fields absent? → yes
    target_app == PluginsBundle.app_id? → yes
    all slot names known? → warns if any slot has no mounted widget
    ↓
PluginConsentDialog (sandboxed variant) shown
    ↓
User clicks [Install]
    ↓
Installer.extract(bundle, plugins_dir/<id>/)
    ↓
ContributionRegistry.register(manifest.contributions, app_id)
    ↓
i18n.load_plugin_bundles(plugin_id, plugins_dir/<id>/i18n/*.ftl)
    ↓
ShortcutRegistry.register_plugin_shortcuts(plugin_id, [...])
    ↓
PerWindowPluginHost or SharedPluginHost.instantiate(WasmRuntime, manifest)
    ↓
RuntimeInstance.init_core(InitInfo { ... })  // hello — only if [core]
    ↓
Mounted slot widgets repaint with new contributions
    ↓
For each mounted UI slot whose contribution matches:
    RuntimeInstance.init_ui(builder_id, BuildContext { window_id, slot_id })
```

### Plugin uninstall

```
User clicks "Uninstall" in PluginManagerWidget
    ↓
Confirmation dialog
    ↓
For each currently-mounted UI part:
    RuntimeInstance.shutdown_ui(builder_id, window_id)
    ↓ each returns within shutdown_timeout, or is force-killed
    ↓
RuntimeInstance.shutdown_core()  // goodbye — only if [core]
    ↓ returns within shutdown_timeout, or is force-killed
    ↓
Runtime instance dropped
    ↓
ShortcutRegistry.remove_plugin_shortcuts(plugin_id)
    ↓
i18n.unload_plugin_bundles(plugin_id)
    ↓
ContributionRegistry.unregister(plugin_id)  // slot widgets repaint
    ↓
Installer.purge_storage(plugin_id)          // ~/.config/<app>/plugins/<id>/
    ↓
Installer.remove_extracted_files(plugin_id)
```

### Tier-2 freeform panel rendering (WASM)

```
PluginPanelSlot mounts in window w1
    ↓
slot.subscribe(ContributionRegistry, "dock.right")
    ↓ (registry indexes by (app_id, slot_id) internally)
    ↓
registry returns Vec<Contribution> including
    freeform_panel { plugin_id, builder: "render_main" }
    ↓
slot calls FreeformBuilder::build(plugin_id, builder_id, window_id=w1)
    ↓
WasmInstance dispatches to plugin.lifecycle.init_ui(
    "render_main", BuildContext { window_id: w1, slot_id: "dock.right" }
)
    ↓
plugin's init_ui runs its widget-builder closure, calling host::ui::*
    ↓
host resolves widget resource graph → native Widget instances
    ↓
slot.add_child(returned WidgetId)
```

### Tier-1 declarative status segment

```
PluginStatusSlot mounts
    ↓
slot.subscribe(ContributionRegistry, "status.trailing")
    ↓
registry returns Vec<Contribution> including status_segment {
    plugin_id,
    binding: "wordcount.current",   // plugin-internal signal name
    intent_on_click: "wordcount.open_goals",
}
    ↓
slot.add_child(StatusSegmentRenderer::render(spec, plugin_id))
    ↓
renderer asks plugin runtime for the signal handle named
"wordcount.current" (host has a side-table keyed by plugin_id)
    ↓
creates: HStack { TextWidget.bind_text(plugin_signal) }
    ↓ adds click handler firing the plugin-qualified intent
    ↓ (plugin's UI builder is NOT called for tier-1 — host renders
       entirely; plugin only maintains the bound signal)
```

### Multi-part lifecycle (illustrative)

```
plugin instance created (shared scope)
    ↓
init_core(InitInfo { initial_window: w1 })
    ↓ core spins up its HTTP client, loads cached state from
      storage, subscribes to bus topic "character.changed",
      and registers any dynamic contributions
    ↓
slot "dock.right" mounts in w1
    ↓
init_ui("render_character_panel", { window: w1, slot_id: "dock.right" })
    ↓ UI subscribes to bus "characters.changed", builds initial
      tree from current core state, returns widget root
    ↓
slot "status.trailing" mounts in w1
    ↓
init_ui("render_count_status", { window: w1, slot_id: "status.trailing" })
    ↓ ...
    ↓
new window w2 opens, also enables plugin
    ↓
window_opened(w2)
    ↓ core's window-opened handler updates internal per-window state
    ↓
slot "dock.right" mounts in w2
    ↓
init_ui("render_character_panel", { window: w2, slot_id: "dock.right" })
    ↓
... user types in w1; core scans the change, finds new character
    name, publishes plugin-bus message "characters.changed" ...
    ↓
UI parts in BOTH w1 and w2 receive the bus message, re-render
    ↓
w1 closes
    ↓
shutdown_ui("render_count_status", w1)
shutdown_ui("render_character_panel", w1)
    ↓
window_closed(w1)
    ↓
... w2 still open; plugin still alive ...
    ↓
w2 closes
    ↓
shutdown_ui("render_character_panel", w2)
window_closed(w2)
    ↓
ref_count = 0 → idle timer starts (5 min)
    ↓
... no new window mounts the plugin within idle period ...
    ↓
shutdown_core()
    ↓ core flushes state to storage, closes HTTP client, etc.
    ↓
instance dropped
```

## 18. Error and crash management

Plugins are misbehaving third-party code by default. The framework
treats every failure mode as expected, not exceptional, and the host
stays alive and usable regardless of what plugins do. This section
catalogs the failure surface, the recovery contract per category,
and the user-visible behaviour.

### Failure categories

Five distinct categories, each with different recovery semantics:

| Category | Trigger | Isolation | Recovery |
| --- | --- | --- | --- |
| **Init failure** | `init_core` or `init_ui` returns `Err(...)` cleanly | Per-part (init_ui) or per-instance (init_core) | Marked failed in manager; user retries from manager |
| **Capability denial** | Plugin invokes a gated host call without the capability | Per-call; plugin continues running | Plugin handles the error variant; no host action |
| **Runtime crash** | WASM trap / Python uncaught exception during normal operation | Per-instance (WASM); per-sub-interpreter (Python) | Auto-restart up to N times in M minutes; permanent disable after threshold |
| **Callback timeout** | Any host → plugin callback exceeds its per-call budget | Per-call; sub-interpreter / WASM instance interrupted | Log, surface `PluginError::Timeout`; cooperative refusal escalates to crash count |
| **Shutdown timeout** | `shutdown_ui` or `shutdown_core` exceeds `shutdown_timeout` budget | Per-instance | Force-drop the runtime instance; report "dirty shutdown" in manager |
| **Bus handler error** | Plugin's bus subscriber callback panics / throws | Per-callback | Log, drop the message, keep the subscription alive |

Cross-cutting guarantees: **a plugin can never crash the host
process**, and **one plugin's failure never affects another plugin**.
Both are enforced structurally — sandboxed plugins through WASM
isolation, trusted plugins through PyO3's exception conversion and
per-plugin error boundaries in the runtime adapter.

### Per-category contracts

**Init failure (clean return of `Err`).** Plugin returned `Err(PluginError)`
from `init_core` or `init_ui`. Host:

1. Logs the error with full context (plugin id, part id, error
   variant + message) to the plugin error log (per-plugin ring
   buffer, viewable in the manager).
2. For `init_core` failure: skips all `init_ui`, marks the plugin
   as **failed-to-init**, does not register contributions. The
   plugin stays installed; the manager shows a banner with the
   error and a "Retry" button.
3. For `init_ui` failure: that one UI part is not mounted; the slot
   widget renders an error placeholder (small banner with plugin
   name + error message + dismiss button). Other UI parts and the
   core continue normally.
4. Surfaces `PluginCrashNotification` as a non-modal toast unless
   the user has muted notifications for this plugin.

**Capability denial.** Plugin tried to use a host capability it
didn't declare. Host returns `Err(PluginError::NotPermitted(...))`
from the wit call (sandboxed) or raises `fern_host.NotPermitted`
(trusted). The plugin is expected to handle this — the host doesn't
intervene further. Repeated capability denials within a short window
trigger a manager-UI warning ("this plugin is making frequent
unauthorised calls") so users can spot misbehaving / malicious
plugins.

**Runtime crash.** A WASM trap, a Python uncaught exception, a
panic in any plugin callback. Per-runtime mechanics:

- **WASM (sandboxed)**: the trap propagates to the host's wasmtime
  caller. The host catches it, captures the trap location + plugin
  call site, drops the `WasmInstance`. All currently-mounted UI
  parts for that instance become orphaned (slot widgets render the
  error placeholder, same as init_ui failure).
- **Python (trusted)**: an uncaught exception during a plugin
  callback bubbles up to the PyO3 host wrapper *for that plugin's
  sub-interpreter only*. The host captures the full traceback, marks
  the call as failed, and decides per the call site:
  - Exception in `init_core` / `init_ui` → init failure path.
  - Exception in `shutdown_core` / `shutdown_ui` → shutdown timeout
    path.
  - Exception in a regular host-callback (e.g. a `@on_intent`
    handler) → log, drop the message, keep the sub-interpreter alive.
  - Exception during bus handler → bus handler error path.
  - **Per-call timeout**: host injects `KeyboardInterrupt` into the
    sub-interpreter via `PyThreadState_SetAsyncExc`. Plugin's Python
    code receives it as a normal exception; host surfaces
    `PluginError::Timeout`. If the plugin handles + ignores
    `KeyboardInterrupt` (cooperative refusal), host drops the worker
    thread, recreates the sub-interpreter, counts a crash.
  - Hard interpreter crash (segfault from a misbehaving C extension)
    → kills the **host process**, because the segfault crosses
    sub-interpreter boundaries. Sub-interpreters isolate Python-level
    failures only. Apps that need C-extension crash isolation deploy
    per-process plugins (deferred to v2).

Per-instance **crash budget** (configurable via
`PluginsBundle::crash_budget(...)`): default **3 crashes within
2 minutes** → plugin **auto-disabled**, manager UI shows
"this plugin keeps crashing — disabled. Click to investigate."
Without auto-disable, a plugin that crashes on init would loop
forever consuming CPU. The user can re-enable from the manager,
which resets the crash counter.

**Shutdown timeout.** Plugin's `shutdown_*` didn't return within
`shutdown_timeout` (default 5 s). Host:

1. Logs a "dirty shutdown" entry with the part id and the elapsed
   time.
2. Force-drops the runtime instance. For WASM this is clean (no
   leaks). For Python this is best-effort — the Python plugin may
   leak OS resources (open files, sockets, child processes,
   threads) that survive the instance. The host has no way to
   reclaim them in-process.
3. Surfaces a manager-UI warning: "this plugin didn't shut down
   cleanly N times". If repeated across sessions, recommends
   disabling. (Plugin authors should treat this as a bug to fix.)

**Bus handler error.** Plugin's subscriber callback for a bus topic
crashed mid-delivery. Host:

1. Catches the error per-callback (each subscriber runs in its own
   try-block).
2. Logs to the plugin error log.
3. Drops *this* message; the subscription stays active for future
   messages.
4. Other subscribers to the same topic still receive the message
   normally.

### Error propagation flow

Every error follows a single uniform path from the plugin's runtime
to the user-visible surface. Per-category specifics (above) plug
into this flow; the flow itself never branches.

```
[1] Plugin code triggers an error
    • WASM: trap, undeclared cap call, returned PluginError
    • Python: uncaught exception, KeyboardInterrupt on timeout

         ↓

[2] Runtime adapter catches and normalises
    • WasmInstance / PythonInstance wraps native error
    • Captures stack trace per runtime (see "Error context")
    • Snapshots last N bus messages + capability invocations
    • Constructs PluginError + ErrorContext

         ↓

[3] Adapter posts via AppEventPoster::post_external
    • Same plumbing as file dialogs (§ 20)
    • Payload: PluginErrorPayload { plugin_id, window_id?, context }
    • Cross-thread safe; main thread receives on the AppEvent loop

         ↓

[4] FernAppHandler::AppEvent::External arm downcasts and routes:
    a. PluginManager.record_error(context)
         → updates per-plugin state machine
         → applies retry / auto-disable policy
    b. PluginErrorLog.append(context)
         → in-memory ring buffer + on-disk JSON-lines file
    c. CapabilityAuditLog.append (if cap-denial flavour)
         → separate audit log per § 12
    d. If notable & not muted:
         → PluginCrashNotification surfaced as toast/banner
    e. ContributionRegistry.mark_degraded(plugin_id, state)
         → slot widgets repaint with error placeholders where needed

         ↓

[5] Manager UI reflects the new state on next signal flush
    • Badge colour updates (yellow/orange/red/grey per § 18)
    • Activity log tab gains the new entry
    • "Retry" / "Disable" / "Uninstall" actions become available
      per recovery-actions table below
```

Notable properties of this flow:

- **Errors never bubble synchronously through host code.** Plugin
  errors are always routed asynchronously through the
  `AppEventPoster` boundary. This means a plugin throwing during
  a callback can never leave the host's call site in a broken
  state — the runtime adapter has caught it before the host's
  call stack returns.
- **All errors hit the same five steps.** "Init failure" vs "bus
  handler error" differ in their *contents* (different `PluginError`
  variants, different recovery policy at step [4a]) but follow the
  identical pipeline. New error categories slot in without
  reshaping the propagation layer.
- **Step [3] is the only IO-bound step in the synchronous fault
  path.** Steps [1] and [2] run on the plugin's worker thread;
  step [3] is a single bounded-queue push; steps [4] and [5] run
  on the main thread on the next event-loop turn. A flood of
  plugin errors cannot lock up the UI.

### Error context

Every captured error carries a structured `ErrorContext` payload —
this is what gets written to the log, what appears in the manager
"Activity log" tab, and what forms the body of the crash report
dump (§ "Developer affordances" below).

```rust
pub struct ErrorContext {
    // Identification
    pub plugin_id: PluginId,
    pub plugin_version: Version,
    pub host_version: Version,
    pub runtime_kind: RuntimeKind,
    pub scope: ScopeKind,
    pub window_id: Option<WindowId>,

    // What was happening
    pub error_category: ErrorCategory,    // see Failure categories table
    pub call_site: CallSite,              // init_core | init_ui("render_main")
                                          // | handle_intent("save")
                                          // | bus_handler("characters.changed")
                                          // | capability_call("network.fetch")
                                          // | shutdown_ui("render_main") | ...
    pub timestamp: SystemTime,

    // The error
    pub error: PluginError,               // typed variant from § "PluginError variant"
    pub message: String,                  // human-readable summary
    pub stack_trace: Option<StackTrace>,

    // Diagnostic snapshot (cheap to capture; bounded)
    pub recent_bus_messages: Vec<BusMessageSummary>,         // last 10
    pub recent_capability_invocations: Vec<CapabilityInvocationSummary>,  // last 10
    pub granted_capabilities: Vec<String>,                    // for cross-ref with denials
}

pub enum StackTrace {
    /// Full Python traceback as formatted by `traceback.format_exc()`.
    /// Includes file paths, line numbers, function names, source
    /// excerpts. Always present for Python errors.
    Python(String),

    /// WASM trap location plus frame backtrace (module + function name
    /// + instruction offset). If the plugin shipped DWARF debug info
    /// in its `.wasm` module, source line numbers are resolved.
    Wasm {
        trap_kind: String,                 // "unreachable" / "out_of_bounds" / ...
        frames: Vec<WasmFrame>,
    },

    /// Rust panic from the runtime adapter itself (rare; indicates a
    /// host bug). Includes the Rust backtrace.
    HostAdapter(String),
}
```

Capturing recent bus messages + capability invocations is intentional:
the most common debugging question after a plugin error is "what
*else* was the plugin doing at that moment?" — having the answer
already in the report eliminates a round trip with the plugin
author.

### Logging architecture

Plugin errors and operational events have four sinks. Each error
hits some or all of them based on its severity.

```
ErrorContext (every captured error)
    │
    ├──→ [A] In-memory per-plugin ring buffer
    │         • Last 10 000 entries (configurable)
    │         • Visible immediately in manager Activity log tab
    │         • Lost on app restart
    │
    ├──→ [B] Per-plugin on-disk error log
    │         • <config>/plugins/<plugin_id>/logs/errors.log
    │         • JSON-lines format, one ErrorContext per line
    │         • Daily rotation: errors-2026-05-16.log, errors-2026-05-15.log, ...
    │         • Size cap: 10 MB per file (configurable)
    │         • Retention: 30 days default (configurable)
    │         • Synchronous append on the main thread (bounded I/O)
    │
    ├──→ [C] Host structured log (tracing crate)
    │         • Plugin events emit at TRACE level
    │         • Plugin errors emit at ERROR level with fields:
    │             plugin_id, category, call_site, runtime_kind
    │         • Subject to the host app's own tracing subscriber config
    │         • Apps wiring fern-telemetry get plugin errors automatically
    │
    └──→ [D] Dev-mode console (debug builds only)
              • dev_hot_reload_in_debug() implies dev-mode logging
              • Full stack traces printed to stderr immediately
              • OFF in release builds (use sinks A/B/C instead)
```

**Sink selection per category:**

| Category | A (ring) | B (disk) | C (tracing) | D (dev stderr) |
| --- | --- | --- | --- | --- |
| Init failure | ✓ | ✓ | ERROR | ✓ |
| Capability denial | ✓ | ✓ for trusted; sampled for sandboxed | WARN | ✓ |
| Runtime crash | ✓ | ✓ | ERROR | ✓ |
| Callback timeout | ✓ | ✓ | WARN | ✓ |
| Shutdown timeout | ✓ | ✓ | WARN | ✓ |
| Bus handler error | ✓ | sampled (1 in 100) | DEBUG | ✓ |

Capability denials for sandboxed plugins sample on disk (every error
goes to the ring) because a sandboxed plugin doing a `not-permitted`
call is *expected* — that's the runtime catching a misconfiguration —
and writing every one to disk floods the log.

**Audit log is separate** (see § 12). The capability audit log is
"every capability invocation, success or failure"; the error log is
"errors only, across all categories." Both rotate the same way; both
appear in the manager UI but in different tabs.

### Retry policy

The framework distinguishes **transient** failures (retry sometimes
helps) from **permanent** failures (retry never helps).

| Failure | Retry kind | Backoff | Max attempts |
| --- | --- | --- | --- |
| Plugin `init_core` failure | Manual (user clicks Retry) | — | unlimited (user choice) |
| Plugin `init_ui` failure | Auto on next slot remount | — | per slot mount |
| Plugin runtime crash | Auto via crash budget (§ "Per-category contracts") | none between budgeted crashes | 3 in 2 min → auto-disable |
| Callback timeout | None — surfaced as `PluginError::Timeout` | — | plugin decides |
| Shutdown timeout | None — force-drop | — | — |
| Bus handler error | None — message dropped, subscription persists | — | — |
| Capability denial | None — plugin's responsibility | — | — |
| Registry catalog fetch | Auto | Exponential: 1 s, 5 s, 30 s, 5 min | 3, then fall back to cache |
| Plugin bundle download | Auto | Linear: 5 s, 10 s, 30 s | 3, then abort install with error |
| Signature verification | Never | — | hard-fail, no retry |
| SHA-256 hash mismatch | Never | — | hard-fail, no retry |
| Plugin manifest parse | Never | — | hard-fail, no retry |
| Plugin host-version compat | Never | — | hard-fail, no retry |

Defaults are configurable per `PluginsBundle::retry_policy(...)` where
overriding makes sense (the security-critical "Never" rows are not
overridable — the framework refuses to retry signature or hash
failures regardless of configuration).

### User recovery actions

The manager UI exposes these actions per plugin state:

| State | Available actions |
| --- | --- |
| **Healthy (no badge)** | Configure · Disable · Uninstall · View activity log |
| **Yellow — recently errored** | Configure · View error log · Mute notifications for this plugin · Disable · Uninstall |
| **Orange — failed to init** | Retry · View error log · Disable · Uninstall |
| **Red — crash-disabled** | View error log · Re-enable (resets crash counter) · Uninstall |
| **Grey — dirty shutdown** | View error log · Dismiss · Report to author (opens a prefilled email/issue link if the plugin manifest declared `homepage`) · Uninstall |

Plugin author actions (from inside the plugin):

- `fern_host.fatal_error(message)` — plugin marks itself as failed.
  Host treats it as init failure for the rest of the session.
  Use case: plugin detects its own corrupt state and prefers to
  shut down cleanly rather than misbehave.
- `fern_host.report_diagnostic(level, message, fields...)` — plugin
  adds a custom entry to its own error log without triggering host
  recovery logic. Useful for plugins that want to record their own
  diagnostics alongside framework-captured errors.

### Crash isolation guarantees

- **Plugin → host:** never. WASM traps caught by wasmtime; Python
  exceptions caught by PyO3 wrapper; per-runtime worker threads
  isolated from UI thread.
- **Plugin → plugin:** never (Python-level), with one carve-out for
  native crashes. Each plugin has its own `RuntimeInstance`; WASM
  plugins have full memory isolation; Python plugins each get their
  own **sub-interpreter** (PEP 684) on their own worker thread, with
  per-interpreter GIL. A Python exception in plugin A is contained
  by A's sub-interpreter; plugin B keeps running. The one exception
  is a **segfault from a C extension** — that crosses sub-interpreter
  boundaries and kills the process; per-process plugins (deferred to
  v2) are the answer for apps that need C-extension crash isolation.
- **GIL isolation between Python plugins:** plugin A's CPU-heavy
  loop never holds plugin B's GIL. Each sub-interpreter holds only
  its own.
- **UI part → UI part within the same plugin:** UI parts share
  the same `RuntimeInstance`, so a hard crash takes them all down
  together (the instance dies). Soft errors (handler exception
  during an event) are per-handler isolated.
- **Window → window for per-window plugins:** each window has its
  own per-window `RuntimeInstance`. A crash in window A's instance
  is invisible to window B.
- **Window → window for shared plugins:** all windows share one
  shared `RuntimeInstance`, so a crash takes the plugin out for
  every window simultaneously. The host notifies each window's
  active UI parts that their backing instance died, and they
  render error placeholders.

### `PluginError` variant (extended)

The wit `plugin-error` variant declared in § 10 covers the
cross-runtime error surface:

```wit
variant plugin-error {
    not-permitted(string),         // capability denied
    not-found(string),             // resource doesn't exist
    invalid-state(string),         // plugin in bad state
    stale-revision,                // document revision mismatch
    host-internal(string),         // host-side bug; report it
    timeout(string),               // operation exceeded budget
    capability-violation(string),  // structural violation
                                   //   (e.g. unknown capability name)
    resource-exhausted(string),    // host quota hit (memory, storage)
}
```

The Python `fern_host.PluginError` exception hierarchy mirrors
this 1:1, with subclasses (`NotPermitted`, `NotFound`, etc.) so
plugin authors can `except fern_host.NotPermitted as e:` cleanly.

### `PluginCrashNotification` widget

Lives in `fern-plugins-widgets`. A toast / banner pattern triggered
by the plugin manager when a notable error occurs. Configurable per
user (mute per-plugin, mute all). Shows:

- Plugin name + icon
- Error category (init failure / crash / dirty shutdown / repeated
  capability violation)
- Short error summary (first line of the message)
- Actions: `[Dismiss]` `[View log]` `[Disable plugin]` `[Retry]`
  (some only available per category)

In dev-mode (`PluginsBundle::dev_hot_reload_in_debug()`), full
stack traces are surfaced inline instead of summarised. In
production, full traces are written to the plugin error log file
and a "Show full error" button reveals them.

### Manager UI error states

`PluginManagerWidget` rows visually mark plugins in degraded
states:

- **Yellow badge — recently errored.** Recent capability violations
  or bus handler errors. Plugin still running.
- **Orange badge — failed to init.** Plugin installed but `init_core`
  returned `Err` or threw. "Retry" button on the row.
- **Red badge — crash-disabled.** Plugin exceeded crash budget.
  Manual "Re-enable" from row context menu.
- **Grey "leaky" indicator — dirty shutdown.** Plugin completed but
  didn't return from `shutdown_*` cleanly. Recommendation banner if
  it persists across sessions.

Per-plugin "View log" opens a panel showing the plugin error ring
buffer (last 100 entries, configurable). Entries include timestamp,
category, message, and (in dev-mode) stack trace.

### Developer affordances

- **Dev-mode rethrow.** When `dev_hot_reload_in_debug()` is enabled
  (debug builds), uncaught plugin errors print full diagnostics to
  the host's stderr in addition to capturing them. Plugin authors
  see the crash immediately.
- **Crash report dump.** A "Save crash report" action in the manager
  writes a JSON document containing: plugin manifest snapshot, error
  log, last N bus messages, capability grants, host version. Useful
  for filing bug reports against plugin authors.
- **Plugin SDK assertions.** Both SDKs ship debug-mode assertions
  for common plugin-author errors (returning before clean shutdown,
  forgetting to unsubscribe a bus listener, registering duplicate
  contribution IDs). Disabled in release SDK builds.

### What this is NOT

- **Not a sandbox escape mitigation.** This section covers
  *behavioural* failures (plugin crashes / misbehaves). Sandbox
  escapes are a separate concern handled at the runtime layer
  (WASM's structural sandbox, OS-level sandboxing for Python).
- **Not a security audit log.** Plugin error logs are diagnostics
  for the user / developer, not a security audit trail. If
  per-plugin security auditing is needed, that's a separate plan.
- **Not a crash analytics pipeline.** Errors stay local to the
  user's machine. No automatic telemetry. Apps that want to
  aggregate plugin crash reports across users build that on top
  of the per-plugin log files (which is what fern-telemetry would
  consume).

## 19. Known limitations (v1)

Catalogued so plugin authors don't hit them as surprises.

### Cross-runtime

- **Decoration system out of scope** — squigglies, gutter marks,
  inline ghost-text suggestions are not contributable in v1.
  Deferred to its own plan (touches `fern-widgets::RichTextEditor`).
- **Data-source widgets out of scope** — `ListView` / `TreeView` /
  `TableView` not exposed to plugins in v1. Pull-based resource
  projection deferred to v2. Plugins wanting list-shaped data use
  `VStack` of rows or contribute a Tier-1 `list_panel` (which gets
  full `ListView` chrome by the host renderer, but the data model
  is constrained to typed records).
- **Custom shaders / per-frame paint** — plugins cannot ship custom
  paint callbacks. Use of canvas-level primitives is host-only.

### Sandboxed (WASM)

- **No closures across boundary** — all handlers route through intent
  dispatch.
- **No per-call style overrides** — variant enum only.
- **No derived signal combinators** — plugin re-computes manually.
- **No per-character input validation** — post-hoc on `on-text-changed`.
- **Locale reactivity is opt-in rebuild** — plugin subscribes and rebuilds.
- **No `fern!` macro** — Rust plugin authors use a separate
  `fern_plugin!` macro that desugars to wit-bindgen calls.
- **Per-call cost is microseconds, not nanoseconds** — fine for
  occasional rebuilds, bad for per-frame work.

### Trusted (Python)

- **No structural sandbox in v1.** Trusted plugins run in-process
  and can — if the author is malicious or the code is buggy —
  delete user files, exfiltrate documents over allowlisted hosts,
  install persistence, or crash the app. The framework provides
  eight layered mitigations (see § 12, "Trusted-plugin security
  mitigations"): signed-by-default, capability audit log, network
  firewall, per-platform best-effort OS sandboxing, resource
  limits, stronger consent dialogs with typed confirmation for
  escalation capabilities, rate-limiting on unauthorised calls,
  and the app-side `reject_in_process_trusted` setter (forward-
  compat for v2). Together they make malicious behaviour
  auditable and slower, not impossible. The structural fix is
  **per-process plugins**, which become the trusted-runtime
  default in v2 (§ 24). v1 apps that need strong trusted-plugin
  guarantees should either avoid the trusted runtime entirely or
  ship `reject_in_process_trusted(true)` and wait for v2.
- **OS sandboxing is per-platform best-effort, not uniform.** Linux
  gets per-worker-thread seccomp filter (blocks subprocess + raw
  sockets); macOS inherits the host app's sandbox entitlements (no
  per-plugin scoping in-process); Windows gets Job Object resource
  limits only. Uniform sandboxing requires per-process (v2).
- **Network firewall has a graceful-degradation mode.** Socket
  interception via `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES` can fail
  for statically-linked SSL libraries or custom DNS resolvers; the
  host falls back to audit-only (logs connections but doesn't
  block). Logged loudly in the audit trail.
- **GIL serialization** — only between threads within the same
  plugin's sub-interpreter. Cross-plugin GIL contention is
  eliminated by per-plugin sub-interpreters (§ 11).
- **Bundle size cost** — +30–50 MB for embedded CPython.
- **No structural sandbox** — capability checks are advisory; trust
  contract is "plugin = host privileges."
- **Cross-platform build complexity** — embedded CPython on
  macOS / Windows / Linux × x86_64 / arm64 is a real packaging effort.
- **C-extension version pinning** — plugins shipping native wheels
  must target the bundled CPython's exact ABI version. Upgrading
  the embedded CPython is a breaking change for the plugin
  ecosystem.

## 20. Framework integration changes

Three existing crates gain plugin awareness — no others. This
section enumerates the modifications.

### Repository split and integration sequencing

The modifications enumerated below live in the **main `fern-ui`
repository**. The plugin system itself (the six crates from § 2)
lives in a **standalone `fern-plugins` repository** — design
target #18. Execution order:

1. **Land all § 20 changes in `fern-ui` as a single bounded
   change** (one PR, or a tight sequence of small PRs landing
   together). This includes: `FernAppBuilder::install_plugins`
   plumbing in `fern-app`, lifecycle hooks in `WindowManager`,
   `AppEvent::External` plugin payload variant, `I18nManager::load_plugin_bundle`
   / `unload_plugin_bundle`, plugin-shortcut helpers in
   `ShortcutRegistry`, plugin grouping in `ShortcutSettings`, and
   the new `CommandPalette` widget + `CommandPaletteStyle` trait
   + default style in `fern-widgets`. None of these depend on the
   plugin system existing — they're framework extension hooks
   that any plugin runtime (or app code, in the case of
   `CommandPalette`) can consume. They're stable, well-defined,
   and small enough to ship as one change.

2. **Create the `fern-plugins` repository.** Initial scaffold:
   the six crates listed in § 2 (initially as stubs with traits
   defined and impls empty), Cargo workspace, CI configuration
   for the WASM + Python lanes, contributing docs. The repo
   depends on the main `fern-ui` workspace via git dependency
   during active iteration (pointing at a specific commit on
   `main`), upgrading to a crates.io version dependency once
   both publish.

3. **All Phase 2+ plugin work happens in `fern-plugins`.** The
   runtime implementations, the SDK crates, the slot widgets,
   the manager UI, the example plugins, the demo app, the test
   suite, the documentation — everything from Phase 2 onward
   ships in this repo.

This sequencing absorbs the worst of the cross-repo dev cost
(paired PRs touching both repos) into Phase 1, before the
`fern-plugins` repo exists. After Phase 1 lands, the two repos
iterate independently — `fern-plugins` only needs to bump its
`fern-ui` dep when a new integration point is needed (rare; the
§ 20 surface is small).

Documentation hosting follows the same split: `docs/plugins/`
in `fern-plugins` repo (plugin authors land there), with a
short pointer page in `fern-ui`'s `docs/` linking out.

### Inter-dependency rules

The repo split only works if dependencies are strictly one-
directional. This rule is **load-bearing** for the split — if it
ever breaks, the split collapses (consumers of plain `fern-ui`
end up pulling in wasmtime / embedded CPython transitively).

**The rule:** `fern-plugins-*` crates **may** depend on `fern-ui`
crates; `fern-ui` crates **never** depend on any `fern-plugins-*`
crate, directly or transitively.

**The mechanisms** that keep each integration point one-way:

| § 20 integration point | Mechanism | Why `fern-ui` doesn't see plugin types |
| --- | --- | --- |
| `FernAppBuilder::install_plugins` | **Extension trait** `PluginsInstallExt` defined in `fern-plugins-core`, `impl … for FernAppBuilder`. Orphan rule satisfied because the trait is local to the downstream crate. | Same pattern as `install_file_dialog()` / `install_inspector_in_debug()`. `fern-app` exposes builder hooks; `fern-plugins-core` adds the method downstream. |
| `WindowManager::close_window` plugin shutdown | Generic `OnWindowClose` callback list registered at app build time. | `fern-app` invokes `Box<dyn FnMut(WindowId)>`s; the closure body is provided by `fern-plugins-core` at registration time. |
| `AppEvent::External` plugin payload | Already-existing `Box<dyn Any + Send>` shape (used today by file dialogs). | Plugin runtimes post `PluginEventPayload`; `fern-app` downcasts opaquely. No new typed variant. |
| `I18nManager::load_plugin_bundle` / `unload_plugin_bundle` | Method takes `(&str plugin_id, &str locale, &str ftl_source)`. | Plain strings only. `fern-i18n` namespaces internally as `plugin.<plugin_id>.<key>`. |
| `ShortcutRegistry::register_plugin_shortcuts` / `remove_plugin_shortcuts` | Takes `(&str plugin_id, &[Shortcut])`; `Shortcut` is already a `fern-core` type. | Generic batch helpers; the framework parses `plugin.<id>.<local>` ids via a small string parser. |
| `ShortcutSettings::with_plugin_groups(bool)` | Parses the `plugin.<id>.*` id prefix to group rows. | Pure string parsing. |
| `CommandPalette` widget | Queries `ShortcutRegistry` directly; subscribes to its `version()` signal. | The widget knows about shortcuts, not about plugins. |

**Enforcement.** v1 ships no automated check — the rule is enforced
at PR review (adding any `fern-plugins-*` to a `Cargo.toml` inside
the `fern-ui` workspace is rejected). A future `cargo-deny`
configuration in the `fern-ui` repo (`[bans] deny = ["fern-plugins-core",
"fern-plugins-wasm", "fern-plugins-python", "fern-plugins-widgets",
"fern-plugin-sdk-wasm", "fern-plugin-sdk-python"]`) would mechanise
the check if violations ever sneak in.

**Internal `fern-plugins` dependency graph** (no cycles, SDK
crates are leaves consumed by external plugin authors):

```text
fern-plugin-sdk-wasm    ─┐                  fern-plugin-sdk-python
                         │ (wit bindings    │ (pure Python pip
                         │  shared via      │  package, no Rust
                         │  build script)   │  deps)
                         ▼                  ▼
                fern-plugins-wasm ──┐  ┌── fern-plugins-python
                                    │  │
                                    ▼  ▼
                              fern-plugins-core
                                    ▲
                                    │
                            fern-plugins-widgets
```

**App-side dependency shape.** Apps adopting plugins list these
dependencies, with runtime crates feature-gated so wasmtime /
CPython binary cost is paid only when needed:

```toml
[dependencies]
fern-ui              = "..."                          # always
fern-plugins-core    = { version = "...", optional = true }
fern-plugins-widgets = { version = "...", optional = true }
fern-plugins-wasm    = { version = "...", optional = true }
fern-plugins-python  = { version = "...", optional = true }

[features]
plugins           = ["fern-plugins-core", "fern-plugins-widgets"]
sandboxed-plugins = ["plugins", "fern-plugins-wasm"]
trusted-plugins   = ["plugins", "fern-plugins-python"]
```

### `fern-app`

1. **`FernAppBuilder::install_plugins(PluginsBundle)`** — the entry
   point. Stores the bundle in `AppState` so widgets and runtime
   instances can reach it. Must run after `.app_paths(...)` and
   `.settings(...)` since plugins inherit settings-system paths.
2. **`AppState` carries a `PluginManager` handle** — exposes
   `enabled_plugins()`, `enable(plugin_id)`, `disable(plugin_id)`,
   `install(bundle_path)`, `uninstall(plugin_id)`. Slot widgets
   read from the contribution registry indirectly through this
   handle.
3. **`WindowManager::close_window` calls per-window plugin
   shutdown** — extends the existing file-dialog purge hook to
   also iterate the window's `PerWindowPluginHost` and shutdown
   every mounted UI part + core (in correct order, with timeout).
   Same shape as the existing `FileDialogHandle` purge — one more
   line in `close_window`.
4. **`FernAppHandler::AppEvent::External` arm gains a plugin
   payload variant** — current variant downcasts for
   `FileDialogEventPayload`; add `PluginEventPayload` carrying
   `(plugin_id, window_id, intent_payload)` that routes into the
   target window's `WidgetTree` for handler dispatch.
5. **Boot ordering** — plugins are loaded after `SettingsBundle` is
   initialised (because plugins use the settings store) and before
   the initial `WindowConfig` is materialised (because the initial
   window's slot widgets must see the contribution registry
   populated). New step in the `FernAppBuilder::run` sequence,
   between `settings` init and `initial_window` setup.
6. **`AppEventPoster::post_external` reuse** — no changes; the
   poster is already thread-safe and accepts arbitrary `Box<dyn
   Any + Send>`. Plugin runtimes (Python worker thread, WASM
   shared host thread) post events through it the same way file
   dialogs do today.

### `fern-i18n`

1. **`I18nManager::load_plugin_bundle(plugin_id, locale, ftl_source)`**
   — adds a per-plugin Fluent bundle to the manager. Plugin keys
   are namespaced as `plugin.<plugin_id>.<key>` so they cannot
   collide with app keys. Bundle is registered against the existing
   per-locale resource set.
2. **`I18nManager::unload_plugin_bundle(plugin_id)`** — symmetric
   removal. Drops all `plugin.<plugin_id>.*` keys from every locale.
   Triggers re-resolve of any active `LocalizedString` referencing
   those keys (they fall back to `KEY_NOT_FOUND` style markers,
   then the slot widget unmounts when the contribution is removed).
3. **Plugin-bundle hot-reload** — existing file watcher learns the
   plugin install dir layout and re-loads `.ftl` files when they
   change. Per-plugin path: `<config>/plugins/<plugin_id>/i18n/`.
4. **Manager API surface** — these two methods plus an `iter_plugin_bundles()`
   for the inspector / debug UI. No changes to `tr!` / `tr_signal!` /
   `tr_widget!` / `tr_signal_widget!` — they already handle arbitrary
   key strings, plugin keys just have a longer prefix.

### `ShortcutRegistry` and `ShortcutSettings`

The existing `Shortcut` type has `id: &'static str` and `category:
&'static str` fields. We use these as the integration point — no
new `source` field needed, just convention + framework helpers.

1. **Plugin-registered shortcut convention** — plugin-registered
   shortcuts use `id = "plugin.<plugin_id>.<local_id>"` prefix.
   The framework parses this prefix when needed (grouping in
   settings UI, cleanup on uninstall). Plugin authors don't see
   the convention — the SDK's `register_shortcut(local_id, ...)`
   call qualifies it automatically.
2. **`ShortcutRegistry::register_plugin_shortcuts(plugin_id, [Shortcut])`**
   — convenience method that registers a batch and tracks them for
   cleanup. Routes the user-rebind through the existing rebind
   pipeline (no plugin-specific path).
3. **`ShortcutRegistry::remove_plugin_shortcuts(plugin_id)`** —
   symmetric cleanup. Iterates registered shortcuts, removes those
   matching the plugin's prefix, fires the registry's existing
   `version()` signal to notify subscribers (menu labels, tooltips,
   the settings UI itself).
4. **`ShortcutSettings::with_plugin_groups(bool)`** — opt-in (or
   default-on) flag that groups plugin shortcuts into their own
   sections in the settings UI, sectioned by plugin name. Without
   the flag, plugin shortcuts appear interleaved with app
   shortcuts under their declared `category`. Either behaviour is
   valid; grouped is the recommended default for apps with multiple
   plugins.
5. **`ShortcutSettings::hide_plugin_shortcuts(bool)`** — escape
   hatch for apps that don't want users rebinding plugin shortcuts
   (e.g. for plugins that ship their own rebind UI). Off by default.
   Hidden shortcuts still work — just not surfaced in this
   particular settings widget.

### `fern-widgets` — new `CommandPalette` widget

Fern doesn't ship a command palette today. It needs to, both as a
generic primitive any app would want and as the natural surface for
plugin-contributed commands. Adding it is the **only** modification
to `fern-widgets` in this plan.

**Design.** A composition widget that opens on a registered
keystroke (default `Ctrl+Shift+P`, app-rebindable), shows a search
input over a filtered list of every command in `ShortcutRegistry`,
and dispatches the selected command on Enter / click. Composed from
existing primitives — `Dialog` (or `Popover` if app prefers
overlay), `TextInput` for search, `ListView` for results, with the
established theming protocols.

```rust
// crates/fern-widgets/src/command_palette.rs (new)
pub struct CommandPalette { /* fields */ }

impl CommandPalette {
    /// Open via a shortcut. Default keystroke is Ctrl+Shift+P;
    /// rebindable through ShortcutRegistry like any other shortcut.
    pub fn new() -> Self;

    /// Override the trigger keystroke. The shortcut is registered
    /// under id `app.command_palette.open` so it shows up in
    /// ShortcutSettings.
    pub fn trigger(self, keystroke: KeyStroke) -> Self;

    /// Filter what gets shown. Default: all enabled shortcuts.
    /// Pass `CommandFilter::ExcludePlugins` to hide plugin commands
    /// (rare — usually apps want them shown).
    pub fn filter(self, filter: CommandFilter) -> Self;

    /// Group commands by category in the result list. Default true.
    pub fn group_by_category(self, on: bool) -> Self;

    /// Per-call style override (CommandPaletteStyle trait protocol,
    /// same pattern as other themable widgets).
    pub fn style(self, style: impl CommandPaletteStyle) -> Self;
}

pub enum CommandFilter {
    All,
    ExcludePlugins,
    Custom(Box<dyn Fn(&Shortcut) -> bool>),
}
```

**Data flow.** Subscribes to `ShortcutRegistry::version()` (the
existing `Signal<u64>` that fires on any registry change). Plugin
commands appear in the palette automatically because plugins
register through the same `ShortcutRegistry::register_plugin_shortcuts`
helper added above. Each row shows: command name, keystroke hint,
category, and (for plugin commands, via the `plugin.<id>.*` id
prefix convention) a subtle plugin-source label.

**Accessibility.** The opened palette is a `Role::Dialog` with
`Role::SearchBox` for the input and `Role::ListBox` for the
results. Arrow keys navigate, Enter activates, Esc dismisses.
Type-ahead filtering is announced by screen readers.

**Theming.** New `CommandPaletteStyle` trait in `fern-core::styles`,
default `RecipeCommandPaletteStyle` in `fern-widgets/src/styles/`,
follows the established Tier-3 style-protocol pattern (Button,
Card, Dialog, etc.).

**App opt-in.** Apps mount the palette by adding a single line to
their root build:

```rust
ctx.add(CommandPalette::new())
```

The widget self-mounts as an overlay; no slot composition needed.

**Where to put it.** New file `crates/fern-widgets/src/command_palette.rs`
plus `crates/fern-widgets/src/styles/recipe_command_palette_style.rs`
for the default style. Added to `fern-widgets/src/lib.rs` re-exports.
Listed in `docs/widgets-overview.md`.

### No changes elsewhere

Explicit non-changes for clarity:

- **`fern-widgets`** — beyond `CommandPalette` above, no other
  modifications. Slot widgets live in `fern-plugins-core`, not
  `fern-widgets`. (Decorations would touch `RichTextEditor`, but
  those are deferred to their own plan.)
- **`fern-core`** — zero modifications. `Action` / `Intent` /
  `Shortcut` already support plugin-registered entries through the
  conventions above; no new traits.
- **`fern-data`** — zero modifications. Plugin-contributed list /
  tree panels in Tier-1 use typed contribution-spec records that
  the host renderer projects onto `ListModel<T>` / `TreeModel<T>`
  internally. Plugins never see these types directly.
- **`fern-settings`** — zero modifications. Plugin settings ride
  the existing `SettingsStore` dotted-key namespace
  (`plugins.<id>.*`), with no changes to the store itself.
- **`fern-canvas`, `fern-render`, `fern-platform`, `fern-tokens`** —
  zero. Plugins never reach below the widget surface.

## 21. Reference example

A single demonstrator that exercises the whole surface lives in
`examples/plugins_demo/`. Its existence is non-negotiable — it's
the canonical "does the whole story work" smoke test, and the
template plugin authors will copy from.

### Demo app layout

`examples/plugins_demo/` is a small novelist-IDE-shaped app —
title bar + toolbar + left dock + main editor + right dock +
status bar + menu bar + command palette. It is intentionally
*minimal* outside of plugin contribution points, so the demo's
real content comes from the bundled example plugins.

Mounted plugin slots:

| Slot widget | Local slot ID | Purpose |
| --- | --- | --- |
| `PluginToolbarSlot` | `toolbar.trailing` | Trailing toolbar items |
| `PluginPanelSlot` | `dock.left` | Left-dock plugin panels |
| `PluginPanelSlot` | `dock.right` | Right-dock plugin panels |
| `PluginStatusSlot` | `status.trailing` | Trailing status segments |
| `PluginMenuItemSlot` | `menu.tools` | Tools-menu extensions |
| `CommandPalette` (fern-widgets) | — | Surfaces every command in `ShortcutRegistry`, including plugin-registered ones |
| `PluginManagerWidget` | (inside settings tab) | Manager UI |

The demo's `app_id` is `dev.fernui.plugins_demo`; bundle extension
is `.fernplugin`.

### Bundled example plugins

Four plugins, one per cell of the (runtime × scope) matrix, exercising
**all four contribution shapes** across the set:

```text
examples/plugins_demo/
    src/
        main.rs                          # the demo app
        novelist_docs.rs                 # DocumentProvider impl over a mock manuscript
    plugins/
        wordcount-wasm-per-window/       # WASM + per-window
            Cargo.toml
            manifest.toml
            src/lib.rs                   # tier-1 status_segment + settings_page
            i18n/{en,fr}.ftl
        grammar-wasm-shared/             # WASM + shared
            Cargo.toml
            manifest.toml
            src/lib.rs                   # tier-2 freeform_panel + core part
                                         #   + commands + menu_items + plugin_bus
            i18n/{en,fr}.ftl
        character-db-python-per-window/  # Python + per-window
            manifest.toml
            character_db/
                __init__.py              # tier-1 list_panel + commands + core
                en.ftl, fr.ftl
        ai-assistant-python-shared/      # Python + shared
            manifest.toml
            ai_assistant/
                __init__.py              # tier-2 freeform_panel + core part
                                         #   + status_segment + plugin_bus
                en.ftl, fr.ftl
```

Contribution coverage matrix (each cell has at least one example):

| | tier-1 status | tier-1 settings page | tier-1 list panel | tier-2 freeform | command | menu item | bus / multi-part |
| --- | --- | --- | --- | --- | --- | --- | --- |
| WASM per-window | wordcount ✓ | wordcount ✓ | – | – | – | – | – |
| WASM shared | – | – | – | grammar ✓ | grammar ✓ | grammar ✓ | grammar ✓ |
| Python per-window | – | – | character-db ✓ | – | character-db ✓ | – | character-db ✓ |
| Python shared | ai-assistant ✓ | – | – | ai-assistant ✓ | – | – | ai-assistant ✓ |

### Mocked / faked data

The demo app ships with:

- A fake manuscript loaded from `examples/plugins_demo/fixtures/manuscript.toml`
  — a few scenes of public-domain text. `NovelistDocs::Provider` reads from it.
- A fake `MarketplaceRegistry` (`FakeMarketplace` in the demo) that lists the
  four bundled plugins as "available," even though they're sideloaded from
  `examples/plugins_demo/plugins/` rather than downloaded. This exercises
  the registry UI path without requiring real infrastructure.
- Pre-seeded plugin settings (`fixtures/settings.toml`) so plugins boot with
  realistic state.

### What it demonstrates end-to-end

1. **Boot sequence** — demo app boots, plugins load, contributions appear in
   the right slots immediately.
2. **Install flow** — sideload one of the bundled plugins through the manager
   UI; consent dialog shows correct copy per trust level; plugin appears in
   the right slots.
3. **Multi-part lifecycle** — open a second window of the demo app;
   per-window plugins re-instantiate per window; shared plugins receive
   `window_opened`.
4. **Plugin bus** — observe character-db (Python per-window) publishing
   to its bus; verify both its panel and its status segment receive
   updates.
5. **Cross-runtime parity** — wordcount-wasm and ai-assistant-python both
   contribute status segments; they render and behave identically from the
   user's perspective.
6. **Uninstall** — uninstall a plugin through the manager; verify clean
   teardown (shortcuts removed, i18n bundle removed, slot widgets repaint).
7. **Plugin-internal i18n** — switch the app's locale; plugin labels
   translate via the plugin's own `.ftl` bundles.
8. **Plugin shortcut rebind** — open `ShortcutSettings`; rebind a
   plugin-registered shortcut; verify the menu label / tooltip update.

### Run

```bash
cargo run -p plugins_demo
```

A single CLAUDE.md note added (under "Build Commands") listing the demo.

## 22. Testing strategy

Tests are not optional. Every layer of the plugin system has a test
budget in v1.

### Unit tests (per crate)

**`fern-plugins-core`:**

- Bundle parsing (`bundle.rs`):
  - Valid bundle round-trip (build → parse → re-extract).
  - Malformed manifest TOML rejected with span info.
  - Missing required fields rejected.
  - Signature mismatch rejected before extraction.
- Manifest validation (`manifest.rs`):
  - All rules from § 4 are tested as positive + negative cases:
    - `target_app` mismatch fails.
    - `runtime.kind = "sandboxed"` + `capabilities.trusted_only.*`
      present → rejected.
    - `runtime.kind = "trusted"` accepts trusted_only fields.
    - `wit_interface` version compatibility (additive minor accepted,
      major-incompatible rejected).
    - Slot names without matching mounted slot widget → install warning,
      not failure.
- Capability gate (`capability.rs`):
  - Mock runtime + sandboxed gate: undeclared capability call returns
    `not-permitted`.
  - Mock runtime + trusted gate: undeclared call returns
    advisory exception, action still proceeds.
  - **Audit log**: every invocation captured with plugin id,
    capability, timestamp, result. Ring buffer eviction beyond
    configured limit. On-disk audit trail for trusted plugins
    rotates daily.
  - **Invocation rate-limiting**: 50+ denials/minute triggers
    warning state; 500+/minute triggers automatic pause.
  - **Network firewall**: allowlisted host succeeds; non-allowlisted
    host returns `not-permitted` for sandboxed, blocked + audited
    for trusted (where socket interception works), audit-only
    fallback path when interception unavailable.
- Signing policy:
  - `SigningPolicy::RequireSigned` rejects unsigned bundles.
  - `SigningPolicy::AllowUnsigned` accepts them with warning.
  - `SigningPolicy::RecommendSigned` accepts both, surfaces
    signature presence in consent dialog.
  - Default for trusted-runtime: `RequireSigned`; mismatched
    install attempt rejected with clear error.
- `PluginsBundle::reject_in_process_trusted(true)` refuses every
  trusted-runtime plugin install in v1.
- Contribution registry (`registry.rs`):
  - Register + unregister; subscribers see additions and removals.
  - Multiple plugins contributing to same slot — order preserved.
  - Subscribers filter by `(app_id, slot_id)` correctly.
- Slot widgets (`slot/*.rs`):
  - Mount, then add contribution → child appears.
  - Remove contribution → child disappears.
  - Mock contribution payloads render correctly.
- Lifecycle ordering (`scope/orchestrator.rs`):
  - `init_core` runs before any `init_ui`.
  - `shutdown_ui`s run before `shutdown_core`.
  - Shutdown timeout enforced (mock runtime that hangs).
  - `init_core` failure → no `init_ui` called.
- Error propagation (`runtime.rs` + adapter integration):
  - Mock plugin throws in `init_core` → propagates through the
    5-step flow, manager state goes orange, log entry present,
    no `init_ui` called.
  - Mock plugin crashes in handler → propagates, manager state
    goes yellow, log entry present, plugin keeps running.
  - Crash budget exceeded (3 in 2 min) → auto-disable, state goes
    red, "Re-enable" available.
  - All errors carry full `ErrorContext` (verified field-by-field
    against snapshot fixtures).
  - Recent-bus-messages and recent-capability-invocations snapshots
    are bounded to 10 entries each.
- Logging architecture:
  - In-memory ring buffer wraps at configured size, oldest evicted.
  - On-disk error log rotates daily at midnight UTC.
  - Size cap enforced (synthetic plugin writing > 10 MB triggers
    rotation mid-day).
  - Retention sweep removes files older than configured retention.
  - JSON-lines parse round-trip preserves all `ErrorContext` fields.
  - tracing crate emits at correct levels per category.
- Retry policy:
  - Registry fetch failure retries with exponential backoff,
    succeeds on 3rd attempt → install proceeds.
  - Registry fetch fails 3× → falls back to cache, audit log entry.
  - Plugin download failure retries linearly, succeeds on 3rd
    attempt → install proceeds.
  - Signature failure NEVER retried regardless of configuration
    (tested by attempting to override `retry_policy` and verifying
    the override is ignored for signature/hash failures).
- Plugin bus (`runtime.rs` bus tests):
  - Publish + subscribe round-trip.
  - Multiple subscribers receive in order per topic.
  - Cross-plugin isolation (plugin A's topics don't leak to plugin B).
  - Late subscriber doesn't receive past messages.
  - Ring buffer overflow drops oldest.
- Settings bridge (`settings_bridge.rs`):
  - Plugin settings write to `plugins.<id>.*` namespace.
  - Plugin uninstall purges its namespace.
- Registry (`registry/static_json.rs`):
  - Valid JSON catalog parsed; all listings present.
  - Malformed listings dropped with log warning; rest preserved.
  - `target_app` mismatch filters listings client-side.
  - Schema-version mismatch routed to dedicated parser (or rejected).
  - SHA-256 mismatch on download → `RegistryError::HashMismatch`,
    no install proceeds.
  - Signature mismatch → `RegistryError::SignatureMismatch`,
    no install proceeds.
  - Cached document loaded on startup; fresh fetch happens in background.
  - Offline (network unreachable) → falls back to cache; empty cache
    + offline → empty listing + clear user message.
  - `If-Modified-Since` / ETag handling — `304 Not Modified` reuses cache.
- i18n bridge (`i18n_bridge.rs`):
  - Plugin `.ftl` bundle load → keys queryable as
    `plugin.<id>.<key>`.
  - Unload → keys disappear.
  - Hot-reload on file change.

**`fern-plugins-wasm`:**

- A "hello world" WASM plugin (built as part of test fixtures)
  loads, initialises, registers a contribution, contributes
  successfully, and shuts down cleanly.
- Capability enforcement: a WASM plugin attempting an undeclared
  network call observes `not-permitted` and does not actually
  reach the network (verified with a no-network-bound test
  environment).
- Resource handle accounting: widget resources are reference-counted
  correctly; no leaks after instance teardown.
- Trap recovery: a plugin that traps during `init_core` surfaces
  the error and leaves the instance in a clean state.

**`fern-plugins-python`:**

- A "hello world" Python plugin (in test fixtures) round-trips
  through PyO3 → loads, initialises, contributes, shuts down.
- Capability advisory: an undeclared call raises
  `fern_host.NotPermitted`.
- **Per-plugin GIL isolation**: two plugins simultaneously installed,
  one running a tight CPU loop, the other servicing host callbacks
  on a 100 ms cadence. The callback plugin's response time stays
  bounded (no GIL contention from the looping plugin). This is the
  load-bearing test for the sub-interpreter architecture.
- **Per-call timeout enforcement**: a plugin handler that ignores
  the budget receives `KeyboardInterrupt`; if it ignores that too,
  the worker thread is dropped and the plugin marked crashed.
- **Crash containment between sub-interpreters**: plugin A throws
  uncaught; plugin B continues running unaffected.
- **C-extension multi-phase-init validator**: a fixture plugin
  declaring a wheel without PEP 489 support fails install with a
  clear error.
- CPython embedding boot test: the runtime initialises CPython 3.13
  cleanly on each supported platform CI lane.
- Sub-interpreter creation / teardown: 10 plugins instantiated in
  series, then dropped; no leaked threads, no leaked memory above
  baseline.

**`fern-plugins-widgets`:**

- Manager widget renders correctly with mock plugin list (sandboxed
  + trusted + mixed).
- Consent dialog (both variants) renders with correct copy.
- Install wizard happy path + cancel path.
- Settings host renders a mock plugin settings page from a tier-1
  settings_page spec.

### Integration tests

`crates/fern-plugins-core/tests/integration/`:

- **End-to-end install (sandboxed)** — mock app, mock WASM runtime,
  mock plugin bundle: install → enable → contribution appears →
  uninstall → contribution disappears.
- **End-to-end install (trusted)** — same with Python.
- **Multi-window per-window lifecycle** — open 2 windows of mock
  app, enable a per-window plugin in both, verify 2 instances,
  close 1, verify other still works.
- **Shared plugin across windows** — open 2 windows, enable a
  shared plugin, verify 1 instance, verify both windows see
  the contribution.
- **Crash recovery** — plugin's `init_core` returns error: plugin
  marked failed, contributions not registered, manager shows
  error state. User retries: same plugin re-enables successfully.
- **Settings persistence across plugin reload** — set a plugin's
  settings, disable + re-enable plugin, verify settings preserved.
- **Plugin shortcut cleanup on uninstall** — register a plugin
  with shortcuts, uninstall, verify `ShortcutRegistry` has no
  remaining `plugin.<id>.*` entries.

### E2E tests (against `examples/plugins_demo`)

`examples/plugins_demo/tests/e2e/`:

- Demo app boots with all four bundled plugins enabled.
- Each plugin's contributions render in correct slots.
- Plugin bus delivery: trigger an event in the character-db plugin's
  core, verify the bus message reaches both its UI parts.
- Cross-runtime intent dispatch: fire a command registered by a
  WASM plugin → its `command-handler` runs.
- Locale switch updates plugin labels.
- Plugin manager flow: open manager, disable a plugin, verify its
  contributions disappear; re-enable, verify they return.

### Mock runtimes

`fern-plugins-core` ships a `MockRuntime` + `MockRuntimeInstance`
that don't actually execute WASM or Python — they just record
calls and return scripted responses. Used by:

- Core's own unit tests (no real interpreter needed).
- App authors testing their `DocumentProvider` impl against
  scripted plugin behaviour.
- Plugin authors who want to test their host-facing logic
  without bringing up a full host app.

Plugin authors building real plugins use **real test harnesses
per-runtime**:

- WASM plugins compile to `wasm32-wasip2` and run under
  `wasmtime` against a `TestHost` provided by
  `fern-plugin-sdk-wasm::test_harness`.
- Python plugins run under a real embedded CPython provided by
  `fern-plugin-sdk-python.testing` with mocked `fern_host` module.

### CI gates

Every PR runs:

- All unit tests on Linux x86_64.
- WASM-specific tests on Linux (requires `wasm32-wasip2` toolchain).
- Python-specific tests on Linux + macOS arm64 + Windows (CPython
  embedding cross-platform sanity).
- E2E tests headlessly against `plugins_demo`.

## 23. Documentation deliverables

The docs ship alongside v1 — not deferred to a follow-up. Plugin
authors need them to build anything; app authors need them to
integrate.

### App-developer docs

`docs/plugins/`:

- **`overview.md`** — high-level model, dual-runtime architecture,
  Tier-1 vs Tier-2, security stance per runtime. Entry point for
  app developers deciding whether to add plugin support.
- **`integration-guide.md`** — `PluginsBundle` configuration,
  `DocumentProvider` implementation, slot widget composition,
  `PluginManagerWidget` placement. Worked example: a minimal app
  adopting both runtimes.
- **`contribution-models.md`** — one section per contribution shape
  (status segment, settings page, list panel, tree panel, wizard,
  freeform panel, command, menu item) with a real-world example
  and the host-side render behaviour. Read by app authors picking
  what to expose and by plugin authors picking what to contribute.
- **`shortcuts-and-i18n.md`** — how plugin shortcuts integrate with
  `ShortcutSettings`, how plugin `.ftl` bundles load and namespace.
  Read by app authors who need to surface plugin shortcuts
  alongside their own.
- **`registry.md`** — `PluginRegistry` trait, built-in
  implementations (`LocalFileRegistry` + `StaticJsonRegistry`), the
  JSON catalog schema, how to host a catalog on GitHub Pages /
  Cloudflare Pages / any static CDN, signing keys configuration.

### Plugin-author docs

`docs/plugins/wasm/`:

- **`getting-started.md`** — installing the `wasm32-wasip2` toolchain,
  `fern-plugin-sdk-wasm` setup, hello-world plugin walkthrough,
  building a `.fernplugin` bundle, sideloading into a host app.
- **`wit-interface-reference.md`** — full wit interface
  documentation, every type, every function, every error variant,
  every capability gate behaviour.
- **`multi-part-plugins.md`** — designing plugins with `[core]`,
  UI parts, plugin-bus communication. The character-db example
  walked through end-to-end.
- **`freeform-ui-guide.md`** — using the `ui` interface to build
  widget trees, the `fern_plugin!` macro, the closure / intent
  dispatch pattern, known limitations and workarounds.
- **`packaging.md`** — bundle format, manifest schema, signing,
  `cargo fern-plugin pack` (a planned helper command).

`docs/plugins/python/`:

- **`getting-started.md`** — `pip install fern-plugin-sdk`,
  hello-world plugin walkthrough, `.fernplugin` bundle layout
  for Python plugins.
- **`fern-host-api-reference.md`** — full reference for the
  `fern_host` module exposed by the runtime: `db`, `settings`,
  `network`, `bus`, `tr`, `notify`, every function and exception.
- **`decorators-reference.md`** — `@core`, `@panel`, `@status`,
  `@settings_page`, `@menu_item`, `@on_intent`,
  `@on_text_changed`, every decorator with arguments and
  semantics.
- **`builders-reference.md`** — the context-manager DSL: every
  builder, what it desugars to, signal binding patterns.
- **`multi-part-plugins.md`** — Python equivalent of the WASM
  guide.
- **`packaging.md`** — bundle layout, dependency management
  (vendored wheels vs ambient `pip`), signing.

### Cross-cutting docs

`docs/plugins/`:

- **`manifest-reference.md`** — complete TOML schema. The
  authoritative reference for every field. Generated partly from
  schema annotations in `fern-plugins-core::manifest`.
- **`capability-reference.md`** — every capability, sandboxed vs
  trusted semantics, install-time prompts.
- **`error-handling.md`** — plugin error categories, propagation
  flow, `ErrorContext` schema, logging sinks (in-memory, on-disk,
  tracing, dev-stderr), retry policy table, user recovery actions.
  For both app developers (configuring `PluginsBundle::retry_policy`,
  `error_log_retention`, `notification_policy`) and plugin authors
  (using `fern_host.fatal_error` / `report_diagnostic`,
  understanding what gets captured in `ErrorContext`).
- **`security-model.md`** — sandboxed vs trusted contract, what
  guarantees the framework provides per runtime, threat model.
  Explicitly covers the v1 trusted-plugin gap (in-process, no
  structural sandbox), the eight defence-in-depth mitigations
  shipped in v1, and the v2 per-process commitment. For app
  developers deciding whether to enable the trusted runtime.
- **`trusted-plugin-security.md`** — operational guide for users
  of trusted-plugin-enabled apps. What "trusted" means, what the
  activity log shows, how to spot a misbehaving plugin, how to
  configure `reject_in_process_trusted` to lock things down. For
  end users (not developers).
- **`testing-plugins.md`** — per-runtime test harness patterns,
  mock host fixtures, integration test recipes.
- **`migration-and-versioning.md`** — wit semver, trust escalation,
  scope migration, breaking-change policy.

### Format and style

- All docs follow the existing `docs/` style (used by `docs/settings.md`,
  `docs/multi-window.md`, etc.) — markdown, code blocks with
  language tags, runnable examples where possible, `[`text`](`url`)`
  internal references.
- `docs/SUMMARY.md` is updated to include all new pages.
- `CLAUDE.md` gets a short pointer to `docs/plugins/overview.md`
  in the implementation status / partial section.

## 24. Deferred to follow-up plans

These items came up during design but are scoped out of v1 to keep
the plan tractable. Each will get its own plan when prioritized.

1. **`decorations-plan.md`** — Editor decorations as a third
   contribution shape. Modifications to `RichTextEditor` to query
   a `DecorationSource` trait; `PluginDecorationSource` impl that
   queries the contribution registry; conflict resolution (z-order,
   layered rendering); per-plugin decoration enable/disable.
2. **`plugins-v2-data-sources.md`** — Pull-based resource projection
   for `ListView` / `TreeView` / `TableView`. Wit `list-source` /
   `tree-source` / `table-source` resources with `row-count` /
   `row-at(idx)` / `on-changed` semantics. Python equivalent as
   iterator-based providers.
3. **`plugins-marketplace-plan.md`** — Full first-party plugin
   marketplace if/when warranted. Search, ratings, comments,
   curation pipeline, paid plugins, browse UI, signed registry
   documents, abuse reporting. **NOT deferred:** the minimum
   discovery primitive (`StaticJsonRegistry`, § 15) ships in v1.
   It's enough to bootstrap a community-run plugin catalog hosted
   on GitHub Pages or any static file host. The full marketplace
   builds on the same `PluginRegistry` trait, so adding it later
   doesn't break existing plugin authors or app integrators.
4. **`plugins-out-of-process-plan.md`** — **Per-process trusted
   plugins** become the default for the trusted runtime in v2.
   This is the structural fix for the v1 trusted-plugin security
   gap (see § 12 mitigations and § 19 known limitations). Scope
   includes: per-platform process model (XPC / pipes / named
   pipes), IPC layer for host API + plugin bus, true OS
   sandboxing per platform, crash isolation including
   C-extension segfaults, `[runtime].in_process = true` opt-in
   for plugins that need high-frequency host calls.
   Sandboxed (WASM) plugins stay in-process — they have a
   structural sandbox already. **v1 prepares the ground:** the
   `PluginsBundle::reject_in_process_trusted(true)` setter
   ships in v1 as a forward-compatible no-op (currently rejects
   all trusted plugins since out-of-process doesn't exist yet)
   so security-conscious apps can express intent today and
   benefit automatically when v2 lands.
4. **`plugins-testing-plan.md`** — Per-runtime test harnesses, mock
   host fixtures, integration test patterns. App-defined for v1;
   patterns will emerge from real plugin development and warrant
   formalization.

## 25. Roadmap

Phased delivery. Each phase is independently shippable; subsequent
phases extend rather than break previous ones. Tests and
documentation ship **inside each phase**, not as a final batch —
phase-N tests and docs are part of phase N.

### Phase 1 — Core scaffolding (no runtime)

**Repository layout.** Phase 1 spans both repositories
(design target #18, § 20 sequencing):

- **In `fern-ui` (main repo, single bounded change):** all
  integration points enumerated in § 20 land first —
  `FernAppBuilder::install_plugins`, lifecycle hooks in
  `WindowManager`, `AppEvent::External` plugin payload variant,
  `I18nManager::load_plugin_bundle` / `unload_plugin_bundle`,
  plugin-shortcut helpers in `ShortcutRegistry`, plugin grouping
  in `ShortcutSettings`, new `CommandPalette` widget +
  `CommandPaletteStyle` trait + `RecipeCommandPaletteStyle` in
  `fern-widgets`.
- **In `fern-plugins` (new repo, created after step above
  lands):** everything below ships here.

Phase 1 deliverables in `fern-plugins`:

- `fern-plugins-core` crate skeleton.
- Bundle format spec + parser.
- Manifest schema + validator.
- `ContributionRegistry` + slot widgets (rendering against mock
  contributions).
- `PluginManagerWidget` (showing mock plugin list).
- `PluginConsentDialog` (both variants, rendered against mock data).
- `DocumentProvider` trait + `NoopDocumentProvider`.
- `MockRuntime` + `MockRuntimeInstance` for testing.
- `PluginsBundle` builder integration in `FernAppBuilder`.
- Tier-1 contribution shape definitions (records, no renderers yet).
- Multi-part lifecycle traits (`Runtime`, `RuntimeInstance`,
  `init_core` / `init_ui` / `shutdown_ui` / `shutdown_core`).
- Plugin-bus core implementation (runtime-agnostic, mock-runtime
  validated).
- (Integration points in `fern-app`, `fern-i18n`,
  `ShortcutRegistry`/`ShortcutSettings`, and the new
  `CommandPalette` in `fern-widgets` are listed in the
  "Repository layout" callout above — they land in `fern-ui`
  before this phase's plugin-side work begins.)
- **Unit tests** for everything above (§ 22).
- **Docs**: `overview.md`, `integration-guide.md`,
  `manifest-reference.md` (initial), `security-model.md` (initial).

Validates: framework / app boundary, manifest validation, manager
UX flows, full lifecycle ordering, plugin-bus semantics. Apps can
integrate plugin scaffolding and test against mock plugins.

### Phase 2 — Sandboxed runtime (WASM)

- `fern-plugins-wasm` crate.
- wit interface files + wit-bindgen setup.
- `WasmRuntime` + `WasmInstance` implementations of phase-1 traits.
- Capability gates (enforced).
- WASI filesystem shim.
- Host-side wit interface implementations (ui, document, storage,
  network, notifications, settings, i18n, contributions, intents,
  lifecycle, plugin-bus, windows).
- `fern-plugin-sdk-wasm` crate with `fern_plugin!` macro and
  test harness.
- Tier-1 declarative renderers in `fern-plugins-widgets`.
- One reference plugin compiled and shipped: `wordcount-wasm-per-window`
  (tier-1 status + settings page) — part of `examples/plugins_demo/`.
- **Unit + integration tests** for the WASM runtime path.
- **Docs**: `wasm/getting-started.md`, `wit-interface-reference.md`,
  `wasm/multi-part-plugins.md`, `wasm/freeform-ui-guide.md`,
  `wasm/packaging.md`, `contribution-models.md`,
  `capability-reference.md`.

Validates: sandboxed runtime end-to-end. Apps can ship sandboxed-only
plugin support.

### Phase 3 — Trusted runtime (Python)

- `fern-plugins-python` crate.
- PyO3 + embedded CPython 3.13 setup (3.13+ required for stable
  per-interpreter GIL — PEP 684).
- **Per-plugin sub-interpreter architecture**: each `PythonInstance`
  owns one CPython sub-interpreter on a dedicated worker thread.
  Validates PyO3's current sub-interpreter support against our use
  cases before any other Python work proceeds — if PyO3 has gaps,
  raise an issue upstream and stub around them in
  `fern-plugins-python`.
- Per-call timeout enforcement via `PyThreadState_SetAsyncExc`
  (default 1 s callback, 5 s shutdown, configurable).
- `PythonRuntime` + `PythonInstance` implementations of phase-1
  traits.
- Capability declarations (advisory enforcement).
- `fern_host` Python module exposing host API.
- C-extension multi-phase-init validator at install time (rejects
  plugins whose declared wheels lack PEP 489 support).
- `fern-plugin-sdk-python` with decorators + context-manager
  builders + `async_task` helper + test harness.
- Cross-platform CPython 3.13 bundling (macOS arm64/x86_64, Windows,
  Linux x86_64/arm64) build infrastructure.
- One reference Python plugin: `character-db-python-per-window`
  (tier-1 list panel + commands + multi-part core) — part of
  `examples/plugins_demo/`.
- **Unit + integration tests** for the Python runtime path,
  including per-plugin GIL isolation tests (one CPU-loop plugin
  doesn't stall another) and callback timeout tests
  (long-running handlers get interrupted cleanly).
- **Cross-platform CI**: CPython embedding + sub-interpreter sanity
  test on all supported platforms.
- **Docs**: `python/getting-started.md`,
  `python/fern-host-api-reference.md`,
  `python/decorators-reference.md`, `python/builders-reference.md`,
  `python/multi-part-plugins.md`, `python/packaging.md`,
  `python/runtime-architecture.md` (the sub-interpreter model and
  its implications for plugin authors).

Validates: trusted runtime end-to-end. Apps can ship dual-runtime
plugin support.

### Phase 4 — Reference demo completion + polish

- Remaining two example plugins: `grammar-wasm-shared`,
  `ai-assistant-python-shared` (tier-2 freeform + multi-part with
  shared scope).
- Demo app polish: realistic mock manuscript, `FakeMarketplace`,
  pre-seeded settings.
- **E2E tests** against `plugins_demo` covering full matrix
  (§ 22).
- Plugin signature verification (Ed25519).
- **Signed-by-default policy** for trusted plugins. Unsigned
  trusted plugins rejected at install unless app explicitly opts
  into `SigningPolicy::AllowUnsigned`.
- **Capability audit log**: per-plugin invocation ring buffer +
  on-disk audit trail for trusted plugins. Manager UI "Activity
  log" tab. Invocation rate-limiting with manager warnings.
- **Network firewall**: host-mediated HTTP client with allowlist
  enforcement at socket layer (`LD_PRELOAD` / `DYLD_INSERT_LIBRARIES`
  / Windows LSP), audit-only fallback when interception fails.
- **Per-platform best-effort OS sandboxing for in-process trusted
  plugins**: Linux per-thread seccomp filter (subprocess + raw
  sockets blocked), macOS host-entitlement inheritance documentation,
  Windows Job Object resource limits.
- **Per-plugin resource limits**: memory cap, CPU quota.
- **Trusted-variant consent dialog with typed `INSTALL`
  confirmation** for plugins declaring escalation capabilities.
- `PluginsBundle::reject_in_process_trusted(true)` forward-compat
  setter (currently refuses all trusted-runtime installs).
- `StaticJsonRegistry` implementation: JSON catalog fetcher, cache
  layer, ETag handling, SHA-256 + signature verification on install,
  background refresh.
- Dev-mode hot-reload (file watcher → re-instantiate without
  consent prompt).
- Update mechanism (download → verify → swap, with restart
  prompt for in-use plugins). Uses `StaticJsonRegistry::check_updates`
  for catalog-driven update detection.
- Crash recovery UX (`PluginCrashNotification` + auto-disable
  after N crashes in M minutes).
- Permissions inspector panel.
- Storage usage display + clear.
- Per-plugin debug log capture (visible in manager).
- **Docs**: `testing-plugins.md`, `migration-and-versioning.md`,
  `shortcuts-and-i18n.md`; round-trip with real plugins to fill
  gaps in earlier docs.

Validates: production-ready surface. Apps can publish plugin support
to end users.

### Phase 5+ — Out of scope for this plan

- Decoration system (`decorations-plan.md`).
- Data-source widget projection (`plugins-v2-data-sources.md`).
- Marketplace (`plugins-marketplace-plan.md`).
- Streaming / large-data APIs.

## 26. Open design questions to revisit

Items where the design is settled enough to ship Phase 1 but where
real-world plugin development is likely to surface improvements:

1. **Bundle compression and signing format.** Currently sketched as
   tar.gz + Ed25519 detached signature. May need to revisit for
   transparency log integration if a marketplace ever materializes.
2. **Plugin-to-plugin communication.** Currently plugins are isolated
   except through host intents. Real ecosystems eventually want a
   "plugin A depends on plugin B" graph. v1 ships no support; if
   demand emerges, add a `requires_plugin` manifest field and a
   `plugins.list_installed()` host call.
3. **Tier-1 spec evolution.** New contribution shapes (palettes,
   chart blocks, conditional row templates) are likely to be
   requested by real plugins. Each one needs a new spec record + a
   new renderer; the framework grows incrementally. Cost is bounded
   because each shape is small.
4. **Trusted-plugin OS sandboxing.** macOS App Sandbox per-plugin
   entitlements are theoretically possible but practically
   difficult; Linux seccomp filters are doable; Windows
   AppContainer requires out-of-process execution. The framework
   ships best-effort hooks; concrete sandboxing strategies are
   per-platform and may grow into their own plan.
5. **Plugin manifest UI mockup detail.** The consent / manager
   mockups in this plan are ASCII sketches; actual visual design
   will go through the design pass before Phase 1 ships.
6. **PyO3 sub-interpreter maturity tracking.** Sub-interpreter
   support in PyO3 is still maturing as of early 2026. Phase 3
   begins with a maturity validation pass: build the smallest
   possible PyO3 + sub-interpreter + per-call-timeout proof-of-
   concept, exercise each fern-plugin host API across the boundary,
   document any PyO3 gaps. If gaps are blocking, the choices are
   (a) contribute upstream, (b) work around in
   `fern-plugins-python`, or (c) fall back to per-process plugins
   for v1 (deferred to v2 currently). The plan assumes path (a) or
   (b); if neither works, revisit before Phase 3 ships.
7. **Per-process plugins are committed as the v2 default for the
   trusted runtime.** The in-process model in v1 cannot provide
   structural security guarantees (see § 12 mitigations and § 19
   limitations). v2 ships per-process trusted plugins as the
   default, with in-process as a labelled opt-in via `[runtime].in_process = true`
   in the manifest (extra-loud consent at install time). Sandboxed
   (WASM) plugins remain in-process by design (the WASM runtime
   is the structural sandbox). The cost: ~50–100 ms IPC overhead
   per host call (acceptable for Tier-1 declarative + intents +
   bus, painful for high-frequency Tier-2 freeform UI rebuilds).
   `PluginsBundle::reject_in_process_trusted(true)` already
   exists in v1 as the forward-compatible setter; v2 makes it a
   security guarantee. Open question for v2 design: whether
   per-process is implemented via XPC (macOS) / sd-bus or pipes
   (Linux) / named pipes (Windows), or by reusing a unified
   IPC abstraction.

## 27. Success criteria

Phase 1 done when:

- An app can `.install_plugins(PluginsBundle::new()...)` and have
  `PluginManagerWidget`, slot widgets, and `PluginConsentDialog`
  render with mock data.
- Slot widgets respond to mock contribution registry changes.
- `MockRuntime` exercises the full multi-part lifecycle
  (`init_core` → `init_ui` → `shutdown_ui` → `shutdown_core`)
  in the correct order with deterministic test output.
- Plugin bus round-trips between mock parts.
- `fern-app` cleanly shuts down per-window plugins on window
  close.
- `fern-i18n` loads and unloads plugin `.ftl` bundles; namespaced
  keys queryable.
- `ShortcutSettings` shows mock plugin shortcuts grouped by
  plugin, rebindable.
- `CommandPalette` opens via `Ctrl+Shift+P`, lists every command
  in `ShortcutRegistry` (app and mock-plugin alike), filters by
  typed input, dispatches the selected command on Enter, dismisses
  on Esc. Renders as a proper `Role::Dialog` with `Role::ListBox`
  for the results.
- All unit tests in § 22 (`fern-plugins-core` row) pass on Linux
  x86_64 CI.
- Phase-1 docs published in `docs/plugins/`.

Phase 2 done when:

- A WASM plugin can be packaged as a `.<ext>` bundle and signed.
- The plugin can be sideloaded via the manager, see the sandboxed
  consent dialog, install, and have its declared contributions
  render in the host.
- A freeform Tier-2 panel built via wit `ui` renders correctly
  in a `PluginPanelSlot`.
- Capability gates physically prevent disallowed operations
  (verified by failing-network-call test).
- `wordcount-wasm-per-window` plugin in `examples/plugins_demo/`
  works end-to-end.
- All unit + integration tests in § 22 (`fern-plugins-wasm` row)
  pass on Linux x86_64 CI with `wasm32-wasip2` toolchain.
- Phase-2 docs (wit reference + WASM author guide) published.

Phase 3 done when:

- A Python plugin can be packaged as a `.<ext>` bundle.
- The plugin can be sideloaded, see the trusted-variant consent
  dialog, install, and have its contributions render.
- Both runtimes coexist in the same app with the same manager UI.
- Cross-platform CPython 3.13 bundling produces working binaries on
  Linux x86_64, macOS arm64, macOS x86_64, Windows x86_64.
- `character-db-python-per-window` plugin in
  `examples/plugins_demo/` works end-to-end.
- All unit + integration tests in § 22 (`fern-plugins-python` row)
  pass on all platform CI lanes — including the GIL-isolation,
  callback-timeout, and crash-containment tests.
- A demo "noisy plugin" (provided as a test fixture) running a
  tight CPU loop does NOT degrade the response latency of other
  Python plugins or the host UI thread, verified by a measured
  latency test in CI.
- Phase-3 docs (Python author guide + `runtime-architecture.md`)
  published.

Phase 4 done when:

- Signed plugins verify on install; **unsigned trusted plugins
  rejected by default**; sandboxed plugins pass with warning;
  policy app-configurable per `PluginsBundle::trusted_signing(...)`
  and `PluginsBundle::sandboxed_signing(...)`.
- Trusted-variant consent dialog requires typed `INSTALL`
  confirmation when the plugin declares `filesystem`, `subprocess`,
  or `ffi` capabilities; standard click-to-install otherwise.
- Plugin manager UI exposes an "Activity log" tab per installed
  plugin showing recent capability invocations with filtering and
  JSON export.
- Rate-limiting on capability denials triggers manager-UI warnings
  at configurable thresholds (verified by stress test of a fake
  plugin that floods denied calls).
- Network calls from trusted plugins to non-allowlisted hosts are
  blocked (verified by integration test with a fixture plugin
  that attempts to reach a blocked URL); when socket interception
  isn't available on the test platform, the audit log records the
  attempt and the test verifies the log entry.
- `PluginsBundle::reject_in_process_trusted(true)` correctly
  refuses to install any trusted-runtime plugin in v1.
- Editing a plugin's source in dev mode triggers automatic
  re-instantiation without consent prompts.
- Crashed plugins surface a notification and auto-disable
  after repeated failures.
- All four bundled example plugins
  (wordcount + grammar + character-db + ai-assistant) work
  end-to-end in `examples/plugins_demo`.
- `StaticJsonRegistry` discovers, lists, downloads, and installs
  plugins from a test JSON catalog hosted in the repo (e.g.
  `examples/plugins_demo/test_registry.json`). The demo app's
  `FakeMarketplace` is replaced by a real `StaticJsonRegistry`
  pointed at the test catalog; install flow exercises hash + 
  signature verification end-to-end.
- E2E tests in § 22 pass against `plugins_demo`.
- Manager UI surfaces dirty-shutdown warnings, storage usage,
  permissions inspection.
- `cargo run -p plugins_demo` boots and exercises the full
  matrix without error.
- All docs in § 23 published; `docs/SUMMARY.md` updated; CLAUDE.md
  pointer added.
