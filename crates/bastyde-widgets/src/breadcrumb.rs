use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::HasPopup;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::environment::LayoutDirection;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::OverlayPlacement;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{Color, CornerRadius};

use crate::button::{Button, ButtonVariant};
use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;
use crate::popover_widget::PopoverButton;
use crate::primitives::{HStack, IconWidget, Spacer};
use bastyde_i18n::LocalizedString;

const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;

/// Shared activation closure. `Rc` (not `Box`) so a crumb's action can be
/// fired from BOTH its inline segment AND its row in the overflow menu.
type CommandFactory = Rc<dyn Fn(&mut EventContext)>;

struct BreadcrumbEntry {
    label: LocalizedString,
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

/// Breadcrumb design tokens.
pub const BREADCRUMB_ITEM_HEIGHT: f32 = 20.0;
pub const BREADCRUMB_ITEM_PADDING_HORIZONTAL: f32 = 6.0;
pub const BREADCRUMB_SEPARATOR_GAP: f32 = 4.0;
pub const BREADCRUMB_CORNER_RADIUS: f32 = 4.0;

/// A single breadcrumb segment definition.
pub struct BreadcrumbItem {
    label: LocalizedString,
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
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            action: None,
            current: false,
        }
    }

    pub fn current(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            action: None,
            current: true,
        }
    }

    /// Closure invoked on activation.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Rc::new(f));
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
    label: LocalizedString,
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
    fn new(label: LocalizedString, action: Option<CommandFactory>, current: bool) -> Self {
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
        let resolved = self.label.resolve_now();
        let text_width = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&resolved, &ctx.theme.typography.small, None)
                .width
        } else {
            resolved.len() as f32 * FALLBACK_CHAR_WIDTH
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
        // Locale changes can alter the resolved label (and its width), so
        // re-measure + repaint this custom-painted segment on locale switch.
        ctx.locale_signal()
            .bind_to(self_id, registry, BindingLevel::Relayout);
        self.interaction = interaction.clone();

        let interactive = self.is_interactive();
        let action = self.action.take();
        let action_for_tap = action.clone();
        let action_for_key = action.clone();
        let action_for_access = action;

        let handler_set = HandlerSet::new()
            .on_tap({
                let interaction = interaction.clone();
                move |_pos, ctx: &mut EventContext| {
                    if !interactive {
                        return;
                    }
                    if let Some(ref action) = action_for_tap {
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
                            if let Some(ref action) = action_for_key {
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
                if interactive && action == bastyde_core::accesskit::Action::Click {
                    if let Some(ref action) = action_for_access {
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
    ) -> bastyde_core::widget::LayoutResponse {
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let width = proposal.width.unwrap_or_else(|| self.estimate_width(ctx));
        let text_height = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&self.label.resolve_now(), &ctx.theme.typography.small, None)
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
            &self.label.resolve_now(),
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
        builder.set_role(bastyde_core::accesskit::Role::Link);
        builder.set_name(self.label.resolve_now());
        if self.current {
            builder.set_aria_current(bastyde_core::accesskit::AriaCurrent::Page);
        } else if self.is_interactive() {
            builder.add_action(bastyde_core::accesskit::Action::Click);
            builder.add_action(bastyde_core::accesskit::Action::Focus);
        }
    }
}

#[derive(Debug)]
struct BreadcrumbSeparator;

impl Widget for BreadcrumbSeparator {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Repaint on locale change so the chevron can flip with the layout
        // direction (it points toward the next crumb: right in LTR, left in
        // RTL).
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        ctx.locale_signal()
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        // so this stays reactive across theme switches. The chevron mirrors
        // under RTL — it always points toward the *next* crumb.
        let icon = if ctx.layout_direction == LayoutDirection::RightToLeft {
            IconWidget::chevron_left(size)
        } else {
            IconWidget::chevron_right(size)
        }
        .color(bastyde_tokens::TextRole::Secondary);
        icon.paint(icon_bounds, canvas, ctx);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Decorative chevron between crumbs. Screen readers would
        // otherwise enumerate a generic container between every pair
        // of links; `set_hidden()` keeps the node in the layout tree
        // but removes it from the platform a11y tree.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }
}

enum BreadcrumbSlot {
    Entry(BreadcrumbEntry),
    Id(WidgetId),
}

/// Menu-form of a collapsible crumb — the row shown in the overflow `…` menu.
struct CrumbMenuForm {
    slot: usize,
    label: LocalizedString,
    action: Option<CommandFactory>,
}

/// A breadcrumb navigation row with **automatic overflow**: when the trail is
/// too wide, the middle crumbs collapse into a trailing-of-root `…` menu while
/// the root and the current (last) crumb stay visible — the standard breadcrumb
/// collapse (Windows Explorer / web breadcrumbs / macOS path bar).
pub struct Breadcrumb {
    slots: Vec<BreadcrumbSlot>,
    trailing_slot: Option<PendingChild>,
    label: Option<LocalizedString>,

    // Reactive state.
    /// Per-slot collapsed flag (`true` = hidden in the `…` menu). Only
    /// collapsible slots are ever set; index-aligned with the slots.
    collapsed: Signal<Vec<bool>>,
    /// Whether any crumb is currently collapsed (drives the chevron).
    is_overflowing: Signal<bool>,

    // Build state.
    /// Per-slot "unit" id (the slot's segment, plus its leading separator for
    /// slots after the first) — measured to compute overflow.
    unit_ids: Vec<WidgetId>,
    /// Per-slot: can this crumb collapse? (Entry crumbs that are neither first
    /// nor last; pre-registered `item_id` crumbs never collapse.)
    collapsible: Vec<bool>,
    /// Menu-form per collapsible crumb, for the overflow `…` menu rows.
    menu_forms: Rc<Vec<CrumbMenuForm>>,
    /// The `[separator, …-button]` unit id (measured + gated on overflow).
    ellipsis_unit_id: Option<WidgetId>,
    trailing_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
    /// Cached flags to avoid redundant signal writes.
    last_flags: RefCell<Vec<bool>>,
}

impl Breadcrumb {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            trailing_slot: None,
            label: None,
            collapsed: Signal::new(Vec::new()),
            is_overflowing: Signal::new(false),
            unit_ids: Vec::new(),
            collapsible: Vec::new(),
            menu_forms: Rc::new(Vec::new()),
            ellipsis_unit_id: None,
            trailing_id: None,
            root_child_id: None,
            last_flags: RefCell::new(Vec::new()),
        }
    }

    /// Accessible name for the `Navigation` landmark — distinguishes
    /// this breadcrumb from other nav landmarks on the page
    /// (e.g. "Files", "Settings"). Screen readers announce it as the
    /// name of the landmark when it gains focus or is summoned.
    pub fn label(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.label = Some(ls);
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
    /// Note: a pre-registered crumb never collapses into the overflow menu
    /// (the breadcrumb has no label/action to synthesize a menu row from) —
    /// it is treated like the root/current crumbs as always-visible.
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

    /// Reactive signal that is `true` whenever any crumb is collapsed into the
    /// overflow `…` menu — for adaptive chrome.
    pub fn is_overflowing(&self) -> Signal<bool> {
        self.is_overflowing.clone()
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
        let n = slots.len();

        let mut unit_ids: Vec<WidgetId> = Vec::with_capacity(n);
        let mut collapsible: Vec<bool> = Vec::with_capacity(n);
        let mut menu_forms: Vec<CrumbMenuForm> = Vec::new();

        for (i, slot) in slots.into_iter().enumerate() {
            let is_first = i == 0;
            let is_last = i + 1 == n;

            // Resolve the slot to a segment id + (for Entry slots) a menu form.
            let (seg_id, form): (WidgetId, Option<(LocalizedString, Option<CommandFactory>)>) =
                match slot {
                    BreadcrumbSlot::Entry(entry) => {
                        let action = entry.action;
                        let seg = BreadcrumbSegment::new(
                            entry.label.clone(),
                            action.clone(),
                            entry.current,
                        );
                        (ctx.add(seg), Some((entry.label, action)))
                    }
                    BreadcrumbSlot::Id(id) => (id, None),
                };

            // A crumb can collapse only if it's an Entry and neither the root
            // nor the current (last) crumb.
            let can_collapse = form.is_some() && !is_first && !is_last;
            collapsible.push(can_collapse);

            // The unit is the segment, prefixed by a leading separator for
            // every crumb after the first. The separator lives inside the unit
            // so it hides together with its crumb — no dangling chevrons.
            let unit_id = if is_first {
                seg_id
            } else {
                let sep_id = ctx.add(BreadcrumbSeparator);
                ctx.add(
                    HStack::new()
                        .spacing(0.0)
                        .add_child(sep_id)
                        .add_child(seg_id),
                )
            };
            unit_ids.push(unit_id);

            if can_collapse {
                let collapsed = self.collapsed.clone();
                ctx.visible_when(
                    unit_id,
                    collapsed.map(move |flags| flags.get(i).copied() != Some(true)),
                );
                if let Some((label, action)) = form {
                    menu_forms.push(CrumbMenuForm {
                        slot: i,
                        label,
                        action,
                    });
                }
            }
        }

        self.collapsible = collapsible;
        self.menu_forms = Rc::new(menu_forms);
        self.collapsed.set(vec![false; n]);
        *self.last_flags.borrow_mut() = vec![false; n];

        // Overflow chevron: a `…` PopoverButton (HasPopup::Menu) whose content
        // is a `MenuList` with one row per collapsible crumb, each gated via
        // `item_when(collapsed[slot])`. Only currently-collapsed rows are shown
        // (zero-height + nav-skipped otherwise), so the menu reconciles
        // reactively as the trail resizes — no rebuild of the dormant popover.
        let has_collapsible = self.collapsible.iter().any(|&c| c);
        self.ellipsis_unit_id = if has_collapsible {
            let menu_forms = self.menu_forms.clone();
            let mut menu = MenuList::new();
            for form in menu_forms.iter() {
                let slot = form.slot;
                let action = form.action.clone();
                let mut row = MenuItem::new(form.label.clone()).enabled(action.is_some());
                if let Some(act) = action {
                    row = row.on_activate_fn(move |ctx| {
                        act(ctx);
                        ctx.dismiss_self_overlay_chain();
                    });
                }
                let collapsed = self.collapsed.clone();
                let visible = collapsed.map(move |flags| flags.get(slot).copied() == Some(true));
                menu = menu.item_when(row, visible);
            }

            let trigger = Button::new(bastyde_i18n::lit!("…"))
                .variant(ButtonVariant::Ghost)
                .tooltip(bastyde_i18n::tr_widget!(breadcrumb_overflow()));
            let chevron = PopoverButton::new(trigger)
                .content(menu)
                // `MenuList` self-chromes via the Menu `PopoverStyle`.
                .bare()
                .placement(OverlayPlacement::BelowPreferred)
                .has_popup_kind(HasPopup::Menu);
            let chevron_id = ctx.add(chevron);

            let sep_id = ctx.add(BreadcrumbSeparator);
            let unit_id = ctx.add(
                HStack::new()
                    .spacing(0.0)
                    .add_child(sep_id)
                    .add_child(chevron_id),
            );
            ctx.visible_when(unit_id, self.is_overflowing.clone());
            Some(unit_id)
        } else {
            None
        };

        // Assemble the row: [root] [… (after root)] [crumb 1] … [current]
        // [Spacer trailing?].
        let mut row = HStack::new().spacing(0.0);
        for (i, &uid) in unit_ids.iter().enumerate() {
            row = row.add_child(uid);
            if i == 0 {
                if let Some(eu) = self.ellipsis_unit_id {
                    row = row.add_child(eu);
                }
            }
        }
        self.unit_ids = unit_ids;

        if let Some(trailing) = self.trailing_slot.take() {
            let trailing_id = match trailing {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            self.trailing_id = Some(trailing_id);
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
    ) -> bastyde_core::widget::LayoutResponse {
        let Some(root) = self.root_child_id else {
            return proposal.resolve(0.0, 0.0).into();
        };

        // Natural width = sum of every crumb unit's INTRINSIC width (measured
        // regardless of its current collapse state, so this is stable and the
        // overflow decision can't oscillate). The `…` chevron is excluded — it
        // only appears when something is already collapsed.
        let probe = SizeProposal::unspecified();
        let mut natural_w = 0.0_f32;
        for &uid in &self.unit_ids {
            if let Some(s) = ctx.measure_intrinsic(uid, probe) {
                natural_w += s.width;
            }
        }
        let has_trailing = self.trailing_id.is_some();
        if let Some(tid) = self.trailing_id
            && let Some(s) = ctx.measure_intrinsic(tid, probe)
        {
            natural_w += s.width;
        }

        // With a trailing slot the breadcrumb spans the offered width (the
        // Spacer pushes the trailing control to the edge); otherwise it
        // shrink-wraps to its content, clamped to the offered width so it never
        // spills its container.
        let width = if has_trailing {
            proposal.width.unwrap_or(natural_w)
        } else {
            proposal
                .width
                .map(|w| natural_w.min(w))
                .unwrap_or(natural_w)
        };

        let height = ctx
            .child_size(root, proposal)
            .map(|s| s.height)
            .unwrap_or(BREADCRUMB_ITEM_HEIGHT);
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }

        // Compute the collapse set from intrinsic widths (measured even while
        // hidden) against the width left for the crumbs.
        let probe = SizeProposal::unspecified();
        let trailing_w = self
            .trailing_id
            .and_then(|tid| ctx.measure_intrinsic(tid, probe))
            .map(|s| s.width)
            .unwrap_or(0.0);
        let avail = (bounds.width - trailing_w).max(0.0);

        let unit_w: Vec<f32> = self
            .unit_ids
            .iter()
            .map(|&uid| {
                ctx.measure_intrinsic(uid, probe)
                    .map(|s| s.width)
                    .unwrap_or(0.0)
            })
            .collect();
        let ellipsis_w = self
            .ellipsis_unit_id
            .and_then(|eu| ctx.measure_intrinsic(eu, probe))
            .map(|s| s.width)
            .unwrap_or(0.0);

        let flags = compute_breadcrumb_overflow(avail, &unit_w, &self.collapsible, ellipsis_w);

        if *self.last_flags.borrow() != flags {
            *self.last_flags.borrow_mut() = flags.clone();
            let any = flags.iter().any(|&c| c);
            self.collapsed.set(flags);
            if self.is_overflowing.get() != any {
                self.is_overflowing.set(any);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Navigation);
        if let Some(ref label) = self.label {
            builder.set_name(label.clone());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Decide which crumbs collapse into the overflow `…` menu.
///
/// Returns a per-slot `collapsed` flag (`true` = hidden). The root and current
/// crumbs (and any non-collapsible pre-registered crumb) are kept; collapsible
/// crumbs are hidden from the **left-middle outward** (lowest index first) until
/// the shown crumbs — plus the `…` chevron once anything is hidden — fit in
/// `avail`. If even the kept crumbs + chevron don't fit, the remainder overflows
/// residually (nothing left to collapse).
fn compute_breadcrumb_overflow(
    avail: f32,
    unit_w: &[f32],
    collapsible: &[bool],
    ellipsis_w: f32,
) -> Vec<bool> {
    let n = unit_w.len();
    let mut collapsed = vec![false; n];
    if n == 0 {
        return collapsed;
    }
    let full: f32 = unit_w.iter().sum();
    if full <= avail + 0.5 {
        return collapsed; // everything fits — no chevron
    }
    loop {
        let any_hidden = collapsed.iter().any(|&c| c);
        let shown: f32 = (0..n)
            .filter(|&i| !collapsed[i])
            .map(|i| unit_w[i])
            .sum::<f32>()
            + if any_hidden { ellipsis_w } else { 0.0 };
        if shown <= avail + 0.5 {
            break;
        }
        match (0..n).find(|&i| collapsible[i] && !collapsed[i]) {
            Some(i) => collapsed[i] = true,
            None => break, // nothing left to collapse — residual overflow
        }
    }
    collapsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::MockTextBackend;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    fn themed_tree() -> WidgetTree {
        WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
    }

    fn trail(n: usize) -> Breadcrumb {
        let mut bc = Breadcrumb::new();
        for i in 0..n {
            let last = i + 1 == n;
            let item = if last {
                BreadcrumbItem::current(lit!(format!("Crumb {i}")))
            } else {
                BreadcrumbItem::new(lit!(format!("Crumb {i}"))).on_activate_fn(|_| {})
            };
            bc = bc.item(item);
        }
        bc
    }

    // ── compute_breadcrumb_overflow ──────────────────────────────────────────

    #[test]
    fn nothing_collapses_when_it_all_fits() {
        let flags = compute_breadcrumb_overflow(
            500.0,
            &[40.0, 40.0, 40.0, 40.0, 40.0],
            &[false, true, true, true, false],
            30.0,
        );
        assert_eq!(flags, vec![false; 5]);
    }

    #[test]
    fn middle_collapses_from_the_left_keeping_root_and_current() {
        // full = 200 > 160. hide #1 → 40*4+30=190 > 160; hide #2 → 40*3+30=150 ≤ 160.
        let flags = compute_breadcrumb_overflow(
            160.0,
            &[40.0, 40.0, 40.0, 40.0, 40.0],
            &[false, true, true, true, false],
            30.0,
        );
        assert_eq!(flags, vec![false, true, true, false, false]);
    }

    #[test]
    fn all_middle_collapses_when_very_narrow() {
        let flags = compute_breadcrumb_overflow(
            100.0,
            &[40.0, 40.0, 40.0, 40.0, 40.0],
            &[false, true, true, true, false],
            30.0,
        );
        assert_eq!(
            flags,
            vec![false, true, true, true, false],
            "root and current always survive; all middle collapse"
        );
    }

    #[test]
    fn two_crumbs_never_collapse() {
        let flags = compute_breadcrumb_overflow(10.0, &[40.0, 40.0], &[false, false], 30.0);
        assert_eq!(flags, vec![false, false], "no collapsible middle to hide");
    }

    // ── Integration ──────────────────────────────────────────────────────────

    #[test]
    fn wide_trail_does_not_overflow_narrow_does() {
        let bc = trail(6);
        let overflowing = bc.is_overflowing();
        let mut tree = themed_tree();
        let _id = tree.add(bc);

        tree.layout(SizeProposal::exact(2000.0, 30.0));
        assert!(!overflowing.get(), "a wide trail should not overflow");

        tree.layout(SizeProposal::exact(160.0, 30.0));
        assert!(
            overflowing.get(),
            "a narrow trail should collapse middle crumbs into the … menu"
        );

        tree.layout(SizeProposal::exact(2000.0, 30.0));
        assert!(
            !overflowing.get(),
            "re-widening restores all crumbs (intrinsic measure → no stale collapse)"
        );
    }

    #[test]
    fn overflow_menu_rows_are_dormant_while_the_chevron_is_closed() {
        // The collapsed crumbs' menu rows live in the (closed) chevron popover;
        // they must not render until it opens.
        let bc = trail(6);
        let mut tree = themed_tree();
        let _id = tree.add(bc);
        tree.layout(SizeProposal::exact(160.0, 30.0)); // narrow → middle collapses

        let active_menu_items: u32 = tree
            .widget_type_histogram()
            .iter()
            .filter(|(k, _)| k.contains("menu_item::MenuItem"))
            .map(|(_, v)| *v)
            .sum();
        assert_eq!(
            active_menu_items, 0,
            "overflow menu rows stay dormant until the … chevron opens"
        );
    }

    #[test]
    fn builds_under_rtl() {
        use bastyde_core::environment::LayoutDirection;
        let bc = trail(5);
        let mut tree = themed_tree();
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        let id = tree.add(bc);
        tree.layout(SizeProposal::exact(200.0, 30.0));
        assert!(tree.bounds(id).width > 0.0);
    }
}
