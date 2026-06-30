// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The panel / content layer of [`DockingLayout`](super::DockingLayout):
//! the app-facing [`DockWidget`] declaration, the content-factory registry,
//! and the widgets that render a side's tabs → Splitter/ToolBox arrangement →
//! draggable dock panels (with five-zone drop targets).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::WidgetBuilder;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_core::{DragPayload, DropFeedback};
use bastyde_i18n::{LocalizedString, lit};
use bastyde_tokens::{SurfaceRole, TextRole, TextStyleRole};

use crate::accordion::{
    ACCORDION_FILL_HEADER_EXTENT, ACCORDION_HEADER_PADDING_HORIZONTAL, Accordion,
    AccordionOrientation,
};
use crate::drop_target::DropTarget;
use crate::icon_button::{IconButton, IconButtonSize};
use crate::popover_widget::PopoverIconButton;
use crate::primitives::{
    Center, Divider, Expand, HStack, IconWidget, MinSize, Padding, RectWidget, Spacer, TextWidget,
    VStack, ZStack,
};
use crate::splitter::Splitter;
use bastyde_core::overlay::OverlayPlacement;

use super::context_menu::{
    DockMenuKind, activity_context_menu, background_menu, dock_has_options, dock_options_menu,
};
use super::drag::{
    DockDragData, DockDropOverlay, DropZone, compute_drop_zone, dropped_dock_tab,
    dropped_dock_widget,
};
use super::geometry::DockSide;
use super::model::{
    DockHeaderActionsFactory, DockIconFactory, DockOpenLocation, DockTabId, DockTabView,
    DockWidgetId, DockWidgetMeta, DockingModel, side_orientation,
};

/// Builds a dock widget's content on demand (keyed by its [`DockWidgetId`]).
pub type DockContentFactory = Rc<dyn Fn(DockWidgetId) -> Box<dyn Widget>>;

/// App-facing declaration of a dock widget: identity, chrome metadata, and a
/// lazy content factory. Collect these on [`DockingLayout::dock`](super::DockingLayout::dock).
pub struct DockWidget {
    id: DockWidgetId,
    title: LocalizedString,
    icon: Option<DockIconFactory>,
    default: DockOpenLocation,
    factory: DockContentFactory,
    header_actions: Option<DockHeaderActionsFactory>,
    show_header: bool,
}

impl DockWidget {
    /// Declare a dock widget. `factory` builds its content the first time the
    /// dock appears (and after it is closed and re-opened).
    pub fn new<W: Widget + 'static>(
        id: DockWidgetId,
        title: impl Into<LocalizedString>,
        factory: impl Fn(DockWidgetId) -> W + 'static,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            icon: None,
            default: DockOpenLocation::side(DockSide::Leading),
            factory: Rc::new(move |i| Box::new(factory(i)) as Box<dyn Widget>),
            header_actions: None,
            show_header: false,
        }
    }

    /// Set the dock's tab / rail icon.
    pub fn icon(mut self, f: impl Fn() -> IconWidget + 'static) -> Self {
        self.icon = Some(Rc::new(f));
        self
    }

    /// Attach a factory for the dock's **inline header actions** — a widget
    /// (typically an `HStack` of [`IconButton`]s) shown in the dock header
    /// before the `⋮` options button, the VS Code "view actions" pattern
    /// ("New File", "Collapse All", …). Built on demand each time the dock is
    /// placed into a header. The actions appear in any header the dock has: the
    /// multi-pane [`Accordion`] header always, and the sole-pane (bare) header
    /// when [`show_header(true)`](Self::show_header) is set.
    pub fn header_actions<W: Widget + 'static>(
        mut self,
        f: impl Fn(DockWidgetId) -> W + 'static,
    ) -> Self {
        self.header_actions = Some(Rc::new(move |i| Box::new(f(i)) as Box<dyn Widget>));
        self
    }

    /// Give a **sole-pane** (bare) dock its own header bar (title + actions +
    /// `⋮` options). Default `false`. The multi-pane Accordion header is always
    /// present regardless; this only governs the bare case. Turn it on to get a
    /// discoverable options button (and inline `header_actions`) on a dock that
    /// is the only one on its side.
    pub fn show_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }

    /// The location used when the dock is opened via `toggle` / `reveal`
    /// without an explicit target.
    pub fn default_location(mut self, loc: DockOpenLocation) -> Self {
        self.default = loc;
        self
    }

    pub(crate) fn id(&self) -> DockWidgetId {
        self.id
    }

    pub(crate) fn into_parts(self) -> (DockWidgetId, DockWidgetMeta, DockContentFactory) {
        (
            self.id,
            DockWidgetMeta {
                title: self.title,
                icon: self.icon,
                min_size: None,
                default: self.default,
                header_actions: self.header_actions,
                show_header: self.show_header,
            },
            self.factory,
        )
    }
}

