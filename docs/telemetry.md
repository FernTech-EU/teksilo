<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Telemetry & Privacy Reference

Bastyde's telemetry is **consent-gated by construction** and
**privacy-mode-switchable at runtime**. There is no path through which
an event can reach a server while the user's `ConsentState` is
`Unknown` or `Denied` — the gate lives in the dispatch tap, before
any adapter sees the event. Apps that ship without telemetry pay
nothing; apps that ship with it inherit a working RGPD-compliant
shape (Art. 13 notice + per-scope toggles + Art. 15/17/20 buttons)
out of the box.

Mental model in one line:

```text
TelemetryBundle → OpenedTelemetry → app_state registry → dispatch tap → consent gate → adapter
```

The two modes are an architectural choice, not a tunable knob:

| Mode             | `install_id`     | Default scopes available     | What the user can do            |
|------------------|------------------|------------------------------|---------------------------------|
| **Anonymous**    | None             | `anonymous_metrics_only()`   | Withdraw consent                |
| **Pseudonymous** | UUID, 13-month rotation | `ConsentScope::all()` | Withdraw + Get my data + Erase  |

Apps configure one or both adapters; users (or the framework's mode-switch UI) flip between them.

Three persistence shapes come with the crate:

| Shape | Type | Use for |
|-------|------|---------|
| Consent state | [`ConsentStore`](../crates/bastyde-telemetry/src/consent.rs) → `Signal<ConsentState>` | The user's grant/deny/scope decision, atop `SettingsFile<ConsentFile>` |
| Pseudonymous identity | [`InstallId`](../crates/bastyde-telemetry/src/install_id.rs) | Per-install UUID with 13-month rotation, atop `SettingsFile<InstallIdFile>` |
| Event queue | [`InMemoryEventQueue`](../crates/bastyde-telemetry/src/queue/mem.rs) / [`PersistentEventQueue`](../crates/bastyde-telemetry/src/queue/persistent.rs) | Outbound buffering with retry; redb-backed for cross-restart durability |

Two reference adapters ship in tree:

| Adapter | Crate | Mode(s) | Backend |
|---------|-------|---------|---------|
| `StubReporter` | [`bastyde-telemetry`](../crates/bastyde-telemetry/src/stub.rs) | Anonymous + Pseudonymous | In-memory `Vec` (testing only) |
| `PlausibleAdapter` | [`bastyde-analytics-plausible`](../crates/bastyde-analytics-plausible/) | Anonymous | Plausible Cloud or self-hosted |
| `BastydeAdapter` | [`bastyde-analytics-bastyde`](../crates/bastyde-analytics-bastyde/) | Anonymous + Pseudonymous | Self-hosted [`bastyde-collector`](../../bastyde-collector/) gRPC service |

---

## 1. Quick start

Wire telemetry through `BastydeAppBuilder` alongside `settings(...)` —
both go through the same builder-time validation pattern:

```rust
use bastyde::prelude::*;
use bastyde::app::BastydeAppBuilder;
use bastyde::settings::SettingsBundle;
use bastyde_analytics_bastyde::BastydeAdapter;
use bastyde_telemetry::{TelemetryBundle, TelemetryMode, UsageReporter};
use std::rc::Rc;

const EVENT_SCHEMA_VERSION: u32 = 1;

fn main() {
    let adapter = Rc::new(
        BastydeAdapter::builder()
            .endpoint("https://collector.example.com:50051")
            .product_id("my.app")
            .bearer_token(std::env::var("BASTYDE_TOKEN").unwrap())
            .build(),
    ) as Rc<dyn UsageReporter>;

    let telemetry = TelemetryBundle::new(EVENT_SCHEMA_VERSION)
        .with_anonymous(adapter)
        .with_default_mode(TelemetryMode::Anonymous)
        .with_data_processor_name("MyCo SAS");

    BastydeAppBuilder::new()
        .application("eu", "MyCo", "my-app")
        .settings(SettingsBundle::new())
        .telemetry(telemetry)        // <— telemetry wires in here
        .initial_window(/* ... */)
        .run();
}
```

Three things happen on `.run()`:

1. The `TelemetryBundle` opens — `ConsentStore`, `InstallId` (when in
   pseudonymous mode), the recent-log ring buffer, and the
   `DynamicReporter` are constructed.
2. The resulting `OpenedTelemetry` is registered into the
   `app_state` registry — accessible from any widget via
   [`TelemetryExt`](../crates/bastyde-telemetry/src/ext.rs).
3. The dispatch tap in `bastyde-core` starts forwarding every dispatched
   intent through `DynamicReporter::record`, **but the consent gate
   silently drops everything until the user grants** — the app stays
   functional in the `Unknown` state, no events leave.

Drop the [`PrivacySettings`](../crates/bastyde-widgets/src/privacy_settings.rs)
widget anywhere in the tree (typically a settings tab or first-run
modal) and the user's grant flow + every Art. 13/15/17/20 obligation
is wired.

---

## 2. The pieces

### 2.1 `TelemetryBundle` — declarative configuration

Mirror of [`SettingsBundle`](settings.md). Builder-time validation;
opens at `BastydeAppBuilder::run()` time once `AppPaths` and the
`SettingsStore` are available.

```rust
TelemetryBundle::new(event_schema_version)
    .with_anonymous(adapter)              // Rc<dyn UsageReporter>
    .with_pseudonymous(other_adapter)     // optional second adapter
    .with_default_mode(TelemetryMode::Anonymous)
    .with_data_processor_name("MyCo SAS")
    .with_data_residency_region(DataResidencyRegion::EU)
    .with_recent_log_capacity(200)        // ring buffer for "Inspect data sent"
    .with_debounce(Duration::from_millis(500))
```

Required: at least one of `with_anonymous(...)` or
`with_pseudonymous(...)`. Both let the user flip between modes via
the widget.

### 2.2 `OpenedTelemetry` — runtime handle

The opened-bundle handle is `Clone`-cheap (every field is `Rc`/`Arc`).
Surfaced through `TelemetryExt`:

```rust
use bastyde_telemetry::TelemetryExt;

fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    if let Some(t) = ctx.try_telemetry() {
        // Public fields:
        let _: ConsentStore = t.consent.clone();
        let _: Option<InstallId> = t.install_id.clone();
        let _: Rc<DynamicReporter> = t.reporter.clone();
        let _: Arc<InMemoryEventQueue> = t.recent_log.clone();
        let _: PrivacyPolicy = t.policy.clone();
        let _: u32 = t.event_schema_version;
    }
    // ...
}
```

`recent_log` is a ring buffer `DynamicReporter::record` tees every
consent-gated event into. Independent of the adapter's outbound
queue — events stay in the recent log even after the adapter has
flushed them, until evicted by the ring buffer's capacity (default
200, configurable via `with_recent_log_capacity`). Read by the
"Inspect data sent" accordion in the widget.

