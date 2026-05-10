//! FernUI Widget Catalog — every public widget, classic vs `fern!` side-by-side.
//!
//! Run with: `cargo run -p widget-catalog`
//!
//! Run a specific tab: `cargo run -p widget-catalog -- --tab animations`
//!
//! Auto-cycle through tabs every 100 ms (for screen recordings):
//!   `cargo run -p widget-catalog -- --cycle`
//!
//! See the project plan at
//! `~/.claude/plans/widget-catalog-example-must-contain-valiant-alpaca.md`
//! for the full design.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use fern_ui::core::PlatformTitleBarHost;
use fern_ui::core::widget::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonVariant, Expand, HStack, Padding, ScrollArea, StatusBar, Switcher, TabId,
    TabInfo, TabWidget, TextWidget, TitleBar, Toggle, VStack, WindowFrame,
};

mod cli;
mod shared;
mod tabs;

use crate::shared::{Signals, build_tooltip_registry};
use crate::tabs::{TABS, TabEntry, tab_names};

fn main() {
    let names: Vec<&'static str> = tab_names();
    let options = cli::parse(&names);

    // Allocate stable TabIds once. Cloned into the root closure and then
    // again into WidgetCatalog so the catalog can match selected → index
    // for `--cycle`.
    let tab_ids: Rc<Vec<TabId>> = Rc::new(TABS.iter().map(|_| TabId::fresh()).collect());

    let i18n = I18nConfig::new()
        .source_locale("en-US".parse().expect("en-US is a valid BCP-47 tag"))
        .supported_locales([
            "en-US".parse().expect("en-US is a valid BCP-47 tag"),
            "fr-FR".parse().expect("fr-FR is a valid BCP-47 tag"),
            "ar-SA".parse().expect("ar-SA is a valid BCP-47 tag"),
        ])
        .compile_in(&[
            (
                "en-US",
                &[include_str!("../locales/en-US/widget_catalog.ftl")],
            ),
            (
                "fr-FR",
                &[include_str!("../locales/fr-FR/widget_catalog.ftl")],
            ),
            (
                "ar-SA",
                &[include_str!("../locales/ar-SA/widget_catalog.ftl")],
            ),
        ])
        .auto_detect_os_locale(false)
        .fallback_locale("en-US".parse().expect("en-US is a valid BCP-47 tag"))
        .framework_locales(fern_ui::widgets::framework_locales());

    FernAppBuilder::new()
        .install_inspector_in_debug()
        .install_file_dialog()
        .register_tooltips(build_tooltip_registry())
        .theme(fern_ui::presets::intui::light())
        .i18n(i18n)
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Widget Catalog")
                .size(1400, 900)
                .decorations(DecorationsMode::CustomChrome)
                .root({
                    let opts = options.clone();
                    let tab_ids = tab_ids.clone();
                    move |tree, _state| {
                        let theme = tree.theme().clone();
                        let host = tree.title_bar_host();

                        // Top-level shared state: the toggle that flips
                        // every tab's Switcher and the currently-active
                        // TabId. Lives at the window root so the title
                        // bar can read/write the same signals.
                        let show_fern: Signal<bool> = Signal::new(opts.fern_mode);
                        let selected_tab: Signal<Option<TabId>> =
                            Signal::new(Some(tab_ids[opts.initial_tab]));

                        // ── Title bar ─────────────────────────────────
                        let title_bar: Box<dyn Widget> = match host.clone() {
                            Some(h) => Box::new(build_title_bar(h, &theme)),
                            None => Box::new(
                                TextWidget::new(tr!(app_unsupported_chrome()))
                                    .style(TextStyleRole::Small)
                                    .color(TextRole::Error),
                            ),
                        };
                        let title_bar_id = tree.add_boxed(title_bar);

                        // ── Catalog body ─────────────────────────────
                        let catalog = tree.add(WidgetCatalog::new(
                            opts.clone(),
                            tab_ids.clone(),
                            show_fern.clone(),
                            selected_tab.clone(),
                        ));
                        let catalog_filled = tree.add(
                            Expand::vertical()
                                .respect_intrinsic()
                                .child_id(catalog),
                        );

                        let inner = tree.add(
                            VStack::new()
                                .spacing(0.0)
                                .add_child(title_bar_id)
                                .add_child(catalog_filled),
                        );

                        // Optional resize frame on platforms that need
                        // it (Wayland). On macOS / Windows / X11 fallback
                        // we skip the frame.
                        match host {
                            Some(h) if h.needs_custom_resize_handles() => tree.add(
                                WindowFrame::new(h)
                                    .thickness(6.0)
                                    .content_id(inner),
                            ),
                            _ => inner,
                        }
                    }
                }),
        )
        .run();
}

