//! MenuList — a vertical container for MenuItem and MenuSeparator widgets.
//!
//! Provides a themed surface (background, border, corner radius) and
//! keyboard navigation (ArrowUp/Down, Enter, Escape).

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::primitives::{MaxSize, Padding, RectWidget, VStack, ZStack};
use crate::scroll_area::ScrollArea;

/// Marker for whether a pending item is a menu item or a separator.
enum MenuEntry {
    Item(PendingChild),
    Separator,
}

/// A separator line within a MenuList.
#[derive(Debug)]
pub struct MenuSeparator;

impl Widget for MenuSeparator {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(0.0);
        Size::new(width, ctx.theme.components.menu.separator_height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas, ctx: &PaintContext) {
        // Int UI menu separator: a flush-edge 1 dp line in `divider` color,
        // vertically centered in the `separator_height` (9 dp) slot — that
        // slot provides 4 dp top/bottom breathing room around the line.
        let color = ctx.theme.colors.divider;
        let thickness = ctx.theme.shape.border_width;
        let y = bounds.y + (bounds.height - thickness) * 0.5;
        canvas.fill_rect(Rect::new(bounds.x, y, bounds.width, thickness), color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Splitter);
    }
}

// Note: MenuList's `max_visible_items` caps the panel height and wraps
// the item column in a `ScrollArea`, but does **not** yet virtualize —
// every item widget (plus separators) is still built eagerly. True
// virtualization requires a model-backed MenuList API (item descriptor
// → delegate builds the row) because today's surface accepts arbitrary
// `impl Widget` children directly. Tracked as follow-up; eager build
// is cheap enough that ScrollArea-capped panels of 100+ items are
// already fine in practice.

/// Wrapper that adds a keyboard-focus highlight behind a menu item.
/// The highlight is driven by a shared `focused_index` signal — when
/// `focused_index == Some(my_index)`, a subtle background appears.
/// The binding registry automatically marks this widget for repaint
/// when the signal changes (same mechanism as ComboBox DropdownItem).
#[derive(Debug)]
struct KeyboardHighlightWrapper {
    item_id: WidgetId,
    index: usize,
    focused_index: Signal<Option<usize>>,
    root_child_id: Option<WidgetId>,
}

impl Widget for KeyboardHighlightWrapper {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let index = self.index;

        // Keyboard focus highlight uses the dedicated `surface_selected`
        // token (not an alpha wash over `accent`) so it tracks theme
        // changes and stays distinct from mouse hover (`surface_hover`).
        // Role-based: no theme_signal zip; paint resolves the role.
        let bg_role = self.focused_index.map(move |focused| {
            if *focused == Some(index) {
                SurfaceRole::Selected
            } else {
                SurfaceRole::Transparent
            }
        });

        let bg = RectWidget::new().background(bg_role);
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(self.item_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        // Forward the proposal to the wrapped MenuItem directly rather than
        // going through the internal ZStack. ZStack::size_that_fits always
        // queries its children with `unspecified` (correct for most uses,
        // since ZStack layers typically have independent natural sizes),
        // which would strip the parent's width proposal. But for this
        // wrapper the whole point is that the MenuItem fills the VStack's
        // cross-axis width — bypass the ZStack in the sizing path so the
        // width propagates to the MenuItem → HStack → spacer chain.
        let item_size = ctx
            .child_size(self.item_id, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 32.0));
        // Respect the proposed width when offered, so VStack::place_children
        // places this wrapper at the full popup width.
        let width = proposal.width.unwrap_or(item_size.width);
        Size::new(width, item_size.height).into()
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational wrapper — the real semantics live on the
        // wrapped MenuItem. Without this, the default node would
        // insert an unannotated container between `Role::Menu` and
        // `Role::MenuItem` in the a11y tree.
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }
}

/// A themed vertical menu container.
///
/// ```ignore
/// MenuList::new()
///     .item(MenuItem::new_literal("Cut").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Cut)))
///     .separator()
///     .item(MenuItem::new_literal("Paste").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Paste)))
/// ```
pub struct MenuList {
    entries: Vec<MenuEntry>,
    root_child_id: Option<WidgetId>,
    /// Widget IDs of actual menu items (not separators), for keyboard navigation.
    item_widget_ids: Vec<WidgetId>,
    /// Whether each item (by index into item_widget_ids) is a submenu trigger.
    submenu_flags: Vec<bool>,
    /// When set and the item count (counting every entry — items *and*
    /// separators — against the row count, not pixels) exceeds the
    /// limit, the content column is wrapped in a `ScrollArea` and the
    /// panel height is capped to `n * item_height`. `None` (default)
    /// lets the menu grow with its content.
    max_visible_items: Option<usize>,
    /// Side of the menu panel that is visually attached to its trigger
    /// (e.g. a menu button or combo-box). When set, drop shadow
    /// drawing is suppressed on that side so the menu reads as one
    /// piece with the trigger. Set by the opener based on the chosen
    /// placement; `None` leaves the full halo intact.
    attached_side: Option<crate::shadow::AttachedSide>,
}

