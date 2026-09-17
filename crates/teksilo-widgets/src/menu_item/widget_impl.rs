// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for [`MenuItem`]: build (row
//! assembly, hover / press / submenu pointer wiring, the safe-triangle gate),
//! layout, placement and accessibility — plus the small state-to-role helpers
//! only that build uses.

use super::*;
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

/// Whether a state is the row's *highlighted* one — the state a
/// [`MenuItemStyle::highlighted_label_role`](teksilo_core::styles::MenuItemStyle::highlighted_label_role)
/// applies to. Hover and the
/// keyboard-arrow highlight share `Hovered`; a pressed row is still
/// highlighted underneath the press.
fn is_highlight(state: MenuItemState) -> bool {
    matches!(state, MenuItemState::Hovered | MenuItemState::Pressed)
}

impl Widget for MenuItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward enabled (static or signal-bound) into the arena. A bound
        // signal makes enable/disable reactive — the framework's
        // effective_enabled drives paint / AT (and event gating).
        ctx.enabled_when(self_id, self.enabled.clone());
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Interaction seeds to Idle; the framework's effective_enabled
        // drives the Disabled visual via the recipe and through the
        // leaves' role substitution.
        let interaction = ctx.signal(MenuItemState::Idle);
        self.interaction = interaction.clone();

        // Resolved here rather than at `make_body` because the label is
        // built long before the chrome, and a style whose highlight is a
        // *solid* fill (macOS's accent row) has to say so in time to
        // recolour it. Per-call override > theme slot > shipped recipe.
        let style: SharedMenuItemStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.menu_item.clone())
            .unwrap_or_else(|| {
                Rc::new(crate::styles::RecipeMenuItemStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });
        let highlighted_role = style.highlighted_label_role();
        // The slot sizes the active style owns. `MenuItem` builds the leading
        // icon/check column and the trailing chevron column itself — they are
        // slot *contents*, handed to `make_body` already sized — so they have
        // to be read here rather than inside the style. Hardcoding the IntUI
        // constants instead is what made `MenuItemRecipe::icon_column_width` a
        // no-op: macOS declared a 14 dp check column and got 16.
        let metrics = style.metrics();

        // Combine interaction + effective_enabled so `text_role`
        // resolves to Disabled when disabled. Keeps the icon and label
        // muted on hover-while-disabled too (defense in depth — the
        // leaves' `ColorProp::resolve(theme, ctx.effective_enabled)`
        // would substitute Disabled anyway).
        let text_role = interaction.zip(&effective_enabled).map(move |(s, on)| {
            if !*on {
                TextRole::Disabled
            } else {
                highlighted_role
                    .filter(|_| is_highlight(*s))
                    .unwrap_or_else(|| resolve_text_role(*s))
            }
        });

        // Build the three slots fed to the active `MenuItemStyle`.
        // The style decides the row layout (and chrome); the widget
        // owns the slot contents.
        //
        // Leading: icon column — always reserved at `icon_column_width`,
        // even when the item has no icon, so labels line up vertically
        // between icon'd and icon-less items.
        //
        // For Check / Radio modes the slot becomes a `Switcher`
        // driven by the bound state signal, swapping between the
        // glyph and a `Spacer`. The framework's binding system
        // re-paints the leaf when the signal flips — no rebuild.
        //
        // Icon + Check/Radio are mutually exclusive (Windows
        // convention). If both are set, `debug_assert!` fires and the
        // check/radio mode wins in release.
        let leading = {
            let icon_child_id = match &self.mode {
                MenuItemMode::Plain => match self.icon.take() {
                    // The caller's own colour stands — see `icon_keeps_color`.
                    Some(icon) if self.icon_keeps_color => ctx.add(icon),
                    Some(icon) => ctx.add(icon.color(text_role.clone())),
                    None => ctx.add(Spacer::new()),
                },
                MenuItemMode::Check(CheckKind::TwoState(s)) => {
                    debug_assert!(
                        self.icon.is_none(),
                        "MenuItem: .icon() is mutually exclusive with a checkmark (checked / reflect_checked)"
                    );
                    self.icon = None;
                    // 0 = checkmark, 1 = spacer.
                    let idx = s.map(|b| if *b { 0_usize } else { 1 });
                    ctx.add(
                        Switcher::new(idx)
                            .child(
                                IconWidget::checkmark(MENU_INDICATOR_GLYPH_SIZE)
                                    .color(text_role.clone()),
                            )
                            .child(Spacer::new()),
                    )
                }
                MenuItemMode::Check(CheckKind::Reflect(s)) => {
                    debug_assert!(
                        self.icon.is_none(),
                        "MenuItem: .icon() is mutually exclusive with a checkmark (checked / reflect_checked)"
                    );
                    self.icon = None;
                    // 0 = checkmark, 1 = spacer.
                    let idx = s.as_signal().map(|b| if *b { 0_usize } else { 1 });
                    ctx.add(
                        Switcher::new(idx)
                            .child(
                                IconWidget::checkmark(MENU_INDICATOR_GLYPH_SIZE)
                                    .color(text_role.clone()),
                            )
                            .child(Spacer::new()),
                    )
                }
                MenuItemMode::Check(CheckKind::TriState(s)) => {
                    debug_assert!(
                        self.icon.is_none(),
                        "MenuItem: .icon() is mutually exclusive with .check_state()"
                    );
                    self.icon = None;
                    // 0 = checkmark (Checked), 1 = dash (Indeterminate), 2 = spacer (Unchecked).
                    let idx = s.map(|cs| match cs {
                        CheckState::Checked => 0_usize,
                        CheckState::Indeterminate => 1,
                        CheckState::Unchecked => 2,
                    });
                    ctx.add(
                        Switcher::new(idx)
                            .child(
                                IconWidget::checkmark(MENU_INDICATOR_GLYPH_SIZE)
                                    .color(text_role.clone()),
                            )
                            .child(
                                IconWidget::dash(MENU_INDICATOR_GLYPH_SIZE)
                                    .color(text_role.clone()),
                            )
                            .child(Spacer::new()),
                    )
                }
                MenuItemMode::Radio { value, selected } => {
                    debug_assert!(
                        self.icon.is_none(),
                        "MenuItem: .icon() is mutually exclusive with .radio()"
                    );
                    self.icon = None;
                    let v = *value;
                    // 0 = filled dot (selected == value), 1 = spacer.
                    let idx = selected.map(move |sel| if *sel == v { 0_usize } else { 1 });
                    ctx.add(
                        Switcher::new(idx)
                            .child(
                                IconWidget::radio_dot(MENU_INDICATOR_GLYPH_SIZE)
                                    .color(text_role.clone()),
                            )
                            .child(Spacer::new()),
                    )
                }
            };
            ctx.add(
                crate::primitives::FixedSize::new()
                    .width(metrics.icon_column_width)
                    .height(metrics.icon_column_width)
                    .child(icon_child_id),
            )
        };

        // Label. Uses `MenuLabel` (not `TextWidget`) so a single `&`
        // in the label is parsed as a mnemonic marker — stripped from
        // the visible text and underlined when `alt_down` is held.
        // The parsed form is cached so the enclosing MenuList can
        // read it for type-ahead and in-menu mnemonic activation.
        let parsed = parse_mnemonic(&self.label.resolve_now());
        self.parsed_mnemonic = Some(parsed.clone());
        let alt_down = ctx
            .window()
            .map(|w| w.alt_down().clone())
            .unwrap_or_else(|| Signal::new(false));
        let label_source: teksilo_core::signal::Prop<String> = self.label.clone().into();
        let label_color: teksilo_core::color_prop::ColorProp = self
            .text_role_override
            .clone()
            .unwrap_or_else(|| text_role.clone().into());
        let label_style: teksilo_core::color_prop::TextStyleProp = self
            .label_style
            .clone()
            .unwrap_or_else(|| TextStyleRole::Body.into());
        let label = ctx.add(MenuLabel::new(
            label_source,
            alt_down,
            label_color,
            label_style,
        ));

        // Resolve the trailing accelerator *reactively*. A manual
        // `shortcut_label` is a static string; a `shortcut_id` binds a
        // per-id registry signal (built into the trailing slot below), so
        // a rebind of *that* id refreshes the chord in place. Crucially we
        // do NOT observe the coarse global `shortcut_version` at `Rebuild`
        // here — doing so tore the whole item (its gesture arena) down on
        // *any* shortcut-registry activity anywhere, dropping the click on
        // menu items that show a shortcut. The item is now never rebuilt
        // for shortcut changes; only its trailing label repaints.
        self.shortcut_signal = self.shortcut_id.map(|id| ctx.effective_shortcut_signal(id));

        // Pre-create submenu content if this is a submenu trigger. Kept
        // dormant until hover opens the overlay.
        let submenu_content_id = if let Some(factory) = self.submenu_factory.take() {
            let submenu_widget = factory();
            // Detached (a submenu opens in an overlay beside the item, never
            // inline) but owned, so it dies with the item instead of outliving
            // every menu the user ever opened.
            // Built the first time the submenu is actually wanted. A menu of
            // twenty items with submenus used to build all twenty submenus —
            // and their submenus — the moment the menu was mounted.
            let id = ctx.add_detached_deferred_boxed(self.submenu_needed.clone(), submenu_widget);
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
            // Trailing accelerator. Present whenever this item references a
            // shortcut (manual `shortcut_label`, or a `shortcut_id`). For an
            // id it binds the per-id signal reactively (empty ⇒ zero-width,
            // so a shortcut appearing/disappearing needs no rebuild); for a
            // manual label it's a static string.
            let shortcut: Option<TextWidget> = if let Some(label) = self.shortcut_label.clone() {
                Some(TextWidget::new(lit!(label)))
            } else {
                self.shortcut_signal.clone().map(|sig| {
                    TextWidget::new(lit!(""))
                        .text(sig.map(|ks| (*ks).map(format_keystroke).unwrap_or_default()))
                })
            };
            let has_shortcut = shortcut.is_some();
            if let Some(shortcut) = shortcut {
                let shortcut_role = interaction.map(move |s| {
                    highlighted_role
                        .filter(|_| is_highlight(*s))
                        .unwrap_or_else(|| resolve_shortcut_role(*s))
                });
                trailing_row = trailing_row.child(
                    shortcut
                        .style(TextStyleRole::Body)
                        .color(shortcut_role)
                        .single_line()
                        .a11y_hidden(),
                );
            }
            // Trailing descriptive hint. Unlike the accelerator above this is
            // built straight from the `LocalizedString`, so `TextWidget`'s own
            // `Prop<String>` conversion binds it to the locale signal and it
            // re-resolves in place on a language switch. It is `a11y_hidden`
            // because it is announced as the item's *description* instead (see
            // `accessibility`), never as a keyboard shortcut.
            if let Some(hint) = self.trailing_hint.clone() {
                if has_shortcut {
                    // Both set (rare) — keep the chord and the phrase apart.
                    trailing_row = trailing_row.child(
                        crate::primitives::FixedSize::new().width(metrics.trailing_column_width),
                    );
                }
                let hint_role = interaction.map(move |s| {
                    highlighted_role
                        .filter(|_| is_highlight(*s))
                        .unwrap_or_else(|| resolve_shortcut_role(*s))
                });
                trailing_row = trailing_row.child(
                    TextWidget::new(hint)
                        .style(TextStyleRole::Body)
                        .color(hint_role)
                        .single_line()
                        .a11y_hidden(),
                );
            }
            // Chevron column. Always reserved (Spacer when no submenu)
            // so the row's right edge sits at exactly the same X
            // regardless of submenu-ness.
            //
            // The submenu opens on the trailing edge
            // (`OverlayPlacement::TrailingEdge`) — right under LTR, left
            // under RTL — so the chevron must point the same way. Drive a
            // `Switcher` off the locale's direction signal so it flips
            // live on a locale change (0 = LTR → ▶, 1 = RTL → ◀). With no
            // i18n manager installed there's no RTL, so fall back to the
            // plain right-pointing chevron.
            let chevron_child_id = if submenu_content_id.is_some() {
                match teksilo_i18n::current_direction() {
                    Some(direction) => {
                        let idx = direction.map(|d| {
                            if *d == teksilo_core::environment::LayoutDirection::RightToLeft {
                                1_usize
                            } else {
                                0
                            }
                        });
                        ctx.add(
                            Switcher::new(idx)
                                .child(IconWidget::chevron_right(12.0).color(text_role.clone()))
                                .child(IconWidget::chevron_left(12.0).color(text_role.clone())),
                        )
                    }
                    None => ctx.add(IconWidget::chevron_right(12.0).color(text_role.clone())),
                }
            } else {
                ctx.add(Spacer::new())
            };
            let chevron_column = ctx.add(
                crate::primitives::FixedSize::new()
                    .width(metrics.trailing_column_width)
                    .height(metrics.icon_column_width)
                    .child(chevron_child_id),
            );
            trailing_row = trailing_row.child(chevron_column);
            ctx.add(trailing_row)
        };

        // Derive the four boolean signals the trait wants.
        let is_hovered = interaction.map(|s| matches!(s, MenuItemState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, MenuItemState::Pressed));
        let is_disabled = interaction.map(|s| matches!(s, MenuItemState::Disabled));

        // MenuItem doesn't track focus/highlight separately today —
        // hovered already covers the keyboard-arrow case in the
        // existing dispatcher. Wire is_focused to a constant false
        // signal.
        let is_focused = ctx.signal(false);
        // A submenu trigger stays highlighted for as long as its
        // submenu is on screen, not just while the pointer is on the
        // row — the pointer spends that whole time somewhere else (the
        // diagonal, then the submenu itself), and every desktop menu
        // keeps the parent row lit to show where the open panel came
        // from. Plain rows have no submenu, so this is `is_hovered`.
        let is_highlighted = if submenu_content_id.is_some() {
            is_hovered.or(&self.submenu_open)
        } else {
            is_hovered.clone()
        };

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
        // one branch runs. A `MenuItem` only ever lives in a vertical
        // `MenuList`, so the tooltip opens to the trailing `Side` — a
        // `Below` tooltip would cover the next item down.
        use crate::tooltip::TooltipPlacement;
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed_with_placement(
                ctx,
                root_id,
                content,
                delay,
                TooltipPlacement::Side,
            );
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            // Cloned, not taken: `build()` re-runs on every rebuild, and an item
            // that consumed its source attached a tooltip once and then silently
            // lost it — the surviving entry pointed at the previous build's body,
            // which the rebuild had just destroyed. (`composite_tooltip_content`
            // above is a `Box<dyn Widget>` with no way to clone, so it keeps the
            // take and its one-shot behaviour.)
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source_with_placement(
                ctx,
                root_id,
                source,
                delay,
                TooltipPlacement::Side,
            );
        } else if let Some(tooltip_text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip_with_placement(
                ctx,
                root_id,
                tooltip_text,
                delay,
                TooltipPlacement::Side,
            );
        }

        // --- Handlers ---
        let action = self.action.take();
        let action_rc: std::rc::Rc<Option<CommandFactory>> = std::rc::Rc::new(action);
        let action_for_key = action_rc.clone();

        // Shared closure that performs the bound-state mutation on
        // activation — flips the check signal, cycles the tristate
        // signal, or writes the radio value. Captured by both the
        // tap and key handlers so click and Enter/Space have
        // identical semantics. `None` for `Plain` and for submenu
        // triggers (which never carry a bound state).
        type ActivateFn = std::rc::Rc<dyn Fn()>;
        let mode_activate: Option<ActivateFn> = match &self.mode {
            MenuItemMode::Plain => None,
            MenuItemMode::Check(CheckKind::TwoState(s)) => {
                let s = s.clone();
                Some(std::rc::Rc::new(move || s.set(!s.get())))
            }
            // Reflect-only: no built-in write — the on_activate / intent owns
            // the state change; the checkmark follows `state` reactively.
            MenuItemMode::Check(CheckKind::Reflect(_)) => None,
            MenuItemMode::Check(CheckKind::TriState(s)) => {
                let s = s.clone();
                // Click toggles Unchecked <-> Checked. Indeterminate
                // (driven by external aggregation models) promotes
                // to Checked. Mirrors `Checkbox::toggle`.
                Some(std::rc::Rc::new(move || match s.get() {
                    CheckState::Unchecked => s.set(CheckState::Checked),
                    CheckState::Checked => s.set(CheckState::Unchecked),
                    CheckState::Indeterminate => s.set(CheckState::Checked),
                }))
            }
            MenuItemMode::Radio { value, selected } => {
                let v = *value;
                let selected = selected.clone();
                Some(std::rc::Rc::new(move || selected.set(v)))
            }
        };
        let mode_activate_for_tap = mode_activate.clone();
        let mode_activate_for_key = mode_activate.clone();

        let int_hover = interaction.clone();
        let self_id = ctx.self_id();
        let is_submenu = submenu_content_id.is_some();

        // Shared dismiss callback for the submenu overlay. Flipped
        // to `false` by the overlay manager when the submenu is
        // dismissed by any path (pointer leave, cascade, Escape,
        // click outside) so `accessibility()` can report accurate
        // `set_expanded` without needing to track the overlay state
        // from inside the MenuItem's own handlers.
        //
        // Also retracts the safe-triangle publication when the overlay
        // actually closes — it is deliberately kept alive across
        // hover-leave (that is when the diagonal starts), so it MUST
        // be cleared here once the submenu is finally gone.
        let submenu_open_signal = self.submenu_open.clone();
        let submenu_needed_signal = self.submenu_needed.clone();
        let submenu_content_id_for_dismiss = submenu_content_id;
        let safe_triangle_for_dismiss = self.safe_triangle.clone();
        let submenu_dismiss_callback: teksilo_core::overlay::OverlayDismissCallback = {
            let open = submenu_open_signal.clone();
            std::rc::Rc::new(move || {
                open.set(false);
                if let (Some(sub_id), Some(state_rc)) = (
                    submenu_content_id_for_dismiss,
                    safe_triangle_for_dismiss.as_ref(),
                ) {
                    let mut state = state_rc.borrow_mut();
                    if state.submenu_content_id == Some(sub_id) {
                        state.submenu_content_id = None;
                    }
                }
            })
        };

        // Shared activation for assistive-tech / automation (AccessKit `Click`).
        // Mirrors the Enter/Space `on_key` path exactly: a regular item flips its
        // bound mode, runs the user action, and dismisses the chain; a submenu
        // trigger opens its nested overlay. The item already advertises
        // `Action::Click` in `accessibility()`, but without a handler that
        // advertised action is inert — this makes it activatable.
        let activate_item: std::rc::Rc<dyn Fn(&mut EventContext)> = {
            let mode_activate = mode_activate.clone();
            let action = action_rc.clone();
            let sub_id = submenu_content_id;
            let open = submenu_open_signal.clone();
            let needed = submenu_needed_signal.clone();
            let dismiss = submenu_dismiss_callback.clone();
            std::rc::Rc::new(move |ctx: &mut EventContext| {
                if let Some(ref activate) = mode_activate {
                    activate();
                }
                if let Some(ref action) = *action {
                    action(ctx);
                    ctx.dismiss_self_overlay_chain();
                } else if mode_activate.is_some() {
                    ctx.dismiss_self_overlay_chain();
                } else if let Some(sub_id) = sub_id {
                    ctx.dismiss_child_overlays_except(sub_id);
                    // Build the submenu if this is the first time it is wanted, before
                    // the overlay below is measured against it.
                    needed.set(true);
                    ctx.materialize_now(sub_id);
                    ctx.activate(sub_id);
                    open.set(true);
                    ctx.show_overlay(OverlayRequest {
                        content_id: sub_id,
                        anchor: self_id,
                        placement: OverlayPlacement::TrailingEdge,
                        dismiss: SubmenuOpenRoute::KeyboardOrAt.dismiss(),
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                        on_dismiss: Some(dismiss.clone()),
                        fade_duration: None,
                    });
                    ctx.request_focus(sub_id);
                }
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
            let needed_for_tap = submenu_needed_signal.clone();
            let dismiss_for_tap = submenu_dismiss_callback.clone();
            let open_for_hover = submenu_open_signal.clone();
            let needed_for_hover = submenu_needed_signal.clone();
            let dismiss_for_hover = submenu_dismiss_callback.clone();
            // Capture the safe-triangle shared state so the hover
            // handler can publish / retract "this is the submenu the
            // pointer is travelling to" for its siblings.
            let safe_triangle_hover = self.safe_triangle.clone();
            // Framework gates events on `arena.is_enabled(self_id)`.
            handler_set = handler_set
                .on_tap({
                    move |_pos, ctx: &mut EventContext| {
                        // Click on submenu trigger opens it immediately
                        ctx.dismiss_child_overlays_except(sub_id);
                        // Build the submenu if this is the first time it is wanted, before
                        // the overlay below is measured against it.
                        needed_for_tap.set(true);
                        ctx.materialize_now(sub_id);
                        ctx.activate(sub_id);
                        open_for_tap.set(true);
                        ctx.show_overlay(OverlayRequest {
                            content_id: sub_id,
                            anchor: self_id,
                            placement: OverlayPlacement::TrailingEdge,
                            dismiss: SubmenuOpenRoute::Tap(ctx.pointer_kind()).dismiss(),
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
                            open_for_hover.set(true);
                            // Build the submenu if this is the first time it is wanted, before
                            // the overlay below is measured against it.
                            needed_for_hover.set(true);
                            ctx.materialize_now(sub_id);
                            // Sibling submenus are dismissed when this
                            // one *opens*, not now: a pointer merely
                            // crossing this row on its way to the
                            // submenu already open two rows up must not
                            // take that submenu down with it. See
                            // `show_overlay_after_replacing_siblings`.
                            ctx.show_overlay_after_replacing_siblings(
                                OverlayRequest {
                                    content_id: sub_id,
                                    anchor: self_id,
                                    placement: OverlayPlacement::TrailingEdge,
                                    dismiss: SubmenuOpenRoute::Hover.dismiss(),
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
                            if ctx.overlay_bounds_for_content(sub_id).is_some() {
                                // The submenu is up and the pointer has
                                // just left this row — which is exactly
                                // where the diagonal toward it begins.
                                // Arm the safe triangle from here: the
                                // framework holds off the overlay's
                                // pointer-leave grace, and siblings
                                // consult the same region before they
                                // switch. `submenu_open` deliberately
                                // stays `true` — the panel is still on
                                // screen, so the row stays highlighted
                                // and `set_expanded` stays honest until
                                // the dismiss callback fires.
                                ctx.arm_overlay_safe_region(sub_id);
                                if let Some(state_rc) = safe_triangle_hover.as_ref() {
                                    state_rc.borrow_mut().submenu_content_id = Some(sub_id);
                                }
                            } else {
                                // Never opened — the hover-open delay was
                                // cancelled while still pending, so the
                                // dismiss callback that would reset the
                                // flag will never fire. Do it here.
                                open_for_hover.set(false);
                                if let Some(state_rc) = safe_triangle_hover.as_ref() {
                                    let mut state = state_rc.borrow_mut();
                                    if state.submenu_content_id == Some(sub_id) {
                                        state.submenu_content_id = None;
                                    }
                                }
                            }
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
                        // 1. Flip the bound state first (Check / Radio),
                        //    so the user-supplied action sees the
                        //    post-activation value.
                        if let Some(ref activate) = mode_activate_for_tap {
                            activate();
                        }
                        // 2. Invoke the user action if any.
                        if let Some(ref action) = *action_for_tap {
                            action(ctx);
                        }
                        // 3. Dismiss the chain when EITHER an action
                        //    fired OR a mode flip happened. Plain items
                        //    without an action used to no-op the click;
                        //    Check/Radio items without an action still
                        //    dismiss because the visible state changed.
                        if action_for_tap.is_some() || mode_activate_for_tap.is_some() {
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
                    let safe_triangle_sibling = self.safe_triangle.clone();
                    move |entered: bool, ctx: &mut EventContext| {
                        if entered {
                            // Safe-triangle gate: while a traversal
                            // toward an open submenu in this list is
                            // live, skip the dismiss — the user may be
                            // crossing this row on the way there rather
                            // than choosing it — and let the overlay's
                            // own pointer-leave grace decide. That grace
                            // tests the cone on every sample and closes
                            // the submenu one close-delay after the
                            // pointer stops heading there, which is the
                            // decision this handler cannot make: it
                            // fires once, at the instant the pointer
                            // crosses onto the row, a pixel or two from
                            // the apex where the cone is still a needle.
                            // Asking whether *that* sample was inside
                            // the cone let one quantized step settle it,
                            // and a submenu died the moment the pointer
                            // left the trigger row for any departure
                            // steeper than the cone.
                            let traversal_live = safe_triangle_sibling
                                .as_ref()
                                .and_then(|state_rc| state_rc.borrow().submenu_content_id)
                                .is_some_and(|sub| ctx.overlay_safe_region_armed(sub));
                            if !traversal_live {
                                ctx.dismiss_child_overlays();
                            }
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
            let needed_for_key = submenu_needed_signal.clone();
            let dismiss_for_key = submenu_dismiss_callback.clone();
            move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                // The "open submenu / go deeper" key is inline-forward:
                // ArrowRight under LTR, ArrowLeft under RTL (submenus open
                // on the trailing edge, which mirrors). The inline-back
                // key (ArrowLeft under LTR, ArrowRight under RTL) is left
                // to bubble / to the framework's nested-overlay dismissal.
                let open_submenu_key = if ctx.is_rtl() {
                    Key::ArrowLeft
                } else {
                    Key::ArrowRight
                };
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::Enter | Key::Space,
                        ..
                    } => {
                        // Mirror the tap activation order: bound-state
                        // mutation first, then user action, then chain
                        // dismissal. Submenu triggers fall through to
                        // the existing open path (they never carry a
                        // bound mode signal).
                        if let Some(ref activate) = mode_activate_for_key {
                            activate();
                        }
                        if let Some(ref action) = *action_for_key {
                            action(ctx);
                            ctx.dismiss_self_overlay_chain();
                        } else if mode_activate_for_key.is_some() {
                            // Check/Radio with no user action — still dismiss.
                            ctx.dismiss_self_overlay_chain();
                        } else if let Some(sub_id) = sub_id {
                            ctx.dismiss_child_overlays_except(sub_id);
                            // Build the submenu if this is the first time it is wanted, before
                            // the overlay below is measured against it.
                            needed_for_key.set(true);
                            ctx.materialize_now(sub_id);
                            ctx.activate(sub_id);
                            open_for_key.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: SubmenuOpenRoute::KeyboardOrAt.dismiss(),
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
                    // Inline-forward arrow opens submenu (ignored on
                    // regular items). RTL-flipped via `open_submenu_key`.
                    WidgetEvent::KeyDown { key, .. } if *key == open_submenu_key => {
                        if let Some(sub_id) = sub_id {
                            ctx.dismiss_child_overlays_except(sub_id);
                            // Build the submenu if this is the first time it is wanted, before
                            // the overlay below is measured against it.
                            needed_for_key.set(true);
                            ctx.materialize_now(sub_id);
                            ctx.activate(sub_id);
                            open_for_key.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: SubmenuOpenRoute::KeyboardOrAt.dismiss(),
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

        // Assistive-tech / automation activation. Click (the default action)
        // and Expand (submenu triggers) both run the shared activation.
        handler_set = handler_set.on_access_action({
            let activate = activate_item.clone();
            move |action, ctx: &mut EventContext| -> EventResponse {
                use teksilo_core::accesskit::Action;
                if matches!(action, Action::Click | Action::Expand) {
                    activate(ctx);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
        });

        // Cursor is always Pointer. `HandlerSet::cursor` stores a *static*
        // `CursorIcon` on the node — there is no reactive form — so reading
        // `effective_enabled.get()` here only snapshots the value at build
        // time. Menu-bar dropdowns materialise their items while dormant
        // (often with every enablement signal still `false`), so that
        // snapshot permanently stuck rows on `NotAllowed` even after the
        // signal later went true and clicks started working. The framework
        // also gates *all* events — including `PointerEnter`, the path that
        // applies `node_cursor` — on `arena.is_enabled`, so a `NotAllowed`
        // icon could never show for a truly-disabled item either. Match
        // `Button` / `IconButton`: Pointer while interactive; greyed paint
        // + gated events while disabled.
        handler_set = handler_set.cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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
        use teksilo_core::accesskit::{HasPopup, Role, Toggled};

        // Role reflects the mode: Plain → MenuItem, Check → MenuItemCheckBox,
        // Radio → MenuItemRadio. Submenu triggers always render as
        // Role::MenuItem (independent of mode — submenu+checkable is
        // not a supported combination).
        let role = match &self.mode {
            MenuItemMode::Plain => Role::MenuItem,
            MenuItemMode::Check(_) => Role::MenuItemCheckBox,
            MenuItemMode::Radio { .. } => Role::MenuItemRadio,
        };
        builder.set_role(role);
        // Use the stripped form for the announced name — screen readers
        // say "Save", not "ampersand-Save". Re-parse from a fresh
        // `resolve_now()` every walk rather than reading the build-time
        // `parsed_mnemonic` cache: a locale switch marks the tree dirty
        // (re-walking AT) but does NOT rebuild the item, so the cache
        // would otherwise announce the stale-locale name. The cached
        // mnemonic index is still used for the underline in `paint`.
        let parsed_name = parse_mnemonic(&self.label.resolve_now()).stripped;
        builder.set_name(parsed_name);

        // Toggle state for Check / Radio. Mirrors `Checkbox`:
        // `set_toggled(bool)` for binary, `inner_mut().set_toggled(Toggled::Mixed)`
        // for tri-state Indeterminate.
        match &self.mode {
            MenuItemMode::Plain => {}
            MenuItemMode::Check(CheckKind::TwoState(s)) => {
                builder.set_toggled(s.get());
            }
            MenuItemMode::Check(CheckKind::Reflect(s)) => {
                builder.set_toggled(s.get());
            }
            MenuItemMode::Check(CheckKind::TriState(s)) => match s.get() {
                CheckState::Unchecked => builder.set_toggled(false),
                CheckState::Checked => builder.set_toggled(true),
                CheckState::Indeterminate => {
                    builder.inner_mut().set_toggled(Toggled::Mixed);
                }
            },
            MenuItemMode::Radio { value, selected } => {
                builder.set_toggled(selected.get() == *value);
            }
        }

        // Radio "2 of N" — push every group member id (including self)
        // into the AT node so assistive tech can announce
        // position-in-set. Only emitted for Radio items where the
        // enclosing MenuList wired up the group buffer. Mirrors
        // [`RadioButton::accessibility`] exactly.
        if let (MenuItemMode::Radio { .. }, Some(buf)) = (&self.mode, self.radio_group_ids.as_ref())
        {
            for sibling in buf.borrow().iter().copied() {
                builder.push_to_radio_group(teksilo_core::accessibility::widget_id_to_node_id(
                    sibling,
                ));
            }
        }

        // A submenu trigger exposes `has_popup(Menu)` so screen
        // readers announce the item as leading into a nested menu,
        // and `set_expanded` reflects whether the submenu is
        // currently visible. We check `submenu_content_id` rather
        // than `submenu_factory`: the factory is moved out during
        // `build()` via `take()`, so by the time the framework
        // queries accessibility the factory is always `None`,
        // but the content id survives.
        if self.submenu_content_id.is_some() {
            builder.set_has_popup(HasPopup::Menu);
            let open = self.submenu_open.get();
            builder.set_expanded(open);
            // State-appropriate Expand/Collapse (Click, advertised below, opens
            // it too). Handled by the `on_access_action` handler in `build()`.
            if open {
                builder.add_action(teksilo_core::accesskit::Action::Collapse);
            } else {
                builder.add_action(teksilo_core::accesskit::Action::Expand);
            }
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(teksilo_core::accesskit::Action::Click);
        // Announce the current chord *live*: a manual label, else the
        // per-id signal's present value — so AT reflects a rebind even
        // though the item itself is never rebuilt for shortcut changes.
        let accel = self.shortcut_label.clone().or_else(|| {
            self.shortcut_signal
                .as_ref()
                .and_then(|sig| sig.get().map(format_keystroke))
        });
        if let Some(accel) = accel {
            builder.set_keyboard_shortcut(accel);
        }
        // A trailing hint is prose, not a chord — it belongs in the
        // description so AT reads "Scene, inside" rather than announcing
        // "inside" as a key to press. Resolved here rather than at build
        // time so the a11y tree follows a live locale change too.
        if let Some(hint) = self.trailing_hint.as_ref() {
            builder.set_description(hint.resolve_now());
        }

        // Mnemonic — populates AccessKit's `access_key` field, which
        // Windows Narrator announces as "Access key: F" on items
        // carrying a single-character menu accelerator. Distinct from
        // the (rebindable) `keyboard_shortcut` field above, which
        // carries Ctrl+S-style accelerators. Empty / non-mnemonic
        // labels emit nothing.
        if let Some(parsed) = self.parsed_mnemonic.as_ref()
            && let Some(k) = parsed.key_lower
        {
            builder
                .inner_mut()
                .set_access_key(k.to_ascii_uppercase().to_string());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }

    /// Opt into reflection so [`MenuList::build`](crate::menu_list::MenuList::build)
    /// can downcast a pending boxed item and install its radio group
    /// buffer before the item is added to the arena.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