/// Registry of content factories, owned by the layout, shared into the panel
/// widgets so closed-then-reopened docks rebuild fresh content.
#[derive(Default)]
pub(crate) struct DockContentRegistry {
    factories: HashMap<DockWidgetId, DockContentFactory>,
}

impl std::fmt::Debug for DockContentRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockContentRegistry")
            .field("factories", &self.factories.len())
            .finish()
    }
}

impl DockContentRegistry {
    pub(crate) fn insert(&mut self, id: DockWidgetId, factory: DockContentFactory) {
        self.factories.insert(id, factory);
    }
    pub(crate) fn build(&self, id: DockWidgetId) -> Option<Box<dyn Widget>> {
        self.factories.get(&id).map(|f| f(id))
    }
}

/// A shared handle to the content-factory registry, passed down so each dock
/// panel builds its content **in-context** (where it is placed), avoiding
/// cross-build-context parenting.
pub(crate) type DockContent = Rc<RefCell<DockContentRegistry>>;

/// Kind tag for a side's dynamic dock tabs (so `dynamic_tab` registers them).
const DOCK_TAB_KIND: &str = "__dock_tab__";

/// The dynamic-tab payload carried by a side's `TabWidget` — identifies the
/// DockTab so cross-side whole-tab drag (`accept_external_tabs`) can relocate
/// it via [`DockingModel::move_tab`].
#[derive(Clone, Copy)]
struct DockTabPayload {
    tab_id: DockTabId,
}

// ───────────────────────────────────────────────────────────────────────
// DockSidePanel — a side's content: optional in-side tab strip + Switcher.
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct DockSidePanel {
    side: DockSide,
    model: DockingModel,
    content: DockContent,
    root: Option<WidgetId>,
}

impl DockSidePanel {
    pub(crate) fn new(side: DockSide, model: DockingModel, content: DockContent) -> Self {
        Self {
            side,
            model,
            content,
            root: None,
        }
    }
}

/// The drop target shown when a side has **no** docks, so a revealed-but-empty
/// side (opened from a toolbar button, the rail, or a drag-reveal strip) still
/// accepts content. Accepts a whole tab (`DockTabDragData` → `move_tab`) or a
/// single dock (`DockDragData` → `move_dock`); both reveal the side.
fn empty_side_drop_target(
    ctx: &mut BuildContext,
    model: &DockingModel,
    side: DockSide,
) -> WidgetId {
    let text = ctx.add(
        TextWidget::new(lit!("Drop a panel here"))
            .style(TextStyleRole::Body)
            .color(TextRole::Secondary),
    );
    let label = ctx.add(Center::new().child_id(text));
    let m = model.clone();
    ctx.add(
        DropTarget::new()
            .child_id(label)
            .accept_when(|p| dropped_dock_tab(p).is_some() || dropped_dock_widget(p).is_some())
            .on_drop(move |p, _pos, ctx| {
                if !m.is_side_enabled(side) {
                    return false;
                }
                if let Some(tab_id) = dropped_dock_tab(&p) {
                    m.move_tab(tab_id, side, 0);
                    m.set_side_visible(side, true);
                    ctx.request_accessibility_update();
                    true
                } else if let Some(dock_id) = dropped_dock_widget(&p) {
                    m.move_dock(dock_id, DockOpenLocation::side(side));
                    m.set_side_visible(side, true);
                    ctx.request_accessibility_update();
                    true
                } else {
                    false
                }
            }),
    )
}

