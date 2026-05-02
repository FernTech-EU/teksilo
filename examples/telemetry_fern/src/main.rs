//! `telemetry-fern` example — demonstrates `fern-analytics-fern`
//! against a localhost `fern-collector` instance.
//!
//! This is the home-grown-backend twin of `examples/telemetry_plausible/`.
//! Same shape (three intent buttons + the `PrivacySettings` widget)
//! but with the gRPC adapter from
//! [`crates/fern-analytics-fern/`](../../crates/fern-analytics-fern)
//! pointing at a self-hosted server documented in
//! [`fern-collector`](../../../fern-collector).
//!
//! ## Running
//!
//! Start a local collector first:
//!
//! ```text
//! cd ../fern-collector
//! cargo run --bin fern-collector -- token mint --product telemetry-fern-demo \
//!     --tokens-db /tmp/fern-demo/tokens.sqlite
//! # → fct_xxx_yyy   ← copy this
//! cargo run --bin fern-collector -- serve \
//!     --bind 127.0.0.1:50051 \
//!     --storage-dir /tmp/fern-demo/data \
//!     --tokens-db /tmp/fern-demo/tokens.sqlite
//! ```
//!
//! Then in another terminal:
//!
//! ```text
//! FERN_TOKEN=fct_xxx_yyy cargo run -p telemetry-fern
//! ```
//!
//! Click the buttons; the events flow:
//!
//! * dispatch tap → `DynamicReporter::record` (consent gate +
//!   recent-log tee) → `FernAdapter` worker → tonic bidi-stream →
//!   `fern-collector` → Parquet on disk.
//!
//! Watch the live update in the `PrivacySettings` "Inspect data
//! sent" accordion — Phase 3.2 added a revision signal so the
//! accordion refreshes as events land.
//!
//! ## Pseudonymous mode
//!
//! Set `FERN_INSTALL_ID=<uuid>` (or leave it empty for a fresh one)
//! to flip the adapter into pseudonymous mode. Then "Get my data"
//! and "Erase my data" become wired and round-trip through
//! `Telemetry.Fetch` / `Telemetry.Erase`.
//!
//! Defaults are anonymous-only.
//!
//! ## TLS
//!
//! Set `FERN_TLS_CA=/path/to/cert.pem` to negotiate TLS against a
//! collector started with `--tls-cert / --tls-key`. The example
//! uses the simplest TLS shape (server-only, custom CA, domain
//! override). Production deployments do mTLS — see the docstrings
//! on `fern_analytics_fern::TlsClientConfig`.

use fern_analytics_fern::{FernAdapter, TlsClientConfig};
use fern_telemetry::{TelemetryBundle, TelemetryMode, UsageReporter};
use fern_ui::core::Action;
use fern_ui::prelude::*;
use fern_ui::widgets::{Button, ButtonVariant, Padding, PrivacySettings, TextWidget, VStack};
use std::rc::Rc;

#[derive(Debug)]
struct DemoRoot;

impl Widget for DemoRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        for name in ["app.demo.click", "app.demo.save", "app.demo.about"] {
            ctx.register_action(
                Action::new(name).on_invoke(|intent: &fern_ui::core::Intent, _ctx| {
                    println!("intent dispatched: {}", intent.name);
                }),
            );
        }

        let click_btn = Button::new_literal("Fire 'click' intent")
            .style(ButtonVariant::Default)
            .on_activate_fn(|ctx| {
                ctx.send_intent(fern_ui::core::Intent::new("app.demo.click"));
            });
        let save_btn = Button::new_literal("Fire 'save' intent")
            .style(ButtonVariant::Default)
            .on_activate_fn(|ctx| {
                ctx.send_intent(fern_ui::core::Intent::new("app.demo.save"));
            });
        let about_btn = Button::new_literal("Fire 'about' intent")
            .style(ButtonVariant::Default)
            .on_activate_fn(|ctx| {
                ctx.send_intent(fern_ui::core::Intent::new("app.demo.about"));
            });

        let column = VStack::new()
            .spacing(16.0)
            .child(TextWidget::new_literal("fern-collector telemetry demo"))
            .child(TextWidget::new_literal(
                "Each click fires an intent through the dispatch tap → \
                 fern-analytics-fern adapter → gRPC → your local \
                 fern-collector. The 'Inspect data sent' accordion \
                 below auto-refreshes as events land (Phase 3.2 \
                 revision signal). In pseudonymous mode (set \
                 FERN_INSTALL_ID), 'Get my data' opens a Save-as-JSON \
                 dialog and 'Erase my data' round-trips through \
                 Telemetry.Erase.",
            ))
            .child(click_btn)
            .child(save_btn)
            .child(about_btn)
            .child(
                PrivacySettings::new()
                    .data_processor_name("FernTech (self-hosted)")
                    .privacy_policy_url("https://example.com/privacy"),
            );

        let id = ctx.add(Padding::uniform(20.0).child(column));
        vec![id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let children = self.children();
        children
            .first()
            .and_then(|c| ctx.child_size(*c, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0)).into()
    }
}

