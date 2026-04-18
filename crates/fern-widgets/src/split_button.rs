//! SplitButton — a button split into two regions sharing a single frame.
//!
//! The left region is the **default action**: it shows the label of the
//! currently-selected item and, on click, fires that item's command
//! (behaving like a regular [`Button`](crate::button::Button)). The right
//! region is a narrow chevron zone that, on click, opens a
//! [`MenuList`](crate::menu_list::MenuList) of related actions. Picking an
//! action from the dropdown fires it and promotes its index to become the
//! new default for the session (IntelliJ's "remember last used"
//! convention).
//!
//! SplitButton reuses [`MenuItem`](crate::menu_item::MenuItem) verbatim
//! for the dropdown rows — the caller passes real `MenuItem` values via
//! `.item(...)`, so icons, shortcut labels, enabled flags, and separators
//! all come for free.
//!
//! ```ignore
//! SplitButton::new()
//!     .item(MenuItem::new_literal("Run").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Run)))
//!     .item(MenuItem::new_literal("Run Tests").on_activate_fn(|ctx| ctx.send_intent(AppIntent::RunTests)))
//!     .separator()
//!     .item(MenuItem::new_literal("Debug").on_activate_fn(|ctx| ctx.send_intent(AppIntent::Debug)))
//!     .style(ButtonVariant::Regular)
//! ```

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::{HandlerSet, WidgetBuilder};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, ColorTokens, CornerRadius};

use crate::button::{ButtonVariant, InteractionState};
use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;
use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, MinSize, Padding, RectWidget, TextWidget,
    ZStack,
};

/// One row of the SplitButton's dropdown: either a real MenuItem or a
/// separator. Stored unbuilt until `build()` hands the items to a MenuList.
/// MenuItem is boxed because it is substantially larger than `Separator`,
/// which would otherwise bloat every `Row::Separator` slot.
enum Row {
    Item(Box<MenuItem>),
    Separator,
}

pub struct SplitButton {
    rows: Vec<Row>,
    style: ButtonVariant,
    enabled: bool,
    initial_selected: usize,
    /// Whether picking an item from the dropdown promotes it to the new
    /// session default (IntelliJ's "remember last used"). `true` for
    /// [`SplitButton::new`], `false` for [`SplitButton::new_static`].
    promote_on_select: bool,
    /// Tooltip shown on hover over the main (default-action) region.
    tooltip_text: Option<String>,
    /// Tooltip shown on hover over the trailing chevron region. Falls
    /// back to a generic "Show dropdown menu" label when not explicitly
    /// set, since the chevron region has no label of its own.
    chevron_tooltip_text: Option<String>,
    // Build state
    interaction: Signal<InteractionState>,
    selected: Signal<usize>,
    labels: Rc<Vec<String>>,
    menu_content_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
}

