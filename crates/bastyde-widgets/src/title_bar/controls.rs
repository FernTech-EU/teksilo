// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The minimize / maximize / close button cluster on the trailing edge of
//! a `TitleBar`. Rendered only when
//! [`PlatformTitleBarHost::renders_custom_controls`] is `true`
//! (Windows + Wayland; never on macOS).
//!
//! These are deliberately NOT built on top of the regular `Button` widget:
//! `Button` carries a 72 dp minimum width, themed padding, focus ring and
//! border, none of which are appropriate for a flush-fitting Win11-style
//! window control. Instead, each control is a small composing widget
//! [`ControlButton`] built from primitives (FixedSize + ZStack +
//! RectWidget + Center + TextWidget) so we inherit centering, theming and
//! reactive hover for free.
//!
//! For M2 the maximize/restore swap is *not* implemented — the maximize
//! button always shows the `□` glyph. M3+ will add a `Signal<bool>`-driven
//! glyph swap once the host can update it from `WindowEvent::Resized`.

use bastyde_i18n::lit;
use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::PlatformTitleBarHost;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{SurfaceRole, TextRole, TextStyleRole};

use crate::primitives::{Center, FixedSize, HStack, RectWidget, Switcher, TextWidget, ZStack};
use crate::title_bar::CloseAction;

/// Layout snapshot a `WindowControls` exports back to its parent
/// `TitleBar` so the parent's `after_paint` aggregator can read the
/// per-button `WidgetId`s. Populated during `WindowControls::build`.
///
/// The maximize slot is the **Switcher** that wraps the two glyph
/// buttons (`□` / `❐`), not either child directly: the inactive
/// Switcher child is dormant and has `Rect::ZERO` bounds, but the
/// Switcher container itself is always laid out by the parent
/// HStack and has valid bounds. A synthetic tap dispatched at the
/// Switcher's bounds-center routes through hit-testing to whichever
/// child is currently visible.
#[derive(Debug, Clone)]
pub struct WindowControlsLayout {
    pub minimize_id: WidgetId,
    pub maximize_id: WidgetId,
    pub close_id: WidgetId,
}

/// Action invoked when a [`ControlButton`] is tapped.
pub type ControlAction = Rc<dyn Fn(&mut EventContext)>;

/// A compact, flush-fitting window-control button.
///
/// Composes existing primitives — a `FixedSize` cell wrapping a `ZStack`
/// of (hover background, centred glyph). Hover state is tracked in a
/// `Signal<bool>` that drives a derived `Signal<SurfaceRole>` background,
/// so a hover change repaints with no relayout. Both the glyph color
/// (`fg`) and the hover surface are stored as *roles* (`ColorProp` /
/// `SurfaceRole`) that resolve against the current theme at paint time —
/// so the cluster retints live across `ctx.set_theme(...)` without a
/// rebuild.
pub struct ControlButton {
    glyph: &'static str,
    width: f32,
    height: f32,
    fg: ColorProp,
    /// Surface role painted over the title bar when the cursor is
    /// inside the cell. `SurfaceRole::Transparent` keeps the cell flat.
    hover_role: SurfaceRole,
    action: Option<ControlAction>,
    /// Accessible name exposed to AT. Reactive so `WindowControls` can
    /// flip it between "Maximize" and "Restore" without rebuilding.
    a11y_name: Signal<String>,
    /// External hover input — the Windows host writes this when the
    /// OS reports `WM_NCMOUSEMOVE` over the button rect (the OS owns
    /// non-client hover events, so the widget's own `on_hover`
    /// handler never fires for those pixels). Wired through an effect
    /// that drives `bg_signal` so the visual hover state is identical
    /// to widget-tree-driven hover. `None` means no external feed —
    /// only the widget's internal hover handler runs.
    external_hover: Option<Signal<bool>>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for ControlButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlButton")
            .field("glyph", &self.glyph)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl ControlButton {
    pub fn new(glyph: &'static str, width: f32, height: f32, fg: impl Into<ColorProp>) -> Self {
        Self {
            glyph,
            width,
            height,
            fg: fg.into(),
            hover_role: SurfaceRole::Transparent,
            action: None,
            a11y_name: Signal::new(String::new()),
            external_hover: None,
            root_child_id: None,
        }
    }

    /// Bind an external boolean hover input. The Windows backend
    /// writes this signal on `WM_NCMOUSEMOVE` / `WM_NCMOUSELEAVE`
    /// over the button rect, since those events never reach the
    /// widget tree (the OS treats the area as non-client). `build`
    /// installs an effect that maps the bool to the `bg_signal`
    /// colour identically to the internal hover handler.
    pub(crate) fn bind_external_hover(mut self, signal: Signal<bool>) -> Self {
        self.external_hover = Some(signal);
        self
    }

    pub fn hover_background(mut self, role: SurfaceRole) -> Self {
        self.hover_role = role;
        self
    }

    pub fn on_tap(mut self, action: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Rc::new(action));
        self
    }

