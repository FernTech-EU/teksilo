//! Recent Projects Demo — end-to-end exercise of `fern-settings`.
//!
//! Run with: `cargo run -p recent-projects`
//!
//! Demonstrates:
//!
//! * `FernAppBuilder::application(...)` + `.settings(SettingsBundle)`
//!   wiring the dynamic K/V store and the window-state service into
//!   the app's `app_state` registry. Window geometry save/restore is
//!   automatic when the `WindowConfig` carries an `id("main")` —
//!   `fern-app` reads the saved state, sanitizes it against the
//!   current monitor (so a coordinate from a now-disconnected screen
//!   is recentered, never spawned off-screen), then opens the
//!   window. Every move/resize/maximize is debounced into the
//!   on-disk service.
//! * An app-defined [`RecentProject`] with an `MruEntry` impl, so a
//!   generic `MruList<RecentProject>` provides dedupe / pinning /
//!   capping for free. The framework knows nothing about projects.
//! * `SettingsKey<T>` constants for canonical setting names with
//!   defaults at one place.
//! * Reactive `Signal` bindings: a font-size scalar surfaces in a
//!   `TextWidget::bind_text(font_size.map(...))`. A `show_paths` flag
//!   conditionally clears each row's path string via the same
//!   mapping pattern.
//! * The recents list rendered through `Repeater<RecentProject>` —
//!   `ListModel` mutations cascade to incremental UI updates.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fern_ui::IntentKind;
use fern_ui::core::Action;
use fern_ui::prelude::*;
use fern_ui::settings::{AppPaths, MruEntry, MruList, SettingsBundle, SettingsExt, SettingsKey};
use fern_ui::widgets::{
    Button, ButtonVariant, Expand, HStack, Padding, Panel, Repeater, Spacer, TextWidget, Toolbar,
    VStack,
};
use serde::{Deserialize, Serialize};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new_literal("Toggle Dark Mode").on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                fern_ui::presets::intui::dark()
            } else {
                fern_ui::presets::intui::light()
            });
        }),
    ))
}

// ----- App-defined recents item ------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RecentProject {
    pub path: PathBuf,
    pub display_name: String,
    pub last_opened: u64,
    #[serde(default)]
    pub pinned: bool,
}

impl RecentProject {
    pub fn new(path: impl Into<PathBuf>, display_name: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            display_name: display_name.into(),
            last_opened: now_unix(),
            pinned: false,
        }
    }
    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }
}

