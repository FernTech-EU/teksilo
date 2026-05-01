# `fern-collector` + `fern-dashboard` — gRPC analytics ingestion service & desktop dashboard

> **Status: planning.** Sibling document to
> [`telemetry-plan.md`](telemetry-plan.md) Phase 2.6. The framework
> side (client adapter, proto crate location, public trait surface)
> is committed there; this document covers the operated server and
> the desktop dashboard.

## Goals & non-goals

### Goals

1. **Multi-product, multi-year, non-stop.** Several FernUI
   applications (Skribisto, future apps, possibly third-party apps the
   author ships under FernTech) pump events into one collector
   instance. The instance runs unattended for years, with a growing
   database, with zero-downtime deploys.
2. **Anonymous and pseudonymous in the same server.** Mode is per
   request (gRPC metadata), not per server. Operators don't run two
   instances to support both.
3. **Wire format under our control.** Proto3 schema, additive
   evolution, no vendor lock-in.
4. **Optional encrypted transport.** TLS via tonic. Localhost dev runs
   plain; production runs TLS terminated at the binary (no nginx).
5. **Usable interface.** A FernUI desktop app for browsing the data —
   product picker, time range, event funnels, install_id detail,
   per-day breakdown.
6. **Self-contained operations.** Single binary collector, single
   binary dashboard, one DB process. No Kubernetes required to run
   one's own analytics.

### Non-goals (v1)

- **No web dashboard.** Browsers can't speak gRPC natively, and
  building both desktop *and* web doubles the effort. The desktop
  dashboard doubles as a FernUI showcase. Web can come later via
  `tonic-web` if there's demand — the gRPC service definition won't
  change.
- **No real-time live-tailing UI in v1.** Periodic refresh (e.g.,
  every 30 s) is enough for the analytics use case. Streaming
  query results stays in the proto for future use.
- **No alerting / anomaly detection.** Out of scope. Use a generic
  monitoring stack (Prometheus + Grafana) on top of the collector if
  needed.
- **No multi-tenant SaaS.** This is *operator-runs-one-instance-for-
  their-products*, not a hosted multi-tenant service. Per-product API
  keys exist for separation between *the operator's own products*,
  not between unrelated tenants.
- **No SDK for non-FernUI clients in v1.** The proto file is
  documented and stable, so anyone can generate a client; we don't
  ship one for Python / Go / Swift until there's a real ask.

## Architecture overview

```text
┌─────────────────────────────────────────┐
│  FernUI app instances (Skribisto, …)    │
│  ┌─────────────────────────────────┐    │
│  │ fern-analytics-fern             │    │
│  │  - tonic::Channel               │    │
│  │  - bidi-stream batched ingest   │    │
│  │  - redb persistent queue        │    │
│  └────────────────┬────────────────┘    │
└───────────────────┼─────────────────────┘
                    │  gRPC over TLS (mTLS optional)
                    │  Authorization: Bearer prod_<uuid>
                    ▼
┌─────────────────────────────────────────┐
│  fern-collector binary                  │
│  ┌─────────────────────────────────┐    │
│  │ tonic ingest service            │    │
│  │  - per-product token auth       │    │
│  │  - schema_version check         │    │
│  │  - rate limit per product       │    │
│  │  - in-memory batching buffer    │    │
│  └────────────────┬────────────────┘    │
│                   │ async batch insert  │
│  ┌────────────────▼────────────────┐    │
│  │ Storage trait                   │    │
│  │  - ClickHouseStorage            │    │
│  │  - PostgresStorage (Timescale)  │    │
│  │  - DuckDBParquetStorage         │    │
│  └────────────────┬────────────────┘    │
│                   │                     │
│  ┌────────────────▼────────────────┐    │
│  │ tonic query/admin service       │    │
│  │  - per-product token auth       │    │
│  │  - dashboard API surface        │    │
│  │  - GDPR fetch / erase endpoints │    │
│  └─────────────────────────────────┘    │
└────────────────┬────────────────────────┘
                 │  gRPC over TLS
                 ▼
┌─────────────────────────────────────────┐
│  fern-dashboard (FernUI desktop app)    │
│  ┌─────────────────────────────────┐    │
│  │ Qleany-generated backend        │    │
│  │  - entities: Product,           │    │
│  │    Connection, SavedQuery,      │    │
│  │    ChartView, FilterPreset      │    │
│  │  - features: connections,       │    │
│  │    queries, dashboards          │    │
│  │  - ICollectorService trait      │    │
│  │    impl in outer layer          │    │
│  └────────────────┬────────────────┘    │
│  ┌────────────────▼────────────────┐    │
│  │ FernUI widget tree              │    │
│  │  - product picker               │    │
│  │  - time-range picker            │    │
│  │  - event leaderboard            │    │
│  │  - per-day breakdown chart      │    │
│  │  - install_id detail viewer     │    │
│  │  - SavedQuery CRUD              │    │
│  └─────────────────────────────────┘    │
└─────────────────────────────────────────┘
```

## Repository layout — sub-crate of fern-ui or separate project?

**Recommendation: one separate repository, one Cargo workspace,
eight crates inside it.**

```text
fern-collector/                          (NEW REPO — single Cargo workspace)
├── Cargo.toml                           (Qleany generates v0; you own it after)
├── Cargo.lock
├── qleany.yaml                          (dashboard manifest only)
├── README.md
├── docs/
│   ├── deploy-clickhouse.md
│   ├── deploy-duckdb.md
│   ├── tls-setup.md
│   └── proto-versioning.md
├── proto/
│   └── telemetry/
│       └── v1.proto                     (single source of truth)
└── crates/
    ├── fern-collector-proto/            (hand-written; tonic-build → client + server stubs)
    ├── fern-collector-storage/          (hand-written; Storage trait + ClickHouse/DuckDB/Postgres impls)
    ├── fern-collector/                  (hand-written; server binary, depends on -proto + -storage)
    ├── common/                          (Qleany-generated; dashboard entities & events)
    ├── direct_access/                   (Qleany-generated; per-entity controllers, repos, use cases)
    ├── macros/                          (Qleany-generated; helper proc macros)
    ├── frontend/                        (Qleany-generated; AppContext, EventHubClient, services
    │                                     — re-exports common + direct_access + macros so the
    │                                     FernUI app crate only ever depends on `frontend`)
    └── fern_dashboard_app/              (hand-written FernUI desktop app; deps =
                                          `fern-ui`, `fern-i18n`, `frontend` only)

fern-ui/                                 (this repo, framework only)
└── crates/
    └── fern-analytics-fern/             (NEW: client adapter, git-dep on fern-collector-proto)
```

### Rationale

#### Why one workspace, not two

Per Qleany's [regeneration-workflow doc](#references), `Cargo.toml`
is a **Scaffold-nature file**: generated once when you first run the
generator, then yours to modify freely. Qleany won't touch it on
subsequent runs unless you explicitly request `qleany generate file
Cargo.toml` (and even then the GUI defaults to a temp folder so you
diff before merging). So the dashboard's Qleany-generated crates
(`common`, `direct_access`, `macros`, `frontend`) and the
hand-written server crates (`fern-collector-proto`,
`fern-collector-storage`, `fern-collector`) coexist comfortably in
one workspace `Cargo.toml` that you author after the initial Qleany
run.

Benefits of a single workspace:

- One `Cargo.lock` — shared dep versions across server and dashboard
  (e.g., both depend on `fern-collector-proto`).
- One `cargo build`, one `cargo test`, one CI matrix.
- Server and dashboard share `[workspace.dependencies]` for
  `tonic`, `prost`, `tokio`, etc.
- The proto crate is path-dep-able from either side — no git/version
  juggling between sibling workspaces.
- Cargo's per-target dep-tree culling means the server binary
  doesn't pull FernUI; the dashboard doesn't pull
  ClickHouse/Postgres clients. The "shared workspace" doesn't bloat
  either binary.

The only ergonomic cost is that `cargo` builds intermediate caches
for the union of deps; in practice irrelevant on any dev machine.

**Why the proto crate goes in `fern-collector`, not in `fern-ui`**

- The proto schema is the contract between client and server. The
  server is *the* canonical consumer; the client adapter follows.
- `fern-ui` is a framework. Adding a path-dep on a sibling repo's
  proto crate keeps `fern-ui` self-contained — `fern-analytics-fern`
  depends on `fern-collector-proto = { git = "..." }` (or a
  published crate version), not on a sibling-directory path.
