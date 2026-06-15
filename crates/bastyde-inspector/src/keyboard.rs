// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Panel-scoped keyboard shortcuts for the inspector.
//!
//! `PanelShortcutHost` wraps the panel content and registers all
//! inspector-internal shortcuts (P / B / T / Shift+T / Esc) with
//! `BuildContext::register_shortcut`, which scopes them to the host's
//! own `WidgetId`. Effect: the chord only fires when focus is on the
//! host or any descendant — i.e. somewhere inside the panel — so a
//! single-letter chord like `P` doesn't hijack typing in the user
//! app's text inputs.

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{Key, Modifiers};
use bastyde_core::intent::Intent;
use bastyde_core::shortcut::{KeyStroke, Shortcut};
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use crate::state::{InspectorState, NUM_TABS};

pub(crate) struct PanelShortcutHost {
    state: InspectorState,
    inner: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl PanelShortcutHost {
    pub fn new(state: InspectorState, inner: impl Widget + 'static) -> Self {
        Self {
            state,
            inner: Some(Box::new(inner)),
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for PanelShortcutHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelShortcutHost").finish()
    }
}

impl Widget for PanelShortcutHost {
    fn declare_shortcuts(&self) -> Vec<Shortcut> {
        // Mirror the build()-time registrations *without* handlers so
        // settings UIs (and any other registry consumer) see the
        // inspector chords from the moment the host mounts — and,
        // because Switcher walks declare_shortcuts on its Pending
        // slots too, even before the panel branch is selected.
        // build() then upserts the same ids with real `on_activate`
        // closures.
        vec![
            Shortcut::new("__bastyde_inspector.pick")
                .name("Toggle Picker")
                .primary(KeyStroke::ctrl(Key::P))
                .build(),
            Shortcut::new("__bastyde_inspector.bounds_cycle")
                .name("Cycle Bounds Overlay")
                .primary(KeyStroke::ctrl(Key::B))
                .build(),
            Shortcut::new("__bastyde_inspector.tab_next")
                .name("Next Tab")
                .primary(KeyStroke::ctrl(Key::Tab))
                .build(),
            Shortcut::new("__bastyde_inspector.tab_prev")
                .name("Previous Tab")
                .primary(KeyStroke::ctrl_shift(Key::Tab))
                .build(),
            Shortcut::new("__bastyde_inspector.escape")
                .name("Inspector: Escape")
                .primary(KeyStroke::new(Key::Escape, Modifiers::empty()))
                .build(),
        ]
    }

    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // ── Pick (Ctrl+P) ─────────────────────────────────────────
        let st = self.state.clone();
        ctx.register_shortcut(
            Shortcut::new("__bastyde_inspector.pick")
                .name("Toggle Picker")
                .primary(KeyStroke::ctrl(Key::P))
                .on_activate(move |_ks, _c| {
                    st.picker_mode.set(!st.picker_mode.get());
                    Intent::new("__bastyde_inspector.pick")
                })
                .build(),
        );

        // ── Bounds cycle (Ctrl+B) ─────────────────────────────────
        let st = self.state.clone();
        ctx.register_shortcut(
            Shortcut::new("__bastyde_inspector.bounds_cycle")
                .name("Cycle Bounds Overlay")
                .primary(KeyStroke::ctrl(Key::B))
                .on_activate(move |_ks, _c| {
                    st.overlay_mode.set(st.overlay_mode.get().next());
                    Intent::new("__bastyde_inspector.bounds_cycle")
                })
                .build(),
        );

        // ── Next tab (Ctrl+Tab) ───────────────────────────────────
        let st = self.state.clone();
        ctx.register_shortcut(
            Shortcut::new("__bastyde_inspector.tab_next")
                .name("Next Tab")
                .primary(KeyStroke::ctrl(Key::Tab))
                .on_activate(move |_ks, _c| {
                    let cur = st.active_tab.get();
                    st.active_tab.set((cur + 1) % NUM_TABS);
                    Intent::new("__bastyde_inspector.tab_next")
                })
                .build(),
        );

        // ── Previous tab (Ctrl+Shift+Tab) ─────────────────────────
        let st = self.state.clone();
        ctx.register_shortcut(
            Shortcut::new("__bastyde_inspector.tab_prev")
                .name("Previous Tab")
                .primary(KeyStroke::ctrl_shift(Key::Tab))
                .on_activate(move |_ks, _c| {
                    let cur = st.active_tab.get();
                    st.active_tab
                        .set(if cur == 0 { NUM_TABS - 1 } else { cur - 1 });
                    Intent::new("__bastyde_inspector.tab_prev")
                })
                .build(),
        );

        // ── Esc: stop picking, otherwise close the panel ──────────
        let st = self.state.clone();
        ctx.register_shortcut(
            Shortcut::new("__bastyde_inspector.escape")
                .name("Inspector: Escape")
                .primary(KeyStroke::new(Key::Escape, Modifiers::empty()))
                .on_activate(move |_ks, _c| {
                    if st.picker_mode.get() {
                        st.picker_mode.set(false);
                    } else {
                        st.open.set(false);
                    }
                    Intent::new("__bastyde_inspector.escape")
                })
                .build(),
        );

        // Wire the actual panel content as our single child.
        if let Some(inner) = self.inner.take() {
            let id = ctx.add_boxed(inner);
            self.root_child_id = Some(id);
            vec![id]
        } else {
            Vec::new()
        }
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .map(LayoutResponse::from)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for c in children.iter_mut() {
            c.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            c.size = bastyde_canvas::Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