impl Widget for DockSidePanel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::tab_widget::{
            TabBarVisibility, TabDisplayMode, TabHandle, TabId, TabInfo, TabWidget,
        };
        use bastyde_data::ListModel;
        use std::any::Any;
        use std::num::NonZeroU64;

        let all_tabs = self.model.side_tabs(self.side);
        if all_tabs.is_empty() {
            // A side with no docks. When it's visible (revealed from a button,
            // the rail, or a drag-reveal strip) it shows a drop target so the
            // first dock can be dragged in; when hidden it's dormant anyway.
            let drop = empty_side_drop_target(ctx, &self.model, self.side);
            self.root = Some(drop);
            return vec![drop];
        }

        // Rebuild the strip when this side's tab-display pref flips (context
        // menu "Tab size"). The bar then re-derives its headers in the chosen
        // mode (a scoped, content-preserving rebuild).
        let self_id = ctx.self_id();
        self.model.tab_display_signal(self.side).bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );
        let display = match self.model.side_tab_display(self.side) {
            super::model::DockTabDisplay::Icon => TabDisplayMode::Icon,
            super::model::DockTabDisplay::IconText => TabDisplayMode::IconText,
            super::model::DockTabDisplay::Text => TabDisplayMode::Text,
        };

        // Stable TabWidget id per dock tab (dock tab ids start at 1).
        let to_tab_id = |t: &DockTabView| {
            TabId::from_raw(NonZeroU64::new(t.id.raw()).unwrap_or(NonZeroU64::MIN))
        };
        // model-index → TabId for the whole side (selection maps through it).
        let all_tab_ids: Vec<TabId> = all_tabs.iter().map(&to_tab_id).collect();

        // Only non-hidden tabs render in the strip; remember each shown tab's
        // model index (visible-position → model-index) for selection + drop
        // routing.
        let model_indices: Vec<usize> = all_tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.hidden)
            .map(|(i, _)| i)
            .collect();
        let presentation = self.model.side_presentation(self.side);
        if model_indices.is_empty() && presentation == super::model::TabPresentation::Rail {
            // Every activity hidden in Rail presentation: the activity rail (with
            // its own background menu) is the restore affordance — blank content.
            let empty = ctx.add(RectWidget::new().background(SurfaceRole::Transparent));
            self.root = Some(empty);
            return vec![empty];
        }
        // In Strip presentation we still build the bar below — even with zero
        // visible tabs — so its trailing "hidden activities" hamburger can
        // restore them (right-clicking a tab is impossible when none show).

        let dock_selected = self.model.side_selected_tab_signal(self.side);
        let initial = all_tab_ids
            .get(dock_selected.get().min(all_tab_ids.len().saturating_sub(1)))
            .copied();
        let tw_selected: Signal<Option<TabId>> = ctx.signal(initial);

        // model → TabWidget: map the selected model index to its TabId,
        // resolved against the **live** model (not the build-time `all_tab_ids`
        // snapshot). This is the exact inverse of effect 2's live id → index
        // lookup, so the round-trip is the identity and the equality guards
        // stop the chain at once. A stale snapshot here would disagree with
        // effect 2 after a reorder (idx 1 → snapshot id B, id B → live idx 2,
        // idx 2 → snapshot id A, …) and feed back unboundedly — the
        // "Signal notification nested 257 deep" panic when an activity is
        // imported onto a side and then reordered within it.
        {
            let model = self.model.clone();
            let side = self.side;
            let tw = tw_selected.clone();
            ctx.effect(&dock_selected, move |&idx| {
                let target = model.tab_id_at(side, idx).map(|id| {
                    TabId::from_raw(NonZeroU64::new(id.raw()).unwrap_or(NonZeroU64::MIN))
                });
                if tw.get() != target {
                    tw.set(target);
                }
            });
        }
        // TabWidget → model (an in-strip click) — position-independent so a
        // hidden tab in the middle doesn't shift the mapping.
        {
            let model = self.model.clone();
            let side = self.side;
            ctx.effect(&tw_selected, move |maybe| {
                if let Some(tid) = maybe {
                    model.select_tab_by_id(side, DockTabId::from_raw(tid.raw().get()));
                }
            });
        }

        // Rail presentation → the in-side strip is hidden (the activity rail is
        // the selector). Strip → always show the real TabWidget bar (so even a
        // single-panel side reads as a TabWidget tab, not a custom title bar).
        let bar_visibility = match presentation {
            super::model::TabPresentation::Rail => TabBarVisibility::Never,
            super::model::TabPresentation::Strip => TabBarVisibility::Always,
        };
        // No visible tab → no tab to right-click, so the bar needs a trailing
        // hamburger to reach the activities menu. When at least one tab shows,
        // its own right-click menu already lists (and restores) the hidden ones.
        let needs_hamburger = model_indices.is_empty();

        // Build the visible tabs as a dynamic `ListModel<TabHandle>` so a whole
        // tab can be dragged between sides via TabWidget's `accept_external_tabs`.
        // Tabs are not closable (you hide the side / move the dock, you don't
        // close a view container from its tab). Each tab carries a context menu
        // and renders per the side's tab-display mode.
        let mut handles: Vec<TabHandle> = Vec::with_capacity(model_indices.len());
        for &model_i in &model_indices {
            let tab = &all_tabs[model_i];
            // Label / icon: explicit activity title (set_tab_title) → primary
            // (first non-collapsed) pane's dock → "Panel" / no-icon.
            let label = self.model.activity_label(tab);
            let icon_factory = self.model.activity_icon(tab);

            // Each tab declares its title + icon; the bar's reactive
            // `tab_display` (wired below from the side's "Tab size" pref) decides
            // what's painted — icon, text, or both — and handles the icon-only
            // sizing, tooltip promotion, and icon-less initial-letter fallback.
            let mut info = TabInfo::new().closable(false).title(label.clone());
            if let Some(icf) = icon_factory {
                info = info.icon(move || (icf)());
            }
            {
                let m = self.model.clone();
                let menu_side = self.side;
                let tid = tab.id;
                info = info.context_menu(move |_pos, _ctx| {
                    Some(Box::new(activity_context_menu(
                        &m,
                        menu_side,
                        tid,
                        DockMenuKind::Strip,
                    )))
                });
            }
            handles.push(TabHandle::dynamic(
                to_tab_id(tab),
                DOCK_TAB_KIND,
                info,
                DockTabPayload { tab_id: tab.id },
            ));
        }
        let list: ListModel<TabHandle> = ListModel::from_vec(handles);

        let side = self.side;
        let factory_model = self.model.clone();
        let factory_content = self.content.clone();
        // The bar deals in *visible* positions; translate them back to model
        // tab indices (a no-op when nothing is hidden) for `move_tab`.
        let ext_indices = model_indices.clone();
        let ext_model = self.model.clone();
        // Appending past the last visible tab must land just **after the last
        // visible tab's model index**, not at the absolute end — otherwise a
        // dropped/promoted tab is ordered after any trailing *hidden* tabs and
        // reappears out of place when they are restored.
        let after_last_visible = model_indices
            .last()
            .map(|&i| i + 1)
            .unwrap_or(all_tab_ids.len());

        let policy = self.model.policy();
        let mut tw = TabWidget::new(tw_selected)
            .bar_visibility(bar_visibility)
            // Dock side strips use the denser compact (38 dp) tab bar, each tab
            // sized to its own content (not a shared width) — and a compact min
            // so an icon-only tab shrinks to its icon and an icon + text tab
            // grows to fit both, instead of all clamping to the editor-tab min.
            .compact_bar()
            .tab_sizing(crate::tab_widget::TabSizing::Independent)
            .tab_display(display)
            .min_tab_width(40.0)
            .dynamic_model(list)
            .dynamic_tab::<DockTabPayload>(DOCK_TAB_KIND, move |_handle, payload| {
                match factory_model.tab_view_by_id(payload.tab_id) {
                    Some((tside, view)) => Box::new(DockTabContentWidget::new(
                        tside,
                        view,
                        factory_model.clone(),
                        factory_content.clone(),
                    )) as Box<dyn Widget>,
                    None => Box::new(RectWidget::new().background(SurfaceRole::Transparent)),
                }
            })
            // A drop from a source that ISN'T a peer `TabBar<TabHandle>` — an
            // **activity-rail item** (`DockTabDragData`) or a single dock (a
            // split-pane header, `DockDragData`). The native `on_tab_received`
            // path only fires for `TabBarDragData<TabHandle>`; without this the
            // bar would be the drop target (`find_drop_target_at_or_above` stops
            // at the first handler) and silently reject the rail drag. `idx` is
            // this bar's visible insertion position → model tab index. (Kept
            // unconditionally — when a lock is on, the gated source simply never
            // produces the matching payload, so the branch is inert.)
            .on_external_drop(move |payload, idx, ctx| {
                // A disabled side never mutates from a UI drop (its panel isn't
                // even built — this keeps that a local invariant rather than
                // consuming the drop while the model silently rejects it).
                if !ext_model.is_side_enabled(side) {
                    return false;
                }
                let at = ext_indices.get(idx).copied().unwrap_or(after_last_visible);
                if let Some(tab_id) = dropped_dock_tab(payload) {
                    ext_model.move_tab(tab_id, side, at);
                    ctx.request_accessibility_update();
                    true
                } else if let Some(dock_id) = dropped_dock_widget(payload) {
                    // A lone dock becomes a new activity at the drop position.
                    ext_model.promote_to_tab(dock_id, side, at);
                    ctx.request_accessibility_update();
                    true
                } else {
                    false
                }
            });
        // Activity drag-and-drop (reorder within a side + transfer between
        // sides) is a user affordance — gate it on the policy. When off, the
        // tab headers are neither drag sources nor reorder/transfer targets.
        if policy.allow_activity_drag {
            let reorder_model = self.model.clone();
            let recv_model = self.model.clone();
            let reorder_indices = model_indices.clone();
            let recv_indices = model_indices.clone();
            tw = tw
                .reorderable(true)
                .accept_external_tabs(true)
                // Same-side reorder.
                .on_reorder(move |tid, dest, _ctx| {
                    let at = reorder_indices
                        .get(dest)
                        .copied()
                        .unwrap_or(after_last_visible);
                    reorder_model.move_tab(DockTabId::from_raw(tid.raw().get()), side, at);
                })
                // Cross-side drop: relocate the whole tab to this side.
                .on_tab_received(move |handle, idx, ctx| {
                    if let Some(p) =
                        (handle.payload.as_ref() as &dyn Any).downcast_ref::<DockTabPayload>()
                    {
                        let at = recv_indices.get(idx).copied().unwrap_or(after_last_visible);
                        recv_model.move_tab(p.tab_id, side, at);
                        ctx.request_accessibility_update();
                    }
                })
                // The source side: `move_tab` (above) already removed the tab
                // from the model; the rebuild reconciles this side's list.
                .on_transfer_out(|_tid, _ctx| {});
        }

        // When activities are hidden, a trailing **hamburger** in the bar opens
        // the activities checklist (the restore affordance — and the only one
        // left once *every* activity is hidden and no tab can be right-clicked).
        if needs_hamburger {
            let m = self.model.clone();
            let hb_side = self.side;
            tw = tw.bar_trailing_slot(
                PopoverIconButton::new(IconButton::menu().tooltip(lit!("Hidden activities")))
                    .content(background_menu(&m, hb_side, DockMenuKind::Strip))
                    .placement(OverlayPlacement::BelowPreferred),
            );
        }
        let root = ctx.add(tw);
        self.root = Some(root);

        // Side-level drop target for a whole-tab drag (an activity-rail button
        // or a tab header from another side). A drop landing on a *pane* is
        // consumed by that `DockPanePane` (split / stack); a drop landing on
        // the **tab bar** (or any non-pane chrome) bubbles up to here and
        // relocates the tab to the end of this side.
        let drop_model = self.model.clone();
        let drop_side = self.side;
        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_drag_hover(move |_payload, _pos, _ctx| {
                    // Accept silently; the drop is routed in `on_drop`. (A pane
                    // under the pointer paints its own five-zone overlay; the
                    // bar just needs to register as a valid target.)
                    DropFeedback::NoFeedback
                })
                .on_drop(move |payload, _pos, ctx| {
                    if !drop_model.is_side_enabled(drop_side) {
                        return false;
                    }
                    // A drop landing on non-pane chrome (the strip, gaps): a tab
                    // relocates to this side; a single dock joins it too.
                    if let Some(tab_id) = dropped_dock_tab(&payload) {
                        let at = drop_model.side_append_index(drop_side);
                        drop_model.move_tab(tab_id, drop_side, at);
                        ctx.request_accessibility_update();
                        true
                    } else if let Some(dock_id) = dropped_dock_widget(&payload) {
                        drop_model.move_dock(dock_id, DockOpenLocation::side(drop_side));
                        ctx.request_accessibility_update();
                        true
                    } else {
                        false
                    }
                }),
        );
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
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
        use bastyde_core::accesskit::Role;
        builder.set_role(Role::Complementary);
        builder.set_name(super::a11y::side_label(self.side).resolve_now());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

