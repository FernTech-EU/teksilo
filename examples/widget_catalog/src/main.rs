// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Teksilo Widget Catalog — every public widget, classic vs `teksu!` side-by-side.
//!
//! Run with: `cargo run -p widget-catalog`
//!
//! Run a specific tab: `cargo run -p widget-catalog -- --tab animations`
//!
//! Auto-cycle through tabs every 100 ms (for screen recordings):
//!   `cargo run -p widget-catalog -- --cycle`
//!
//! Auto-cycle on an explicit interval (e.g. for timed screenshots):
//!   `cargo run -p widget-catalog -- --cycle-ms 6000`
//!

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo::core::PlatformTitleBarHost;
use teksilo::core::event::{Key, Modifiers};
use teksilo::core::shortcut::{KeyStroke, Shortcut};
use teksilo::core::widget::WidgetPlacement;
use teksilo::prelude::*;
use teksilo::widgets::{
    Button, ButtonVariant, Expand, HStack, MaxSize, MenuBar, MenuItem, MenuList, Padding,
    ScrollArea, Spacer, StatusBar, Switcher, TabId, TabInfo, TabWidget, TextScaleControl,
    TextWidget, TitleBar, Toggle, VStack, WindowFrame, keystroke_format::format_keystroke,
};
use teksilo_telemetry::{StubReporter, TelemetryBundle, TelemetryMode};

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
        .framework_locales(teksilo::widgets::framework_locales());

    TeksiloAppBuilder::new()
        // Paths + settings must be set BEFORE the persistent-archive
        // toast install and before `.telemetry(...)` (both read them).
        .application("eu", "FernTech", "widget-catalog")
        .settings(SettingsBundle::new())
        .install_inspector_in_debug()
        // Debug-only automation bridge: drive this catalog with
        // `teksilo-automation-mcp --attach` (or `--attach-pid <pid>`). The app
        // publishes an endpoint descriptor carrying its token under its runtime
        // dir, so there is nothing to copy out of stderr. No-op in release.
        .install_automation_bridge_in_debug()
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
        .theme(
            options
                .theme
                .as_deref()
                .and_then(theme_from_name)
                .unwrap_or_else(teksilo::presets::intui::light),
        )
        .i18n(i18n)
        .install_native_menu()
        .initial_window(
            WindowConfig::new()
                .title("Teksilo — Widget Catalog")
                .size(1400, 900)
                .min_size(600, 600)
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
                        let show_teksi: Signal<bool> = Signal::new(opts.teksi_mode);
                        let selected_tab: Signal<Option<TabId>> =
                            Signal::new(Some(tab_ids[opts.initial_tab]));

                        // ── Minimal MenuBar — File / Help ─────────────
                        // Showcases mnemonics (Alt+F / Alt+H), the
                        // window-level dispatcher (F10, bare-Alt-tap),
                        // and in-menu mnemonic activation (just press
                        // the underlined letter once a menu is open).
                        let menu_bar = tree.add(build_menu_bar());

                        // ── Title bar ─────────────────────────────────
                        let title_bar: Box<dyn Widget> = match host.clone() {
                            Some(h) => Box::new(build_title_bar(h, &theme, menu_bar)),
                            None => Box::new(
                                VStack::new().spacing(4.0).add_child(menu_bar).child(
                                    TextWidget::new(tr!(app_unsupported_chrome()))
                                        .style(TextStyleRole::Small)
                                        .color(TextRole::Error),
                                ),
                            ),
                        };
                        let title_bar_id = tree.add_boxed(title_bar);

                        // ── Catalog body ─────────────────────────────
                        let catalog = tree.add(WidgetCatalog::new(
                            opts.clone(),
                            tab_ids.clone(),
                            show_teksi.clone(),
                            selected_tab.clone(),
                        ));
                        let catalog_filled =
                            tree.add(Expand::vertical().respect_intrinsic().child_id(catalog));

                        // Invisible: persists + restores the chosen theme.
                        // `--theme` forces the startup theme and skips restore.
                        let theme_persist = tree.add(ThemePersistenceSlot {
                            skip_restore: opts.theme.is_some(),
                        });
                        let inner = tree.add(
                            VStack::new()
                                .spacing(0.0)
                                .add_child(theme_persist)
                                .add_child(title_bar_id)
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

/// Composite-tooltip body for the title-bar `ThemeSwitcher`. Spells out the
/// one non-obvious thing about runtime theme switching: colours retint live,
/// but widget *chrome* (shapes) is resolved at build time — so a live switch
/// between the IntUI, Material 3 and Fluent families keeps the current shapes.
/// Points the user at the `--theme` startup flag to see a preset's true chrome.
/// (Demonstrates the new `ThemeSwitcher::composite_tooltip` setter.)
fn theme_switch_caveat() -> impl Widget + 'static {
    // The tooltip body MUST use the tooltip text roles — `TooltipText` (full
    // contrast) / `TooltipShortcut` (de-emphasised) — not normal on-surface
    // roles (`Primary`/`Secondary`/`Accent`). The chip is a dark inverse
    // surface under IntUI and Material 3 (`inverseSurface`) and a light
    // flyout under Fluent, so a fixed on-surface role would be unreadable in
    // one of them. These two roles resolve to the theme's `tooltip_text` /
    // `tooltip_shortcut`, so they track the chip across all three families,
    // light and dark.
    MaxSize::width(360.0).child(
        VStack::new()
            .spacing(6.0)
            .child(
                TextWidget::new(lit!("About switching theme at runtime"))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::TooltipText),
            )
            .child(
                TextWidget::new(lit!(
                    "Colours retint instantly — IntUI Light ↔ Dark is fully live \
                     and preserves your focus, scroll position and text."
                ))
                .style(TextStyleRole::Small)
                .color(TextRole::TooltipShortcut),
            )
            .child(
                TextWidget::new(lit!(
                    "Widget shapes (Material 3's pill buttons and switch, Fluent's \
                     elevation edge and list selection pill, card radii) are chosen \
                     when the UI is built. Switching theme family here changes only \
                     the colours — the shapes stay until the UI is rebuilt."
                ))
                .style(TextStyleRole::Small)
                .color(TextRole::TooltipShortcut),
            )
            .child(
                TextWidget::new(lit!(
                    "To see a preset's true chrome, start the catalog with \
                     --theme macos-light (or macos-dark / fluent-light / \
                     fluent-dark / material3-light / material3-dark / \
                     intui-light / intui-dark)."
                ))
                .style(TextStyleRole::Small)
                .color(TextRole::TooltipText),
            ),
    )
}

