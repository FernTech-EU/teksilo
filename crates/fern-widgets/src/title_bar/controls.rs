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

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_core::PlatformTitleBarHost;
use fern_tokens::{Color, TextStyleRole};

use crate::primitives::{Center, FixedSize, HStack, RectWidget, Switcher, TextWidget, ZStack};
use crate::title_bar::CloseAction;

/// Action invoked when a [`ControlButton`] is tapped.
pub type ControlAction = Rc<dyn Fn(&mut EventContext)>;

/// A compact, flush-fitting window-control button.
///
/// Composes existing primitives — a `FixedSize` cell wrapping a `ZStack`
/// of (hover background, centred glyph). Hover state is tracked in a
/// `Signal<Color>` that drives the background's `bind_background` so a
/// hover change repaints with no relayout.
pub struct ControlButton {
    glyph: &'static str,
    width: f32,
    height: f32,
    fg: Color,
    /// Background colour drawn over the title bar when the cursor is
    /// inside the cell. `Color::TRANSPARENT` keeps the cell flat.
    hover_bg: Color,
    action: Option<ControlAction>,
    bg_signal: Signal<Color>,
    /// Accessible name exposed to AT. Reactive so `WindowControls` can
    /// flip it between "Maximize" and "Restore" without rebuilding.
    a11y_name: Signal<String>,
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
    pub fn new(glyph: &'static str, width: f32, height: f32, fg: Color) -> Self {
        Self {
            glyph,
            width,
            height,
            fg,
            hover_bg: Color::TRANSPARENT,
            action: None,
            bg_signal: Signal::new(Color::TRANSPARENT),
            a11y_name: Signal::new(String::new()),
            root_child_id: None,
        }
    }

    pub fn hover_background(mut self, color: Color) -> Self {
        self.hover_bg = color;
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
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<WidgetId> {

        // Reactive hover background: starts transparent, flips to
        // `hover_bg` while the pointer is inside, back to transparent on
        // leave. RectWidget with `bind_background` repaints on change.
        let bg_signal = ctx.signal(Color::TRANSPARENT);
        self.bg_signal = bg_signal.clone();

        let bg_rect = ctx.add(RectWidget::new().bind_background(bg_signal.clone()));

        let glyph_text = TextWidget::new_literal(self.glyph)
            .style(TextStyleRole::Body)
            .color(self.fg)
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

        // Self handlers: tap fires the action, hover drives bg_signal.
        let bg_enter = bg_signal.clone();
        let bg_leave = bg_signal.clone();
        let hover_color = self.hover_bg;
        let mut handlers = HandlerSet::new()
            .cursor(CursorIcon::Pointer)
            .on_hover(move |entered, _ctx| {
                if entered {
                    bg_enter.set(hover_color);
                } else {
                    bg_leave.set(Color::TRANSPARENT);
                }
            });

        if let Some(action) = self.action.take() {
            handlers = handlers.on_tap(move |_pos, ctx| action(ctx));
        }

        ctx.apply_self_handlers(handlers);

        // Refresh the a11y node whenever the name signal changes
        // (maximize ⇄ restore toggle).
        let self_id = ctx.self_id();
        self.a11y_name.bind_to(
            self_id,
            ctx.binding_registry(),
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

        self.root_child_id = Some(sized);
        vec![sized]
    }

    fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // Always exactly the configured cell. Returning the proposal here
        // would let an HStack stretch us to the leftover width.
        Size::new(self.width, self.height)
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

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Button);
        let name = self.a11y_name.get();
        if !name.is_empty() {
            builder.set_name(name);
        }
        builder.add_action(fern_core::accesskit::Action::Click);
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
        }
    }
}

impl Widget for WindowControls {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<WidgetId> {
        // Window controls are painted once during build — swapping the
        // `Color` fields on a new build would be wasted allocation when
        // the glyph/hover colors follow the theme. We read a snapshot and
        // leave fine-grained reactive repaint to `mark_all_dirty` on the
        // static `.color(...)` values inside each `ControlButton`.
        let theme = ctx.theme_signal().get();
        let fg = theme.colors.text_primary;
        let hover_bg = theme.colors.surface_hover;
        let close_hover = theme.colors.status_error_bg;

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
                w.placement().set(fern_core::WindowPlacement::Minimized);
            }
        });
        let maximize_action: ControlAction = Rc::new(move |ctx| {
            if let Some(w) = ctx.window() {
                let next = if w.placement().get().is_maximized() {
                    fern_core::WindowPlacement::Floating
                } else {
                    fern_core::WindowPlacement::Maximized
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
        let minimize_name = fern_i18n::tr_widget!(a11y_window_minimize_name()).to_signal();
        let close_name = fern_i18n::tr_widget!(a11y_window_close_name()).to_signal();
        let maximize_name = fern_i18n::tr_widget!(a11y_window_maximize_name()).to_signal();
        let restore_name = fern_i18n::tr_widget!(a11y_window_restore_name()).to_signal();

        let minimize = ControlButton::new("\u{2014}", cell_w, cell_h, fg)
            .hover_background(hover_bg)
            .with_action(minimize_action)
            .bind_a11y_name(minimize_name);
        // Maximize/restore glyph swap. `□` (U+25A1) when the window is
        // normal, `❐` (U+2750) when maximized. Wrapping both in a
        // `Switcher` driven by `is_maximized` keeps the geometry stable
        // (Switcher lays out all children, shows one) — and the hidden
        // child's a11y node doesn't reach AT, so each button gets its
        // own static name rather than a reactive toggle.
        let switcher_idx = self
            .is_maximized
            .map(|b| if *b { 1usize } else { 0usize });
        let maximize_action_restore = maximize_action.clone();
        let maximize_normal = ControlButton::new("\u{25A1}", cell_w, cell_h, fg)
            .hover_background(hover_bg)
            .with_action(maximize_action)
            .bind_a11y_name(maximize_name);
        let maximize_zoomed = ControlButton::new("\u{2750}", cell_w, cell_h, fg)
            .hover_background(hover_bg)
            .with_action(maximize_action_restore)
            .bind_a11y_name(restore_name);
        let maximize = Switcher::new(switcher_idx)
            .child(maximize_normal)
            .child(maximize_zoomed);
        // U+00D7 (Latin-1 ×) instead of U+2715 (Dingbats ✕): the latter
        // is missing from many default Linux sans-serif fonts, leaving the
        // close cell unlabelled. The Latin-1 multiplication sign is in
        // basically every font.
        let close = ControlButton::new("\u{00D7}", cell_w, cell_h, fg)
            .hover_background(close_hover)
            .with_action(close_action)
            .bind_a11y_name(close_name);

        let row = HStack::new()
            .spacing(0.0)
            .child(minimize)
            .child(maximize)
            .child(close);

        let root = ctx.add(row);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Group);
        builder.set_name(
            fern_i18n::tr_widget!(a11y_window_controls_name()).resolve_now(),
        );
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(root_id) => ctx
                .child_size(root_id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
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

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
