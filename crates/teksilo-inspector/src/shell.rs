// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `InspectorShell` — the composing widget that wraps a window's user
//! root in the inspector's UI surface.
//!
//! Tree shape produced:
//!
//! ```text
//! InspectorShell
//! └── VStack
//!     ├── Expanded (flex=1) {
//!     │     ZStack {
//!     │       user_root,                              // pre-existing
//!     │       HighlightLayer { event_pass_through },  // selection border
//!     │       PickerOverlay { mounted when picking },
//!     │     }
//!     │   }
//!     ├── BoundsTracker (zero size)
//!     ├── PickResolver (zero size)
//!     └── Switcher(open as 0|1) {
//!           0: zero-size placeholder,
//!           1: Panel { Tabs (Tree, Properties, A11y) },
//!         }
//! ```

use teksilo_canvas::SizeProposal;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::lit;
use teksilo_widgets::primitives::{Expand, FixedSize, HStack, Padding, VStack, ZStack};
use teksilo_widgets::{Button, Panel, Segment, SegmentId, SegmentedControl, Slider, TabWidget};

use crate::grip::InspectorGrip;
use crate::highlight::{BoundsTracker, HighlightLayer};
use crate::keyboard::PanelShortcutHost;
use crate::picker::{PickResolver, PickerOverlay};
use crate::resize_handle::ResizeHandle;
use crate::state::{InspectorState, OverlayMode};
use crate::tabs::accessibility::A11yTab;
use crate::tabs::data_models::DataModelsTab;
use crate::tabs::focus::FocusTab;
use crate::tabs::locale::LocaleTab;
use crate::tabs::overlays::OverlaysTab;
use crate::tabs::pointers::{PointerWatchOverlay, PointersTab};
use crate::tabs::properties::PropertiesTab;
use crate::tabs::shortcuts::ShortcutsTab;
use crate::tabs::theme::ThemeTab;
use crate::tabs::tree::TreeTab;

/// Composing widget that takes ownership of wrapping a user-root id
/// with the inspector UI. Created by the post-root hook in
/// `state::install`.
pub(crate) struct InspectorShell {
    user_root_id: WidgetId,
    state: InspectorState,
    root_child_id: Option<WidgetId>,
}

impl InspectorShell {
    pub fn new(user_root_id: WidgetId, state: InspectorState) -> Self {
        Self {
            user_root_id,
            state,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for InspectorShell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorShell")
            .field("user_root_id", &self.user_root_id)
            .finish()
    }
}

impl Widget for InspectorShell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();

        // Visual + helper layers riding on top of the user root.
        let highlight = HighlightLayer::new(state.clone()).event_pass_through(true);
        let bounds_tracker = BoundsTracker::new(state.clone()).event_pass_through(true);
        let pick_resolver = PickResolver::new(state.clone()).event_pass_through(true);

        // Picker overlay — only active when picker_mode is true.
        //
        // We deliberately do NOT use a `Switcher { empty_filler,
        // PickerOverlay }` here: the outer ZStack centers any 0×0
        // child at the window center, and `Rect::contains` is
        // inclusive on every side, so a 0×0 rect at the center point
        // claims the center click and prevents it from reaching the
        // user widgets behind. Using `visible_when` directly leaves
        // the overlay dormant (hit-test skips it via `is_active`)
        // when picker mode is off.
        let picker_overlay_id = ctx.add(PickerOverlay::new(state.clone()));
        ctx.visible_when(picker_overlay_id, state.picker_mode.clone());
        // Initial state: park dormant immediately so the very first
        // hit-test (which can run before layout's
        // `process_state_changes` has had a chance to evaluate
        // `visible_state`) doesn't see the picker overlay as a
        // full-window click sponge.
        if !state.picker_mode.get() {
            ctx.set_dormant(picker_overlay_id);
        }

        // Pre-register the picker chain menu as an orphan widget,
        // parked dormant. The picker activates + shows it via
        // `ctx.show_overlay` once `pending_pick_chain` is populated;
        // the rows read live from that signal so the menu rebuilds
        // for each pick. Same orphan-dormant pattern that
        // `PropertiesRows` uses for its "Copy value" context menu.
        let pick_menu_id = build_pick_chain_menu(ctx, state.clone());
        ctx.set_dormant(pick_menu_id);
        state.pick_menu_id.set(Some(pick_menu_id));