/// Build the custom title bar. Uses role-driven background/border so
/// the chrome retints live across `ctx.set_theme(...)` switches.
fn build_title_bar(
    host: Rc<dyn PlatformTitleBarHost>,
    _theme: &Theme,
    menu_bar: WidgetId,
) -> impl Widget + 'static {
    let brand = TextWidget::new(tr!(app_title()))
        .style(TextStyleRole::BodyBold)
        .color(TextRole::Primary);

    let leading: HStack = HStack::new().spacing(4.0).add_child(menu_bar).child(brand);

    // Center the subtitle with flexible spacers rather than `Center`:
    // `Center` reports `flex = 0`, so in an HStack it shrink-wraps to the
    // text's intrinsic width — the ellipsis text never gets a bounded width
    // to truncate against and overflows its neighbours on a deficit. With
    // equal `Spacer`s the text still receives a bounded width — slack
    // centers it, an over-constraint deficit shrinks/ellipsizes it.
    let center = HStack::new()
        .child(Spacer::new())
        .child(
            TextWidget::new(tr!(app_subtitle()))
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary)
                .overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing)),
        )
        .child(Spacer::new());

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

    // Global text-scale control — grows all text in the app for low-vision
    // users. Lives next to the language buttons because it is the same kind of
    // app-wide accessibility/locale preference. `TextScaleSlot` binds it to the
    // persisted `TEXT_SCALE_KEY` from inside a `build()` (where `ctx.settings()`
    // is reachable), so the chosen size persists and restores on restart.
    let scale_ctrl = TextScaleSlot::default();

    // Theme switcher — IntUI Light/Dark plus the Material 3, Fluent and
    // macOS presets, with the OS-follow "System" entry kept on. Selecting an
    // entry re-tints the catalog live (colours, focus/scroll preserved). It
    // does NOT change widget *chrome* at runtime: shapes (M3 pills and switch,
    // Fluent's elevation edge and selection pill, macOS's bezels and selection
    // capsule, card radii) are chosen when the UI is built, so a live family
    // switch keeps the current shapes — start with `--theme macos-light` to
    // see a preset's true chrome. The composite tooltip below spells this out
    // for the user.
    let theme_switcher = teksilo::widgets::ThemeSwitcher::new()
        .themes([
            (lit!("IntUI Light"), teksilo::presets::intui::light()),
            (lit!("IntUI Dark"), teksilo::presets::intui::dark()),
            (
                lit!("Material 3 Light"),
                teksilo::prelude::material3::light(),
            ),
            (lit!("Material 3 Dark"), teksilo::prelude::material3::dark()),
            (lit!("Fluent Light"), teksilo::prelude::fluent::light()),
            (lit!("Fluent Dark"), teksilo::prelude::fluent::dark()),
            (lit!("macOS Light"), teksilo::prelude::macos::light()),
            (lit!("macOS Dark"), teksilo::prelude::macos::dark()),
        ])
        .composite_tooltip(theme_switch_caveat());

    let trailing = HStack::new()
        .spacing(4.0)
        .child(en_btn)
        .child(fr_btn)
        .child(ar_btn)
        .child(scale_ctrl)
        .child(theme_switcher);

    TitleBar::new(host)
        .height(40.0)
        .background(SurfaceRole::Raised)
        .border(BorderRole::Default, 1.0)
        .leading(leading)
        .center(center)
        .trailing(trailing)
        .close_action(|ctx| ctx.close_window())
}