impl MruEntry for RecentProject {
    type Key = Path;
    fn key(&self) -> &Path {
        &self.path
    }
    fn is_pinned(&self) -> bool {
        self.pinned
    }
    fn set_pinned(&mut self, p: bool) {
        self.pinned = p;
    }
    fn touch(&mut self) {
        self.last_opened = now_unix();
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ----- Settings keys (single source of truth) ----------------------------

const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("ui.font_size", || 16.0);
const SHOW_PATHS: SettingsKey<bool> = SettingsKey::new("ui.show_paths", || true);

const WINDOW_LABEL: &str = "main";
const DEFAULT_SIZE: (u32, u32) = (960, 720);
const MIN_SIZE: (u32, u32) = (480, 360);

// ----- Typed intent catalog ----------------------------------------------

#[derive(Debug, IntentKind)]
enum AppIntent {
    #[name = "ui.font_grow"]
    FontGrow,
    #[name = "ui.font_shrink"]
    FontShrink,
    #[name = "ui.toggle_show_paths"]
    ToggleShowPaths,
    #[name = "app.open_recent"]
    OpenRecent(String),
    #[name = "app.toggle_pin"]
    TogglePin(String),
    #[name = "app.remove_recent"]
    RemoveRecent(String),
    #[name = "app.clear_recents"]
    ClearRecents,
    #[name = "demo.seed"]
    Seed,
}

// ----- Root widget --------------------------------------------------------

#[derive(Debug, Default)]
struct Root {
    root_child_id: Option<WidgetId>,
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // ----- Wire actions -------------------------------------------
        ctx.register_action(Action::new("ui.font_grow").on_invoke(|_, ctx| {
            let s = ctx.settings().signal_for(&FONT_SIZE);
            s.set((s.get() + 1.0).min(40.0));
        }));
        ctx.register_action(Action::new("ui.font_shrink").on_invoke(|_, ctx| {
            let s = ctx.settings().signal_for(&FONT_SIZE);
            s.set((s.get() - 1.0).max(8.0));
        }));
        ctx.register_action(Action::new("ui.toggle_show_paths").on_invoke(|_, ctx| {
            let s = ctx.settings().signal_for(&SHOW_PATHS);
            s.set(!s.get());
        }));
        ctx.register_action(Action::new("app.open_recent").on_invoke(|i, ctx| {
            if let Some(AppIntent::OpenRecent(path)) = AppIntent::from_intent(i) {
                println!("[demo] Open project at {path}");
                ctx.mru::<RecentProject>().touch(Path::new(path));
            }
        }));
        ctx.register_action(Action::new("app.toggle_pin").on_invoke(|i, ctx| {
            if let Some(AppIntent::TogglePin(path)) = AppIntent::from_intent(i) {
                ctx.mru::<RecentProject>().toggle_pin(Path::new(path));
            }
        }));
        ctx.register_action(Action::new("app.remove_recent").on_invoke(|i, ctx| {
            if let Some(AppIntent::RemoveRecent(path)) = AppIntent::from_intent(i) {
                ctx.mru::<RecentProject>().remove(Path::new(path));
            }
        }));
        ctx.register_action(Action::new("app.clear_recents").on_invoke(|_, ctx| {
            ctx.mru::<RecentProject>().clear();
        }));
        ctx.register_action(Action::new("demo.seed").on_invoke(|_, ctx| {
            let mru = ctx.mru::<RecentProject>();
            for (path, name, pinned) in [
                ("/projects/skribisto", "Skribisto", true),
                ("/projects/fern-ui", "FernUI", false),
                ("/notes/journal-2026.md", "journal-2026.md", false),
                ("/sandbox/playground", "playground", false),
            ] {
                let mut p = RecentProject::new(path, name);
                if pinned {
                    p = p.pinned();
                }
                mru.add(p);
            }
        }));

        // (No widget-side window-persist call needed — fern-app's
        // window manager handles it automatically when the
        // WindowConfig carries `id(WINDOW_LABEL)` and a
        // WindowStateService is registered via `.settings(...)`.)

        // ----- Reactive bindings --------------------------------------
        let font_size = ctx.settings().signal_for(&FONT_SIZE);
        let show_paths = ctx.settings().signal_for(&SHOW_PATHS);
        let store_path = ctx.settings().path().display().to_string();
        let recents_model = ctx.mru::<RecentProject>().model().clone();
        let theme = ctx.theme_signal().get();

        // ----- Header -------------------------------------------------
        let title = TextWidget::new_literal("Recent Projects")
            .style(theme.typography.body_bold.clone())
            .color(theme.colors.text_primary);
        let subtitle = TextWidget::new_literal(format!("Settings stored at: {store_path}"))
            .color(theme.colors.text_secondary);

        let font_label = TextWidget::new_literal("")
            .color(theme.colors.text_primary)
            .bind_text(font_size.clone().map(|v| format!("Font size: {v:.0} pt")));
        let show_paths_label = TextWidget::new_literal("")
            .color(theme.colors.text_secondary)
            .bind_text(
                show_paths
                    .clone()
                    .map(|v| format!("Show paths: {}", if *v { "on" } else { "off" })),
            );

        let header = Panel::new()
            .background(theme.colors.surface_content)
            .corner_radius(8.0)
            .padding(16.0)
            .child(
                VStack::new()
                    .spacing(6.0)
                    .child(title)
                    .child(subtitle)
                    .child(font_label)
                    .child(show_paths_label),
            );

        // ----- Toolbar (font size + show/hide paths + clear) ---------
        let smaller_btn = ctx.add(
            Button::new_literal("Smaller font")
                .style(ButtonVariant::Default)
                .on_activate_fn(|ctx: &mut EventContext| {
                    ctx.send_intent(AppIntent::FontShrink);
                }),
        );
        let bigger_btn = ctx.add(
            Button::new_literal("Bigger font")
                .style(ButtonVariant::Default)
                .on_activate_fn(|ctx: &mut EventContext| {
                    ctx.send_intent(AppIntent::FontGrow);
                }),
        );
        let show_paths_btn = ctx.add(Button::new_literal("Show paths").on_activate_fn(
            |ctx: &mut EventContext| {
                ctx.send_intent(AppIntent::ToggleShowPaths);
            },
        ));
        ctx.visible_when(show_paths_btn, show_paths.not());

        let hide_paths_btn = ctx.add(Button::new_literal("Hide paths").on_activate_fn(
            |ctx: &mut EventContext| {
                ctx.send_intent(AppIntent::ToggleShowPaths);
            },
        ));
        ctx.visible_when(hide_paths_btn, show_paths.clone());

        let seed_btn = ctx.add(Button::new_literal("Seed demo entries").on_activate_fn(
            |ctx: &mut EventContext| {
                ctx.send_intent(AppIntent::Seed);
            },
        ));
        let clear_btn = ctx.add(
            Button::new_literal("Clear recents")
                .style(ButtonVariant::Default)
                .on_activate_fn(|ctx: &mut EventContext| {
                    ctx.send_intent(AppIntent::ClearRecents);
                }),
        );

        let toolbar = HStack::new()
            .spacing(8.0)
            .add_child(smaller_btn)
            .add_child(bigger_btn)
            .add_child(show_paths_btn)
            .add_child(hide_paths_btn)
            .child(Spacer::new())
            .add_child(seed_btn)
            .add_child(clear_btn);

        // ----- Recents list (Repeater) -------------------------------
        let theme_for_factory = theme.clone();
        let show_paths_for_factory = show_paths.clone();
        let repeater = Repeater::new(recents_model, move |_idx, project: &RecentProject| {
            let path_str = project.path.display().to_string();
            let display_name = project.display_name.clone();
            let pinned = project.pinned;
            let theme = theme_for_factory.clone();

            let title_text = if pinned {
                format!("★ {display_name}")
            } else {
                display_name.clone()
            };
            let row_title = TextWidget::new_literal(title_text).color(theme.colors.text_primary);

            let path_for_display = path_str.clone();
            let path_text_signal = show_paths_for_factory.clone().map(move |show| {
                if *show {
                    path_for_display.clone()
                } else {
                    String::new()
                }
            });
            let path_text_widget = TextWidget::new_literal("")
                .color(theme.colors.text_secondary)
                .bind_text(path_text_signal);

            let label_stack = VStack::new()
                .spacing(2.0)
                .child(row_title)
                .child(path_text_widget);

            let path_for_open = path_str.clone();
            let open_btn =
                Button::new_literal("Open").on_activate_fn(move |ctx: &mut EventContext| {
                    ctx.send_intent(AppIntent::OpenRecent(path_for_open.clone()));
                });
            let path_for_pin = path_str.clone();
            let pin_label = if pinned { "Unpin" } else { "Pin" };
            let pin_btn =
                Button::new_literal(pin_label).on_activate_fn(move |ctx: &mut EventContext| {
                    ctx.send_intent(AppIntent::TogglePin(path_for_pin.clone()));
                });
            let path_for_remove = path_str.clone();
            let remove_btn = Button::new_literal("Remove")
                .style(ButtonVariant::Default)
                .on_activate_fn(move |ctx: &mut EventContext| {
                    ctx.send_intent(AppIntent::RemoveRecent(path_for_remove.clone()));
                });

            Box::new(
                Panel::new()
                    .background(theme.colors.surface_main)
                    .corner_radius(6.0)
                    .padding(10.0)
                    .child(
                        HStack::new()
                            .spacing(10.0)
                            .child(label_stack)
                            .child(Spacer::new())
                            .child(open_btn)
                            .child(pin_btn)
                            .child(remove_btn),
                    ),
            )
        })
        .spacing(8.0);

        let recents_panel = Panel::new()
            .background(theme.colors.surface_content)
            .corner_radius(8.0)
            .padding(12.0)
            .child(
                VStack::new()
                    .spacing(8.0)
                    .child(
                        TextWidget::new_literal("Recents")
                            .style(theme.typography.body_bold.clone())
                            .color(theme.colors.text_primary),
                    )
                    .child(repeater),
            );

        let root = ctx.add(
            Padding::uniform(16.0).child(
                VStack::new()
                    .spacing(16.0)
                    .child(header)
                    .child(toolbar)
                    .child(recents_panel),
            ),
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
}

fn main() {
    let paths = AppPaths::new("com", "FernTech", "RecentProjectsDemo")
        .or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            Some(AppPaths::for_testing(
                &cwd.join(".recent-projects-demo-state"),
            ))
        })
        .expect("could not resolve a usable directory for settings");

    // The MruList is app-typed (the framework doesn't know about
    // RecentProject), so the app constructs and registers it.
    let recents: MruList<RecentProject> =
        MruList::open(&paths, "recent_projects", 8).expect("open recent_projects.toml");

    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(fern_ui::presets::intui::light())
        .app_paths(paths)
        .settings(SettingsBundle::new().with_window_state(true))
        .app_state(recents)
        .initial_window(
            // Auto save/restore is enabled by `.id(WINDOW_LABEL)`:
            // fern-app's window manager looks up the saved geometry
            // for that id, sanitizes it against the current monitor,
            // and opens the window at the corrected size/position.
            // Subsequent move/resize/maximize events flow back to
            // disk debounced.
            WindowConfig::new()
                .id(WINDOW_LABEL)
                .title("FernUI — Recent Projects Demo")
                .size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
                .min_size(MIN_SIZE.0, MIN_SIZE.1)
                .root(|tree, _state| {
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(Root::default())),
                    )
                }),
        )
        .run();
}
