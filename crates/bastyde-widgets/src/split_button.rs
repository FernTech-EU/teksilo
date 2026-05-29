//! SplitButton — a button split into two regions sharing a single frame.
//!
//! The left region is the **default action**: it shows the label of the
//! currently-selected item and, on click, fires that item's command
//! (behaving like a regular [`Button`](crate::button::Button)). The right
//! region is a narrow chevron zone that, on click, opens a
//! [`MenuList`] of related actions. Picking an
//! action from the dropdown fires it and promotes its index to become the
//! new default for the session (IntelliJ's "remember last used"
//! convention).
//!
//! SplitButton reuses [`MenuItem`] verbatim
//! for the dropdown rows — the caller passes real `MenuItem` values via
//! `.item(...)`, so icons, shortcut labels, enabled flags, and separators
//! all come for free.
//!
//! ```ignore
//! SplitButton::new()
//!     .item(MenuItem::new(lit!("Run")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Run)))
//!     .item(MenuItem::new(lit!("Run Tests")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::RunTests)))
//!     .separator()
//!     .item(MenuItem::new(lit!("Debug")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Debug)))
//!     .variant(ButtonVariant::Plain)
//! ```

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::{HandlerSet, WidgetBuilder};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, CornerRadius, SurfaceRole, TextRole};

use crate::button::{ButtonVariant, InteractionState};
use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;
use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, MinSize, Padding, RectWidget, TextWidget, ZStack,
};
use bastyde_i18n::LocalizedString;

/// One row of the SplitButton's dropdown: either a real MenuItem or a
/// separator. Stored unbuilt until `build()` hands the items to a MenuList.
/// MenuItem is boxed because it is substantially larger than `Separator`,
/// which would otherwise bloat every `Row::Separator` slot.
enum Row {
    Item(Box<MenuItem>),
    Separator,
}

/// SplitButton design tokens.
pub const SPLIT_BUTTON_HEIGHT: f32 = 24.0;
pub const SPLIT_BUTTON_MIN_WIDTH: f32 = 72.0;
pub const SPLIT_BUTTON_PADDING_HORIZONTAL: f32 = 14.0;
pub const SPLIT_BUTTON_PADDING_VERTICAL: f32 = 0.0;
pub const SPLIT_BUTTON_CORNER_RADIUS: f32 = 4.0;
pub const SPLIT_BUTTON_BORDER_WIDTH: f32 = 1.0;
pub const SPLIT_BUTTON_CHEVRON_WIDTH: f32 = 22.0;
pub const SPLIT_BUTTON_DIVIDER_WIDTH: f32 = 1.0;
pub const SPLIT_BUTTON_CHEVRON_ICON_SIZE: f32 = 12.0;

