//! Bastyde Widget Catalog — every public widget, classic vs `bati!` side-by-side.
//!
//! Run with: `cargo run -p widget-catalog`
//!
//! Run a specific tab: `cargo run -p widget-catalog -- --tab animations`
//!
//! Auto-cycle through tabs every 100 ms (for screen recordings):
//!   `cargo run -p widget-catalog -- --cycle`
//!

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use bastyde::core::PlatformTitleBarHost;
use bastyde::core::event::{Key, Modifiers};
use bastyde::core::shortcut::{KeyStroke, Shortcut};
use bastyde::core::widget::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Expand, HStack, MenuBar, MenuItem, MenuList, Padding, ScrollArea,
    StatusBar, Switcher, TabId, TabInfo, TabWidget, TextWidget, TitleBar, Toggle, VStack,
    WindowFrame, keystroke_format::format_keystroke,
};
use bastyde_telemetry::{StubReporter, TelemetryBundle, TelemetryMode};

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
        .framework_locales(bastyde::widgets::framework_locales());

    BastydeAppBuilder::new()
        // Paths + settings must be set BEFORE the persistent-archive
        // toast install and before `.telemetry(...)` (both read them).
        .application("com", "FernTech", "widget-catalog")
        .settings(SettingsBundle::new())
        .install_inspector_in_debug()
        .install_file_dialog()
        // Toast host + persistent notification archive — drives the
        // Overlays tab's Toast section and notification bell.
        .install_toast_default()
        // External (OS) drag-and-drop — drives the Drag & Drop tab's
        // DropZone / DropTarget file drops.
        .install_external_dnd()
        .register_tooltips(build_tooltip_registry())
        // Telemetry: a stub (no-network) reporter is enough to make the
        // Settings tab's PrivacySettings render its live consent UI
        // instead of the "not configured" placeholder.
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(Rc::new(StubReporter::anonymous()))
                .with_default_mode(TelemetryMode::Anonymous)
                .with_data_processor_name("FernTech"),
        )
        .theme(bastyde::presets::intui::light())
        .i18n(i18n)
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Widget Catalog")
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
                        let show_bati: Signal<bool> = Signal::new(opts.bati_mode);
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

                        // ── Minimal MenuBar — File / Help ─────────────
                        // Showcases mnemonics (Alt+F / Alt+H), the
                        // window-level dispatcher (F10, bare-Alt-tap),
                        // and in-menu mnemonic activation (just press
                        // the underlined letter once a menu is open).
                        let menu_bar = tree.add(build_menu_bar());

                        // ── Catalog body ─────────────────────────────
                        let catalog = tree.add(WidgetCatalog::new(
                            opts.clone(),
                            tab_ids.clone(),
                            show_bati.clone(),
                            selected_tab.clone(),
                        ));
                        let catalog_filled =
                            tree.add(Expand::vertical().respect_intrinsic().child_id(catalog));

                        let inner = tree.add(
                            VStack::new()
                                .spacing(0.0)
                                .add_child(title_bar_id)
                                .add_child(menu_bar)
                                .add_child(catalog_filled),
                        );

                        // Optional resize frame on platforms that need
                        // it (Wayland). On macOS / Windows / X11 fallback
                        // we skip the frame.
                        match host {
                            Some(h) if h.needs_custom_resize_handles() => {
                                tree.add(WindowFrame::new(h).thickness(6.0).content_id(inner))
                            }
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
        .variant(ButtonVariant::Ghost)
        .on_activate_fn(|ctx| ctx.set_locale("en-US"));
    let fr_btn = Button::new(tr!(locale_fr()))
        .variant(ButtonVariant::Ghost)
        .on_activate_fn(|ctx| ctx.set_locale("fr-FR"));
    let ar_btn = Button::new(tr!(locale_ar()))
        .variant(ButtonVariant::Ghost)
        .on_activate_fn(|ctx| ctx.set_locale("ar-SA"));

    // Theme toggle — flips between light and dark. Tracks state in a
    // `Signal<bool>` captured by the closure so the next click flips
    // back. Matches the title_bar_demo / internationalization patterns.
    let is_dark = Signal::new(false);
    let theme_btn = Button::new(tr!(theme_label()))
        .variant(ButtonVariant::Ghost)
        .tooltip(tr!(theme_tooltip()))
        .on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                bastyde::presets::intui::dark()
            } else {
                bastyde::presets::intui::light()
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

/// Minimal menubar — `&File → &Quit` and `&Help → &Documentation,
/// &About`. Demonstrates the window-level mnemonic dispatcher
/// (Alt+F / Alt+H), bare-Alt-tap + F10 menubar focus, and in-menu
/// mnemonic activation (open File, press 'Q' to quit; open Help,
/// press 'D' or 'A'). All labels carry `&`-markers so the
/// underlines appear while Alt is held.
///
/// On macOS the dispatcher install is skipped automatically — the
/// menubar still renders and mouse-clicks work, but Alt-letter
/// chords are left alone (Option+letter is reserved for accented
/// character input on macOS keyboards).
fn build_menu_bar() -> impl Widget + 'static {
    MenuBar::new()
        .menu(tr!(app_menu_file()), || {
            Box::new(
                MenuList::new().item(
                    MenuItem::new(tr!(app_menu_quit()))
                        .shortcut_label(format_keystroke(KeyStroke::ctrl(Key::Q)))
                        .on_activate_fn(|ctx| ctx.close_window()),
                ),
            )
        })
        .menu(tr!(app_menu_help()), || {
            Box::new(
                MenuList::new()
                    .item(
                        MenuItem::new(tr!(app_menu_documentation())).on_activate_fn(|_| {
                            println!("Documentation: https://github.com/FernTech/bastyde");
                        }),
                    )
                    .separator()
                    .item(MenuItem::new(tr!(app_menu_about())).on_activate_fn(|_| {
                        println!(
                            "Bastyde Widget Catalog — every public widget, classic vs bati! \
                             side-by-side.\nLicense: MPL2.0  •  Copyright (c) 2026 FernTech"
                        );
                    })),
            )
        })
}

