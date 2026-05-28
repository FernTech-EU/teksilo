//! MenuItem widget — a single item in a menu or context menu.
//!
//! Non-generic, closure-based command erasure (same pattern as Button).
//! Supports icons, shortcut labels, disabled state, and submenu triggers.

use bastyde_i18n::lit;
use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{MenuItemStyleConfig, SharedMenuItemStyle};
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::keystroke_format::format_keystroke;
use crate::primitives::{HStack, IconWidget, Spacer, TextWidget};

/// Type-erased command factory. Stored as `Rc` (not `Box`) so the closure
/// can be cloned and shared — in particular with SplitButton, which reads
/// the action out of a MenuItem via `MenuItem::action()` and re-fires it
/// from its main region without disturbing the MenuItem's own use of it.
type CommandFactory = Rc<dyn Fn(&mut EventContext)>;

/// Interaction state for a menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItemState {
    Idle,
    Hovered,
    Pressed,
    Disabled,
}

/// Default delay before a submenu opens on hover (400 ms — IntelliJ's value).
/// This delay also provides diagonal movement tolerance: when the pointer
/// crosses other menu items while moving toward a submenu, those items
/// don't open their submenus because the delay hasn't elapsed yet. 400 ms
/// is long enough that a casual sweep past a submenu trigger doesn't
/// accidentally open it, but short enough that a deliberate hover feels
/// responsive.
const DEFAULT_SUBMENU_OPEN_DELAY: Duration = Duration::from_millis(400);
const DEFAULT_SUBMENU_CLOSE_DELAY: Duration = Duration::from_millis(150);

/// A single menu item: icon + label + shortcut label + optional submenu chevron.
pub struct MenuItem {
    label: bastyde_i18n::LocalizedString,
    icon: Option<IconWidget>,
    shortcut_label: Option<String>,
    /// Optional shortcut id. When set and `shortcut_label` is not,
    /// the rendered trailing label is pulled from the tree's
    /// [`ShortcutRegistry`](bastyde_core::shortcut::ShortcutRegistry) and
    /// tracks user rebindings automatically (the build registers the
    /// registry's version signal as a Relayout binding on self).
    shortcut_id: Option<&'static str>,
    tooltip_text: Option<bastyde_i18n::LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,
    action: Option<CommandFactory>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    submenu_factory: Option<Box<dyn Fn() -> Box<dyn Widget>>>,
    submenu_open_delay: Duration,
    // Build state
    interaction: Signal<MenuItemState>,
    /// Whether this item's submenu overlay is currently visible.
    /// Flipped to `true` by every open path (tap, hover, Enter,
    /// ArrowRight) and flipped back to `false` by the overlay
    /// manager's `on_dismiss` callback — regardless of dismiss
    /// path. `accessibility()` reads this for `set_expanded`.
    /// Only meaningful when `submenu_factory.is_some()`.
    submenu_open: Signal<bool>,
    /// Shortcut text after resolving `shortcut_label` / `shortcut_id`.
    /// Captured during `build()` and read by `accessibility()` for
    /// `set_keyboard_shortcut`, so screen readers announce the chord
    /// that the visual trailing label shows.
    resolved_shortcut: Option<String>,
    /// Per-call style override. When `None`, falls back to the
    /// theme-wide slot (`theme.style_slots.menu_item`) and finally to
    /// the IntUI default `RecipeMenuItemStyle`.
    style_override: Option<SharedMenuItemStyle>,
    root_child_id: Option<WidgetId>,
    submenu_content_id: Option<WidgetId>,
}

impl MenuItem {
    pub fn new(label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        Self {
            label: ls,
            icon: None,
            shortcut_label: None,
            shortcut_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            action: None,
            initial_enabled: true,
            submenu_factory: None,
            submenu_open_delay: DEFAULT_SUBMENU_OPEN_DELAY,
            interaction: Signal::new(MenuItemState::Idle),
            submenu_open: Signal::new(false),
            resolved_shortcut: None,
            style_override: None,
            root_child_id: None,
            submenu_content_id: None,
        }
    }