// ───────────────────────────────────────────────────────────────────────
// DockTabContentWidget — one tab's Splitter of panes.
// ───────────────────────────────────────────────────────────────────────

struct DockTabContentWidget {
    side: DockSide,
    tab: DockTabView,
    model: DockingModel,
    content: DockContent,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for DockTabContentWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockTabContentWidget")
            .field("side", &self.side)
            .field("panes", &self.tab.panes.len())
            .finish()
    }
}

impl DockTabContentWidget {
    fn new(side: DockSide, tab: DockTabView, model: DockingModel, content: DockContent) -> Self {
        Self {
            side,
            tab,
            model,
            content,
            root: None,
        }
    }

    /// Build a dock's content widget in-context via the registry.
    fn build_dock_content(&self, ctx: &mut BuildContext, dock: DockWidgetId) -> WidgetId {
        match self.content.borrow().build(dock) {
            Some(w) => ctx.add_boxed(w),
            None => ctx.add(TextWidget::new(lit!("(missing content)"))),
        }
    }
}

impl Widget for DockTabContentWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Find this tab's index in the side for drop-routing.
        let tab_idx = self
            .model
            .side_tabs(self.side)
            .iter()
            .position(|t| t.id == self.tab.id)
            .unwrap_or(0);

        let root = if self.tab.panes.len() <= 1 {
            // Single pane: render the dock bare (a 1-pane Splitter is
            // degenerate). The side's tab / rail is its header.
            match self.tab.panes.first() {
                Some(dock) => {
                    let inner = self.build_pane_inner(ctx, *dock, 0, None);
                    ctx.add(DockPanePane::new(
                        self.side,
                        tab_idx,
                        0,
                        self.model.clone(),
                        inner,
                    ))
                }
                None => ctx.add(RectWidget::new().background(SurfaceRole::Transparent)),
            }
        } else {
            // Split panes: each dock is its own Accordion, separated by the
            // Splitter. Collapsing an accordion collapses its Splitter pane.
            let splitter_model = self.tab.splitter.clone();
            let mut splitter = Splitter::new(splitter_model.clone());
            for (pane_idx, dock) in self.tab.panes.iter().enumerate() {
                let inner = self.build_pane_inner(ctx, *dock, pane_idx, Some(&splitter_model));
                let pane_widget = ctx.add(DockPanePane::new(
                    self.side,
                    tab_idx,
                    pane_idx,
                    self.model.clone(),
                    inner,
                ));
                splitter = splitter.pane_id(pane_widget);
            }
            ctx.add(splitter)
        };
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
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

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