        // The pointer watch: the same mounted-only-while-armed shape as the
        // picker, and for the same reason — while it is up it takes the
        // application's input.
        let watch_overlay_id = ctx.add(PointerWatchOverlay::new(state.clone()));
        ctx.visible_when(watch_overlay_id, state.pointer_watch.clone());
        if !state.pointer_watch.get() {
            ctx.set_dormant(watch_overlay_id);
        }

        // The coarse-pointer door. Mounted only once a finger has been seen
        // AND while the panel is closed, so a mouse-driven session never grows
        // one and an open panel (which has its own Close button) does not
        // either. `visible_when` parks it dormant otherwise, which takes it out
        // of hit-testing entirely.
        let grip_id = ctx.add(InspectorGrip::new(state.clone()));
        let grip_visible = state.coarse_pointer.zip(&state.open).map(|t| {
            let (coarse, open) = *t;
            coarse && !open
        });
        ctx.visible_when(grip_id, grip_visible.clone());
        if !grip_visible.get() {
            ctx.set_dormant(grip_id);
        }

        let z = ZStack::new()
            .add_child(self.user_root_id)
            .child(highlight)
            .add_child(picker_overlay_id)
            .add_child(watch_overlay_id)
            .add_child(grip_id);

        // Slot for the inspector panel + its top-edge resize handle.
        // The Switcher gates the whole block on `state.open`: closed
        // collapses both handle and panel to zero so the user-root
        // takes the full window. Open shows the handle on top of the
        // panel (handle drags drive `state.panel_height`).
        //
        // `PanelShortcutHost` wraps the *outer* Switcher (not the
        // panel content inside) so the P / B / T / Shift+T / Esc
        // shortcuts register with the registry the moment the host
        // mounts — without this, the lazy `Switcher` skipped building
        // its panel branch until the first F12, and a separate
        // `ShortcutSettings` UI mounted before that wouldn't see the
        // inspector chords. Scoping still holds: shortcuts only fire
        // when focus is in PanelShortcutHost's subtree, and focus
        // can't reach the inert `empty_filler` when the panel is
        // closed (it has no focusable descendants).
        //
        // Inner panel content (ResizeHandle + panel body). Each piece
        // is wrapped in `Expand::horizontal().flex(0)` + `FixedSize`
        // so the wrapper claims the parent's full-width proposal
        // (which `FixedSize` alone wouldn't — it reports the child's
        // natural width when no `width` is set). `flex(0)` opts
        // out of the parent VStack's height-slack distribution so we
        // don't compete with the user-root's `Expand(flex=1)`.
        // The strip's own box stays 6 dp of paint, and at Touch density the slot
        // around it grows to the density's target size with the strip centred in
        // it — `TouchTarget`, A10's third mechanism, which is the only one that
        // can work here. The strip's `hit_outset` claims the space *inside* that
        // slot; on its own it claims nothing at all, because an outset is only
        // ever offered the points its own ancestors' bounds contain, and a
        // wrapper that hugs a 6 dp strip contains none of them. Identity at
        // Compact, so a mouse's panel is unchanged to the pixel.
        let panel_block = VStack::new()
            .child(
                Expand::horizontal().flex(0.0).child(
                    teksilo_widgets::primitives::TouchTarget::new()
                        .child(ResizeHandle::new(state.clone())),
                ),
            )
            .child(
                Expand::horizontal().flex(0.0).child(
                    FixedSize::new()
                        .height(state.panel_height.clone())
                        .child(build_panel(state.clone())),
                ),
            );
        let panel_index = state.open.map(|open| if *open { 1usize } else { 0 });
        let panel_switcher = PanelShortcutHost::new(
            state.clone(),
            teksilo_widgets::primitives::Switcher::new(panel_index)
                .child(empty_filler())
                .child(panel_block),
        );

        // Derived height signal — depends on BOTH `open` and
        // `panel_height` so dragging the handle OR toggling the panel
        // re-runs layout. `Signal::zip` dirties on either source.
        //
        // The strip's *slot* is what has to be reserved, not its paint: at Touch
        // the `TouchTarget` above grows it to the density's target size, and
        // reserving 6 dp there would squeeze the panel by the difference. Read
        // once per build, which is exactly when it can change — a density switch
        // rebuilds every root.
        let handle_slot = crate::resize_handle::slot_height(&ctx.theme().input);
        let height_signal = state
            .open
            .zip(&state.panel_height)
            .map(move |(open, h)| if *open { *h + handle_slot } else { 0.0 });