/// Thin wrapper that binds [`TextScaleControl`] to the persisted
/// `TEXT_SCALE_KEY`. The title bar is built without a `BuildContext`, so the
/// settings handle isn't reachable there — this slot defers the binding to its
/// own `build()`, where `ctx.settings()` is available. Edits then persist (and
/// restore on the next launch) while still applying app-wide live.
#[derive(Debug, Default)]
struct TextScaleSlot {
    root_child_id: Option<WidgetId>,
}

impl Widget for TextScaleSlot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let scale = ctx.settings().signal_for(&TEXT_SCALE_KEY);
        let id = ctx.add(TextScaleControl::new(scale));
        self.root_child_id = Some(id);
        vec![id]
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

/// Invisible slot that persists the active theme to settings and restores it
/// on the next launch — so a user who picks Dark (or System) via the
/// `ThemeSwitcher` finds it preserved across runs. The selection is keyed by
/// the theme's stable [`teksilo::core::ThemeId`]: `"intui.light"` /
/// `"intui.dark"` for fixed themes, `"system"` for follow-OS. Mirrors
/// `TextScaleSlot` (which does the same for the text-scale preference); the
/// binding lives in `build()` where `ctx.settings()` is reachable.
#[derive(Debug, Default)]
struct ThemePersistenceSlot {
    /// When a `--theme` flag forced the startup theme, skip the restore so
    /// the forced theme (set on the builder) wins; changes still persist.
    skip_restore: bool,
}

const THEME_PREF_KEY: &str = "ui.theme";

/// Resolve a `--theme NAME` value to a concrete `Theme`.
fn theme_from_name(name: &str) -> Option<teksilo::core::Theme> {
    use teksilo::prelude::{fluent, macos, material3};
    use teksilo::presets::intui;
    match name {
        "intui-light" => Some(intui::light()),
        "intui-dark" => Some(intui::dark()),
        "material3-light" | "m3-light" => Some(material3::light()),
        "material3-dark" | "m3-dark" => Some(material3::dark()),
        "fluent-light" => Some(fluent::light()),
        "fluent-dark" => Some(fluent::dark()),
        "macos-light" => Some(macos::light()),
        "macos-dark" => Some(macos::dark()),
        other => {
            eprintln!("--theme: unknown theme `{other}` — using the default");
            None
        }
    }
}

/// Restore a persisted theme id (`ui.theme`) to a concrete `Theme`.
///
/// Keyed by the theme's stable `ThemeId`, which uses dots
/// (`"fluent.dark"`) where `--theme` uses hyphens — the two namespaces are
/// deliberately separate, so this is not a duplicate of
/// [`theme_from_name`]. Every preset offered by the switcher must appear
/// here or picking it persists fine and silently reverts on next launch.
fn theme_from_id(id: &str) -> Option<teksilo::core::Theme> {
    use teksilo::prelude::{fluent, macos, material3};
    use teksilo::presets::intui;
    match id {
        "intui.light" => Some(intui::light()),
        "intui.dark" => Some(intui::dark()),
        "material3.light" => Some(material3::light()),
        "material3.dark" => Some(material3::dark()),
        "fluent.light" => Some(fluent::light()),
        "fluent.dark" => Some(fluent::dark()),
        "macos.light" => Some(macos::light()),
        "macos.dark" => Some(macos::dark()),
        // Unknown / custom id: keep the builder default.
        _ => None,
    }
}