### 2.3 `UsageReporter` trait — the adapter surface

```rust
pub trait UsageReporter {
    fn record(&self, event: &Event<'_>);
    fn flush(&self) -> Result<(), TelemetryError>;
    fn discard_pending(&self) -> Result<(), TelemetryError>;
    fn supported_scopes(&self) -> ConsentScope;
    fn install_id(&self) -> Option<&str>;
    fn endpoint(&self) -> &str;
    fn adapter_name(&self) -> &'static str;
    fn fetch_remote_data(&self) -> Result<RemoteDataExport, TelemetryError>;
    fn erase_remote_data(&self) -> Result<(), TelemetryError>;
}
```

The trait is **single-threaded** — adapters are `Rc`-shared, called
only from the UI-thread dispatch tap. Adapters that need I/O behind
a worker thread (Plausible, Bastyde) own that thread internally and
communicate via `mpsc` channels; the `UsageReporter` impl is just
the sync surface.

`fetch_remote_data` and `erase_remote_data` return
`TelemetryError::FetchUnsupported` / `ErasureUnsupported` for
anonymous-mode adapters; the widget hides the corresponding buttons
in that case.

### 2.4 `ConsentStore` — the gate

Wraps `SettingsFile<ConsentFile>` with a `Signal<ConsentState>` for
widget reactivity.

```rust
pub enum ConsentState {
    Unknown,                     // first run; no events emitted
    Granted(ConsentScope),       // events flow per the scope
    Denied,                      // explicit no; events dropped
}

pub struct ConsentScope {
    pub anonymous_metrics: bool,
    pub crash_reports: bool,
    pub feature_flags: bool,
    pub session_recording: bool,    // reserved — not implemented yet (PII risk)
}
```

