//! MenuItem widget — a single item in a menu or context menu.
//!
//! Non-generic, closure-based command erasure (same pattern as Button).
//! Supports icons, shortcut labels, disabled state, and submenu triggers.

use std::rc::Rc;
use std::time::Duration;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, SurfaceRole, TextRole, TextStyleRole};

use crate::keystroke_format::format_keystroke;
use crate::primitives::{HStack, IconWidget, Padding, RectWidget, Spacer, TextWidget, ZStack};

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
    label: String,
    icon: Option<IconWidget>,
    shortcut_label: Option<String>,
    /// Optional shortcut id. When set and [`shortcut_label`] is not,
    /// the rendered trailing label is pulled from the tree's
    /// [`ShortcutRegistry`](fern_core::shortcut::ShortcutRegistry) and
    /// tracks user rebindings automatically (the build registers the
    /// registry's version signal as a Relayout binding on self).
    shortcut_id: Option<&'static str>,
    tooltip_text: Option<String>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    action: Option<CommandFactory>,
    enabled: bool,
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
    root_child_id: Option<WidgetId>,
    submenu_content_id: Option<WidgetId>,
}

impl MenuItem {
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            icon: None,
            shortcut_label: None,
            shortcut_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            action: None,
            enabled: true,
            submenu_factory: None,
            submenu_open_delay: DEFAULT_SUBMENU_OPEN_DELAY,
            interaction: Signal::new(MenuItemState::Idle),
            submenu_open: Signal::new(false),
            resolved_shortcut: None,
            root_child_id: None,
            submenu_content_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
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
    pub fn label(&self) -> &str {
        &self.label
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
    /// [`Shortcut`](fern_core::shortcut::Shortcut) by its stable id.
    /// At build time the effective primary keystroke is rendered;
    /// rebinds performed through
    /// [`ShortcutRegistry`](fern_core::shortcut::ShortcutRegistry)
    /// rebuild this item automatically via the registry's version
    /// signal.
    ///
    /// A manual [`shortcut_label`](Self::shortcut_label) takes
    /// precedence when both are set.
    pub fn for_shortcut(mut self, id: &'static str) -> Self {
        self.shortcut_id = Some(id);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Attach a tooltip that appears after a hover delay, same mechanism
    /// as [`Button::tooltip`](crate::button::Button::tooltip).
    pub fn tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self.rich_tooltip_source = None;
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip
    /// registry. Body text supports inline markup
    /// (`[label](url)`, `*italic*`, `**bold**`); the entry's shortcut
    /// and long-form "more" fields are rendered automatically.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self
    }

    /// Attach a rich tooltip driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self
    }