pub struct SplitButton {
    rows: Vec<Row>,
    variant: ButtonVariant,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    initial_selected: usize,
    /// Whether picking an item from the dropdown promotes it to the new
    /// session default (IntelliJ's "remember last used"). `true` for
    /// [`SplitButton::new`], `false` for [`SplitButton::new_static`].
    promote_on_select: bool,
    /// Tooltip shown on hover over the main (default-action) region.
    tooltip_text: Option<LocalizedString>,
    /// Rich tooltip source for the main region (registry key or inline
    /// content). Mutually exclusive with `tooltip_text` and
    /// `composite_tooltip_content`.
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Composite tooltip body for the main region (CK3-style widget
    /// tree). Mutually exclusive with the other two main slots.
    composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,
    /// Tooltip shown on hover over the trailing chevron region. Falls
    /// back to a generic "Show dropdown menu" label when not explicitly
    /// set, since the chevron region has no label of its own.
    chevron_tooltip_text: Option<LocalizedString>,
    /// Rich tooltip source for the chevron region.
    chevron_rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Composite tooltip body for the chevron region.
    chevron_composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,
    // Build state
    interaction: Signal<InteractionState>,
    selected: Signal<usize>,
    labels: Rc<Vec<String>>,
    /// Tracks whether the dropdown overlay is currently visible.
    /// Drives the accessibility `set_expanded()` state so AT announces
    /// "collapsed" / "expanded" as the menu opens and closes.
    menu_open: Signal<bool>,
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
            variant: ButtonVariant::Plain,
            initial_enabled: true,
            initial_selected: 0,
            promote_on_select: true,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            chevron_tooltip_text: None,
            chevron_rich_tooltip_source: None,
            chevron_composite_tooltip_content: None,
            interaction: Signal::new(InteractionState::Idle),
            selected: Signal::new(0),
            labels: Rc::new(Vec::new()),
            menu_open: Signal::new(false),
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

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. Use `ctx.enabled_when(button_id, signal)` for reactivity.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
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
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip to the main region.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip to the main region driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip to the main region.
    pub fn composite_tooltip(
        mut self,
        content: impl bastyde_core::widget::Widget + 'static,
    ) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Override the tooltip shown on hover over the trailing chevron
    /// region. When unset, the chevron gets a default "Show dropdown
    /// menu" tooltip so its affordance isn't silent.
    pub fn chevron_tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.chevron_tooltip_text = Some(text.into());
        self.chevron_rich_tooltip_source = None;
        self.chevron_composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip to the chevron region.
    pub fn chevron_rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.chevron_rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.chevron_tooltip_text = None;
        self.chevron_composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip to the chevron region driven by inline `TooltipContent`.
    pub fn chevron_rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.chevron_rich_tooltip_source =
            Some(crate::tooltip::RichTooltipSource::Content(content));
        self.chevron_tooltip_text = None;
        self.chevron_composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip to the chevron region.
    pub fn chevron_composite_tooltip(
        mut self,
        content: impl bastyde_core::widget::Widget + 'static,
    ) -> Self {
        self.chevron_composite_tooltip_content = Some(Box::new(content));
        self.chevron_tooltip_text = None;
        self.chevron_rich_tooltip_source = None;
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
            .field("style", &self.variant)
            .field("initial_enabled", &self.initial_enabled)
            .finish()
    }
}

// --- Color resolution (variant × state × theme) ---
//
// Mirrors Button::resolve_bg / resolve_text / resolve_border so a Button
// and a SplitButton with the same variant look identical. If Button's
// color tables ever diverge from these, update both sides.

// SplitButton has not yet migrated to the trait-driven `ButtonStyle`
// surface. For now it normalises the 7-value `ButtonVariant` down to
// the three buckets it knows how to paint: `Filled` family
// (Filled / Destructive), `Plain` family (Plain / Tinted / Outlined),
// `Ghost` family (Ghost / Link).
#[derive(Copy, Clone, Eq, PartialEq)]
#[allow(clippy::enum_variant_names)]
enum SplitButtonFamily {
    FilledLike,
    PlainLike,
    GhostLike,
}

fn classify(variant: ButtonVariant) -> SplitButtonFamily {
    match variant {
        ButtonVariant::Filled | ButtonVariant::Destructive => SplitButtonFamily::FilledLike,
        ButtonVariant::Plain | ButtonVariant::Tinted | ButtonVariant::Outlined => {
            SplitButtonFamily::PlainLike
        }
        ButtonVariant::Ghost | ButtonVariant::Link => SplitButtonFamily::GhostLike,
    }
}

fn resolve_bg_role(variant: ButtonVariant, state: InteractionState) -> SurfaceRole {
    match (classify(variant), state) {
        (SplitButtonFamily::FilledLike, InteractionState::Disabled) => SurfaceRole::AccentDisabled,
        (SplitButtonFamily::FilledLike, InteractionState::Pressed) => SurfaceRole::AccentPressed,
        (SplitButtonFamily::FilledLike, InteractionState::Hovered) => SurfaceRole::AccentHover,
        (SplitButtonFamily::FilledLike, _) => SurfaceRole::Accent,

        (SplitButtonFamily::PlainLike, InteractionState::Pressed) => SurfaceRole::Pressed,
        (SplitButtonFamily::PlainLike, InteractionState::Hovered) => SurfaceRole::Hover,
        (SplitButtonFamily::PlainLike, _) => SurfaceRole::Main,

        (SplitButtonFamily::GhostLike, InteractionState::Pressed) => SurfaceRole::Pressed,
        (SplitButtonFamily::GhostLike, InteractionState::Hovered) => SurfaceRole::Hover,
        (SplitButtonFamily::GhostLike, _) => SurfaceRole::Transparent,
    }
}

fn resolve_text_role(variant: ButtonVariant, state: InteractionState) -> TextRole {
    if state == InteractionState::Disabled {
        return TextRole::Disabled;
    }
    match classify(variant) {
        SplitButtonFamily::FilledLike => TextRole::OnAccent,
        SplitButtonFamily::PlainLike | SplitButtonFamily::GhostLike => TextRole::Primary,
    }
}

fn resolve_border_role(variant: ButtonVariant, state: InteractionState) -> BorderRole {
    if state == InteractionState::Focused {
        return BorderRole::Focused;
    }
    match classify(variant) {
        SplitButtonFamily::FilledLike | SplitButtonFamily::GhostLike => BorderRole::Transparent,
        SplitButtonFamily::PlainLike => match state {
            InteractionState::Hovered | InteractionState::Pressed => BorderRole::Strong,
            _ => BorderRole::Default,
        },
    }
}

/// Border width for the SplitButton frame: thickens to the theme's
/// `focus_ring_width` on focus, rests at the variant's normal
/// width otherwise.
fn resolve_border_width(
    variant: ButtonVariant,
    state: InteractionState,
    normal_bw: f32,
    focus_bw: f32,
) -> f32 {
    if state == InteractionState::Focused {
        return focus_bw;
    }
    match classify(variant) {
        SplitButtonFamily::FilledLike | SplitButtonFamily::GhostLike => 0.0,
        SplitButtonFamily::PlainLike => normal_bw,
    }
}

impl Widget for SplitButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let normal_bw = SPLIT_BUTTON_BORDER_WIDTH;
        let focus_bw = ctx.theme().shape.focus_ring_width;
        let style = self.variant;
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        // `effective_enabled` available if the chrome ever needs it
        // for reactive `is_disabled` derivation. Today the leaves
        // handle disabled-color substitution via their own paint
        // (`PaintContext::effective_enabled`).
        let _effective_enabled = ctx.effective_enabled_signal(self_id);