API:

```rust
consent.state_signal();                    // Signal<ConsentState>
consent.is_granted();                      // bool — what the dispatch tap checks
consent.grant(scope, endpoint);            // Granted with full scope
consent.deny();                            // Denied
consent.withdraw();                        // shortcut for deny()
consent.set_scope(|s| s.crash_reports = false);
consent.set_or_grant_scope(endpoint, |s| s.anonymous_metrics = true);
                                            // Unknown→Granted-with-one-scope; no-op when Denied
consent.reset();                           // back to Unknown (used by mode switch)
consent.with_settings_mirror(settings_store);
                                            // optional one-way mirror to per-scope
                                            // SettingsKey<bool> values
```

**Re-prompt rules** (consent goes back to `Unknown` in any of):
- `event_schema_version` bumps from one the user previously consented to.
- The endpoint string changes (recipient-change rule).
- The user runs `consent.reset()` (typically from the mode-switch flow).

`with_settings_mirror(SettingsStore)` writes the per-scope booleans
into [`scopes::TELEMETRY_ANONYMOUS_METRICS`](../crates/bastyde-telemetry/src/scopes.rs) /
`TELEMETRY_CRASH_REPORTS` / `TELEMETRY_FEATURE_FLAGS` keys so power
users editing `general.toml` directly see the live state. One-way
(consent → settings, not the reverse — the consent file is
authoritative).

### 2.5 `InstallId` — pseudonymous identity

Generated lazily on first pseudonymous-mode use; rotated every 13
months to align with the CNIL cookie-consent SLA. Stored in
`SettingsFile<InstallIdFile>` under `AppPaths::config_dir()`.

```rust
let install_id: Option<InstallId> = telemetry.install_id.clone();
if let Some(id) = &install_id {
    let uuid: String = id.get();
    id.clear();    // user clicked "Erase my data"
}
```

`reporter.install_id()` is the value that ends up on every emitted
`Event::install_id` field — adapters override anything the event
itself carried, so the per-install identity is consistent with what
the server sees.

### 2.6 Event queues

| Type | Use case | Backend |
|------|----------|---------|
| `InMemoryEventQueue` | Tests, the recent-log ring buffer, simple deployments | `Mutex<VecDeque<OwnedEvent>>` |
| `PersistentEventQueue` | Adapter outbound buffering across process restarts | redb (pure Rust, no C deps) |

Both implement the `EventQueue` trait (`push`, `drain_batch`, `len`,
`peek_recent`, `discard_all`). Adapters typically hold one as their
outbound queue; the `OpenedTelemetry::recent_log` is always an
`InMemoryEventQueue`.

`PersistentEventQueue` opens a redb file at a configured path with
capacity + age caps:

```rust
let queue = PersistentEventQueue::open_with(
    &path,
    10_000,                              // capacity (oldest evicted past this)
    Duration::from_secs(60 * 60 * 24 * 7), // max age — events past this drop
)?;
```

The Plausible and Bastyde adapters expose a
`.persistent_queue_path(path)` builder method to opt into
durability. Without it, they fall back to an `InMemoryEventQueue`
(events lost on hard exit).

`OtlpAdapter` is the exception: it deliberately has no persistent
queue. The OTel deployment model assumes a *collector* sits between
the app and the backend, and that collector (`file_storage`
extension on `otelcol-contrib`, or its built-in queue) owns
durability. See §3.4 for the full reasoning.

### 2.7 The dispatch tap

`bastyde-core`'s `event_dispatch_impl.rs` taps every dispatched intent
through:

```text
intent.fired
  → ctx.try_telemetry_context()
  → ConsentStore::is_granted()? if not → return
  → DynamicReporter::record(&Event)
       ├── recent_log.push(event.to_owned())          // user-visible
       ├── recent_log_revision.set(version + 1)       // signals widget rebuild
       └── active_adapter.record(event)               // outbound

```

The `recent_log_revision` signal lives on `DynamicReporter` and is
the binding the `PrivacySettings` widget watches at
`BindingLevel::Rebuild` so its "Inspect data sent" accordion stays
in sync without polling.

---

## 3. Adapters

### 3.1 `StubReporter` (testing)