    /// Closure invoked on activation.
    /// See architecture Section 9.2.6.
    /// Note: shortcut label auto-lookup is not available with this variant
    /// since there is no typed command to look up.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Rc::new(f));
        self
    }

    /// Read the item's display label. Exposed so SplitButton (and any other
    /// compound widget that embeds a MenuItem) can mirror the label in its
    /// own chrome.
    pub fn label(&self) -> String {
        self.label.resolve_now()
    }

    /// Clone out a shared handle to the activation closure. Returns `None`
    /// when this MenuItem has no action (e.g. it's a submenu trigger). The
    /// returned `Rc` aliases MenuItem's own internal handle — invoking it
    /// has the same effect as the user clicking this menu item (minus the
    /// overlay dismissal that the tap handler also performs).
    pub fn action(&self) -> Option<Rc<dyn Fn(&mut EventContext)>> {
        self.action.clone()
    }

    /// Set a leading icon.
    pub fn icon(mut self, icon: IconWidget) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set a trailing shortcut label (e.g., "Ctrl+X"). Shortcut labels are
    /// typically not translated (they're the key combination literal), so
    /// this accepts a plain string.
    pub fn shortcut_label(mut self, label: impl Into<String>) -> Self {
        self.shortcut_label = Some(label.into());
        self
    }

    /// Bind the trailing shortcut label to a registered
    /// [`Shortcut`](bastyde_core::shortcut::Shortcut) by its stable id.
    /// At build time the effective primary keystroke is rendered;
    /// rebinds performed through
    /// [`ShortcutRegistry`](bastyde_core::shortcut::ShortcutRegistry)
    /// rebuild this item automatically via the registry's version
    /// signal.
    ///
    /// A manual [`shortcut_label`](Self::shortcut_label) takes
    /// precedence when both are set.
    pub fn for_shortcut(mut self, id: &'static str) -> Self {
        self.shortcut_id = Some(id);
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. For reactive enable/disable, call
    /// `ctx.enabled_when(menu_item_id, signal)` on the composing widget.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `MenuItemStyle` for just this MenuItem instance.
    pub fn style(mut self, style: impl bastyde_core::styles::MenuItemStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Attach a tooltip that appears after a hover delay, same mechanism
    /// as [`Button::tooltip`](crate::button::Button::tooltip).
    pub fn tooltip(mut self, text: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip
    /// registry. Body text supports inline markup
    /// (`[label](url)`, `*italic*`, `**bold**`); the entry's shortcut
    /// and long-form "more" fields are rendered automatically.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree. See [`Button::composite_tooltip`](crate::button::Button::composite_tooltip).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Create a submenu trigger item. The factory is invoked during `build()` to
    /// pre-create the submenu content (typically a `MenuList`), which is kept
    /// dormant until the hover delay elapses.
    pub fn submenu(
        label: impl Into<bastyde_i18n::LocalizedString>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        Self {
            label: ls,
            icon: None,
            shortcut_label: None,
            shortcut_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            action: None,
            initial_enabled: true,
            submenu_factory: Some(Box::new(factory)),
            submenu_open_delay: DEFAULT_SUBMENU_OPEN_DELAY,
            interaction: Signal::new(MenuItemState::Idle),
            submenu_open: Signal::new(false),
            resolved_shortcut: None,
            style_override: None,
            root_child_id: None,
            submenu_content_id: None,
        }
    }

    /// Set a custom submenu open delay (default: 200ms).
    pub fn submenu_delay(mut self, delay: Duration) -> Self {
        self.submenu_open_delay = delay;
        self
    }

    /// Whether this is a submenu trigger.
    pub fn is_submenu(&self) -> bool {
        self.submenu_factory.is_some()
    }
}

impl std::fmt::Debug for MenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuItem")
            .field("label", &self.label)
            .field("initial_enabled", &self.initial_enabled)
            .field("is_submenu", &self.submenu_factory.is_some())
            .finish()
    }
}

fn resolve_text_role(state: MenuItemState) -> TextRole {
    match state {
        MenuItemState::Disabled => TextRole::Disabled,
        _ => TextRole::Primary,
    }
}

fn resolve_shortcut_role(state: MenuItemState) -> TextRole {
    match state {
        MenuItemState::Disabled => TextRole::Disabled,
        _ => TextRole::TooltipShortcut,
    }
}

