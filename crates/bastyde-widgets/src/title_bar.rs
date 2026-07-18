// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Custom window title bar widget.
//!
//! `TitleBar` replaces a window's native chrome with a horizontal bar that
//! can host menus, tools, and the standard window controls (minimize /
//! maximize / close). The platform plumbing — beginning a window drag,
//! returning the right `WM_NCHITTEST` codes on Windows, repositioning the
//! macOS traffic lights — lives behind the
//! [`PlatformTitleBarHost`] trait in
//! `bastyde-platform`. The widget itself is platform-agnostic.
//!
//! Construct a `TitleBar` from inside the root-builder closure, fetching
//! the host from the widget tree:
//!
//! ```ignore
//! .root(|tree| {
//!     let host = tree.title_bar_host().expect("custom_chrome enabled");
//!     tree.add(
//!         VStack::new()
//!             .child(TitleBar::new(host)
//!                 .background(theme.colors.surface_raised)
//!                 .border(theme.colors.border, 1.0)
//!                 .leading(TextWidget::new(lit!("My App"))))
//!             .child(Expand::new().child(/* body */)))
//! })
//! ```

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::widget::{
    EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
    WidgetTreeView,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::{HitRegions, PlatformTitleBarHost, Signal};
use bastyde_tokens::{Color, CornerRadius};

use crate::primitives::{FixedSize, HStack};

mod controls;
mod drag_region;
mod resize_strip;
mod window_frame;
mod window_menu;

pub use controls::{ControlAction, ControlButton, WindowControls, WindowControlsLayout};
pub use drag_region::DragRegion;
pub use resize_strip::ResizeStrip;
pub use window_frame::WindowFrame;

/// Type alias for the user-supplied close action that overrides
/// `host.close()` (which on Wayland is currently a no-op due to winit 0.30
/// lacking `Window::request_close`). Set via [`TitleBar::close_action`].
pub type CloseAction = Rc<dyn Fn(&mut EventContext)>;

/// A custom window title bar.
///
/// Layout (left to right):
///
/// ```text
/// [leading inset] [leading slot] [drag region (flexible)] [trailing slot] [trailing inset] [window controls]
/// ```
///
/// The leading inset reserves space for the OS-drawn traffic lights on
/// macOS. The drag region is a `Spacer`-style flex
/// child that absorbs all leftover horizontal space and forwards
/// pointer / drag / double-tap gestures to the platform host. The window
/// controls (minimize / maximize / close) are rendered only when the host
/// advertises [`PlatformTitleBarHost::renders_custom_controls`] — i.e. on
/// Windows and Wayland but not on macOS.
pub struct TitleBar {
    host: Rc<dyn PlatformTitleBarHost>,
    leading: Option<PendingChild>,
    center: Option<PendingChild>,
    trailing: Option<PendingChild>,
    height: f32,
    background: ColorProp,
    border_color: ColorProp,
    border_width: f32,
    /// Optional override for the close button. When set, the close
    /// button invokes this closure instead of `ctx.close_window()`.
    close_action: Option<CloseAction>,
    root_child_id: Option<WidgetId>,
    /// `WidgetId` of the `DragRegion` we install. Memoised at build
    /// time so `after_paint` can read its bounds via `WidgetTreeView`
    /// without walking the subtree.
    drag_region_id: Cell<Option<WidgetId>>,
    /// Sink that the inner `WindowControls` populates with its
    /// per-button ids during build. `None` when no controls are
    /// rendered (macOS, where the OS draws traffic lights).
    controls_layout: Rc<Cell<Option<WindowControlsLayout>>>,
}

impl std::fmt::Debug for TitleBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TitleBar")
            .field("height", &self.height)
            .field("has_leading", &self.leading.is_some())
            .field("has_center", &self.center.is_some())
            .field("has_trailing", &self.trailing.is_some())
            .finish_non_exhaustive()
    }
}

impl TitleBar {
    /// Construct a `TitleBar` bound to the given platform host.
    ///
    /// The maximize/restore glyph follows `WindowState::placement` via
    /// `ctx.window()` at build time — the host no longer owns the
    /// maximize signal.
    pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self {
        Self {
            host,
            leading: None,
            center: None,
            trailing: None,
            height: 40.0,
            background: ColorProp::Static(Color::TRANSPARENT),
            border_color: ColorProp::Static(Color::TRANSPARENT),
            border_width: 0.0,
            close_action: None,
            root_child_id: None,
            drag_region_id: Cell::new(None),
            controls_layout: Rc::new(Cell::new(None)),
        }
    }