In-memory `Vec<OwnedEvent>` collector. The `last_recorded_name()`
helper makes integration tests trivial:

```rust
let stub = Rc::new(StubReporter::anonymous());
let bundle = TelemetryBundle::new(1).with_anonymous(stub.clone());
// ...
assert_eq!(
    stub.last_recorded_name().as_deref(),
    Some("intent.dispatched"),
);
```

Both `anonymous()` and `pseudonymous("uuid")` constructors exist.

### 3.2 `PlausibleAdapter` (anonymous mode → Plausible)

Wire-format: `{name, url, domain, props}` POSTed to
`<endpoint>/api/event`. Synthetic `app://<domain>/<event-name>` URL
since Plausible expects a URL and we don't have one.

```rust
let adapter = PlausibleAdapter::builder()
    .endpoint("https://plausible.io/api/event")    // or self-hosted
    .domain("my.app")
    .max_batch_size(50)
    .flush_interval(Duration::from_secs(60))
    .persistent_queue_path(paths.data_dir().join("plausible-queue.redb"))
    .endpoint_override(
        settings.signal_for(&scopes::TELEMETRY_ENDPOINT_OVERRIDE).get(),
    )                                              // no-op when empty
    .build();
```

Anonymous-by-design: no `install_id` ever, `fetch_remote_data` /
`erase_remote_data` always return `Unsupported`. CNIL audience-
measurement-exemption posture by default.

See [`crates/bastyde-analytics-plausible/`](../crates/bastyde-analytics-plausible/)
and [`examples/telemetry_plausible/`](../examples/telemetry_plausible/).

### 3.3 `BastydeAdapter` (anonymous + pseudonymous → bastyde-collector)

Home-grown gRPC adapter for the Bastyde-operated
[`bastyde-collector`](../../bastyde-collector/) backend. Single adapter
covers both modes; flip via `.install_id(uuid)` on the builder.

```rust
let adapter = BastydeAdapter::builder()
    .endpoint("https://collector.example.com:50051")
    .product_id("my.app")
    .bearer_token("fct_id_secret")                  // from `bastyde-collector token mint`
    .tls(TlsClientConfig {                          // optional — server may run plain
        ca_pem: Some(std::fs::read("/etc/ssl/ca.pem")?),
        client_cert_pem: None,                      // optional mTLS
        client_key_pem: None,
        domain_name: Some("collector.example.com".into()),
    })
    .install_id("UUID-STRING")                      // pseudonymous mode; omit for anonymous
    .max_batch_size(50)
    .flush_interval(Duration::from_secs(60))
    .persistent_queue_path(paths.data_dir().join("bastyde-queue.redb"))
    .build();
```

In pseudonymous mode (`install_id` set):
- `supported_scopes()` returns `ConsentScope::all()`.
- Every batch is tagged `mode = Pseudonymous`.
- `fetch_remote_data()` calls `Telemetry.Fetch` and rebuilds a
  `RemoteDataExport`.
- `erase_remote_data()` calls `Telemetry.Erase`.

Multiple instances of the same Bastyde app, each with its own
`install_id`, hit the same `bastyde-collector` endpoint with the same
bearer token; per-product scope is enforced server-side.

See [`crates/bastyde-analytics-bastyde/`](../crates/bastyde-analytics-bastyde/)
and [`examples/telemetry_bastyde/`](../examples/telemetry_bastyde/).

### 3.4 `OtlpAdapter` (anonymous + pseudonymous → OTLP/HTTP logs)

Speaks **OTLP/HTTP logs** over **JSON**. Works with any
OTel-compatible collector — `otelcol-contrib`, Honeycomb,
self-hosted Tempo+Loki via the OTel collector's HTTP receiver.

```rust
let adapter = OtlpAdapter::builder()
    .endpoint("http://127.0.0.1:4318/v1/logs")
    .service_name("my.app")
    .service_version(env!("CARGO_PKG_VERSION"))
    .header("x-honeycomb-team", api_key)
    .max_batch_size(50)
    .flush_interval(Duration::from_secs(60))
    .build();
```

The mapping is:

```text
Bastyde Event             OTLP LogRecord
──────────────────────── ────────────────────────────────────
event.name               body.stringValue
event.category           attributes["bastyde.category"]
event.timestamp          timeUnixNano (string, OTLP/JSON)
event.install_id         resource.service.instance.id (when set)
event.session_id         attributes["bastyde.session_id"]
event.props.<key>        attributes["bastyde.<key>"]
```

