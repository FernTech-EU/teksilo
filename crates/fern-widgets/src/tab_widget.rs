use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::binding::BindingLevel;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{Expand, HStack, Switcher, VStack};
use crate::scroll_area::{ScrollArea, ScrollBarPolicy, ScrollBarMode};

const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;
const HEADER_MIN_WIDTH: f32 = 72.0;
const HEADER_PADDING_V: f32 = 6.0;

struct TabEntry {
    label: String,
    content: PendingChild,
    enabled: bool,
}

/// A single tab definition used by `TabWidget`.
pub struct TabItem {
    label: String,
    content: PendingChild,
    enabled: bool,
}

impl std::fmt::Debug for TabItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabItem")
            .field("label", &self.label)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl TabItem {
    pub fn new(
        label: impl Into<fern_i18n::LocalizedString>,
        content: impl Widget + 'static,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            content: PendingChild::Deferred(Box::new(content)),
            enabled: true,
        }
    }

    /// Construct from a pre-registered content widget id.
    pub fn from_id(
        label: impl Into<fern_i18n::LocalizedString>,
        id: WidgetId,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            content: PendingChild::Id(id),
            enabled: true,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>, content: impl Widget + 'static) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label), content)
    }

    /// Shim for `from_id(...)` accepting a raw string label.
    #[doc(hidden)]
    pub fn from_id_literal(label: impl Into<String>, id: WidgetId) -> Self {
        Self::from_id(fern_i18n::LocalizedString::literal(label), id)
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabHeaderInteraction {
    Idle,
    Hovered,
}

#[derive(Debug)]
struct TabPane {
    label: String,
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
}

impl TabPane {
    fn new(label: String, child: PendingChild) -> Self {
        Self {
            label,
            child_id: None,
            pending_child: Some(child),
        }
    }
}

impl Widget for TabPane {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0)).into()
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
        builder.set_role(fern_core::accesskit::Role::TabPanel);
        builder.set_name(&self.label);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[derive(Debug)]
struct TabHeader {
    label: String,
    index: usize,
    enabled: bool,
    selected: Signal<usize>,
    header_ids: Rc<RefCell<Vec<WidgetId>>>,
    /// Shared buffer of the matching TabPanel widget ids, populated
    /// during TabWidget's build(). Read in `accessibility()` to
    /// publish the Tab→TabPanel `aria-controls` relation.
    panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    enabled_tabs: Rc<Vec<bool>>,
    interaction: Signal<TabHeaderInteraction>,
    /// Focus origin at the moment focus was gained. The focus ring only
    /// paints when this is `Some(Keyboard)` — pointer-clicking a tab moves
    /// focus to it but must not show the ring, matching IntelliJ's and
    /// VS Code's behavior. Follows the same pattern used by
    /// `SegmentedControl`, `Slider`, and `Toggle`.
    focus_origin: Signal<Option<fern_core::focus::FocusOrigin>>,
}

impl TabHeader {
    fn new(
        label: String,
        index: usize,
        enabled: bool,
        selected: Signal<usize>,
        header_ids: Rc<RefCell<Vec<WidgetId>>>,
        panel_ids: Rc<RefCell<Vec<WidgetId>>>,
        enabled_tabs: Rc<Vec<bool>>,
    ) -> Self {
        Self {
            label,
            index,
            enabled,
            selected,
            header_ids,
            panel_ids,
            enabled_tabs,
            interaction: Signal::new(TabHeaderInteraction::Idle),
            focus_origin: Signal::new(None),
        }
    }

    fn estimate_width(&self, ctx: &LayoutContext) -> f32 {
        let pad_h = ctx.theme.components.tab.padding_horizontal;
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let text_width = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&self.label, &ctx.theme.typography.small, None)
                .width
        } else {
            self.label.len() as f32 * FALLBACK_CHAR_WIDTH
        };
        // Reserve the focus-ring envelope on both sides so the ring isn't clipped.
        (text_width + pad_h * 2.0 + envelope * 2.0).max(HEADER_MIN_WIDTH + envelope * 2.0)
    }
}