- Versioning the proto independently of either client or server
  keeps the wire contract honest. Bump `fern-collector-proto` to 0.2
  → every consumer (client + server) updates explicitly.

**Why `fern-collector` is a separate repo from `fern-ui`**

- It's an application + a service, not a framework. Different
  audience (operators vs. app developers), different release cadence,
  different licensing freedom (could be MPL/AGPL while fern-ui stays
  proprietary).
- Server-side dependencies (tonic, sqlx/clickhouse-rs, tracing,
  hyper) and Qleany-generated code don't belong in the framework
  workspace.
- Keeps `fern-ui`'s test matrix focused on the framework itself.

#### When to consider splitting later

If `fern-dashboard` ever becomes a polished end-user product
shippable independently of the collector (e.g., "self-host the
collector once, install the dashboard on every laptop in the team"),
move it to its own repo at that point. Splitting after the fact is
mechanical: the dashboard already lives in clearly-named crates with
no source dependencies on the server crates beyond `fern-collector-
proto`. Until then, one repo is simpler.

### Files Qleany owns vs files you own (in the dashboard half)

Per Qleany's three file-natures from the regeneration doc:

| Nature           | Examples                                                          | How to treat                                                          |
|------------------|-------------------------------------------------------------------|-----------------------------------------------------------------------|
| Infrastructure   | entity structs, repositories, DTOs, table/cache defs, event glue  | Regenerate freely on every manifest change.                           |
| Scaffold         | `Cargo.toml`, `qleany.yaml`, `main.rs`, use-case bodies           | Generated once; modify freely; protect from accidental regeneration.  |
| Aggregate        | `common/event.rs`, `common/entities.rs`, `direct_access/lib.rs`   | Regenerated on entity add/remove; manual merge if you've modified.    |

The hand-written server crates (`fern-collector-proto`,
`fern-collector-storage`, `fern-collector`, and the `fern_dashboard_app`
front-end binary) are entirely outside Qleany's awareness — Qleany
won't generate them, won't list them, won't touch them. They're just
extra workspace members the user adds to the Cargo.toml after the
initial scaffold (`qleany generate --all` on the greenfield repo;
every subsequent regeneration uses the scoped
`qleany generate file <paths>` form — see "Generation discipline" §).

## Wire format — `proto/telemetry/v1.proto`

Sketch only. The real file lives in the `fern-collector-proto` crate.

```protobuf
syntax = "proto3";
package fern.telemetry.v1;

import "google/protobuf/timestamp.proto";

// ---------- Ingest ----------

service Telemetry {
  // Bidirectional stream: client opens one stream per session,
  // pushes EventBatch messages, server acks each batch by id.
  // Server may close the stream with a status (auth failure, rate
  // limit, schema rejection); client reconnects with backoff.
  rpc Ingest (stream EventBatch) returns (stream IngestAck);

  // Single fetch — used by the FernUI app's "Get my data" button
  // (GDPR Art. 15 + 20). Returns every event recorded under the
  // install_id in the request, paginated.
  rpc Fetch (FetchRequest) returns (stream FetchPage);

  // Erase — GDPR Art. 17. Deletes every event under the install_id.
  rpc Erase (EraseRequest) returns (EraseAck);
}

// ---------- Query (used by fern-dashboard) ----------

service Query {
  // Aggregate count by event name in a time range, grouped by an
  // optional dimension (os, app_version, etc).
  rpc CountEvents (CountRequest) returns (CountResponse);

  // Time-bucketed counts for charts. Bucket = minute / hour / day,
  // server-chosen based on range width.
  rpc EventsOverTime (TimeSeriesRequest) returns (TimeSeriesResponse);

  // Distinct install_ids active in a time range (pseudonymous mode
  // products only — anonymous-mode products return 0).
  rpc ActiveInstalls (ActiveInstallsRequest) returns (ActiveInstallsResponse);

  // List products visible to the caller's API token (typically all
  // products, since dashboard tokens are operator-scoped).
  rpc ListProducts (ListProductsRequest) returns (ListProductsResponse);
}

// ---------- Admin ----------

service Admin {
  // Mint a new per-product API token.
  rpc CreateProductToken (CreateProductTokenRequest) returns (ProductToken);

  // Revoke a token.
  rpc RevokeProductToken (RevokeProductTokenRequest) returns (RevokeAck);

  // Per-product retention policy override.
  rpc SetRetention (SetRetentionRequest) returns (RetentionAck);

  // Server health + storage stats.
  rpc Health (HealthRequest) returns (HealthResponse);
}

// ---------- Types ----------

message EventBatch {
  uint64 batch_id = 1;
  string product_id = 2;          // matched against bearer token's allowed scope
  TelemetryMode mode = 3;
  uint32 schema_version = 4;
  repeated Event events = 5;
}

enum TelemetryMode {
  TELEMETRY_MODE_UNSPECIFIED = 0;
  TELEMETRY_MODE_ANONYMOUS = 1;
  TELEMETRY_MODE_PSEUDONYMOUS = 2;
}

message Event {
  string name = 1;                // e.g. "intent.dispatched"
  EventCategory category = 2;
  google.protobuf.Timestamp timestamp = 3;
  optional string install_id = 4; // present iff mode == PSEUDONYMOUS
  string session_id = 5;
  repeated Prop props = 6;
}

enum EventCategory {
  EVENT_CATEGORY_UNSPECIFIED = 0;
  EVENT_CATEGORY_INTENT = 1;
  EVENT_CATEGORY_LIFECYCLE = 2;
  EVENT_CATEGORY_NAVIGATION = 3;
  EVENT_CATEGORY_CENSUS = 4;
  EVENT_CATEGORY_CUSTOM = 5;
}

message Prop {
  string key = 1;
  oneof value {
    string str = 2;
    uint32 u32 = 3;
    int64 i64 = 4;
    bool boolean = 5;
    F64Bucket f64_bucket = 6;
    HistogramStrU32 histogram_str_u32 = 7;
  }
}

message F64Bucket { int64 min_x100 = 1; int64 max_x100 = 2; }
message HistogramStrU32 { repeated HistogramEntry entries = 1; }
message HistogramEntry { string key = 1; uint32 count = 2; }

message IngestAck {
  uint64 batch_id = 1;
  uint32 events_accepted = 2;
  uint32 events_rejected = 3;
  string rejection_reason = 4;    // empty when rejected == 0
}

// ---------- Versioning notes ----------
//
// Tag numbers are NEVER reused. Fields are NEVER renamed. New
// fields get new tag numbers. Removed fields are commented out
// with `reserved`. This is the rule, not advice.
//
// schema_version on EventBatch refers to the *application's* event
// schema (see fern-telemetry events.yaml versioning), not this
// proto's wire version. Wire-version negotiation lives in the
// crate version of fern-collector-proto.
```

## Storage

The collector defines a `Storage` trait and ships three impls. The
binary picks one at startup via config.

```rust
// fern-collector-storage/src/lib.rs (sketch)
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    async fn append_batch(&self, product_id: &str, batch: EventBatchRef<'_>) -> Result<()>;
    async fn fetch_install(&self, product_id: &str, install_id: &str, page: PageCursor) -> Result<FetchPage>;
    async fn erase_install(&self, product_id: &str, install_id: &str) -> Result<u64>;
    async fn count_by_name(&self, q: CountQuery<'_>) -> Result<Vec<(String, u64)>>;
    async fn time_series(&self, q: TimeSeriesQuery<'_>) -> Result<Vec<TimeBucket>>;
    async fn active_installs(&self, q: ActiveQuery<'_>) -> Result<u64>;
    async fn enforce_retention(&self) -> Result<RetentionReport>;
    async fn health(&self) -> Result<StorageHealth>;
}
```

Three implementations:

### `ClickHouseStorage` — recommended for serious scale

- One table per product or one table partitioned by `(product_id,
  toYYYYMMDD(timestamp))`.
- Native columnar compression — events compress 10–20× over JSON.
- TTL clauses auto-drop old partitions per product retention.
- Async batch inserts via `clickhouse-rs`. Server buffers up to
  `--clickhouse-batch-size` events or `--clickhouse-flush-interval`
  before flushing.
- Replication via ClickHouse Keeper / Zookeeper — both supported in
  recent versions.
- Operationally: one ClickHouse server is fine for 100M events/day.
  Replicated 3-node cluster handles billions/day.

### `PostgresStorage` — recommended for "I already run Postgres"

- TimescaleDB extension turns it into a first-class time-series DB
  with hypertables and continuous aggregates.
- Native HA story (Patroni, pg_auto_failover) is mature.
- Runs anywhere Postgres runs (CockroachDB compat is partial).
- Good if existing infra has Postgres ops experience.

### `DuckDBParquetStorage` — recommended for self-hosters

- Each batch appends rows to a per-day-per-product Parquet file.
- Queries run via DuckDB against the Parquet files in a temporary
  in-process database.
- Zero ops: just files on disk. Backups = `rsync` the directory.
- Great up to ~10M events/day per product.
- Cheap object-storage tier for old data: move old Parquet files to
  S3-compat object storage; DuckDB reads them transparently.

Default for the ship-ready binary: `DuckDBParquetStorage`. Operators
flip to ClickHouse/Postgres when they outgrow it.

## Multi-product authentication

- Per-product bearer token in gRPC metadata:
  `Authorization: Bearer fct_<base32-id>_<base32-secret>`.
- Token rows stored in a metadata DB (separate small SQLite or
  Postgres table — *not* the analytics DB).
- Each token has a scope: `product_ids: [String]`, `roles:
  [Ingest|Read|Admin]`.
- Server validates token, extracts allowed product_ids, rejects any
  EventBatch / query for a product the token doesn't list.
- Operator dashboard tokens get `Admin + Read` for all product_ids.
- Per-app ingest tokens get `Ingest` for one product_id.
- mTLS optional — clients present a client cert; the cert's CN maps
  to a product. Useful for auto-rotated machine certs (cert-manager,
  Vault PKI). Not required for v1.

Token lifecycle:

1. Operator runs `fern-collector-cli token mint --product skribisto
   --role ingest`. Server writes token to metadata DB, prints it.
2. Operator pastes the token into the FernUI app's
   `fern-analytics-fern` builder config or env var.
3. Token is revocable from the dashboard (Admin service).

## Deployment / HA / backup

### Single-VPS minimum

- One `fern-collector` binary + DuckDB+Parquet storage on a $5/mo
  Hetzner VPS.
- Caddy or rustls-acme for TLS cert auto-rotation (or just put
  `tonic::transport::ServerTlsConfig` straight on the binary with a
  Let's Encrypt cert from `acme.sh`).
- systemd service file for restart-on-failure.
- Daily `borg` backup of the Parquet directory + metadata DB to a
  remote.

### Multi-VPS HA

- Two collector instances behind a TCP load balancer (HAProxy,
  nginx-stream, or a managed LB).
- Storage migrated to ClickHouse (3-node cluster) or Postgres
  (Patroni 3-node).
- Backups: continuous WAL archiving (Postgres) or
  ClickHouse-Keeper-driven snapshots.
- Zero-downtime deploy: rolling restart of the collector instances.
  The redb-backed client queue handles the brief LB unavailability
  during cutover (events buffered, flushed when an instance comes
  back).

### Health monitoring

- The collector exposes `Admin.Health()` returning storage status,
  recent ingest rate, and queue depths.
- Optional Prometheus `/metrics` HTTP endpoint on a separate port
  (off the gRPC service). Standard observability. Don't dogfood —
  use a separate stack for monitoring the analytics service.

## Retention & GDPR

- Per-product retention policy (default 90 days) stored in the
  metadata DB.
- Storage layer's `enforce_retention()` runs on a cron-like ticker
  inside the server (every 6h).
- For ClickHouse / Postgres: TTL clauses + scheduled `OPTIMIZE TABLE`.
  For DuckDB+Parquet: delete old per-day files.
- GDPR Art. 17 erase: `Telemetry.Erase` deletes every row under
  `(product_id, install_id)`. Returns count erased.
- GDPR Art. 15 + 20 fetch: `Telemetry.Fetch` paginates events under
  `(product_id, install_id)`, returns them in a wire-portable
  `RemoteDataExport` shape (the proto mirrors `fern_core::telemetry::
  RemoteDataExport`).

## `fern-dashboard` — Qleany-managed FernUI desktop app

This section is the dashboard side. Read alongside the
[fern-test-app reference](../../../fern-test-app/) which is the
canonical "Qleany backend + FernUI frontend" example in this
codebase.

### Dependency boundary

The FernUI app crate (`fern_dashboard_app`) **only consumes Qleany's
`frontend` crate**, never `direct_access` / `common` / `macros`
directly. `frontend` re-exports everything the UI needs (entities,
DTOs, controllers, the event hub client, the `AppContext`,
`ICollectorService`). This is the same convention as
[fern-test-app/crates/fern_app/Cargo.toml](../../../fern-test-app/crates/fern_app/Cargo.toml),
whose deps are exactly `fern-ui`, `fern-i18n`, and `frontend` — and
nothing else from the Qleany side.

Why this matters:

- One re-export hub means UI imports stay short
  (`use frontend::commands::query_commands::run_query;`) and the
  `frontend` crate is the single place where the "what's part of the
  public backend API" decision lives.
- Adding or removing a Qleany-generated crate (e.g. introducing a
  new feature in the manifest) doesn't ripple into the UI's
  `Cargo.toml` — only `frontend` changes.
- `ICollectorService` and its tonic-backed implementation
  (`TonicCollectorService`) live inside the `frontend` crate per
  Qleany's "Online APIs / external services" guidance — so the gRPC
  client is on the same side of the dependency boundary as
  everything else the UI talks to. The UI never sees `tonic`
  directly.

### Why Qleany

- The dashboard manages structured data (Products, Connections,
  SavedQueries, ChartViews, FilterPresets) with relationships and
  user-edit semantics — the use case Qleany is built for.
- Undo/redo is a real desktop expectation for "I deleted my saved
  query".
- The Qleany event hub gives a clean way to push collector
  responses into the UI: collector reply → service-layer publishes
  an event → UI subscribes via the `EventSource` adapter pattern
  from [fern-test-app/crates/fern_app/src/main.rs:24-44](../../../fern-test-app/crates/fern_app/src/main.rs).
- Per the Qleany design doc, "Online APIs / external services" is
  exactly the pattern for the gRPC client: define
  `ICollectorService` in the outer layer, instantiate it once at
  startup, store it in the `ServiceLocator`, inject it into use
  cases that need it.

### Manifest design notes

A few of the entity fields use Qleany's **complex Rust enum
variants** escape hatch (Manifest Reference §"Complex Enum Variants
(Rust Only)"): tuple- and struct-shaped variants that carry data
inline. They replace patterns that would otherwise need parallel
DTOs or auxiliary entity rows:

| Field                     | Variants used                                    | What we avoided                                          |
|---------------------------|--------------------------------------------------|----------------------------------------------------------|
| `Connection.tls`          | `Plain`, `ServerOnly`, `MutualTls{…}`            | A flat `tls_enabled: bool` plus two unused path fields.  |
| `SavedQuery.time_range`   | `Last24h…`, `Custom{from,to}`                    | A separate `TimeRange` entity + many-to-one relationship.|
| `SavedQuery.event_filter` | `All`, `ByName(String)`, `Compound(Vec<String>)` | A `SavedQueryFilter` entity per query.                   |

The Qleany docs explicitly recommend these "when simple flat enums
are not enough, typically when you would otherwise need multiple
entities or DTOs to model the same concept." That's exactly the
fit here. Use cases pattern-match on the variants directly; UniFFI
copes with them natively if mobile bridges are ever added.

Trade-off acknowledged: complex variants land in JSON differently
than flat enums (serde defaults to externally-tagged for tuple/
struct variants). The `dashboard_state` save/load use case bodies
need to handle the round-trip explicitly — easy enough, just calls
out for awareness.

### `qleany.yaml` sketch

```yaml
schema:
  version: 5
global:
  language: rust
  application_name: FernDashboard
  organisation:
    name: FernTech
    domain: com.ferntech
  prefix_path: crates
entities:
  - name: EntityBase
    only_for_heritage: true
    fields:
      - { name: id,         type: uinteger }
      - { name: created_at, type: datetime }
      - { name: updated_at, type: datetime }
    undoable: false

  - name: Connection
    inherits_from: EntityBase
    fields:
      - { name: name,      type: string }                    # "Production", "Local dev"
      - { name: endpoint,  type: string }                    # "https://collector.example.com:50051"
      - { name: api_token, type: string, sensitive: true }   # bearer token
      # Complex Rust enum (Qleany escape hatch — tuple + struct variants).
      # Replaces a flat `tls_enabled: boolean` plus N optional path fields
      # with a single algebraic type the use case can match on.
      - name: tls
        type: enum
        enum_name: TlsConfig
        enum_values:
          - Plain                                            # http://, no TLS
          - ServerOnly                                       # https://, validate server cert
          - "MutualTls { client_cert_path: String, client_key_path: String }"
    undoable: true

  - name: Product
    inherits_from: EntityBase
    fields:
      - { name: connection,   type: entity, entity: Connection, relationship: many_to_one }
      - { name: product_id,   type: string }                 # matches the collector's product_id
      - { name: display_name, type: string }
      - name: mode
        type: enum
        enum_name: ProductMode
        enum_values:
          - Anonymous
          - Pseudonymous
          - Mixed                                            # both modes seen in this product
    undoable: true

  - name: SavedQuery
    inherits_from: EntityBase
    fields:
      - { name: name,    type: string }
      - { name: product, type: entity, entity: Product, relationship: many_to_one }
      # Complex Rust enum: presets carry no data, `Custom` carries the
      # Unix-second window inline. The use case matches on the variant
      # to compute the gRPC request's `from_unix_s` / `to_unix_s`.
      - name: time_range
        type: enum
        enum_name: TimeRange
        enum_values:
          - Last24h
          - Last7d
          - Last30d
          - "Custom { from_unix_s: i64, to_unix_s: i64 }"
      # Complex enum captures the filter shape directly. `Compound`
      # lets the user combine N name-matchers without a parallel
      # SavedQueryFilter entity.
      - name: event_filter
        type: enum
        enum_name: EventFilter
        enum_values:
          - All
          - "ByName(String)"
          - "ByCategory(String)"
          - "Compound(Vec<String>)"
      - name: group_by
        type: enum
        enum_name: GroupBy
        enum_values: [None, ByOs, ByAppVersion, ByLocale, ByThemeKind]
    undoable: true

  - name: ChartView
    inherits_from: EntityBase
    fields:
      - { name: query, type: entity, entity: SavedQuery, relationship: many_to_one }
      - { name: title, type: string }
      - name: chart_kind
        type: enum
        enum_name: ChartKind
        enum_values: [Bar, Line, Table]
    undoable: true

  - name: Dashboard
    inherits_from: EntityBase
    fields:
      - { name: name,    type: string }
      - { name: charts,  type: entity, entity: ChartView, relationship: ordered_one_to_many,
                          strong: true, list_model: true }
    undoable: true

  - name: Root
    inherits_from: EntityBase
    fields:
      - { name: connections, type: entity, entity: Connection, relationship: one_to_many,
                              strong: true, list_model: true }
      - { name: dashboards,  type: entity, entity: Dashboard,  relationship: one_to_many,
                              strong: true, list_model: true }
    undoable: false

features:
  # Every gRPC-backed query is a `long_operation: true` use case.
  # Qleany generates a 3-method controller API per use case
  # (`run_<uc>`, `get_<uc>_progress`, `get_<uc>_result`) plus a
  # `cancel_operation` shared by all of them, runs the body on a
  # dedicated thread, and pipes progress/completion through the
  # EventHub as `Origin::LongOperation(...)`. The FernUI app
  # subscribes to those events to refresh charts and tables.
  - name: collector_queries
    use_cases:
      - name: count_events
        long_operation: true
        read_only: true
        dto_in: { name: CountEventsDto, fields: [
            { name: connection_id, type: uinteger },
            { name: product_id,    type: string },
            { name: from_unix_s,   type: integer },
            { name: to_unix_s,     type: integer },
            { name: event_filter,  type: string, optional: true },
            { name: group_by,      type: string, optional: true } ] }
        dto_out: { name: CountEventsResultDto, fields: [
            { name: rows, type: string, is_list: true } ] }     # JSON-encoded buckets
      - name: events_over_time
        long_operation: true
        read_only: true
        # similar shape, returns time-bucketed counts
      - name: active_installs
        long_operation: true
        read_only: true
      - name: fetch_install
        long_operation: true
        read_only: true                                         # GDPR Art. 15 + 20
      - name: erase_install
        long_operation: true                                    # GDPR Art. 17 (writes server-side)
      - name: list_products
        long_operation: true                                    # network call, not bulk; still long_op for cancel + progress
        read_only: true
      - name: test_connection
        long_operation: true
        read_only: true                                         # health-check the gRPC endpoint + token

  # Ephemeral Database Pattern — load/save the in-memory entities
  # to/from a JSON file. See "Persistence — the Ephemeral Database
  # Pattern" §. Synchronous: small file, no need for long_operation.
  - name: dashboard_state
    use_cases:
      - name: load_state
        read_only: false
        dto_in:  { name: LoadStateDto,  fields: [ { name: path, type: string } ] }
        dto_out: { name: LoadStateResultDto, fields: [
            { name: connections_loaded, type: integer },
            { name: dashboards_loaded,  type: integer } ] }
      - name: save_state
        read_only: true
        dto_in:  { name: SaveStateDto,  fields: [ { name: path, type: string } ] }
        dto_out: { name: SaveStateResultDto, fields: [
            { name: bytes_written, type: integer } ] }
ui:
  rust_cli: false
  rust_slint: false
  cpp_qt_qtwidgets: false
  cpp_qt_qtquick: false
  rust_ios: false
  rust_android: false       # the FernUI frontend is hand-written, not generated
```

Note: `ui:` is all-false. The FernUI frontend in `crates/fern_app/` is
hand-written, like fern-test-app does. Qleany generates the backend
(`common`, `direct_access`, `frontend`, `macros`); the
`fern_dashboard_app` crate consumes only `frontend` and adds the
FernUI widget tree on top.

### Entity persistence vs UI settings — two distinct concerns

Per Qleany's design philosophy (`qleany docs design --md`, "User
settings and UI configuration"), the dashboard splits state along
the standard line:

| Concern                                                     | Where it lives  | Why                                                                                                                                            |
|-------------------------------------------------------------|-----------------|------------------------------------------------------------------------------------------------------------------------------------------------|
| Connections, Products, SavedQueries, ChartViews, Dashboards | Qleany entities | Business data the user is curating. Use cases + repositories own it. Fires entity events on change, undoable.                                  |
| Window geometry, theme, last-selected-tab, sidebar width    | `fern-settings` | UI state of the FernUI shell. No business meaning, no entity events, no undo expectation.                                                      |

#### Persistence — the Ephemeral Database Pattern

Qleany's Rust generation uses an in-memory HashMap repository at
runtime — and that doesn't change. Persistence is layered on top via
the **Ephemeral Database Pattern** documented in
`qleany docs rust --md`:

1. **Load**: a user-written use case transforms a file → the
   in-memory repositories at startup.
2. **Work**: every Qleany operation runs against the ephemeral
   in-memory database. No I/O on the hot path.
3. **Save**: a user-written use case transforms the in-memory
   repositories → a file at deliberate moments.

The internal entity model is decoupled from the on-disk format.
The user file (the "dashboard config bundle") can be JSON, RON,
SQLite — Qleany doesn't care; the load/save use cases are the
boundary.

The dashboard's `dashboard_state` feature in the manifest sketch
above declares the two use cases (`load_state` and `save_state`).
The user-written bodies walk every entity controller's
`get_all`/`create` API to round-trip through the standard interfaces;
no special back-door into the repositories.

File format: **JSON** at `AppPaths::data_dir().join("dashboard.json")`.
JSON is human-readable for diff/recovery, trivially serde-derivable
on the entity DTOs, and the volumes are kilobytes (a few hundred
entity rows total — connections, products, saved queries, charts,
dashboards). No SQLite, no DuckDB, no migration tooling needed at
this scale. If the file grows past a megabyte (it won't), revisit.

#### When Load and Save fire

- **Load**: the dashboard's `main.rs` runs the `load_state` use case
  *before* `FernAppBuilder::run()`. If the file doesn't exist
  (first launch), the use case is a no-op and the app starts with
  empty entity stores.
- **Save**: triggered two ways:
  1. **Automatic, after every successful entity mutation.** The
     dashboard subscribes to `Origin::DirectAccess(_, EntityEvent::
     Created | Updated | Removed | RelationshipChanged)` and
     dispatches `save_state` with a debounce (500 ms — same shape
     as `fern-settings::SettingsFile`'s debounced flush). One
     subscription, all entities covered.
  2. **On graceful shutdown** — `app_context.shutdown()` calls
     `save_state` synchronously to flush the latest debounced
     change, mirroring the fern-test-app pattern.

This gives the user a "config persists automatically" UX without
asking the framework to fight Qleany's design — the in-memory
repository is the runtime source of truth, the JSON file is the
durability layer, and the load/save use cases are the well-defined
boundary the docs prescribe.

#### What `fern-settings` keeps

`fern-settings` still owns the FernUI-shell side per its existing
contracts:

- `WindowStateService` — window geometry, position, maximize state
  (auto-wired by `FernAppBuilder` when `WindowConfig::id("main")`
  is set).
- `SettingsStore` keys for theme preference, default time range,
  last-selected-connection-id, last-selected-tab.

These never go through Qleany. The dashboard's `main.rs` reads
them directly the same way any FernUI app does.

### `ICollectorService` — the gRPC client behind a Qleany interface

Per Qleany's "Online APIs / databases / external services" design
guidance: the gRPC client never appears inside use cases. It hides
behind a service trait defined in the outer layer (`frontend`),
implemented by a tonic-backed struct.

```rust
// crates/frontend/src/services/collector.rs
//
// Sync trait — no async, no async-trait dep. The use case body
// (which runs on a Qleany-spawned long-operation thread) calls these
// methods directly. The implementation bridges sync→async internally
// via a tokio runtime held in `AppContext`.
pub trait ICollectorService: Send + Sync {
    fn list_products(&self, conn: &ConnectionDto, cancel: &CancelFlag)
        -> Result<Vec<ProductDto>>;
    fn count_events(&self, q: CountEventsParams, cancel: &CancelFlag)
        -> Result<Vec<(String, u64)>>;
    fn events_over_time(&self, q: TimeSeriesParams, cancel: &CancelFlag)
        -> Result<TimeSeriesData>;
    fn active_installs(&self, q: ActiveInstallsParams, cancel: &CancelFlag)
        -> Result<u64>;
    fn fetch_install(&self, product: &str, install_id: &str, cancel: &CancelFlag)
        -> Result<RemoteDataExport>;
    fn erase_install(&self, product: &str, install_id: &str)
        -> Result<u64>;
}

// Implementation lives next to ServiceLocator (created at app startup).
pub struct TonicCollectorService {
    runtime: Arc<tokio::runtime::Runtime>,
    channel_per_endpoint: std::sync::Mutex<HashMap<String, tonic::transport::Channel>>,
    // … per-connection auth interceptors, retry policies, etc.
}

impl TonicCollectorService {
    fn block_on<F: std::future::Future>(&self, fut: F) -> F::Output {
        self.runtime.block_on(fut)
    }
}
```

Use cases call the trait, not the gRPC client directly. The
`frontend::AppContext` (Qleany pattern) holds a `Arc<dyn
ICollectorService>` alongside the `EventHub`, `UnitOfWork`, and the
shared `Arc<tokio::runtime::Runtime>` that the service uses
internally.

### Long operations — Qleany's first-class pattern

Every gRPC-backed query is marked `long_operation: true` in the
manifest (see the `features:` block above). Qleany generates the
infrastructure for free: a dedicated worker thread per call, a
shared cancellation flag, a progress callback, and event-based
result delivery through the `EventHub`. The FernUI app never blocks
on a query and never invents its own thread management.

#### Generated controller surface (per use case)

For each `long_operation: true` use case Qleany generates three
controller methods plus a shared cancel:

```rust
// Generated by Qleany — do not edit:
impl CollectorQueriesController {
    pub fn run_count_events(&self, dto: CountEventsDto)
        -> Result<OperationId>;
    pub fn get_count_events_progress(&self, id: &OperationId)
        -> Option<OperationProgress>;
    pub fn get_count_events_result(&self, id: &OperationId)
        -> Option<Result<CountEventsResultDto>>;

    // Inherited from LongOperationManager — cancels any in-flight op:
    pub fn cancel_operation(&self, id: &OperationId);
}
```

`run_<uc>` returns synchronously with an `OperationId`; the worker
thread is already running. Progress and results are delivered two
ways the UI can choose between (or combine):

1. **Polling** — `get_<uc>_progress(id)` and `get_<uc>_result(id)`
   from a UI tick or a `Signal<u64>::animate_to(...)` driven loop.
2. **Events** (preferred) — subscribe to the `EventHub` for
   `Origin::LongOperation(Progress | Completed | Failed | Cancelled)`
   filtered by the `OperationId` carried in the event payload. The
   FernUI app uses the same `EventSource` adapter pattern as
   [fern-test-app/crates/fern_app/src/main.rs:24-44](../../../fern-test-app/crates/fern_app/src/main.rs)
   to bridge `EventHubClient` to `fern_core::EventSource`.

#### Use-case body

The user-edited use-case body implements the `LongOperation` trait
that Qleany scaffolds. The gRPC call goes inside, with the runtime
bridging sync→async:

```rust
// Hand-written body in crates/collector_queries/src/use_cases/count_events.rs
impl LongOperation for CountEventsUseCase {
    type Output = CountEventsResultDto;

    fn execute(
        &self,
        progress: Box<dyn Fn(OperationProgress) + Send>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self::Output> {
        progress(OperationProgress::indeterminate("Connecting…"));
        let svc = self.app_context.collector_service();
        let cancel_flag = CancelFlag::from(cancel);

        progress(OperationProgress::indeterminate("Querying server…"));
        let rows = svc.count_events(self.dto.clone().into(), &cancel_flag)?;

        progress(OperationProgress::done());
        Ok(CountEventsResultDto {
            rows: rows.into_iter().map(|(k, v)| serde_json::json!({k: v}).to_string()).collect(),
        })
    }
}
```

The `CancelFlag` is a tiny wrapper that lets the gRPC client call
`Channel::abort()` when the flag flips, so cancelling the operation
actually closes the in-flight HTTP/2 stream rather than waiting for
the response.

#### Why this is the right pattern (and not ad-hoc tokio)

- **Cancellation comes for free.** "Cancel query" button → button
  on_activate calls `controller.cancel_operation(id)` → flag flips
  → gRPC stream aborts → use case returns `Cancelled` → UI gets
  the event. No bespoke plumbing.
- **Progress comes for free.** A loading spinner with "Connecting…
  / Querying… / Decoding…" stages is just `progress(...)` calls in
  the use-case body.
- **Result delivery comes for free.** `Origin::LongOperation::Completed`
  carries the `OperationId` and serialized output; one
  `subscribe_event` call in `App::build()` covers every query type.
- **No interference with undo/redo.** Long operations bypass the
  undo stack (see Qleany docs §"Long Operations / Scenarios"). For
  read-only queries that's exactly what we want.
- **Non-undoable side effects are explicit.** `erase_install` is
  `long_operation: true` *without* `read_only: true` — the
  documented "modifies non-undoable entities" scenario from the
  Qleany guide. After completion the EventHub fires entity events
  that refresh the UI.

#### Save / load / pure-local use cases stay synchronous

Use cases that don't touch the network — saving a `Dashboard`,
listing `SavedQuery` rows, renaming a `Connection` — stay regular
synchronous use cases. They run on the calling thread (the UI
thread, since handlers fire from `on_activate_fn`), and the existing
Qleany unit-of-work + entity-event flow handles UI refresh.

### FernUI widget tree (sketch)

```text
Window
└── SplitView (vertical)
    ├── Sidebar
    │   ├── Connection picker (ListView<ConnectionDto>)
    │   ├── Product picker (ListView<ProductDto>, filtered by selected Connection)
    │   └── Saved queries (ListView<SavedQueryDto>)
    └── Main area (TabWidget)
        ├── Tab: Overview
        │   ├── Time-range picker (last 24h / 7d / 30d / custom)
        │   ├── KPI cards (total events, active installs, top events)
        │   └── Time-series chart (events over time, server-bucketed)
        ├── Tab: Events
        │   ├── Event-name leaderboard (TableView)
        │   └── Per-event drill-down (props histogram)
        ├── Tab: Installs (pseudonymous mode only)
        │   ├── Install_id search box
        │   └── Install detail viewer (events for one install_id)
        └── Tab: Admin
            ├── Token management
            ├── Per-product retention overrides
            └── Server health + storage stats
```

Charts: the dashboard consumes the planned **`fern-charts`** crate
(`BarChart` + `LineChart`) — a set goal in
[`widgets-plan.md`](widgets-plan.md) §"Charts (2D)". Both widgets
are bound to a `Signal<ChartSeries<T>>` populated by
`Origin::LongOperation::Completed` events from the
`collector_queries:*` use cases. No bespoke `paint()`-level chart
code in the dashboard; if the framework charts aren't ready when
sub-phase E lands, the dashboard ships placeholder
`TextWidget`-based summaries until they are. Pie/scatter/heatmap
etc. are explicitly out of scope per the widgets plan, which
matches the dashboard's needs (counts and time series, nothing
exotic).

### Working with the Qleany CLI

Per the Qleany intro doc ("AI ready" §), the toolchain ships
several commands designed for both interactive editing and
LLM-assisted implementation. The dashboard's manifest evolution and
use-case body authoring rely on them throughout.

#### Validation, listing, inspection

- **`qleany check`** — manifest schema + cross-reference validation.
  Run it before every regeneration; CI runs it on every push.
  `qleany check --rules` lists every rule the validator enforces
  (entity inheritance cycles, missing DTO references, field-type
  consistency, etc.).
- **`qleany list files`** — shows what would be generated, with
  status flags `[N]ew`, `[M]odified`, `[U]nchanged` and natures
  `[I]nfra` / `[A]ggregate` / `[S]caffold`. Use the filters
  (`--modified`, `--scaffolds`, `--infra`) to scope what you're
  about to overwrite.
- **`qleany list entities | features | groups`** — the manifest's
  surface area at a glance. Useful when adding a use case to an
  existing feature.
- **`qleany show feature collector_queries`** — prints the
  resolved feature definition (use cases, DTOs, entity references)
  with cross-references resolved. Faster than reading the YAML
  block when the manifest grows.
- **`qleany show config --format json`** — machine-readable
  `global:` section. Useful in scripts that need the
  `application_name` or `prefix_path`.
- **`qleany diff <file>`** — unified diff between what Qleany
  *would* write and what's on disk. The first thing to run when
  `qleany list files` flags a file as `[M]odified` and you don't
  remember which side changed.

#### LLM-assisted implementation — `qleany prompt`

`qleany prompt` is the single CLI surface for getting context-rich
prompts into Claude (or any other coding LLM). Three forms:

- **`qleany prompt --context`** — full project context (entities,
  features, generated layout, file natures, conventions). Drop the
  output into `.claude/CLAUDE.md` so every Claude session in the
  dashboard repo starts with the right mental model. Regenerate the
  file after every manifest change.
- **`qleany prompt --list`** — every use case in the manifest,
  flagged by whether its body has been implemented or still
  contains the generated `TODO`. Quick "what's left to wire up"
  view.
- **`qleany prompt --use-case feature:use_case`** — generates a
  guardrailed prompt for implementing the named use case. Includes
  the relevant entity DTOs, the unit-of-work shape, and the
  surrounding feature so the LLM doesn't have to grep.

#### Generation discipline — never run bare `qleany generate`

Bare `qleany generate` is a destructive operation: it writes every
file the manifest currently produces with `[N]ew` or `[M]odified`
status. That set includes Scaffold-nature files like
`fern_dashboard_app/src/main.rs` and use-case bodies — all the code
you've hand-edited. The "in temp" GUI default and the `--dry-run`
flag exist precisely because this is too easy to get wrong.

The discipline followed in this project: **always pass an explicit
file list to `qleany generate file <paths…>`.** That command takes
one or more paths and writes only those. It can be combined with
`feature <name>` / `entity <name>` / `group <name>` for batch
scoping, but never with the bare form.

Workflow for the dashboard:

```bash
# --- One-time initial scaffolding (empty repo, no hand-edits to lose):
qleany check                               # confirm manifest is valid
qleany prompt --context > .claude/CLAUDE.md
qleany generate --all                      # OK: nothing exists yet, nothing to lose
git add -A && git commit -m "qleany initial scaffold"

# --- Implementing each use case body:
qleany prompt --use-case collector_queries:count_events       \
  > /tmp/prompt-count-events.md
qleany prompt --use-case collector_queries:fetch_install      \
  > /tmp/prompt-fetch-install.md
qleany prompt --use-case dashboard_state:save_state           \
  > /tmp/prompt-save-state.md
# … then hand each prompt to Claude inside the dashboard repo.

qleany prompt --list                       # what still has TODOs

# --- After modifying the manifest (adding an entity, a use case):
qleany check                               # validate first
qleany list files --modified --new         # see what would be touched

# Generate ONLY the files you actually want to refresh — never the
# bare `qleany generate` (which would also overwrite Scaffold files
# like `fern_dashboard_app/src/main.rs` and use-case bodies you've
# hand-written).
qleany generate file                                                   \
  crates/common/src/entities/saved_query.rs                            \
  crates/common/src/event.rs                                           \
  crates/direct_access/src/saved_query/saved_query_repository.rs       \
  crates/direct_access/src/saved_query/saved_query_table.rs            \
  crates/direct_access/lib.rs                                          \
  crates/frontend/src/commands/saved_query_commands.rs

# Aggregate-nature files (event.rs, entities.rs, lib.rs) often need
# manual merge after a manifest change. Run `qleany generate --temp`
# for those and diff before merging:
qleany generate --temp file crates/common/src/event.rs
qleany diff crates/common/src/event.rs
# …merge by hand…

# Sanity-check before committing:
qleany list files --modified               # should show only the files you intended
git diff -- crates/                        # eyeball the changes
```

`qleany generate file` is idempotent and scoped — exactly the
property you want when the working tree contains hand-written code
the framework knows nothing about (Scaffold bodies, the server
crates that aren't even Qleany-managed). Treat the unscoped form
as a footgun that's only safe on the very first scaffold of a
greenfield repo (where nothing of yours exists to overwrite).

Embedded docs are also a valid prompt source for ad-hoc questions:

```bash
qleany docs all --md > /tmp/qleany-full-reference.md
qleany docs design --md                    # design philosophy
qleany docs flow --md                      # how operations flow
qleany docs undo --md                      # undo/redo architecture
qleany docs api-rust --md                  # controller / UoW APIs
qleany docs regen --md                     # regeneration safety rules
```

The dashboard repo's CI should run `qleany check` on every PR. The
release workflow runs `qleany list files --modified` and fails the
build if any infrastructure files differ from on-disk state — a
catch for "someone hand-edited a generated infra file by accident."

## Phasing for `fern-collector` + `fern-dashboard`

### Sub-phase A — proto + skeletal client + skeletal server

- `fern-collector-proto` crate with the v1.proto compiled.
- `fern-collector` binary with `Telemetry.Ingest` writing to
  `DuckDBParquetStorage`. No auth yet; localhost only.
- `fern-analytics-fern` adapter (lives in the fern-ui workspace)
  hitting it.
- Acceptance: a FernUI app emits events; events appear in a Parquet
  file readable by `duckdb -c "SELECT * FROM 'events_*.parquet'"`.

### Sub-phase B — auth + TLS + multi-product

- Per-product bearer tokens, validated server-side.
- TLS via `tonic::transport::ServerTlsConfig` and `ClientTlsConfig`.
- Multi-product table partitioning.
- `fern-collector-cli token mint` subcommand.
- Acceptance: two FernUI apps with different product tokens emit
  events; queries scoped per product return correctly; mTLS optional
  path verified.

### Sub-phase C — query service

- `Query.CountEvents`, `EventsOverTime`, `ActiveInstalls`,
  `ListProducts`.
- `Telemetry.Fetch` and `Telemetry.Erase`.
- Acceptance: `grpcurl` against the query service returns sane
  aggregates; fetch/erase round-trip through `fern-analytics-fern`'s
  `UsageReporter::fetch_remote_data` / `erase_remote_data`.

### Sub-phase D — storage swap-out

- `ClickHouseStorage` and `PostgresStorage` impls.
- Config selection at startup.
- Migration tool: dump from one storage, restore into another.
- Acceptance: same query returns identical results across all three
  storage backends in a fixture-based integration test.

### Sub-phase E — `fern-dashboard` v1

- Qleany manifest written; **`qleany check` passes**; backend
  generated via `qleany generate --all` once on the greenfield
  scaffold, then committed. From that point on, manifest updates
  go through the scoped `qleany generate file <paths>` workflow
  (see "Generation discipline" §) — never the bare form.
- `qleany prompt --context > .claude/CLAUDE.md` committed so future
  LLM-assisted edits start with the right project context.
- `ICollectorService` + `TonicCollectorService` impl living in
  `frontend/src/services/`. Use-case bodies for each
  `collector_queries:*` written with `qleany prompt --use-case ...`
  as the starting prompt.
- `dashboard_state` feature with `load_state` / `save_state` use
  cases implementing the Ephemeral Database Pattern. Save triggered
  by `EntityEvent::*` subscription with 500 ms debounce, plus
  shutdown flush.
- FernUI frontend with the four tabs above; `fern_dashboard_app`
  consumes only the `frontend` crate (Qleany side) plus
  `fern-ui`, `fern-i18n`, and `fern-charts` (framework side).
  Time-series and event-leaderboard tabs use `fern-charts::LineChart`
  and `fern-charts::BarChart`. If `fern-charts` lands after
  sub-phase E, ship the dashboard with `TextWidget` summaries first
  and swap to charts when the crate is available — the
  `Signal<ChartSeries<T>>` shape is the same.
- Acceptance: connect to a local `fern-collector`, browse events,
  add a `Connection` entity, add a `SavedQuery`, restart the app,
  and confirm both reload from `dashboard.json`. Window geometry
  and theme survive via `fern-settings` independently.
  `qleany prompt --list` reports zero remaining `TODO` use-case
  bodies for both features.

### Sub-phase F — production polish

- Backup + restore docs.
- Multi-instance HA deploy guide.
- Prometheus `/metrics` endpoint.
- Schema drift detector: server checks incoming `schema_version`
  against a per-product registry.
- Rate limiting per product.
- **CI integration:** `qleany check` on every PR; `qleany list files
  --modified --infra` fails the build if any infrastructure-nature
  file diverges from on-disk state (catches accidental hand-edits
  to generated code that should round-trip cleanly).
- `qleany prompt --context` regenerated and committed whenever the
  manifest changes (CI check: file is up-to-date).
- Acceptance: documented runbook for a $10/mo VPS and for a 3-node
  HA deploy; both backed by smoke tests.

## Decisions

1. **Repository layout — DECIDED.** Single repo `fern-collector`,
   single Cargo workspace, eight crates inside (see "Repository
   layout" §). Qleany generates `Cargo.toml` once as a Scaffold
   file; the user owns it from then on, so the hand-written server
   crates and the Qleany-generated dashboard backend coexist
   comfortably.
2. **Default analytics storage backend — DECIDED: DuckDB + Parquet.**
   Per-day-per-product Parquet files on disk; queries via DuckDB
   in-process. Single binary, zero external services, backups via
   `rsync`/`borg` of the directory. ClickHouse and Postgres+Timescale
   stay implemented behind the `Storage` trait for operators who
   outgrow DuckDB later, but DuckDB is the v1 default and the
   documented happy path.
3. **Metadata DB for tokens — DECIDED: separate small SQLite next
   to the binary.** Independent file from the analytics Parquet
   tree so the analytics data can be wiped (retention, GDPR mass
   erase, schema change) without losing auth. Stays cheap on disk
   (kilobytes). One `sqlx::SqlitePool` in the server, completely
   isolated from the `Storage` trait.
4. **Dashboard config storage — DECIDED: Qleany's Ephemeral Database
   Pattern with JSON file as the durability layer.**
   Per `qleany docs rust --md` ("Ephemeral Database Pattern"), the
   in-memory HashMap repositories stay as Qleany's runtime
   storage — they are *not* swapped. Persistence is layered on top
   via two user-written use cases in a `dashboard_state` feature:
   `load_state` (file → in-memory) at app start, `save_state`
   (in-memory → file) after every successful mutation (debounced
   500 ms) and on graceful shutdown. File: JSON at
   `AppPaths::data_dir().join("dashboard.json")`. The save trigger
   is an `EventHub` subscription on every
   `Origin::DirectAccess(_, EntityEvent::*)` flavor — one
   subscription, all entities covered.
   `fern-settings` keeps its scope: `WindowStateService` +
   `SettingsStore` keys for window geometry, theme, last-selected
   tab/connection. These never go through Qleany.
   See "Persistence — the Ephemeral Database Pattern" §.
5. **gRPC-Web for browser access later — DECIDED: no.** Desktop
   dashboard is the only client. The proto file stays clean of
   web-specific concessions.
6. **Operator-side feature flags for the dashboard — DECIDED: no.**
   The dashboard ships with all tabs visible. Tabs that have no
   data to show (e.g., the Installs tab when a product is anonymous-
   only) render an empty state explaining why. No per-deployment
   config surface to maintain.

## Progress log

Updated as each sub-phase ships. This is a working document — check
the dates when reading.

### Sub-phase A — proto + skeletal client + skeletal server  *(done 2026-04-30)*

- Repo `fern-collector/` initialized with one Cargo workspace, three
  crates: `fern-collector-proto`, `fern-collector-storage`,
  `fern-collector` (server bin) plus an `examples/smoke/` workspace
  member with `send` and `verify` binaries.
- `proto/telemetry/v1.proto` defines `Telemetry.Ingest` plus the
  `EventBatch` / `Event` / `Prop` shape. Built via `tonic-build`
  with bundled `protobuf-src` so consumers don't need a system `protoc`.
- `ParquetStorage` writes one Parquet file per ingested batch under
  the `--storage-dir` directory. Schema: 9 columns, `props_json`
  carries the per-event prop map serialized as JSON.
- `fern-analytics-fern` adapter in fern-ui implements `UsageReporter`
  (anonymous mode) on top of the Phase 2.5 redb persistent queue.
  Tonic + tokio + prost are private to this crate — never promoted
  to fern-ui's workspace deps.
- **Acceptance verified**: 7-event live e2e — server bound on
  `127.0.0.1:50111`, client posted via gRPC bidi-stream, parquet
  file round-tripped through arrow's reader, server graceful
  shutdown on SIGINT.
- **Tests**: 3 storage unit + 4 client integration. Workspace stayed
  green at 1544 fern-ui tests.

### Sub-phase B — auth + TLS + multi-product  *(done 2026-04-30)*

- Per-product bearer-token auth via a SQLite-backed `TokenStore`
  living in `crates/fern-collector/src/auth.rs`. Tokens are
  `fct_<id>_<secret>` (16 + 52 base32 chars); database stores
  `sha256(secret)` only, validated with constant-time compare via
  `subtle::ConstantTimeEq`.
- Server gained `--tokens-db`, `--tls-cert` / `--tls-key`,
  `--client-ca` (mTLS) flags. CLI subcommands: `fern-collector token
  mint | list | revoke`.
- `authenticate()` checks the role against an `allowed: &[Role]`
  parameter so each RPC declares which roles it accepts.
  `Telemetry.Ingest` requires `Ingest` or `Admin`.
- Per-product scope check at batch-handling time: token's product
  ≠ batch's product → NACK with reason string.
- `fern-analytics-fern` gained `.bearer_token(...)` and `.tls(cfg)`
  builder methods. `TlsClientConfig` carries CA cert, optional
  client cert+key for mTLS, optional domain override.
- **Acceptance verified live**: 2-product / 2-token e2e showed
  unauth requests rejected, wrong-token rejected, scope-mismatch
  NACKed, valid token + product accepted. TLS path verified with a
  self-signed end-entity cert (SAN=DNS:localhost,IP:127.0.0.1).
- **Tests**: 7 auth unit tests + 5 sub-phase-B client integration
  tests. fern-ui stayed at 1549.

### Sub-phase C — Query service + Telemetry.Fetch / Erase  *(done 2026-04-30)*

- `proto/telemetry/v1.proto` extended with `Query` service
  (`CountEvents`, `EventsOverTime`, `ActiveInstalls`,
  `ListProducts`) and `Telemetry.Fetch` / `Telemetry.Erase`.
- `Storage` trait gained six methods: `count_events`,
  `events_over_time`, `active_installs`, `list_products`,
  `fetch_install`, `erase_install`. `ParquetStorage` impl decodes
  only the columns it needs for aggregations (event_name +
  install_id + timestamp_unix_ms) — no full deserialization on the
  count path.
- `erase_install` rewrites parquet files atomically (write `.tmp`,
  rename); fully-emptied files are removed.
- Server's `Telemetry.Fetch` paginates results in 256-event chunks;
  `is_last` flag terminates the stream. Role gate: `Ingest|Admin`
  for fetch/erase, `Read|Admin` for `Query.*`.
- Auto-pick bucket size for `events_over_time` when client passes
  0 — snaps to 1 m / 1 h / 1 d boundaries.
- `FernAdapter` gained pseudonymous-mode support (`.install_id(...)`
  builder + matching wire-format `mode=Pseudonymous` flag).
  `UsageReporter::fetch_remote_data` calls `Telemetry.Fetch` and
  builds a `RemoteDataExport`; `erase_remote_data` calls
  `Telemetry.Erase`.
- **Acceptance verified live**: ingested 12 events (5 anon + 4
  alice + 3 bob), `Query.ListProducts` showed 12, `CountEvents`
  grouped correctly, `ActiveInstalls` reported 2 (anonymous events
  excluded). `Telemetry.Fetch alice` returned 4 events; `Erase
  alice` removed exactly those 4; post-erase verifier confirmed
  alice's parquet file was rewritten + removed.
- **Tests**: 8 new storage unit tests (count, time-series,
  active-installs, list-products, fetch, erase incl. file-removal),
  4 new client integration tests. fern-ui ended at 1553.

### Sub-phase D — storage swap-out (ClickHouse + Postgres)  *(done 2026-04-30)*

- Cargo features `clickhouse-storage` and `postgres-storage` (off
  by default) gate the new backend impls; `live-tests` opts into
  testcontainers-driven Docker integration.
- **`backends/clickhouse.rs`** — `MergeTree`, partitioned by day,
  `ORDER BY (product_id, timestamp_unix_ms)`. Aggregations push
  down to native CH SQL (`GROUP BY`, `count(DISTINCT)`, `intDiv`
  for time bucketing). Erase uses `ALTER TABLE … DELETE …
  SETTINGS mutations_sync = 2` so the rows are gone by the time
  the RPC returns — important for the GDPR Art. 17 contract.
- **`backends/postgres.rs`** — vanilla `events` table + two indexes
  (a partial one on `(product_id, install_id) WHERE install_id IS
  NOT NULL`). Bulk insert chunked at 5_000 rows. Compatible with
  Timescale (operator runs `create_hypertable` after open).
- **Server `--storage` flag** with `parquet` (default), `clickhouse`,
  `postgres` choices. Env-var fallbacks `FERN_COLLECTOR_*_URL`.
  Mismatched feature → friendly error pointing at the rebuild.
- **`fern-collector-migrate` binary** — copy events between any
  two backends. After the deferred-work delivery (below), uses
  `Storage::scan_product` for a true full-migration pass that
  includes anonymous-mode events.
- **Conformance suite** at `crates/fern-collector-storage/src/
  conformance.rs` — single canonical 12-event fixture, single
  `run_conformance(&dyn Storage)` async function. Same assertions
  run against all three backends; passes verify identical results.
- **Live integration tests** via `testcontainers` + Docker. Default
  `cargo test` stays Docker-free; opt in with
  `--features clickhouse-storage,postgres-storage,live-tests`.
- **Acceptance verified live**: PG container conformance passed
  in 9.9 s (image pull + schema + run); CH container conformance
  passed in 2.4 s (image already cached). Binary-level e2e
  `--storage clickhouse` ingested 12 events, `Query.*` returned
  expected aggregates, `Fetch + Erase` round-tripped, direct CH
  SQL `SELECT count()` confirmed `mutations_sync=2` synchrony.
- **Tests**: 11 storage unit tests + 3 conformance tests (Parquet
  always, ClickHouse + Postgres when feature flags + Docker
  available) = 14 total when all features on.

#### Sub-phase D deferred work — done 2026-04-30 (same day)

- **`scan_product` trait method** added: returns every event for
  a product, anonymous-mode events included. Implemented on all
  three backends. Conformance suite extended with an assertion
  that the scan returns 12 rows + 5 anonymous.
- **Migration tool rewritten** around `scan_product` — anonymous
  events now migrate correctly, no per-install enumeration needed.
  Live-verified end-to-end: 9 events (4 anon + 3 alice + 2 bob)
  written via `--storage parquet`, migrated to Postgres with one
  `fern-collector-migrate` invocation, queried back via
  `--storage postgres`, all aggregations match.
- **Deploy docs** written:
  - [`docs/deploy-clickhouse.md`](../../../fern-collector/docs/deploy-clickhouse.md)
    — single-node Docker quick-start, sizing table, schema reference,
    TTL/retention, S3 backups, 3-node replicated cluster sketch,
    direct-query access, monitoring.
  - [`docs/deploy-postgres.md`](../../../fern-collector/docs/deploy-postgres.md)
    — single-instance Docker, sizing, schema, TimescaleDB
    hypertable + compression policy, pg_dump + pgBackRest,
    Patroni HA cluster sketch, direct-query examples.

### Sub-phase E — `fern-dashboard` v1  *(done 2026-05-01)*

Shipped. `qleany.yaml` written and `qleany check` passes; six
backend crates (`common`, `direct_access`, `macros`,
`collector_queries`, `dashboard_state`, `frontend`) generated and
committed alongside the hand-written `fern_dashboard_app`.
`qleany prompt --context` written to `.claude/CLAUDE.md`.

`ICollectorService` and `TonicCollectorService` (bearer auth +
TLS / mTLS) live in `frontend/src/services/`; cache key includes
the TLS fingerprint so two `Connection`s with the same hostname
but different client identities get distinct channels. The
`MutualTls { client_cert_path, client_key_path }` variant of
`Connection.tls` is now honoured end-to-end — every Params struct
(`CountEventsParams`, `EventsOverTimeParams`,
`ActiveInstallsParams`) and the four positional-arg trait methods
(`list_products`, `test_connection`, `fetch_install`,
`erase_install`) carry a `TlsConfigRequest` plumbed from the
selected `Connection` entity.

`dashboard_state` `load_state` reconstructs both weak FKs
(serialised as plain entity columns) and **strong-ownership
junctions**: a second pass calls `set_dashboard_relationship`
(`Charts`) and `set_root_relationship` (`Connections`,
`Dashboards`) so a restart restores the on-screen ordering of
strong children, not just the orphan entities.

`save_state` runs through a 500 ms debouncer
(`fern_dashboard_app::save_debouncer::SaveDebouncer`): every
`EntityEvent::*` ping resets the deadline; the worker thread
serialises after the burst and `flush_now` is called
synchronously at shutdown after `app_context.shutdown()`. Single
disk write per drag-burst of edits.

FernUI frontend ships the four-tab layout (Overview / Events /
Installs / Admin) plus the sidebar with three live pickers
(Connection, Product, SavedQuery) cascading via a shared
`DashboardSelection` bundle of signals. Selecting a Connection
filters Products; selecting a Product writes both the entity id
**and** the wire-level `product_id` slug, used by every tab in
place of the legacy `FERN_PRODUCT_ID` env var (kept as a
fallback). Overview uses `fern_charts::LineChart` for the
time-series; Events uses `TableView` for the leaderboard.
Installs replaces the `FERN_INSTALL_ID` env var with an in-app
`TextInput` (gated behind the `rich-text` feature on `fern-ui`),
seeded from the env var when set.

Window geometry and theme persist via `fern-settings` /
`fern-app`'s auto-save wiring.

### Sub-phase F — production polish  *(not started)*

Backups + restore docs (partial — covered for both DBs above),
multi-instance HA (sketches in deploy docs; full deploy guides
TBD), Prometheus `/metrics` endpoint, schema drift detector,
rate limiting per product, CI integration.

## References

- Sibling: [`telemetry-plan.md`](telemetry-plan.md) Phase 2.6 entry.
- Framework chart widgets: [`widgets-plan.md`](widgets-plan.md)
  §"Charts (2D)" — `BarChart` + `LineChart` in the `fern-charts`
  crate. The dashboard consumes these directly.
- Reference integration:
  [`/home/cyril/Devel/fern-test-app/`](../../../fern-test-app/) —
  Qleany backend + FernUI frontend, EventHub-to-EventSource adapter
  pattern.
- Qleany docs (run locally; embedded in the binary):
  - `qleany docs all --md` — full reference dump
  - `qleany docs intro --md` — overview, "AI ready" §
  - `qleany docs design --md` — Clean Architecture rationale,
    User Settings vs Entities boundary
  - `qleany docs flow --md` — how operations flow, EventHub
  - `qleany docs undo --md` — undo/redo architecture
  - `qleany docs api-rust --md` — controller / UoW APIs
  - `qleany docs regen --md` — regeneration safety rules
- Qleany CLI for ongoing work — see "Working with the Qleany CLI" §:
  `qleany check`, `qleany list`, `qleany show`, `qleany diff`,
  `qleany prompt --context | --list | --use-case feature:uc`.
- Tonic: <https://github.com/hyperium/tonic> — TLS and streaming
  examples.
- ClickHouse Rust client: <https://github.com/loyd/clickhouse.rs>.
- DuckDB Rust binding: <https://github.com/duckdb/duckdb-rs>.
