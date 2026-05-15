use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{HStack, IconWidget, Spacer};

const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;

type CommandFactory = Box<dyn Fn(&mut EventContext)>;

struct BreadcrumbEntry {
    label: String,
    action: Option<CommandFactory>,
    current: bool,
}

impl std::fmt::Debug for BreadcrumbEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreadcrumbEntry")
            .field("label", &self.label)
            .field("current", &self.current)
            .finish()
    }
}

/// Breadcrumb design tokens — relocated from
/// `theme.components.breadcrumb` in Stage G of the styling migration.
pub const BREADCRUMB_ITEM_HEIGHT: f32 = 20.0;
pub const BREADCRUMB_ITEM_PADDING_HORIZONTAL: f32 = 6.0;
pub const BREADCRUMB_SEPARATOR_GAP: f32 = 4.0;
pub const BREADCRUMB_CORNER_RADIUS: f32 = 4.0;

/// A single breadcrumb segment definition.
pub struct BreadcrumbItem {
    label: String,
    action: Option<CommandFactory>,
    current: bool,
}

impl std::fmt::Debug for BreadcrumbItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreadcrumbItem")
            .field("label", &self.label)
            .field("current", &self.current)
            .finish()
    }
}

impl BreadcrumbItem {
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            action: None,
            current: false,
        }
    }

    pub fn current(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            action: None,
            current: true,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `current(...)` accepting a raw label.
    #[doc(hidden)]
    pub fn current_literal(label: impl Into<String>) -> Self {
        Self::current(fern_i18n::LocalizedString::literal(label))
    }

    /// Closure invoked on activation.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Box::new(f));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentInteraction {
    Idle,
    Hovered,
    Focused,
}

struct BreadcrumbSegment {
    label: String,
    action: Option<CommandFactory>,
    current: bool,
    interaction: Signal<SegmentInteraction>,
}

impl std::fmt::Debug for BreadcrumbSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreadcrumbSegment")
            .field("label", &self.label)
            .field("current", &self.current)
            .field("interaction", &self.interaction.get())
            .finish()
    }
}

impl BreadcrumbSegment {
    fn new(label: String, action: Option<CommandFactory>, current: bool) -> Self {
        Self {
            label,
            action,
            current,
            interaction: Signal::new(SegmentInteraction::Idle),
        }
    }

    fn is_interactive(&self) -> bool {
        !self.current && self.action.is_some()
    }

    fn estimate_width(&self, ctx: &LayoutContext) -> f32 {
        let pad_h = BREADCRUMB_ITEM_PADDING_HORIZONTAL;
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let text_width = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&self.label, &ctx.theme.typography.small, None)
                .width
        } else {
            self.label.len() as f32 * FALLBACK_CHAR_WIDTH
        };
        text_width + pad_h * 2.0 + envelope * 2.0
    }
}

impl Widget for BreadcrumbSegment {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let interaction = ctx.signal(SegmentInteraction::Idle);
        let registry = ctx.binding_registry();
        interaction.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.interaction = interaction.clone();