/// Build the custom title bar. Uses role-driven background/border so
/// the chrome retints live across `ctx.set_theme(...)` switches.
fn build_title_bar(host: Rc<dyn PlatformTitleBarHost>, _theme: &Theme) -> impl Widget + 'static {
    let brand = TextWidget::new(tr!(app_title()))
        .style(TextStyleRole::BodyBold)
        .color(TextRole::Primary);

    let center = TextWidget::new(tr!(app_subtitle()))
        .style(TextStyleRole::Small)
        .color(TextRole::Secondary);

    // Locale switch — two flat buttons. `EventContext::set_locale`
    // requires an event handler, so the SegmentedControl pattern
    // doesn't fit (its `Signal<usize>` mutates from inside the widget,
    // not via a callback that hands you `&mut EventContext`).
    let en_btn = Button::new(tr!(locale_en()))
        .style(ButtonVariant::Flat)
        .on_activate_fn(|ctx| ctx.set_locale("en-US"));
    let fr_btn = Button::new(tr!(locale_fr()))
        .style(ButtonVariant::Flat)
        .on_activate_fn(|ctx| ctx.set_locale("fr-FR"));
    let ar_btn = Button::new(tr!(locale_ar()))
        .style(ButtonVariant::Flat)
        .on_activate_fn(|ctx| ctx.set_locale("ar-SA"));

    // Theme toggle — flips between light and dark. Tracks state in a
    // `Signal<bool>` captured by the closure so the next click flips
    // back. Matches the title_bar_demo / internationalization patterns.
    let is_dark = Signal::new(false);
    let theme_btn = Button::new(tr!(theme_label()))
        .style(ButtonVariant::Flat)
        .tooltip(tr!(theme_tooltip()))
        .on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                fern_ui::presets::intui::dark()
            } else {
                fern_ui::presets::intui::light()
            });
        });

    let trailing = HStack::new()
        .spacing(4.0)
        .child(en_btn)
        .child(fr_btn)
        .child(ar_btn)
        .child(theme_btn);

    TitleBar::new(host)
        .height(40.0)
        .background(SurfaceRole::Raised)
        .border(BorderRole::Default, 1.0)
        .leading(brand)
        .center(center)
        .trailing(trailing)
        .close_action(|ctx| ctx.close_window())
}

// ---------------------------------------------------------------------------
// Catalog body widget — TabWidget + StatusBar.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WidgetCatalog {
    options: cli::CliOptions,
    tab_ids: Rc<Vec<TabId>>,
    show_fern: Signal<bool>,
    selected_tab: Signal<Option<TabId>>,
    root_child_id: Option<WidgetId>,
}

impl WidgetCatalog {
    fn new(
        options: cli::CliOptions,
        tab_ids: Rc<Vec<TabId>>,
        show_fern: Signal<bool>,
        selected_tab: Signal<Option<TabId>>,
    ) -> Self {
        Self {
            options,
            tab_ids,
            show_fern,
            selected_tab,
            root_child_id: None,
        }
    }
}