impl MenuList {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            root_child_id: None,
            item_widget_ids: Vec::new(),
            submenu_flags: Vec::new(),
            max_visible_items: None,
            attached_side: None,
        }
    }

    /// Suppress drop-shadow drawing on the side that visually merges
    /// with the menu's trigger. See [`crate::shadow::AttachedSide`]
    /// for the available edges.
    pub fn attached_side(mut self, side: crate::shadow::AttachedSide) -> Self {
        self.attached_side = Some(side);
        self
    }

    /// Add a menu item (typically a `MenuItem`).
    pub fn item(mut self, widget: impl Widget + 'static) -> Self {
        // Detect submenu items via Any downcast before boxing
        let is_submenu = (&widget as &dyn std::any::Any)
            .downcast_ref::<crate::menu_item::MenuItem>()
            .is_some_and(|mi| mi.is_submenu());
        self.submenu_flags.push(is_submenu);
        self.entries
            .push(MenuEntry::Item(PendingChild::Deferred(Box::new(widget))));
        self
    }

    /// Add a separator line.
    pub fn separator(mut self) -> Self {
        self.entries.push(MenuEntry::Separator);
        self
    }

    /// Cap the panel height to roughly `n * item_height` and make the
    /// content scrollable when that height is exceeded. Clamped to at
    /// least 1. Useful for long menus (e.g. a "Recent files" list) —
    /// without this, a very long menu grows to exceed the window.
    ///
    /// Note: items are still materialized eagerly; this is a viewport
    /// cap, not virtualization. See the module-level note.
    pub fn max_visible_items(mut self, n: usize) -> Self {
        self.max_visible_items = Some(n.max(1));
        self
    }
}

impl Default for MenuList {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MenuList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuList")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Widget for MenuList {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme_signal = ctx.theme_signal();
        let menu_style = theme_signal.get().components.menu;

        // Keyboard-focused item index (shared with the key handler and wrappers).
        // The binding registry propagates repaints when this changes.
        let focused_index: Signal<Option<usize>> = ctx.signal(None);

        // Build all entries into a VStack, wrapping items in highlight wrappers
        let mut vstack = VStack::new();
        self.item_widget_ids.clear();
        let mut item_counter = 0_usize;

        for entry in self.entries.drain(..) {
            match entry {
                MenuEntry::Item(pending) => {
                    let item_id = match pending {
                        PendingChild::Id(id) => id,
                        PendingChild::Deferred(w) => ctx.add_boxed(w),
                    };
                    self.item_widget_ids.push(item_id);

                    // Wrap in a highlight container driven by focused_index
                    let wrapper = KeyboardHighlightWrapper {
                        item_id,
                        index: item_counter,
                        focused_index: focused_index.clone(),
                        root_child_id: None,
                    };
                    vstack = vstack.child(wrapper);
                    item_counter += 1;
                }
                MenuEntry::Separator => {
                    vstack = vstack.child(MenuSeparator);
                }
            }
        }

        let vstack_id = ctx.add(vstack);

        let padding = Padding::uniform(4.0).child_id(vstack_id);
        let padding_id = ctx.add(padding);

        // Viewport cap. When `max_visible_items` is set and the real
        // item count exceeds it, wrap the padded column in a
        // `ScrollArea` + `MaxSize` pair sized to `cap * item_height`
        // + the 4 px outer padding on each edge. Separators don't
        // count against the cap — they're visually small and no real
        // menu stacks enough of them for the slight under-shoot to
        // matter.
        let visible_cap_id = match self.max_visible_items {
            Some(cap) if self.item_widget_ids.len() > cap => {
                let max_height = cap as f32 * menu_style.item_height + 8.0;
                let scrollable = ScrollArea::from_id(padding_id).preferred_size(0.0, max_height);
                let scrollable_id = ctx.add(scrollable);
                ctx.add(MaxSize::height(max_height).child_id(scrollable_id))
            }
            _ => padding_id,
        };

        // Themed surface background — Int UI menus use the popup radius (8 dp)
        // and a 1 dp border on the raised surface.
        let bg = RectWidget::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .bind_border_width(menu_style.popup_border_width)
            .corner_radius(CornerRadius::uniform(menu_style.popup_corner_radius));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(visible_cap_id);
        let root_id = ctx.add(zstack);

        self.root_child_id = Some(root_id);