    fn with_action(mut self, action: ControlAction) -> Self {
        self.action = Some(action);
        self
    }

    /// Bind the accessible name read by AT when this button's a11y node
    /// is queried. The glyph text drawn in the cell is purely visual and
    /// is hidden from AT — assistive users get this name instead.
    pub(crate) fn bind_a11y_name(mut self, name: Signal<String>) -> Self {
        self.a11y_name = name;
        self
    }
}

impl Widget for ControlButton {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        // Reactive hover background: a `Signal<bool>` tracks whether the
        // pointer is inside the cell, and a derived `Signal<SurfaceRole>`
        // maps it to the hover role (while inside) or
        // `SurfaceRole::Transparent` (flat). Driving the RectWidget with a
        // *role* signal — rather than a resolved `Color` — means the hover
        // fill resolves against the live theme at paint time, so it
        // retints across `set_theme` as well as repainting on hover.
        let hovered = ctx.signal(false);
        let hover_role = self.hover_role;
        let bg_role = hovered.map(move |inside| {
            if *inside {
                hover_role
            } else {
                SurfaceRole::Transparent
            }
        });

        let bg_rect = ctx.add(RectWidget::new().background(bg_role));

        let glyph_text = TextWidget::new(lit!(self.glyph))
            .style(TextStyleRole::Body)
            .color(self.fg.clone())
            .single_line()
            .a11y_hidden();
        let centred_glyph = ctx.add(Center::new().child(glyph_text));

        let stack = ctx.add(ZStack::new().add_child(bg_rect).add_child(centred_glyph));
        let sized = ctx.add(
            FixedSize::new()
                .bind_width(self.width)
                .bind_height(self.height)
                .child_id(stack),
        );

        // Self handlers: tap fires the action, hover drives the `hovered`
        // bool (which the derived role signal above reacts to).
        let hovered_handler = hovered.clone();
        let mut handlers =
            HandlerSet::new()
                .cursor(CursorIcon::Pointer)
                .on_hover(move |entered, _ctx| {
                    hovered_handler.set(entered);
                });

        if let Some(action) = self.action.take() {
            handlers = handlers.on_tap(move |_pos, ctx| action(ctx));
        }

        ctx.apply_self_handlers(handlers);

        // External hover feed (Windows non-client hover): write the same
        // `hovered` bool the internal handler writes, so OS-driven hover
        // renders identically to widget-tree-driven hover. The effect
        // handle is owned by the BuildContext so it lives as long as the
        // widget node.
        if let Some(ext) = self.external_hover.take() {
            let hovered_ext = hovered.clone();
            ctx.effect(&ext, move |entered| {
                hovered_ext.set(*entered);
            });
        }

        // Refresh the a11y node whenever the name signal changes
        // (maximize ⇄ restore toggle).
        let self_id = ctx.self_id();
        self.a11y_name.bind_to(
            self_id,
            ctx.binding_registry(),
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );

        self.root_child_id = Some(sized);
        vec![sized]
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Always exactly the configured cell. Returning the proposal here
        // would let an HStack stretch us to the leftover width.
        Size::new(self.width, self.height).into()
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

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Button);
        let name = self.a11y_name.get();
        if !name.is_empty() {
            builder.set_name(name);
        }
        builder.add_action(bastyde_core::accesskit::Action::Click);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// The minimize / maximize / close cluster, laid out as an HStack of
/// [`ControlButton`]s. Each cell forwards taps to the supplied host.
pub struct WindowControls {
    host: Rc<dyn PlatformTitleBarHost>,
    is_maximized: Signal<bool>,
    /// User-supplied override for the close action — see
    /// [`crate::title_bar::TitleBar::close_action`].
    close_action: Option<CloseAction>,
    root_child_id: Option<WidgetId>,
    /// Sink the parent `TitleBar` shares with us so its
    /// `after_paint` aggregator can read our per-button `WidgetId`s.
    /// `None` when the controls are used standalone (tests / docs).
    layout_sink: Option<Rc<Cell<Option<WindowControlsLayout>>>>,
}