impl Widget for TabHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let interaction = ctx.signal(TabHeaderInteraction::Idle);
        let focus_origin: Signal<Option<fern_core::focus::FocusOrigin>> = ctx.signal(None);
        let registry = ctx.binding_registry();

        self.selected
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        interaction.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        focus_origin.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.interaction = interaction.clone();
        self.focus_origin = focus_origin.clone();

        let index = self.index;
        let enabled = self.enabled;
        let selected = self.selected.clone();
        let header_ids = self.header_ids.clone();
        let enabled_tabs = self.enabled_tabs.clone();

        // Shared hover flag the focus handler reads to decide the origin:
        // if the pointer is over the tab at the moment focus is gained,
        // the focus came from a click and we mark it as `Pointer`.
        // Otherwise we assume `Keyboard`. This matches how SegmentedControl,
        // Slider, and Toggle handle it.
        let handler_set = HandlerSet::new()
            .on_tap(move |_pos, _ctx: &mut EventContext| {
                if enabled {
                    selected.set(index);
                }
            })
            .on_hover({
                let interaction = interaction.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        interaction.set(TabHeaderInteraction::Idle);
                        return;
                    }
                    interaction.set(if entered {
                        TabHeaderInteraction::Hovered
                    } else {
                        TabHeaderInteraction::Idle
                    });
                }
            })
            .on_focus({
                let focus_origin = focus_origin.clone();
                let interaction_for_focus = interaction.clone();
                move |gained: bool, _ctx: &mut EventContext| {
                    if !enabled || !gained {
                        focus_origin.set(None);
                        return;
                    }
                    // If the pointer is currently over this tab, focus came
                    // from a click — mark as Pointer so paint() skips the
                    // focus ring. Otherwise treat it as a keyboard-driven
                    // focus.
                    let origin = if interaction_for_focus.get() == TabHeaderInteraction::Hovered {
                        fern_core::focus::FocusOrigin::Pointer
                    } else {
                        fern_core::focus::FocusOrigin::Keyboard
                    };
                    focus_origin.set(Some(origin));
                }
            })
            .on_key({
                let selected = self.selected.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    let headers = header_ids.borrow();
                    if headers.is_empty() {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            let next = next_enabled_index(&enabled_tabs, index, 1);
                            selected.set(next);
                            ctx.request_focus(headers[next]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            let next = next_enabled_index(&enabled_tabs, index, -1);
                            selected.set(next);
                            ctx.request_focus(headers[next]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            selected.set(index);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_access_action({
                let selected = self.selected.clone();
                move |action, _ctx: &mut EventContext| {
                    if enabled && action == fern_core::accesskit::Action::Click {
                        selected.set(index);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .focusable(enabled)
            .cursor(if enabled {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });

        ctx.apply_self_handlers(handler_set);

        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        let tab_style = ctx.theme.components.tab;
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        // Reserve the focus-ring envelope around the visual tab.
        let min_height = tab_style.editor_tab_height + envelope * 2.0;
        let width = proposal.width.unwrap_or_else(|| self.estimate_width(ctx));
        let height = if let Some(backend) = ctx.text_backend {
            let text_height = backend
                .borrow_mut()
                .layout_single_line(&self.label, &ctx.theme.typography.small, None)
                .height;
            (text_height + HEADER_PADDING_V * 2.0 + envelope * 2.0).max(min_height)
        } else {
            (FALLBACK_LINE_HEIGHT + HEADER_PADDING_V * 2.0 + envelope * 2.0).max(min_height)
        };
        Size::new(width, proposal.height.unwrap_or(height)).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Int UI tab visual — IntelliJ new UI / VS Code convention:
        //
        //   * Selected, enabled:
        //       background = `surface_content` (same fill as the pane
        //         below, so the tab "merges" into the content area)
        //       label      = `text_primary`
        //       indicator  = 1 dp `accent` bar at the **top** edge
        //       bottom     = the tab's `surface_content` fill extends
        //         all the way to `bounds.bottom`, overpainting the
        //         TabBar's own 1 dp separator so there is NO visible
        //         bottom border under the selected tab — the tab and
        //         the content pane read as one continuous surface.
        //
        //   * Unselected, hovered:
        //       background = `surface_hover`, inset from the envelope
        //       label      = `text_primary`
        //       bottom     = separator remains visible
        //
        //   * Unselected, idle:
        //       background = TRANSPARENT
        //       label      = `text_secondary`
        //       bottom     = separator remains visible
        //
        //   * Disabled: TRANSPARENT background, `text_disabled` label,
        //     no top indicator, separator remains visible.
        //
        //   * Focus ring: 2 dp `focus_ring` stroke drawn outside the
        //     reserved envelope — but **only on keyboard focus**. A
        //     click-to-focus does not trigger the ring.

        let selected = self.selected.get() == self.index;
        let interaction = self.interaction.get();
        let colors = &ctx.theme.colors;
        let tab_style = ctx.theme.components.tab;
        let shape = &ctx.theme.shape;
        let pad_h = tab_style.padding_horizontal;
        let top_indicator = shape.border_width;

        // Envelope reserves space for the keyboard focus ring on all
        // four sides. The visual rect is the symmetric inset — the label
        // never shifts, and selected-tab bottom-border erasure is handled
        // by the TabBar drawing its separator *inside* the visual rect
        // (at `bounds.bottom - envelope - 1`) so the selected tab's fill
        // naturally covers it without any out-of-bounds painting.
        let envelope = shape.focus_ring_offset + shape.focus_ring_width;
        let visual = Rect::new(
            bounds.x + envelope,
            bounds.y + envelope,
            (bounds.width - envelope * 2.0).max(0.0),
            (bounds.height - envelope * 2.0).max(0.0),
        );

        // Background fill.
        let background = if !self.enabled {
            Color::TRANSPARENT
        } else if selected {
            colors.surface_content
        } else if interaction == TabHeaderInteraction::Hovered {
            colors.surface_hover
        } else {
            Color::TRANSPARENT
        };
        if background.a() > 0.0 {
            canvas.fill_rect(visual, background);
        }

        // 1 dp accent indicator along the top edge of the selected tab.
        // Drawn after the fill so it sits on top of it.
        if selected && self.enabled {
            let indicator = Rect::new(visual.x, visual.y, visual.width, top_indicator);
            canvas.fill_rect(indicator, colors.accent);
        }

        let text_color = if !self.enabled {
            colors.text_disabled
        } else if selected || interaction == TabHeaderInteraction::Hovered {
            colors.text_primary
        } else {
            colors.text_secondary
        };
        let text_rect = Rect::new(
            visual.x + pad_h,
            visual.y + HEADER_PADDING_V,
            (visual.width - pad_h * 2.0).max(0.0),
            (visual.height - HEADER_PADDING_V * 2.0).max(0.0),
        );
        canvas.draw_text(
            &self.label,
            text_rect,
            &ctx.theme.typography.small,
            text_color,
        );

        // Focus ring — ONLY on keyboard focus. Click-to-focus sets
        // `focus_origin = Pointer` and this branch is skipped.
        if self.focus_origin.get() == Some(fern_core::focus::FocusOrigin::Keyboard) {
            let half_stroke = shape.focus_ring_width * 0.5;
            let ring_rect = Rect::new(
                bounds.x + half_stroke,
                bounds.y + half_stroke,
                (bounds.width - half_stroke * 2.0).max(0.0),
                (bounds.height - half_stroke * 2.0).max(0.0),
            );
            let ring_radius =
                shape.radius_control + shape.focus_ring_offset + half_stroke;
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(ring_radius),
                colors.focus_ring,
                shape.focus_ring_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Tab);
        builder.set_name(&self.label);
        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(fern_core::accesskit::Action::Click);
        }
        builder.add_action(fern_core::accesskit::Action::Focus);
        builder.set_selected(self.selected.get() == self.index);
        // Publish the Tab -> TabPanel relation. AccessKit / ARIA models
        // this via `controls`: the tab "controls" the panel that becomes
        // visible when it's activated. Read from the shared buffer the
        // parent TabWidget populated during build().
        if let Some(&panel_id) = self.panel_ids.borrow().get(self.index) {
            builder.push_controlled(fern_core::accessibility::widget_id_to_node_id(panel_id));
        }
    }
}

fn next_enabled_index(enabled_tabs: &[bool], current: usize, direction: isize) -> usize {
    if enabled_tabs.is_empty() {
        return current;
    }

    let len = enabled_tabs.len() as isize;
    let mut offset = 1_isize;
    while offset <= len {
        let candidate = (current as isize + direction * offset).rem_euclid(len) as usize;
        if enabled_tabs[candidate] {
            return candidate;
        }
        offset += 1;
    }
    current
}

#[derive(Debug)]
struct TabBar {
    header_ids: Vec<WidgetId>,
    trailing_child_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
}

impl TabBar {
    fn new(header_ids: Vec<WidgetId>, trailing_child_id: Option<WidgetId>) -> Self {
        Self {
            header_ids,
            trailing_child_id,
            root_child_id: None,
        }
    }
}

impl Widget for TabBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut headers = HStack::new().spacing(4.0);
        for &header_id in &self.header_ids {
            headers = headers.add_child(header_id);
        }

        // TabHeader's intrinsic height is `editor_tab_height + envelope*2`
        // — it reserves the focus-ring envelope on all four sides. The
        // ScrollArea's preferred height must match, or the 38-dp-tall
        // headers get clipped inside a 30-dp viewport, causing visible
        // pixel shifts in labels whenever layout is re-run. The snapshot
        // is read once at build time because the enclosing ScrollArea
        // keeps its preferred size frozen; theme-driven size changes are
        // picked up on the next rebuild.
        let snapshot = ctx.theme_signal().get();
        let shape = &snapshot.shape;
        let envelope = shape.focus_ring_offset + shape.focus_ring_width;
        let header_min_height = snapshot.components.tab.editor_tab_height + envelope * 2.0;
        let headers_scroll_id = ctx.add(
            ScrollArea::new()
                .child(headers)
                .scroll_bar_style(ScrollBarMode::Overlay)
                .vertical_scroll_bar_policy(ScrollBarPolicy::AlwaysOff)
                .horizontal_scroll_bar_policy(ScrollBarPolicy::AsNeeded)
                .widget_resizable(true)
                .preferred_size(0.0, header_min_height),
        );

        let mut row = HStack::new().spacing(8.0).child(
            Expand::horizontal()
                
                .child_id(headers_scroll_id),
        );

        if let Some(trailing_child_id) = self.trailing_child_id {
            row = row.add_child(trailing_child_id);
        }

        let root_id = ctx.add(row);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0)).into()
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
        // Int UI tab bar is a flat row on the same surface as the content.
        // The only chrome is a 1 dp horizontal separator that runs along
        // the bottom of the **visual** tab row. Each TabHeader reserves a
        // focus-ring envelope on all four sides, so the visual row bottom
        // sits at `bounds.bottom - envelope`, not at `bounds.bottom`.
        // Drawing the separator there places it *inside* the headers'
        // visual rect — the selected header's `surface_content` fill then
        // overpaints it in its own column, producing the "tab merges into
        // content pane" effect without depending on out-of-bounds painting.
        let border_width = ctx.theme.shape.border_width;
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let separator = fern_canvas::Rect::new(
            bounds.x,
            (bounds.bottom() - envelope - border_width).max(bounds.y),
            bounds.width,
            border_width,
        );
        canvas.fill_rect(separator, ctx.theme.colors.border);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::TabList);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// A tabbed container with keyboard navigation, a trailing action slot,
/// and dormant content panes backed by `Switcher`.
pub struct TabWidget {
    selected: Signal<usize>,
    entries: Vec<TabEntry>,
    trailing_slot: Option<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl TabWidget {
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            selected,
            entries: Vec::new(),
            trailing_slot: None,
            root_child_id: None,
        }
    }

    pub fn tab(
        mut self,
        label: impl Into<fern_i18n::LocalizedString>,
        content: impl Widget + 'static,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.entries.push(TabEntry {
            label: ls.resolve_now(),
            content: PendingChild::Deferred(Box::new(content)),
            enabled: true,
        });
        self
    }

    /// Add a tab whose content is a pre-registered widget id.
    pub fn tab_id(
        mut self,
        label: impl Into<fern_i18n::LocalizedString>,
        content_id: WidgetId,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.entries.push(TabEntry {
            label: ls.resolve_now(),
            content: PendingChild::Id(content_id),
            enabled: true,
        });
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tab(...)` accepting a raw label.
    #[doc(hidden)]
    pub fn tab_literal(
        self,
        label: impl Into<String>,
        content: impl Widget + 'static,
    ) -> Self {
        self.tab(fern_i18n::LocalizedString::literal(label), content)
    }

    /// Shim for `tab_id(...)` accepting a raw label.
    #[doc(hidden)]
    pub fn tab_literal_id(self, label: impl Into<String>, content_id: WidgetId) -> Self {
        self.tab_id(fern_i18n::LocalizedString::literal(label), content_id)
    }

    pub fn tab_item(mut self, item: TabItem) -> Self {
        self.entries.push(TabEntry {
            label: item.label,
            content: item.content,
            enabled: item.enabled,
        });
        self
    }

    /// Alias for `tab_item(...)` accepting a `TabItem` constructed from
    /// a pre-registered widget id via `TabItem::from_id`.
    pub fn tab_item_id(self, item: TabItem) -> Self {
        self.tab_item(item)
    }

    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn trailing_slot_id(mut self, id: WidgetId) -> Self {
        self.trailing_slot = Some(PendingChild::Id(id));
        self
    }
}

impl std::fmt::Debug for TabWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabWidget")
            .field("tab_count", &self.entries.len())
            .finish()
    }
}

impl Widget for TabWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let entries = std::mem::take(&mut self.entries);
        let enabled_tabs = Rc::new(
            entries
                .iter()
                .map(|entry| entry.enabled)
                .collect::<Vec<_>>(),
        );
        let mut header_ids = Vec::with_capacity(entries.len());
        let shared_header_ids = Rc::new(RefCell::new(Vec::with_capacity(entries.len())));
        // Parallel buffer for TabPanel ids. Switcher's build() pushes
        // each child's WidgetId into this buffer as it adds it to the
        // arena (see `Switcher::capture_child_ids_into`). TabHeader's
        // `accessibility()` reads `panel_ids[index]` to publish the
        // tab -> panel `controls` relation.
        let shared_panel_ids = Rc::new(RefCell::new(Vec::with_capacity(entries.len())));
        let mut switcher =
            Switcher::new(self.selected.clone()).capture_child_ids_into(shared_panel_ids.clone());

        for (index, entry) in entries.into_iter().enumerate() {
            let header_id = ctx.add(TabHeader::new(
                entry.label.clone(),
                index,
                entry.enabled,
                self.selected.clone(),
                shared_header_ids.clone(),
                shared_panel_ids.clone(),
                enabled_tabs.clone(),
            ));
            header_ids.push(header_id);
            shared_header_ids.borrow_mut().push(header_id);

            switcher = switcher.child_boxed(Box::new(TabPane::new(entry.label, entry.content)));
        }

        let trailing_child_id = self.trailing_slot.take().map(|pending| match pending {
            PendingChild::Id(id) => id,
            PendingChild::Deferred(w) => ctx.add_boxed(w),
        });
        let tab_bar_id = ctx.add(TabBar::new(header_ids, trailing_child_id));
        let switcher_id = ctx.add(switcher);
        // Content sits flush under the tab bar — the TabBar paints its own
        // 1 dp bottom separator, and selected tabs overpaint that with
        // their 3 dp underline. No inset, no extra divider.
        let content_id = ctx.add(Expand::vertical().child_id(switcher_id));

        let root_id = ctx.add(
            VStack::new()
                .add_child(tab_bar_id)
                .add_child(content_id),
        );

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0)).into()
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
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod a11y_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[derive(Debug)]
    struct BuildCountingLeaf {
        build_count: Rc<Cell<usize>>,
    }

    impl Widget for BuildCountingLeaf {
        fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
            self.build_count.set(self.build_count.get() + 1);
            Vec::new()
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
            proposal.resolve(120.0, 48.0).into()
        }
    }

    fn header_id(tree: &WidgetTree, tabs_id: WidgetId, index: usize) -> WidgetId {
        let root = tree.child_widget(tabs_id, 0);
        let tab_bar = tree.child_widget(root, 0);
        let row = tree.child_widget(tab_bar, 0);
        let expand = tree.child_widget(row, 0);
        let scroll = tree.child_widget(expand, 0);
        let headers = tree.child_widget(scroll, 0);
        tree.child_widget(headers, index)
    }

    fn switcher_id(tree: &WidgetTree, tabs_id: WidgetId) -> WidgetId {
        // Root VStack now has two children: tab bar (index 0) and the
        // content Expand (index 1). The padding wrapper was dropped —
        // content sits flush under the tab bar's own bottom separator.
        let root = tree.child_widget(tabs_id, 0);
        let expand = tree.child_widget(root, 1);
        tree.child_widget(expand, 0)
    }

    #[test]
    fn access_click_updates_selected_index() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected.clone())
                .tab_literal("Overview", FixedLeaf(120.0, 48.0))
                .tab_literal("Details", FixedLeaf(140.0, 52.0)),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));
        let second_header = header_id(&tree, tabs, 1);
        tree.dispatch_event(WidgetEvent::AccessAction { action: fern_core::accesskit::Action::Click, target: Some(second_header), target_node: fern_core::accessibility::root_node_id(), data: None });

        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn keyboard_navigation_updates_selection_and_focus() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected.clone())
                .tab_literal("Overview", FixedLeaf(120.0, 48.0))
                .tab_literal("Details", FixedLeaf(140.0, 52.0))
                .tab_literal("Activity", FixedLeaf(160.0, 56.0)),
        );

        tree.layout(SizeProposal::exact(640.0, 320.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 1);

        let second_header = header_id(&tree, tabs, 1);
        assert_eq!(tree.focused(), Some(second_header));

        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(selected.get(), 0);
    }

    #[test]
    fn inactive_panes_are_dormant() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected.clone())
                .tab_literal("Overview", FixedLeaf(120.0, 48.0))
                .tab_literal("Details", FixedLeaf(140.0, 52.0)),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));

        let switcher = switcher_id(&tree, tabs);
        let zstack = tree.child_widget(switcher, 0);
        let panes = tree.children(zstack);
        assert_eq!(panes.len(), 2);
        assert!(tree.is_visible(panes[0]));
        assert!(!tree.is_visible(panes[1]));

        selected.set(1);
        tree.layout(SizeProposal::exact(480.0, 240.0));

        assert!(!tree.is_visible(panes[0]));
        assert!(tree.is_visible(panes[1]));
    }

    #[test]
    fn panes_preserve_state_across_switches() {
        let selected = Signal::new(0_usize);
        let first_builds = Rc::new(Cell::new(0));
        let second_builds = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

        tree.add(
            TabWidget::new(selected.clone())
                .tab_literal(
                    "Overview",
                    BuildCountingLeaf {
                        build_count: first_builds.clone(),
                    },
                )
                .tab_literal(
                    "Details",
                    BuildCountingLeaf {
                        build_count: second_builds.clone(),
                    },
                ),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));
        assert_eq!(first_builds.get(), 1);
        assert_eq!(second_builds.get(), 1);

        selected.set(1);
        tree.layout(SizeProposal::exact(480.0, 240.0));
        selected.set(0);
        tree.layout(SizeProposal::exact(480.0, 240.0));

        assert_eq!(first_builds.get(), 1);
        assert_eq!(second_builds.get(), 1);
    }

    #[test]
    fn accessibility_roles_are_exposed() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            TabWidget::new(selected)
                .tab_literal("Overview", FixedLeaf(120.0, 48.0))
                .tab_literal("Details", FixedLeaf(140.0, 52.0)),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));

        let tab_list = tree.find_by_role(fern_core::accesskit::Role::TabList);
        let tab = tree.find_by_role(fern_core::accesskit::Role::Tab);
        let tab_panel = tree.find_by_role(fern_core::accesskit::Role::TabPanel);

        assert!(tab_list.is_some());
        assert!(tab.is_some());
        assert!(tab_panel.is_some());

        let info = tree.accessibility_node(tab.unwrap());
        assert_eq!(info.role(), fern_core::accesskit::Role::Tab);
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }

    #[test]
    fn disabled_tabs_do_not_activate_and_are_skipped_by_keyboard() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected.clone())
                .tab_literal("Overview", FixedLeaf(120.0, 48.0))
                .tab_item(TabItem::new_literal("Locked", FixedLeaf(120.0, 48.0)).enabled(false))
                .tab_literal("Activity", FixedLeaf(120.0, 48.0)),
        );

        tree.layout(SizeProposal::exact(640.0, 320.0));

        let disabled_header = header_id(&tree, tabs, 1);

        tree.click(disabled_header);
        assert_eq!(selected.get(), 0);

        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 2);

        let info = tree.accessibility_node(disabled_header);
        assert_eq!(info.role(), fern_core::accesskit::Role::Tab);
        assert!(
            !info
                .actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }

    #[test]
    fn content_is_positioned_below_tab_strip() {
        // Int UI: no Divider child between the tab bar and the content —
        // the TabBar paints its own 1 dp bottom separator. The VStack now
        // has exactly two children: the TabBar and the content Expand.
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected)
                .tab_literal("Overview", FixedLeaf(120.0, 48.0))
                .tab_literal("Details", FixedLeaf(140.0, 52.0)),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));

        let root = tree.child_widget(tabs, 0);
        let tab_bar = tree.child_widget(root, 0);
        let content_expand = tree.child_widget(root, 1);
        let switcher = tree.child_widget(content_expand, 0);

        let tab_bar_bounds = tree.bounds(tab_bar);
        let switcher_bounds = tree.bounds(switcher);

        assert!(switcher_bounds.y >= tab_bar_bounds.bottom() - 0.01);
    }

    #[test]
    fn tab_bar_wraps_headers_in_horizontal_scroll_area() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected)
                .tab_literal("One", FixedLeaf(120.0, 48.0))
                .tab_literal("Two", FixedLeaf(120.0, 48.0))
                .tab_literal("Three", FixedLeaf(120.0, 48.0))
                .tab_literal("Four", FixedLeaf(120.0, 48.0))
                .tab_literal("Five", FixedLeaf(120.0, 48.0)),
        );

        tree.layout(SizeProposal::exact(220.0, 240.0));

        let root = tree.child_widget(tabs, 0);
        let tab_bar = tree.child_widget(root, 0);
        let row = tree.child_widget(tab_bar, 0);
        let expand = tree.child_widget(row, 0);
        let scroll = tree.child_widget(expand, 0);
        let info = tree.accessibility_node(scroll);

        assert_eq!(info.role(), fern_core::accesskit::Role::ScrollView);
    }
}
