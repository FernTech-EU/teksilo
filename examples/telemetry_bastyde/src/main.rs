//! `telemetry-bastyde` example — demonstrates `bastyde-analytics-bastyde`
//! against a localhost `bastyde-collector` instance.
//!
//! This is the home-grown-backend twin of `examples/telemetry_plausible/`.
//! Same shape (three intent buttons + the `PrivacySettings` widget)
//! but with the gRPC adapter from
//! [`crates/bastyde-analytics-bastyde/`](../../crates/bastyde-analytics-bastyde)
//! pointing at a self-hosted server documented in
//! [`bastyde-collector`](../../../bastyde-collector).
//!
//! ## Running
//!
//! Start a local collector first:
//!
//! ```text
//! cd ../bastyde-collector
//! cargo run --bin bastyde-collector -- token mint --product telemetry-bastyde-demo \
//!     --tokens-db /tmp/bastyde-demo/tokens.sqlite
//! # → fct_xxx_yyy   ← copy this
//! cargo run --bin bastyde-collector -- serve \
//!     --bind 127.0.0.1:50051 \
//!     --storage-dir /tmp/bastyde-demo/data \
//!     --tokens-db /tmp/bastyde-demo/tokens.sqlite
//! ```
//!
//! Then in another terminal:
//!
//! ```text
//! BASTYDE_TOKEN=fct_xxx_yyy cargo run -p telemetry-bastyde
//! ```
//!
//! Click the buttons; the events flow:
//!
//! * dispatch tap → `DynamicReporter::record` (consent gate +
//!   recent-log tee) → `BastydeAdapter` worker → tonic bidi-stream →
//!   `bastyde-collector` → Parquet on disk.
//!
//! Watch the live update in the `PrivacySettings` "Inspect data
//! sent" accordion — a revision signal was added so the
//! accordion refreshes as events land.
//!
//! ## Pseudonymous mode
//!
//! Set `BASTYDE_INSTALL_ID=<uuid>` (or leave it empty for a fresh one)
//! to flip the adapter into pseudonymous mode. Then "Get my data"
//! and "Erase my data" become wired and round-trip through
//! `Telemetry.Fetch` / `Telemetry.Erase`.
//!
//! Defaults are anonymous-only.
//!
//! ## TLS
//!
//! Set `BASTYDE_TLS_CA=/path/to/cert.pem` to negotiate TLS against a
//! collector started with `--tls-cert / --tls-key`. The example
//! uses the simplest TLS shape (server-only, custom CA, domain
//! override). Production deployments do mTLS — see the docstrings
//! on `bastyde_analytics_bastyde::TlsClientConfig`.

use bastyde_analytics_bastyde::{BastydeAdapter, TlsClientConfig};
use bastyde_telemetry::{TelemetryBundle, TelemetryMode, UsageReporter};
use bastyde::core::Action;
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Expand, HStack, Padding, PrivacySettings, Spacer, TextWidget, Toolbar,
    VStack,
};
use std::rc::Rc;

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new(lit!("Toggle Dark Mode")).on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                bastyde::presets::intui::dark()
            } else {
                bastyde::presets::intui::light()
            });
        }),
    ))
}

#[derive(Debug)]
struct DemoRoot;

impl Widget for DemoRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        for name in ["app.demo.click", "app.demo.save", "app.demo.about"] {
            ctx.register_action(Action::new(name).on_invoke(
                |intent: &bastyde::core::Intent, _ctx| {
                    println!("intent dispatched: {}", intent.name);
                },
            ));
        }

        let click_btn = Button::new(lit!("Fire 'click' intent"))
            .variant(ButtonVariant::Filled)
            .on_activate_fn(|ctx| {
                ctx.send_intent(bastyde::core::Intent::new("app.demo.click"));
            });
        let save_btn = Button::new(lit!("Fire 'save' intent"))
            .variant(ButtonVariant::Filled)
            .on_activate_fn(|ctx| {
                ctx.send_intent(bastyde::core::Intent::new("app.demo.save"));
            });
        let about_btn = Button::new(lit!("Fire 'about' intent"))
            .variant(ButtonVariant::Filled)
            .on_activate_fn(|ctx| {
                ctx.send_intent(bastyde::core::Intent::new("app.demo.about"));
            });

        let column = VStack::new()
            .spacing(16.0)
            .child(TextWidget::new(lit!("bastyde-collector telemetry demo")))
            .child(TextWidget::new(lit!("Each click fires an intent through the dispatch tap → \
                 bastyde-analytics-bastyde adapter → gRPC → your local \
                 bastyde-collector. The 'Inspect data sent' accordion \
                 below auto-refreshes as events land. In pseudonymous \
                 mode (set BASTYDE_INSTALL_ID), 'Get my data' opens a \
                 Save-as-JSON dialog and 'Erase my data' round-trips \
                 through Telemetry.Erase."),
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
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}

const EVENT_SCHEMA_VERSION: u32 = 1;

fn main() {
    let endpoint =
        std::env::var("BASTYDE_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let product_id =
        std::env::var("BASTYDE_PRODUCT_ID").unwrap_or_else(|_| "telemetry-bastyde-demo".into());
    let token = std::env::var("BASTYDE_TOKEN").ok();

    // Optional TLS — set BASTYDE_TLS_CA to enable.
    let tls = std::env::var("BASTYDE_TLS_CA").ok().map(|ca_path| {
        let pem = std::fs::read(&ca_path).expect("read TLS CA pem");
        let domain = std::env::var("BASTYDE_TLS_DOMAIN").ok();
        TlsClientConfig {
            ca_pem: Some(pem),
            client_cert_pem: None,
            client_key_pem: None,
            domain_name: domain,
        }
    });

    // Optional pseudonymous mode — set BASTYDE_INSTALL_ID to enable.
    let install_id = std::env::var("BASTYDE_INSTALL_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let mode = if install_id.is_some() {
        TelemetryMode::Pseudonymous
    } else {
        TelemetryMode::Anonymous
    };

    // Build the adapter.
    let mut builder = BastydeAdapter::builder()
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
    .with_data_residency_region(bastyde_telemetry::DataResidencyRegion::EU);

    println!("→ endpoint:   {endpoint}");
    println!("→ product_id: {product_id}");
    println!("→ mode:       {mode:?}");
    if let Some(uuid) = &install_id {
        println!("→ install_id: {uuid}");
    }
    println!(
        "→ token:      {}",
        if token.is_some() {
            "set"
        } else {
            "(none — server should be in unauth mode)"
        }
    );

    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .application("eu", "FernTech", "telemetry-bastyde-demo")
        .settings(SettingsBundle::new())
        .telemetry(telemetry)
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — bastyde-collector telemetry demo")
                .size(680, 760)
                .root(|tree, _state| {
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(DemoRoot)),
                    )
                }),
        )
        .run();
}
