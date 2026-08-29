// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
//! ```rust
//! # use teksilo_widgets::{SplitButton, MenuItem, ButtonVariant};
//! # use teksilo_i18n::lit;
//! # use teksilo_core::Intent;
//! let _w = SplitButton::new()
//!     .item(MenuItem::new(lit!("Run")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.run"))))
//!     .item(MenuItem::new(lit!("Run Tests")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.run-tests"))))
//!     .separator()
//!     .item(MenuItem::new(lit!("Debug")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.debug"))))
//!     .variant(ButtonVariant::Plain);
//! ```

use std::rc::Rc;
use teksilo_i18n::lit;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{SharedSplitButtonStyle, SplitButtonStyle, SplitButtonStyleConfig};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextRole;

use crate::button::{ButtonVariant, InteractionState};
use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;
use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, MinSize, Padding, RectWidget, TextWidget, ZStack,
};
use teksilo_i18n::LocalizedString;

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
/// Gap between an optional main-region leading icon and the label.
pub const SPLIT_BUTTON_ICON_LABEL_GAP: f32 = 6.0;

/// A button split into a default-action region and a chevron dropdown region.
///
/// See the [module-level documentation](self) for a usage overview.
pub struct SplitButton {
    rows: Vec<Row>,
    variant: ButtonVariant,
    /// Per-call Tier-3 chrome override. `None` ⇒ theme slot ⇒ the built-in
    /// `RecipeSplitButtonStyle`.
    style_override: Option<SharedSplitButtonStyle>,
    /// Per-call override for the main-region label text style (font, size,
    /// weight). `None` ⇒ the inner `TextWidget` default.
    label_style: Option<teksilo_core::color_prop::TextStyleProp>,
    /// Per-call override for the main-region label text color. `None` ⇒ the
    /// variant/interaction-derived cascade; setting this replaces it.
    text_role_override: Option<teksilo_core::color_prop::ColorProp>,
    /// Optional leading icon for the main (default-action) region, rendered
    /// before the label (mirrors `Button`'s `IconLocation::Leading`). The
    /// dropdown rows carry their own `MenuItem::icon`s independently.
    icon: Option<IconWidget>,
    /// Enabled state, static or reactive; forwarded to the arena at build
    /// time.
    enabled: Prop<bool>,
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
    composite_tooltip_content: Option<Box<dyn teksilo_core::widget::Widget>>,
    /// Tooltip shown on hover over the trailing chevron region. Falls
    /// back to a generic "Show dropdown menu" label when not explicitly
    /// set, since the chevron region has no label of its own.
    chevron_tooltip_text: Option<LocalizedString>,
    /// Rich tooltip source for the chevron region.
    chevron_rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Composite tooltip body for the chevron region.
    chevron_composite_tooltip_content: Option<Box<dyn teksilo_core::widget::Widget>>,
    // Build state
    interaction: Signal<InteractionState>,
    selected: Signal<usize>,
    /// Unresolved labels mirrored from the menu items, kept as
    /// `LocalizedString` (not snapshots) so the main-region label and AT
    /// name follow a live locale switch — `build` re-resolves them through
    /// a locale-zipped signal and `accessibility` re-resolves on each walk.
    labels: Rc<Vec<LocalizedString>>,
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
            style_override: None,
            label_style: None,
            text_role_override: None,
            icon: None,
            enabled: Prop::Static(true),
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