impl DockTabContentWidget {
    /// Render one pane = one dock.
    ///
    /// A **sole** pane (`splitter == None`) is rendered bare — the side's tab /
    /// rail is already its header. A **split** pane is wrapped in an
    /// [`Accordion`] whose draggable header titles the dock, is the drag handle,
    /// and collapses the dock on click. The accordion fills the pane (`fill`);
    /// toggling it **collapses its Splitter pane** to just the header (siblings
    /// grow), and re-expands it to the same size — wired here via the pane's
    /// `expanded` signal driving `SplitterModel::set_collapsed`.
    fn build_pane_inner(
        &self,
        ctx: &mut BuildContext,
        dock: DockWidgetId,
        pane_idx: usize,
        splitter: Option<&crate::splitter::SplitterModel>,
    ) -> WidgetId {
        let content = self.build_dock_content(ctx, dock);
        let multi_pane = splitter.is_some();
        let Some(splitter) = splitter else {
            // Sole-pane (bare) dock. By default it renders headerless (the side
            // tab / rail is its header). Opting in (`DockWidget::show_header`)
            // gives it a VS Code–style header bar carrying its own actions + the
            // `⋮` options menu.
            if !self.model.dock_show_header(dock) {
                return content;
            }
            return self.build_bare_dock_header(ctx, dock, content);
        };
        let title = self.model.dock_title(dock).unwrap_or_else(|| lit!("Panel"));
        // Initial expanded state follows the Splitter (so a rebuild preserves a
        // collapsed pane); toggling drives the pane collapse/expand.
        let expanded = ctx.signal(!splitter.is_collapsed(pane_idx));
        splitter.set_collapsed_size(pane_idx, crate::accordion::ACCORDION_FILL_COLLAPSED_EXTENT);
        {
            let sp = splitter.clone();
            ctx.effect(&expanded, move |&e| {
                sp.set_collapsed(pane_idx, !e);
            });
        }
        let mut accordion = Accordion::new(title, expanded)
            .orientation(
                if side_orientation(self.side) == bastyde_tokens::Orientation::Vertical {
                    AccordionOrientation::Vertical
                } else {
                    AccordionOrientation::Horizontal
                },
            )
            .fill(true);
        // The dock's header actions (app-supplied) + the framework `⋮` options
        // menu sit in the accordion header's trailing slot.
        if let Some(trailing) = self.dock_header_trailing(ctx, dock, multi_pane) {
            accordion = accordion.trailing_id(trailing);
        }
        // The accordion header is the dock's drag handle — only when the policy
        // allows dragging a single dock out of a split pane.
        if self.model.policy().allow_dock_drag {
            accordion = accordion.on_header_drag(move |ctx| {
                ctx.start_drag(content, DragPayload::typed(DockDragData { dock_id: dock }));
            });
        }
        ctx.add(accordion.content_id(content))
    }