        // ---- Extract label / action for each MenuItem and wrap each item's
        // activation so selecting it from the menu also promotes its index
        // to the current default. ----

        let mut labels_vec: Vec<String> = Vec::new();
        let mut actions_vec: Vec<Option<Rc<dyn Fn(&mut EventContext)>>> = Vec::new();
        // Split button menus always open Below the trigger (the chevron
        // half lives at the bottom-right of the button), so the menu's
        // top edge is attached to the trigger.
        let mut menu = MenuList::new().attached_side(crate::shadow::AttachedSide::Top);

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

        // ---- Menu-open tracker (drives accessibility set_expanded). ----
        // `selected` also feeds the a11y name, so bind it AccessibilityOnly
        // so AT updates when the promoted item changes without relayout.
        let menu_open = self.menu_open.clone();
        let self_id_for_bindings = ctx.self_id();
        menu_open.bind_to(
            self_id_for_bindings,
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );
        selected.bind_to(
            self_id_for_bindings,
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        // ---- Interaction state signal ----
        // Seeded to Idle; never carries Disabled. The framework gates
        // event dispatch on `arena.is_enabled(self_id)`, so disabled
        // SplitButtons simply don't receive events. Style chrome
        // reads `is_disabled` from `effective_enabled` if needed.
        let interaction = ctx.signal(InteractionState::Idle);
        self.interaction = interaction.clone();

        // Subtree hover signal — the framework writes `true` whenever the
        // pointer is over a strict descendant of the row container (main
        // region, divider, or chevron region). Replaces per-region
        // `on_hover` handlers with a single `hover_within` binding on the
        // row HStack below.
        let hovered_signal = ctx.signal(false);
        ctx.effect(&hovered_signal, {
            let interaction = interaction.clone();
            move |entered| {
                // Pressed / Focused are owned by on_tap, on_key,
                // and on_focus; only flip the ambient Idle <-> Hovered pair.
                match interaction.get() {
                    InteractionState::Pressed
                    | InteractionState::Focused
                    | InteractionState::Disabled => {}
                    _ => {
                        interaction.set(if *entered {
                            InteractionState::Hovered
                        } else {
                            InteractionState::Idle
                        });
                    }
                }
            }
        });

        // ---- Derived reactive roles ----
        let bg_role = interaction.map(move |s| resolve_bg_role(style, *s));
        let text_role = interaction.map(move |s| resolve_text_role(style, *s));
        let border_role = interaction.map(move |s| resolve_border_role(style, *s));
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
        let label_widget = TextWidget::new(lit!(""))
            .bind_text(main_label_text)
            .bind_color(text_role.clone())
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label_widget);

