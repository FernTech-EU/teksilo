//! `ToastSurface` — the rendered chrome of one toast.
//!
//! Built by [`ToastHost`] for each live entry. Owns the severity
//! glyph, title + body column, action row, close button, and the
//! `Role::Alert` / `Role::Status` AccessKit node mapping. The visual
//! chrome (background, padding, layout) is delegated to the active
//! [`ToastStyle`] via `make_body`.

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Path, Point, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{SharedToastStyle, ToastStyleConfig};
use bastyde_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::button::Button;
use crate::icon_button::IconButton;
use crate::link::Link;
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use crate::styles::recipe_toast_style as toast_tokens;
use crate::toast::registry::ToastRegistry;
use crate::toast::{
    DEFAULT_TOAST_AUTO_DISMISS, ToastAction, ToastActionStyle, ToastDismissCause, ToastSeverity,
};

/// Snapshot data passed to a `ToastSurface` for one live entry. Owned
/// by the host's `LiveEntry` and cloned into the surface at build
/// time. `Rc<...>` fields keep callbacks cheap to copy.
#[derive(Clone)]
pub struct ToastSurfaceData {
    pub entry_id: u64,
    pub severity: ToastSeverity,
    pub priority: bastyde_core::styles::ToastPriority,
    pub title: String,
    pub body: Option<String>,
    pub announcement: Option<String>,
    pub actions: Rc<Vec<ToastAction>>,
    pub show_close_button: bool,
    pub on_click: Option<Rc<dyn Fn(&mut bastyde_core::widget::EventContext)>>,
    pub style_override: Option<SharedToastStyle>,
}

impl std::fmt::Debug for ToastSurfaceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastSurfaceData")
            .field("entry_id", &self.entry_id)
            .field("severity", &self.severity)
            .field("priority", &self.priority)
            .field("title", &self.title)
            .field("body", &self.body)
            .field("actions_count", &self.actions.len())
            .field("show_close", &self.show_close_button)
            .finish()
    }
}

/// Internal severity glyph leaf — circle for Info / Success / Error,
/// triangle for Warning. Hidden from the AT tree (the toast surface
/// already carries the role and announcement).
struct SeverityGlyph {
    severity: ToastSeverity,
    size: f32,
}

impl std::fmt::Debug for SeverityGlyph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeverityGlyph")
            .field("severity", &self.severity)
            .finish()
    }
}

impl Widget for SeverityGlyph {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(self.size, self.size).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let color = self.severity.glyph_color(ctx.theme);
        let cx = bounds.x + bounds.width / 2.0;
        let cy = bounds.y + bounds.height / 2.0;
        let half = (bounds.width.min(bounds.height) / 2.0).max(2.0);
        let path = match self.severity {
            ToastSeverity::Warning => {
                let mut p = Path::new();
                p.move_to(Point::new(cx, cy - half));
                p.line_to(Point::new(cx + half, cy + half));
                p.line_to(Point::new(cx - half, cy + half));
                p.close();
                p
            }
            _ => Path::circle(Point::new(cx, cy), half),
        };
        canvas.fill_path(&path, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }
}

/// One rendered toast — chrome owned by [`ToastStyle::make_body`],
/// functional pieces (glyph, body, action row, close button) owned
/// by this widget. Built fresh for each entry — there is no internal
/// `Signal<Option<…>>` slot binding (the host rebuilds on changes).
pub struct ToastSurface {
    data: ToastSurfaceData,
    leading_widget: Option<Box<dyn Widget>>,
    registry: ToastRegistry,
    /// Captured at build time so timer + close handler don't need a
    /// theme lookup at fire-time. `None` under reduced-motion.
    closable_on_escape: bool,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for ToastSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastSurface")
            .field("data", &self.data)
            .field("closable_on_escape", &self.closable_on_escape)
            .finish()
    }
}

impl ToastSurface {
    pub fn new(
        data: ToastSurfaceData,
        leading_widget: Option<Box<dyn Widget>>,
        registry: ToastRegistry,
        closable_on_escape: bool,
    ) -> Self {
        Self {
            data,
            leading_widget,
            registry,
            closable_on_escape,
            root_child_id: None,
        }
    }