        let stack = VStack::new()
            .child(Expand::new().flex(1.0).child(z))
            .child(bounds_tracker)
            .child(pick_resolver)
            .child(
                Expand::horizontal()
                    .flex(0.0)
                    .child(FixedSize::new().height(height_signal).child(panel_switcher)),
            );

        let root = ctx.add(stack);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .map(LayoutResponse::from)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
            child.size = teksilo_canvas::Size::new(bounds.width, bounds.height);
        }
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        // **Load-bearing.** The shell's one irreplaceable child is the
        // application's own root, which it did not build and cannot rebuild: it
        // is handed the id by the post-root hook and re-attaches it every
        // build. Without the reconcile mode, a rebuild of the shell destroys
        // that subtree and the window is left showing inspector chrome over
        // nothing.
        //
        // Measured, not reasoned: `WidgetTree::set_input_density` marks every
        // root for rebuild, and with `false` the wrapped root came back from a
        // density switch with `widget_type_name() == None` — destroyed. The
        // chrome this build allocates fresh (highlight, tracker, resolver,
        // picker, watch, grip, panel) is *not* re-attached and so is reaped by
        // the same reconcile, which is exactly the `TabWidget` / `SceneView`
        // case: re-attach what is memoised, let the rest go.
        true
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Deliberately empty, and it must stay that way. This node wraps the
        // whole window; a role or a name here would insert an element between
        // the window and the application's own tree, and `set_hidden` would
        // prune the application out of the accessibility tree altogether. A
        // node that emits no properties keeps its default `GenericContainer`,
        // which the walker prunes while promoting its children in order — for a
        // pure wrapper, being invisible to AT *is* the correct behaviour.
    }
}

/// Zero-size placeholder used in Switchers when we want "nothing here".
fn empty_filler() -> impl Widget + 'static {
    FixedSize::new()
        .width(Signal::new(0.0_f32))
        .height(Signal::new(0.0_f32))
}

/// Build the inspector panel's content. Toolbar above a `TabWidget`
/// with [`NUM_TABS`](crate::state::NUM_TABS) tabs, all inside a `Panel`.
fn build_panel(state: InspectorState) -> impl Widget + 'static {
    use teksilo_widgets::TabInfo;
    fn ti(label: &'static str) -> TabInfo {
        TabInfo::new().title(lit!(label))
    }
    let tabs = TabWidget::new(state.active_tab_id.clone())
        // Tree tab is self-scrolling (it owns its own ScrollArea so it
        // can drive scroll-into-view when the picker selects a widget).
        .static_tab(ti("Tree"), fill_width(TreeTab::new(state.clone())))
        .static_tab(
            ti("Properties"),
            fill_width(scrollable_tab(PropertiesTab::new(state.clone()))),
        )
        .static_tab(
            ti("Accessibility"),
            fill_width(scrollable_tab(A11yTab::new(state.clone()))),
        )
        .static_tab(
            ti("Theme"),
            fill_width(scrollable_tab(ThemeTab::new(state.clone()))),
        )
        .static_tab(
            ti("Locale"),
            fill_width(scrollable_tab(LocaleTab::new(state.clone()))),
        )
        .static_tab(
            ti("Focus"),
            fill_width(scrollable_tab(FocusTab::new(state.clone()))),
        )
        .static_tab(
            ti("Shortcuts"),
            fill_width(scrollable_tab(ShortcutsTab::new(state.clone()))),
        )
        .static_tab(
            ti("Overlays"),
            fill_width(scrollable_tab(OverlaysTab::new(state.clone()))),
        )
        .static_tab(
            ti("Models"),
            fill_width(scrollable_tab(DataModelsTab::new(state.clone()))),
        )
        .static_tab(
            ti("Pointers"),
            fill_width(scrollable_tab(PointersTab::new(state.clone()))),
        );

    let toolbar = build_toolbar(state.clone());

    let body = VStack::new()
        .child(toolbar)
        .child(Expand::new().flex(1.0).child(tabs));

    Panel::new()
        .background(teksilo_tokens::SurfaceRole::Main)
        .border_width(1.0)
        .border_color(teksilo_tokens::BorderRole::Default)
        .child(body)
}

