//! `TabHeader` — one tab's chrome (icon + leading slot + label + trailing slot),
//! plus background/indicator/focus-ring painting and the input handlers.
//!
//! Layout contract:
//! - `layout_response(proposal)` honors `proposal.width` when the parent
//!   row forces a width (Shared-sizing path). When the parent leaves
//!   width unspecified, the header reports its natural content width
//!   clamped to `[min_tab_width, max_tab_width]` (Independent path).
//! - Height is the tab style's `editor_tab_height` plus the focus-ring
//!   envelope reserved on top and bottom — the row's place_children
//!   uniforms the height across all headers.
//!
//! Paint contract (Int UI / IntelliJ):
//! - Selected: `surface_content` background, `text_primary` label,
//!   1 dp accent bar at the top.
//! - Hovered: `surface_hover` background.
//! - Idle: transparent background.
//! - Disabled: transparent background, `text_disabled` label.
//! - Focus ring: 2 dp `focus_ring` stroke painted around the visual
//!   rect, and only on keyboard focus (pointer focus does not paint
//!   the ring — IntelliJ / VS Code convention).

use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::LocalizedString;
use fern_tokens::{Color, CornerRadius, TextRole};

use crate::{BuiltInButton, BuiltInButtonSize, HStack, IconWidget, TextWidget};

/// Minimum natural width when the label is empty / extremely short.
const NATURAL_MIN_WIDTH: f32 = 72.0;
/// Vertical padding around the label inside the tab.
const HEADER_PADDING_V: f32 = 6.0;
/// Spacing inside the inner row between icon, slots, and label.
const INNER_GAP: f32 = 6.0;
/// Fallback char width for natural-size estimation when the text
/// backend is unavailable (test contexts).
const FALLBACK_CHAR_WIDTH: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabHeaderInteraction {
    Idle,
    Hovered,
}

/// Shared state plumbing the bar populates and the headers read.
///
/// `header_ids` and `panel_ids` are filled during the bar's `build()`
/// (after each header is added to the arena). `enabled_tabs` is a
/// frozen snapshot taken at build time — it does not need interior
/// mutability because the bar rebuilds when the data source changes.
pub(crate) struct HeaderShared {
    pub header_ids: Rc<RefCell<Vec<WidgetId>>>,
    pub panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    pub enabled_tabs: Rc<Vec<bool>>,
}

pub(crate) struct TabHeader {
    label: LocalizedString,
    /// Live `Signal<String>` derived from `label` via `to_signal()`.
    /// `accessibility()` reads this rather than a frozen `String`
    /// snapshot, so a locale change re-resolves the AT name without
    /// rebuilding the header. The signal is bound at
    /// `BindingLevel::AccessibilityOnly` in `build()` so the
    /// framework dirties the AT cache on update.
    label_signal: Option<Signal<String>>,
    icon: Option<IconWidget>,
    leading_slot: Option<Box<dyn Widget>>,
    trailing_slot: Option<Box<dyn Widget>>,
    tooltip: Option<LocalizedString>,
    context_menu_factory: Option<super::delegate::ContextMenuFactory>,
    /// Per-tab close callback. Set when the tab is closable AND the
    /// bar carries an `on_close` handler (or its source is a
    /// `ListModel<T>` providing a default-remove). The header wires
    /// it to a trailing close button and a middle-click handler.
    on_close: Option<Rc<dyn Fn()>>,
    /// Per-tab reorder callback. Set when the bar is reorderable;
    /// receives the destination index. Wired to AT custom actions
    /// "Move Left" / "Move Right" so screen-reader users can
    /// reorder tabs without dragging. Argument is the destination
    /// index in the unified bar ordering.
    on_reorder_to: Option<Rc<dyn Fn(usize)>>,
    /// `Some(bar_id)` when the bar enables drag-to-reorder and this
    /// tab is therefore a drag source. The header attaches `on_drag`
    /// that emits a `TabBarDragData` payload with this `bar_id`.
    drag_source_bar_id: Option<WidgetId>,

    index: usize,
    enabled: bool,
    selected: Signal<usize>,
    shared: Rc<HeaderShared>,

    interaction: Signal<TabHeaderInteraction>,
    focus_origin: Signal<Option<FocusOrigin>>,