    /// Build the trailing cluster of a dock header — the app's inline
    /// `header_actions` (if any) followed by the framework `⋮` options button
    /// ([`dock_options_menu`]). Returns `None` when there is nothing to show
    /// (no app actions and an empty options menu).
    fn dock_header_trailing(
        &self,
        ctx: &mut BuildContext,
        dock: DockWidgetId,
        multi_pane: bool,
    ) -> Option<WidgetId> {
        let actions = self.model.dock_header_actions(dock);
        let has_options = dock_has_options(&self.model, self.side, multi_pane);
        if actions.is_none() && !has_options {
            return None;
        }
        let mut kids: Vec<WidgetId> = Vec::new();
        if let Some(factory) = actions {
            kids.push(ctx.add_boxed(factory(dock)));
        }
        if has_options {
            let title = self.model.dock_title(dock).unwrap_or_else(|| lit!("Panel"));
            let menu = dock_options_menu(&self.model, self.side, self.tab.id, dock, multi_pane);
            let options = PopoverIconButton::new(IconButton::more().size(IconButtonSize::Compact))
                // `.bare()` so the `MenuList` is the popover content directly,
                // not wrapped in a second popover surface (a menu-on-a-popover).
                .bare()
                .content(menu)
                .placement(OverlayPlacement::BelowPreferred)
                .access_label(lit!(format!("More actions: {}", title.resolve_now())));
            kids.push(ctx.add(options));
        }
        // Top / bottom sides render the accordion header as a rotated *vertical*
        // strip (`AccordionOrientation::Horizontal`), so the action cluster must
        // stack vertically there; leading / trailing sides keep the horizontal
        // header row and an `HStack`.
        let vertical_header =
            side_orientation(self.side) == bastyde_tokens::Orientation::Horizontal;
        let cluster = if vertical_header {
            let mut col = VStack::new().spacing(2.0);
            for k in kids {
                col = col.add_child(k);
            }
            ctx.add(col)
        } else {
            let mut row = HStack::new().spacing(2.0);
            for k in kids {
                row = row.add_child(k);
            }
            ctx.add(row)
        };
        Some(cluster)
    }