impl Widget for MenuItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::styles::recipe_menu_item_style as menu;
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Interaction seeds to Idle; the framework's effective_enabled
        // drives the Disabled visual via the recipe and through the
        // leaves' role substitution.
        let interaction = ctx.signal(MenuItemState::Idle);
        self.interaction = interaction.clone();

        // Combine interaction + effective_enabled so `text_role`
        // resolves to Disabled when disabled. Keeps the icon and label
        // muted on hover-while-disabled too (defense in depth — the
        // leaves' `ColorProp::resolve(theme, ctx.effective_enabled)`
        // would substitute Disabled anyway).
        let text_role = interaction.zip(&effective_enabled).map(|(s, on)| {
            if !*on {
                TextRole::Disabled
            } else {
                resolve_text_role(*s)
            }
        });

        // Build the three slots fed to the active `MenuItemStyle`.
        // The style decides the row layout (and chrome); the widget
        // owns the slot contents.
        //
        // Leading: icon column — always reserved at `icon_column_width`,
        // even when the item has no icon, so labels line up vertically
        // between icon'd and icon-less items.
        let leading = {
            let icon_child_id = if let Some(icon) = self.icon.take() {
                ctx.add(icon.bind_color(text_role.clone()))
            } else {
                ctx.add(Spacer::new())
            };
            ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(menu::MENU_ICON_COLUMN_WIDTH)
                    .bind_height(menu::MENU_ICON_COLUMN_WIDTH)
                    .child_id(icon_child_id),
            )
        };

        // Label.
        let label = ctx.add(
            TextWidget::new(self.label.clone())
                .style(TextStyleRole::Body)
                .bind_color(text_role.clone())
                .single_line()
                .a11y_hidden(),
        );

        // Resolve the trailing shortcut text — manual label wins;
        // otherwise pull from the registry by id. Bind the registry's
        // version signal so a rebind triggers a Rebuild on this widget.
        let resolved_shortcut = self.shortcut_label.clone().or_else(|| {
            self.shortcut_id.and_then(|id| {
                ctx.effective_shortcut(id)
                    .and_then(|eff| eff.primary.map(format_keystroke))
            })
        });
        self.resolved_shortcut = resolved_shortcut.clone();
        if self.shortcut_id.is_some() {
            ctx.shortcut_version().bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::Rebuild,
            );
        }

        // Pre-create submenu content if this is a submenu trigger. Kept
        // dormant until hover opens the overlay.
        let submenu_content_id = if let Some(factory) = self.submenu_factory.take() {
            let submenu_widget = factory();
            let id = ctx.add_boxed(submenu_widget);
            ctx.set_dormant(id);
            self.submenu_content_id = Some(id);
            Some(id)
        } else {
            None
        };

        // Trailing slot — combines (optional shortcut + fixed gap +
        // optional chevron column). The chevron column is always
        // reserved at `item_padding_horizontal` so submenu and
        // regular items share the same trailing edge.
        let trailing = {
            let mut trailing_row = HStack::new().spacing(0.0);
            if let Some(ref shortcut_text) = resolved_shortcut {
                let shortcut_role = interaction.map(|s| resolve_shortcut_role(*s));
                let shortcut = TextWidget::new(lit!(shortcut_text))
                    .style(TextStyleRole::Body)
                    .bind_color(shortcut_role)
                    .single_line()
                    .a11y_hidden();
                trailing_row = trailing_row.child(shortcut);
            }
            // Chevron column. Always reserved (Spacer when no submenu)
            // so the row's right edge sits at exactly the same X
            // regardless of submenu-ness.
            let chevron_child_id = if submenu_content_id.is_some() {
                ctx.add(IconWidget::chevron_right(12.0).bind_color(text_role.clone()))
            } else {
                ctx.add(Spacer::new())
            };
            let chevron_column = ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(menu::MENU_ITEM_PADDING_HORIZONTAL)
                    .bind_height(menu::MENU_ICON_COLUMN_WIDTH)
                    .child_id(chevron_child_id),
            );
            trailing_row = trailing_row.add_child(chevron_column);
            ctx.add(trailing_row)
        };

        // Derive the four boolean signals the trait wants.
        let is_hovered = interaction.map(|s| matches!(s, MenuItemState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, MenuItemState::Pressed));
        let is_disabled = interaction.map(|s| matches!(s, MenuItemState::Disabled));

        // MenuItem doesn't track focus/highlight separately today —
        // hovered already covers the keyboard-arrow case in the
        // existing dispatcher. Wire is_focused to a constant false
        // signal; is_highlighted reads the same as is_hovered for
        // the IntUI default (the recipe `or`s them anyway).
        let is_focused = ctx.signal(false);
        let is_highlighted = is_hovered.clone();

        let style: SharedMenuItemStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.menu_item.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeMenuItemStyle));
        let cfg = MenuItemStyleConfig {
            label,
            leading: Some(leading),
            trailing: Some(trailing),
            is_hovered,
            is_pressed,
            is_focused,
            is_disabled,
            is_highlighted,
        };
        let root_id = style.make_body(&cfg, ctx);

        self.root_child_id = Some(root_id);

        // Attach tooltip if configured. The three setters
        // (`tooltip`, `rich_tooltip*`, `composite_tooltip`) are
        // mutually exclusive — setters clear the other two so at most
        // one branch runs.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(tooltip_text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        // --- Handlers ---
        let action = self.action.take();
        let action_rc: std::rc::Rc<Option<CommandFactory>> = std::rc::Rc::new(action);
        let action_for_key = action_rc.clone();

        let int_hover = interaction.clone();
        let self_id = ctx.self_id();
        let is_submenu = submenu_content_id.is_some();

        // Shared dismiss callback for the submenu overlay. Flipped
        // to `false` by the overlay manager when the submenu is
        // dismissed by any path (pointer leave, cascade, Escape,
        // click outside) so `accessibility()` can report accurate
        // `set_expanded` without needing to track the overlay state
        // from inside the MenuItem's own handlers.
        let submenu_open_signal = self.submenu_open.clone();
        let submenu_dismiss_callback: bastyde_core::overlay::OverlayDismissCallback = {
            let open = submenu_open_signal.clone();
            std::rc::Rc::new(move || {
                open.set(false);
            })
        };

        let mut handler_set = HandlerSet::new();

        if is_submenu {
            // --- Submenu trigger: timer-based delayed open ---
            // On hover enter: request a delayed overlay via the widget tree's
            // timer system (like tooltips). On hover leave: cancel the pending
            // request. The widget tree checks pending overlays during layout()
            // and opens them once the delay elapses.
            let sub_id = submenu_content_id.expect("is_submenu implies submenu_content_id is Some");
            let open_delay = self.submenu_open_delay;

            let open_for_tap = submenu_open_signal.clone();
            let dismiss_for_tap = submenu_dismiss_callback.clone();
            let open_for_hover = submenu_open_signal.clone();
            let dismiss_for_hover = submenu_dismiss_callback.clone();
            // Framework gates events on `arena.is_enabled(self_id)`.
            handler_set = handler_set
                .on_tap({
                    move |_pos, ctx: &mut EventContext| {
                        // Click on submenu trigger opens it immediately
                        ctx.dismiss_child_overlays_except(sub_id);
                        ctx.activate(sub_id);
                        open_for_tap.set(true);
                        ctx.show_overlay(OverlayRequest {
                            content_id: sub_id,
                            anchor: self_id,
                            placement: OverlayPlacement::TrailingEdge,
                            dismiss: DismissBehavior::PointerLeave {
                                delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                            },
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                            on_dismiss: Some(dismiss_for_tap.clone()),
                            fade_duration: None,
                        });
                        ctx.request_focus(sub_id);
                    }
                })
                .on_hover({
                    let int_hover = int_hover.clone();
                    move |entered: bool, ctx: &mut EventContext| {
                        if entered {
                            int_hover.set(MenuItemState::Hovered);
                            ctx.dismiss_child_overlays_except(sub_id);
                            open_for_hover.set(true);
                            ctx.show_overlay_after_with_focus(
                                OverlayRequest {
                                    content_id: sub_id,
                                    anchor: self_id,
                                    placement: OverlayPlacement::TrailingEdge,
                                    dismiss: DismissBehavior::PointerLeave {
                                        delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                    },
                                    layer: OverlayLayer::InTree,
                                    parent_overlay: None,
                                    on_dismiss: Some(dismiss_for_hover.clone()),
                                    fade_duration: None,
                                },
                                open_delay,
                                sub_id,
                            );
                        } else {
                            int_hover.set(MenuItemState::Idle);
                            ctx.cancel_delayed_overlay(sub_id);
                            // If the overlay was still pending (delay
                            // not yet elapsed), its dismiss callback
                            // will never fire — we must reset the
                            // open flag ourselves. Idempotent if the
                            // overlay already showed: the framework
                            // dismiss callback will also set it false
                            // when the PointerLeave behavior tears
                            // the overlay down shortly afterward.
                            open_for_hover.set(false);
                        }
                    }
                });
        } else {
            // --- Regular menu item: tap to activate ---
            let action_for_tap = action_rc.clone();
            let int_tap = interaction.clone();

            handler_set = handler_set
                .on_tap({
                    move |_pos, ctx: &mut EventContext| {
                        int_tap.set(MenuItemState::Pressed);
                        if let Some(ref action) = *action_for_tap {
                            action(ctx);
                            ctx.dismiss_self_overlay_chain();
                        }
                        // Reset to Idle after dispatching — the
                        // overlay dismissal swallows the trailing
                        // PointerUp that would normally clear Pressed,
                        // and the dormant content widgets keep their
                        // last-painted state. Without this the
                        // previously-clicked item reads as Pressed
                        // (highlighted) the next time the menu opens,
                        // until a hover transition overwrites it.
                        int_tap.set(MenuItemState::Idle);
                    }
                })
                .on_hover({
                    move |entered: bool, ctx: &mut EventContext| {
                        if entered {
                            ctx.dismiss_child_overlays();
                            int_hover.set(MenuItemState::Hovered);
                        } else {
                            int_hover.set(MenuItemState::Idle);
                        }
                    }
                });
        }

        // Keyboard handler shared by both submenu and regular items
        handler_set = handler_set.on_key({
            let interaction = interaction.clone();
            let sub_id = submenu_content_id;
            let open_for_key = submenu_open_signal.clone();
            let dismiss_for_key = submenu_dismiss_callback.clone();
            move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::Enter | Key::Space,
                        ..
                    } => {
                        if let Some(ref action) = *action_for_key {
                            action(ctx);
                            ctx.dismiss_self_overlay_chain();
                        } else if let Some(sub_id) = sub_id {
                            ctx.dismiss_child_overlays_except(sub_id);
                            ctx.activate(sub_id);
                            open_for_key.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: DismissBehavior::PointerLeave {
                                    delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                },
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: Some(dismiss_for_key.clone()),
                                fade_duration: None,
                            });
                            ctx.request_focus(sub_id);
                        }
                        interaction.set(MenuItemState::Pressed);
                        EventResponse::Handled
                    }
                    // ArrowRight opens submenu (ignored on regular items)
                    WidgetEvent::KeyDown {
                        key: Key::ArrowRight,
                        ..
                    } => {
                        if let Some(sub_id) = sub_id {
                            ctx.dismiss_child_overlays_except(sub_id);
                            ctx.activate(sub_id);
                            open_for_key.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: DismissBehavior::PointerLeave {
                                    delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                },
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: Some(dismiss_for_key.clone()),
                                fade_duration: None,
                            });
                            ctx.request_focus(sub_id);
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                    _ => EventResponse::Ignored,
                }
            }
        });

        // Cursor: NotAllowed when effectively disabled (the original
        // intent), Pointer otherwise. Sourced from the arena via the
        // reactive effective_enabled signal so it reacts to
        // `enabled_when` flips.
        handler_set = handler_set.cursor(if effective_enabled.get() {
            CursorIcon::Pointer
        } else {
            CursorIcon::NotAllowed
        });

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => {
                let size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                // Claim the full proposed width when the parent offers one.
                // This is what makes menu items stretch to the popup width:
                // MenuList sizes its VStack to the widest item, then the
                // VStack proposes that width to each child. Without this
                // line, each MenuItem would report only its own content
                // width and the row's internal Spacer would have no room
                // to stretch — so the shortcut would sit flush against
                // the label instead of pushing to the trailing edge.
                let width = proposal.width.unwrap_or(size.width);
                Size::new(width, size.height)
            }
            None => proposal.resolve(120.0, 24.0),
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
        builder.set_role(bastyde_core::accesskit::Role::MenuItem);
        builder.set_name(self.label.resolve_now());
        // A submenu trigger exposes `has_popup(Menu)` so screen
        // readers announce the item as leading into a nested menu,
        // and `set_expanded` reflects whether the submenu is
        // currently visible. We check `submenu_content_id` rather
        // than `submenu_factory`: the factory is moved out during
        // `build()` via `take()`, so by the time the framework
        // queries accessibility the factory is always `None`,
        // but the content id survives.
        if self.submenu_content_id.is_some() {
            builder.set_has_popup(bastyde_core::accesskit::HasPopup::Menu);
            builder.set_expanded(self.submenu_open.get());
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(bastyde_core::accesskit::Action::Click);
        if let Some(ref shortcut) = self.resolved_shortcut {
            builder.set_keyboard_shortcut(shortcut.clone());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}
