//! MenuBar — a horizontal menu bar with dropdown menus.
//!
//! # FernUI
//! ```ignore
//! MenuBar::new()
//!     .menu_literal("File", || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new_literal("New").on_activate_fn(|ctx| ctx.send_intent(AppIntent::New)))
//!             .separator()
//!             .item(MenuItem::new_literal("Quit").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Quit)))
//!     ))
//!     .menu_literal("Edit", || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new_literal("Cut").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Cut)))
//!             .item(MenuItem::new_literal("Copy").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Copy)))
//!     ))
//!     .trailing_slot(Button::new_literal("Settings").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Settings)))
//! ```

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{SurfaceRole, TextStyleRole};

use crate::menu_context::MenuContext;
use crate::primitives::{HStack, Padding, RectWidget, Spacer, TextWidget, ZStack};

// ---------------------------------------------------------------------------
// MenuBarEntry — pending menu definition
// ---------------------------------------------------------------------------

struct MenuBarEntry {
    label: String,
    factory: Box<dyn Fn() -> Box<dyn Widget>>,
}

// ---------------------------------------------------------------------------
// MenuBar — public widget
// ---------------------------------------------------------------------------

/// A horizontal menu bar with dropdown menus.
///
/// Supports the Slot system (architecture Section 5.3):
/// - `leading_slot`: content before the menu buttons (e.g., app icon)
/// - `trailing_slot`: content after the menu buttons (e.g., search, user avatar)
pub struct MenuBar {
    entries: Vec<MenuBarEntry>,
    leading_slot: Vec<PendingChild>,
    trailing_slot: Vec<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            leading_slot: Vec::new(),
            trailing_slot: Vec::new(),
            root_child_id: None,
        }
    }

    pub fn menu(
        mut self,
        label: impl Into<fern_i18n::LocalizedString>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.entries.push(MenuBarEntry {
            label: ls.resolve_now(),
            factory: Box::new(factory),
        });
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `menu(...)` accepting a raw label.
    #[doc(hidden)]
    pub fn menu_literal(
        self,
        label: impl Into<String>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.menu(fern_i18n::LocalizedString::literal(label), factory)
    }

    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Default for MenuBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MenuBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuBar")
            .field("entries", &self.entries.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// MenuBarTrigger — internal trigger label
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MenuBarTrigger {
    label: String,
    index: usize,
    menu_ctx: MenuContext,
    root_child_id: Option<WidgetId>,
}

impl Widget for MenuBarTrigger {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme();
        let menu_style = theme.components.menu;
        let radius_control = theme.shape.radius_control;
        let index = self.index;
        let menu_ctx = self.menu_ctx.clone();

        // Background role: `AccentSubtle` when open (the Int UI token for
        // highlighted menu-bar entries) or `Transparent` at rest. Replaces
        // the previous hand-mixed `accent.with_alpha(0.12)` wash.
        let bg_role = menu_ctx.open_index.map(move |open| {
            if *open == Some(index) {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        });

        // Text color can't collapse to a pure role: the at-rest state is
        // `text_primary.with_alpha(0.8)` (dimmed primary — distinct from
        // TextRole::Secondary, which is a different hue). Keep a direct
        // `theme_signal` map for the blended case.
        let theme_signal = ctx.theme_signal();
        let text_color = menu_ctx
            .open_index
            .zip(&theme_signal)
            .map(move |(open, t)| {
                if *open == Some(index) {
                    t.colors.text_primary
                } else {
                    t.colors.text_primary.with_alpha(0.8)
                }
            });

        let label = TextWidget::new_literal(&self.label)
            .style(TextStyleRole::Small)
            .bind_color(text_color)
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label);

        let padding =
            Padding::symmetric(4.0, menu_style.item_padding_horizontal).child_id(label_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .bind_background(bg_role)
            .corner_radius(fern_tokens::CornerRadius::uniform(radius_control));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new()
            .on_tap({
                let menu_ctx = menu_ctx.clone();
                move |_pos, ctx: &mut EventContext| {
                    if menu_ctx.open_index.get() == Some(index) {
                        menu_ctx.close(ctx);
                    } else {
                        menu_ctx.open_at(index, ctx);
                    }
                }
            })
            .on_hover({
                let menu_ctx = menu_ctx.clone();
                move |entered: bool, ctx: &mut EventContext| {
                    if entered {
                        // If another menu is open, switch immediately (no delay)
                        let current = menu_ctx.open_index.get();
                        if current.is_some() && current != Some(index) {
                            menu_ctx.open_at(index, ctx);
                        }
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown | Key::Enter | Key::Space,
                            ..
                        } => {
                            menu_ctx.open_at(index, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(-1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(1, ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        // Re-query accessibility when this trigger's open/closed state flips so
        // `set_expanded` stays in sync with the open menu index.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.menu_ctx.open_index.bind_to(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 28.0)),
            None => proposal.resolve(60.0, 28.0),
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::MenuItem);
        builder.set_name(&self.label);
        // Every top-level menu bar entry opens a dropdown Menu.
        builder.set_has_popup(fern_core::accesskit::HasPopup::Menu);
        builder.set_expanded(self.menu_ctx.open_index.get() == Some(self.index));
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// MenuOverlayHost — wraps dropdown content, handles focus + cross-menu keys
// ---------------------------------------------------------------------------

/// Wraps dropdown menu content (typically a MenuList). Responsibilities:
/// - Resets `open_index` when focus is lost (overlay dismissed)
/// - Handles ArrowLeft/Right for cross-menu navigation (bubbles up from MenuList)
#[derive(Debug)]
struct MenuOverlayHost {
    inner: Option<Box<dyn Widget>>,
    menu_ctx: MenuContext,
    menu_index: usize,
    inner_id: Option<WidgetId>,
}

impl Widget for MenuOverlayHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let inner_widget = self.inner.take().expect("MenuOverlayHost built twice");
        let id = ctx.add_boxed(inner_widget);
        self.inner_id = Some(id);

        // Register inner widget as the focus target for this menu index
        self.menu_ctx.set_focus_id(self.menu_index, id);

        let menu_ctx = self.menu_ctx.clone();
        let menu_index = self.menu_index;
        let handler_set = HandlerSet::new()
            .on_focus({
                let menu_ctx = menu_ctx.clone();
                move |gained: bool, ctx: &mut EventContext| {
                    if !gained && menu_ctx.open_index.get() == Some(menu_index) {
                        // Overlay was dismissed — close the menu and restore focus
                        menu_ctx.close(ctx);
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    // These keys bubble up from the inner MenuList when it returns Ignored
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(-1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Escape, ..
                        } => {
                            menu_ctx.close(ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            });
        // NOT focusable — the inner MenuList receives focus directly.
        // ArrowLeft/Right and FocusLost bubble from MenuList through here.
        ctx.apply_self_handlers(handler_set);

        vec![id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        self.inner_id
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The inner widget (typically `MenuList`) owns the `Role::Menu`
        // semantics. A second Menu role here would nest two Menu nodes
        // per dropdown, confusing screen readers that look for a single
        // Menu per popup. `GenericContainer` is the ARIA `none`/`presentation`
        // equivalent: the host is kept in the tree for focus/key routing
        // but is ignored by assistive tech.
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.inner_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// MenuBar Widget impl
// ---------------------------------------------------------------------------

impl Widget for MenuBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme_signal = ctx.theme_signal();

        let open_index: Signal<Option<usize>> = ctx.signal(None);
        let menu_ctx = MenuContext::new(open_index);

        // Build the full row: [leading_slot | triggers... | Spacer | trailing_slot]
        let mut row = HStack::new().spacing(2.0);

        // Leading slot
        for pending in self.leading_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }

        // Menu triggers + content
        let mut trigger_ids = Vec::new();
        let mut content_ids = Vec::new();

        for (i, entry) in self.entries.drain(..).enumerate() {
            // Wrap factory output in MenuOverlayHost for focus/key handling
            let host = MenuOverlayHost {
                inner: Some((entry.factory)()),
                menu_ctx: menu_ctx.clone(),
                menu_index: i,
                inner_id: None,
            };
            let content_id = ctx.add(host);
            ctx.set_dormant(content_id);

            let trigger = MenuBarTrigger {
                label: entry.label,
                index: i,
                menu_ctx: menu_ctx.clone(),
                root_child_id: None,
            };
            let trigger_id = ctx.add(trigger);
            row = row.add_child(trigger_id);

            trigger_ids.push(trigger_id);
            content_ids.push(content_id);
        }

        // Register all trigger/content IDs in the context.
        // focus_id is initially content_id; MenuOverlayHost::build() will
        // overwrite it with the actual inner MenuList ID.
        for (i, (&tid, &cid)) in trigger_ids.iter().zip(content_ids.iter()).enumerate() {
            menu_ctx.register(i, tid, cid, cid);
        }

        // Spacer pushes triggers left, trailing slot right
        row = row.child(Spacer::new());

        // Trailing slot
        for pending in self.trailing_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }

        let row_id = ctx.add(row);

        let bg = RectWidget::new()
            .background(SurfaceRole::Main)
            .bind_border_color(theme_signal.map(|t| t.colors.border.with_alpha(0.2)))
            .bind_border_width(0.0_f32);
        let bg_id = ctx.add(bg);

        let padding = Padding::symmetric(0.0, 2.0).child_id(row_id);
        let padding_id = ctx.add(padding);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => {
                let content_proposal = SizeProposal {
                    width: proposal.width,
                    height: None,
                };
                let size = ctx
                    .child_size(id, content_proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(proposal.width.unwrap_or(size.width), size.height)
            }
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::MenuBar);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
