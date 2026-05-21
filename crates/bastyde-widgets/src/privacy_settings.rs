//! `PrivacySettings` — user-facing widget for telemetry consent.
//!
//! Embeddable in any container (typically a [`Dialog`] for first-run
//! consent or a tab in the app's settings UI). Reads from
//! [`OpenedTelemetry`] and writes to [`ConsentStore`]; the UI rebuilds
//! whenever the consent state signal changes.
//!
//! # Sections (top-to-bottom)
//!
//! 1. **Plain-language Art. 13 notice** — controller, processor name,
//!    purposes, lawful basis, retention, recipients, withdrawal right.
//!    All strings flow through `tr_widget!` against keys defined in
//!    [`crates/bastyde-widgets/locales/en-US.ftl`](../../../locales/en-US.ftl)
//!    and [`fr-FR.ftl`](../../../locales/fr-FR.ftl) under the
//!    `privacy-*` namespace. Apps install the framework bundle via
//!    `I18nConfig::framework_locales(bastyde_widgets::framework_locales())`.
//! 2. **Per-scope toggles** — one per
//!    [`ConsentScope`](bastyde_telemetry::ConsentScope) field, intersected
//!    with `reporter.supported_scopes()` so toggles for
//!    unsupported scopes are hidden, not just disabled. Toggles work
//!    from `Unknown` (auto-transition to `Granted` with the toggled
//!    scope) and `Granted` states; they're disabled when state is
//!    `Denied` until the user clicks Withdraw → Accept.
//! 3. **Accept all / Reject all** — equal-prominence buttons (CNIL
//!    parity rule, GDPR Art. 7).
//! 4. **Identity row** (pseudonymous mode only) — install_id display,
//!    Get-my-data button (Art. 15 + 20), Erase-my-data button (Art. 17).
//! 5. **Inspect data sent** — accordion listing the most-recent events
//!    from the bundle's recent-log ring buffer.
//! 6. **Mode switch** (when both adapters configured) — confirm-button
//!    pair to flip anonymous ↔ pseudonymous.
//! 7. **Footer** — Withdraw consent (equal prominence to Accept,
//!    GDPR Art. 7(3)).
//!
//! When no [`OpenedTelemetry`] is registered in `app_state`, the
//! widget renders a "Telemetry not configured" placeholder. Apps that
//! ship without analytics pay nothing.