Anonymous-mode batches (no `install_id`) omit
`service.instance.id`; the OTel collector treats them as aggregate
logs.

**Queue durability — intentional asymmetry.** Unlike the Plausible
and Bastyde adapters, `OtlpAdapter` has no `.persistent_queue_path(...)`
method: pending events live in an in-memory `VecDeque` and are lost
on hard exit. The OTel deployment model expects a *collector*
(sidecar, system service, or `localhost:4318`) to own the durability
layer via its `file_storage` extension or built-in queue. Layering
redb inside the desktop adapter would duplicate work the collector
already does. Apps that need client-side durability against hard
exits should run a local collector with `file_storage`.

**Fetch + erase.** OTLP has no read or delete RPC, so
`fetch_remote_data` / `erase_remote_data` return
`FetchUnsupportedByBackend` / `ErasureUnsupportedByBackend`. The
`PrivacySettings` widget hides the "Get my data" / "Erase my data"
controls when these come back.

See [`crates/bastyde-analytics-otlp/`](../crates/bastyde-analytics-otlp/).

### 3.5 Retry semantics — comparison

The three adapters share the same outline (drain → send → on
failure, exponential backoff with jitter) but differ in how a
failed batch interacts with the queue. The differences are
visible in operations and in stats counters.

| Behavior | Plausible | OTLP | Bastyde |
| -------- | --------- | ---- | ---- |
| Send unit | Per event (one HTTP POST per event) | Per batch (one OTLP request per drain) | Per batch (one gRPC call per drain) |
| First failure inside a drain | Re-enqueue failed event at the **tail**, mark `hit_retry`, re-enqueue remaining events without trying | Push the whole batch back to the **front** of the buffer in reverse order | Re-enqueue every event, reset `channel = None` to force re-dial |
| Subsequent events in the same drain | Skipped — re-enqueued unsent | n/a (batched) | n/a (batched) |
| Order preservation across retries | FIFO **only** among events that succeeded; failed events drift to the tail | Strict — failed batch retries before any newer events drain | Strict — failed batch is the first thing the next attempt sends |
| Effect on flush latency for poison events | Bounded — newer events still ship; bad event keeps cycling at the tail | Head-of-line blocking — buffer stuck behind the bad batch | Head-of-line blocking — drain breaks, retried on next opportunity |
| Backoff reset | Reset on first `Accepted` | Reset on `Accepted` | Reset implicitly by re-dial |

**When the differences matter:**

- **Plausible's tail-requeue** is the right call for analytics
  ordering tolerance — losing a few events to the tail beats
  blocking the queue behind a poison event.
- **OTLP's head-requeue** preserves strict log order, which OTel
  consumers (Tempo, Honeycomb) sometimes assume.
- **Bastyde's batch-requeue + redial** matches the gRPC stream
  model: a transient stream error is treated as fatal to the
  current channel; subsequent batches start from a fresh dial.

Apps that need strict ordering across all events should prefer
OTLP or Bastyde. Apps that prioritize availability under transient
server flakiness should prefer Plausible.

---

## 4. The `PrivacySettings` widget

Drop-in widget that surfaces every consent + RGPD obligation. Lives
in `bastyde-widgets`:

```rust
use bastyde::widgets::PrivacySettings;

let widget = PrivacySettings::new()
    .data_processor_name("MyCo SAS")
    .privacy_policy_url("https://example.com/privacy")
    .compact(false)              // first-run modal mode
    .show_inspect(true)          // "Inspect data sent" accordion
    .show_mode_switch(true)      // anonymous ↔ pseudonymous (when both adapters configured)
    .show_identity_row(true)     // install_id + Get my data + Erase my data
    .inspect_event_count(50);
```

Layout (in order, top-to-bottom):

