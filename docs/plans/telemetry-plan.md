# Usage Telemetry Plan

Privacy-respecting product analytics for FernUI: which intents fire, which
widgets get instantiated, what app/OS the user runs — never *what* they
typed, opened, or selected. Two interchangeable privacy postures (anonymous
vs. pseudonymous), one schema-first event model, one consent widget,
adapter-based transport. RGPD/GDPR compliant on both paths.

## Context

FernUI today has **zero usage telemetry**. The only existing observability
is `IdleTrace`
([`crates/fern-app/src/app.rs:163`](../../crates/fern-app/src/app.rs#L163-L211)),
an env-var-gated event-loop *perf* probe — different problem entirely.
Architecture doc mentions telemetry only as an *external event source*
feeding the UI loop
([`docs/fern-ui-architecture.md:793`](../fern-ui-architecture.md#L793)),
never as framework self-instrumentation.

The architectural choke point that makes this plan cheap is already in
place: every meaningful user action funnels through `Intent` and
`Action::on_invoke` ([`crates/fern-core/src/intent.rs`](../../crates/fern-core/src/intent.rs),
[`action.rs`](../../crates/fern-core/src/action.rs)). Intent **names** are
dev-authored static strings (`"app.save"` from `#[name = "app.save"]`);
intent **payloads** may carry user data and must never be auto-serialized.
Tapping the dispatch point gives us free coverage of every button, menu
item, shortcut, and gesture-derived action without instrumenting individual
widgets.

### Reference reading

- Mozilla Glean — schema-first telemetry SDK for desktop/mobile.
  [docs](https://mozilla.github.io/glean/book/) — model for our YAML
  manifest and codegen.
- [`docs/plans/settings-plan.md`](settings-plan.md) — `fern-settings`
  crate. Provides `SettingsFile<T>`, `SettingsStore`, `Versioned`,
  `Migrator<T>`, and `AppPaths`. **The consent store, install_id, and
  per-scope toggles ride on top of this** rather than rolling their own
  TOML/JSON persistence. Dependency direction (`fern-settings` depends
  on `fern-core`) drives the crate split in §3.
- CNIL Sheet n°16 — *Use analytics on your websites and applications*.
  [cnil.fr/en/sheet-ndeg16](https://www.cnil.fr/en/sheet-ndeg16-use-analytics-your-websites-and-applications)
  — the current canonical guidance (replaces the older
  "mesure d'audience" page).
- CNIL self-assessment tool for the consent-exempt audience-measurement
  path (in force since 1 January 2026 — the previous certified-tools list
  was retired). The controller (app developer) must produce and retain
  a self-assessment record.
- EDPB Guidelines 2/2023 on Article 5(3) ePrivacy (final version
  adopted 7 October 2024).
  [edpb.europa.eu (PDF)](https://www.edpb.europa.eu/system/files/2024-10/edpb_guidelines_202302_technical_scope_art_53_eprivacydirective_v2_en_0.pdf)
  — confirms 5(3) is technology-agnostic; storage *or access* on
  terminal equipment triggers consent unless an exemption applies. Local
  processing that never leaves the device is out of scope.
- CNIL Sheet n°14 — data retention. Cookie/tracker lifespan ≤13 months;
  analytics data retention ≤25 months (two distinct ceilings).
  [cnil.fr/en/sheet-ndeg14](https://www.cnil.fr/en/sheet-ndeg14-define-data-retention-period)
- EU-US Data Privacy Framework — upheld by the General Court on 3
  September 2025 (T-553/23, *La Quadrature du Net*); a Schrems-III-style
  challenge before the CJEU is anticipated. Treat as conditionally valid
  and prefer EU-resident processors regardless.
- GDPR Art. 12(3) — data-subject-rights response within one month.
- GDPR Art. 17 (right to erasure) — the user-facing erase button.
- GDPR Art. 28 — controller/processor relationship; DPA required when
  using a hosted analytics backend in pseudonymous mode.
- Plausible data policy.
  [plausible.io/data-policy](https://plausible.io/data-policy) — reference
  for the anonymous-by-design wire format.
- PostHog DPA + EU Cloud (Frankfurt, AWS eu-central-1).
  [posthog.com/dpa](https://posthog.com/dpa),
  [posthog.com/blog/posthog-cloud-eu](https://posthog.com/blog/posthog-cloud-eu).

## Design targets

1. **Two modes, runtime-selectable.** Anonymous-by-design and
   pseudonymous (with stable install_id) are both first-class. A
   single config field picks one; the same `UsageReporter` trait, the
   same event taxonomy, the same widget. Switching modes after first
   run is a one-shot wipe-and-reseed, not a migration.
2. **Schema-first events.** Every event and every property is declared in a
   YAML manifest, type-checked at compile time, validated server-side. No
   ad-hoc string keys. Glean-style.
3. **Allowlisted properties only.** The type system makes it impossible to
   accidentally serialize an `Intent` payload, a file path, or a
   `Display`-impl of user data. Properties are scalars from a closed enum.
4. **Always-available consent widget.** `PrivacySettings` lives in
   `fern-widgets` next to `ShortcutSettings`
   ([`crates/fern-widgets/src/shortcut_settings.rs`](../../crates/fern-widgets/src/shortcut_settings.rs)),
   embeddable in any `Dialog` or settings panel.
5. **Off by default.** No reporter unless the app builder installs one. No
   emission unless consent is granted. EU-friendly out of the box.
6. **Adapters, not a server.** FernUI defines the wire format and ships
   reference adapters (Plausible, PostHog, OTLP). No FernUI-operated
   ingestion service.
7. **Erase button is real.** In pseudonymous mode, "Erase my data from the server" is
   a one-click HTTP call keyed by install_id. In anonymous mode it is hidden with
   an explanation.
8. **Reuse `fern-settings` for all persistence.** Consent state, install_id,
   and per-scope toggles all ride on `SettingsFile<T>` / `SettingsStore`
   from [`fern-settings`](settings-plan.md). No bespoke JSON files, no
   ad-hoc atomic-write code. Migration uses `Versioned` + `Migrator<T>`.
   Path resolution uses `AppPaths`. The on-disk event queue is the only
   thing that doesn't fit (typed K/V, not TOML) — it uses
   [`redb`](https://crates.io/crates/redb) (pure-Rust embedded database)
   under `AppPaths::data_dir()`. Pure-Rust matters: FernUI's foundational
   posture is "no C deps", and an event queue is K/V, not relational —
   redb fits both constraints, SQLite would import a C toolchain just
   for one component.

## 1. The two modes

| | Anonymous mode | Pseudonymous mode |
|---|---|---|
| Client-side identifier transmitted? | **No.** Adapter sends no install_id, no UUID, no fingerprint. The server may derive a daily-rotating session hash from request metadata (IP+UA+server-side daily salt, Plausible-style) — **server's responsibility, not the SDK's**. | Yes — random UUID v4 generated at first run, stored in app data dir, sent with every event. |
| GDPR Art. 6 lawful basis | Legitimate interest (Art. 6(1)(f)) + LIA. | Explicit consent (Art. 6(1)(a)). |
| ePrivacy Art. 5(3) trigger | No client-side storage *or access* on terminal equipment for analytics purposes → 5(3) does not apply. | install_id stored locally → 5(3) applies → consent required. |
| Consent banner required (ePrivacy + CNIL) | No, **conditional on the controller completing a CNIL self-assessment** (in force since 1 Jan 2026; the certified-tools list was retired). The exemption is not automatic by adapter choice. | Yes, via `PrivacySettings` first-run flow. |
| Right to erasure (Art. 17) applies? | Only if the server-side session hash + retained metadata is, in practice, re-identifiable. If genuinely anonymous (Recital 26 standard) the data is out of scope. Defensive position: the controller documents this in the LIA. | Yes — erase button operative. |
| Retention | Server-side data ≤25 months (CNIL Sheet n°14). No client-side identifier to rotate. | install_id rotation ≤13 months (treated as a tracker analog); server-side data ≤25 months. Both configurable per adapter. |
| DPA required (Art. 28) | If using a hosted backend (e.g. Plausible Cloud), yes. Self-hosted: no. | If using a hosted backend (PostHog Cloud EU, etc.), **yes** — the controller must sign the processor's DPA before going live. |
| Reference adapter | `fern-analytics-plausible` | `fern-analytics-posthog` |
| Analytics depth | Counts, top events, country, device, version | Funnels, retention, cohorts, per-install paths |

The path is chosen by the app via
`FernAppBuilder::usage_reporter(...)` — the adapter type *is* the path.
Both paths emit the same event names with the same property schemas; only
the `install_id` field differs (`None` vs. `Some(uuid)`).

Switching mode = the consent widget shows a confirmation, wipes the local
event queue + install_id (if any), calls `erase_remote_data()` whenever
leaving pseudonymous mode (so any pseudonymous server records are deleted before
the install_id is forgotten — once the UUID is gone locally, the user
loses the only handle to their server data), then resets `ConsentState`
to `Unknown`.

## 2. Event manifest — YAML, schema-first

All events declared in `crates/fern-core/telemetry/events.yaml`:

```yaml
schema_version: 1
retention_days: 395   # ~13 months

events:
  intent.dispatched:
    description: An Action was invoked via Intent dispatch.
    bug: SKR-1
    expires: never
    category: intent
    properties:
      name: { type: string, allowlist: dev_static }
      source: { type: enum, values: [shortcut, menu, handler, programmatic, accessibility] }

  lifecycle.app_started:
    description: Process boot, after first window open.
    category: lifecycle
    properties:
      app_version: { type: string }
      fern_ui_version: { type: string }
      os: { type: enum, values: [linux, macos, windows, freebsd, other] }
      arch: { type: enum, values: [x86_64, aarch64, other] }
      locale: { type: string, max_len: 10 }
      theme_kind: { type: enum, values: [light, dark, custom] }

  lifecycle.app_exited:
    category: lifecycle
    properties:
      session_duration_bucket: { type: enum, values: [under_1m, m1_5, m5_30, m30_2h, h2_8, over_8h] }

  window.opened:
    category: lifecycle
    properties:
      kind: { type: enum, values: [main, dialog, popover, secondary] }

  window.closed:
    category: lifecycle
    properties:
      kind: { type: enum, values: [main, dialog, popover, secondary] }

  widget.census:
    description: Periodic histogram of widget types in the live arena.
    category: census
    cadence: hourly
    properties:
      types: { type: histogram_string_to_u32 }

property_types:
  string:
    max_len_default: 64
  dev_static:
    description: |
      Value must be a 'static &str defined in source code, never user input.
      Validated by codegen; the emit() function won't accept &String.
```

The `dev_static` property kind enforces the privacy rule at the type
level — only `&'static str` literals fit. An app author can't pass a
`String` from a `TextField` because the generated emit signature won't
accept it.

A `build.rs` in `fern-core` parses this manifest and generates
`crates/fern-core/src/telemetry/generated.rs` containing:

- One typed `pub fn emit_<event_name>(reporter: &dyn UsageReporter, ...)`
  per event, with strongly-typed parameter list matching the schema.
- A `pub static EVENT_SCHEMA: &[EventSchema]` for runtime validation by
  adapters (server-side schema check) and for the `PrivacySettings`
  "Inspect data sent" tab.

App-defined events live in the *application's* `events.yaml` and are
codegen'd the same way via a `fern-telemetry-codegen` proc macro
(`include_telemetry_schema!("path/to/events.yaml")`). Schemas merge.

## 3. Library API — `fern-core` + `fern-telemetry`

The split follows the dependency graph. `fern-core` is foundational and
cannot reach into `fern-settings` (which already depends on `fern-core`).
So pure trait/type surface stays in `fern-core`; everything that touches
disk, codegen, or the consent flow lives in a new `fern-telemetry` crate
that sits between `fern-settings` and `fern-widgets`.

```text
crates/fern-core/src/telemetry/
    mod.rs              # public surface, re-exports
    reporter.rs         # UsageReporter trait (no I/O — pure trait + types)
    event.rs            # Event, EventCategory, PropValue, EventSchema
    intent_tap.rs       # the dispatch-side hook called from event_dispatch_impl.rs

crates/fern-telemetry/                     # NEW crate
    Cargo.toml          # depends on fern-core + fern-settings + serde + uuid + redb
    build.rs            # YAML manifest → generated emit_* fns
    telemetry/
        events.yaml     # the framework-level event schema
    src/
        lib.rs          # public surface, re-exports
        consent.rs      # ConsentStore on top of SettingsFile<ConsentFile>
        install_id.rs   # pseudonymous mode: SettingsFile<InstallIdFile> + 13-mo rotation
        queue/
            mod.rs      # re-exports + EventQueue trait
            mem.rs      # InMemoryEventQueue (Mutex<VecDeque<OwnedEvent>>)
            persistent.rs  # PersistentEventQueue — redb-backed, survives restart
        manifest.rs     # YAML loader + validator (build.rs side)
        generated.rs    # codegen output (gitignored)
        scopes.rs       # SettingsKey<bool> constants for each ConsentScope toggle
```

Dependency graph:

```text
fern-core ──► fern-data ──► fern-settings ──► fern-telemetry ──► fern-widgets
       └────────────────────────────────────────────┘
                  (UsageReporter trait + Event types)
```

`fern-ui` re-exports `fern_telemetry as telemetry`. App-defined event
schemas use the same `include_telemetry_schema!` proc macro from
`fern-telemetry-codegen` and merge into the generated module.

### `UsageReporter` trait — `reporter.rs`

```rust
pub trait UsageReporter: Send + Sync + 'static {
    /// Invoked synchronously from any thread. Adapters MUST NOT block the
    /// caller (queue and return). Drops events when `consent != Granted`.
    fn record(&self, event: &Event<'_>);

    /// Best-effort drain. Called on graceful exit. **Not** called on
    /// consent revocation — see `discard_pending()`.
    fn flush(&self) -> FlushFuture { Box::pin(async { Ok(()) }) }

    /// Drop the on-disk queue without sending. Called when consent is
    /// revoked, when the path is switched, or when the user clicks
    /// "Erase my data". Once consent is `Denied` or `Unknown`, the
    /// buffered events are no longer permitted to leave the device.
    fn discard_pending(&self) -> DiscardFuture;

    /// GDPR Art. 17. Pseudonymous mode: send DELETE keyed by install_id; clear
    /// local queue. Anonymous mode: noop, returns `Err(ErasureUnsupported)` so
    /// the widget can hide the button.
    fn erase_remote_data(&self) -> ErasureFuture;

    /// GDPR Art. 15 + 20. Pseudonymous mode: fetch all server-side events for
    /// this install_id as a `RemoteDataExport` (JSON-serializable, schema-tagged).
    /// Anonymous mode: returns `Err(FetchUnsupported)` so the widget hides the
    /// button — there's nothing linkable to fetch.
    fn fetch_remote_data(&self) -> FetchFuture;

    /// `Some(uuid)` in pseudonymous mode, `None` in anonymous mode. Surfaced in the consent
    /// widget so the user can copy it.
    fn install_id(&self) -> Option<&str>;

    /// `"plausible"`, `"posthog"`, `"otlp"`, `"custom"`. Shown in the
    /// "what gets sent" UI.
    fn adapter_name(&self) -> &'static str;

    /// Endpoint URL displayed verbatim in the consent widget.
    fn endpoint(&self) -> &str;

    /// Drives the consent widget toggle group: which scopes does this
    /// adapter actually use?
    fn supported_scopes(&self) -> ConsentScope;
}

pub struct RemoteDataExport {
    pub install_id: String,
    pub fetched_at: SystemTime,
    pub adapter: &'static str,
    pub endpoint: String,
    pub schema_version: u32,
    pub events: Vec<RemoteEvent>,         // server-side records
    pub server_metadata: serde_json::Value, // adapter-specific (PostHog person props, etc.)
}

pub struct RemoteEvent {
    pub name: String,
    pub timestamp: SystemTime,
    pub properties: BTreeMap<String, serde_json::Value>,
}

pub enum FetchError {
    FetchUnsupported,                     // anonymous mode
    FetchUnsupportedByBackend,            // OTLP without configured query endpoint
    Network(io::Error),
    Server { status: u16, body: String },
    QuotaExceeded,                        // adapter rate-limited the export
}
```

The export is a portable, self-describing JSON document — when written to
disk it's a complete RGPD Art. 20 portability artifact (format + schema
version + endpoint + records).

### Event types — `event.rs`

```rust
pub struct Event<'a> {
    pub name: &'static str,                  // dev_static — must be a literal
    pub category: EventCategory,
    pub timestamp: SystemTime,
    pub install_id: Option<&'a str>,
    pub session_id: &'a str,                 // per-process random, not persisted
    pub schema_version: u32,
    pub props: &'a [Prop<'a>],
}

pub enum EventCategory { Intent, Lifecycle, Navigation, Census, Custom }

pub struct Prop<'a> { pub key: &'static str, pub value: PropValue<'a> }

pub enum PropValue<'a> {
    StaticStr(&'static str),                 // for dev_static
    BoundedStr(&'a str),                     // length-checked at codegen-call site
    U32(u32),
    I64(i64),
    F64Bucket(F64Bucket),                    // pre-bucketed; raw f64 not allowed
    Bool(bool),
    Enum { variant: &'static str },
    HistogramStrU32(&'a [(&'static str, u32)]),
}
```

Note: there is no `String` variant. Anything dynamic must already be
length-bounded by the schema, and arbitrary user-provided strings simply
have nowhere to go.

### Consent state — `fern-telemetry::consent`

The pure types live in `fern-core::telemetry::reporter`:

```rust
// fern-core
pub enum ConsentState {
    Unknown,                                  // pre-decision; widget prompts
    Granted(ConsentScope),
    Denied,
}

pub struct ConsentScope {
    pub anonymous_metrics: bool,              // anonymous mode always uses this
    pub crash_reports: bool,
    pub feature_flags: bool,
    pub session_recording: bool,              // not implemented yet, reserved
}
```

Persistence lives in `fern-telemetry::consent`, riding on
`SettingsFile<ConsentFile>` from `fern-settings`:

```rust
// fern-telemetry
use fern_settings::{SettingsFile, Versioned, Migrator, AppPaths};

#[derive(Serialize, Deserialize, Default)]
pub struct ConsentFile {
    pub version: u32,                         // ConsentFile schema version
    pub state: PersistedConsentState,
    pub decided_at: Option<SystemTime>,
    pub consented_to_event_schema: u32,       // EVENT schema version at consent time
}

#[derive(Serialize, Deserialize, Default)]
pub enum PersistedConsentState {
    #[default] Unknown,
    Granted { scope: ConsentScope },
    Denied,
}

impl Versioned for ConsentFile {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 { self.version }
    fn set_version(&mut self, v: u32) { self.version = v; }
}

pub struct ConsentStore {
    file: SettingsFile<ConsentFile>,          // fern-settings handle (Rc'd)
    state: Signal<ConsentState>,              // bridge for widgets
    current_event_schema: u32,                // codegen'd constant
}

impl ConsentStore {
    pub fn open(
        paths: &AppPaths,
        delay: Duration,
        current_event_schema: u32,
    ) -> Result<Self, SettingsFileError> {
        let migrator = Migrator::<ConsentFile>::new();   // future bumps register here
        let file = SettingsFile::load(
            paths.config_file("telemetry-consent"),
            delay,
            &migrator,
        )?;

        // Re-prompt rule: if the user consented to an older EVENT schema
        // version, reset to Unknown. mutate() in one round-trip.
        let snap = file.snapshot();
        if snap.consented_to_event_schema < current_event_schema {
            file.mutate(|f| {
                f.state = PersistedConsentState::Unknown;
                f.decided_at = None;
                f.consented_to_event_schema = current_event_schema;
            })?;
        }

        let state = Signal::new(ConsentState::from(file.snapshot().state));
        Ok(Self { file, state, current_event_schema })
    }

    pub fn state_signal(&self) -> Signal<ConsentState> { self.state.clone() }

    /// Granular per-scope view. Used by the consent widget toggles.
    /// Each call returns a derived `Signal<bool>` that updates whenever
    /// the underlying state changes. Setting from the toggle calls
    /// `set_scope(...)` which round-trips through the SettingsFile.
    pub fn scope_signal_anonymous_metrics(&self) -> Signal<bool> { ... }
    pub fn scope_signal_crash_reports(&self)      -> Signal<bool> { ... }
    pub fn scope_signal_feature_flags(&self)      -> Signal<bool> { ... }

    pub fn grant(&self, scope: ConsentScope) {
        let mut snap = self.file.snapshot();
        snap.state = PersistedConsentState::Granted { scope: scope.clone() };
        snap.decided_at = Some(SystemTime::now());
        snap.consented_to_event_schema = self.current_event_schema;
        self.file.replace(snap);                  // schedules debounced flush
        self.state.set(ConsentState::Granted(scope));
    }

    pub fn deny(&self) { ... }                    // sets PersistedConsentState::Denied
    pub fn withdraw(&self) { ... }                // alias for deny() + triggers reporter.discard_pending()
    pub fn set_scope(&self, mutation: impl FnOnce(&mut ConsentScope)) { ... }
}
```

Why the indirection through `SettingsFile<T>`:

- **Atomic writes for free** — `fern-settings`'s `DebouncedWriter` already
  does write-temp + rename with `tempfile::NamedTempFile::persist`. No
  partial writes if the process is killed mid-flush.
- **Migration for free** — `Versioned` + `Migrator<ConsentFile>` handles
  any future `ConsentFile` schema bump. Distinct from the *event* schema
  bump tracked by `consented_to_event_schema`.
- **Shutdown flush for free** — `FernAppBuilder` already wires
  `SettingsStore::flush_now()` to the shutdown path; `SettingsFile<T>`
  uses the same writer infrastructure.
- **Path resolution for free** — `AppPaths::config_file("telemetry-consent")`
  produces `~/.config/<app>/telemetry-consent.toml` on Linux,
  `%APPDATA%\<app>\telemetry-consent.toml` on Windows, the right
  `Application Support` path on macOS.

The granular `scope_signal_*` methods are how the widget binds each
toggle to a `Signal<bool>` — same pattern as `SettingsStore::signal_for`
but derived from `ConsentFile.state` rather than a dynamic K/V cache.

### Install ID (pseudonymous mode) — `fern-telemetry::install_id`

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct InstallIdFile {
    pub version: u32,
    pub uuid: String,
    pub generated_at: SystemTime,
}

impl Versioned for InstallIdFile { ... }

pub struct InstallId { file: SettingsFile<InstallIdFile> }

impl InstallId {
    pub fn open_or_create(
        paths: &AppPaths,
        delay: Duration,
    ) -> Result<Self, SettingsFileError> {
        let migrator = Migrator::<InstallIdFile>::new();
        let file = SettingsFile::load(
            paths.config_file("telemetry-install-id"),
            delay,
            &migrator,
        )?;
        let now = SystemTime::now();

        // Empty UUID OR rotation overdue (>13 months) → regenerate.
        let snap = file.snapshot();
        let needs_rotation = snap.uuid.is_empty()
            || now.duration_since(snap.generated_at).map_or(true, |d| d > Duration::from_days(395));
        if needs_rotation {
            file.mutate(|f| {
                f.uuid = Uuid::new_v4().to_string();
                f.generated_at = now;
            })?;
        }
        Ok(Self { file })
    }

    pub fn get(&self) -> String { self.file.snapshot().uuid }

    /// Wipe the local UUID (called by `discard_pending` on revoke + by
    /// `erase_remote_data` after the server delete completes).
    pub fn clear(&self) -> Result<(), SettingsFileError> {
        self.file.replace(InstallIdFile::default())
    }
}
```

Rotation check runs on `open_or_create()` (i.e. every app start). The
rotation must be preceded by a successful `erase_remote_data()` call —
otherwise the user loses the only handle to their server data. The
`UsageReporter` impl orchestrates this in pseudonymous mode; `InstallId::clear()`
does no I/O of its own beyond the local file.

Anonymous mode constructs no `InstallId` at all — `install_id()` returns `None`
unconditionally.

### Intent-bus tap

Single insertion point in
[`crates/fern-core/src/widget_tree/event_dispatch_impl.rs`](../../crates/fern-core/src/widget_tree/event_dispatch_impl.rs)
near `Action` invocation. After the action fires:

```rust
if let Some(reporter) = self.usage_reporter() {
    telemetry::generated::emit_intent_dispatched(
        reporter,
        intent.kind_name(),                   // &'static str from IntentKind
        IntentSource::from_dispatch_origin(origin),
    );
}
```

`reporter.record()` short-circuits on `ConsentState != Granted`, so this
is safe to leave wired regardless of consent state.

### `TelemetryBundle` / `TelemetryExt` — bundle/ext pattern

Mirrors [`SettingsBundle`](../../crates/fern-settings/src/bundle.rs) and
[`SettingsExt`](../../crates/fern-settings/src/ext.rs) exactly. App
authors construct a `TelemetryBundle` declaratively, the eventual
`FernAppBuilder::telemetry(bundle)` integration calls
`bundle.open(paths, settings_store)` during startup, registers the
returned services into `app_state`, and apps reach them via the
`TelemetryExt` trait.

```rust
// crates/fern-telemetry/src/bundle.rs

#[derive(Clone)]
pub struct TelemetryBundle {
    anonymous: Option<Arc<dyn UsageReporter>>,
    pseudonymous: Option<Arc<dyn UsageReporter>>,
    default_mode: TelemetryMode,
    event_schema_version: u32,
    debounce: Duration,
    data_processor_name: Option<String>,
    privacy_policy_url: Option<String>,
    data_residency_region: DataResidencyRegion,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TelemetryMode {
    /// No client identifier transmitted. CNIL consent-exempt under the
    /// audience-measurement self-assessment, GDPR Art. 6(1)(f) basis.
    /// Adapter example: `fern-analytics-plausible`.
    Anonymous,
    /// Stable per-install UUID transmitted with every event. Requires
    /// explicit consent under GDPR Art. 6(1)(a) + ePrivacy 5(3).
    /// Adapter example: `fern-analytics-posthog`.
    Pseudonymous,
}

#[derive(Copy, Clone, Debug)]
pub enum DataResidencyRegion { EU, US, Other }

impl TelemetryBundle {
    pub fn new(event_schema_version: u32) -> Self;

    pub fn with_anonymous(mut self, reporter: impl UsageReporter + 'static) -> Self;
    pub fn with_pseudonymous(mut self, reporter: impl UsageReporter + 'static) -> Self;
    pub fn with_default_mode(mut self, p: TelemetryMode) -> Self;
    pub fn with_debounce(mut self, d: Duration) -> Self;

    /// Surfaced verbatim in the `PrivacySettings` Art. 13 notice.
    pub fn with_data_processor_name(mut self, name: impl Into<String>) -> Self;
    pub fn with_privacy_policy_url(mut self, url: impl Into<String>) -> Self;
    pub fn with_data_residency_region(mut self, r: DataResidencyRegion) -> Self;

    pub fn open(
        self,
        paths: &AppPaths,
        settings: &SettingsStore,
    ) -> Result<OpenedTelemetry, TelemetryBundleError>;
}

#[derive(Clone)]
pub struct OpenedTelemetry {
    pub consent: ConsentStore,
    pub install_id: Option<InstallId>,        // None in anonymous mode
    pub reporter: Arc<DynamicReporter>,        // see below
    pub policy: PrivacyPolicy,                 // processor name, URL, region
}

impl OpenedTelemetry {
    pub fn flush_all(&self) -> Result<(), TelemetryBundleError>;
    pub fn discard_all(&self) -> Result<(), TelemetryBundleError>;
}
```

`DynamicReporter` is a small wrapper that holds both Path-A and Path-B
adapters (whichever were provided) and forwards `record` / `flush` /
`fetch` / `erase` to whichever is *active* per the consent state. Path
switching in the widget swaps the active pointer atomically — no
adapter teardown, no reconstruction. If the bundle was built with only
one adapter, the mode switch in `PrivacySettings::advanced` is hidden.

```rust
pub struct DynamicReporter {
    anonymous: Option<Arc<dyn UsageReporter>>,
    pseudonymous: Option<Arc<dyn UsageReporter>>,
    active: AtomicCell<TelemetryMode>,
    consent: ConsentStore,                     // gates emission
}

impl UsageReporter for DynamicReporter {
    fn record(&self, e: &Event<'_>) {
        if !matches!(self.consent.state_signal().get(), ConsentState::Granted(_)) { return; }
        match self.active.load() {
            TelemetryMode::Anonymous => self.anonymous.as_ref().map(|r| r.record(e)),
            TelemetryMode::Pseudonymous => self.pseudonymous.as_ref().map(|r| r.record(e)),
        };
    }
    // erase/fetch/flush/discard all delegate similarly
}
```

The `TelemetryExt` trait paralleling `SettingsExt`:

```rust
// crates/fern-telemetry/src/ext.rs
pub trait TelemetryExt {
    fn try_telemetry(&self) -> Option<&OpenedTelemetry>;
    fn telemetry(&self) -> &OpenedTelemetry { /* unwrap_or_else panic */ }
    fn consent(&self) -> &ConsentStore { &self.telemetry().consent }
    fn usage_reporter(&self) -> &Arc<DynamicReporter> { &self.telemetry().reporter }
}

impl<'a> TelemetryExt for BuildContext<'a> { ... }
impl<'a> TelemetryExt for EventContext<'a> { ... }
```

Apps `use fern_telemetry::TelemetryExt;` to get `ctx.consent()` /
`ctx.usage_reporter()` from any handler — same convention as
`fern_settings::SettingsExt`.

When `FernAppBuilder::telemetry()` lands in `fern-app`, it will look
roughly like:

```rust
// inside fern-app
pub fn telemetry(mut self, bundle: TelemetryBundle) -> Self {
    self.pending_telemetry = Some(bundle);
    self
}
// ...later, inside run() after app_state is populated by .settings():
let paths = self.app_state.get::<AppPaths>().expect(
    "FernAppBuilder::telemetry(...) requires .application(...) + .settings(...)"
);
let store = self.app_state.get::<SettingsStore>().expect(/* same */);
let opened = bundle.open(paths, store)?;
self.app_state.register(opened.clone());
self.app_state.register(opened.consent.clone());
self.app_state.register(opened.reporter.clone());
```

Apps that don't call `.telemetry(...)` get no reporter, no consent
prompt, and `PrivacySettings` falls back to a "Telemetry not configured"
state. Apps that don't ship telemetry pay nothing.

## 4. Reference adapters

Three crates outside the workspace path-deps, but inside the workspace
member glob:

```
crates/fern-analytics-plausible/        # anonymous mode
crates/fern-analytics-posthog/          # pseudonymous mode
crates/fern-analytics-otlp/             # OpenTelemetry, pseudonymous mode-style
```

Each implements `UsageReporter` and follows the same internal structure:

```
src/
    lib.rs              # Builder + UsageReporter impl
    config.rs           # endpoint, batch size, retention, EU/US region
    queue.rs            # uses fern-core::telemetry::queue
    transport.rs        # reqwest + retry/backoff + IP truncation note
    schema.rs           # adapter-specific event mapping
```

### `fern-analytics-plausible` (anonymous mode)

- Wire format: `POST /api/event` with `{name, url, domain, props}`. No
  install_id, no UUID, no fingerprint sent by the SDK. Plausible derives
  a per-day session hash server-side from IP + User-Agent + a daily-
  rotating server-held salt; the SDK does not transmit IP/UA explicitly,
  they are inherent to the HTTPS request. The salt rotation is what
  prevents cross-day correlation and is the basis of the CNIL
  consent-exempt posture — **this is the server's responsibility, not
  the SDK's**, and a controller running a self-hosted Plausible must
  verify it remains in place.
- `install_id()` returns `None`.
- `erase_remote_data()` returns `Err(ErasureUnsupported)`.
- `fetch_remote_data()` returns `Err(FetchUnsupported)` — without a
  stable id there is no per-user query surface; the widget hides the
  button. Aggregate stats are public on the Plausible dashboard
  regardless.
- Default endpoint: `https://plausible.io/api/event` (EU-hosted by
  vendor); self-host friendly via `endpoint()`.

### `fern-analytics-posthog` (pseudonymous mode)

- Wire format: `POST /capture/` with `{event, distinct_id, properties}`.
- `install_id()` returns `Some(uuid)`.
- `erase_remote_data()`: PostHog's
  `POST /api/projects/:pid/persons/?distinct_id=<id>` then `DELETE`.
- `fetch_remote_data()`: PostHog's
  `GET /api/projects/:pid/events/?distinct_id=<id>&limit=10000` paginated
  via `next` cursor; person properties via
  `GET /api/projects/:pid/persons/?distinct_id=<id>`. Result mapped into
  `RemoteDataExport` with one `RemoteEvent` per returned row. Requires a
  read-scoped Personal API token configured on the adapter (separate from
  the project capture key — the capture key is write-only).
- Default endpoint: `https://eu.posthog.com/capture/` (EU region by
  default).

### `fern-analytics-otlp` (pseudonymous mode-style)

- Wire format: OTLP/HTTP logs over JSON.
- `install_id()` returns `Some(uuid)` mapped to `service.instance.id`
  resource attribute.
- `erase_remote_data()`: backend-dependent; trait method returns
  `Err(ErasureUnsupportedByBackend)` if not configured. Adapter accepts a
  user-provided `EraseEndpoint` for backends that expose one (Honeycomb
  doesn't; self-hosted Tempo/Loki can).
- `fetch_remote_data()`: backend-dependent; same shape — adapter accepts
  a user-provided `QueryEndpoint` (e.g. self-hosted Loki LogQL by
  `service.instance.id`). Defaults to `Err(FetchUnsupportedByBackend)`.

### Common queue (`fern-telemetry::queue`)

Two implementations behind the [`EventQueue`] trait:

- **`InMemoryEventQueue`** — `Mutex<VecDeque<OwnedEvent>>`. Used by
  tests and by adapters that don't need cross-restart durability.
- **`PersistentEventQueue`** — backed by [redb](https://crates.io/crates/redb)
  at `AppPaths::data_dir().join("fern-telemetry/queue.redb")`. Pure
  Rust, no C dependency, ~250–400 KB binary footprint, ~1–2 s
  compile-time impact (vs. SQLite's ~1 MB + 5–10 s C compile + need
  for a system C toolchain at cross-compile time).

**Why redb over SQLite:** the queue is a typed K/V workload — append
events, drain in FIFO batches, capped size with oldest-eviction,
single writer. We don't need SQL queries, joins, or cross-language
file portability. SQLite's strengths don't apply; redb keeps the
framework's "pure Rust, no C deps" posture intact (see CLAUDE.md).
File-format stability since redb 2.0 (April 2024); current users
include Atuin and Iroh.

redb table layout:

```rust
const EVENTS: redb::TableDefinition<u64, &[u8]> =
    redb::TableDefinition::new("events");
// key:   u64 monotonic id (FIFO order = ascending key order)
// value: serde_json bytes of EventRecord {
//          event: OwnedEvent,
//          enqueued_at_unix_ms: u64,
//          attempts: u32,
//          next_attempt_at_unix_ms: u64,
//        }
```

Flush trigger: every 60 s, every 50 events, or on graceful exit.
Backoff: `min(60s * 2^attempts, 1h)`. Drop after 7 days
(`enqueued_at < now - 7d`). Max queue size 10 k events; oldest
dropped past that.

Why JSON inside the value blob (rather than bincode/postcard):
serde_json is already a workspace dep for the wire format, the
overhead vs. bincode is irrelevant at queue scale (tens-to-hundreds
of bytes per event, kilobytes total in the worst case), and
human-readable bytes make corrupt-file forensics trivial.

## 5. `PrivacySettings` widget — `fern-widgets`

New file
`crates/fern-widgets/src/privacy_settings.rs`, exported alongside
[`shortcut_settings.rs`](../../crates/fern-widgets/src/shortcut_settings.rs).

### Builder API

```rust
PrivacySettings::new(consent_store: Arc<ConsentStore>, reporter: Arc<dyn UsageReporter>)
    .privacy_policy_url("https://skribisto.app/privacy")    // app-provided
    .data_processor_name("FernTech")                        // legal entity
    .show_install_id(true)
    .show_inspect_data(true)
    .show_erase_button(true)                                // auto-hidden in anonymous mode
    .show_fetch_button(true)                                // auto-hidden in anonymous mode
    .show_mode_switch(true)                                 // toggle anonymous ↔ pseudonymous
    .compact(false)                                         // first-run modal vs. settings panel
```

### Layout (uses existing FernUI primitives)

```
VStack(spacing = 16)
├── Heading: tr!("privacy.heading")
├── Panel(role = SurfaceRole::Card)                       // Art. 13 notice
│   ├── TextWidget: tr!("privacy.notice.controller", processor = adapter.processor_name())
│   │   # "Data is processed by <FernTech>; the technical processor is <PostHog Inc.>."
│   ├── TextWidget: tr!("privacy.notice.purposes")
│   │   # "Improve the app: which features get used, where bugs cluster."
│   ├── TextWidget: tr!("privacy.notice.lawful_basis", path = path_label)
│   │   # anonymous mode → "legitimate interest"; pseudonymous mode → "your consent"
│   ├── TextWidget: tr!("privacy.notice.retention", days = retention_days)
│   ├── TextWidget: tr!("privacy.notice.recipients", endpoint = reporter.endpoint())
│   ├── TextWidget: tr!("privacy.notice.withdrawal_right")
│   └── Link("privacy.policy_link", url)
├── Panel(role = SurfaceRole::Card)            // toggles
│   ├── HStack: tr!("privacy.scope.anonymous_metrics") + Toggle
│   ├── HStack: tr!("privacy.scope.crash_reports") + Toggle
│   └── HStack: tr!("privacy.scope.feature_flags") + Toggle
├── HStack(equal-width)                        // CNIL parity
│   ├── Button("privacy.reject_all", style = Secondary)
│   └── Button("privacy.accept_all", style = Primary)
├── Accordion("privacy.inspect_data")           // expandable
│   ├── ListView of last 50 queued events (read from queue.redb)
│   │   each row: timestamp, event name, JSON-pretty props
│   └── Button("privacy.export_data")           // dump to clipboard or file
├── Panel(role = SurfaceRole::Card)            // identity row (pseudonymous mode only)
│   ├── HStack: Label("privacy.install_id") + Code(uuid) + Copy button
│   ├── HStack(spacing = 8)
│   │   ├── Button("privacy.fetch_data", style = Secondary)
│   │   │   on click → reporter.fetch_remote_data()
│   │   │     · spinner while pending
│   │   │     · on Ok(export) → open RemoteDataViewer in a Dialog with
│   │   │       a JSON tree of `events` + "Save as JSON…" + "Copy" buttons
│   │   │     · on Err → inline error TextWidget with retry
│   │   └── Button("privacy.erase_data", style = Destructive)
│   │       on click → confirm dialog → reporter.erase_remote_data()
│   │       shows progress, success, or error
│   └── TextWidget("privacy.retention_notice", retention_days)
├── Accordion("privacy.advanced")
│   ├── Mode switch: SegmentedControl(Anonymous | Pseudonymous)
│   │   on change → confirm → wipe queue/install_id, reset consent
│   ├── Endpoint display: Code(reporter.endpoint())
│   └── Adapter name: tr!("privacy.adapter", reporter.adapter_name())
└── HStack(footer)
    └── Button("privacy.withdraw_consent", style = Secondary)
        # Same prominence as "Accept all" — Art. 7(3)
```

### State binding

```rust
let scope = consent_store.scope_signal();           // Signal<ConsentScope>
HStack::new()
    .child(TextWidget::new(tr!("privacy.scope.anonymous_metrics")))
    .child(Toggle::new()
        .bind_value(scope.map(|s| s.anonymous_metrics))
        .on_change_fn(move |v, _| consent_store.set_anonymous_metrics(v)))
```

`reporter.supported_scopes()` is intersected with the displayed toggles —
toggles for unsupported scopes are hidden, not just disabled, to avoid
"why is this greyed out" confusion.

### Two presentations from one widget

```rust
// First-run modal
Dialog::new()
    .title(tr!("privacy.first_run.title"))
    .content(PrivacySettings::new(store, reporter).compact(true))
    .modal(true)
    .show(ctx);

// Settings panel
TabWidget::new().tab("Privacy", PrivacySettings::new(store, reporter))
```

`compact(true)` collapses the accordions, hides "Advanced", and prefers
"Accept all" / "Reject all" as the primary action surface. `compact(false)`
expands everything for the settings view.

### i18n keys

All strings via `tr_widget!` from
[`crates/fern-i18n-macros`](../../crates/fern-i18n-macros). Keys defined
under `privacy.*` namespace in
`crates/fern-widgets/locales/en/privacy.ftl` and `fr/privacy.ftl`. French
translation is the canonical reference for RGPD wording (CNIL templates).

## 6. Codegen — `fern-telemetry-codegen`

New crate `crates/fern-telemetry-codegen` (proc-macro):

```rust
include_telemetry_schema!("telemetry/events.yaml");
```

Expands to typed `emit_*` functions and a registered `EventSchema` array.
Compile errors on:

- unknown property type
- `dev_static` property passed a non-`&'static str`
- enum value not in declared variant list
- missing `expires` or `bug` field on new events (Glean-style governance)
- duplicate event name across merged schemas

The build also emits `target/telemetry/events.json` — the merged, fully
resolved schema — which adapters ship to the server for cross-check, and
which the privacy widget loads for the "Inspect data sent" tab.

## 7. RGPD compliance mapping

| Requirement | Article | Implementation |
|---|---|---|
| Lawful basis | Art. 6(1) | anonymous mode: legitimate interest (6(1)(f)) + LIA template in `docs/telemetry-lia.md`. pseudonymous mode: explicit consent (6(1)(a)) via widget. |
| Consent quality | Art. 4(11), 7 | First-run modal, no pre-tick, equal-prominence reject/accept (CNIL parity), per-scope granularity, "Decide later" leaves state `Unknown` (no emission). App functionality is **not** gated on consent (EDPB 2024 "consent or pay" rule). |
| Withdrawal as easy as grant | Art. 7(3) | "Withdraw consent" button in same widget, equal prominence; reachable from settings without re-engaging the first-run modal. |
| Information notice | Art. 13 | Plain-language summary must explicitly state: (i) the controller and the data processor (e.g. PostHog), (ii) the purposes of processing, (iii) the lawful basis, (iv) the retention period, (v) the recipients/transfers, (vi) the right to withdraw consent at any time. Surfaced in the widget; full text in the controller's privacy policy URL. |
| Data minimization | Art. 5(1)(c) | Closed `PropValue` enum; `dev_static` enforcement; no `String` for user input; no fingerprinting (see non-goals). |
| Storage limitation | Art. 5(1)(e) | Server-side data: 25-month max (CNIL Sheet n°14); configurable per adapter. pseudonymous mode install_id: rotated/regenerated at ≤13 months (or on app reinstall). |
| Right of access | Art. 15 | "Get my data" button (pseudonymous mode) → `fetch_remote_data()` returns the server-side record set. Plus "Inspect data sent" for the local pre-flush queue and install_id displayed. |
| Response time | Art. 12(3) | Fetch and erase are interactive (sub-second to seconds). Trait contract guarantees responses within the legal one-month ceiling; adapters that can't honor the ceiling must `Err(QuotaExceeded)` and document an out-of-band route in their privacy policy. |
| Right to rectification | Art. 16 | N/A — analytics events are factual records, not statements about the user. |
| Right to erasure | Art. 17 | "Erase my data" button (pseudonymous mode). In anonymous mode, server-side hashed data is anonymous if Recital 26 is met; the controller's LIA documents this. |
| Right to portability | Art. 20 | `RemoteDataExport` is a self-describing JSON document (schema_version + endpoint + records); "Save as JSON…" in the fetch dialog produces the portability artifact. |
| Right to object | Art. 21 | "Withdraw consent" + mode switch to Anonymous; consent revoke calls `discard_pending()` so no buffered events leak after objection. |
| Cookies/ePrivacy | Art. 5(3) | anonymous mode: no client storage *or access* on terminal equipment for analytics → 5(3) does not apply. pseudonymous mode: install_id stored → consent obtained via widget. |
| Controller / processor relationship | Art. 28 | When using a hosted backend, the controller (app developer) signs the processor's DPA. **Not** the SDK's responsibility to sign on behalf of the user; the plan documents the requirement and links to PostHog/Plausible DPA generators in `docs/telemetry-controller-checklist.md`. |
| Records of processing | Art. 30 | YAML manifest documents the *categories* of data collected and the *purposes*. The full Art. 30 record is the controller's responsibility — the manifest + a controller-side template (`docs/telemetry-art30-template.md`) cover the required fields (controller identity, recipients, transfers, retention, technical/organizational measures). |
| Data residency / international transfers | Ch. V, Art. 44–49 | Adapters' default endpoints are EU-resident (Plausible.io DE/FR, PostHog Cloud EU Frankfurt). The adapter does not enforce EU; the controller chooses the endpoint and signs the appropriate transfer mechanism (DPF for US, SCCs as fallback). DPF was upheld 03 Sep 2025 but treat as conditionally valid. |
| DPIA assessment | Art. 35 | Telemetry on a desktop app is generally not "high risk" per CNIL guidance; a DPIA is *recommended but not required* unless scale or processing changes shift the risk profile. Template at `docs/telemetry-dpia-template.md`. |

### Consent / fetch / erase — scope clarification

Three separate concerns, three separate controls. In pseudonymous mode all three
are present:

- **Consent** controls *whether new data is collected*.
- **Fetch ("Get my data")** lets the user *see / export what was
  collected* — Art. 15 access + Art. 20 portability.
- **Erase ("Erase my data")** *deletes previously-collected data* —
  Art. 17.

Withdrawing consent stops new emission but doesn't retroactively delete
or expose past data. The widget lays out fetch + erase side by side under
the install_id, with withdraw-consent in the footer — so a "see what you
have, then forget me" flow is three clicks (fetch → save JSON → erase),
and a plain "stop and forget me" flow is two (withdraw + erase).

## 8. Phased rollout

**Prerequisite:** `fern-settings` Phases 1-4 from
[settings-plan.md](settings-plan.md#11-implementation-order) (skeleton,
`AppPaths`, `DebouncedWriter`, `SettingsFile<T>` with `Versioned` +
`Migrator`). Without these, `ConsentStore` and `InstallId` have nothing
to ride on.

### Phase 1 — Plumbing (no widgets, no adapters)

- `fern-core/src/telemetry/`: trait, event types, intent-bus tap.
- `fern-telemetry/`: `ConsentStore` (atop `SettingsFile<ConsentFile>`),
  `InstallId` (atop `SettingsFile<InstallIdFile>`), `EventQueue` trait
  with `InMemoryEventQueue` (Phase 1) and `PersistentEventQueue`
  (redb-backed, lands in Phase 2.5 alongside the first real adapter).
- `fern-telemetry/build.rs` + `events.yaml` + `fern-telemetry-codegen`
  proc macro.
- Intent-bus tap in `event_dispatch_impl.rs` (in `fern-core` — only the
  trait is reached; storage stays in `fern-telemetry`).
- `FernAppBuilder::usage_reporter` + `consent_store` with builder-time
  panic if `.application(...)` + `.settings(...)` are missing.
- Unit tests: codegen rejects bad schemas; consent gate drops events
  when `Denied`; `discard_pending` clears queue without sending; queue
  persists across restart; `ConsentFile` migration v1→v2 works; install_id
  rotation triggers after 13 months (test with mock clock); event-schema
  bump resets to `Unknown`.

Acceptance: `cargo test -p fern-core -p fern-telemetry` passes; building
an app with no reporter is a no-op; building one with a stub
`Vec<Event>`-collector reporter shows intents flow through; revoking
consent mid-session drops the queue without flushing.

### Phase 2 — Plausible adapter (anonymous mode)

- `fern-analytics-plausible` crate.
- Disk queue + retry + backoff.
- Example app `examples/telemetry_plausible/` showing minimal setup.

Acceptance: pointing at a self-hosted Plausible instance shows events
arriving, with no install_id, no consent prompt.

### Phase 3 — `PrivacySettings` widget

- Widget in `fern-widgets`.
- i18n keys in en + fr.
- Both presentations (Dialog + tab) demonstrated in
  `examples/telemetry_plausible/`.
- "Inspect data sent" reads from `queue.redb`.

Acceptance: widget catalog example shows the widget; first-run flow gates
emission; toggling scopes is reflected immediately in `ConsentState`.

### Phase 4 — PostHog adapter (pseudonymous mode) + fetch & erase buttons

- `fern-analytics-posthog` crate.
- Install_id generation + persistence.
- Fetch button wires through `reporter.fetch_remote_data()` — paginated
  PostHog events query, JSON tree viewer Dialog, "Save as JSON…" export.
- Erase button wires through `reporter.erase_remote_data()`.
- Mode-switch UX in `PrivacySettings::advanced`.

Acceptance: against a real PostHog project, fetch returns this install's
events with correct schema mapping; "Save as JSON…" produces a valid
`RemoteDataExport`; erase deletes the install_id's events; mode switch
wipes local state and re-prompts consent.

### Phase 5 — OTLP adapter + census + governance tooling

- `fern-analytics-otlp` crate.
- `widget.census` periodic emitter (hourly or on idle).
- Schema linter CLI: `cargo fern-telemetry-lint` checks `expires`, `bug`,
  unused events, drift between manifest and code.

Acceptance: events visible in any OTel-compatible backend; census shows
widget-type histogram; lint CI step in workflow.

## 9. Non-goals

- **No FernUI-operated ingestion service.** We define the wire format and
  ship adapters; users self-host or contract a vendor.
- **No session replay.** PII risk is too high for a sole-developer
  framework to take on.
- **No user-account identity.** `install_id` is per-install, not per-user;
  apps that want account-scoped analytics can extend their own adapter.
- **No A/B testing or feature flags in v1.** Trait surface accommodates a
  future `flags()` method but the v1 widget doesn't expose flag UX.
- **No automatic capture of widget contents.** Ever. Even with consent.
  Out of scope by design.
- **No clipboard, keystroke, or screen capture.** Same.
- **No device fingerprinting.** The `lifecycle.app_started` properties
  (os, arch, locale, theme_kind, app_version) are individually low-entropy
  but their *combination* could approach a fingerprint at small user
  populations. Adapters and controllers MUST treat the combination as
  pseudonymous data in pseudonymous mode, and in anonymous mode MUST ensure the server-side
  aggregator does not retain the combination at a per-event granularity
  past the daily salt rotation. The framework does not, and cannot,
  enforce k-anonymity — this is a controller responsibility documented
  in the LIA template.
- **No silent telemetry.** The first-run modal must always be reachable;
  "Decide later" is a valid outcome that leaves `ConsentState` as
  `Unknown` and **no events are emitted**. The app must remain fully
  functional in that state.

## 10. Controller responsibilities (what FernUI does NOT do)

FernUI provides the SDK plumbing, the consent widget, and reference
adapters. The app developer is the **data controller** under RGPD and
remains responsible for the items below. The plan ships templates in
`docs/` to make each one a fill-in exercise rather than a from-scratch
legal task.

| Responsibility | Owner | Artifact shipped |
|---|---|---|
| Privacy policy text (linked from widget) | Controller | `docs/telemetry-privacy-policy-template.md` (en + fr) |
| Choice of anonymous mode or pseudonymous mode for the app | Controller | this plan |
| LIA (Legitimate Interest Assessment) for anonymous mode | Controller | `docs/telemetry-lia.md` |
| CNIL self-assessment record (anonymous mode, in force since 1 Jan 2026) | Controller | `docs/telemetry-cnil-self-assessment.md` |
| Art. 30 record of processing activities | Controller | `docs/telemetry-art30-template.md` |
| Art. 35 DPIA (if risk profile warrants) | Controller | `docs/telemetry-dpia-template.md` |
| DPA signed with hosted-backend processor | Controller | links to PostHog/Plausible DPA generators |
| Choice of EU vs. non-EU endpoint | Controller | adapter docs |
| Age gating (Art. 8 — minors under 13–16 depending on member state) | Controller | the consent widget exposes a `min_age_required` config; controller decides |
| Notification of personal data breach (Art. 33–34) | Controller | not in scope; standard incident response |
| Schema-version bump triggering re-prompt | Both — framework re-prompts on bump; controller decides what counts as a bump | `consent.rs` schema_version |

The framework will refuse to emit events when `ConsentStore::path` does
not exist or `decided_at` is `None` — i.e. a missing controller setup is
a fail-closed condition, not a fail-open one.

## 11. Configuration — wiring the analytics server

Three layers of configuration, each with a different lifetime and
privacy implication.

### 11.1 Build-time (compiled into the binary)

What goes here: the **vendor endpoint default** and the **public capture
key**. Plus, when needed for fetch/erase, a **read-scoped Personal API
token**.

```rust
// build.rs or .cargo/config.toml or CI env
//   SKRIBISTO_POSTHOG_PROJECT_KEY = phc_xxxxxxxxx           (public — capture-only)
//   SKRIBISTO_POSTHOG_API_TOKEN   = phx_xxxxxxxxx           (private — read scope)

const POSTHOG_PROJECT_KEY: &str = env!("SKRIBISTO_POSTHOG_PROJECT_KEY");
const POSTHOG_API_TOKEN:   &str = env!("SKRIBISTO_POSTHOG_API_TOKEN");
```

PostHog's project key is a write-only public key embedded in clients
(it cannot read or delete data); embedding it in the binary is the
intended pattern. The Personal API token is private and only needed for
`fetch_remote_data()` / `erase_remote_data()`. If the app ships only
`record` (no fetch/erase), the API token is omitted and those buttons
hide.

**Never put either in `SettingsStore`** — settings live in the user's
config dir as plaintext TOML and would be visible to any process the
user runs. Adapter constructors accept these via builder methods, hold
them in adapter state (memory only), and never serialize them.

The event-schema version is computed at compile time from the YAML
manifest:

```rust
// generated by include_telemetry_schema!
pub const EVENT_SCHEMA_VERSION: u32 = 7;
```

### 11.2 App-builder time (per-process configuration)

The `TelemetryBundle` is constructed once at startup. All adapter
instances are built here, passed in by ownership, and then frozen.

```rust
use fern_app::FernAppBuilder;
use fern_settings::SettingsBundle;
use fern_telemetry::{TelemetryBundle, TelemetryMode, DataResidencyRegion};
use fern_analytics_plausible::PlausibleAdapter;
use fern_analytics_posthog::PostHogAdapter;

fn main() {
    let plausible = PlausibleAdapter::builder()
        .endpoint("https://plausible.io/api/event")     // EU-resident default
        .domain("skribisto.app")                        // identifies the property
        .build();

    let posthog = PostHogAdapter::builder()
        .endpoint("https://eu.posthog.com/capture/")    // EU-resident default
        .project_key(POSTHOG_PROJECT_KEY)               // build-time const
        .personal_api_token(POSTHOG_API_TOKEN)          // build-time const
        .request_timeout(Duration::from_secs(10))
        .max_batch_size(50)
        .max_queue_size(10_000)
        .build();

    let telemetry = TelemetryBundle::new(EVENT_SCHEMA_VERSION)
        .with_anonymous(plausible)
        .with_pseudonymous(posthog)
        .with_default_mode(TelemetryMode::Anonymous)            // anonymous-by-default
        .with_data_processor_name("PostHog Inc. / Plausible Insights OÜ")
        .with_privacy_policy_url("https://skribisto.app/privacy")
        .with_data_residency_region(DataResidencyRegion::EU)
        .with_debounce(Duration::from_millis(500));

    FernAppBuilder::new()
        .application("com", "FernTech", "FernUI")       // → AppPaths
        .settings(SettingsBundle::new()                 // → SettingsStore
            .with_recent_projects(10)
            .with_window_state(true))
        .telemetry(telemetry)                            // → OpenedTelemetry
        .root(|tree| tree.add(SkribitoRoot::new()))
        .run();
}
```

What's resolved at this layer:

| Knob | Source | Why |
|---|---|---|
| Endpoint URL (default) | Adapter builder | Vendor decides the EU-resident default; controller can override |
| API keys / tokens | Adapter builder, from `env!()` | Must not touch disk |
| Adapter timeouts, batch sizes | Adapter builder | Performance tuning, no PII |
| Anonymous / pseudonymous mode availability | `TelemetryBundle::with_anonymous` / `::with_pseudonymous` | Compile-time decision: ship one or both |
| Default mode | `TelemetryBundle::with_default_mode` | First-run posture before user picks |
| Debounce window | `TelemetryBundle::with_debounce` | Per-bundle, applied to consent + install_id files |
| Processor name + policy URL | `TelemetryBundle::with_data_*` | Surfaced verbatim in Art. 13 notice |
| Residency region label | `TelemetryBundle::with_data_residency_region` | Surfaced in widget; no enforcement |

### 11.3 Runtime (user-overridable, persisted)

A small set of telemetry keys lives in the **app's `SettingsStore`** —
the same TOML file (`general.toml`) that holds editor preferences. The
user (or a power user editing the file by hand) can override the
endpoint to point at a self-hosted backend.

```rust
// crates/fern-telemetry/src/scopes.rs
use fern_settings::SettingsKey;

pub const TELEMETRY_ENDPOINT_OVERRIDE: SettingsKey<Option<String>> =
    SettingsKey::new("telemetry.endpoint_override", || None);

pub const TELEMETRY_REGION_OVERRIDE: SettingsKey<Option<String>> =
    SettingsKey::new("telemetry.region_override", || None);

// Per-scope toggles — bound directly to the consent widget Toggles
// via SettingsStore::signal_for(&KEY). These are mirrored from
// ConsentStore on every mutation so they survive even if the consent
// file is deleted.
pub const TELEMETRY_ANONYMOUS_METRICS: SettingsKey<bool> =
    SettingsKey::new("telemetry.consent.anonymous_metrics", || false);
pub const TELEMETRY_CRASH_REPORTS: SettingsKey<bool> =
    SettingsKey::new("telemetry.consent.crash_reports", || false);
pub const TELEMETRY_FEATURE_FLAGS: SettingsKey<bool> =
    SettingsKey::new("telemetry.consent.feature_flags", || false);
```

`TelemetryBundle::open` reads the endpoint override exactly once, at
startup:

```rust
pub fn open(
    self,
    paths: &AppPaths,
    settings: &SettingsStore,
) -> Result<OpenedTelemetry, TelemetryBundleError> {
    let override_endpoint = settings.signal_for(&TELEMETRY_ENDPOINT_OVERRIDE).get();

    let anonymous = self.anonymous.map(|r| {
        if let Some(url) = override_endpoint.clone() { r.with_endpoint(url) } else { r }
    });
    let pseudonymous = self.pseudonymous.map(|r| {
        if let Some(url) = override_endpoint.clone() { r.with_endpoint(url) } else { r }
    });

    let consent = ConsentStore::open(paths, self.debounce, self.event_schema_version)?;
    let install_id = match self.pseudonymous.is_some() {
        true  => Some(InstallId::open_or_create(paths, self.debounce)?),
        false => None,
    };
    let reporter = Arc::new(DynamicReporter::new(anonymous, pseudonymous, self.default_mode, consent.clone()));
    Ok(OpenedTelemetry { consent, install_id, reporter, policy: self.policy() })
}
```

Endpoint changes mid-session are not honored — the adapter holds the
URL by value. To change the endpoint, the user edits the TOML and
restarts. This is intentional: redirecting in-flight events to a
different server post-consent would violate the "recipients" notice the
user agreed to.

### 11.4 Self-hosted setup — worked example

A user wants to point FernUI at their own self-hosted Plausible
instance:

```toml
# ~/.config/skribisto/general.toml
[telemetry]
endpoint_override = "https://analytics.example.com/api/event"
region_override   = "Self-hosted"
```

On next launch:

1. `SettingsStore::open` reads the file.
2. `TelemetryBundle::open` reads `telemetry.endpoint_override`, applies
   it to whichever adapter(s) the app shipped.
3. The `PrivacySettings` widget shows the new endpoint verbatim under
   "Endpoint:" so the user can verify.
4. If the user previously consented to the vendor endpoint, the
   `consented_to_event_schema` re-prompt rule does **not** fire (schema
   didn't change) — but the recipient changed. **Rule:** changing the
   endpoint override triggers a one-shot re-prompt, treated as a
   "recipient changed" event. Implemented by checking
   `consent.endpoint_at_consent_time` against `reporter.endpoint()` at
   `OpenedTelemetry::open` and forcing `state = Unknown` if they
   differ.

### 11.5 Testing setup

Production code uses `AppPaths::new(qual, org, app)`. Tests use
`AppPaths::for_testing(tmp.path())` and a stub `UsageReporter` that
collects events into a `Vec`:

```rust
#[cfg(test)]
pub struct StubReporter {
    pub recorded: Mutex<Vec<OwnedEvent>>,
    pub installed_id: Option<String>,
}

impl UsageReporter for StubReporter {
    fn record(&self, e: &Event<'_>) {
        self.recorded.lock().unwrap().push(e.to_owned());
    }
    fn install_id(&self) -> Option<&str> { self.installed_id.as_deref() }
    fn adapter_name(&self) -> &'static str { "stub" }
    fn endpoint(&self) -> &str { "stub://" }
    fn supported_scopes(&self) -> ConsentScope { ConsentScope::all() }
    fn discard_pending(&self) -> DiscardFuture { Box::pin(async { Ok(()) }) }
    fn erase_remote_data(&self) -> ErasureFuture { Box::pin(async { Ok(()) }) }
    fn fetch_remote_data(&self) -> FetchFuture { /* return empty export */ }
}

#[test]
fn intent_dispatch_records_through_dynamic_reporter() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());
    let stub = Arc::new(StubReporter::default());

    let settings = SettingsStore::open_with_delay(
        paths.config_file("general"), Duration::ZERO,
    ).unwrap();

    let bundle = TelemetryBundle::new(1)
        .with_anonymous(stub.clone())
        .with_default_mode(TelemetryMode::Anonymous)
        .with_debounce(Duration::ZERO);
    let opened = bundle.open(&paths, &settings).unwrap();

    opened.consent.grant(ConsentScope::all());

    // simulate an intent
    fern_telemetry::generated::emit_intent_dispatched(
        &*opened.reporter, "app.save", IntentSource::Shortcut,
    );

    assert_eq!(stub.recorded.lock().unwrap().len(), 1);
}
```

Tests use `Duration::ZERO` for the debounce so writes flush immediately
— same idiom as `fern-settings`'s integration tests at
[`crates/fern-settings/tests/`](../../crates/fern-settings/tests/).

### 11.6 What lives where — summary

| Concern | Layer | Where it sits |
|---|---|---|
| Default endpoint URL | Build-time | Adapter builder method, vendor-supplied default |
| API keys / tokens | Build-time | `env!()` → adapter builder; never on disk |
| Anonymous vs pseudonymous availability | App-builder | `TelemetryBundle::with_anonymous` / `::with_pseudonymous` |
| Default mode on first run | App-builder | `TelemetryBundle::with_default_mode` |
| Debounce, batch, timeout tuning | App-builder | Adapter builder + bundle |
| Processor name, policy URL | App-builder | `TelemetryBundle::with_data_*`, surfaced in widget |
| User's endpoint override | Runtime | `SettingsStore` key `telemetry.endpoint_override` |
| User's per-scope toggles | Runtime | `ConsentStore` (mirrored to `SettingsStore` keys) |
| Active mode | Runtime | `DynamicReporter::active`, mutated by widget |
| Install_id (pseudonymous mode) | Runtime | `SettingsFile<InstallIdFile>` |
| Consent decision | Runtime | `SettingsFile<ConsentFile>` |
| Pending events | Runtime | redb at `AppPaths::data_dir().join("fern-telemetry/queue.redb")` |

## 12. Open questions

- **Census cadence**: hourly is arbitrary. Better: emit on first build
  after each `app_started`, plus on significant tree changes (debounced).
  Decide during Phase 5.
- **Custom `ConsentScope` extension**: do app-defined events get to add
  their own scope categories ("editor-content-stats")? Plumbing-wise it
  is cheap, UX-wise it bloats the widget. Defer until needed.
- **Crash reporter integration**: the `crash_reports` scope toggle
  exists in the schema but has no transport. Sentry adapter is the
  obvious choice; not v1.
- **Consent revalidation cadence**: schema-version bump re-prompts. Do
  we *also* re-prompt every N months as best practice? CNIL says 13
  months for cookies; for desktop analytics it's less clear. Tentatively:
  no automatic re-prompt, only on schema change.