/// Toolbar above the tabs:
/// `[Pick] [Bounds: Off|Sel|All] [Opacity slider] ··· [×]`.
fn build_toolbar(state: InspectorState) -> impl Widget + 'static {
    let picker_state_for_label = state.picker_mode.clone();
    let picker_label_signal = picker_state_for_label.map(|active| {
        if *active {
            "Stop picking".to_string()
        } else {
            "Pick".to_string()
        }
    });

    let picker_state_for_click = state.picker_mode.clone();
    let pick_button = Button::new(lit!("Pick"))
        .label(picker_label_signal)
        .on_activate_fn(move |_ctx| {
            let next = !picker_state_for_click.get();
            picker_state_for_click.set(next);
        });

    // Bounds-overlay segmented control, keyed straight to `OverlayMode`:
    // one id per mode, so no positional bridge signal is needed. One
    // observer follows an `overlay_mode` change made elsewhere (the F12
    // shortcut, a restored setting); `on_change` carries a click back.
    // The handle is attached to the selection signal so it lives as long
    // as the toolbar.
    let bounds_selected = Signal::new(Some(overlay_mode_to_segment(state.overlay_mode.get())));
    {
        let selection_target = bounds_selected.clone();
        let h = state.overlay_mode.observe(move |mode| {
            let target = Some(overlay_mode_to_segment(*mode));
            if selection_target.get() != target {
                selection_target.set(target);
            }
        });
        bounds_selected.attach_keepalive(h);
    }
    let bounds_seg = SegmentedControl::new(bounds_selected)
        .segments([
            Segment::new(teksilo_i18n::lit!("Off")).id(OVERLAY_OFF),
            Segment::new(teksilo_i18n::lit!("Sel")).id(OVERLAY_SELECTION),
            Segment::new(teksilo_i18n::lit!("All")).id(OVERLAY_ALL),
        ])
        .on_change({
            let mode_target = state.overlay_mode.clone();
            move |id, _ctx| {
                let next = segment_to_overlay_mode(id);
                if mode_target.get() != next {
                    mode_target.set(next);
                }
            }
        });

    let opacity_slider = FixedSize::new()
        .width(Signal::new(120.0_f32))
        .child(Slider::new(state.overlay_opacity.clone(), 0.1, 1.0));

    // Overflow-overlay toggle. Independent of the Off/Sel/All segmented
    // control; on by default. A check mark in the label reflects the state.
    let overflow_label = state.overflow_overlay.map(|on| {
        if *on {
            "Overflow ✓".to_string()
        } else {
            "Overflow".to_string()
        }
    });
    let overflow_target = state.overflow_overlay.clone();
    let overflow_button = Button::new(lit!("Overflow"))
        .label(overflow_label)
        .on_activate_fn(move |_ctx| {
            let next = !overflow_target.get();
            overflow_target.set(next);
        });

    // Arming the watch is what makes live contacts visible, and it costs the
    // application its input while armed — so the label says which state it is
    // in rather than what it will do.
    let watch_label = state.pointer_watch.map(|on| {
        if *on {
            "Watching ✓".to_string()
        } else {
            "Watch".to_string()
        }
    });
    let watch_target = state.pointer_watch.clone();
    let watch_rows = state.pointer_rows.clone();
    let watch_button = Button::new(lit!("Watch"))
        .label(watch_label)
        .on_activate_fn(move |_ctx| {
            let next = !watch_target.get();
            watch_target.set(next);
            if !next {
                // Disarming clears the readout: the rows described contacts
                // that are no longer being followed.
                watch_rows.set(Vec::new());
            }
        });

    let open_state_for_close = state.open.clone();
    let close_button = Button::new(lit!("×")).on_activate_fn(move |_ctx| {
        open_state_for_close.set(false);
    });

    Padding::symmetric(4.0, 8.0).child(
        HStack::new()
            .spacing(6.0)
            .child(pick_button)
            .child(bounds_seg)
            .child(opacity_slider)
            .child(overflow_button)
            .child(watch_button)
            .child(Expand::new().flex(1.0).child(empty_filler()))
            .child(close_button),
    )
}