    /// Set the title bar's logical-pixel height. Default: 40.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Fill the title bar with a solid background color. Default:
    /// transparent (the window's clear color shows through).
    ///
    /// Accepts a `Color`, a `Signal<Color>`, or any of the role types
    /// (`SurfaceRole`, `TextRole`, `BorderRole`, or their `Signal<…>`
    /// variants). Role values resolve at paint time, so the title bar
    /// retints live across `ctx.set_theme(...)` switches.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = color.into();
        self
    }

    /// Draw a 1px-or-thicker bottom border separating the title bar from
    /// the body.
    ///
    /// Color accepts the same range as [`Self::background`]; pair with
    /// `BorderRole::Default` for a theme-tracking divider.
    pub fn border(mut self, color: impl Into<ColorProp>, width: f32) -> Self {
        self.border_color = color.into();
        self.border_width = width;
        self
    }

    /// Set the leading-edge content (e.g. app icon, menus). Rendered to the
    /// right of the macOS traffic-light inset.
    pub fn leading(mut self, widget: impl Widget + 'static) -> Self {
        self.leading = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Set the leading-edge content by pre-registered ID.
    pub fn leading_id(mut self, id: WidgetId) -> Self {
        self.leading = Some(PendingChild::Id(id));
        self
    }

    /// Set the center content (e.g. search box, breadcrumbs). Wrapped in a
    /// flexible drag region: clicks that are not consumed by the child
    /// initiate a window drag.
    pub fn center(mut self, widget: impl Widget + 'static) -> Self {
        self.center = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Set the center content by pre-registered ID.
    pub fn center_id(mut self, id: WidgetId) -> Self {
        self.center = Some(PendingChild::Id(id));
        self
    }

    /// Set the trailing-edge content (e.g. user avatar, notification bell).
    /// Rendered before the window controls.
    pub fn trailing(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Set the trailing-edge content by pre-registered ID.
    pub fn trailing_id(mut self, id: WidgetId) -> Self {
        self.trailing = Some(PendingChild::Id(id));
        self
    }

    /// Override the close-button action. When set, the close button calls
    /// this closure instead of `host.close()`. Required on Wayland where
    /// the host's `close()` is a no-op (winit 0.30 has no
    /// `Window::request_close`); the application typically wires this to
    /// call `EventContext::close_window` directly, or to send an
    /// `Intent` whose root-level `Action` handler calls it.
    pub fn close_action(mut self, action: impl Fn(&mut EventContext) + 'static) -> Self {
        self.close_action = Some(Rc::new(action));
        self
    }
}

impl Widget for TitleBar {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        // Register repaint-on-change for any signal-bearing ColorProp.
        // Static + role-only variants need no registration — `set_theme`
        // already mark-all-dirties the tree.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.background.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );
        self.border_color.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );

        let leading_inset = self.host.reserved_leading_inset();
        let trailing_inset = self.host.reserved_trailing_inset();
        let renders_controls = self.host.renders_custom_controls();
        let height = self.height;

        // The drag region is a spacer (claims all leftover horizontal space
        // in the HStack) that forwards drag / double-tap / right-click to
        // the host. Its child — if any — fills the spacer's full bounds.
        let drag_region = match self.center.take() {
            Some(PendingChild::Deferred(child)) => DragRegion::with_child(self.host.clone(), child),
            Some(PendingChild::Id(id)) => DragRegion::with_child_id(self.host.clone(), id),
            None => DragRegion::new(self.host.clone()),
        }
        // Only consulted when the platform has no OS window menu and the drag
        // region therefore builds its own (X11); see `title_bar/window_menu.rs`.
        .close_action(self.close_action.clone());
        let drag_region_id = ctx.add(drag_region);
        self.drag_region_id.set(Some(drag_region_id));

        // Derive the maximize signal from the hosting window's
        // `WindowState::placement`. When no state is attached (standalone
        // / tests) fall back to a static `false`.
        let is_maximized_signal = ctx
            .window()
            .map(|w| w.placement().map(|p| p.is_maximized()))
            .unwrap_or_else(|| Signal::new(false));
        let controls: Option<WindowControls> = if renders_controls {
            Some(
                WindowControls::new(
                    self.host.clone(),
                    is_maximized_signal,
                    self.close_action.clone(),
                )
                .layout_sink(self.controls_layout.clone()),
            )
        } else {
            None
        };

        // The leading and trailing slots arrive as `Box<dyn Widget>`, which
        // does not itself implement `Widget`, so we register them via
        // `BuildContext::add_boxed` first and then attach them by id. The
        // `add_child` and `child` calls on `HStack` push into the same
        // ordered pending list, so interleaving is safe.
        let mut row = HStack::new().spacing(0.0);

        if leading_inset.width > 0.0 {
            row = row.child(FixedSize::new().width(leading_inset.width).height(height));
        }

        if let Some(leading) = self.leading.take() {
            let id = match leading {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            row = row.add_child(id);
        }

        row = row.add_child(drag_region_id);

        if let Some(trailing) = self.trailing.take() {
            let id = match trailing {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            row = row.add_child(id);
        }

        if trailing_inset.width > 0.0 {
            row = row.child(FixedSize::new().width(trailing_inset.width).height(height));
        }

        if let Some(controls) = controls {
            row = row.child(controls);
        }

        let root = ctx.add(row);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Always claim the full width offered by the parent and the
        // configured fixed height. Ignoring the child HStack's natural
        // width is intentional: when the title bar is laid out by a
        // shrink-to-fit container the inner HStack would otherwise
        // collapse to the sum of its non-spacer children, leaving the
        // drag region with zero pixels.
        Size::new(proposal.width.unwrap_or(0.0), self.height).into()
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

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let bg = self.background.resolve(ctx.theme, ctx.effective_enabled);
        if bg.a() > 0.0 {
            canvas.fill_rounded_rect(bounds, CornerRadius::ZERO, bg);
        }
        if self.border_width > 0.0 {
            let border = self.border_color.resolve(ctx.theme, ctx.effective_enabled);
            if border.a() > 0.0 {
                canvas.draw_border_bottom(bounds, border, self.border_width);
            }
        }
    }

    fn wants_after_paint(&self) -> bool {
        // We aggregate descendant rects (drag region + min/max/close
        // buttons) into a single `HitRegions` payload for the host
        // every frame. The Windows backend reads it from
        // `WM_NCHITTEST`; Wayland and macOS backends ignore it.
        true
    }

    fn after_paint(&self, view: &WidgetTreeView<'_>, _ctx: &PaintContext) {
        // Build one complete `HitRegions` snapshot per frame. The
        // widget tree publishes logical-pixel rects (its native
        // coordinate system); platform backends that need physical
        // pixels (Windows) convert internally.
        let mut regions = HitRegions::new();

        if let Some(drag_id) = self.drag_region_id.get() {
            let drag_bounds = view.bounds(drag_id);
            // A zero-size drag bounds (host doesn't render controls,
            // tree not laid out yet, etc.) would still hit-test true
            // for any point at the origin — skip it.
            if drag_bounds.width > 0.0 && drag_bounds.height > 0.0 {
                regions.drag.push(drag_bounds);
                // Punch a hole for every `DeadZone` the app put inside the
                // `center` slot. On Windows the drag rect becomes `HTCAPTION`,
                // and the OS then owns those pixels outright — a button living
                // there would never see a click, a hover or a cursor change; it
                // would only drag the window. The dead-zone flag already means
                // "not draggable chrome" to widget-land's drag arming, so it is
                // the same declaration the OS needs. Wrap an interactive
                // title-bar control in a `DeadZone` and it works on both layers.
                collect_dead_zones(view, drag_id, drag_bounds, &mut regions.no_drag);
            }
        }

        if let Some(layout) = self.controls_layout.take() {
            regions.minimize = Some(view.bounds(layout.minimize_id));
            regions.minimize_id = Some(layout.minimize_id);

            // The maximize id is the Switcher's, not either glyph
            // button's. The Switcher's bounds are always valid (the
            // parent HStack lays it out regardless of which child is
            // visible); a synthetic tap at the Switcher center routes
            // through hit-testing to whichever child is currently
            // visible — handles the floating ↔ maximized swap without
            // the dormant-child-zero-bounds trap.
            regions.maximize = Some(view.bounds(layout.maximize_id));
            regions.maximize_id = Some(layout.maximize_id);

            regions.close = Some(view.bounds(layout.close_id));
            regions.close_id = Some(layout.close_id);

            // Restore the layout cell so we don't have to rebuild it
            // every frame.
            self.controls_layout.set(Some(layout));
        }

        self.host.update_hit_regions(&regions);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Banner);
        builder.set_name(bastyde_i18n::tr_widget!(a11y_title_bar_name()).resolve_now());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Depth-first walk of `root`'s descendants collecting the bounds of every
/// gesture dead zone, clipped to `clip` (the drag rect). A dead zone is not
/// descended into — its whole subtree is already inside its bounds.
///
/// Two nodes are deliberately skipped: dormant ones (a `Switcher`'s hidden page
/// keeps stale bounds), and anything that does not overlap the drag rect — an
/// *open* popover is an arena descendant of its trigger but hangs below the
/// title bar, and its rect must not be mistaken for a hole in the caption.
fn collect_dead_zones(view: &WidgetTreeView<'_>, root: WidgetId, clip: Rect, out: &mut Vec<Rect>) {
    for &child in view.children(root) {
        if !view.is_active(child) {
            continue;
        }
        let Some(hit) = intersect(view.bounds(child), clip) else {
            continue;
        };
        if view.is_gesture_dead_zone(child) {
            out.push(hit);
            continue;
        }
        collect_dead_zones(view, child, clip, out);
    }
}

/// Overlap of two rects, or `None` when they do not overlap — `Rect` has
/// `contains` but no intersection helper. Guards against publishing a
/// degenerate (zero-area) exclusion rect.
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{DeadZone, Expand};
    use bastyde_canvas::Point;
    use bastyde_core::event::PointerButton;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_core::{HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge};
    use std::cell::{Cell, RefCell};

    /// A test host that records calls. Pretends the platform supports
    /// custom controls (`renders_custom_controls = true`) and reports
    /// zero macOS traffic-light insets.
    struct TestHost {
        minimized: Cell<u32>,
        maximize_toggled: Cell<u32>,
        closed: Cell<u32>,
        drags_started: Cell<u32>,
        is_max: Signal<bool>,
        /// Last snapshot handed to `update_hit_regions` — what a real
        /// platform backend would hit-test against.
        last_regions: RefCell<HitRegions>,
    }

    impl Default for TestHost {
        fn default() -> Self {
            Self {
                minimized: Cell::new(0),
                maximize_toggled: Cell::new(0),
                closed: Cell::new(0),
                drags_started: Cell::new(0),
                is_max: Signal::new(false),
                last_regions: RefCell::new(HitRegions::default()),
            }
        }
    }

    impl PlatformTitleBarHost for TestHost {
        fn reserved_leading_inset(&self) -> Size {
            Size::ZERO
        }
        fn reserved_trailing_inset(&self) -> Size {
            Size::ZERO
        }
        fn renders_custom_controls(&self) -> bool {
            true
        }
        fn needs_custom_resize_handles(&self) -> bool {
            true
        }
        fn begin_drag(&self) -> Result<(), PlatformError> {
            self.drags_started.set(self.drags_started.get() + 1);
            Ok(())
        }
        fn begin_resize(&self, _edge: ResizeEdge) -> Result<(), PlatformError> {
            Ok(())
        }
        fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
            Ok(())
        }
        fn update_hit_regions(&self, regions: &HitRegions) {
            *self.last_regions.borrow_mut() = regions.clone();
        }
    }

    /// Build a tree where the title bar is wrapped in the same VStack +
    /// Expand body shape the demo uses. Returns the laid-out tree plus the
    /// title-bar widget id.
    fn build_realistic_tree(
        host: Rc<TestHost>,
        bar_setup: impl FnOnce(TitleBar) -> TitleBar,
    ) -> (WidgetTree, WidgetId) {
        use crate::primitives::{Expand, VStack};

        let bar_widget =
            bar_setup(TitleBar::new(host as Rc<dyn PlatformTitleBarHost>).height(40.0));

        // A theme + text backend so `render()` (and with it the `after_paint`
        // pass that publishes `HitRegions`) can run: the control buttons carry
        // glyphs, which need a typesetter.
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let bar_id = tree.add(bar_widget);
        let body_id = tree.add(Expand::new());
        let _root = tree.add(
            VStack::new()
                .spacing(0.0)
                .add_child(bar_id)
                .add_child(body_id),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));
        (tree, bar_id)
    }

    /// Walk the title bar tree to find the trio of control button ids in
    /// order: minimize, maximize, close. Layout-shape-aware — if the build
    /// changes shape this test will tell us by panicking with a helpful
    /// debug print of the children at each level.
    ///
    /// The maximize slot is a `Switcher` whose two pages (`□` normal and
    /// `❐` zoomed) are pre-mounted ControlButtons handed in via
    /// `child_id`. With Switcher's lazy-mount semantics, `PreMounted`
    /// entries become `Mounted` eagerly on first build, so the Switcher
    /// reports both pages as direct children — this helper picks the
    /// first (normal-state) since `TestHost::default()` reports
    /// `is_maximized = false`.
    fn locate_control_buttons(tree: &WidgetTree, bar: WidgetId) -> [WidgetId; 3] {
        // bar -> [HStack root]
        let bar_kids = tree.children(bar);
        assert_eq!(
            bar_kids.len(),
            1,
            "TitleBar should have a single root: {bar_kids:?}"
        );
        let row = bar_kids[0];

        // row -> [DragRegion (spacer), WindowControls]
        let row_kids = tree.children(row);
        assert_eq!(
            row_kids.len(),
            2,
            "row should have drag_region + controls, got {row_kids:?}"
        );
        let controls = row_kids[1];

        // controls -> [inner HStack]
        let controls_kids = tree.children(controls);
        assert_eq!(controls_kids.len(), 1, "controls should wrap one HStack");
        let inner_row = controls_kids[0];

        // inner_row -> [minimize, max_switcher, close]
        let inner_kids = tree.children(inner_row);
        assert_eq!(
            inner_kids.len(),
            3,
            "inner controls row should contain 3 items, got {inner_kids:?}"
        );
        // Switcher's direct children are its mounted pages — both
        // pre-mounted ControlButtons (□ normal + ❐ zoomed) in
        // declaration order.
        let max_buttons = tree.children(inner_kids[1]);
        assert_eq!(
            max_buttons.len(),
            2,
            "maximize Switcher should expose 2 ControlButtons (□ + ❐), got {max_buttons:?}"
        );
        [inner_kids[0], max_buttons[0], inner_kids[2]]
    }

    #[test]
    fn title_bar_claims_full_width_and_configured_height() {
        let host = Rc::new(TestHost::default());
        let (tree, bar) = build_realistic_tree(host, |b| b);
        let b = tree.bounds(bar);
        assert!((b.width - 900.0).abs() < 0.01, "width = {}", b.width);
        assert!((b.height - 40.0).abs() < 0.01, "height = {}", b.height);
    }

    #[test]
    fn drag_region_is_a_spacer_so_controls_sit_flush_right() {
        // Regression: in the first M2 cut DragRegion was not a spacer and
        // collapsed to zero width, leaving the buttons clustered next to
        // the leading text instead of at the trailing edge.
        let host = Rc::new(TestHost::default());
        let (tree, bar) = build_realistic_tree(host, |b| b);

        let [_minimize, _maximize, close] = locate_control_buttons(&tree, bar);
        let close_b = tree.bounds(close);

        // 46 px wide cell, three of them, flush right against the 900 px
        // window edge → close button right edge ≈ 900, left edge ≈ 854.
        assert!(
            (close_b.right() - 900.0).abs() < 1.0,
            "close right edge = {}, expected ~900",
            close_b.right()
        );
        assert!(
            (close_b.width - 46.0).abs() < 1.0,
            "close cell width = {}, expected 46",
            close_b.width
        );
    }

    #[test]
    fn close_action_override_is_invoked_instead_of_host_close() {
        let host = Rc::new(TestHost::default());
        let close_calls = Rc::new(Cell::new(0u32));
        let close_calls_clone = close_calls.clone();

        let host_clone = host.clone();
        let (mut tree, bar) = build_realistic_tree(host_clone, move |b| {
            b.close_action(move |_ctx| {
                close_calls_clone.set(close_calls_clone.get() + 1);
            })
        });

        let [_min, _max, close] = locate_control_buttons(&tree, bar);
        tree.click(close);

        assert!(
            close_calls.get() >= 1,
            "close_action should have been called, got {}",
            close_calls.get()
        );
        assert_eq!(
            host.closed.get(),
            0,
            "host.close() must NOT be called when close_action override is set"
        );
    }

    /// Attach a fresh `WindowState` to the tree so the title bar's
    /// maximize/minimize actions have a target. Returns the state so
    /// the test can assert against `placement().get()` after an action.
    fn attach_window_state(tree: &mut WidgetTree) -> bastyde_core::WindowState {
        let state = bastyde_core::WindowState::new(bastyde_core::WindowStateInit {
            id: bastyde_core::BastydeWindowId::new(1),
            string_id: None,
            placement: bastyde_core::WindowPlacement::Floating,
            title: "Test".to_string(),
            size: (800, 600),
            position: (0, 0),
            focused: true,
            resizable: true,
            always_on_top: false,
        });
        tree.set_window_state(state.clone());
        state
    }

    #[test]
    fn minimize_button_sets_placement_to_minimized() {
        let host = Rc::new(TestHost::default());
        let (mut tree, bar) = build_realistic_tree(host.clone(), |b| b);
        let state = attach_window_state(&mut tree);

        let [minimize, _max, _close] = locate_control_buttons(&tree, bar);
        tree.click(minimize);

        assert_eq!(
            state.placement().get(),
            bastyde_core::WindowPlacement::Minimized,
            "minimize button should flip WindowState::placement to Minimized"
        );
    }

    #[test]
    fn maximize_button_toggles_placement() {
        let host = Rc::new(TestHost::default());
        let (mut tree, bar) = build_realistic_tree(host.clone(), |b| b);
        let state = attach_window_state(&mut tree);

        let [_min, maximize, _close] = locate_control_buttons(&tree, bar);
        tree.click(maximize);
        assert_eq!(
            state.placement().get(),
            bastyde_core::WindowPlacement::Maximized
        );

        tree.click(maximize);
        assert_eq!(
            state.placement().get(),
            bastyde_core::WindowPlacement::Floating
        );
    }

    /// Each control button advertises `Action::Click` — on macOS that is
    /// precisely what makes VoiceOver offer a press (`is_clickable` ==
    /// `supports_action(Click)`). Invoking it must actually drive the
    /// window, or a screen-reader user cannot minimize / maximize /
    /// close the window at all.
    #[test]
    fn access_click_drives_window_controls() {
        let host = Rc::new(TestHost::default());
        let (mut tree, bar) = build_realistic_tree(host.clone(), |b| b);
        let state = attach_window_state(&mut tree);

        let [minimize, maximize, _close] = locate_control_buttons(&tree, bar);

        let at_click = |tree: &mut WidgetTree, id: WidgetId| {
            tree.dispatch_event(bastyde_core::event::WidgetEvent::AccessAction {
                action: bastyde_core::accesskit::Action::Click,
                target: Some(id),
                target_node: bastyde_core::accessibility::root_node_id(),
                data: None,
            });
        };

        at_click(&mut tree, minimize);
        assert_eq!(
            state.placement().get(),
            bastyde_core::WindowPlacement::Minimized,
            "AT click on minimize must flip placement to Minimized"
        );

        state
            .placement()
            .set(bastyde_core::WindowPlacement::Floating);
        at_click(&mut tree, maximize);
        assert_eq!(
            state.placement().get(),
            bastyde_core::WindowPlacement::Maximized,
            "AT click on maximize must flip placement to Maximized"
        );
    }

    /// The close button's AT click must run the same action a pointer tap
    /// does — including a `close_action` override.
    #[test]
    fn access_click_invokes_close_action() {
        let host = Rc::new(TestHost::default());
        let close_calls = Rc::new(Cell::new(0u32));
        let close_calls_clone = close_calls.clone();

        let (mut tree, bar) = build_realistic_tree(host.clone(), move |b| {
            b.close_action(move |_ctx| {
                close_calls_clone.set(close_calls_clone.get() + 1);
            })
        });

        let [_min, _max, close] = locate_control_buttons(&tree, bar);
        tree.dispatch_event(bastyde_core::event::WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(close),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });

        assert_eq!(
            close_calls.get(),
            1,
            "AT click on close must invoke the close action"
        );
    }

    /// Locate the `DragRegion` widget id by walking the title-bar subtree.
    /// Path: bar → HStack root → [drag_region, controls].
    fn locate_drag_region(tree: &WidgetTree, bar: WidgetId) -> WidgetId {
        let bar_kids = tree.children(bar);
        let row = bar_kids[0];
        let row_kids = tree.children(row);
        row_kids[0]
    }

    #[test]
    fn dragging_inside_drag_region_calls_host_begin_drag() {
        // Regression for: in M2 the gesture-arena auto-wiring in bastyde-core
        // only built a TapRecognizer when on_tap was set. DragRegion uses
        // on_drag (no on_tap) and so was getting no arena at all → drag
        // never fired. The fix in event_dispatch_impl::ensure_gesture_arena
        // installs DragRecognizer whenever on_drag is set.
        let host = Rc::new(TestHost::default());
        let (mut tree, bar) = build_realistic_tree(host.clone(), |b| b);

        let drag = locate_drag_region(&tree, bar);
        let drag_b = tree.bounds(drag);
        let from = Point::new(drag_b.x + 50.0, drag_b.y + drag_b.height / 2.0);
        let to = Point::new(drag_b.x + 200.0, drag_b.y + drag_b.height / 2.0);

        tree.drag(from, to);

        assert!(
            host.drags_started.get() >= 1,
            "host.begin_drag() should be called on drag-start, got {}",
            host.drags_started.get()
        );
    }

    #[test]
    fn title_bar_exposes_banner_landmark() {
        let host = Rc::new(TestHost::default());
        let (tree, bar) = build_realistic_tree(host, |b| b);
        let info = tree.accessibility_node(bar);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Banner);
        assert!(
            info.name().is_some(),
            "TitleBar Banner landmark should have a localised name"
        );
    }

    #[test]
    fn window_control_glyphs_retint_on_theme_switch() {
        // Regression: `WindowControls` froze `text_primary` / `surface_hover`
        // / `status_error_bg` into `Color` snapshots at build time and relied
        // on `mark_all_dirty` to "follow the theme" — but a static `Color` is
        // a `ColorProp::Static` that always re-resolves to the same value, so
        // the min/max/close glyphs kept the build-time theme's color after a
        // `set_theme`. The fix hands the buttons `TextRole::Primary` /
        // `SurfaceRole::*`, which resolve against the live theme at paint
        // time. This test renders the control glyphs under light then dark
        // and asserts they actually change color.
        use crate::primitives::{Expand, VStack};
        use bastyde_canvas::MockTextBackend;
        use std::cell::RefCell;

        let host = Rc::new(TestHost::default());
        let bar_widget = TitleBar::new(host as Rc<dyn PlatformTitleBarHost>).height(40.0);

        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let bar_id = tree.add(bar_widget);
        let body_id = tree.add(Expand::new());
        tree.add(
            VStack::new()
                .spacing(0.0)
                .add_child(bar_id)
                .add_child(body_id),
        );

        tree.layout(SizeProposal::exact(900.0, 600.0));
        let light_glyphs: Vec<[f32; 4]> = tree.render().glyphs.iter().map(|g| g.color).collect();
        assert!(
            !light_glyphs.is_empty(),
            "control glyphs (—, □, ×) should have rendered"
        );

        // Every control glyph uses TextRole::Primary; under light it must
        // resolve to the light theme's primary text color.
        let light_primary = bastyde_core::presets::intui::light()
            .colors
            .text_primary
            .to_array();
        assert!(
            light_glyphs.iter().all(|c| *c == light_primary),
            "control glyphs should paint with the light theme's text_primary, got {light_glyphs:?}"
        );

        tree.set_theme(bastyde_core::presets::intui::dark());
        tree.layout(SizeProposal::exact(900.0, 600.0));
        let dark_glyphs: Vec<[f32; 4]> = tree.render().glyphs.iter().map(|g| g.color).collect();

        let dark_primary = bastyde_core::presets::intui::dark()
            .colors
            .text_primary
            .to_array();
        assert!(
            dark_glyphs.iter().all(|c| *c == dark_primary),
            "control glyphs should retint to the dark theme's text_primary, got {dark_glyphs:?}"
        );
        assert_ne!(
            light_glyphs, dark_glyphs,
            "control glyph colors must change across a theme switch"
        );
    }

    #[test]
    fn window_controls_have_semantic_names_not_glyphs() {
        let host = Rc::new(TestHost::default());
        let (tree, bar) = build_realistic_tree(host, |b| b);
        let [minimize, maximize, close] = locate_control_buttons(&tree, bar);

        let min_info = tree.accessibility_node(minimize);
        let max_info = tree.accessibility_node(maximize);
        let close_info = tree.accessibility_node(close);

        // Screen readers must get a semantic verb, not the raw glyph
        // character (`—`, `□`, `×`) which Unicode-aware AT pronounces
        // as "em dash" / "white square" / "multiplication sign".
        for info in [&min_info, &max_info, &close_info] {
            let name = info.name().expect("control button must have a name");
            assert!(!name.is_empty(), "name empty");
            assert_ne!(name, "\u{2014}", "minimize reads glyph literal");
            assert_ne!(name, "\u{25A1}", "maximize reads glyph literal");
            assert_ne!(name, "\u{00D7}", "close reads glyph literal");
            assert_eq!(info.role(), bastyde_core::accesskit::Role::Button);
        }
    }

    #[test]
    fn drag_region_is_hidden_from_a11y() {
        let host = Rc::new(TestHost::default());
        let (tree, bar) = build_realistic_tree(host, |b| b);
        let drag = locate_drag_region(&tree, bar);
        let info = tree.accessibility_node(drag);
        assert!(
            info.is_hidden(),
            "DragRegion is pointer-only; should be hidden from AT"
        );
    }

    /// Render one frame so `after_paint` runs and the host receives a
    /// `HitRegions` snapshot. `WidgetTree::render` drives the paint pass.
    fn paint_once(tree: &mut WidgetTree) {
        tree.layout(SizeProposal::exact(900.0, 600.0));
        let _ = tree.render();
    }

    #[test]
    fn dead_zone_in_center_is_published_as_a_no_drag_hole() {
        // Regression (Windows): the whole `center` slot is wrapped in a
        // DragRegion whose rect goes out as `HitRegions::drag`, which the
        // Windows backend answers with HTCAPTION. An interactive control
        // living there was therefore unclickable — the OS took the press and
        // started a window move instead. Wrapping it in a `DeadZone` must now
        // punch a hole in the caption so the OS hands the pixels back.

        let host = Rc::new(TestHost::default());
        let host_for_bar = host.clone();
        let (mut tree, _bar) = build_realistic_tree(host_for_bar, |b| {
            b.center(
                HStack::new()
                    .child(DeadZone::new().child(FixedSize::new().width(60.0).height(30.0)))
                    .child(Expand::new()),
            )
        });
        paint_once(&mut tree);

        let regions = host.last_regions.borrow();
        assert_eq!(
            regions.drag.len(),
            1,
            "the drag region should still be published"
        );
        assert_eq!(
            regions.no_drag.len(),
            1,
            "the DeadZone in `center` must be published as one no_drag hole, got {:?}",
            regions.no_drag
        );
        let hole = regions.no_drag[0];
        let drag = regions.drag[0];
        assert!(
            (hole.width - 60.0).abs() < 1.0,
            "the hole should match the dead zone's width, got {}",
            hole.width
        );
        // The hole must lie inside the caption it is carving out of, or the
        // Windows backend would test it against a region that never matches.
        assert!(
            hole.x >= drag.x - 0.01 && hole.right() <= drag.right() + 0.01,
            "hole {hole:?} must be clipped to the drag rect {drag:?}"
        );
    }

    #[test]
    fn passive_center_content_punches_no_hole() {
        // The inverse guard: a plain centred title must NOT become a no_drag
        // hole, or the user could no longer drag the window by its title —
        // which is the drag region's entire purpose.
        let host = Rc::new(TestHost::default());
        let host_for_bar = host.clone();
        let (mut tree, _bar) = build_realistic_tree(host_for_bar, |b| {
            b.center(crate::TextWidget::new(bastyde_i18n::lit!("My App")))
        });
        paint_once(&mut tree);

        let regions = host.last_regions.borrow();
        assert_eq!(regions.drag.len(), 1, "drag region still published");
        assert!(
            regions.no_drag.is_empty(),
            "a passive centred title must not punch a hole in the caption, got {:?}",
            regions.no_drag
        );
    }

    #[test]
    fn dead_zone_in_center_does_not_arm_the_window_drag() {
        // The widget-land half of the same bug, live on every platform: a
        // press on a control inside the drag region armed the DragRegion's
        // `on_drag` via `arm_drag_observers`, so a few px of pointer jitter
        // during an ordinary click started a window move and ate the tap.
        // The `DeadZone` boundary must stop that arming.

        let host = Rc::new(TestHost::default());
        let host_for_bar = host.clone();
        let (mut tree, _bar) = build_realistic_tree(host_for_bar, |b| {
            b.center(
                HStack::new()
                    .child(DeadZone::new().child(FixedSize::new().width(60.0).height(30.0)))
                    .child(Expand::new()),
            )
        });
        paint_once(&mut tree);

        let hole = host.last_regions.borrow().no_drag[0];
        let (cx, cy) = (hole.x + hole.width / 2.0, hole.y + hole.height / 2.0);

        // A jittery press on the dead-zoned control.
        tree.pointer_down_button(Point::new(cx, cy), PointerButton::Primary);
        for i in 1..=10 {
            tree.pointer_move(Point::new(cx + (i as f32) * 3.0, cy + 1.0));
        }
        tree.pointer_up_button(Point::new(cx + 30.0, cy + 1.0), PointerButton::Primary);

        assert_eq!(
            host.drags_started.get(),
            0,
            "a jittery click on a DeadZone inside the title bar must not drag the window"
        );
    }

    #[test]
    fn dragging_the_bare_drag_region_still_works_with_a_dead_zone_present() {
        // Guard the fix's blast radius: punching a hole must not disable the
        // drag surface around it.

        let host = Rc::new(TestHost::default());
        let host_for_bar = host.clone();
        let (mut tree, bar) = build_realistic_tree(host_for_bar, |b| {
            b.center(
                HStack::new()
                    .child(DeadZone::new().child(FixedSize::new().width(60.0).height(30.0)))
                    .child(Expand::new()),
            )
        });
        paint_once(&mut tree);

        // Drag from well to the right of the dead zone — still bare caption.
        let drag_b = tree.bounds(locate_drag_region(&tree, bar));
        let from = Point::new(drag_b.right() - 40.0, drag_b.y + drag_b.height / 2.0);
        let to = Point::new(drag_b.right() - 200.0, drag_b.y + drag_b.height / 2.0);
        tree.drag(from, to);

        assert!(
            host.drags_started.get() >= 1,
            "the drag region outside the hole must still move the window"
        );
    }

    #[test]
    fn double_clicking_drag_region_toggles_placement() {
        // Regression for: same auto-wiring gap. on_double_tap was wired in
        // HandlerSet but the dispatch never installed a DoubleTapRecognizer
        // unless on_tap was also set, so the handler was unreachable.
        let host = Rc::new(TestHost::default());
        let (mut tree, bar) = build_realistic_tree(host.clone(), |b| b);
        let state = attach_window_state(&mut tree);

        let drag = locate_drag_region(&tree, bar);
        tree.click(drag);
        tree.click(drag);

        assert_eq!(
            state.placement().get(),
            bastyde_core::WindowPlacement::Maximized,
            "double-tap on drag region should flip WindowState::placement to Maximized"
        );
    }
}
