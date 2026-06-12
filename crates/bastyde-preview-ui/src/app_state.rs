//! Shared app state and the previewer's root widget.
//!
//! `AppState` is a `Clone` bundle of signals consumed by every pane.
//! `PreviewerRoot` is the actual composite widget that builds the
//! 4-way layout (toolbar over a 3-pane Splitter) inside its
//! `build()`.
//!
//! The bare minimum is the navigator widget list; the full 4-pane
//! layout is built incrementally on top.

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_preview::{KnobOverrides, KnobValues, PreviewVariant};
use bastyde_widgets::{
    Center, Expand, Orientation, PaneDescriptor, Splitter, SplitterModel, TextWidget, VStack,
};

use crate::canvas::PreviewCanvas;
use crate::inspector::build_inspector;
use crate::navigator::build_navigator;
use crate::toolbar::build_toolbar;

/// Background-mode selector for the canvas pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundMode {
    /// Use `SurfaceRole::Main` from the canvas's own theme.
    Themed,
    /// Use `SurfaceRole::Content` (sunken / inset look).
    ContentSurface,
    /// Use `SurfaceRole::Sunken` — flat sunken look.
    Sunken,
    /// Show a checkered transparency pattern (light + dark squares).
    Checkered,
}

impl BackgroundMode {
    pub const ALL: &'static [BackgroundMode] = &[
        BackgroundMode::Themed,
        BackgroundMode::ContentSurface,
        BackgroundMode::Sunken,
        BackgroundMode::Checkered,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BackgroundMode::Themed => "Themed",
            BackgroundMode::ContentSurface => "Content",
            BackgroundMode::Sunken => "Sunken",
            BackgroundMode::Checkered => "Checkered",
        }
    }
}

/// Theme choice for the canvas sub-tree (separate from the previewer
/// chrome's theme so a buggy theme can't break the tool itself).
///
/// - `Native` — adopt the OS desktop's actual colour palette (KDE
///   Breeze, GNOME Adwaita, Cinnamon Mint-Y, etc.) by querying
///   `bastyde_platform::os_theme::query_os_theme_colors`. Same semantics
///   as the framework's `ThemeMode::Native`. Resolves at click-time;
///   clicking again re-queries.
/// - `Light` / `Dark` — the framework's built-in JetBrains Int UI
///   light/dark themes. Independent of the OS.
///
/// We deliberately do *not* expose a "follow OS light/dark
/// preference but stay on Int UI" option — that would coincide with
/// `Light`/`Dark` whenever the OS preference matched, producing two
/// buttons that look identical. The current three options each give
/// a distinct, predictable result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasTheme {
    Native,
    Light,
    Dark,
}

impl CanvasTheme {
    pub const ALL: &'static [CanvasTheme] =
        &[CanvasTheme::Native, CanvasTheme::Light, CanvasTheme::Dark];

    pub fn label(self) -> &'static str {
        match self {
            CanvasTheme::Native => "Native",
            CanvasTheme::Light => "Light",
            CanvasTheme::Dark => "Dark",
        }
    }

    /// Resolve to a concrete theme. For `Native`, builds a theme that
    /// *adopts* the OS palette rather than picking between Bastyde's
    /// built-in Int UI light/dark — this is what gives a visibly
    /// distinct result from clicking Light/Dark on most desktops.
    pub fn theme(self) -> bastyde_core::Theme {
        match self {
            CanvasTheme::Native => {
                let os = bastyde_platform::os_theme::query_os_theme_colors();
                let base = if os.color_scheme.is_dark() {
                    bastyde_core::presets::intui::dark()
                } else {
                    bastyde_core::presets::intui::light()
                };
                bastyde_core::Theme {
                    colors: bastyde_tokens::ColorTokens::from_os_colors(&os),
                    ..base
                }
            }
            CanvasTheme::Light => bastyde_core::presets::intui::light(),
            CanvasTheme::Dark => bastyde_core::presets::intui::dark(),
        }
    }
}

/// Cache key — `(widget_id, variant_name)`.
pub type CacheKey = (&'static str, &'static str);