    /// AT role per the severity × priority matrix.
    fn at_role(&self) -> bastyde_core::accesskit::Role {
        use bastyde_core::styles::ToastPriority;
        let elevated_priority = matches!(
            self.data.priority,
            ToastPriority::High | ToastPriority::Urgent
        );
        match (self.data.severity, elevated_priority) {
            (ToastSeverity::Error, _) => bastyde_core::accesskit::Role::Alert,
            (ToastSeverity::Warning, true) => bastyde_core::accesskit::Role::Alert,
            _ => bastyde_core::accesskit::Role::Status,
        }
    }

    /// `Live::Polite` for Status, `Live::Assertive` for Alert.
    /// `Urgent` priority forces Assertive regardless of severity.
    fn at_live(&self) -> bastyde_core::accesskit::Live {
        use bastyde_core::styles::ToastPriority;
        if matches!(self.data.priority, ToastPriority::Urgent) {
            return bastyde_core::accesskit::Live::Assertive;
        }
        match self.at_role() {
            bastyde_core::accesskit::Role::Alert => bastyde_core::accesskit::Live::Assertive,
            _ => bastyde_core::accesskit::Live::Polite,
        }
    }

    fn build_action_widget(
        &self,
        ctx: &mut BuildContext,
        action: &ToastAction,
        entry_id: u64,
        registry: ToastRegistry,
    ) -> WidgetId {
        let callback = action.callback();
        let closes_toast = action.closes_toast_flag();
        let label_owned = action.label().to_string();
        let tooltip_owned = action.tooltip_ref().cloned();
        let registry_for_handler = registry.clone();
        let activate = move |ctx: &mut bastyde_core::widget::EventContext| {
            callback(ctx);
            if closes_toast {
                registry_for_handler.dismiss_entry(entry_id, ToastDismissCause::ActionInvoked, ctx);
            }
        };
        // `shortcut_id` is stored on the action for archive replay —
        // NotificationLog renders archived entries' actions as
        // `ctx.send_intent(Intent::by_name(id))` buttons. For the
        // live toast itself the action's own callback is the source
        // of truth, so we don't wire shortcut_id into the rendered
        // widget here.
        match action.style_ref() {
            ToastActionStyle::Link => {
                let mut link = Link::new(lit!(label_owned)).on_activate_fn(activate);
                if let Some(tip) = tooltip_owned {
                    link = link.tooltip(tip);
                }
                ctx.add(link)
            }
            ToastActionStyle::Button { variant } => {
                let mut btn = Button::new(lit!(label_owned))
                    .variant(*variant)
                    .on_activate_fn(activate);
                if let Some(tip) = tooltip_owned {
                    btn = btn.tooltip(tip);
                }
                ctx.add(btn)
            }
        }
    }
}

impl Widget for ToastSurface {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let severity = self.data.severity;
        let entry_id = self.data.entry_id;
        let registry = self.registry.clone();

        // Leading: custom widget (loading spinner, app icon) or the
        // default severity glyph.
        let leading_id = match self.leading_widget.take() {
            Some(w) => ctx.add_boxed(w),
            None => ctx.add(SeverityGlyph {
                severity,
                size: toast_tokens::TOAST_GLYPH_SIZE,
            }),
        };

        // Title + optional body column.
        let title = ctx.add(
            TextWidget::new(lit!(&self.data.title))
                .style(TextStyleRole::BodyBold)
                .bind_color(TextRole::Primary)
                .single_line(),
        );
        let mut text_column = VStack::new()
            .spacing(toast_tokens::TOAST_TITLE_BODY_GAP)
            .add_child(title);
        if let Some(body) = &self.data.body {
            let body_widget = ctx.add(
                TextWidget::new(lit!(body))
                    .style(TextStyleRole::Body)
                    .bind_color(TextRole::Secondary),
            );
            text_column = text_column.add_child(body_widget);
        }
        let text_column_id = ctx.add(text_column);

        // Build action widgets — Links inline, Buttons in a footer row.
        let mut inline_link_ids: Vec<WidgetId> = Vec::new();
        let mut footer_button_ids: Vec<WidgetId> = Vec::new();
        for action in self.data.actions.iter() {
            let widget_id = self.build_action_widget(ctx, action, entry_id, registry.clone());
            match action.style_ref() {
                ToastActionStyle::Link => inline_link_ids.push(widget_id),
                ToastActionStyle::Button { .. } => footer_button_ids.push(widget_id),
            }
        }