impl Widget for ThemePersistenceSlot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let pref = ctx
            .settings()
            .signal::<String>(THEME_PREF_KEY, String::new());

        // Persist whenever the active theme changes (debounced by the store).
        {
            let pref = pref.clone();
            ctx.effect(&ctx.theme_signal(), move |theme| {
                let id = theme.id.as_str().to_string();
                if pref.get() != id {
                    pref.set(id);
                }
            });
        }

        // Restore the saved theme once, after mount (an `EventContext` — needed
        // for set_theme / follow_system_theme — is only reachable post-mount).
        // Skipped when `--theme` forced the startup theme.
        let saved = pref.get();
        if !self.skip_restore && !saved.is_empty() {
            ctx.run_after_mount(move |ectx| {
                if saved == "system" {
                    ectx.follow_system_theme();
                } else if let Some(theme) = theme_from_id(&saved) {
                    ectx.set_theme(theme);
                }
            });
        }

        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
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
        .collapse_policy(teksilo::widgets::CollapsePolicy::Always)
        .hamburger_size(teksilo::widgets::IconButtonSize::Large)
        .native_on_macos(teksilo::widgets::NativeMenuMode::Suppress)
        .menu(tr!(app_menu_file()), || {
            Box::new(
                MenuList::new().item(
                    MenuItem::new(tr!(app_menu_quit()))
                        .shortcut_label(format_keystroke(KeyStroke::command(Key::Q)))
                        .on_activate_fn(|ctx| ctx.close_window()),
                ),
            )
        })
        .menu(tr!(app_menu_help()), || {
            Box::new(
                MenuList::new()
                    .item(
                        MenuItem::new(tr!(app_menu_documentation())).on_activate_fn(|_| {
                            println!("Documentation: https://github.com/FernTech/teksilo");
                        }),
                    )
                    .separator()
                    .item(MenuItem::new(tr!(app_menu_about())).on_activate_fn(|_| {
                        println!(
                            "Teksilo Widget Catalog — every public widget, classic vs teksu! \
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
    show_teksi: Signal<bool>,
    selected_tab: Signal<Option<TabId>>,
    root_child_id: Option<WidgetId>,
}

impl WidgetCatalog {
    fn new(
        options: cli::CliOptions,
        tab_ids: Rc<Vec<TabId>>,
        show_teksi: Signal<bool>,
        selected_tab: Signal<Option<TabId>>,
    ) -> Self {
        Self {
            options,
            tab_ids,
            show_teksi,
            selected_tab,
            root_child_id: None,
        }
    }
}

impl Widget for WidgetCatalog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let sigs = Signals::new(ctx);

        // `view_mode: Signal<usize>` — derived from `show_teksi` so that
        // every tab's Switcher binds to the same source.
        let view_mode = self.show_teksi.map(|b| if *b { 1 } else { 0 });

        // ── --cycle: auto-advance the tab on a timer ─────────────────
        if let Some(period) = self.options.cycle {
            self.install_cycle(ctx, period);
        }

        // ── Demo shortcuts so the Settings tab's ShortcutSettings has
        //    something to display. Mostly catalogued for the rebind UI,
        //    not for execution — except `app.quit`, which is wired to a
        //    real Action below so Ctrl+Q actually closes the window.
        for shortcut in demo_shortcuts() {
            ctx.register_shortcut_global(shortcut);
        }

        // Wire the one demo shortcut we want live: Ctrl+Q → close window.
        // The File ▸ Quit menu item shows the same "Ctrl+Q" label, but a
        // label alone is display-only; the chord needs an Action keyed by
        // the shortcut's intent name to fire.
        ctx.register_action(Action::new("app.quit").on_invoke(|_i, ctx| ctx.close_window()));

        // ── TabWidget ────────────────────────────────────────────────
        let mut tw = TabWidget::new(self.selected_tab.clone())
            .vertical()
            .max_tab_width(180.0)
            .tab_background(SurfaceRole::Sunken);
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
        // Zero flex-basis (the default) — deliberately NOT `respect_intrinsic()`.
        //
        // `respect_intrinsic()` switches to an AUTO basis, making the child's
        // natural size a floor. A *vertical* `TabBar` answers an unbounded
        // height query with `natural_height_vertical` — every tab stacked, ~1050 dp
        // for this catalog's 21 tabs (see `tab_widget/bar.rs`). That became the
        // Expand's wanted height, so the root `VStack` wanted 1050 + status bar
        // and, since `Expand` has `shrink = 0`, the deficit could not be absorbed:
        // the StatusBar was placed at y=1050 and stayed below the fold until the
        // window was grown past ~1066 dp tall.
        //
        // The auto basis is for a wrapper inside an *unconstrained* parent; the
        // window gives this root a bounded height, so the zero basis is correct —
        // the bar takes the slack left after the status bar and scrolls its tabs.
        let tabs_filling = ctx.add(Expand::vertical().child_id(tabs_id));

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
        Toggle::new(self.show_teksi.clone()).label(tr!(mode_label()))
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
// TabContent — the per-tab widget that hosts the classic/teksu Switcher.
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
        let teksi_id = (self.entry.teksu)(ctx, &self.sigs);

        let switcher = ctx.add(
            Switcher::new(self.view_mode.clone())
                .child_id(classic_id)
                .child_id(teksi_id),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every preset the title-bar switcher offers, as `(--theme name,
    /// persisted ThemeId)`. The switcher itself is built inside a
    /// `BuildContext`, so this list is the testable stand-in — keep it in
    /// step with `build_title_bar`'s `.themes([...])`.
    const OFFERED: &[(&str, &str)] = &[
        ("intui-light", "intui.light"),
        ("intui-dark", "intui.dark"),
        ("material3-light", "material3.light"),
        ("material3-dark", "material3.dark"),
        ("fluent-light", "fluent.light"),
        ("fluent-dark", "fluent.dark"),
        ("macos-light", "macos.light"),
        ("macos-dark", "macos.dark"),
    ];

    #[test]
    fn every_cli_theme_name_resolves() {
        for (name, _) in OFFERED {
            assert!(
                theme_from_name(name).is_some(),
                "--theme {name} does not resolve"
            );
        }
    }

    #[test]
    fn cli_names_resolve_to_the_theme_they_name() {
        for (name, id) in OFFERED {
            let theme = theme_from_name(name).expect("resolves");
            assert_eq!(
                theme.id.as_str(),
                *id,
                "--theme {name} resolved to {}",
                theme.id
            );
        }
    }

    #[test]
    fn material3_aliases_still_resolve() {
        assert_eq!(
            theme_from_name("m3-light").map(|t| t.id.as_str().to_string()),
            Some("material3.light".to_string())
        );
        assert_eq!(
            theme_from_name("m3-dark").map(|t| t.id.as_str().to_string()),
            Some("material3.dark".to_string())
        );
    }

    #[test]
    fn unknown_theme_name_falls_back() {
        assert!(theme_from_name("nope").is_none());
        assert!(theme_from_name("").is_none());
    }

    /// The regression this file is most exposed to: a preset added to the
    /// switcher persists its `ThemeId` generically, but is restored by a
    /// hand-written match. Miss the arm and the theme silently reverts on
    /// the next launch — which looks like nothing at all went wrong.
    #[test]
    fn every_offered_theme_survives_a_persist_restore_round_trip() {
        for (name, id) in OFFERED {
            let persisted = theme_from_name(name).expect("resolves").id;
            let restored = theme_from_id(persisted.as_str())
                .unwrap_or_else(|| panic!("`{persisted}` has no restore arm"));
            assert_eq!(restored.id.as_str(), *id);
        }
    }

    #[test]
    fn unknown_persisted_id_keeps_the_builder_default() {
        assert!(theme_from_id("custom").is_none());
        // "system" is handled by the caller as follow-OS, not here.
        assert!(theme_from_id("system").is_none());
    }

    #[test]
    fn light_and_dark_variants_report_their_appearance() {
        use teksilo::core::styles::ThemeAppearance;
        for (name, _) in OFFERED {
            let theme = theme_from_name(name).expect("resolves");
            let expected = if name.ends_with("-dark") {
                ThemeAppearance::Dark
            } else {
                ThemeAppearance::Light
            };
            assert_eq!(theme.appearance, expected, "{name}");
        }
    }
}