    /// Create a submenu trigger item. The factory is invoked during `build()` to
    /// pre-create the submenu content (typically a `MenuList`), which is kept
    /// dormant until the hover delay elapses.
    pub fn submenu(
        label: impl Into<fern_i18n::LocalizedString>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            icon: None,
            shortcut_label: None,
            shortcut_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            action: None,
            enabled: true,
            submenu_factory: Some(Box::new(factory)),
            submenu_open_delay: DEFAULT_SUBMENU_OPEN_DELAY,
            interaction: Signal::new(MenuItemState::Idle),
            submenu_open: Signal::new(false),
            resolved_shortcut: None,
            root_child_id: None,
            submenu_content_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `submenu(...)` accepting a raw label.
    #[doc(hidden)]
    pub fn submenu_literal(
        label: impl Into<String>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::submenu(fern_i18n::LocalizedString::literal(label), factory)
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
            .field("enabled", &self.enabled)
            .field("is_submenu", &self.submenu_factory.is_some())
            .finish()
    }
}

fn resolve_bg_role(state: MenuItemState) -> SurfaceRole {
    match state {
        MenuItemState::Idle | MenuItemState::Disabled => SurfaceRole::Transparent,
        MenuItemState::Hovered => SurfaceRole::AccentSubtle,
        MenuItemState::Pressed => SurfaceRole::Pressed,
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
        let menu_style = ctx.theme().components.menu;
        let enabled = self.enabled;

        let interaction = ctx.signal(if enabled {
            MenuItemState::Idle
        } else {
            MenuItemState::Disabled
        });
        self.interaction = interaction.clone();

        let bg_role = interaction.map(|s| resolve_bg_role(*s));
        let text_role = interaction.map(|s| resolve_text_role(*s));

        // Row layout:
        //   [icon column][gap][label][Spacer][shortcut?][chevron column]
        //
        // HStack spacing is 0 — we insert an explicit `icon_label_gap`
        // only between the icon column and the label. Nothing else in the
        // row should have inter-child gaps: the Spacer handles stretch,
        // the chevron column handles the trailing padding, and the
        // shortcut (when present) sits directly adjacent to the chevron
        // column. Using HStack::spacing here would inject extra gaps
        // around the Spacer and shortcut, pushing the shortcut visibly
        // away from the trailing edge — which is why "Ctrl+X" used to
        // land short of where regular items had their right padding.
        //
        // * `icon column` is always reserved at `icon_column_width`, even
        //   when the item has no icon, so labels line up vertically
        //   between icon'd and icon-less items.
        //
        // * `chevron column` is always reserved at `item_padding_horizontal`
        //   width. For submenu items it contains the chevron; for regular
        //   items it's empty. Because the outer wrapper sets right
        //   padding = 0, the chevron column visually IS the right
        //   padding — regular items and submenu items share the same
        //   trailing edge.
        let mut row = HStack::new().spacing(0.0);

        // Icon column — fixed width, optional IconWidget inside.
        let icon_child_id = if let Some(icon) = self.icon.take() {
            ctx.add(icon.bind_color(text_role.clone()))
        } else {
            ctx.add(Spacer::new())
        };
        let icon_column = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_width(menu_style.icon_column_width)
                .bind_height(menu_style.icon_column_width)
                .child_id(icon_child_id),
        );
        row = row.add_child(icon_column);

        // Explicit icon-to-label gap (rendered as a fixed-width Spacer
        // rather than HStack::spacing to avoid injecting gaps around the
        // other children).
        let icon_label_spacer = ctx.add(Spacer::new());
        let icon_label_gap = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_width(menu_style.icon_label_gap)
                .bind_height(1.0_f32)
                .child_id(icon_label_spacer),
        );
        row = row.add_child(icon_label_gap);

        // Label
        let label = TextWidget::new_literal(&self.label)
            .style(TextStyleRole::Body)
            .bind_color(text_role.clone())
            .single_line()
            .a11y_hidden();
        row = row.child(label);

        // Stretch spacer — pushes trailing content to the right edge.
        row = row.child(Spacer::new());