```text
PrivacySettings
├── Heading
├── Plain-language Art. 13 notice
│     (controller, processor, purposes, lawful basis,
│      retention, withdrawal right, optional policy URL)
├── Per-scope toggles
│     (anonymous_metrics / crash_reports / feature_flags,
│      intersected with reporter.supported_scopes() — toggles
│      for unsupported scopes are HIDDEN, not just disabled)
├── Reject all  ←→  Accept all     (CNIL parity, GDPR Art. 7)
├── Identity row                   (pseudonymous mode only:
│     install_id display
│     Get my data         → opens a save-as-JSON file dialog
│     Erase my data       → confirm → server delete + local discard + withdraw)
├── Inspect data sent              (accordion; lists last N events)
├── Privacy mode switch            (when both adapters configured:
│     confirm → wipe install_id + queue + reset consent + flip mode)
└── Withdraw consent               (footer, equal prominence to Accept)
```

Confirmation dialogs (`MessageBox::question` + `OkCancel`) gate the
destructive actions: erase, withdraw, mode switch. Misclicks survive
a confirm step.

The "Inspect data sent" accordion auto-refreshes as events land —
the `recent_log_revision` signal triggers a widget rebuild whenever
`DynamicReporter::record` or `discard_pending` fires.

i18n: 42 keys under the `privacy-*` namespace in
[`crates/bastyde-widgets/locales/en-US.ftl`](../crates/bastyde-widgets/locales/en-US.ftl)
and [`fr-FR.ftl`](../crates/bastyde-widgets/locales/fr-FR.ftl). Apps install
the framework bundle via `I18nConfig::framework_locales(bastyde_widgets::framework_locales())`.

---

## 5. Configuration layering

Three places where telemetry behavior is set, in order of precedence:

| Concern | Where set | Mechanism |
|---------|-----------|-----------|
| Adapter type / wire format / API token | Build-time in the binary | Adapter builder calls in `main.rs` |
| Default mode, retention policy, processor name | App-builder time | `TelemetryBundle::with_*` |
| User's per-scope toggles | Runtime, per-user | `ConsentStore` + per-scope `SettingsKey<bool>` mirror |
| User's endpoint override | Runtime, per-deployment | `scopes::TELEMETRY_ENDPOINT_OVERRIDE` — apps feed this into adapter builder via `.endpoint_override(...)` |
| Active mode | Runtime | `DynamicReporter::active`, mutated by the widget |
| Install ID | Runtime, automatic | `SettingsFile<InstallIdFile>`, 13-month rotation |
| Consent decision | Runtime, persistent | `SettingsFile<ConsentFile>` |
| Pending events | Runtime, persistent (Plausible + Bastyde adapters) | redb at `AppPaths::data_dir().join("<adapter>-queue.redb")` |
| Pending events | Runtime, in-memory only (OTLP adapter) | `VecDeque<OwnedEvent>` — durability deferred to the OTel collector |

### Endpoint override

Set the `telemetry.endpoint_override` settings key in
`general.toml` to redirect all adapters at a different server
without rebuilding:

```toml
[telemetry]
endpoint_override = "https://my-other-collector.example.com:50051"
```

Apps wire it through:

```rust
let override_url = settings
    .signal_for(&bastyde_telemetry::scopes::TELEMETRY_ENDPOINT_OVERRIDE)
    .get();
let adapter = BastydeAdapter::builder()
    .endpoint("https://default-collector.example.com:50051")
    .endpoint_override(override_url)         // applies iff non-empty
    .product_id("my.app")
    .build();
```

Triggers the **recipient-change re-prompt rule** in `ConsentStore`:
when the endpoint stored at consent grant time differs from the
endpoint at app start, consent flips back to `Unknown` and the
widget re-asks. RGPD Art. 13 transparency.

---

## 6. RGPD / GDPR compliance summary

The framework provides the SDK plumbing; the **app developer is the
data controller** and remains responsible for the legal artifacts
(Art. 13 controller notice, privacy policy, DPA with processors, etc.).
What Bastyde *does* automate:

| Article | What Bastyde does |
|---------|------------------|
| **Art. 6(1)(a)** consent | `ConsentStore`. No event flows in `Unknown` or `Denied`. |
| **Art. 6(1)(f)** legitimate interest (anonymous mode) | Anonymous-mode adapters set `supported_scopes() = anonymous_metrics_only()` and report `install_id() = None`. CNIL audience-measurement-exemption posture by default. |
| **Art. 7(3)** right to withdraw | Withdraw button in the widget, equal prominence to Accept. |
| **Art. 13** transparency | Plain-language notice block in the widget — controller, processor, purposes, lawful basis, retention, recipients. |
| **Art. 15** right of access | "Get my data" button → `fetch_remote_data()` → JSON export with file-save dialog. |
| **Art. 17** right to erasure | "Erase my data" button → confirm → `erase_remote_data()` → local queue wipe + consent withdrawal. |
| **Art. 20** portability | `RemoteDataExport` is JSON-serializable, self-describing (includes `schema_version`, `endpoint`, `adapter`). |

