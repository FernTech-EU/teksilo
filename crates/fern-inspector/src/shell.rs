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

use fern_canvas::SizeProposal;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_widgets::primitives::{Expand, FixedSize, HStack, Padding, VStack, ZStack};
use fern_widgets::{Button, Panel, SegmentedControl, Slider, TabWidget};

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

        let z = ZStack::new()
            .add_child(self.user_root_id)
            .child(highlight)
            .add_child(picker_overlay_id);

        // Slot for the inspector panel + its top-edge resize handle.
        // The Switcher gates the whole block on `state.open`: closed
        // collapses both handle and panel to zero so the user-root
        // takes the full window. Open shows the handle on top of the
        // panel (handle drags drive `state.panel_height`).
        // The panel content is wrapped in `PanelShortcutHost` so the
        // P / B / T / Shift+T / Esc shortcuts are scoped to the panel
        // subtree — single-letter chords don't hijack typing in the
        // user app's text inputs.
        // Inner panel content (ResizeHandle + panel body). Each piece
        // is wrapped in `Expand::horizontal().flex(0)` + `FixedSize`
        // so the wrapper claims the parent's full-width proposal
        // (which `FixedSize` alone wouldn't — it reports the child's
        // natural width when no `bind_width` is set). `flex(0)` opts
        // out of the parent VStack's height-slack distribution so we
        // don't compete with the user-root's `Expand(flex=1)`.
        let panel_inner = PanelShortcutHost::new(state.clone(), build_panel(state.clone()));
        let panel_block = VStack::new()
            .child(
                Expand::horizontal().flex(0.0).child(
                    FixedSize::new()
                        .bind_height(Signal::new(crate::resize_handle::HANDLE_HEIGHT))
                        .child(ResizeHandle::new(state.clone())),
                ),
            )
            .child(
                Expand::horizontal().flex(0.0).child(
                    FixedSize::new()
                        .bind_height(state.panel_height.clone())
                        .child(panel_inner),
                ),
            );
        let panel_index = state.open.map(|open| if *open { 1usize } else { 0 });
        let panel_switcher = fern_widgets::primitives::Switcher::new(panel_index)
            .child(empty_filler())
            .child(panel_block);

        // Derived height signal — depends on BOTH `open` and
        // `panel_height` so dragging the handle OR toggling the panel
        // re-runs layout. `Signal::zip` dirties on either source.
        let height_signal = state.open.zip(&state.panel_height).map(|(open, h)| {
            if *open {
                *h + crate::resize_handle::HANDLE_HEIGHT
            } else {
                0.0
            }
        });

        let stack = VStack::new()
            .child(Expand::new().flex(1.0).child(z))
            .child(bounds_tracker)
            .child(pick_resolver)
            .child(
                Expand::horizontal().flex(0.0).child(
                    FixedSize::new()
                        .bind_height(height_signal)
                        .child(panel_switcher),
                ),
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
        bounds: fern_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = fern_canvas::Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Zero-size placeholder used in Switchers when we want "nothing here".
fn empty_filler() -> impl Widget + 'static {
    FixedSize::new()
        .bind_width(Signal::new(0.0_f32))
        .bind_height(Signal::new(0.0_f32))
}

/// Build the inspector panel's content. Toolbar above a `TabWidget`
/// with nine tabs, all inside a `Panel`.
fn build_panel(state: InspectorState) -> impl Widget + 'static {
    use fern_widgets::TabInfo;
    fn ti(label: &'static str) -> TabInfo {
        TabInfo::new().title(fern_i18n::LocalizedString::literal(label))
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
        );

    let toolbar = build_toolbar(state.clone());

    let body = VStack::new()
        .child(toolbar)
        .child(Expand::new().flex(1.0).child(tabs));

    Panel::new()
        .background(fern_tokens::SurfaceRole::Main)
        .border_width(1.0)
        .border_color(fern_tokens::BorderRole::Default)
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
    let pick_button = Button::new_literal("Pick")
        .bind_label(picker_label_signal)
        .on_activate_fn(move |_ctx| {
            let next = !picker_state_for_click.get();
            picker_state_for_click.set(next);
        });

    // Bounds-overlay segmented control. `SegmentedControl` is driven
    // by a `Signal<usize>`. We bridge it to `OverlayMode` via two
    // observers (one each direction) so toggling either side syncs
    // the other. The observer handles are attached to the bridge
    // signal so they live as long as the toolbar.
    let bounds_index = Signal::new(overlay_mode_to_index(state.overlay_mode.get()));
    {
        let bounds_index_target = bounds_index.clone();
        let h = state.overlay_mode.observe(move |mode| {
            let new_idx = overlay_mode_to_index(*mode);
            if bounds_index_target.get() != new_idx {
                bounds_index_target.set(new_idx);
            }
        });
        bounds_index.attach_keepalive(h);
    }
    {
        let mode_target = state.overlay_mode.clone();
        let h = bounds_index.observe(move |idx| {
            let new_mode = index_to_overlay_mode(*idx);
            if mode_target.get() != new_mode {
                mode_target.set(new_mode);
            }
        });
        bounds_index.attach_keepalive(h);
    }
    let bounds_seg = SegmentedControl::new(
        vec!["Off".to_string(), "Sel".to_string(), "All".to_string()],
        bounds_index,
    );

    let opacity_slider = FixedSize::new()
        .bind_width(Signal::new(120.0_f32))
        .child(Slider::new(state.overlay_opacity.clone(), 0.1, 1.0));

    let open_state_for_close = state.open.clone();
    let close_button = Button::new_literal("×").on_activate_fn(move |_ctx| {
        open_state_for_close.set(false);
    });

    Padding::symmetric(4.0, 8.0).child(
        HStack::new()
            .spacing(6.0)
            .child(pick_button)
            .child(bounds_seg)
            .child(opacity_slider)
            .child(Expand::new().flex(1.0).child(empty_filler()))
            .child(close_button),
    )
}

fn overlay_mode_to_index(mode: OverlayMode) -> usize {
    match mode {
        OverlayMode::Off => 0,
        OverlayMode::SelectionOnly => 1,
        OverlayMode::AllBounds => 2,
    }
}

fn index_to_overlay_mode(idx: usize) -> OverlayMode {
    match idx {
        0 => OverlayMode::Off,
        1 => OverlayMode::SelectionOnly,
        _ => OverlayMode::AllBounds,
    }
}

/// Wrap a tab leaf widget in a `ScrollArea` so long content scrolls
/// instead of overflowing the panel.
fn scrollable_tab(content: impl Widget + 'static) -> impl Widget + 'static {
    fern_widgets::ScrollArea::new().child(content)
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
        let row = Button::new_literal("")
            .bind_label(label_signal)
            .on_activate_fn(move |c| {
                let chain = state_for_action.pending_pick_chain.get();
                if let Some(entry) = chain.get(i) {
                    state_for_action.selected_id.set(Some(entry.id));
                }
                state_for_action.pending_pick_chain.set(Vec::new());
                if state_for_action.picker_mode.get() {
                    state_for_action.picker_mode.set(false);
                }
                c.dismiss_all_overlays();
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
        .background(fern_tokens::SurfaceRole::Raised)
        .border_color(fern_tokens::BorderRole::Default)
        .border_width(1.0)
        .padding(4.0)
        .child(menu_vstack);
    ctx.add(panel)
}