/// Shared state passed to each pane. Cheap `Clone` — every field is a
/// signal or `Rc`-backed.
#[derive(Clone)]
pub struct AppState {
    pub selected_widget: Signal<Option<&'static str>>,
    pub selected_variant: Signal<Option<&'static str>>,

    /// Per-(widget, variant) cache of `KnobValues`. Lazily populated;
    /// reused across navigations so user knob edits persist.
    pub knobs_cache: Rc<RefCell<HashMap<CacheKey, KnobValues>>>,

    /// Bumped to force the canvas to rebuild — used by "Reset" and by
    /// any operation that swaps the cached `KnobValues` out from under
    /// the canvas.
    pub canvas_rebuild_tick: Signal<u64>,

    pub canvas_theme: Signal<CanvasTheme>,
    pub canvas_locale: Signal<Option<String>>,
    #[allow(dead_code)]
    pub zoom_percent: Signal<f32>,
    pub background_mode: Signal<BackgroundMode>,

    /// Navigator filter text.
    pub navigator_filter: Signal<String>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            selected_widget: Signal::new(None),
            selected_variant: Signal::new(None),
            knobs_cache: Rc::new(RefCell::new(HashMap::new())),
            canvas_rebuild_tick: Signal::new(0),
            // Default to "Native" so the previewer adopts the OS
            // desktop palette at startup (KDE Breeze, GNOME Adwaita,
            // etc.). `run_previewer` calls `CanvasTheme::Native.theme()`
            // to resolve the initial concrete `Theme` so chrome,
            // canvas, and the highlighted toolbar button all agree
            // on frame 1.
            canvas_theme: Signal::new(CanvasTheme::Native),
            canvas_locale: Signal::new(None),
            zoom_percent: Signal::new(100.0),
            background_mode: Signal::new(BackgroundMode::Themed),
            navigator_filter: Signal::new(String::new()),
        }
    }

    /// Look up or create the `KnobValues` for `(widget_id, variant_name)`.
    pub fn knobs_for(&self, widget_id: &'static str, variant_name: &'static str) -> KnobValues {
        let key = (widget_id, variant_name);
        if let Some(values) = self.knobs_cache.borrow().get(&key) {
            return values.clone();
        }
        let entry = match bastyde_preview::find_by_id(widget_id) {
            Some(e) => e,
            None => panic!(
                "AppState::knobs_for: no entry registered with id '{}'",
                widget_id
            ),
        };
        let spec = entry.knobs();
        let overrides = entry
            .variants()
            .into_iter()
            .find_map(|v| match v {
                PreviewVariant::Knobs { name, overrides } if name == variant_name => {
                    Some(overrides)
                }
                _ => None,
            })
            .unwrap_or_else(KnobOverrides::new);
        let values = KnobValues::from_spec(&spec, Some(&overrides));
        self.knobs_cache.borrow_mut().insert(key, values.clone());
        values
    }

    /// Drop the cache entry for `(widget_id, variant_name)` and force
    /// the canvas to rebuild — used by the inspector's "Reset" button.
    pub fn reset_knobs(&self, widget_id: &'static str, variant_name: &'static str) {
        self.knobs_cache
            .borrow_mut()
            .remove(&(widget_id, variant_name));
        let v = self.canvas_rebuild_tick.get();
        self.canvas_rebuild_tick.set(v.wrapping_add(1));
    }

    /// Select the first registered entry (in registry iteration order)
    /// and its first variant. Used at startup when no `--widget` was
    /// supplied on the CLI.
    pub fn select_first_registered(&self) {
        if let Some(entry) = bastyde_preview::iter_entries().next() {
            self.selected_widget.set(Some(entry.id()));
            if let Some(first_variant) = entry.variants().first() {
                self.selected_variant.set(Some(first_variant.name()));
            }
        }
    }

    /// Select a specific widget (and its first variant if no
    /// variant is currently active for that widget).
    pub fn select_widget(&self, widget_id: &'static str) {
        if self.selected_widget.get() == Some(widget_id) {
            return;
        }
        self.selected_widget.set(Some(widget_id));
        let entry = bastyde_preview::find_by_id(widget_id).expect("entry exists");
        if let Some(first) = entry.variants().first() {
            self.selected_variant.set(Some(first.name()));
        } else {
            self.selected_variant.set(None);
        }
    }

    pub fn select_variant(&self, variant_name: &'static str) {
        self.selected_variant.set(Some(variant_name));
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Top-level previewer composite. Holds the [`AppState`] and builds
/// the toolbar + 3-pane Splitter layout.
pub struct PreviewerRoot {
    state: Option<AppState>,
    initial_widget: Option<String>,
    initial_variant: Option<String>,
    root_id: Option<WidgetId>,
}

impl PreviewerRoot {
    pub fn new(initial_widget: Option<String>, initial_variant: Option<String>) -> Self {
        Self {
            state: None,
            initial_widget,
            initial_variant,
            root_id: None,
        }
    }

    fn resolve_initial_widget(&self) -> Option<&'static str> {
        let id = self.initial_widget.as_deref()?;
        bastyde_preview::find_by_id(id).map(|e| e.id())
    }

    fn resolve_initial_variant(&self, widget_id: &'static str) -> Option<&'static str> {
        let want = self.initial_variant.as_deref()?;
        let entry = bastyde_preview::find_by_id(widget_id)?;
        entry
            .variants()
            .into_iter()
            .find(|v| v.name() == want)
            .map(|v| v.name())
    }
}

impl std::fmt::Debug for PreviewerRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewerRoot")
            .field("initial_widget", &self.initial_widget)
            .field("initial_variant", &self.initial_variant)
            .finish()
    }
}