impl Widget for WidgetCatalog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let sigs = Signals::new(ctx);

        // `view_mode: Signal<usize>` — derived from `show_fern` so that
        // every tab's Switcher binds to the same source.
        let view_mode = self.show_fern.map(|b| if *b { 1 } else { 0 });

        // ── --cycle: auto-advance the tab on a timer ─────────────────
        if let Some(period) = self.options.cycle {
            self.install_cycle(ctx, period);
        }

        // ── TabWidget ────────────────────────────────────────────────
        let mut tw = TabWidget::new(self.selected_tab.clone())
            .vertical()
            .max_tab_width(180.0)
            .tab_surface_role(SurfaceRole::Sunken);
        for (i, entry) in TABS.iter().enumerate() {
            let id = self.tab_ids[i];
            let info = TabInfo::new()
                .title((entry.title_fn)())
                .tooltip((entry.refs_fn)());
            let content = TabContent::new(entry, sigs.clone(), view_mode.clone());
            tw = tw.static_tab_with_id(id, info, content);
        }
        tw = tw.bar_trailing_slot(self.build_mode_toggle());
        let tabs_id = ctx.add(tw);
        let tabs_filling = ctx.add(Expand::vertical().respect_intrinsic().child_id(tabs_id));

        // ── StatusBar ────────────────────────────────────────────────
        let status = ctx.add(
            StatusBar::new().child(
                TextWidget::new(tr!(mode_tooltip()))
                    .style(TextStyleRole::Tiny)
                    .color(TextRole::Secondary),
            ),
        );

        let root = ctx.add(
            VStack::new()
                .spacing(0.0)
                .add_child(tabs_filling)
                .add_child(status),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

impl WidgetCatalog {
    fn build_mode_toggle(&self) -> impl Widget + 'static {
        Toggle::new(self.show_fern.clone()).label(tr!(mode_label()))
    }

    fn install_cycle(&self, ctx: &mut BuildContext, period: Duration) {
        // Mirrors the per-frame timer pattern that `Cycle` / `Pulse`
        // use: accumulate elapsed delta from `frame_tick`, advance the
        // selected tab when the period elapses, and `frame_request.set(true)`
        // each tick to keep the event loop pumping. `wake_at_handle` (the
        // sleep-mode deadline) is the wrong primitive here because nothing
        // else dirty-marks the tree — without `request_frame()` the loop
        // never wakes.
        let tab_ids = self.tab_ids.clone();
        let selected_tab = self.selected_tab.clone();
        let period_secs = period.as_secs_f32().max(0.001);
        let elapsed = Rc::new(Cell::new(0.0_f32));
        let frame_request = ctx.frame_request_handle();
        ctx.effect(&ctx.frame_tick(), move |&delta| {
            let t = elapsed.get() + delta;
            if t >= period_secs {
                let cur = selected_tab.get();
                let cur_idx = cur
                    .and_then(|id| tab_ids.iter().position(|t| *t == id))
                    .unwrap_or(0);
                let new_idx = (cur_idx + 1) % tab_ids.len();
                selected_tab.set(Some(tab_ids[new_idx]));
                elapsed.set(t - period_secs);
            } else {
                elapsed.set(t);
            }
            frame_request.set(true);
        });
        ctx.request_frame();
    }
}

// ---------------------------------------------------------------------------
// TabContent — the per-tab widget that hosts the classic/fern Switcher.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct TabContent {
    entry: &'static TabEntry,
    sigs: Signals,
    view_mode: Signal<usize>,
    root_child_id: Option<WidgetId>,
}

impl TabContent {
    fn new(entry: &'static TabEntry, sigs: Signals, view_mode: Signal<usize>) -> Self {
        Self {
            entry,
            sigs,
            view_mode,
            root_child_id: None,
        }
    }
}

impl Widget for TabContent {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let classic_id = (self.entry.classic)(ctx, &self.sigs);
        let fern_id = (self.entry.fern)(ctx, &self.sigs);

        let switcher = ctx.add(
            Switcher::new(self.view_mode.clone())
                .child_id(classic_id)
                .child_id(fern_id),
        );
        let padded = ctx.add(Padding::uniform(20.0).child_id(switcher));
        let scrolled = ctx.add(ScrollArea::from_id(padded));
        self.root_child_id = Some(scrolled);
        vec![scrolled]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