        let main_padding_id = ctx.add(
            Padding::symmetric(
                SPLIT_BUTTON_PADDING_VERTICAL,
                SPLIT_BUTTON_PADDING_HORIZONTAL,
            )
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
            MinSize::new(SPLIT_BUTTON_MIN_WIDTH, SPLIT_BUTTON_HEIGHT)
                .child_id(main_content_id)
                .on_tap(move |_pos, ctx: &mut EventContext| {
                    let idx = selected_for_tap.get();
                    if let Some(Some(action)) = actions_for_tap.get(idx) {
                        action(ctx);
                    }
                })
                .cursor(CursorIcon::Pointer)
        };
        let main_region_id = ctx.add(main_region);

        // Attach the main-region tooltip if configured. Three
        // mutually-exclusive setters; setters clear the others.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, main_region_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, main_region_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(main_region_id, tooltip_id, delay);
        }

        // ---- Divider between main and chevron regions ----
        let divider_fill_id =
            ctx.add(RectWidget::new().background(bastyde_tokens::BorderRole::Default));
        let divider_id = ctx.add(
            FixedSize::new()
                .bind_width(SPLIT_BUTTON_DIVIDER_WIDTH)
                .bind_height(SPLIT_BUTTON_HEIGHT)
                .child_id(divider_fill_id),
        );

        // ---- Chevron region ----
        let chevron_icon_id = ctx.add(
            IconWidget::chevron_down(SPLIT_BUTTON_CHEVRON_ICON_SIZE).bind_color(text_role.clone()),
        );
        let chevron_centered_id = ctx.add(Center::new().child_id(chevron_icon_id));

        let chevron_region = {
            let int_for_tap = interaction.clone();
            FixedSize::new()
                .bind_width(SPLIT_BUTTON_CHEVRON_WIDTH)
                .bind_height(SPLIT_BUTTON_HEIGHT)
                .child_id(chevron_centered_id)
                .on_tap({
                    let menu_open = self.menu_open.clone();
                    move |_pos, ctx: &mut EventContext| {
                        int_for_tap.set(InteractionState::Pressed);
                        ctx.activate(menu_id);
                        menu_open.set(true);
                        let on_dismiss_open = menu_open.clone();
                        ctx.show_overlay(OverlayRequest {
                            content_id: menu_id,
                            anchor: self_id,
                            placement: OverlayPlacement::BelowPreferred,
                            dismiss: DismissBehavior::EscapeOrClickOutside,
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                            on_dismiss: Some(Rc::new(move || on_dismiss_open.set(false))),
                            fade_duration: None,
                        });
                        // The MenuList owns the keyboard-navigation handler
                        // (ArrowUp/ArrowDown/Enter/Escape) and that handler
                        // only fires when the MenuList is focused. Hand focus
                        // over so the user can immediately keyboard-walk the
                        // items they just opened.
                        ctx.request_focus(menu_id);
                    }
                })
                .cursor(CursorIcon::Pointer)
        };
        let chevron_region_id = ctx.add(chevron_region);

        // Attach the chevron tooltip. Defaults to "Show dropdown menu"
        // so the bare ▾ affordance is never silent — the caller can
        // override via `.chevron_tooltip(...)` (plain),
        // `.chevron_rich_tooltip(...)`, or `.chevron_composite_tooltip(...)`.
        if let Some(content) = self.chevron_composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, chevron_region_id, content, delay);
        } else if let Some(source) = self.chevron_rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, chevron_region_id, source, delay);
        } else {
            let chevron_text = self
                .chevron_tooltip_text
                .clone()
                .unwrap_or_else(|| lit!("Show dropdown menu"));
            let tooltip_widget = crate::tooltip::TooltipWidget::new(chevron_text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(chevron_region_id, tooltip_id, delay);
        }

        // ---- Row: main | divider | chevron ----
        // `hover_within` writes `hovered_signal` whenever the pointer is
        // over a strict descendant of this HStack — i.e. main_region,
        // divider, or chevron_region — driving the unified Hovered halo.
        let row_id = ctx.add(
            HStack::new()
                .spacing(0.0)
                .add_child(main_region_id)
                .add_child(divider_id)
                .add_child(chevron_region_id)
                .hover_within(hovered_signal),
        );

        // Border width reacts to focus state — thickens to the
        // accent `focus_ring_width` on focus, matching the Int UI
        // convention applied uniformly across all input widgets.
        let border_width =
            interaction.map(move |s| resolve_border_width(style, *s, normal_bw, focus_bw));

        // ---- Shared frame (single RectWidget behind the row) ----
        let bg_rect = RectWidget::new()
            .bind_background(bg_role)
            .bind_border_color(border_role)
            .bind_border_width(border_width)
            .corner_radius(CornerRadius::uniform(SPLIT_BUTTON_CORNER_RADIUS));
        let bg_id = ctx.add(bg_rect);

        let frame_id = ctx.add(ZStack::new().add_child(bg_id).add_child(row_id));

        // Enforce an overall minimum size: main min_width + divider + chevron.
        let total_min_width =
            SPLIT_BUTTON_MIN_WIDTH + SPLIT_BUTTON_DIVIDER_WIDTH + SPLIT_BUTTON_CHEVRON_WIDTH;
        let root_id =
            ctx.add(MinSize::new(total_min_width, SPLIT_BUTTON_HEIGHT).child_id(frame_id));
        self.root_child_id = Some(root_id);

        // ---- Self handlers: the SplitButton is the single focus stop.
        // Space/Enter fires the current default; ArrowDown opens the menu.
        let actions_for_key = actions_rc.clone();
        let selected_for_key = selected.clone();
        let int_for_key = interaction.clone();
        let int_for_focus = interaction.clone();
        let menu_open_for_key = self.menu_open.clone();

        let handler_set = HandlerSet::new()
            .on_key(
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
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
                            menu_open_for_key.set(true);
                            let on_dismiss_key = menu_open_for_key.clone();
                            ctx.show_overlay(OverlayRequest {
                                content_id: menu_id,
                                anchor: self_id,
                                placement: OverlayPlacement::BelowPreferred,
                                dismiss: DismissBehavior::EscapeOrClickOutside,
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: Some(Rc::new(move || on_dismiss_key.set(false))),
                                fade_duration: None,
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
            .focusable(true);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
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
        builder.set_role(bastyde_core::accesskit::Role::Button);
        if !self.labels.is_empty() {
            let idx = self.selected.get().min(self.labels.len() - 1);
            builder.set_name(self.labels[idx].as_str());
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.set_has_popup(bastyde_core::accesskit::HasPopup::Menu);
        builder.set_expanded(self.menu_open.get());
        builder.add_action(bastyde_core::accesskit::Action::Click);
        builder.add_action(bastyde_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}