    /// Width clamps applied to the natural content width when the bar
    /// is in Independent sizing mode.
    min_width: f32,
    max_width: f32,
    /// `true` for pinned tabs — icon-only, fixed-width, no close
    /// button, no label visible (the label becomes the tooltip).
    pinned: bool,
    /// Bar orientation — controls whether the selected-state accent
    /// indicator paints on the top edge (horizontal bars, the
    /// browser-tab convention) or the leading edge (vertical bars,
    /// the IDE-perspective / sidebar convention).
    orientation: super::delegate::TabBarOrientation,
    /// Uniform background applied regardless of selection / hover
    /// state, set by the parent [`TabBar`](super::TabBar) via
    /// `tab_background(...)`. `None` means transparent.
    tab_background: Option<fern_core::color_prop::ColorProp>,

    inner_root_id: Option<WidgetId>,
}

impl std::fmt::Debug for TabHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabHeader")
            .field("index", &self.index)
            .field("enabled", &self.enabled)
            .field("label", &self.label.clone().resolve_now())
            .field("has_icon", &self.icon.is_some())
            .field("has_leading_slot", &self.leading_slot.is_some())
            .field("has_trailing_slot", &self.trailing_slot.is_some())
            .field("has_tooltip", &self.tooltip.is_some())
            .field("has_context_menu", &self.context_menu_factory.is_some())
            .finish()
    }
}

pub(crate) struct TabHeaderConfig {
    pub label: LocalizedString,
    pub icon: Option<IconWidget>,
    pub leading_slot: Option<Box<dyn Widget>>,
    pub trailing_slot: Option<Box<dyn Widget>>,
    pub tooltip: Option<LocalizedString>,
    pub context_menu_factory: Option<super::delegate::ContextMenuFactory>,
    pub on_close: Option<Rc<dyn Fn()>>,
    pub on_reorder_to: Option<Rc<dyn Fn(usize)>>,
    pub drag_source_bar_id: Option<WidgetId>,
    pub index: usize,
    pub enabled: bool,
    pub selected: Signal<usize>,
    pub shared: Rc<HeaderShared>,
    pub min_width: f32,
    pub max_width: f32,
    pub pinned: bool,
    pub orientation: super::delegate::TabBarOrientation,
    pub tab_background: Option<fern_core::color_prop::ColorProp>,
}

impl TabHeader {
    pub(crate) fn new(cfg: TabHeaderConfig) -> Self {
        Self {
            label: cfg.label,
            label_signal: None,
            icon: cfg.icon,
            leading_slot: cfg.leading_slot,
            trailing_slot: cfg.trailing_slot,
            tooltip: cfg.tooltip,
            context_menu_factory: cfg.context_menu_factory,
            on_close: cfg.on_close,
            on_reorder_to: cfg.on_reorder_to,
            drag_source_bar_id: cfg.drag_source_bar_id,
            index: cfg.index,
            enabled: cfg.enabled,
            selected: cfg.selected,
            shared: cfg.shared,
            interaction: Signal::new(TabHeaderInteraction::Idle),
            focus_origin: Signal::new(None),
            min_width: cfg.min_width,
            max_width: cfg.max_width,
            pinned: cfg.pinned,
            orientation: cfg.orientation,
            tab_background: cfg.tab_background,
            inner_root_id: None,
        }
    }

    fn estimate_natural_width(&self, ctx: &LayoutContext) -> f32 {
        let pad_h = ctx.theme.components.tab.padding_horizontal;
        // Resolve the label once for measurement. Cheap: a literal
        // resolves to a clone; a translated key looks up Fluent.
        let label = self
            .label_signal
            .as_ref()
            .map(|s| s.get())
            .unwrap_or_else(|| self.label.clone().resolve_now());
        let text_width = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&label, &ctx.theme.typography.small, None)
                .width
        } else {
            label.len() as f32 * FALLBACK_CHAR_WIDTH
        };
        let icon_size = self
            .icon
            .as_ref()
            .map(|_| ctx.theme.components.button.icon_size + INNER_GAP)
            .unwrap_or(0.0);
        let leading_size = self
            .leading_slot
            .as_ref()
            .map(|_| ctx.theme.components.button.icon_size + INNER_GAP)
            .unwrap_or(0.0);
        let trailing_size = self
            .trailing_slot
            .as_ref()
            .map(|_| ctx.theme.components.button.icon_size + INNER_GAP)
            .unwrap_or(0.0);
        // Bounds == visual rect now (no focus-ring envelope), so
        // natural width is purely content + horizontal padding.
        (text_width + icon_size + leading_size + trailing_size + pad_h * 2.0).max(NATURAL_MIN_WIDTH)
    }

    pub(crate) fn intrinsic_height(ctx: &LayoutContext) -> f32 {
        // `editor_tab_height` is the **outer** measurement (the
        // total bounds height of one tab header). The focus-ring
        // envelope is reserved *inside* this — not added on top —
        // so the tab strip sits at exactly the token's value
        // regardless of focus-ring tokens. The visible pill rect
        // inside the bounds is `editor_tab_height - envelope*2`,
        // which `place_children` and `paint` shrink to.
        ctx.theme.components.tab.editor_tab_height
    }
}

