// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Telemetry-Plausible example.
//!
//! Demonstrates the anonymous-mode telemetry pipeline end-to-end:
//!
//! 1. `TeksiloAppBuilder::application(...)` resolves OS-correct paths.
//! 2. `.settings(...)` registers the persistence layer.
//! 3. `.telemetry(TelemetryBundle::new(...).with_anonymous(...))` wires
//!    a `PlausibleAdapter` and registers the `TelemetryContext` so
//!    the dispatch tap can record `intent.dispatched` events.
//! 4. Three buttons each fire a distinct intent. Every click flows
//!    through the dispatch tap → DynamicReporter → consent gate →
//!    Plausible adapter worker → HTTP POST.
//!
//! ## Running against a real Plausible instance
//!
//! Default endpoint is `http://127.0.0.1:8000/api/event` (a
//! self-hosted Plausible at localhost). Override at runtime by
//! editing `~/.config/telemetry-plausible-demo/general.toml`
//! and setting:
//!
//! ```toml
//! [telemetry]
//! endpoint_override = "https://plausible.io/api/event"
//! ```
//!
//! ## Consent UX
//!
//! Nothing reaches the adapter until the user grants consent: the
//! dispatch tap is consent-gated, and this example ships the
//! `PrivacySettings` widget as its consent UI. Click the intent buttons
//! before and after "Accept all" to see the difference.

use std::rc::Rc;
use teksilo::core::Action;
use teksilo::prelude::*;
use teksilo::widgets::{
    Button, ButtonVariant, Expand, HStack, Padding, PrivacySettings, Spacer, TextWidget, Toolbar,
    VStack,
};
use teksilo_analytics_plausible::PlausibleAdapter;
use teksilo_telemetry::{TelemetryBundle, TelemetryMode, UsageReporter};

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(teksilo::widgets::ThemeSwitcher::new()),
    )
}

#[derive(Debug)]
struct DemoRoot;

impl Widget for DemoRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Register actions for each intent name so the dispatch
        // chain has a handler. Without an action the intent still
        // fires through the tap (we'd record it), but it walks all
        // the way to the root looking for a handler.
        for name in ["app.demo.click", "app.demo.save", "app.demo.about"] {
            ctx.register_action(Action::new(name).on_invoke(
                |intent: &teksilo::core::Intent, _ctx| {
                    println!("intent dispatched: {}", intent.name);
                },
            ));
        }

        let click_btn = Button::new(lit!("Fire 'click' intent"))
            .variant(ButtonVariant::Filled)
            .on_activate_fn(|ctx| {
                ctx.send_intent(teksilo::core::Intent::new("app.demo.click"));
            });
        let save_btn = Button::new(lit!("Fire 'save' intent"))
            .variant(ButtonVariant::Filled)
            .on_activate_fn(|ctx| {
                ctx.send_intent(teksilo::core::Intent::new("app.demo.save"));
            });
        let about_btn = Button::new(lit!("Fire 'about' intent"))
            .variant(ButtonVariant::Filled)
            .on_activate_fn(|ctx| {
                ctx.send_intent(teksilo::core::Intent::new("app.demo.about"));
            });

        let column = VStack::new()
            .spacing(16.0)
            .child(TextWidget::new(lit!("Plausible telemetry demo")))
            .child(TextWidget::new(lit!(
                "Each click fires an intent that flows through the dispatch tap, \
                 the anonymous Plausible adapter, and (with consent + a working \
                 endpoint) lands in your Plausible dashboard."
            )))
            .child(click_btn)
            .child(save_btn)
            .child(about_btn)
            // The PrivacySettings widget is the consent UI. Until the
            // user clicks Accept all (or flips a single toggle), the
            // dispatch tap drops every event — try clicking the
            // intent buttons before and after granting to see the
            // difference.
            .child(
                PrivacySettings::new()
                    .data_processor_name("Plausible Insights OÜ")
                    .privacy_policy_url("https://plausible.io/privacy"),
            );

        let id = ctx.add(Padding::uniform(20.0).child(column));
        vec![id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Single child — delegate.
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
    let plausible = Rc::new(
        PlausibleAdapter::builder()
            .endpoint(
                std::env::var("PLAUSIBLE_ENDPOINT")
                    .unwrap_or_else(|_| "http://127.0.0.1:8000/api/event".to_string()),
            )
            .domain(
                std::env::var("PLAUSIBLE_DOMAIN")
                    .unwrap_or_else(|_| "teksilo.localhost".to_string()),
            )
            .build(),
    ) as Rc<dyn UsageReporter>;

    let telemetry = TelemetryBundle::new(EVENT_SCHEMA_VERSION)
        .with_anonymous(plausible)
        .with_default_mode(TelemetryMode::Anonymous)
        .with_data_processor_name("Plausible Insights OÜ")
        .with_data_residency_region(teksilo_telemetry::DataResidencyRegion::EU);

    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .application("eu", "FernTech", "telemetry-plausible-demo")
        .settings(SettingsBundle::new())
        .telemetry(telemetry)
        .theme(teksilo::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Teksilo — Plausible telemetry demo")
                .size(640, 720)
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