        // Keyboard navigation handler
        let item_count = self.item_widget_ids.len();
        let item_ids = self.item_widget_ids.clone();
        let sub_flags = self.submenu_flags.clone();
        let handler_set = HandlerSet::new()
            .on_key(
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown,
                            ..
                        } => {
                            if item_count == 0 {
                                return EventResponse::Ignored;
                            }
                            let current = focused_index.get().unwrap_or(usize::MAX);
                            let next = if current >= item_count - 1 {
                                0
                            } else {
                                current + 1
                            };
                            focused_index.set(Some(next));
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowUp, ..
                        } => {
                            if item_count == 0 {
                                return EventResponse::Ignored;
                            }
                            let current = focused_index.get().unwrap_or(0);
                            let next = if current == 0 {
                                item_count - 1
                            } else {
                                current - 1
                            };
                            focused_index.set(Some(next));
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            // Activate the focused item via synthetic click.
                            if let Some(idx) = focused_index.get()
                                && idx < item_ids.len()
                            {
                                ctx.synthetic_click(item_ids[idx]);
                                return EventResponse::Handled;
                            }
                            EventResponse::Ignored
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            // Only open submenus; for non-submenu items, let it bubble
                            // to MenuOverlayHost which navigates to the next bar menu.
                            if let Some(idx) = focused_index.get()
                                && idx < sub_flags.len()
                                && sub_flags[idx]
                            {
                                ctx.synthetic_click(item_ids[idx]);
                                return EventResponse::Handled;
                            }
                            EventResponse::Ignored
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft | Key::Escape,
                            ..
                        } => {
                            // Let it bubble to MenuOverlayHost (for bar navigation)
                            // or the tree-level Escape handler (for overlay dismissal).
                            EventResponse::Ignored
                        }
                        _ => EventResponse::Ignored,
                    }
                },
            )
            .focusable(true);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => {
                // Menu lists size to their content, with a minimum width
                let child_size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(child_size.width.max(120.0), child_size.height)
            }
            None => proposal.resolve(120.0, 0.0),
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

    fn paint(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas, ctx: &PaintContext) {
        // Drop shadow underneath the menu surface. The bg + border
        // themselves are painted by a child `RectWidget` set up in
        // `build()`, so this method only contributes the shadow.
        let radius = CornerRadius::uniform(ctx.theme.components.menu.popup_corner_radius);
        crate::shadow::paint_layered_shadow(
            canvas,
            bounds,
            radius,
            &ctx.theme.shape.shadow_sm,
            &ctx.theme.shape.shadow_inner_sm,
            ctx.theme.components.menu.shadow_density,
            self.attached_side,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Menu);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_item::MenuItem;
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    fn light_tree() -> WidgetTree {
        WidgetTree::new().with_theme(fern_core::presets::intui::light())
    }

    #[test]
    fn unbounded_menu_grows_with_content() {
        // Without `max_visible_items`, a long menu should size to its
        // content — not be silently clipped. Use `with_width` so the
        // root takes its natural height from `size_that_fits` rather
        // than the proposal's exact height.
        let mut tree = light_tree();
        let mut menu = MenuList::new();
        for i in 0..20 {
            menu = menu.item(MenuItem::new_literal(format!("Entry {i}")));
        }
        let id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        let h = tree.bounds(id).height;
        // 20 items × 24 px ≈ 480 px — well above a capped viewport.
        assert!(
            h > 400.0,
            "uncapped menu should grow to fit all items, got height={}",
            h
        );
    }

    #[test]
    fn max_visible_items_caps_height() {
        // With `max_visible_items(5)`, a 20-entry menu must cap near
        // `5 * item_height + outer padding` rather than growing to fit
        // every row.
        let mut tree = light_tree();
        let mut menu = MenuList::new().max_visible_items(5);
        for i in 0..20 {
            menu = menu.item(MenuItem::new_literal(format!("Entry {i}")));
        }
        let id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        let h = tree.bounds(id).height;
        // 5 rows × 24 px + 8 px padding = 128. Give a generous
        // tolerance band (theme may tweak item_height); the key
        // regression to catch is "grew to fit everything" (~480 px).
        assert!(
            h < 200.0,
            "capped menu height should be bounded by max_visible_items, got {}",
            h
        );
        assert!(h > 0.0, "capped menu should have positive height");
    }

    #[test]
    fn max_visible_items_below_count_has_no_effect() {
        // When item count fits under the cap, the ScrollArea wrapper
        // must not be inserted — sanity check that we don't pay the
        // wrapper cost (or its minor layout overhead) for small menus.
        let mut tree = light_tree();
        let menu = MenuList::new()
            .max_visible_items(10)
            .item(MenuItem::new_literal("A"))
            .item(MenuItem::new_literal("B"));
        let id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        let h = tree.bounds(id).height;
        // 2 items × 24 = 48 px + padding ≈ 56 px. Much less than the
        // cap of 10 × 24 = 240 px.
        assert!(h < 100.0, "small menu should size to content, got {}", h);
    }
}