    /// The sole-pane dock header bar (opt-in via `DockWidget::show_header`):
    /// `[title] [Spacer] [actions + ⋮]` above the content, matching the VS Code
    /// view-header layout. Always a horizontal bar regardless of side.
    fn build_bare_dock_header(
        &self,
        ctx: &mut BuildContext,
        dock: DockWidgetId,
        content: WidgetId,
    ) -> WidgetId {
        let title = self.model.dock_title(dock).unwrap_or_else(|| lit!("Panel"));
        let title_id = ctx.add(
            TextWidget::new(title)
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary)
                .single_line(),
        );
        let spacer_id = ctx.add(Spacer::new());
        let mut row = HStack::new()
            .spacing(2.0)
            .add_child(title_id)
            .add_child(spacer_id);
        if let Some(trailing) = self.dock_header_trailing(ctx, dock, false) {
            row = row.add_child(trailing);
        }
        let row_id = ctx.add(row);
        let padded =
            ctx.add(Padding::symmetric(2.0, ACCORDION_HEADER_PADDING_HORIZONTAL).child_id(row_id));
        // Fixed-height header bar (matching the Accordion header extent) with a
        // 1 dp divider beneath it, above the content.
        let header = ctx.add(MinSize::new(0.0, ACCORDION_FILL_HEADER_EXTENT).child_id(padded));
        let divider = ctx.add(Divider::horizontal());
        ctx.add(
            VStack::new()
                .add_child(header)
                .add_child(divider)
                .child(Expand::new().flex(1.0).child_id(content)),
        )
    }
}