    /// Set the visual style variant (filled, plain, ghost, …) for the entire
    /// button frame. Mirrors the same variants as
    /// [`Button::variant`](crate::button::Button::variant).
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set a leading icon for the main (default-action) region, rendered before
    /// the label (mirrors [`Button::icon`](crate::button::Button::icon) with
    /// `IconLocation::Leading`). Unlike the per-row `MenuItem::icon`s, this glyph
    /// is fixed regardless of which item is the current default — use it for a
    /// stable action affordance (e.g. a "＋" add glyph).
    ///
    /// The icon's tint follows the main-region label (the variant/interaction
    /// cascade, or [`text_role`](Self::text_role) when overridden), so any
    /// colour set on the passed `IconWidget` is replaced — same contract as
    /// `Button`. Its size is left alone, so `.icon_size(..)` on the caller's
    /// widget is honoured.
    pub fn icon(mut self, icon: IconWidget) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Override the Tier-3 frame chrome for this instance. Takes precedence
    /// over `theme.style_slots.split_button` and the built-in
    /// `RecipeSplitButtonStyle`.
    pub fn style(mut self, style: impl SplitButtonStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Override the main-region label text style (font, size, weight).
    /// Accepts a `TextStyleRole`, a `TextStyle`, or a `Signal` of either.
    /// Default (unset) is the inner `TextWidget` default — e.g. pass
    /// `TextStyleRole::BodyBold` for a bold default action.
    pub fn text_style(mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// Override the control's text colour — the main-region label, its
    /// leading [`icon`](Self::icon), and the chevron, which the
    /// variant/interaction cascade tints together. Accepts `Color`, a role,
    /// or a `Signal` of either. Default (unset) is that cascade; setting this
    /// replaces it wholesale (loses hover/disabled tint).
    pub fn text_role(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self {
        self.text_role_override = Some(color.into());
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena at build time.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
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
        content: impl teksilo_core::widget::Widget + 'static,
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
        content: impl teksilo_core::widget::Widget + 'static,
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
            .field("enabled", &self.enabled.get())
            .finish()
    }
}

// --- Text-color resolution (variant × state) ---
//
// Only the default-action label and chevron icon colour are resolved here;
// the frame background / border moved to `RecipeSplitButtonStyle`. Mirrors
// `Button::resolve_text_role` so a Button and a SplitButton with the same
// variant read identically — keep them in lockstep if Button's text table
// changes.

// SplitButton normalises the 7-value `ButtonVariant` down to the three
// buckets it knows how to paint: `Filled` family (Filled / Destructive),
// `Plain` family (Plain / Tinted / Outlined), `Ghost` family (Ghost / Link).
// `classify` is shared with the Tier-3 `RecipeSplitButtonStyle` (frame
// background / border) so the frame and the widget-owned text colour stay in
// lockstep; the widget keeps `resolve_text_role` (mirrors how `Button` keeps
// its own text-role resolution while delegating chrome to `ButtonStyle`).
#[derive(Copy, Clone, Eq, PartialEq)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum SplitButtonFamily {
    FilledLike,
    PlainLike,
    GhostLike,
}

pub(crate) fn classify(variant: ButtonVariant) -> SplitButtonFamily {
    match variant {
        ButtonVariant::Filled | ButtonVariant::Destructive => SplitButtonFamily::FilledLike,
        ButtonVariant::Plain | ButtonVariant::Tinted | ButtonVariant::Outlined => {
            SplitButtonFamily::PlainLike
        }
        ButtonVariant::Ghost | ButtonVariant::Link => SplitButtonFamily::GhostLike,
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

impl Widget for SplitButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let variant = self.variant;
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        // Drives the style's reactive `is_disabled` (custom chrome may dim
        // the frame). The recipe default leaves the frame undimmed and the
        // leaves substitute disabled colours in their own paint.
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Resolve the active frame chrome: per-call override > theme slot >
        // built-in `RecipeSplitButtonStyle`.
        let split_style: SharedSplitButtonStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.split_button.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeSplitButtonStyle));

        // ---- Extract label / action for each MenuItem and wrap each item's
        // activation so selecting it from the menu also promotes its index
        // to the current default. ----

        let mut labels_vec: Vec<LocalizedString> = Vec::new();
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
                    let label = item.label_localized();
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

        // ---- Derived reactive text role (frame bg/border live in the style) ----
        // Text colour stays a widget concern (mirrors Button's
        // `resolve_text_role`); it tints the default-action label, its leading
        // icon, and the chevron — all three go through `label_color` below, so
        // a `text_role(..)` override replaces the cascade for the whole
        // control. The frame background / border is resolved inside the active
        // `SplitButtonStyle` from the interaction bools built below.
        let text_role = interaction.map(move |s| resolve_text_role(variant, *s));
        // The divider is a RectWidget used as a 1-dp vertical rule; role-based
        // so it follows theme changes without an intermediate signal.

        // ---- Main-region label bound to `selected` ----
        let main_label_text = {
            let labels = labels_rc.clone();
            // Zip the locale signal so the displayed default-action label
            // re-resolves on a locale switch, not only on selection change.
            selected.zip(&ctx.locale_signal()).map(move |(i, _)| {
                if labels.is_empty() {
                    String::new()
                } else {
                    labels[(*i).min(labels.len() - 1)].resolve_now()
                }
            })
        };

        // ---- Pre-register the menu overlay (dormant until opened) ----
        // Built the first time the popup is opened, not on every rebuild of the
        // field. See `teksilo_core::deferred_subtree::DeferredSubtree`.
        let menu_id = ctx.add_deferred(self.menu_open.clone(), menu);
        ctx.set_dormant(menu_id);
        self.menu_content_id = Some(menu_id);

        let self_id = ctx.self_id();

        // ---- Main region subtree ----
        let label_color: teksilo_core::color_prop::ColorProp = self
            .text_role_override
            .clone()
            .unwrap_or_else(|| text_role.clone().into());
        let mut label_widget = TextWidget::new(lit!(""))
            .text(main_label_text)
            .color(label_color.clone())
            .single_line()
            .a11y_hidden();
        if let Some(style) = &self.label_style {
            label_widget = label_widget.style(style.clone());
        }
        let label_id = ctx.add(label_widget);

        // Optional leading icon in the main region: `[icon, gap, label]` inside
        // the padding (mirrors Button's `IconLocation::Leading`). When no icon is
        // set, the label goes straight into the padding — node count unchanged.
        //
        // The glyph is tinted with the *label's* colour, exactly as
        // `Button::make_icon` does — an untinted icon keeps `IconWidget`'s
        // default `TextRole::Primary`, which silently matches on a light theme
        // (`text_primary` and `text_on_accent` are both black) and then paints
        // near-white on the accent fill in dark mode.
        let main_inner_id = if let Some(icon) = self.icon.take() {
            let icon_id = ctx.add(icon.color(label_color.clone()));
            ctx.add(
                HStack::new()
                    .spacing(SPLIT_BUTTON_ICON_LABEL_GAP)
                    .add_child(icon_id)
                    .add_child(label_id),
            )
        } else {
            label_id
        };

        let main_padding_id = ctx.add(
            Padding::symmetric(
                SPLIT_BUTTON_PADDING_VERTICAL,
                SPLIT_BUTTON_PADDING_HORIZONTAL,
            )
            .child_id(main_inner_id),
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
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, main_region_id, text, delay);
        }