impl std::fmt::Debug for WindowControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowControls").finish_non_exhaustive()
    }
}

impl WindowControls {
    pub fn new(
        host: Rc<dyn PlatformTitleBarHost>,
        is_maximized: Signal<bool>,
        close_action: Option<CloseAction>,
    ) -> Self {
        Self {
            host,
            is_maximized,
            close_action,
            root_child_id: None,
            layout_sink: None,
        }
    }

    /// Wire a sink the parent `TitleBar` will read from in its
    /// `after_paint` hook. The sink receives a [`WindowControlsLayout`]
    /// snapshot during this widget's `build` pass.
    pub(crate) fn layout_sink(mut self, sink: Rc<Cell<Option<WindowControlsLayout>>>) -> Self {
        self.layout_sink = Some(sink);
        self
    }
}

impl Widget for WindowControls {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        // Hand the buttons *roles*, not a frozen `theme.colors.*` snapshot.
        // A resolved `Color` is a `ColorProp::Static` that `mark_all_dirty`
        // re-resolves to the same value, so a build-time snapshot would
        // freeze the glyph/hover colors at whatever theme was active when
        // the tree was built — they would not retint on `set_theme`. Roles
        // resolve against the current theme at paint time, so the cluster
        // follows light ↔ dark live without a rebuild.
        let fg = TextRole::Primary;
        let hover_bg = SurfaceRole::Hover;
        let close_hover = SurfaceRole::StatusError;

        // Win11-style cell: 46 dp wide × 32 dp tall fits comfortably into a
        // 40 dp title bar. Height here is the cell's natural size; the
        // final placed height is driven by the parent HStack's bounds.
        let cell_w = 46.0;
        let cell_h = 32.0;

        let close_override = self.close_action.clone();

        // All three controls write through `WindowState::placement` and
        // `WindowState::close` now. The signal flip fires the state's
        // observer which queues a `WindowCommand`; the app-level manager
        // translates that into the appropriate winit call on the next
        // tick. OS-initiated state changes (green-light zoom, drag-to-
        // top-snap) come back through `set_placement_from_os`, keeping
        // the button glyph in sync without echoing back out.
        let minimize_action: ControlAction = Rc::new(move |ctx| {
            if let Some(w) = ctx.window() {
                w.placement().set(bastyde_core::WindowPlacement::Minimized);
            }
        });
        let maximize_action: ControlAction = Rc::new(move |ctx| {
            if let Some(w) = ctx.window() {
                let next = if w.placement().get().is_maximized() {
                    bastyde_core::WindowPlacement::Floating
                } else {
                    bastyde_core::WindowPlacement::Maximized
                };
                w.placement().set(next);
            }
        });
        let close_action: ControlAction = match close_override {
            Some(user_action) => user_action,
            None => Rc::new(move |ctx| ctx.close_window()),
        };

        // `to_signal()` observes the i18n manager so the name updates
        // when `tree.set_locale(...)` is called; `resolve_now()` would
        // freeze the English string at build time.
        let minimize_name = bastyde_i18n::tr_widget!(a11y_window_minimize_name()).to_signal();
        let close_name = bastyde_i18n::tr_widget!(a11y_window_close_name()).to_signal();
        let maximize_name = bastyde_i18n::tr_widget!(a11y_window_maximize_name()).to_signal();
        let restore_name = bastyde_i18n::tr_widget!(a11y_window_restore_name()).to_signal();

        // Per-button hover signals for the Windows custom-chrome
        // path. The host writes them on `WM_NCMOUSEMOVE` /
        // `WM_NCMOUSELEAVE` over the matching button rect; the
        // button's effect maps the bool to its visual `bg_signal`.
        // On Wayland and macOS the host's `register_hover_signal` is
        // a no-op, so these are never written from outside — the
        // buttons fall back to their internal `on_hover` handler.
        let minimize_hover = Signal::new(false);
        let maximize_hover = Signal::new(false);
        let close_hover_signal = Signal::new(false);
        self.host.register_hover_signal(
            bastyde_core::ControlTarget::Minimize,
            minimize_hover.clone(),
        );
        self.host.register_hover_signal(
            bastyde_core::ControlTarget::Maximize,
            maximize_hover.clone(),
        );
        self.host.register_hover_signal(
            bastyde_core::ControlTarget::Close,
            close_hover_signal.clone(),
        );