        // Shortcut label — the manual label wins; otherwise, if the
        // item was bound to a shortcut id via `.for_shortcut(id)`, the
        // effective primary keystroke is pulled from the tree's
        // `ShortcutRegistry`. The registry's `version` signal is
        // bound to this widget at the `Relayout` level so user
        // rebindings (or late registrations) refresh the label on
        // the next pass.
        //
        // A fixed-width gap (`shortcut_left_gap`, 24 dp) is inserted
        // between the stretch Spacer and the shortcut label so that even
        // when the row is packed tight (Spacer stretch = 0), there is
        // always a visible gap between label and shortcut.
        let resolved_shortcut = self.shortcut_label.clone().or_else(|| {
            self.shortcut_id.and_then(|id| {
                ctx.effective_shortcut(id)
                    .and_then(|eff| eff.primary.map(format_keystroke))
            })
        });
        self.resolved_shortcut = resolved_shortcut.clone();
        if self.shortcut_id.is_some() {
            // Rebuild (not Relayout) because the shortcut label is
            // read from the registry by value during build() — a
            // rebind must re-enter build() to pick up the new chord.
            ctx.shortcut_version().bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                fern_core::binding::BindingLevel::Rebuild,
            );
        }
        if let Some(ref shortcut_text) = resolved_shortcut {
            // Fixed minimum gap, always present.
            let shortcut_gap_spacer = ctx.add(Spacer::new());
            let shortcut_gap = ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(menu_style.shortcut_left_gap)
                    .bind_height(1.0_f32)
                    .child_id(shortcut_gap_spacer),
            );
            row = row.add_child(shortcut_gap);

            let shortcut_role = interaction.map(|s| resolve_shortcut_role(*s));
            let shortcut = TextWidget::new_literal(shortcut_text)
                .style(TextStyleRole::Body)
                .bind_color(shortcut_role)
                .single_line()
                .a11y_hidden();
            row = row.child(shortcut);
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

        // Chevron column — always reserved at `item_padding_horizontal`
        // width so submenu and regular items share the same trailing edge.
        let chevron_child_id = if submenu_content_id.is_some() {
            ctx.add(IconWidget::chevron_right(12.0).bind_color(text_role.clone()))
        } else {
            ctx.add(Spacer::new())
        };
        let chevron_column = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_width(menu_style.item_padding_horizontal)
                .bind_height(menu_style.icon_column_width)
                .child_id(chevron_child_id),
        );
        row = row.add_child(chevron_column);

        let row_id = ctx.add(row);

        // Padding: vertical derived so the row has the full `item_height`.
        // Compare against the rendered text line (`size * line_height`),
        // not the bare font size — TextWidget lays out at the line height,
        // so using `size` alone over-pads by ~`size * (line_height - 1)`
        // (e.g. 13 × 0.4 = 5.2 dp, pushing a nominal 24 dp row to ~29 dp).
        let body = &ctx.theme().typography.body;
        let body_line = body.size * body.line_height;
        let pad_v = ((menu_style.item_height - body_line) * 0.5).max(0.0);
        let padding = Padding::new(
            pad_v,                              // top
            0.0,                                // right — chevron column fills this
            pad_v,                              // bottom
            menu_style.item_padding_horizontal, // left
        )
        .child_id(row_id);
        let padding_id = ctx.add(padding);

        // Background rect
        let rect = RectWidget::new()
            .bind_background(bg_role)
            .corner_radius(CornerRadius::uniform(menu_style.item_corner_radius));
        let rect_id = ctx.add(rect);

        let zstack = ZStack::new().add_child(rect_id).add_child(padding_id);
        let root_id = ctx.add(zstack);

        self.root_child_id = Some(root_id);

        // Attach tooltip if configured. Same 500ms delay as Button.
        // Rich-tooltip source takes precedence — setters clear the
        // other field, so at most one branch runs.
        if let Some(source) = self.rich_tooltip_source.take() {
            crate::tooltip::attach_rich_tooltip_source(
                ctx,
                root_id,
                source,
                crate::tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
            );
        } else if let Some(ref tooltip_text) = self.tooltip_text {
            let tooltip_widget = crate::tooltip::TooltipWidget::new_literal(tooltip_text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = std::time::Duration::from_millis(500);
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
        let submenu_dismiss_callback: fern_core::overlay::OverlayDismissCallback = {
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
            let sub_id = submenu_content_id.unwrap();
            let open_delay = self.submenu_open_delay;

            let open_for_tap = submenu_open_signal.clone();
            let dismiss_for_tap = submenu_dismiss_callback.clone();
            let open_for_hover = submenu_open_signal.clone();
            let dismiss_for_hover = submenu_dismiss_callback.clone();
            handler_set = handler_set
                .on_tap({
                    move |_pos, ctx: &mut EventContext| {
                        if !enabled {
                            return;
                        }
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
                        if !enabled {
                            return;
                        }
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
                        if !enabled {
                            return;
                        }
                        int_tap.set(MenuItemState::Pressed);
                        if let Some(ref action) = *action_for_tap {
                            action(ctx);
                            ctx.dismiss_all_overlays();
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
                        if !enabled {
                            return;
                        }
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
                if !enabled {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::Enter | Key::Space,
                        ..
                    } => {
                        if let Some(ref action) = *action_for_key {
                            action(ctx);
                            ctx.dismiss_all_overlays();
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

        handler_set = handler_set.cursor(if enabled {
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
    ) -> fern_core::widget::LayoutResponse {
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
        builder.set_role(fern_core::accesskit::Role::MenuItem);
        builder.set_name(&self.label);
        // A submenu trigger exposes `has_popup(Menu)` so screen
        // readers announce the item as leading into a nested menu,
        // and `set_expanded` reflects whether the submenu is
        // currently visible. We check `submenu_content_id` rather
        // than `submenu_factory`: the factory is moved out during
        // `build()` via `take()`, so by the time the framework
        // queries accessibility the factory is always `None`,
        // but the content id survives.
        if self.submenu_content_id.is_some() {
            builder.set_has_popup(fern_core::accesskit::HasPopup::Menu);
            builder.set_expanded(self.submenu_open.get());
        }
        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(fern_core::accesskit::Action::Click);
        }
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
