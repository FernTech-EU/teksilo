// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for `MenuBarTrigger` — one top-level
//! menu header: its label and mnemonic underline, its hover-to-switch and
//! press-to-open pointer handling, and the dropdown overlay it opens.

use super::*;
impl Widget for MenuBarTrigger {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme();
        let radius_control = theme.shape.radius_control;
        use crate::styles::recipe_menu_item_style as menu;
        let index = self.index;
        let menu_ctx = self.menu_ctx.clone();

        // Background role: `AccentSubtle` when open (the Int UI token for
        // highlighted menu-bar entries) or `Transparent` at rest. Replaces
        // the previous hand-mixed `accent.with_alpha(0.12)` wash.
        let bg_role = menu_ctx.open_index.map(move |open| {
            if *open == Some(index) {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        });

        // Text color can't collapse to a pure role: the at-rest state is
        // `text_primary.with_alpha(0.8)` (dimmed primary — distinct from
        // TextRole::Secondary, which is a different hue). Keep a direct
        // `theme_signal` map for the blended case.
        let theme_signal = ctx.theme_signal();
        let text_color = menu_ctx
            .open_index
            .zip(&theme_signal)
            .map(move |(open, t)| {
                if *open == Some(index) {
                    t.colors.text_primary
                } else {
                    t.colors.text_primary.with_alpha(0.8)
                }
            });

        // Label. Uses `MenuLabel` so a single `&` in the trigger
        // string acts as a mnemonic marker — stripped from the
        // visible text and underlined when the window's `alt_down`
        // signal is true.
        let alt_down = ctx
            .window()
            .map(|w| w.alt_down().clone())
            .unwrap_or_else(|| Signal::new(false));
        let label_source: teksilo_core::signal::Prop<String> = self.label.clone().into();
        let label_id = ctx.add(MenuLabel::new(
            label_source,
            alt_down,
            text_color,
            TextStyleRole::Small,
        ));

        let padding = Padding::symmetric(4.0, menu::MENU_ITEM_PADDING_HORIZONTAL).child(label_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .background(bg_role)
            .corner_radius(teksilo_tokens::CornerRadius::uniform(radius_control));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().child(bg_id).child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new()
            .on_tap({
                let menu_ctx = menu_ctx.clone();
                move |_pos, ctx: &mut EventContext| {
                    if menu_ctx.open_index.get() == Some(index) {
                        menu_ctx.close(ctx);
                    } else {
                        menu_ctx.open_at(index, ctx);
                    }
                }
            })
            .on_hover({
                let menu_ctx = menu_ctx.clone();
                move |entered: bool, ctx: &mut EventContext| {
                    if entered {
                        // If another menu is open, switch immediately (no delay)
                        let current = menu_ctx.open_index.get();
                        if current.is_some() && current != Some(index) {
                            menu_ctx.open_at(index, ctx);
                        }
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    // The menu bar lays out right-to-left under RTL, so the
                    // visual "previous/next menu" arrows swap: ArrowLeft moves
                    // to the next (visually-left) menu and ArrowRight to the
                    // previous one.
                    let (left_delta, right_delta) = if ctx.is_rtl() { (1, -1) } else { (-1, 1) };
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown | Key::Enter | Key::Space,
                            ..
                        } => {
                            menu_ctx.open_at(index, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(left_delta, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(right_delta, ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_access_action({
                // Assistive-tech / automation activation. Click toggles the
                // dropdown (matching `on_tap`); Expand opens it, Collapse closes
                // it. Without this the trigger's advertised actions are inert.
                let menu_ctx = menu_ctx.clone();
                move |action, ctx: &mut EventContext| -> EventResponse {
                    use teksilo_core::accesskit::Action;
                    match action {
                        Action::Click => {
                            if menu_ctx.open_index.get() == Some(index) {
                                menu_ctx.close(ctx);
                            } else {
                                menu_ctx.open_at(index, ctx);
                            }
                            EventResponse::Handled
                        }
                        Action::Expand => {
                            menu_ctx.open_at(index, ctx);
                            EventResponse::Handled
                        }
                        Action::Collapse => {
                            menu_ctx.close(ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        // Re-query accessibility when this trigger's open/closed state flips so
        // `set_expanded` stays in sync with the open menu index.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.menu_ctx.open_index.bind_to(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 28.0)),
            None => proposal.resolve(60.0, 28.0),
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
        builder.set_role(teksilo_core::accesskit::Role::MenuItem);
        // Stripped name — set in `build()` from the parsed mnemonic.
        // Falls back to a fresh resolve if the trigger has not been
        // built yet (rare; AT walks always happen post-build).
        if !self.stripped_name.is_empty() {
            builder.set_name(self.stripped_name.clone());
        } else {
            builder.set_name(parse_mnemonic(&self.label.resolve_now()).stripped);
        }
        // Every top-level menu bar entry opens a dropdown Menu.
        builder.set_has_popup(teksilo_core::accesskit::HasPopup::Menu);
        let is_open = self.menu_ctx.open_index.get() == Some(self.index);
        builder.set_expanded(is_open);
        // Advertise the default action (Click) plus the state-appropriate
        // Expand/Collapse so assistive tech (and automation) can open/close the
        // dropdown — the `on_access_action` handler in `build()` drives them.
        // Without this a screen-reader user cannot open any menu.
        builder.add_action(teksilo_core::accesskit::Action::Click);
        if is_open {
            builder.add_action(teksilo_core::accesskit::Action::Collapse);
        } else {
            builder.add_action(teksilo_core::accesskit::Action::Expand);
        }
        // Mnemonic — announced by Windows Narrator as "Access key: F".
        if let Some(k) = self.mnemonic_key {
            builder
                .inner_mut()
                .set_access_key(k.to_ascii_uppercase().to_string());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
