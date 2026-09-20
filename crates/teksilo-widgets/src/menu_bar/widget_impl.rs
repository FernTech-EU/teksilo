// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementations for [`MenuBar`] and the two private
//! wrappers it builds — `MenuOverlayHost` (the open dropdown's frame, which
//! resets the bar's open index on dismissal and routes ArrowLeft / ArrowRight
//! between sibling menus) and `RevealHeightBox` (which matches the floating
//! collapsed bar's height to its hamburger).

use super::*;

// ---------------------------------------------------------------------------
// MenuBar Widget impl
// ---------------------------------------------------------------------------

impl Widget for MenuBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Mirror the model into the native OS menu bar (macOS) when requested.
        // The bridge is a no-op without a `NativeMenuHandle` in app-state.
        if self.native_mode.installs_native()
            && cfg!(target_os = "macos")
            && let Some(model) = &self.model
        {
            *self.native_binding.borrow_mut() = crate::menu::native::install(model, ctx);
        }

        // A runtime structural change (`MenuModel::push_item`/`remove`/…) bumps
        // the model version; rebuild so the in-window dropdowns AND the native
        // menu re-derive from the new structure.
        if let Some(model) = &self.model {
            model.version().bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                teksilo_core::BindingLevel::Rebuild,
            );
        }

        // On macOS with `Suppress`, the global menu bar IS the menu — render
        // only the optional leading/trailing slots in-window (no triggers, no
        // F10/Alt dispatcher).
        if self.native_mode.suppresses_in_window() {
            return self.build_suppressed(ctx);
        }

        let theme_signal = ctx.theme_signal();

        let open_index: Signal<Option<usize>> = ctx.signal(None);
        let menu_ctx = MenuContext::new(open_index);

        // Build the full row: [leading_slot | triggers... | Spacer | trailing_slot]
        let mut row = HStack::new().spacing(2.0);

        // Leading slot (memoized — the same widgets survive each rebuild)
        row = Self::add_slot(ctx, row, &mut self.leading_slot, &mut self.leading_slot_ids);

        // Menu triggers + content
        let mut trigger_ids = Vec::new();
        let mut content_ids = Vec::new();
        // Mnemonic table built alongside triggers: `lowercase char →
        // trigger array index`. Drives the window-level dispatcher
        // for Alt+letter activation.
        let mut mnemonic_table: HashMap<char, usize> = HashMap::new();

        // Both bar flavours re-derive their entries every build and re-run
        // the (Fn) factories, so neither consumes the state it needs to
        // rebuild: model-built bars re-derive from the (possibly mutated)
        // model, classic `.menu()` bars iterate their retained entries by
        // reference. Consuming `self.entries` here (the old `mem::take`)
        // left the bar empty on the next theme / locale rebuild.
        let model_entries = self.model.as_ref().map(Self::model_entries);
        let entries: &[MenuBarEntry] = match &model_entries {
            Some(derived) => derived,
            None => &self.entries,
        };
        for (i, entry) in entries.iter().enumerate() {
            let parsed: ParsedMnemonic = parse_mnemonic(&entry.label.resolve_now());

            // Wrap factory output in MenuOverlayHost for focus/key handling
            let host = MenuOverlayHost {
                inner: Some((entry.factory)()),
                menu_ctx: menu_ctx.clone(),
                menu_index: i,
                inner_id: None,
            };
            // Detached: a menu's content is shown through an overlay, never
            // inline under the bar. Owned all the same, so a rebuilt menubar
            // reaps the menus it replaced instead of stranding one host — and
            // its whole `MenuList` — per rebuild.
            // Built the first time *this* menu is opened. A menu bar used to
            // build every menu's whole `MenuList` — and every submenu under it —
            // on each rebuild of the bar, which a locale or shortcut change
            // triggers. See `teksilo_core::deferred_subtree::DeferredSubtree`.
            let opened_here = menu_ctx.open_index.map(move |open| *open == Some(i));
            let content_id = ctx.add_detached_deferred(opened_here, host);
            ctx.set_dormant(content_id);

            let trigger = MenuBarTrigger {
                label: entry.label.clone(),
                stripped_name: parsed.stripped.clone(),
                mnemonic_key: parsed.key_lower,
                index: i,
                menu_ctx: menu_ctx.clone(),
                root_child_id: None,
            };
            let trigger_id = ctx.add(trigger);
            row = row.child(trigger_id);

            if let Some(k) = parsed.key_lower {
                if let Some(prev) = mnemonic_table.insert(k, i) {
                    debug_assert!(
                        false,
                        "MenuBar: duplicate mnemonic {:?} (triggers {} and {})",
                        k, prev, i
                    );
                }
            }

            trigger_ids.push(trigger_id);
            content_ids.push(content_id);
        }

        // Register all trigger/content IDs in the context.
        // focus_id is initially content_id; MenuOverlayHost::build() will
        // overwrite it with the actual inner MenuList ID.
        for (i, (&tid, &cid)) in trigger_ids.iter().zip(content_ids.iter()).enumerate() {
            menu_ctx.register(i, tid, cid, cid);
        }

        // Spacer pushes triggers left, trailing slot right
        row = row.child(Spacer::new());

        // Trailing slot (memoized — the same widgets survive each rebuild)
        row = Self::add_slot(
            ctx,
            row,
            &mut self.trailing_slot,
            &mut self.trailing_slot_ids,
        );

        let row_id = ctx.add(row);

        let bg = RectWidget::new()
            .background(SurfaceRole::Main)
            .border_color(theme_signal.map(|t| t.colors.border.with_alpha(0.2)))
            .border_width(0.0_f32);
        let bg_id = ctx.add(bg);

        let padding = Padding::symmetric(0.0, 2.0).child(row_id);
        let padding_id = ctx.add(padding);

        let zstack_id = ctx.add(ZStack::new().child(bg_id).child(padding_id));
        // Shared cell holding the hamburger id once it's built below — the
        // `RevealHeightBox` measures it to size the floating bar (filled at
        // `anchor_cell.set(...)`, the same pattern as the overlay anchor).
        let ham_cell: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
        // In collapsible mode the `Role::MenuBar` landmark lives on the
        // bar content node (not the composing widget) so it travels into
        // the floating overlay AND so `overlay_is_host_surface` treats
        // the revealed bar as a host (menu-open dismissal spares it). The
        // content is wrapped in a `RevealHeightBox` so the *floating* bar's
        // height matches the hamburger button — the triggers center
        // vertically (the inner `HStack`'s default `VAlignment::Center`);
        // the inline bar keeps its natural height. That, in turn, is
        // wrapped in an `Unroll` so the floating bar unrolls out of the
        // hamburger on open and rolls back into it on close (driven by
        // `reveal_progress`; the overlay owns the tween + dismissal
        // deferral — see the reveal closure below). `reveal_progress`
        // stays at `1.0` for the inline bar, so `Unroll` is a no-op there.
        let root_id = if self.collapse_policy.is_some() {
            let height_box = ctx.add(RevealHeightBox {
                child_id: None,
                pending_child: Some(PendingChild::Id(zstack_id)),
                revealed: self.revealed.clone(),
                hamburger_id: ham_cell.clone(),
            });
            // Unrolls trailing-ward from the hamburger's edge (RTL flip is
            // a follow-up, matching the docking handle-direction caveat).
            ctx.add(
                Unroll::from_progress(self.reveal_progress.clone())
                    .child(height_box)
                    .access_role(teksilo_core::accesskit::Role::MenuBar),
            )
        } else {
            zstack_id
        };
        self.root_child_id = Some(root_id);
        self.bar_id = Some(root_id);

        // Collapsible (hamburger) mode: build the hamburger button and
        // the reveal closure that floats the bar as an overlay; gate
        // inline visibility on `collapsed` / `revealed`.
        let mut children = vec![root_id];
        let collapsible_reveal: Option<MenubarReveal> = if self.collapse_policy.is_some() {
            let bar_id = root_id;
            let revealed = self.revealed.clone();
            let collapsed = self.collapsed.clone();
            // Captured at build (EventContext can't reach motion / pref):
            // the unroll tween duration and whether to snap. A theme /
            // reduced-motion change rebuilds the bar, refreshing both.
            let reveal_progress = self.reveal_progress.clone();
            let reveal_duration = ctx.theme().motion.duration_collapse;
            let reduced_motion = ctx.prefers_reduced_motion();

            // The bar overlay trails the hamburger (the developer is
            // responsible for placing the hamburger). The anchor cell is
            // filled after the button is added, since the reveal closure
            // is created before the button id is known.
            let anchor_cell: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
            // First trigger, focused on reveal so the bar is immediately
            // keyboard-navigable (arrows move between menus, Enter opens).
            let first_trigger = trigger_ids.first().copied();

            let reveal: MenubarReveal = {
                let revealed = revealed.clone();
                let anchor_cell = anchor_cell.clone();
                let reveal_progress = reveal_progress.clone();
                Rc::new(move |ctx: &mut EventContext| {
                    if revealed.get() {
                        return; // idempotent — already revealed
                    }
                    revealed.set(true);
                    ctx.activate(bar_id);
                    let anchor = anchor_cell.get().unwrap_or(bar_id);
                    let on_dismiss: teksilo_core::overlay::OverlayDismissCallback = {
                        let revealed = revealed.clone();
                        Rc::new(move |_, _| revealed.set(false))
                    };
                    let request = OverlayRequest {
                        content_id: bar_id,
                        anchor,
                        placement: OverlayPlacement::TrailingEdge,
                        dismiss: DismissBehavior::EscapeOrClickOutside,
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                        on_dismiss: Some(on_dismiss),
                        fade_duration: None,
                    };
                    if reduced_motion {
                        // No tween: show fully unrolled; dismissal is immediate.
                        reveal_progress.set(1.0);
                        ctx.show_overlay(request);
                    } else {
                        // Start rolled up, then the overlay tweens 0 → 1 on
                        // show and 1 → 0 on close (deferring teardown until
                        // the roll-back completes).
                        reveal_progress.set(0.0);
                        ctx.show_overlay_with_reveal(
                            request,
                            reveal_progress.clone(),
                            reveal_duration,
                        );
                    }
                    if let Some(trigger) = first_trigger {
                        ctx.request_focus(trigger);
                    }
                })
            };

            // `IconButton::menu()` already advertises `HasPopup::Menu` and
            // an accessible name ("Menu"). Binding `expanded_when(revealed)`
            // completes the ARIA disclosure pattern: the button reports
            // `expanded=true` while the bar is shown, `false` while collapsed.
            let hamburger = IconButton::menu()
                .size(self.hamburger_size)
                .expanded_when(revealed.clone())
                .on_activate_fn({
                    let reveal = reveal.clone();
                    move |ctx| reveal(ctx)
                });
            let hamburger_id = ctx.add(hamburger);
            anchor_cell.set(Some(hamburger_id));
            // Let the bar's `RevealHeightBox` measure the hamburger so the
            // floating overlay's height matches the button.
            ham_cell.set(Some(hamburger_id));
            self.hamburger_id = Some(hamburger_id);

            // Hamburger visible only while collapsed.
            ctx.visible_when(hamburger_id, collapsed.clone());
            // Bar active when shown inline (`!collapsed`) OR as the
            // floating overlay (`revealed`). Keeping it active while
            // revealed prevents the visibility binding from fighting the
            // overlay activation.
            let bar_active = collapsed.zip(&revealed).map(|(c, r)| !*c || *r);
            ctx.visible_when(bar_id, bar_active);

            children.push(hamburger_id);
            Some(reveal)
        } else {
            None
        };

        // Window-level menubar key dispatcher (F10 / Alt+letter /
        // Alt-tap). Installed on every platform — `MenuBar` is an
        // in-window widget menu, not the OS system menu, so the
        // dispatcher's job is to wire framework menus to keyboard
        // accelerators regardless of host OS.
        //
        // **macOS**: the dispatcher's `Alt+letter` branch is compiled
        // out (see `MenuBarDispatcher::try_handle`) because the OS
        // rewrites Option+letter for accented character composition
        // before the app sees the keystroke. F10 and bare-Alt-tap
        // continue to fire on macOS through this same dispatcher.
        //
        // Drop the previous guard BEFORE installing the new one so
        // the slot is empty when `install_menubar_dispatcher` runs
        // its `debug_assert!(slot.is_none())`. Otherwise a rebuild
        // of `MenuBar` (e.g. when a composing ancestor rebuilds)
        // trips the assert in debug builds and would over-write the
        // slot under another live guard in release.
        if self.install_dispatcher
            && let Some(window) = ctx.window()
        {
            *self.menubar_guard.borrow_mut() = None;
            let inner = MenuBarDispatcher {
                trigger_ids: trigger_ids.clone(),
                mnemonic_table,
            };
            let dispatcher: Rc<dyn MenubarDispatcher> = match collapsible_reveal {
                Some(reveal) => Rc::new(CollapsibleMenuBarDispatcher {
                    inner,
                    collapsed: self.collapsed.clone(),
                    reveal,
                }),
                None => Rc::new(inner),
            };
            let guard = window.install_menubar_dispatcher(dispatcher);
            *self.menubar_guard.borrow_mut() = Some(guard);
        }

        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Collapsed: size to the hamburger's natural size (a small box),
        // don't stretch to the full allotted width.
        if self.collapse_policy.is_some() && self.collapsed.get() {
            return match self.hamburger_id {
                Some(id) => ctx
                    .child_size(id, SizeProposal::unspecified())
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
                None => proposal.resolve(0.0, 0.0),
            }
            .into();
        }
        match self.root_child_id {
            Some(id) => {
                let content_proposal = SizeProposal {
                    width: proposal.width,
                    height: None,
                };
                let size = ctx
                    .child_size(id, content_proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(proposal.width.unwrap_or(size.width), size.height)
            }
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Responsive collapse decision (Toolbar pattern): compare the
        // bar's intrinsic width against the allotted width and toggle
        // `collapsed`, idempotently (the guard avoids relayout churn).
        if let Some(policy) = self.collapse_policy {
            let should_collapse = match policy {
                CollapsePolicy::Always => true,
                CollapsePolicy::Responsive => {
                    if self.revealed.get() {
                        // Don't un-collapse while the overlay is up — it
                        // would make the bar both inline and floating.
                        self.collapsed.get()
                    } else if let (Some(bar_id), Some(avail)) = (self.bar_id, proposal.width) {
                        ctx.measure_intrinsic(bar_id, SizeProposal::unspecified())
                            .map(|s| s.width)
                            .unwrap_or(0.0)
                            > avail + 0.5
                    } else {
                        // Unbounded width (or no bar) → never collapse.
                        false
                    }
                }
            };
            if self.last_collapsed.get() != should_collapse {
                self.last_collapsed.set(should_collapse);
                self.collapsed.set(should_collapse);
            }
        }

        // The hamburger keeps a constant width: place it at its intrinsic
        // size, leading-aligned, so a stretching parent can't widen it.
        // Everything else (the bar, inline or as the re-laid overlay) fills
        // the bounds; dormant children are skipped by the layout pass.
        let collapsed = self.collapse_policy.is_some() && self.collapsed.get();
        for child in children.iter_mut() {
            if collapsed && Some(child.id) == self.hamburger_id {
                let size = ctx
                    .measure_intrinsic(child.id, SizeProposal::unspecified())
                    .unwrap_or_else(|| bounds.size());
                let x = if ctx.is_rtl() {
                    bounds.right() - size.width
                } else {
                    bounds.x
                };
                child.origin = Point::new(x, bounds.y);
                child.size = size;
            } else {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // In collapsible mode the `Role::MenuBar` landmark lives on the
        // bar content node so it travels into the floating overlay; the
        // composing widget node stays a generic container.
        if self.collapse_policy.is_none() {
            builder.set_role(teksilo_core::accesskit::Role::MenuBar);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut v: Vec<WidgetId> = self.root_child_id.into_iter().collect();
        if let Some(h) = self.hamburger_id {
            v.push(h);
        }
        v
    }

    /// Reconcile on rebuild. The menu triggers are re-derived fresh each build
    /// (the model may have changed) and the reconcile reaps the superseded
    /// ones; the memoized leading/trailing slot widgets (see `add_slot`) are
    /// re-attached by id and kept alive, so a stateful slot control — a search
    /// field, a focused button, an avatar with hover state — survives a
    /// model-version / theme / locale rebuild instead of being rebuilt from
    /// scratch.
    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }
}
impl Widget for RevealHeightBox {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Re-layout when the bar reveals / hides so the height switches
        // between hamburger-matched (floating) and natural (inline).
        self.revealed.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::Relayout,
        );
        self.child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let child = self.child_id;
        if self.revealed.get() {
            if let Some(ham) = self.hamburger_id.get() {
                if let Some(h) = ctx
                    .measure_intrinsic(ham, SizeProposal::unspecified())
                    .map(|s| s.height)
                {
                    let child_w = child
                        .and_then(|id| {
                            ctx.child_size(
                                id,
                                SizeProposal {
                                    width: proposal.width,
                                    height: Some(h),
                                },
                            )
                        })
                        .map(|s| s.width)
                        .unwrap_or(0.0);
                    let w = proposal.width.unwrap_or(child_w);
                    return Size::new(w, h).into();
                }
            }
        }
        child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or(Size::ZERO)
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

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
impl Widget for MenuOverlayHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let inner_widget = self.inner.take().expect("MenuOverlayHost built twice");
        let id = ctx.add_boxed(inner_widget);
        self.inner_id = Some(id);

        // Register inner widget as the focus target for this menu index
        self.menu_ctx.set_focus_id(self.menu_index, id);

        let menu_ctx = self.menu_ctx.clone();
        let menu_index = self.menu_index;
        let handler_set = HandlerSet::new()
            .on_focus({
                let menu_ctx = menu_ctx.clone();
                move |gained: bool, _ctx: &mut EventContext| {
                    // Focus left this menu, so it is on its way out — record
                    // that, and nothing more. The dismissal itself belongs to
                    // the framework's focus-out rule
                    // (`dismiss_overlays_left_by_focus`), and the trigger gets
                    // its focus back from the overlay's own `focus_restore`.
                    //
                    // Doing either of those *here* was a race: this handler
                    // fires from inside the `FocusLost` dispatch, i.e. before
                    // `focus_with_origin_ops` has installed the new target, so
                    // the `request_focus(trigger)` it used to queue resolved
                    // first and was then silently overwritten by the very
                    // `set_focused` that was still in flight — a focus flash
                    // onto the trigger that no `FocusLost` ever accounted for.
                    // Keeping only the signal write leaves this side idempotent
                    // and lets every dismissal path (Escape, click-outside,
                    // Tab) converge on the same a11y state.
                    if !gained && menu_ctx.open_index.get() == Some(menu_index) {
                        menu_ctx.open_index.set(None);
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    // These keys bubble up from the inner MenuList when it
                    // returns Ignored. Under RTL the bar is laid out
                    // right-to-left, so the previous/next arrows swap.
                    let (left_delta, right_delta) = if ctx.is_rtl() { (1, -1) } else { (-1, 1) };
                    match event {
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
                        WidgetEvent::KeyDown {
                            key: Key::Escape, ..
                        } => {
                            menu_ctx.close(ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            });
        // NOT focusable — the inner MenuList receives focus directly.
        // ArrowLeft/Right and FocusLost bubble from MenuList through here.
        ctx.apply_self_handlers(handler_set);

        vec![id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.inner_id
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
        // The inner widget (typically `MenuList`) owns the `Role::Menu`
        // semantics. A second Menu role here would nest two Menu nodes
        // per dropdown, confusing screen readers that look for a single
        // Menu per popup. `GenericContainer` is the ARIA `none`/`presentation`
        // equivalent: the host is kept in the tree for focus/key routing
        // but is ignored by assistive tech.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.inner_id.into_iter().collect()
    }
}