impl Widget for TabHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let interaction = ctx.signal(TabHeaderInteraction::Idle);
        let focus_origin: Signal<Option<FocusOrigin>> = ctx.signal(None);
        let registry = ctx.binding_registry();

        // Repaint on selection / hover / focus changes.
        self.selected
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        interaction.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        focus_origin.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.interaction = interaction.clone();
        self.focus_origin = focus_origin.clone();

        // Locale-reactive label signal — used by `accessibility()`
        // to keep the AT name in sync with locale changes (and by
        // the natural-width estimate when present). Bound at
        // AccessibilityOnly so a locale change refreshes the AT
        // cache without a layout pass.
        let label_signal = self.label.to_signal();
        label_signal.bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        self.label_signal = Some(label_signal);

        // Build the inner content row.
        //
        // - Standard tab: [icon? leading? label trailing? close?]
        // - Pinned tab:   [icon] only (label collapses into the tooltip;
        //   close button suppressed; leading/trailing slots ignored).
        //   This matches Firefox's pinned-tab presentation.
        let mut row = HStack::new().spacing(INNER_GAP);

        // Per-state text role: Int UI editor-tab convention puts
        // selected at `Primary`, idle at `Secondary`, disabled at
        // `Disabled` — the label-color shift on click is one of the
        // strongest "this tab is active" cues besides the accent
        // indicator.
        let enabled = self.enabled;
        let index_for_role = self.index;
        let role_signal = self.selected.map(move |sel| {
            if !enabled {
                TextRole::Disabled
            } else if *sel == index_for_role {
                TextRole::Primary
            } else {
                TextRole::Secondary
            }
        });

        if let Some(icon) = self.icon.take() {
            // Icon tint follows the same role — selected tab gets
            // a primary-tinted icon, idle gets secondary, disabled
            // gets the disabled tint.
            let icon = icon.bind_color(role_signal.clone());
            let id = ctx.add(icon);
            row = row.add_child(id);
        }

        if !self.pinned {
            if let Some(slot) = self.leading_slot.take() {
                let id = ctx.add_boxed(slot);
                row = row.add_child(id);
            }

            // Label as a TextWidget — reactive on locale change because
            // `From<LocalizedString> for Prop<String>` produces a bound
            // `Signal<String>` that tracks the i18n manager's locale
            // version. Color is bound to the per-state role above so
            // the selected/idle/disabled distinction is visible.
            let label_widget = TextWidget::new(self.label.clone())
                .single_line()
                .color(role_signal.clone());
            let label_id = ctx.add(label_widget);
            row = row.add_child(label_id);

            // Flexible spacer between the label and the trailing
            // controls. `Spacer::new()` reports natural size 0 with
            // `flex = 1`, so it contributes zero in Independent
            // sizing (HStack reports the natural sum and there's no
            // slack to share) and absorbs all the slack in Shared
            // sizing (the bar gives every tab the same fixed
            // extent), pushing the close button against the
            // trailing edge of the pill regardless of label length.
            let spacer_id = ctx.add(crate::primitives::Spacer::new());
            row = row.add_child(spacer_id);

            if let Some(slot) = self.trailing_slot.take() {
                let id = ctx.add_boxed(slot);
                row = row.add_child(id);
            }

            // Trailing close button for closable tabs. Visible only
            // on hover — Firefox / Chrome convention; the button's
            // tap is consumed by its own handler so selection
            // (driven by the surrounding TabHeader's `on_tap`) does
            // not fire.
            if let Some(close_fn) = self.on_close.clone() {
                // The close button must NOT be focusable: Tab
                // navigation walks between tab headers, not onto
                // each header's close button. Activation happens
                // via mouse click, middle-click on the tab body
                // (handled by the surrounding `on_pointer_event`),
                // or AT custom action.
                let close_button = BuiltInButton::clear()
                    .size(BuiltInButtonSize::Compact)
                    .focusable(false)
                    .tooltip(LocalizedString::literal("Close tab"))
                    .on_activate_fn(move |_ctx| (close_fn)());
                let close_id = ctx.add(close_button);
                // Hover-only: the button is hidden when the
                // surrounding tab header is in the Idle interaction
                // state. The interaction signal flips to Hovered
                // via the `on_hover` handler installed below.
                let visible_when = interaction.map(|s| matches!(*s, TabHeaderInteraction::Hovered));
                ctx.visible_when(close_id, visible_when);
                row = row.add_child(close_id);
            }
        } else {
            // Pinned: promote the label into a tooltip so the user
            // can still identify the tab on hover. Take precedence
            // over any tooltip already configured (the explicit one
            // is rare for pinned tabs and would otherwise drop the
            // label entirely).
            self.tooltip = Some(self.label.clone());
        }

        // Wrap the row in a Padding for breathing room.
        //
        // - Standard tab: symmetric vertical + horizontal padding.
        // - Pinned tab: NO horizontal padding (the pill is 32 dp
        //   wide and the icon is 16 dp; symmetric horizontal
        //   padding would push the icon off-center). Wrap in
        //   `Center` so the icon sits exactly in the middle of the
        //   bounds regardless of the pill width.
        let pad_h = ctx.theme().components.tab.padding_horizontal;
        let inner_id = if self.pinned {
            let centered = crate::primitives::Center::new().child(row);
            let padded = crate::Padding::symmetric(HEADER_PADDING_V, 0.0).child(centered);
            ctx.add(padded)
        } else {
            let padded = crate::Padding::symmetric(HEADER_PADDING_V, pad_h).child(row);
            ctx.add(padded)
        };
        self.inner_root_id = Some(inner_id);

        // Attach handlers: tap selects, hover updates state, focus
        // tracks origin, key handles arrow-nav + Enter/Space.
        let enabled = self.enabled;
        let index = self.index;
        let selected = self.selected.clone();
        let header_ids = self.shared.header_ids.clone();
        let enabled_tabs = self.shared.enabled_tabs.clone();
        let interaction_for_hover = interaction.clone();
        let interaction_for_focus = interaction.clone();
        let focus_origin_for_handler = focus_origin.clone();

        let mut handler_set = HandlerSet::new()
            .on_tap(move |_event, _ctx: &mut EventContext| {
                if enabled {
                    selected.set(index);
                }
            })
            .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                if !enabled {
                    interaction_for_hover.set(TabHeaderInteraction::Idle);
                    return;
                }
                interaction_for_hover.set(if entered {
                    TabHeaderInteraction::Hovered
                } else {
                    TabHeaderInteraction::Idle
                });
            })
            .on_focus({
                let focus_origin = focus_origin_for_handler.clone();
                let interaction = interaction_for_focus.clone();
                move |gained: bool, _ctx: &mut EventContext| {
                    if !enabled || !gained {
                        focus_origin.set(None);
                        return;
                    }
                    let origin = if interaction.get() == TabHeaderInteraction::Hovered {
                        FocusOrigin::Pointer
                    } else {
                        FocusOrigin::Keyboard
                    };
                    focus_origin.set(Some(origin));
                }
            })
            .on_key({
                let selected = self.selected.clone();
                let header_ids = header_ids.clone();
                let enabled_tabs = enabled_tabs.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    let headers = header_ids.borrow();
                    if headers.is_empty() {
                        return EventResponse::Ignored;
                    }
                    // ArrowLeft/Up = previous tab, ArrowRight/Down =
                    // next. We accept both axes regardless of bar
                    // orientation — pressing ArrowLeft on a vertical
                    // bar is harmless (no headers to the side; the
                    // user just gets the prev neighbor) and lets the
                    // bar work without orientation-specific wiring at
                    // the header level.
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight | Key::ArrowDown,
                            ..
                        } => {
                            let next = next_enabled_index(&enabled_tabs, index, 1);
                            selected.set(next);
                            ctx.request_focus(headers[next]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft | Key::ArrowUp,
                            ..
                        } => {
                            let prev = next_enabled_index(&enabled_tabs, index, -1);
                            selected.set(prev);
                            ctx.request_focus(headers[prev]);
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
            .on_access_action_request({
                let selected = self.selected.clone();
                let on_reorder_to = self.on_reorder_to.clone();
                let header_count_signal = self.shared.header_ids.clone();
                move |action, _node, data, _ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    use fern_core::accesskit::{Action, ActionData};
                    match action {
                        Action::Click => {
                            selected.set(index);
                            EventResponse::Handled
                        }
                        Action::CustomAction => {
                            let Some(reorder) = on_reorder_to.as_ref() else {
                                return EventResponse::Ignored;
                            };
                            // Custom action indices we advertise:
                            //   0 = Move Left  (index → index - 1)
                            //   1 = Move Right (index → index + 1)
                            let Some(ActionData::CustomAction(idx)) = data else {
                                return EventResponse::Ignored;
                            };
                            let total = header_count_signal.borrow().len();
                            match idx {
                                0 if index > 0 => {
                                    reorder(index - 1);
                                    EventResponse::Handled
                                }
                                1 if index + 1 < total => {
                                    reorder(index + 1);
                                    EventResponse::Handled
                                }
                                _ => EventResponse::Ignored,
                            }
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .focusable(enabled)
            .cursor(if enabled {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });

        // Drag source: emit a `TabBarDragData` payload when the
        // user starts dragging this header. The bar's `on_drop`
        // accepts the payload only if its `source_bar_id` matches —
        // a tab from another TabBar that happens to be in the same
        // window can't accidentally reorder into ours.
        if let Some(bar_id) = self.drag_source_bar_id {
            let index = self.index;
            let label_for_preview = self.label.clone();
            handler_set = handler_set.on_drag(move |phase, ctx| {
                if let fern_core::gesture::DragPhase::Started { .. } = phase {
                    let payload = fern_core::drag_payload::DragPayload::typed(
                        crate::tab_widget::bar::TabBarDragData {
                            source_index: index,
                            source_bar_id: bar_id,
                        },
                    );
                    let preview_inner: Box<dyn fern_core::widget::Widget> = Box::new(
                        crate::Padding::symmetric(HEADER_PADDING_V, INNER_GAP * 2.0)
                            .child(TextWidget::new(label_for_preview.clone()).single_line()),
                    );
                    // Hardcoded preview footprint — the live tab is
                    // not a known size at handler time (no
                    // LayoutContext). 160×32 dp matches the average
                    // tab footprint and keeps the preview legible.
                    let preview = crate::drag_preview::DragPreview::new(160.0, 32.0, preview_inner);
                    ctx.start_drag_with_preview(self_id, payload, Box::new(preview));
                }
            });
        }

        // Middle-click closes the tab on Up (Firefox / Chrome
        // convention). Non-Primary activation is already filtered at
        // the framework level: `TapRecognizer` defaults to
        // `ButtonMask::PRIMARY`, so a right-click or middle-click
        // press never fires the inner `on_tap`. The `on_pointer_event`
        // handler here only adds the close-on-middle-up behaviour.
        if let Some(close_fn) = self.on_close.clone() {
            handler_set = handler_set.on_pointer_event(move |event, _ctx| match event {
                WidgetEvent::PointerUp {
                    button: PointerButton::Middle,
                    ..
                } if enabled => {
                    (close_fn)();
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        if let Some(factory) = self.context_menu_factory.clone() {
            handler_set = handler_set.context_menu(move |pos, ctx| (factory)(pos, ctx));
        }

        ctx.apply_self_handlers(handler_set);

        // Attach tooltip via the framework helper.
        if let Some(tip) = self.tooltip.take() {
            let tip_widget = crate::tooltip::TooltipWidget::new(tip);
            let tip_id = ctx.add(tip_widget);
            ctx.attach_tooltip(self_id, tip_id, std::time::Duration::from_millis(400));
        }

        vec![inner_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let height = proposal
            .height
            .unwrap_or_else(|| Self::intrinsic_height(ctx));
        // Pinned tabs are always rendered at exactly `min_width`
        // (the bar's `pinned_tab_width`), regardless of any width
        // proposal — they're icon-only squares and shouldn't stretch.
        let width = if self.pinned {
            self.min_width
        } else {
            match proposal.width {
                Some(w) => w,
                None => self
                    .estimate_natural_width(ctx)
                    .clamp(self.min_width, self.max_width),
            }
        };
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Fill the full bounds — adjacent tab headers sit flush
        // against each other (Firefox / Chrome convention; the
        // accent indicator + per-state surface fill carry the
        // selected/idle distinction without needing a gap). The
        // focus ring is painted *inside* `bounds` (inset by
        // `focus_ring_width / 2`), so it never bleeds into the
        // neighbour tab.
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let selected = self.selected.get() == self.index;
        let colors = &ctx.theme.colors;
        let shape = &ctx.theme.shape;
        // The visible pill rect IS the bounds — no envelope shrink,
        // so adjacent tabs sit flush against each other. The focus
        // ring is painted *inside* the bounds (inset by half the
        // stroke width) so it never bleeds into the neighbour tab.
        let visual = bounds;

        // Uniform background regardless of state — all tabs read
        // visually identical except for the accent indicator and the
        // label-color shift. The role is set by the parent `TabBar`
        // via `tab_background(...)`; default is transparent.
        let background = if let Some(ref prop) = self.tab_background {
            prop.resolve(ctx.theme)
        } else {
            Color::TRANSPARENT
        };
        if background.a() > 0.0 {
            canvas.fill_rect(visual, background);
        }

        // Accent indicator on the layout-axis "outside" edge of the
        // selected, enabled tab. Int UI convention:
        //   - Horizontal bar → indicator on TOP (browser-tab look,
        //     selected tab "merges" into the content panel below).
        //   - Vertical bar → indicator on the LEADING edge (sidebar
        //     / IDE perspective look — the tab "points into" the
        //     content panel on the trailing side).
        // Thickness comes from the `TabStyle::underline_active`
        // token (3 dp by Int UI default) to match other Int UI
        // tab variants.
        let indicator_thickness = ctx.theme.components.tab.underline_active;
        if selected && self.enabled {
            let indicator = match self.orientation {
                super::delegate::TabBarOrientation::Horizontal => {
                    Rect::new(visual.x, visual.y, visual.width, indicator_thickness)
                }
                super::delegate::TabBarOrientation::Vertical => {
                    Rect::new(visual.x, visual.y, indicator_thickness, visual.height)
                }
            };
            canvas.fill_rect(indicator, colors.accent);
        }

        // Focus ring — keyboard focus only. Drawn *inside* `bounds`
        // (inset by `focus_ring_width / 2 + focus_ring_offset`) so
        // adjacent tabs aren't visually overlapped by the ring.
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let half_stroke = shape.focus_ring_width * 0.5;
            let inset = half_stroke + shape.focus_ring_offset;
            let ring_rect = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            );
            let ring_radius = shape.radius_control;
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
        let resolved = self
            .label_signal
            .as_ref()
            .map(|s| s.get())
            .unwrap_or_else(|| self.label.clone().resolve_now());
        builder.set_name(&resolved);
        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(fern_core::accesskit::Action::Click);
        }
        builder.add_action(fern_core::accesskit::Action::Focus);
        builder.set_selected(self.selected.get() == self.index);
        if let Some(&panel_id) = self.shared.panel_ids.borrow().get(self.index) {
            builder.push_controlled(fern_core::accessibility::widget_id_to_node_id(panel_id));
        }

        // Advertise reorder custom actions for AT users who can't drag.
        // Order matters: index 0 = "Move Left", index 1 = "Move Right"
        // — `on_access_action_request` reads the index from
        // `ActionData::CustomAction(idx)` and routes accordingly.
        // Suppressed for pinned tabs (whose order is conceptually fixed
        // by the pinned-strip layout — Firefox convention).
        if self.on_reorder_to.is_some() && self.enabled && !self.pinned {
            let total = self.shared.header_ids.borrow().len();
            let mut actions = Vec::with_capacity(2);
            if self.index > 0 {
                actions.push(fern_core::accesskit::CustomAction {
                    id: 0,
                    description: "Move Left".into(),
                });
            }
            if self.index + 1 < total {
                actions.push(fern_core::accesskit::CustomAction {
                    id: 1,
                    description: "Move Right".into(),
                });
            }
            if !actions.is_empty() {
                builder.add_action(fern_core::accesskit::Action::CustomAction);
                builder.set_custom_actions(actions);
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.inner_root_id.into_iter().collect()
    }
}

pub(crate) fn next_enabled_index(enabled_tabs: &[bool], current: usize, direction: isize) -> usize {
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