impl Widget for PreviewerRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.get_or_insert_with(AppState::new).clone();

        // Initial selection — only on first build, never overwrite an
        // existing selection on rebuild.
        if state.selected_widget.get().is_none() {
            if let Some(widget_id) = self.resolve_initial_widget() {
                state.selected_widget.set(Some(widget_id));
                let variant = self.resolve_initial_variant(widget_id).or_else(|| {
                    bastyde_preview::find_by_id(widget_id)
                        .and_then(|e| e.variants().first().map(|v| v.name()))
                });
                state.selected_variant.set(variant);
            } else {
                state.select_first_registered();
            }
        }

        // If no widget at all is registered, render an empty-state
        // message so the previewer is still launchable in the
        // pre-vertical-slice scaffold.
        if bastyde_preview::iter_entries().next().is_none() {
            let empty = ctx.add(
                Center::new().child(
                    TextWidget::new(lit!(
                        "No widget catalog entries registered.\n\
                         Add `WidgetCatalog` impls and link with the `preview` feature."
                    ))
                    .style(bastyde_tokens::TextStyleRole::Body)
                    .color(bastyde_tokens::TextRole::Secondary),
                ),
            );
            self.root_id = Some(empty);
            return vec![empty];
        }

        let toolbar = build_toolbar(ctx, &state);
        let navigator = build_navigator(ctx, &state);
        let canvas = ctx.add(PreviewCanvas::new(state.clone()));
        let inspector = build_inspector(ctx, &state);

        // Three-pane split: navigator | canvas | inspector. The canvas
        // (stretch 1) absorbs window-resize slack; the side panes keep
        // their pixel widths.
        let layout = SplitterModel::from_panes(
            vec![
                PaneDescriptor::new().size(260.0).min_size(180.0).stretch(0.0),
                PaneDescriptor::new().min_size(360.0).stretch(1.0),
                PaneDescriptor::new().size(320.0).min_size(260.0).stretch(0.0),
            ],
            Orientation::Horizontal,
        );
        let outer_split_id = ctx.add(
            Splitter::new(layout)
                .pane_id(navigator)
                .pane_id(canvas)
                .pane_id(inspector),
        );

        // Wrap the split in `Expand::vertical()` so the
        // VStack gives it all remaining vertical space below the
        // toolbar — without this the split collapses to its minimum
        // and the whole previewer renders in <360 px height.
        let outer_split_expanded = ctx.add(Expand::vertical().child_id(outer_split_id));

        let root = VStack::new()
            .add_child(toolbar)
            .add_child(outer_split_expanded);
        let root_id = ctx.add(root);
        self.root_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
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
        match self.root_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Window);
        builder.set_name("Bastyde Widget Previewer");
    }
}
