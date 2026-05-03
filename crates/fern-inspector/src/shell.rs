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
use fern_widgets::{Button, Panel, TabWidget};

use crate::highlight::{BoundsTracker, HighlightLayer};
use crate::picker::{PickResolver, PickerOverlay};
use crate::state::InspectorState;
use crate::tabs::accessibility::A11yTab;
use crate::tabs::properties::PropertiesTab;
use crate::tabs::tree::TreeTab;

const PANEL_HEIGHT: f32 = 280.0;

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

        // Picker overlay — only when picker_mode is true. Modeled via
        // a Switcher (0 = empty, 1 = picker overlay).
        let picker_index = state.picker_mode.map(|active| if *active { 1usize } else { 0 });
        let picker_switcher = fern_widgets::primitives::Switcher::new(picker_index)
            .child(empty_filler())
            .child(PickerOverlay::new(state.clone()));

        let z = ZStack::new()
            .add_child(self.user_root_id)
            .child(highlight)
            .child(picker_switcher);

        // Slot for the inspector panel. Visibility is driven by
        // `state.open` via a Switcher: index 0 hides, index 1 shows.
        let panel_index = state.open.map(|open| if *open { 1usize } else { 0 });
        let panel_switcher = fern_widgets::primitives::Switcher::new(panel_index)
            .child(empty_filler())
            .child(build_panel(state.clone()));

        let stack = VStack::new()
            .child(Expand::new().flex(1.0).child(z))
            .child(bounds_tracker)
            .child(pick_resolver)
            .child(FixedSize::new().bind_height(panel_height_signal(&state)).child(panel_switcher));

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

/// Derives a height signal: 0 when closed, `PANEL_HEIGHT` when open.
/// Used to drive `FixedSize::bind_height`. (No animation in slice 2 —
/// hard show/hide.)
fn panel_height_signal(state: &InspectorState) -> Signal<f32> {
    state
        .open
        .map(|open| if *open { PANEL_HEIGHT } else { 0.0 })
}

/// Zero-size placeholder used in Switchers when we want "nothing here".
fn empty_filler() -> impl Widget + 'static {
    FixedSize::new().bind_width(Signal::new(0.0_f32)).bind_height(Signal::new(0.0_f32))
}

/// Build the inspector panel's content. Toolbar (Pick / Close) above
/// a `TabWidget` with three tabs, all inside a `Panel`.
fn build_panel(state: InspectorState) -> impl Widget + 'static {
    let active_tab = Signal::new(0_usize);
    let tabs = TabWidget::new(active_tab)
        .tab_literal("Tree", scrollable_tab(TreeTab::new(state.clone())))
        .tab_literal("Properties", scrollable_tab(PropertiesTab::new(state.clone())))
        .tab_literal("Accessibility", scrollable_tab(A11yTab::new(state.clone())));

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

/// Toolbar above the tabs: Pick toggle on the leading side, Close (×)
/// on the trailing side.
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

    let open_state_for_close = state.open.clone();
    let close_button = Button::new_literal("×").on_activate_fn(move |_ctx| {
        open_state_for_close.set(false);
    });

    Padding::symmetric(4.0, 8.0).child(
        HStack::new()
            .spacing(6.0)
            .child(pick_button)
            .child(Expand::new().flex(1.0).child(empty_filler()))
            .child(close_button),
    )
}

/// Wrap a tab leaf widget in a `ScrollArea` so long content scrolls
/// instead of overflowing the panel.
fn scrollable_tab(content: impl Widget + 'static) -> impl Widget + 'static {
    fern_widgets::ScrollArea::new().child(content)
}