Anonymous mode never collects per-user data, so Art. 15 / 17 / 20
buttons hide automatically — there's nothing to fetch or erase.

---

## 7. Code references

| File | Purpose |
|------|---------|
| [`crates/bastyde-core/src/telemetry/event.rs`](../crates/bastyde-core/src/telemetry/event.rs) | `Event`, `OwnedEvent`, `Prop`, `RemoteDataExport`, serde derives |
| [`crates/bastyde-core/src/telemetry/reporter.rs`](../crates/bastyde-core/src/telemetry/reporter.rs) | `UsageReporter` trait, `TelemetryError` |
| [`crates/bastyde-telemetry/src/bundle.rs`](../crates/bastyde-telemetry/src/bundle.rs) | `TelemetryBundle`, `OpenedTelemetry`, `PrivacyPolicy` |
| [`crates/bastyde-telemetry/src/consent.rs`](../crates/bastyde-telemetry/src/consent.rs) | `ConsentStore`, `ConsentFile`, settings-mirror integration |
| [`crates/bastyde-telemetry/src/install_id.rs`](../crates/bastyde-telemetry/src/install_id.rs) | `InstallId` with 13-month rotation |
| [`crates/bastyde-telemetry/src/dynamic_reporter.rs`](../crates/bastyde-telemetry/src/dynamic_reporter.rs) | `DynamicReporter`, recent-log tee, revision signal |
| [`crates/bastyde-telemetry/src/queue.rs`](../crates/bastyde-telemetry/src/queue.rs) + [`queue/`](../crates/bastyde-telemetry/src/queue/) | `EventQueue` trait (`queue.rs`), `InMemoryEventQueue` (`queue/mem.rs`), `PersistentEventQueue` (`queue/persistent.rs`) |
| [`crates/bastyde-telemetry/src/scopes.rs`](../crates/bastyde-telemetry/src/scopes.rs) | `SettingsKey<bool>` constants for per-scope mirror, `TELEMETRY_ENDPOINT_OVERRIDE`, `TELEMETRY_REGION_OVERRIDE` |
| [`crates/bastyde-telemetry/src/ext.rs`](../crates/bastyde-telemetry/src/ext.rs) | `TelemetryExt` accessors on `BuildContext` / `EventContext` |
| [`crates/bastyde-widgets/src/privacy_settings.rs`](../crates/bastyde-widgets/src/privacy_settings.rs) | The widget |
| [`crates/bastyde-widgets/locales/en-US.ftl`](../crates/bastyde-widgets/locales/en-US.ftl) | i18n keys (`privacy-*`) |
| [`crates/bastyde-analytics-plausible/`](../crates/bastyde-analytics-plausible/) | Plausible adapter |
| [`crates/bastyde-analytics-bastyde/`](../crates/bastyde-analytics-bastyde/) | Home-grown gRPC adapter |

For the home-grown server backend, see the [`bastyde-collector`](../../bastyde-collector/) sibling repo.

---

## 8. Worked examples

- **[`examples/telemetry_plausible/`](../examples/telemetry_plausible/)** —
  anonymous mode against Plausible. Three intent buttons + the
  `PrivacySettings` widget. Default endpoint
  `http://127.0.0.1:8000/api/event`; override via
  `PLAUSIBLE_ENDPOINT` env var.

- **[`examples/telemetry_bastyde/`](../examples/telemetry_bastyde/)** —
  anonymous OR pseudonymous against a self-hosted `bastyde-collector`.
  Env vars: `BASTYDE_ENDPOINT`, `BASTYDE_PRODUCT_ID`, `BASTYDE_TOKEN`,
  `BASTYDE_INSTALL_ID` (set to flip to pseudonymous), `BASTYDE_TLS_CA`,
  `BASTYDE_TLS_DOMAIN`. See the example's docstring for the complete
  run procedure including spinning up the sibling `bastyde-collector`.