/// Stable segment ids for the bounds-overlay picker, so the control's
/// selection means "this mode" rather than "the segment at position 1".
const OVERLAY_OFF: SegmentId = SegmentId::from_u64(1);
const OVERLAY_SELECTION: SegmentId = SegmentId::from_u64(2);
const OVERLAY_ALL: SegmentId = SegmentId::from_u64(3);

fn overlay_mode_to_segment(mode: OverlayMode) -> SegmentId {
    match mode {
        OverlayMode::Off => OVERLAY_OFF,
        OverlayMode::SelectionOnly => OVERLAY_SELECTION,
        OverlayMode::AllBounds => OVERLAY_ALL,
    }
}

fn segment_to_overlay_mode(id: SegmentId) -> OverlayMode {
    if id == OVERLAY_OFF {
        OverlayMode::Off
    } else if id == OVERLAY_SELECTION {
        OverlayMode::SelectionOnly
    } else {
        OverlayMode::AllBounds
    }
}

/// Wrap a tab leaf widget in a `ScrollArea` so long content scrolls
/// instead of overflowing the panel.
fn scrollable_tab(content: impl Widget + 'static) -> impl Widget + 'static {
    teksilo_widgets::ScrollArea::new().child(content)
}

/// Wrap a tab content widget so it claims the full proposed width
/// regardless of its natural size. Without this, narrow leaves
/// (`TextInput`, `ComboBox`) would render at their intrinsic width
/// rather than spanning the panel.
fn fill_width(content: impl Widget + 'static) -> impl Widget + 'static {
    Expand::horizontal().flex(0.0).child(content)
}

/// Maximum number of ancestor rows the picker chain menu can show.
/// Mirrors the cap in `PickResolver::layout_response` — both must
/// match: the menu pre-registers exactly this many `Button` slots,
/// each gated by `visible_when` so unused slots stay dormant for
/// shorter chains.
const PICK_CHAIN_MAX: usize = 10;

/// Build the picker's chain menu. A `Panel(VStack)` of
/// `PICK_CHAIN_MAX` `Button` rows; each row's label is bound to
/// `state.pending_pick_chain[i]` so a single static structure
/// serves every pick. Rows beyond the current chain length collapse
/// via `visible_when` (a row that resolves to no chain entry stays
/// dormant and reports zero size). Returns the panel's `WidgetId`
/// so the caller can park it dormant + reference it from the
/// picker's `OverlayRequest`.
fn build_pick_chain_menu(ctx: &mut BuildContext, state: InspectorState) -> WidgetId {
    let chain_signal = state.pending_pick_chain.clone();
    let mut row_ids: Vec<WidgetId> = Vec::with_capacity(PICK_CHAIN_MAX);
    for i in 0..PICK_CHAIN_MAX {
        let label_signal = chain_signal.map(move |chain| {
            chain
                .get(i)
                .map(|entry| format!("{}  ·  #{:?}", entry.label, entry.id))
                .unwrap_or_default()
        });
        let visible_signal = chain_signal.map(move |chain| chain.len() > i);
        let state_for_action = state.clone();
        let row = Button::new(lit!(""))
            .label(label_signal)
            .on_activate_fn(move |c| {
                let chain = state_for_action.pending_pick_chain.get();
                if let Some(entry) = chain.get(i) {
                    state_for_action.selected_id.set(Some(entry.id));
                }
                state_for_action.pending_pick_chain.set(Vec::new());
                if state_for_action.picker_mode.get() {
                    state_for_action.picker_mode.set(false);
                }
                c.dismiss_self_overlay_chain();
            });
        let row_id = ctx.add(row);
        ctx.visible_when(row_id, visible_signal);
        row_ids.push(row_id);
    }
    let mut menu_vstack = VStack::new().spacing(0.0);
    for id in row_ids {
        menu_vstack = menu_vstack.add_child(id);
    }
    let panel = Panel::new()
        .background(teksilo_tokens::SurfaceRole::Raised)
        .border_color(teksilo_tokens::BorderRole::Default)
        .border_width(1.0)
        .padding(4.0)
        .child(menu_vstack);
    // `add_detached`, not `add`: overlay content parked dormant must record its
    // ownership edge, or every rebuild of the shell strands another copy in the
    // arena. Latent while nothing rebuilt the shell; a density switch does.
    ctx.add_detached(panel)
}