use bastyde_canvas::{Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::lit;
use bastyde_i18n::tr_widget;
use bastyde_telemetry::{
    ConsentScope, ConsentState, ConsentStore, OpenedTelemetry, RemoteDataExport, TelemetryExt,
    TelemetryMode, UsageReporter,
};
use bastyde_tokens::TextStyleRole;

use crate::accordion::Accordion;
use crate::button::{Button, ButtonVariant};
use crate::message_box::{MessageBox, MessageBoxButtons, StandardButton};
use crate::panel::Panel;
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use crate::toggle::Toggle;

/// Settings widget for telemetry consent. Construct with
/// [`PrivacySettings::new`] and embed in any container.
pub struct PrivacySettings {
    /// Compact layout for first-run modals: hides the mode-switch
    /// section and tightens spacing. Default `false` (full settings
    /// panel layout).
    compact: bool,
    /// Show the install-id + fetch + erase row in pseudonymous mode.
    /// Default `true`. Set to `false` when the host app ships its
    /// own equivalent UI.
    show_identity_row: bool,
    /// Show the anonymous-vs-pseudonymous mode switch when both
    /// adapters are configured. Default `true`. No effect when only
    /// one mode is configured (the section is hidden regardless).
    show_mode_switch: bool,
    /// Show the "Inspect data sent" accordion. When enabled, the
    /// widget peeks the last `inspect_event_count` events from the
    /// `recent_log` ring buffer and lists them. Default `true`.
    /// Note: snapshot-at-build — opening and closing the
    /// accordion refreshes the list to current state.
    show_inspect: bool,
    /// How many recent events to show in the accordion. Default 50.
    inspect_event_count: usize,
    /// Optional URL surfaced as "Read full privacy policy". When
    /// `None` the link is hidden — the controller is responsible for
    /// hosting their own policy text.
    privacy_policy_url: Option<String>,
    /// Plain-text controller name surfaced in the Art. 13 notice
    /// ("Data is processed by <X>"). Defaults to "the application".
    data_processor_name: Option<String>,

    /// Inner — `Some` once `build()` has constructed the layout.
    root_id: Option<WidgetId>,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivacySettings {
    pub fn new() -> Self {
        Self {
            compact: false,
            show_identity_row: true,
            show_mode_switch: true,
            show_inspect: true,
            inspect_event_count: 50,
            privacy_policy_url: None,
            data_processor_name: None,
            root_id: None,
        }
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    pub fn show_identity_row(mut self, show: bool) -> Self {
        self.show_identity_row = show;
        self
    }

    pub fn show_mode_switch(mut self, show: bool) -> Self {
        self.show_mode_switch = show;
        self
    }

    pub fn show_inspect(mut self, show: bool) -> Self {
        self.show_inspect = show;
        self
    }

    pub fn inspect_event_count(mut self, n: usize) -> Self {
        self.inspect_event_count = n.max(1);
        self
    }

    pub fn privacy_policy_url(mut self, url: impl Into<String>) -> Self {
        self.privacy_policy_url = Some(url.into());
        self
    }

    pub fn data_processor_name(mut self, name: impl Into<String>) -> Self {
        self.data_processor_name = Some(name.into());
        self
    }
}

impl std::fmt::Debug for PrivacySettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivacySettings")
            .field("compact", &self.compact)
            .field("show_identity_row", &self.show_identity_row)
            .field("show_mode_switch", &self.show_mode_switch)
            .finish()
    }
}

impl Widget for PrivacySettings {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Resolve the telemetry handle. Without one we render a
        // graceful placeholder so apps that don't ship telemetry can
        // still embed the widget without panicking.
        let Some(telemetry) = ctx.try_telemetry().cloned() else {
            let placeholder = ctx.add(VStack::new().spacing(8.0).child(
                TextWidget::new(tr_widget!(privacy_not_configured())).style(TextStyleRole::Body),
            ));
            self.root_id = Some(placeholder);
            return vec![placeholder];
        };

        // Rebuild on consent-state change so toggles + action visibility
        // track the live state.
        let consent_signal = telemetry.consent.state_signal();
        consent_signal.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Live-update the "Inspect data sent" accordion as
        // events stream in. The reporter bumps `recent_log_revision`
        // on every `record()` and `discard_pending()`. Bound at
        // `BindingLevel::Rebuild` so the accordion's snapshot
        // refreshes without user interaction.
        let revision_signal = telemetry.reporter.recent_log_revision();
        revision_signal.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let state = consent_signal.get();
        let supported = telemetry.reporter.supported_scopes();
        let endpoint = telemetry.reporter.endpoint().to_string();
        let pseudonymous = matches!(
            telemetry.reporter.active_mode(),
            TelemetryMode::Pseudonymous,
        ) && telemetry.reporter.install_id().is_some();
        let supports_mode_switch = telemetry.reporter.supports_mode_switch();
        let processor = self
            .data_processor_name
            .clone()
            .unwrap_or_else(|| "the application".to_string());

        let column = VStack::new()
            .spacing(if self.compact { 12.0 } else { 18.0 })
            .child(self.build_heading())
            .child(self.build_notice(&telemetry, &processor, &endpoint, pseudonymous))
            .child(build_scope_panel(&telemetry, &state, supported))
            .child(build_accept_reject(&telemetry, &endpoint))
            .child_opt(
                (self.show_identity_row && pseudonymous).then(|| build_identity_row(&telemetry)),
            )
            .child_opt(
                (self.show_inspect && !self.compact)
                    .then(|| build_inspect_accordion(&telemetry, self.inspect_event_count)),
            )
            .child_opt(
                (self.show_mode_switch && supports_mode_switch && !self.compact)
                    .then(|| build_mode_switch(&telemetry)),
            )
            .child(build_footer(&telemetry));

        let id = ctx.add(column);
        self.root_id = Some(id);
        vec![id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.root_id
            .and_then(|c| ctx.child_size(c, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: bastyde_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_id.map(|id| vec![id]).unwrap_or_default()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Group);
        builder.set_name("Privacy & Telemetry settings");
    }
}

impl PrivacySettings {
    fn build_heading(&self) -> TextWidget {
        TextWidget::new(tr_widget!(privacy_heading())).style(TextStyleRole::BodyBold)
    }

    fn build_notice(
        &self,
        telemetry: &OpenedTelemetry,
        processor: &str,
        endpoint: &str,
        pseudonymous: bool,
    ) -> Panel {
        let lawful_basis = if pseudonymous {
            tr_widget!(privacy_notice_lawful_pseudonymous())
        } else {
            tr_widget!(privacy_notice_lawful_anonymous())
        };
        let retention_days = telemetry.policy.retention_days as i64;
        let adapter = telemetry.reporter.adapter_name().to_string();
        let processor_owned = processor.to_string();
        let endpoint_owned = endpoint.to_string();
        let processor_line = tr_widget!(privacy_notice_controller(
            processor = processor_owned,
            adapter = adapter,
            endpoint = endpoint_owned,
        ));

        let mut notice = VStack::new()
            .spacing(6.0)
            .child(TextWidget::new(processor_line).style(TextStyleRole::Body))
            .child(
                TextWidget::new(tr_widget!(privacy_notice_purposes())).style(TextStyleRole::Body),
            )
            .child(TextWidget::new(lawful_basis).style(TextStyleRole::Body))
            .child(
                TextWidget::new(tr_widget!(privacy_notice_retention(days = retention_days)))
                    .style(TextStyleRole::Body),
            )
            .child(
                TextWidget::new(tr_widget!(privacy_notice_withdrawal_right()))
                    .style(TextStyleRole::Body),
            );
        if let Some(url) = self.privacy_policy_url.clone() {
            notice = notice.child(
                TextWidget::new(tr_widget!(privacy_notice_policy_link(url = url)))
                    .style(TextStyleRole::Small),
            );
        }
        Panel::new().padding(14.0_f32).child(notice)
    }
}

// ----- helpers (free functions so the build sites stay small) -------

fn build_scope_panel(
    telemetry: &OpenedTelemetry,
    state: &ConsentState,
    supported: ConsentScope,
) -> Panel {
    let mut column = VStack::new().spacing(8.0).child(
        TextWidget::new(tr_widget!(privacy_scope_section_heading())).style(TextStyleRole::BodyBold),
    );

    if supported.anonymous_metrics {
        column = column.child(scope_row(
            tr_widget!(privacy_scope_anonymous_metrics_label()).resolve_now(),
            tr_widget!(privacy_scope_anonymous_metrics_description()).resolve_now(),
            current_value(state, |s| s.anonymous_metrics),
            !matches!(state, ConsentState::Denied),
            telemetry.consent.clone(),
            telemetry.reporter.endpoint().to_string(),
            |scope, v| scope.anonymous_metrics = v,
        ));
    }
    if supported.crash_reports {
        column = column.child(scope_row(
            tr_widget!(privacy_scope_crash_reports_label()).resolve_now(),
            tr_widget!(privacy_scope_crash_reports_description()).resolve_now(),
            current_value(state, |s| s.crash_reports),
            !matches!(state, ConsentState::Denied),
            telemetry.consent.clone(),
            telemetry.reporter.endpoint().to_string(),
            |scope, v| scope.crash_reports = v,
        ));
    }
    if supported.feature_flags {
        column = column.child(scope_row(
            tr_widget!(privacy_scope_feature_flags_label()).resolve_now(),
            tr_widget!(privacy_scope_feature_flags_description()).resolve_now(),
            current_value(state, |s| s.feature_flags),
            !matches!(state, ConsentState::Denied),
            telemetry.consent.clone(),
            telemetry.reporter.endpoint().to_string(),
            |scope, v| scope.feature_flags = v,
        ));
    }
    Panel::new().padding(14.0_f32).child(column)
}

fn current_value(state: &ConsentState, f: impl FnOnce(&ConsentScope) -> bool) -> bool {
    match state {
        ConsentState::Granted(scope) => f(scope),
        _ => false,
    }
}

fn scope_row(
    label: String,
    description: String,
    initial: bool,
    enabled: bool,
    consent: ConsentStore,
    endpoint: String,
    apply: impl Fn(&mut ConsentScope, bool) + 'static,
) -> HStack {
    // One-way binding: when the toggle changes, push to the consent
    // store. The consent store's state signal triggers a widget
    // rebuild, which constructs a fresh local signal seeded with the
    // new state — so there's no write-back loop to worry about.
    let signal = Signal::new(initial);
    let consent_for_observe = consent.clone();
    let endpoint_for_observe = endpoint;
    // Observer is forgotten because the signal is owned by the
    // Toggle, which is dropped on rebuild, dropping the observer
    // chain via the standard Rc-cycle teardown.
    std::mem::forget(signal.observe(move |&new_value| {
        let _ = consent_for_observe
            .set_or_grant_scope(&endpoint_for_observe, |scope| apply(scope, new_value));
    }));

    HStack::new()
        .spacing(12.0)
        .child(
            VStack::new()
                .spacing(2.0)
                .child(TextWidget::new(lit!(label.clone())).style(TextStyleRole::Body))
                .child(TextWidget::new(lit!(description)).style(TextStyleRole::Small)),
        )
        .child(Spacer::new())
        .child(Toggle::new(signal).label(lit!(label)).enabled(enabled))
}

fn build_accept_reject(telemetry: &OpenedTelemetry, endpoint: &str) -> HStack {
    let supported = telemetry.reporter.supported_scopes();
    let endpoint = endpoint.to_string();
    let consent_for_reject = telemetry.consent.clone();
    let consent_for_accept = telemetry.consent.clone();

    let reject = Button::new(tr_widget!(privacy_btn_reject_all()))
        .variant(ButtonVariant::Plain)
        .on_activate_fn(move |_ctx| {
            let _ = consent_for_reject.deny();
        });
    let accept = Button::new(tr_widget!(privacy_btn_accept_all()))
        .variant(ButtonVariant::Filled)
        .on_activate_fn(move |_ctx| {
            let _ = consent_for_accept.grant(supported, &endpoint);
        });

    HStack::new()
        .spacing(8.0)
        .child(reject)
        .child(Spacer::new())
        .child(accept)
}

fn build_identity_row(telemetry: &OpenedTelemetry) -> Panel {
    let install_id = telemetry
        .install_id
        .as_ref()
        .map(|id| id.get())
        .unwrap_or_else(|| "(none)".to_string());
    let retention_days = telemetry.policy.retention_days as i64;

    let consent_for_erase = telemetry.consent.clone();
    let reporter_for_erase = telemetry.reporter.clone();
    let erase = Button::new(tr_widget!(privacy_btn_erase()))
        .variant(ButtonVariant::Plain)
        .tooltip(tr_widget!(privacy_btn_erase_tooltip()))
        .on_activate_fn(move |ctx| {
            let consent = consent_for_erase.clone();
            let reporter = reporter_for_erase.clone();
            MessageBox::question(tr_widget!(privacy_confirm_erase_title()))
                .text(tr_widget!(privacy_confirm_erase_text()))
                .buttons(MessageBoxButtons::OkCancel)
                .on_result(move |result, _ctx| {
                    if matches!(result.button, StandardButton::Ok) {
                        let _ = reporter.erase_remote_data();
                        let _ = reporter.discard_pending();
                        let _ = consent.withdraw();
                    }
                })
                .present(ctx);
        });

    let reporter_for_fetch = telemetry.reporter.clone();
    let install_id_for_fetch = install_id.clone();
    let fetch = Button::new(tr_widget!(privacy_btn_fetch()))
        .variant(ButtonVariant::Filled)
        .tooltip(tr_widget!(privacy_btn_fetch_tooltip()))
        .on_activate_fn(move |ctx| {
            match reporter_for_fetch.fetch_remote_data() {
                Ok(export) => {
                    let event_count = export.events.len() as i64;
                    let json = serde_json_export_label(&export);

                    // Open a "Save as JSON…" dialog via the async
                    // file-dialog service. The result callback runs
                    // back on the main thread once the OS dialog
                    // closes; the event loop keeps ticking in the
                    // meantime so other windows / animations stay
                    // responsive.
                    let suggested_name = format!(
                        "bastyde-export-{}.json",
                        sanitize_filename(&install_id_for_fetch)
                    );
                    use bastyde_platform::file_dialog::{
                        EventContextFileDialogExt, FileDialogRequest, FileDialogResult,
                    };
                    let request = FileDialogRequest::save_file()
                        .title("Save your data export as JSON")
                        .default_file_name(&suggested_name)
                        .add_filter("JSON", &["json"]);

                    // The closure captures `export`, `json`, and
                    // `event_count` by move so they remain available
                    // when the dialog resolves on a later event-loop
                    // tick.
                    let submit = ctx.save_file(request, move |result, ctx| match result {
                        FileDialogResult::Saved(Some(path)) => {
                            match std::fs::write(&path, json.as_bytes()) {
                                Ok(()) => {
                                    let mut details = String::new();
                                    for (n, ev) in export.events.iter().enumerate().take(20) {
                                        if !details.is_empty() {
                                            details.push('\n');
                                        }
                                        details.push_str(&format!("{}. {}", n + 1, ev.name));
                                    }
                                    if export.events.len() > 20 {
                                        details.push_str(&format!(
                                            "\n… and {} more.",
                                            export.events.len() - 20
                                        ));
                                    }
                                    MessageBox::information(tr_widget!(
                                        privacy_fetch_success_title()
                                    ))
                                    .text(tr_widget!(privacy_fetch_success_text(
                                        count = event_count
                                    )))
                                    .informative_text(lit!(format!(
                                        "{}\n\n{details}",
                                        tr_widget!(privacy_fetch_saved_to(
                                            path = path.display().to_string()
                                        ))
                                        .resolve_now()
                                    )))
                                    .buttons(MessageBoxButtons::Ok)
                                    .present(ctx);
                                }
                                Err(e) => {
                                    MessageBox::warning(tr_widget!(privacy_fetch_error_title()))
                                        .text(tr_widget!(privacy_fetch_write_error(
                                            path = path.display().to_string(),
                                            error = e.to_string(),
                                        )))
                                        .buttons(MessageBoxButtons::Ok)
                                        .present(ctx);
                                }
                            }
                        }
                        FileDialogResult::Saved(None) => {
                            // User cancelled the save dialog — fall
                            // back to the inline display so the
                            // export isn't lost (Art. 20 portability
                            // requires a working path either way).
                            let mut details = String::new();
                            for (n, ev) in export.events.iter().enumerate().take(20) {
                                if !details.is_empty() {
                                    details.push('\n');
                                }
                                details.push_str(&format!("{}. {}", n + 1, ev.name));
                            }
                            if export.events.len() > 20 {
                                details.push_str(&format!(
                                    "\n… and {} more.",
                                    export.events.len() - 20
                                ));
                            }
                            MessageBox::information(tr_widget!(privacy_fetch_success_title()))
                                .text(tr_widget!(privacy_fetch_success_text(count = event_count)))
                                .informative_text(lit!(details))
                                .detailed_text(lit!(json))
                                .buttons(MessageBoxButtons::Ok)
                                .present(ctx);
                        }
                        FileDialogResult::Error(msg) => {
                            MessageBox::warning(tr_widget!(privacy_fetch_error_title()))
                                .text(lit!(msg))
                                .buttons(MessageBoxButtons::Ok)
                                .present(ctx);
                        }
                        // The save_file kind only returns Saved(_) /
                        // Error(_) — but match exhaustively for
                        // forward-compat with future result variants.
                        _ => {}
                    });
                    if let Err(msg) = submit {
                        MessageBox::warning(tr_widget!(privacy_fetch_error_title()))
                            .text(lit!(msg))
                            .buttons(MessageBoxButtons::Ok)
                            .present(ctx);
                    }
                }
                Err(e) => {
                    MessageBox::warning(tr_widget!(privacy_fetch_error_title()))
                        .text(lit!(format!("{e}")))
                        .buttons(MessageBoxButtons::Ok)
                        .present(ctx);
                }
            }
        });

    Panel::new().padding(14.0_f32).child(
        VStack::new()
            .spacing(6.0)
            .child(
                TextWidget::new(tr_widget!(privacy_identity_heading()))
                    .style(TextStyleRole::BodyBold),
            )
            .child(
                TextWidget::new(tr_widget!(privacy_identity_install_id(
                    id = install_id.clone()
                )))
                .style(TextStyleRole::Small),
            )
            .child(
                TextWidget::new(tr_widget!(privacy_identity_retention(
                    days = retention_days
                )))
                .style(TextStyleRole::Small),
            )
            .child(HStack::new().spacing(8.0).child(fetch).child(erase)),
    )
}

fn build_mode_switch(telemetry: &OpenedTelemetry) -> Panel {
    let active = telemetry.reporter.active_mode();
    let (current_label, target_mode, target_label, target_blurb) = match active {
        TelemetryMode::Anonymous => (
            tr_widget!(privacy_mode_current_anonymous()),
            TelemetryMode::Pseudonymous,
            tr_widget!(privacy_btn_switch_to_pseudonymous()),
            tr_widget!(privacy_mode_blurb_pseudonymous()),
        ),
        TelemetryMode::Pseudonymous => (
            tr_widget!(privacy_mode_current_pseudonymous()),
            TelemetryMode::Anonymous,
            tr_widget!(privacy_btn_switch_to_anonymous()),
            tr_widget!(privacy_mode_blurb_anonymous()),
        ),
    };

    let reporter = telemetry.reporter.clone();
    let consent = telemetry.consent.clone();
    let install_id = telemetry.install_id.clone();

    let switch_btn = Button::new(target_label)
        .variant(ButtonVariant::Plain)
        .on_activate_fn(move |ctx| {
            let reporter = reporter.clone();
            let consent = consent.clone();
            let install_id = install_id.clone();
            let leaving_pseudonymous =
                matches!(reporter.active_mode(), TelemetryMode::Pseudonymous);
            let body = if leaving_pseudonymous {
                tr_widget!(privacy_confirm_mode_switch_leaving_pseudonymous())
            } else {
                tr_widget!(privacy_confirm_mode_switch_leaving_anonymous())
            };
            MessageBox::question(tr_widget!(privacy_confirm_mode_switch_title()))
                .text(body)
                .buttons(MessageBoxButtons::OkCancel)
                .on_result(move |result, _ctx| {
                    if !matches!(result.button, StandardButton::Ok) {
                        return;
                    }
                    if leaving_pseudonymous {
                        let _ = reporter.erase_remote_data();
                        if let Some(id) = &install_id {
                            let _ = id.clear();
                        }
                    }
                    let _ = reporter.discard_pending();
                    let _ = consent.reset();
                    reporter.set_active_mode(target_mode);
                })
                .present(ctx);
        });

    Panel::new().padding(14.0_f32).child(
        VStack::new()
            .spacing(6.0)
            .child(
                TextWidget::new(tr_widget!(privacy_mode_heading())).style(TextStyleRole::BodyBold),
            )
            .child(TextWidget::new(current_label).style(TextStyleRole::Body))
            .child(TextWidget::new(target_blurb).style(TextStyleRole::Small))
            .child(HStack::new().child(switch_btn).child(Spacer::new())),
    )
}

fn build_footer(telemetry: &OpenedTelemetry) -> HStack {
    let consent = telemetry.consent.clone();
    let withdraw = Button::new(tr_widget!(privacy_btn_withdraw()))
        .variant(ButtonVariant::Plain)
        .tooltip(tr_widget!(privacy_btn_withdraw_tooltip()))
        .on_activate_fn(move |ctx| {
            let consent = consent.clone();
            MessageBox::question(tr_widget!(privacy_confirm_withdraw_title()))
                .text(tr_widget!(privacy_confirm_withdraw_text()))
                .buttons(MessageBoxButtons::OkCancel)
                .on_result(move |result, _ctx| {
                    if matches!(result.button, StandardButton::Ok) {
                        let _ = consent.withdraw();
                    }
                })
                .present(ctx);
        });
    HStack::new().child(Spacer::new()).child(withdraw)
}

/// Pretty-print a fetched export as JSON for the "Get my data"
/// detailed-text panel. Falls back to a placeholder if serialization
/// fails (which shouldn't happen — `RemoteDataExport` is plain data).
fn serde_json_export_label(export: &RemoteDataExport) -> String {
    serde_json::to_string_pretty(export)
        .unwrap_or_else(|e| format!("(failed to serialize export: {e})"))
}

/// Make a string safe for use as a filename across the three
/// desktop platforms: ASCII alnum + `-` + `_` survive verbatim;
/// everything else collapses to `_`. Used to embed the install_id
/// in the suggested save-dialog filename.
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// "Inspect data sent" accordion — shows up to `n` most-recent events
/// from the recent-log ring buffer (newest first). Snapshot-at-build:
/// expanding/collapsing the accordion refreshes the list to current
/// state via the framework's rebuild-on-binding mechanism.
fn build_inspect_accordion(telemetry: &OpenedTelemetry, n: usize) -> Accordion {
    use crate::primitives::Padding;
    use bastyde_telemetry::{EventQueue, OwnedPropValue};

    let recent = telemetry.recent_log.peek_recent(n);
    let count = recent.len();
    let count_i64 = count as i64;

    let body = if count == 0 {
        VStack::new()
            .spacing(6.0)
            .child(TextWidget::new(tr_widget!(privacy_inspect_empty())).style(TextStyleRole::Small))
    } else {
        let mut col = VStack::new().spacing(4.0).child(
            TextWidget::new(tr_widget!(privacy_inspect_summary(count = count_i64)))
                .style(TextStyleRole::Small),
        );
        for (idx, event) in recent.iter().enumerate() {
            let mut props_summary = String::new();
            for prop in event.props.iter().take(4) {
                let v = match &prop.value {
                    OwnedPropValue::Str(s) => s.clone(),
                    OwnedPropValue::U32(n) => n.to_string(),
                    OwnedPropValue::I64(n) => n.to_string(),
                    OwnedPropValue::Bool(b) => b.to_string(),
                    OwnedPropValue::F64Bucket(b) => format!("{}-{}", b.min_x100, b.max_x100),
                    OwnedPropValue::HistogramStrU32(_) => "{histogram}".into(),
                };
                if !props_summary.is_empty() {
                    props_summary.push_str(", ");
                }
                props_summary.push_str(&format!("{}={v}", prop.key));
            }
            if event.props.len() > 4 {
                props_summary.push_str(&format!(", … +{} more", event.props.len() - 4));
            }
            let line = if props_summary.is_empty() {
                format!("{}. {}", idx + 1, event.name)
            } else {
                format!("{}. {} ({})", idx + 1, event.name, props_summary)
            };
            col = col.child(TextWidget::new(lit!(line)).style(TextStyleRole::Mono));
        }
        col
    };

    Accordion::new(
        tr_widget!(privacy_inspect_title(count = count_i64)),
        Signal::new(false),
    )
    .content(Padding::uniform(10.0).child(body))
}