// ---------------------------------------------------------------------------
// Catalog body widget — TabWidget + StatusBar.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WidgetCatalog {
    options: cli::CliOptions,
    tab_ids: Rc<Vec<TabId>>,
    show_bati: Signal<bool>,
    selected_tab: Signal<Option<TabId>>,
    root_child_id: Option<WidgetId>,
}

impl WidgetCatalog {
    fn new(
        options: cli::CliOptions,
        tab_ids: Rc<Vec<TabId>>,
        show_bati: Signal<bool>,
        selected_tab: Signal<Option<TabId>>,
    ) -> Self {
        Self {
            options,
            tab_ids,
            show_bati,
            selected_tab,
            root_child_id: None,
        }
    }
}

impl Widget for WidgetCatalog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let sigs = Signals::new(ctx);

        // `view_mode: Signal<usize>` — derived from `show_bati` so that
        // every tab's Switcher binds to the same source.
        let view_mode = self.show_bati.map(|b| if *b { 1 } else { 0 });

        // ── --cycle: auto-advance the tab on a timer ─────────────────
        if let Some(period) = self.options.cycle {
            self.install_cycle(ctx, period);
        }

        // ── Demo shortcuts so the Settings tab's ShortcutSettings has
        //    something to display. Not wired to Actions — these are
        //    catalogued for the rebind UI, not for execution.
        for shortcut in demo_shortcuts() {
            ctx.register_shortcut_global(shortcut);
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
        Toggle::new(self.show_bati.clone()).label(tr!(mode_label()))
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
// TabContent — the per-tab widget that hosts the classic/bati Switcher.
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
        let bati_id = (self.entry.bati)(ctx, &self.sigs);

        let switcher = ctx.add(
            Switcher::new(self.view_mode.clone())
                .child_id(classic_id)
                .child_id(bati_id),
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

/// Sample shortcut catalog so `ShortcutSettings` on the Settings tab
/// has rows to display. Mirrors the shape of the `shortcuts_demo`
/// example — Save / Open / Find / Bold / Italic / Help — without
/// wiring the corresponding Actions, since the catalog isn't trying
/// to *invoke* these commands, only to *exhibit* the rebind UI.
fn demo_shortcuts() -> Vec<Shortcut> {
    vec![
        Shortcut::new("app.save")
            .name("Save")
            .category("File")
            .primary(KeyStroke::ctrl(Key::S))
            .build(),
        Shortcut::new("app.open")
            .name("Open…")
            .category("File")
            .primary(KeyStroke::ctrl(Key::O))
            .build(),
        Shortcut::new("app.quit")
            .name("Quit")
            .category("File")
            .primary(KeyStroke::ctrl(Key::Q))
            .build(),
        Shortcut::new("edit.find")
            .name("Find…")
            .category("Edit")
            .primary(KeyStroke::ctrl(Key::F))
            .build(),
        Shortcut::new("edit.format.bold")
            .name("Bold")
            .category("Edit")
            .primary(KeyStroke::ctrl(Key::B))
            .build(),
        Shortcut::new("edit.format.italic")
            .name("Italic")
            .category("Edit")
            .primary(KeyStroke::ctrl(Key::I))
            .build(),
        Shortcut::new("help.show")
            .name("Help")
            .category("Help")
            .primary(KeyStroke::new(Key::F1, Modifiers::NONE))
            .build(),
    ]
}