        // Body column: text + inline link row (if any) + footer row (if any).
        let mut body_column = VStack::new()
            .spacing(toast_tokens::TOAST_BODY_ACTIONS_GAP)
            .add_child(text_column_id);
        if !inline_link_ids.is_empty() {
            let mut link_row = HStack::new().spacing(toast_tokens::TOAST_CONTENT_GAP);
            for id in inline_link_ids {
                link_row = link_row.add_child(id);
            }
            body_column = body_column.add_child(ctx.add(link_row));
        }
        if !footer_button_ids.is_empty() {
            let spacer_id = ctx.add(Spacer::new());
            let mut footer = HStack::new()
                .spacing(toast_tokens::TOAST_CONTENT_GAP)
                .add_child(spacer_id);
            for id in footer_button_ids {
                footer = footer.add_child(id);
            }
            body_column = body_column.add_child(ctx.add(footer));
        }
        let body_id = ctx.add(body_column);

        // Optional close (X) button — registered as a CustomAction
        // on the AT node too (Action::Dismiss does not exist in
        // AccessKit 0.24).
        let close_id = if self.data.show_close_button {
            let registry_for_close = registry.clone();
            Some(
                ctx.add(IconButton::clear().embedded().on_activate_fn(move |ctx| {
                    registry_for_close.dismiss_entry(
                        entry_id,
                        ToastDismissCause::CloseClicked,
                        ctx,
                    );
                })),
            )
        } else {
            None
        };

        // Delegate chrome to the active ToastStyle.
        let style: SharedToastStyle = self
            .data
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.toast.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeToastStyle));
        let root = style.make_body(
            &ToastStyleConfig {
                severity,
                priority: self.data.priority,
                content: body_id,
                leading_glyph: leading_id,
                trailing_close: close_id,
            },
            ctx,
        );

        // Hover-pause: any pointer entering the toast bumps the host's
        // `hover_count` Signal up; leaving bumps it down. The host's
        // frame-tick effect reads the count > 0 as "pause all timers".
        // The handler attaches to the root chrome widget so the entire
        // visible surface counts as hover area.
        let hover_count = registry.hover_count_signal();
        use bastyde_core::widget_builder::HandlerSet;
        let mut handlers = HandlerSet::new().on_hover(move |entered, _ctx| {
            let n = hover_count.get();
            let next = if entered { n + 1 } else { n.saturating_sub(1) };
            hover_count.set(next);
        });

        // Optional click-through-body callback.
        if let Some(on_click) = self.data.on_click.clone() {
            handlers = handlers
                .on_tap(move |_event, ctx| on_click(ctx))
                .cursor(bastyde_core::widget::CursorIcon::Pointer);
        }

        // Escape dismisses while focused — implemented via on_key so
        // we don't have to register a tree-wide shortcut for every
        // toast.
        if self.closable_on_escape {
            let registry_for_esc = registry.clone();
            handlers = handlers.focusable(true).on_key(move |event, ctx| {
                use bastyde_core::event::{EventResponse, Key, WidgetEvent};
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::Escape, ..
                    } => {
                        registry_for_esc.dismiss_entry(
                            entry_id,
                            ToastDismissCause::EscapePressed,
                            ctx,
                        );
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(280.0, 56.0))
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(self.at_role());
        builder.set_live(self.at_live());
        // `live_atomic` so the whole title+body is announced as one
        // unit — matches `aria-atomic=true` default for role=alert /
        // role=status.
        builder.inner_mut().set_live_atomic();
        // Name = announcement override or visible title (matches
        // Snackbar / Banner convention).
        let name = self
            .data
            .announcement
            .clone()
            .unwrap_or_else(|| self.data.title.clone());
        builder.set_name(name);
        if let Some(body) = &self.data.body {
            builder.set_description(body.clone());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Convert milliseconds to a Duration — used for default tests.
#[doc(hidden)]
pub fn _default_dismiss() -> std::time::Duration {
    DEFAULT_TOAST_AUTO_DISMISS
}