        // ---- Divider between main and chevron regions ----
        let divider_fill_id =
            ctx.add(RectWidget::new().background(teksilo_tokens::BorderRole::Default));
        let divider_id = ctx.add(
            FixedSize::new()
                .width(SPLIT_BUTTON_DIVIDER_WIDTH)
                .height(SPLIT_BUTTON_HEIGHT)
                .child_id(divider_fill_id),
        );

        // ---- Chevron region ----
        // Tinted with `label_color`, not the raw `text_role` cascade: the
        // cascade is control-wide (one `interaction` signal fed by
        // `hover_within` across main region + divider + chevron), so a
        // `text_role(..)` override that reached only the main region would
        // split a previously-unified tint — a `.text_role(Error)` Filled
        // button would paint a red label beside a black chevron.
        let chevron_icon_id = ctx.add(
            IconWidget::chevron_down(SPLIT_BUTTON_CHEVRON_ICON_SIZE).color(label_color.clone()),
        );
        let chevron_centered_id = ctx.add(Center::new().child_id(chevron_icon_id));

        let chevron_region = {
            let int_for_tap = interaction.clone();
            FixedSize::new()
                .width(SPLIT_BUTTON_CHEVRON_WIDTH)
                .height(SPLIT_BUTTON_HEIGHT)
                .child_id(chevron_centered_id)
                .on_tap({
                    let menu_open = self.menu_open.clone();
                    move |_pos, ctx: &mut EventContext| {
                        int_for_tap.set(InteractionState::Pressed);
                        // Build the popup if this is its first open, before the overlay
                        // below is measured against it and focus moves into it.
                        ctx.materialize_now(menu_id);
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
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, chevron_region_id, chevron_text, delay);
        }

        // ---- Row: main | divider | chevron ----
        // `hover_within` writes `hovered_signal` whenever the pointer is
        // over a strict descendant of this HStack — i.e. main_region,
        // divider, or chevron_region — driving the unified Hovered halo.
        // This assembled row is the interactive `content` the style frames.
        let content_id = ctx.add(
            HStack::new()
                .spacing(0.0)
                .add_child(main_region_id)
                .add_child(divider_id)
                .add_child(chevron_region_id)
                .hover_within(hovered_signal),
        );

        // ---- Delegate the shared frame chrome to the Tier-3 style ----
        // The style owns the background fill, border, corner radius, and
        // overall min size; we hand it the interactive row plus the live
        // interaction bools (derived from the single `interaction` enum,
        // which carries exactly one transient state at a time).
        let cfg = SplitButtonStyleConfig {
            content: content_id,
            is_pressed: interaction.map(|s| *s == InteractionState::Pressed),
            is_hovered: interaction.map(|s| *s == InteractionState::Hovered),
            // `:focus-visible`: keyboard-only focus ring (gate raw focus on
            // the input-modality signal).
            is_focused: interaction
                .map(|s| *s == InteractionState::Focused)
                .and(&ctx.focus_visible()),
            is_disabled: effective_enabled.map(|on| !*on),
            variant,
        };
        let root_id = split_style.make_body(&cfg, ctx);
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
                            // Lone-KeyUp guard: only fire if we saw the
                            // matching KeyDown (state is Pressed). A lone
                            // KeyUp means the KeyDown was consumed
                            // elsewhere (shortcut, focus transfer) and
                            // this widget is not the activation target.
                            // Mirrors `build_interaction_handlers`.
                            if int_for_key.get() != InteractionState::Pressed {
                                return EventResponse::Ignored;
                            }
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
                            // Build the popup if this is its first open, before the overlay
                            // below is measured against it and focus moves into it.
                            ctx.materialize_now(menu_id);
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
            // `accessibility` exposes ONE node (this one) advertising
            // `Action::Click`, but the pointer handlers live on the
            // descendant main / chevron regions — an AT click dispatched
            // to this node never reaches them (preview walks strict
            // ancestors, bubble walks target → root; neither descends).
            // Fire the current default action, mirroring Enter/Space.
            .on_access_action({
                let actions = actions_rc.clone();
                let selected = selected.clone();
                move |action, ctx: &mut EventContext| {
                    if action == teksilo_core::accesskit::Action::Click {
                        if let Some(Some(default_action)) = actions.get(selected.get()) {
                            default_action(ctx);
                        }
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .focusable(true);

        ctx.apply_self_handlers(handler_set);

        // Return BOTH the visible root AND the dormant menu content so
        // the framework links `menu_id` under this SplitButton in the
        // arena. Without this the menu stays an orphan root: it leaks on
        // `destroy_subtree` (never reached from this widget's child list)
        // and `arena.hit_test_at` walks its subtree on every click even
        // while dormant. Mirrors `PopoverButton::build`. The layout pass
        // skips dormant children automatically; `place_children` zeroes
        // the slot if it ever surfaces active.
        vec![root_id, menu_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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
        // The visible row fills our bounds; the menu content is owned by
        // the overlay manager when shown and stays zero-sized otherwise.
        // Dormant children are filtered out before placements reach here;
        // zero the menu slot defensively if it ever surfaces active so we
        // don't clobber the overlay's own positioning.
        for child in children.iter_mut() {
            if Some(child.id) == self.menu_content_id {
                child.size = teksilo_canvas::Size::ZERO;
                continue;
            }
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Button);
        if !self.labels.is_empty() {
            let idx = self.selected.get().min(self.labels.len() - 1);
            builder.set_name(self.labels[idx].resolve_now());
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.set_has_popup(teksilo_core::accesskit::HasPopup::Menu);
        builder.set_expanded(self.menu_open.get());
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        // Include the dormant menu content alongside the visible root so
        // `set_dormant` cascades correctly and `arena.hit_test_at` can
        // prune the menu subtree when it isn't visible.
        let mut out = Vec::new();
        if let Some(id) = self.root_child_id {
            out.push(id);
        }
        if let Some(id) = self.menu_content_id {
            out.push(id);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell as StdCell;
    use std::rc::Rc as StdRc;
    use teksilo_core::event::Modifiers;
    use teksilo_core::widget_tree::WidgetTree;

    fn themed_tree() -> WidgetTree {
        WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
    }

    /// Regression: the dropdown menu must be linked as a child of the
    /// SplitButton, not left as an orphan arena root. An orphan root
    /// leaks on `destroy_subtree` (never reached from the widget's child
    /// list) and is walked by `hit_test_at` on every click. Mirrors the
    /// content-linking contract `PopoverButton` documents.
    #[test]
    fn menu_content_is_linked_as_child_not_orphan_root() {
        let mut tree = themed_tree();
        let split = tree.add(
            SplitButton::new()
                .item(MenuItem::new(lit!("Save")))
                .item(MenuItem::new(lit!("Save As"))),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));

        let children = tree.children(split);
        assert_eq!(
            children.len(),
            2,
            "SplitButton must expose both the visible root and the dormant menu"
        );
        let menu_id = children[1];
        assert_eq!(
            tree.parent(menu_id),
            Some(split),
            "menu must be parented under the SplitButton, not left an orphan root"
        );
    }

    /// Enter activates the *currently-selected* item's action — the core
    /// SplitButton contract (the primary region fires the default).
    #[test]
    fn enter_fires_current_default_action() {
        let fired: StdRc<StdCell<Option<usize>>> = StdRc::new(StdCell::new(None));
        let (f0, f1) = (fired.clone(), fired.clone());
        let mut tree = themed_tree();
        let split = tree.add(
            SplitButton::new()
                .initial_selected(1)
                .item(MenuItem::new(lit!("A")).on_activate_fn(move |_| f0.set(Some(0))))
                .item(MenuItem::new(lit!("B")).on_activate_fn(move |_| f1.set(Some(1)))),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.focus(split);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(
            fired.get(),
            Some(1),
            "Enter must fire the currently-selected item's action"
        );
    }

    /// The SplitButton exposes ONE a11y node advertising `Action::Click`,
    /// but the pointer handlers live on descendant regions the dispatch
    /// never reaches. An AT / automation click must therefore fire the
    /// current default action, exactly like Enter.
    #[test]
    fn access_click_fires_current_default_action() {
        let fired: StdRc<StdCell<Option<usize>>> = StdRc::new(StdCell::new(None));
        let (f0, f1) = (fired.clone(), fired.clone());
        let mut tree = themed_tree();
        let split = tree.add(
            SplitButton::new()
                .initial_selected(1)
                .item(MenuItem::new(lit!("A")).on_activate_fn(move |_| f0.set(Some(0))))
                .item(MenuItem::new(lit!("B")).on_activate_fn(move |_| f1.set(Some(1)))),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(split),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        assert_eq!(
            fired.get(),
            Some(1),
            "AT click must fire the currently-selected item's action"
        );
    }

    /// A lone Space/Enter KeyUp (no preceding KeyDown) must NOT fire the
    /// default action — the lone-KeyUp guard, matching the rest of the
    /// button family.
    #[test]
    fn lone_keyup_does_not_fire_default_action() {
        let fired: StdRc<StdCell<u32>> = StdRc::new(StdCell::new(0));
        let f = fired.clone();
        let mut tree = themed_tree();
        let split = tree.add(
            SplitButton::new()
                .item(MenuItem::new(lit!("A")).on_activate_fn(move |_| f.set(f.get() + 1))),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.focus(split);

        // Lone KeyUp — must be a no-op.
        tree.dispatch_event(teksilo_core::event::WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            fired.get(),
            0,
            "lone KeyUp must not fire the default action"
        );

        // Sanity: a full KeyDown+KeyUp DOES fire.
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(
            fired.get(),
            1,
            "full KeyDown+KeyUp fires the default action"
        );
    }

    /// A per-call `.style(...)` override is consulted: the custom
    /// `SplitButtonStyle::make_body` runs and frames the interactive content.
    #[test]
    fn custom_style_make_body_is_invoked() {
        struct MarkerStyle(StdRc<StdCell<bool>>);
        impl SplitButtonStyle for MarkerStyle {
            fn make_body(&self, cfg: &SplitButtonStyleConfig, _ctx: &mut BuildContext) -> WidgetId {
                self.0.set(true);
                // Frame the pre-built interactive row verbatim.
                cfg.content
            }
        }

        let fired = StdRc::new(StdCell::new(false));
        let mut tree = themed_tree();
        tree.add(
            SplitButton::new()
                .item(MenuItem::new(lit!("A")))
                .style(MarkerStyle(fired.clone())),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));
        assert!(
            fired.get(),
            "a per-call SplitButtonStyle override must drive the frame chrome"
        );
    }

    /// ArrowDown opens the dropdown menu overlay.
    #[test]
    fn arrow_down_opens_the_menu() {
        let mut tree = themed_tree();
        let split = tree.add(
            SplitButton::new()
                .item(MenuItem::new(lit!("A")))
                .item(MenuItem::new(lit!("B"))),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.focus(split);
        assert!(tree.active_overlays().is_empty());
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "ArrowDown must open the dropdown menu overlay"
        );
    }

    /// How many path leaves in the rendered frame paint at `expected`. The
    /// leading icon and the chevron are the glyphs a SplitButton draws; the
    /// label is a text run, so it never shows up here.
    fn paths_colored(frame: &teksilo_canvas::RenderFrame, expected: [f32; 4]) -> usize {
        frame.paths.iter().filter(|p| p.color == expected).count()
    }

    /// Regression: the main region's leading icon must be tinted with the
    /// label's colour, not left on `IconWidget`'s default `TextRole::Primary`.
    ///
    /// This only shows up on a dark theme. In `intui::light` `text_primary`
    /// and `text_on_accent` are *both* `#000000`, so an untinted glyph looks
    /// correct by coincidence; in `intui::dark` `text_primary` is `#DFE1E5`
    /// against a black `text_on_accent`, so the untinted "＋" painted white on
    /// the accent fill while the label beside it stayed black.
    #[test]
    fn filled_leading_icon_is_tinted_like_the_label_in_dark_mode() {
        let theme = teksilo_core::presets::intui::dark();
        let mut tree = WidgetTree::new().with_theme(theme.clone());
        tree.add(
            SplitButton::new_static()
                .variant(ButtonVariant::Filled)
                .icon(IconWidget::checkmark(14.0))
                .item(MenuItem::new(lit!("Scene"))),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let frame = tree.render();

        assert_eq!(
            paths_colored(&frame, theme.colors.text_on_accent.to_array()),
            2,
            "both the leading icon and the chevron must paint at text_on_accent"
        );
        assert_eq!(
            paths_colored(&frame, theme.colors.text_primary.to_array()),
            0,
            "no glyph may fall back to IconWidget's default text_primary on an accent fill"
        );
    }

    /// The tint follows `text_role(..)` when the caller overrides it — the
    /// icon and the label stay in lockstep rather than the icon falling back
    /// to the variant cascade.
    #[test]
    fn leading_icon_follows_the_text_role_override() {
        let theme = teksilo_core::presets::intui::dark();
        let mut tree = WidgetTree::new().with_theme(theme.clone());
        tree.add(
            SplitButton::new_static()
                .variant(ButtonVariant::Filled)
                .text_role(teksilo_tokens::TextRole::Error)
                .icon(IconWidget::checkmark(14.0))
                .item(MenuItem::new(lit!("Delete"))),
        );
        tree.layout(SizeProposal::exact(300.0, 60.0));

        assert_eq!(
            paths_colored(&tree.render(), theme.colors.text_error.to_array()),
            2,
            "text_role(..) must retint the leading icon, not just the label"
        );
    }
}