const EVENT_SCHEMA_VERSION: u32 = 1;

fn main() {
    let endpoint = std::env::var("FERN_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let product_id = std::env::var("FERN_PRODUCT_ID")
        .unwrap_or_else(|_| "telemetry-fern-demo".into());
    let token = std::env::var("FERN_TOKEN").ok();

    // Optional TLS — set FERN_TLS_CA to enable.
    let tls = std::env::var("FERN_TLS_CA").ok().map(|ca_path| {
        let pem = std::fs::read(&ca_path).expect("read TLS CA pem");
        let domain = std::env::var("FERN_TLS_DOMAIN").ok();
        TlsClientConfig {
            ca_pem: Some(pem),
            client_cert_pem: None,
            client_key_pem: None,
            domain_name: domain,
        }
    });

    // Optional pseudonymous mode — set FERN_INSTALL_ID to enable.
    let install_id = std::env::var("FERN_INSTALL_ID").ok().filter(|s| !s.is_empty());
    let mode = if install_id.is_some() {
        TelemetryMode::Pseudonymous
    } else {
        TelemetryMode::Anonymous
    };

    // Build the adapter.
    let mut builder = FernAdapter::builder()
        .endpoint(&endpoint)
        .product_id(&product_id)
        .max_batch_size(50)
        .flush_interval(std::time::Duration::from_secs(2));
    if let Some(tok) = &token {
        builder = builder.bearer_token(tok);
    }
    if let Some(tls) = tls {
        builder = builder.tls(tls);
    }
    if let Some(uuid) = &install_id {
        builder = builder.install_id(uuid);
    }
    let adapter = Rc::new(builder.build()) as Rc<dyn UsageReporter>;

    let telemetry = match mode {
        TelemetryMode::Anonymous => TelemetryBundle::new(EVENT_SCHEMA_VERSION)
            .with_anonymous(adapter)
            .with_default_mode(TelemetryMode::Anonymous),
        TelemetryMode::Pseudonymous => TelemetryBundle::new(EVENT_SCHEMA_VERSION)
            .with_pseudonymous(adapter)
            .with_default_mode(TelemetryMode::Pseudonymous),
    }
    .with_data_processor_name("FernTech (self-hosted)")
    .with_data_residency_region(fern_telemetry::DataResidencyRegion::EU);

    println!("→ endpoint:   {endpoint}");
    println!("→ product_id: {product_id}");
    println!("→ mode:       {mode:?}");
    if let Some(uuid) = &install_id {
        println!("→ install_id: {uuid}");
    }
    println!("→ token:      {}", if token.is_some() { "set" } else { "(none — server should be in unauth mode)" });

    FernAppBuilder::new()
        .application("eu", "FernTech", "telemetry-fern-demo")
        .settings(SettingsBundle::new())
        .telemetry(telemetry)
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — fern-collector telemetry demo")
                .size(680, 760)
                .root(|tree, _state| tree.add(DemoRoot)),
        )
        .run();
}