        let minimize = ControlButton::new("\u{2014}", cell_w, cell_h, fg)
            .hover_background(hover_bg)
            .with_action(minimize_action)
            .bind_a11y_name(minimize_name)
            .bind_external_hover(minimize_hover);
        let minimize_id = ctx.add(minimize);

        // Maximize/restore: both states use `□` (U+25A1). The
        // semantically nicer "two stacked squares" glyphs (`❐` U+2750,
        // `⧉` U+29C9, `🗗` U+1F5D7) and even neighbouring Geometric
        // Shapes glyphs like `▭` U+25AD all render as missing on
        // Windows because text-typeset's font fallback chain only
        // reliably hits `□` from Segoe UI's basic geometric coverage
        // (same root cause as the close button using U+00D7 instead
        // of U+2715). State differentiation is still carried by:
        //   - the OS itself (window is or isn't maximized);
        //   - the reactive a11y name (Maximize / Restore — both
        //     Switcher children carry their own static name and the
        //     hidden child's a11y node doesn't reach AT);
        //   - the action (toggles correctly via `WindowState::placement`).
        // A future pass can swap the glyph for custom rect-primitive
        // icons to restore the visual delta.
        let switcher_idx = self.is_maximized.map(|b| if *b { 1usize } else { 0usize });
        let maximize_action_restore = maximize_action.clone();
        // Both Switcher children share the same external_hover
        // signal: only one is visible at a time, and the host
        // doesn't distinguish between "maximize-normal" and
        // "maximize-zoomed" — the OS just reports a hit on
        // `HTMAXBUTTON`, which both buttons occupy.
        let maximize_normal = ControlButton::new("\u{25A1}", cell_w, cell_h, fg)
            .hover_background(hover_bg)
            .with_action(maximize_action)
            .bind_a11y_name(maximize_name)
            .bind_external_hover(maximize_hover.clone());
        let maximize_zoomed = ControlButton::new("\u{25A1}", cell_w, cell_h, fg)
            .hover_background(hover_bg)
            .with_action(maximize_action_restore)
            .bind_a11y_name(restore_name)
            .bind_external_hover(maximize_hover);
        let max_normal_id = ctx.add(maximize_normal);
        let max_zoomed_id = ctx.add(maximize_zoomed);
        let maximize_switcher = Switcher::new(switcher_idx)
            .child_id(max_normal_id)
            .child_id(max_zoomed_id);
        let switcher_id = ctx.add(maximize_switcher);

        // U+00D7 (Latin-1 ×) instead of U+2715 (Dingbats ✕): the latter
        // is missing from many default Linux sans-serif fonts, leaving the
        // close cell unlabelled. The Latin-1 multiplication sign is in
        // basically every font.
        let close = ControlButton::new("\u{00D7}", cell_w, cell_h, fg)
            .hover_background(close_hover)
            .with_action(close_action)
            .bind_a11y_name(close_name)
            .bind_external_hover(close_hover_signal);
        let close_id = ctx.add(close);

        let row = HStack::new()
            .spacing(0.0)
            .add_child(minimize_id)
            .add_child(switcher_id)
            .add_child(close_id);

        let root = ctx.add(row);
        self.root_child_id = Some(root);

        // Publish the layout snapshot for the parent `TitleBar`'s
        // `after_paint` aggregator. The sink is `None` when the
        // controls are used standalone (e.g. in tests that don't go
        // through `TitleBar`); the publish call is a no-op then.
        //
        // The maximize slot is the Switcher's id, not either glyph
        // button: the inactive Switcher child is dormant and reports
        // `Rect::ZERO`, but the Switcher container itself is laid out
        // by the parent HStack and has stable bounds across the
        // floating ↔ maximized transition.
        if let Some(sink) = &self.layout_sink {
            sink.set(Some(WindowControlsLayout {
                minimize_id,
                maximize_id: switcher_id,
                close_id,
            }));
        }

        vec![root]
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Group);
        builder.set_name(bastyde_i18n::tr_widget!(a11y_window_controls_name()).resolve_now());
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(root_id) => ctx
                .child_size(root_id, proposal)
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

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