impl SplitButton {
    /// Standard SplitButton: picking an item from the dropdown both
    /// **fires** the item's action and **promotes** it to become the new
    /// default for the session. The main region's label and click action
    /// update to match the most recently picked item.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            style: ButtonVariant::Regular,
            enabled: true,
            initial_selected: 0,
            promote_on_select: true,
            tooltip_text: None,
            chevron_tooltip_text: None,
            interaction: Signal::new(InteractionState::Idle),
            selected: Signal::new(0),
            labels: Rc::new(Vec::new()),
            menu_content_id: None,
            root_child_id: None,
        }
    }

    /// Static-default SplitButton: the main region is pinned to
    /// `initial_selected` (default 0) and **never** changes after the
    /// user picks something from the dropdown. Picking an item still
    /// fires that item's action — only the promotion is skipped.
    ///
    /// Use this when the main region represents a semantically fixed
    /// primary action (e.g. "Commit") and the dropdown offers related
    /// variants ("Commit and Push", "Commit and Push to…") that should
    /// not displace the primary.
    pub fn new_static() -> Self {
        Self {
            promote_on_select: false,
            ..Self::new()
        }
    }

    /// Add a menu item. The item is reused verbatim as a row of the
    /// dropdown, and its label + action are also used to drive the main
    /// region (when its index is the current default).
    pub fn item(mut self, item: MenuItem) -> Self {
        self.rows.push(Row::Item(Box::new(item)));
        self
    }

    /// Add a separator row in the dropdown. Separators are skipped when
    /// computing item indices for `initial_selected`.
    pub fn separator(mut self) -> Self {
        self.rows.push(Row::Separator);
        self
    }

    pub fn style(mut self, variant: ButtonVariant) -> Self {
        self.style = variant;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Which item index (counting only items, not separators) should be
    /// the initial default. Defaults to 0.
    pub fn initial_selected(mut self, index: usize) -> Self {
        self.initial_selected = index;
        self
    }

    /// Attach a tooltip to the main (default-action) region. Same hover
    /// delay as [`Button::tooltip`](crate::button::Button::tooltip).
    pub fn tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self
    }

    /// Override the tooltip shown on hover over the trailing chevron
    /// region. When unset, the chevron gets a default "Show dropdown
    /// menu" tooltip so its affordance isn't silent.
    pub fn chevron_tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.chevron_tooltip_text = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `chevron_tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn chevron_tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.chevron_tooltip_text = Some(text.into());
        self
    }
}

impl Default for SplitButton {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SplitButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitButton")
            .field("rows", &self.rows.len())
            .field("style", &self.style)
            .field("enabled", &self.enabled)
            .finish()
    }
}

// --- Color resolution (variant × state × theme) ---
//
// Mirrors Button::resolve_bg / resolve_text / resolve_border so a Button
// and a SplitButton with the same variant look identical. If Button's
// color tables ever diverge from these, update both sides.

fn resolve_bg(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    match (style, state) {
        (ButtonVariant::Default, InteractionState::Disabled) => colors.accent_disabled,
        (ButtonVariant::Default, InteractionState::Pressed) => colors.accent_pressed,
        (ButtonVariant::Default, InteractionState::Hovered) => colors.accent_hover,
        (ButtonVariant::Default, _) => colors.accent,

        (ButtonVariant::Regular, InteractionState::Pressed) => colors.surface_pressed,
        (ButtonVariant::Regular, InteractionState::Hovered) => colors.surface_hover,
        (ButtonVariant::Regular, _) => colors.surface_main,

        (ButtonVariant::Flat, InteractionState::Pressed) => colors.surface_pressed,
        (ButtonVariant::Flat, InteractionState::Hovered) => colors.surface_hover,
        (ButtonVariant::Flat, _) => Color::TRANSPARENT,
    }
}

fn resolve_text(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    match (style, state) {
        (ButtonVariant::Default, InteractionState::Disabled) => colors.text_disabled,
        (ButtonVariant::Default, _) => colors.text_on_accent,

        (ButtonVariant::Regular | ButtonVariant::Flat, InteractionState::Disabled) => {
            colors.text_disabled
        }
        (ButtonVariant::Regular | ButtonVariant::Flat, _) => colors.text_primary,
    }
}

fn resolve_border(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    if state == InteractionState::Focused {
        return colors.focus_ring;
    }
    match style {
        ButtonVariant::Default | ButtonVariant::Flat => Color::TRANSPARENT,
        ButtonVariant::Regular => match state {
            InteractionState::Disabled => colors.border,
            InteractionState::Hovered | InteractionState::Pressed => colors.border_strong,
            _ => colors.border,
        },
    }
}

/// Border width for the SplitButton frame: thickens to the theme's
/// `focus_ring_width` on focus, rests at the variant's normal
/// width otherwise.
fn resolve_border_width(style: ButtonVariant, state: InteractionState, normal_bw: f32, focus_bw: f32) -> f32 {
    if state == InteractionState::Focused {
        return focus_bw;
    }
    match style {
        ButtonVariant::Default | ButtonVariant::Flat => 0.0,
        ButtonVariant::Regular => normal_bw,
    }
}

impl Widget for SplitButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme_signal = ctx.theme_signal();
        let sb_style = theme_signal.get().components.split_button;
        let style = self.style;
        let enabled = self.enabled;

        // ---- Extract label / action for each MenuItem and wrap each item's
        // activation so selecting it from the menu also promotes its index
        // to the current default. ----

        let mut labels_vec: Vec<String> = Vec::new();
        let mut actions_vec: Vec<Option<Rc<dyn Fn(&mut EventContext)>>> = Vec::new();
        let mut menu = MenuList::new();

        // Create the `selected` signal early so the wrap closures can
        // capture it.
        let initial = self.initial_selected;
        let selected: Signal<usize> = ctx.signal(initial);
        let promote_on_select = self.promote_on_select;

        for row in self.rows.drain(..) {
            match row {
                Row::Item(boxed_item) => {
                    let mut item = *boxed_item;
                    let label = item.label().to_string();
                    let action = item.action();
                    let my_index = labels_vec.len();
                    labels_vec.push(label);
                    actions_vec.push(action.clone());

                    // Only wrap the item's activation when we need to
                    // promote the selected index. In static mode we hand
                    // the MenuItem through untouched so its original
                    // action runs as-is — no redirection, no extra Rc
                    // churn, and the MenuItem's existing tests still
                    // hold for the inner behavior.
                    if promote_on_select {
                        let prev_action = action.clone();
                        let promote = selected.clone();
                        item = item.on_activate_fn(move |ctx: &mut EventContext| {
                            if let Some(ref a) = prev_action {
                                a(ctx);
                            }
                            promote.set(my_index);
                        });
                    }
                    menu = menu.item(item);
                }
                Row::Separator => {
                    menu = menu.separator();
                }
            }
        }

        // Clamp initial_selected to a valid range now that we know the count.
        let item_count = labels_vec.len();
        if item_count == 0 || selected.get() >= item_count {
            selected.set(0);
        }

        let labels_rc = Rc::new(labels_vec);
        let actions_rc: Rc<Vec<Option<Rc<dyn Fn(&mut EventContext)>>>> = Rc::new(actions_vec);

        self.labels = labels_rc.clone();
        self.selected = selected.clone();

        // ---- Interaction state signal ----
        let interaction = ctx.signal(if enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        self.interaction = interaction.clone();

        // ---- Derived reactive colors ----
        let bg_color = interaction
            .zip(&theme_signal)
            .map(move |(s, t)| resolve_bg(style, *s, &t.colors));
        let text_color = interaction
            .zip(&theme_signal)
            .map(move |(s, t)| resolve_text(style, *s, &t.colors));
        let border_color = interaction
            .zip(&theme_signal)
            .map(move |(s, t)| resolve_border(style, *s, &t.colors));
        // `divider` is a RectWidget used as a 1-dp vertical rule; role-based
        // so it follows theme changes without an intermediate signal.

        // ---- Main-region label bound to `selected` ----
        let main_label_text = {
            let labels = labels_rc.clone();
            selected.map(move |i| {
                if labels.is_empty() {
                    String::new()
                } else {
                    labels[(*i).min(labels.len() - 1)].clone()
                }
            })
        };

        // ---- Pre-register the menu overlay (dormant until opened) ----
        let menu_id = ctx.add(menu);
        ctx.set_dormant(menu_id);
        self.menu_content_id = Some(menu_id);

        let self_id = ctx.self_id();

        // ---- Main region subtree ----
        let label_widget = TextWidget::new_literal("")
            .bind_text(main_label_text)
            .bind_color(text_color.clone())
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label_widget);

        let main_padding_id = ctx.add(
            Padding::symmetric(sb_style.padding_vertical, sb_style.padding_horizontal)
                .child_id(label_id),
        );
        // ZStack (default CENTER alignment) centers the padded label within
        // the MinSize bounds when the region is wider than the text — same
        // pattern Button uses. Without this, MinSize stretches Padding to
        // fill and the label pins to the top-left inset corner.
        let main_content_id = ctx.add(ZStack::new().add_child(main_padding_id));

        let main_region = {
            let actions_for_tap = actions_rc.clone();
            let selected_for_tap = selected.clone();
            let int_for_tap = interaction.clone();
            let int_for_hover = interaction.clone();
            MinSize::new(sb_style.min_width, sb_style.height)
                .child_id(main_content_id)
                .on_tap(move |_pos, ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    let idx = selected_for_tap.get();
                    if let Some(Some(action)) = actions_for_tap.get(idx) {
                        action(ctx);
                    }
                    int_for_tap.set(InteractionState::Hovered);
                })
                .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    int_for_hover.set(if entered {
                        InteractionState::Hovered
                    } else {
                        InteractionState::Idle
                    });
                })
                .cursor(CursorIcon::Pointer)
        };
        let main_region_id = ctx.add(main_region);

        // Attach the main-region tooltip if the caller set one. Uses the
        // same 500 ms delay as Button.
        if let Some(ref text) = self.tooltip_text {
            let tooltip_widget = crate::tooltip::TooltipWidget::new_literal(text);
            let tooltip_id = ctx.add(tooltip_widget);
            ctx.attach_tooltip(
                main_region_id,
                tooltip_id,
                std::time::Duration::from_millis(500),
            );
        }

        // ---- Divider between main and chevron regions ----
        let divider_fill_id =
            ctx.add(RectWidget::new().background(fern_tokens::BorderRole::Default));
        let divider_id = ctx.add(
            FixedSize::new()
                .bind_width(sb_style.divider_width)
                .bind_height(sb_style.height)
                .child_id(divider_fill_id),
        );

        // ---- Chevron region ----
        let chevron_icon_id = ctx.add(
            IconWidget::chevron_down(sb_style.chevron_icon_size).bind_color(text_color.clone()),
        );
        let chevron_centered_id = ctx.add(Center::new().child_id(chevron_icon_id));

        let chevron_region = {
            let int_for_tap = interaction.clone();
            let int_for_hover = interaction.clone();
            FixedSize::new()
                .bind_width(sb_style.chevron_width)
                .bind_height(sb_style.height)
                .child_id(chevron_centered_id)
                .on_tap(move |_pos, ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    int_for_tap.set(InteractionState::Pressed);
                    ctx.activate(menu_id);
                    ctx.show_overlay(OverlayRequest {
                        content_id: menu_id,
                        anchor: self_id,
                        placement: OverlayPlacement::BelowPreferred,
                        dismiss: DismissBehavior::EscapeOrClickOutside,
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                        on_dismiss: None,
                    });
                    // The MenuList owns the keyboard-navigation handler
                    // (ArrowUp/ArrowDown/Enter/Escape) and that handler
                    // only fires when the MenuList is focused. Hand focus
                    // over so the user can immediately keyboard-walk the
                    // items they just opened.
                    ctx.request_focus(menu_id);
                })
                .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    int_for_hover.set(if entered {
                        InteractionState::Hovered
                    } else {
                        InteractionState::Idle
                    });
                })
                .cursor(CursorIcon::Pointer)
        };
        let chevron_region_id = ctx.add(chevron_region);

        // Attach the chevron tooltip. Defaults to "Show dropdown menu"
        // so the bare ▾ affordance is never silent — the caller can
        // override via `.chevron_tooltip(...)`.
        {
            let chevron_text = self
                .chevron_tooltip_text
                .clone()
                .unwrap_or_else(|| "Show dropdown menu".to_string());
            let tooltip_widget = crate::tooltip::TooltipWidget::new_literal(&chevron_text);
            let tooltip_id = ctx.add(tooltip_widget);
            ctx.attach_tooltip(
                chevron_region_id,
                tooltip_id,
                std::time::Duration::from_millis(500),
            );
        }

        // ---- Row: main | divider | chevron ----
        let row_id = ctx.add(
            HStack::new()
                .spacing(0.0)
                .add_child(main_region_id)
                .add_child(divider_id)
                .add_child(chevron_region_id),
        );

        // Border width reacts to focus state — thickens to the
        // accent `focus_ring_width` on focus, matching the Int UI
        // convention applied uniformly across all input widgets.
        let border_width = interaction.zip(&theme_signal).map(move |(s, t)| {
            resolve_border_width(
                style,
                *s,
                t.components.split_button.border_width,
                t.shape.focus_ring_width,
            )
        });

        // ---- Shared frame (single RectWidget behind the row) ----
        let bg_rect = RectWidget::new()
            .bind_background(bg_color)
            .bind_border_color(border_color)
            .bind_border_width(border_width)
            .corner_radius(CornerRadius::uniform(sb_style.corner_radius));
        let bg_id = ctx.add(bg_rect);

        let frame_id = ctx.add(ZStack::new().add_child(bg_id).add_child(row_id));

        // Enforce an overall minimum size: main min_width + divider + chevron.
        let total_min_width = sb_style.min_width + sb_style.divider_width + sb_style.chevron_width;
        let root_id = ctx.add(
            MinSize::new(total_min_width, sb_style.height).child_id(frame_id),
        );
        self.root_child_id = Some(root_id);

        // ---- Self handlers: the SplitButton is the single focus stop.
        // Space/Enter fires the current default; ArrowDown opens the menu.
        let actions_for_key = actions_rc.clone();
        let selected_for_key = selected.clone();
        let int_for_key = interaction.clone();
        let int_for_focus = interaction.clone();

        let handler_set = HandlerSet::new()
            .on_key(
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            int_for_key.set(InteractionState::Pressed);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyUp {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            let idx = selected_for_key.get();
                            if let Some(Some(action)) = actions_for_key.get(idx) {
                                action(ctx);
                            }
                            int_for_key.set(InteractionState::Focused);
                            EventResponse::Handled
                        }
                        // ArrowDown alone, or Alt+ArrowDown (the native
                        // "open dropdown" shortcut) both open the menu.
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown,
                            ..
                        } => {
                            ctx.activate(menu_id);
                            ctx.show_overlay(OverlayRequest {
                                content_id: menu_id,
                                anchor: self_id,
                                placement: OverlayPlacement::BelowPreferred,
                                dismiss: DismissBehavior::EscapeOrClickOutside,
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: None,
                            });
                            ctx.request_focus(menu_id);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                },
            )
            .on_focus(move |gained: bool, _ctx: &mut EventContext| {
                if gained {
                    if int_for_focus.get() == InteractionState::Idle {
                        int_for_focus.set(InteractionState::Focused);
                    }
                } else {
                    int_for_focus.set(InteractionState::Idle);
                }
            })
            .focusable(enabled);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Button);
        if !self.labels.is_empty() {
            let idx = self.selected.get().min(self.labels.len() - 1);
            builder.set_name(self.labels[idx].as_str());
        }
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}

