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

use crate::primitives::{Center, FixedSize, HStack, RectWidget, TextWidget, ZStack};
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
        builder.set_name(self.glyph);
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

        let host_min = self.host.clone();
        let host_max = self.host.clone();
        let host_max_signal = self.is_maximized.clone();
        let host_close = self.host.clone();
        let close_override = self.close_action.clone();

        let minimize_action: ControlAction = Rc::new(move |_ctx| host_min.minimize());
        let maximize_action: ControlAction = Rc::new(move |_ctx| {
            host_max_signal.set(!host_max_signal.get());
            host_max.toggle_maximize();
        });
        let close_action: ControlAction = match close_override {
            Some(user_action) => user_action,
            None => Rc::new(move |_ctx| host_close.close()),
        };

        let minimize = ControlButton::new("\u{2014}", cell_w, cell_h, fg)
            .hover_background(hover_bg)
            .with_action(minimize_action);
        let maximize = ControlButton::new("\u{25A1}", cell_w, cell_h, fg)
            .hover_background(hover_bg)
            .with_action(maximize_action);
        // U+00D7 (Latin-1 ×) instead of U+2715 (Dingbats ✕): the latter
        // is missing from many default Linux sans-serif fonts, leaving the
        // close cell unlabelled. The Latin-1 multiplication sign is in
        // basically every font.
        let close = ControlButton::new("\u{00D7}", cell_w, cell_h, fg)
            .hover_background(close_hover)
            .with_action(close_action);

        let row = HStack::new()
            .spacing(0.0)
            .child(minimize)
            .child(maximize)
            .child(close);

        let root = ctx.add(row);
        self.root_child_id = Some(root);
        vec![root]
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