        let interactive = self.is_interactive();
        let action = self.action.take();
        let action_rc = std::rc::Rc::new(action);
        let action_for_tap = action_rc.clone();
        let action_for_key = action_rc.clone();
        let action_for_access = action_rc.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                let interaction = interaction.clone();
                move |_pos, ctx: &mut EventContext| {
                    if !interactive {
                        return;
                    }
                    if let Some(ref action) = *action_for_tap {
                        action(ctx);
                    }
                    interaction.set(SegmentInteraction::Hovered);
                }
            })
            .on_hover({
                let interaction = interaction.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    if !interactive {
                        interaction.set(SegmentInteraction::Idle);
                        return;
                    }
                    if interaction.get() == SegmentInteraction::Focused {
                        return;
                    }
                    interaction.set(if entered {
                        SegmentInteraction::Hovered
                    } else {
                        SegmentInteraction::Idle
                    });
                }
            })
            .on_focus({
                let interaction = interaction.clone();
                move |gained: bool, _ctx: &mut EventContext| {
                    if !interactive {
                        interaction.set(SegmentInteraction::Idle);
                        return;
                    }
                    interaction.set(if gained {
                        SegmentInteraction::Focused
                    } else {
                        SegmentInteraction::Idle
                    });
                }
            })
            .on_key({
                let interaction = interaction.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !interactive {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            if let Some(ref action) = *action_for_key {
                                action(ctx);
                            }
                            interaction.set(SegmentInteraction::Focused);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_access_action(move |action, ctx: &mut EventContext| {
                if interactive && action == fern_core::accesskit::Action::Click {
                    if let Some(ref action) = *action_for_access {
                        action(ctx);
                    }
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            })
            .focusable(interactive)
            .cursor(if interactive {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });

        ctx.apply_self_handlers(handler_set);
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let width = proposal.width.unwrap_or_else(|| self.estimate_width(ctx));
        let text_height = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&self.label, &ctx.theme.typography.small, None)
                .height
        } else {
            FALLBACK_LINE_HEIGHT
        };
        let visual_h = text_height.max(BREADCRUMB_ITEM_HEIGHT);
        Size::new(width, visual_h + envelope * 2.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;
        let shape = &ctx.theme.shape;
        let envelope = shape.focus_ring_offset + shape.focus_ring_width;
        let interaction = self.interaction.get();
        let interactive = self.is_interactive();

        // Visual bounds — inset by the focus-ring envelope.
        let visual = Rect::new(
            bounds.x + envelope,
            bounds.y + envelope,
            (bounds.width - envelope * 2.0).max(0.0),
            (bounds.height - envelope * 2.0).max(0.0),
        );

        if interactive {
            let background = if interaction == SegmentInteraction::Hovered {
                colors.accent.with_alpha(0.08)
            } else if interaction == SegmentInteraction::Focused {
                colors.accent.with_alpha(0.12)
            } else {
                Color::TRANSPARENT
            };
            if background.a() > 0.0 {
                canvas.fill_rounded_rect(
                    visual,
                    CornerRadius::uniform(BREADCRUMB_CORNER_RADIUS),
                    background,
                );
            }
            // Focus ring — drawn outside the visual, inside the reserved envelope.
            if interaction == SegmentInteraction::Focused {
                let half_stroke = shape.focus_ring_width * 0.5;
                let ring_rect = Rect::new(
                    bounds.x + half_stroke,
                    bounds.y + half_stroke,
                    (bounds.width - half_stroke * 2.0).max(0.0),
                    (bounds.height - half_stroke * 2.0).max(0.0),
                );
                let ring_radius = BREADCRUMB_CORNER_RADIUS + shape.focus_ring_offset + half_stroke;
                canvas.stroke_rounded_rect(
                    ring_rect,
                    CornerRadius::uniform(ring_radius),
                    colors.focus_ring,
                    shape.focus_ring_width,
                );
            }
        }

        let text_color = if self.current {
            colors.text_primary
        } else if interactive && interaction == SegmentInteraction::Hovered {
            colors.accent_hover
        } else if interactive {
            colors.accent
        } else {
            colors.text_secondary
        };

        let pad_h = BREADCRUMB_ITEM_PADDING_HORIZONTAL;
        let text_bounds = Rect::new(
            visual.x + pad_h,
            visual.y,
            (visual.width - pad_h * 2.0).max(0.0),
            visual.height,
        );
        canvas.draw_text(
            &self.label,
            text_bounds,
            &ctx.theme.typography.small,
            text_color,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Every crumb keeps Role::Link — ARIA convention is that the
        // current page is still announced as a link, just tagged with
        // `aria-current="page"` so screen readers say "current page,
        // <label>". Replaces the earlier Label-role + synthesized
        // i18n `set_value` workaround which didn't map to a standard
        // ARIA pattern.
        builder.set_role(fern_core::accesskit::Role::Link);
        builder.set_name(&self.label);
        if self.current {
            builder.set_aria_current(fern_core::accesskit::AriaCurrent::Page);
        } else if self.is_interactive() {
            builder.add_action(fern_core::accesskit::Action::Click);
            builder.add_action(fern_core::accesskit::Action::Focus);
        }
    }
}

#[derive(Debug)]
struct BreadcrumbSeparator;

impl Widget for BreadcrumbSeparator {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let _ = ctx;
        Size::new(BREADCRUMB_SEPARATOR_GAP * 3.0, BREADCRUMB_ITEM_HEIGHT).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let size = 10.0;
        let icon_bounds = Rect::new(
            bounds.x + (bounds.width - size) / 2.0,
            bounds.y + (bounds.height - size) / 2.0,
            size,
            size,
        );
        // Role-based: IconWidget resolves against the current theme at paint,
        // so this stays reactive across theme switches without a build-time
        // capture.
        let icon = IconWidget::chevron_right(size).color(fern_tokens::TextRole::Secondary);
        icon.paint(icon_bounds, canvas, ctx);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Decorative chevron between crumbs. Screen readers would
        // otherwise enumerate a generic container between every pair
        // of links; `set_hidden()` keeps the node in the layout tree
        // but removes it from the platform a11y tree.
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }
}

enum BreadcrumbSlot {
    Entry(BreadcrumbEntry),
    Id(WidgetId),
}

/// A breadcrumb navigation row.
pub struct Breadcrumb {
    slots: Vec<BreadcrumbSlot>,
    trailing_slot: Option<PendingChild>,
    label: Option<String>,
    root_child_id: Option<WidgetId>,
}

impl Breadcrumb {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            trailing_slot: None,
            label: None,
            root_child_id: None,
        }
    }

    /// Accessible name for the `Navigation` landmark — distinguishes
    /// this breadcrumb from other nav landmarks on the page
    /// (e.g. "Files", "Settings"). Screen readers announce it as the
    /// name of the landmark when it gains focus or is summoned.
    pub fn label(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn label_literal(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    pub fn item(mut self, item: BreadcrumbItem) -> Self {
        self.slots.push(BreadcrumbSlot::Entry(BreadcrumbEntry {
            label: item.label,
            action: item.action,
            current: item.current,
        }));
        self
    }

    /// Insert a pre-registered widget as a breadcrumb segment slot.
    /// The caller is responsible for the segment's visual + interaction.
    pub fn item_id(mut self, id: WidgetId) -> Self {
        self.slots.push(BreadcrumbSlot::Id(id));
        self
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

impl Default for Breadcrumb {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Breadcrumb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Breadcrumb")
            .field("item_count", &self.slots.len())
            .finish()
    }
}

impl Widget for Breadcrumb {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let slots = std::mem::take(&mut self.slots);
        let slot_count = slots.len();
        let mut row = HStack::new().spacing(4.0);

        for (index, slot) in slots.into_iter().enumerate() {
            match slot {
                BreadcrumbSlot::Entry(entry) => {
                    row = row.child(BreadcrumbSegment::new(
                        entry.label,
                        entry.action,
                        entry.current,
                    ));
                }
                BreadcrumbSlot::Id(id) => {
                    row = row.add_child(id);
                }
            }
            if index + 1 < slot_count {
                row = row.child(BreadcrumbSeparator);
            }
        }

        if let Some(trailing) = self.trailing_slot.take() {
            let trailing_id = match trailing {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            row = row.child(Spacer::new()).add_child(trailing_id);
        }

        let root_id = ctx.add(row);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        self.root_child_id
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
        builder.set_role(fern_core::accesskit::Role::Navigation);
        if let Some(ref label) = self.label {
            builder.set_name(label.clone());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