// ───────────────────────────────────────────────────────────────────────
// DockPanePane — a Splitter pane that is a five-zone drop target.
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct DockPanePane {
    side: DockSide,
    tab_idx: usize,
    pane_idx: usize,
    model: DockingModel,
    inner: WidgetId,
    zone: Signal<Option<DropZone>>,
    self_size: Rc<Cell<Size>>,
    root: Option<WidgetId>,
}

impl DockPanePane {
    fn new(
        side: DockSide,
        tab_idx: usize,
        pane_idx: usize,
        model: DockingModel,
        inner: WidgetId,
    ) -> Self {
        Self {
            side,
            tab_idx,
            pane_idx,
            model,
            inner,
            zone: Signal::new(None),
            self_size: Rc::new(Cell::new(Size::ZERO)),
            root: None,
        }
    }
}

impl Widget for DockPanePane {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let overlay = ctx.add(DockDropOverlay::new(self.zone.clone()));
        let root = ctx.add(ZStack::new().add_child(self.inner).add_child(overlay));
        self.root = Some(root);

        let side = self.side;
        let tab_idx = self.tab_idx;
        let pane_idx = self.pane_idx;
        let model = self.model.clone();
        let zone_hover = self.zone.clone();
        let zone_leave = self.zone.clone();
        let zone_drop = self.zone.clone();
        let size_hover = self.self_size.clone();
        let size_drop = self.self_size.clone();

        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_drag_hover(move |payload, pos, _ctx| {
                    // Only a single DockWidget splits/stacks a pane (the
                    // five-zone overlay: centre = stack, edge fifths = split). A
                    // whole tab always relocates to the side regardless of where
                    // it lands, so it shows no per-zone overlay.
                    if dropped_dock_widget(payload).is_some() {
                        let z = compute_drop_zone(pos, size_hover.get());
                        zone_hover.set(Some(z));
                    }
                    DropFeedback::NoFeedback
                })
                .on_drag_leave(move |_ctx| zone_leave.set(None))
                .on_drop(move |payload, pos, ctx| {
                    if let Some(dock) = dropped_dock_widget(&payload) {
                        let z = compute_drop_zone(pos, size_drop.get());
                        match z {
                            // Centre = join this tab as another Splitter pane;
                            // an edge = split before / after the target pane.
                            DropZone::Center => model.stack_into_tab(dock, side, tab_idx),
                            DropZone::SplitLeading | DropZone::SplitTop => {
                                model.split_into_tab(dock, side, tab_idx, pane_idx, true)
                            }
                            DropZone::SplitTrailing | DropZone::SplitBottom => {
                                model.split_into_tab(dock, side, tab_idx, pane_idx, false)
                            }
                        }
                        zone_drop.set(None);
                        ctx.request_accessibility_update();
                        true
                    } else if let Some(tab_id) = dropped_dock_tab(&payload) {
                        // A whole tab always relocates to this side (it never
                        // splits a pane — only a single DockWidget does). Append
                        // after the last *visible* tab (not past trailing hidden
                        // ones).
                        let at = model.side_append_index(side);
                        model.move_tab(tab_id, side, at);
                        zone_drop.set(None);
                        ctx.request_accessibility_update();
                        true
                    } else {
                        false
                    }
                }),
        );
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        ctx.child_size(self.inner, proposal)
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
        self.self_size.set(bounds.size());
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}
