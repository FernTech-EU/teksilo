// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ToastSurface` — the rendered chrome of one toast.
//!
//! Built by `ToastHost` for each live entry. Owns the severity
//! glyph, title + body column, action row, close button, and the
//! `Role::Alert` / `Role::Status` AccessKit node mapping. The visual
//! chrome (background, padding, layout) is delegated to the active
//! `ToastStyle` via `make_body`.

use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{SharedToastStyle, ToastStyleConfig};
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{TextRole, TextStyleRole};

use crate::button::Button;
use crate::icon_button::IconButton;
use crate::link::Link;
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use crate::severity_badge::SeverityBadge;
use crate::styles::recipe_toast_style as toast_tokens;
use crate::toast::registry::ToastRegistry;
use crate::toast::{
    DEFAULT_TOAST_AUTO_DISMISS, ToastAction, ToastActionStyle, ToastDismissCause, ToastSeverity,
};
use teksilo_i18n::LocalizedString;

/// Snapshot data passed to a `ToastSurface` for one live entry. Owned
/// by the host's `LiveEntry` and cloned into the surface at build
/// time. `Rc<...>` fields keep callbacks cheap to copy.
#[derive(Clone)]
pub struct ToastSurfaceData {
    pub entry_id: u64,
    pub severity: ToastSeverity,
    pub priority: teksilo_core::styles::ToastPriority,
    pub title: LocalizedString,
    pub body: Option<LocalizedString>,
    pub announcement: Option<LocalizedString>,
    pub actions: Rc<Vec<ToastAction>>,
    pub show_close_button: bool,
    pub on_click: Option<Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>>,
    pub style_override: Option<SharedToastStyle>,
    /// Clamped / unfolded state of the body, owned by the live entry so it survives the
    /// host's surface rebuilds — see `LiveEntry::body_state`.
    pub body_state: teksilo_core::signal::Signal<u8>,
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

/// One rendered toast — chrome owned by `ToastStyle::make_body`,
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
    /// Build a surface for a single live toast entry. Called by [`ToastHost`](crate::toast::host::ToastHost)
    /// once per live registry entry during each rebuild pass. `leading_widget` is `Some` for
    /// `Toast::loading` (a spinner) and `None` for severity-glyph entries (a `SeverityBadge`
    /// is synthesised in `build`). `closable_on_escape` mirrors the matching `Toast` field.
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
    fn at_role(&self) -> teksilo_core::accesskit::Role {
        use teksilo_core::styles::ToastPriority;
        let elevated_priority = matches!(
            self.data.priority,
            ToastPriority::High | ToastPriority::Urgent
        );
        match (self.data.severity, elevated_priority) {
            (ToastSeverity::Error, _) => teksilo_core::accesskit::Role::Alert,
            (ToastSeverity::Warning, true) => teksilo_core::accesskit::Role::Alert,
            _ => teksilo_core::accesskit::Role::Status,
        }
    }

    /// `Live::Polite` for Status, `Live::Assertive` for Alert.
    /// `Urgent` priority forces Assertive regardless of severity.
    fn at_live(&self) -> teksilo_core::accesskit::Live {
        use teksilo_core::styles::ToastPriority;
        if matches!(self.data.priority, ToastPriority::Urgent) {
            return teksilo_core::accesskit::Live::Assertive;
        }
        match self.at_role() {
            teksilo_core::accesskit::Role::Alert => teksilo_core::accesskit::Live::Assertive,
            _ => teksilo_core::accesskit::Live::Polite,
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
        let label_owned = action.label_ls();
        let tooltip_owned = action.tooltip_ref().cloned();
        let registry_for_handler = registry.clone();
        let activate = move |ctx: &mut teksilo_core::widget::EventContext| {
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
                let mut link = Link::new(label_owned).on_activate_fn(activate);
                if let Some(tip) = tooltip_owned {
                    link = link.tooltip(tip);
                }
                ctx.add(link)
            }
            ToastActionStyle::Button { variant } => {
                let mut btn = Button::new(label_owned)
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
            None => ctx.add(SeverityBadge::new(
                severity.into(),
                toast_tokens::TOAST_GLYPH_SIZE,
            )),
        };

        // Title + optional body column.
        let title = ctx.add(
            TextWidget::new(self.data.title.clone())
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary)
                .single_line(),
        );
        let mut text_column = VStack::new()
            .spacing(toast_tokens::TOAST_TITLE_BODY_GAP)
            .add_child(title);
        if let Some(body) = &self.data.body {
            // Not a bare `TextWidget`: a body is whatever the app hands over, and apps
            // hand over error text with no length bound. `CollapsibleBody` clamps it and
            // grows a disclosure row only when clamping actually hid something — see its
            // module docs.
            let registry_for_expand = registry.clone();
            let body_widget = ctx.add(
                crate::toast::body::CollapsibleBody::new(
                    body.clone(),
                    self.data.body_state.clone(),
                )
                .on_expand(move || registry_for_expand.cancel_auto_dismiss(entry_id)),
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
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeToastStyle::default()));
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
        use teksilo_core::widget_builder::HandlerSet;
        let mut handlers = HandlerSet::new().on_hover(move |entered, _ctx| {
            let n = hover_count.get();
            let next = if entered { n + 1 } else { n.saturating_sub(1) };
            hover_count.set(next);
        });

        // Optional click-through-body callback.
        if let Some(on_click) = self.data.on_click.clone() {
            handlers = handlers
                .on_tap(move |_event, ctx| on_click(ctx))
                .cursor(teksilo_core::widget::CursorIcon::Pointer);
        }

        // Escape dismisses while focused — implemented via on_key so
        // we don't have to register a tree-wide shortcut for every
        // toast.
        if self.closable_on_escape {
            let registry_for_esc = registry.clone();
            handlers = handlers.focusable(true).on_key(move |event, ctx| {
                use teksilo_core::event::{EventResponse, Key, WidgetEvent};
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
    ) -> teksilo_core::widget::LayoutResponse {
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
            .as_ref()
            .map(|a| a.resolve_now())
            .unwrap_or_else(|| self.data.title.resolve_now());
        builder.set_name(name);
        if let Some(body) = &self.data.body {
            builder.set_description(body.resolve_now());
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
