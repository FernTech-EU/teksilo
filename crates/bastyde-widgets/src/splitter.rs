// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! N-pane split container with draggable, collapsible dividers.
//!
//! `Splitter` arranges `N ≥ 2` panes along one axis (per [`Orientation`])
//! with `N − 1` grabbable handles between them — the Qt `QSplitter`
//! model. All layout state (per-pane size / min / max / stretch /
//! collapsed) lives in a shared, cloneable [`SplitterModel`]; the app
//! holds a clone to read, mutate, persist, and import/export, while the
//! widget renders it and reacts to the model's `version` signal.
//!
//! Strengths carried over from the old two-pane `SplitView`: anti-jump
//! drag, keyboard resize, `Role::Splitter` accessibility, per-pane content
//! clipping, RTL-correct horizontal layout. New: N panes, per-pane
//! stretch (container-resize policy), animated collapse with four triggers
//! (programmatic / double-click / drag-past-min snap / keyboard), a Tier-3
//! [`SplitterStyle`], and serializable import/export. Intended as the
//! building block for a future `DockingLayout`.
//!
//! ```ignore
//! let model = SplitterModel::from_panes(vec![
//!     PaneDescriptor::new().size(220.0).min_size(160.0).stretch(0.0).collapsible(true),
//!     PaneDescriptor::new().stretch(1.0).min_size(320.0),
//!     PaneDescriptor::new().size(280.0).stretch(0.0).collapsible(true),
//! ], Orientation::Horizontal);
//!
//! Splitter::new(model.clone())
//!     .pane(sidebar).pane(editor).pane(inspector)
//!     .pane_label(0, tr!(sidebar()));
//! ```

mod distribute;
mod handle;
mod model;
#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::styles::{SharedSplitterStyle, SplitterStyle};
use bastyde_core::widget::{LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Orientation;

use self::distribute::distribute;
use self::handle::{SplitterHandle, SplitterHandleConfig};

pub use self::model::{
    PaneDescriptor, PaneSnapshot, PaneState, SPLITTER_GUTTER_THICKNESS, SPLITTER_KEYBOARD_STEP,
    SPLITTER_MIN_PANE_SIZE, SPLITTER_SNAP_OFFSET, SplitterModel, SplitterState,
};

/// Below this collapse progress, a pane's content is parked dormant
/// (and its region hidden from the a11y tree). Matches `Collapse`'s
/// near-zero epsilon so the content stays live across the shrink
/// animation and only drops out once it's effectively gone.
const COLLAPSED_VISIBLE_EPSILON: f32 = 0.01;

/// An N-pane resizable split container driven by a [`SplitterModel`].
///
/// See the [module-level documentation](self) for a usage overview and
/// constructor patterns.
pub struct Splitter {
    model: SplitterModel,
    enabled: bool,
    style_override: Option<SharedSplitterStyle>,
    /// One content slot per pane (in model order), consumed on first build.
    pane_content: Vec<Option<PendingChild>>,
    /// Optional accessible label per pane (locale-reactive).
    pane_labels: Vec<Option<Prop<String>>>,
    // ---- build-time state ----
    pane_clip_ids: Vec<WidgetId>,
    /// The user content widget inside each clip pane (in model order).
    /// `visible_when`-gated on the collapse progress so collapsed content
    /// goes dormant.
    pane_inner_ids: Vec<Option<WidgetId>>,
    /// Per-pane *full* (uncollapsed) main-axis size, written each layout
    /// pass and read by the clip so content lays out at full size and is
    /// clipped (not reflowed) as the pane collapses.
    pane_full_main: Vec<Rc<Cell<f32>>>,
    handle_ids: Vec<WidgetId>,
    progress: Vec<Signal<f32>>,
    prev_collapsed: Rc<RefCell<Vec<bool>>>,
    /// Per-pane visibility tween (1 = visible, 0 = hidden). A hidden pane and
    /// an adjacent gutter shrink to zero; content goes dormant.
    visible_progress: Vec<Signal<f32>>,
    prev_visible: Rc<RefCell<Vec<bool>>>,
    /// Container bounds, shared with handles for event-time coordinate math.
    container_bounds: Rc<Cell<Rect>>,
    /// Layout direction, shared with handles (RTL flips the horizontal axis).
    is_rtl: Rc<Cell<bool>>,
    /// Effective per-pane main-axis sizes from the latest `distribute()`,
    /// shared with handles so a drag can map pointer → boundary.
    layout_sizes: Rc<RefCell<Vec<f32>>>,
    /// Effective per-gap gutter widths (0 when a neighbor is hidden), shared
    /// with handles so the drag math uses the real positions.
    layout_gutters: Rc<RefCell<Vec<f32>>>,
}

impl Splitter {
    /// Create a `Splitter` bound to the given model. Panes must be appended
    /// with [`pane`](Self::pane) in model order.
    pub fn new(model: SplitterModel) -> Self {
        Self {
            model,
            enabled: true,
            style_override: None,
            pane_content: Vec::new(),
            pane_labels: Vec::new(),
            pane_clip_ids: Vec::new(),
            pane_inner_ids: Vec::new(),
            pane_full_main: Vec::new(),
            handle_ids: Vec::new(),
            progress: Vec::new(),
            prev_collapsed: Rc::new(RefCell::new(Vec::new())),
            visible_progress: Vec::new(),
            prev_visible: Rc::new(RefCell::new(Vec::new())),
            container_bounds: Rc::new(Cell::new(Rect::ZERO)),
            is_rtl: Rc::new(Cell::new(false)),
            layout_sizes: Rc::new(RefCell::new(Vec::new())),
            layout_gutters: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Append a content pane (model order). Call once per pane; the count
    /// must match `model.pane_count()`.
    pub fn pane(mut self, widget: impl Widget + 'static) -> Self {
        self.pane_content
            .push(Some(PendingChild::Deferred(Box::new(widget))));
        self
    }

    /// Append a pre-registered content pane by id.
    pub fn pane_id(mut self, id: WidgetId) -> Self {
        self.pane_content.push(Some(PendingChild::Id(id)));
        self
    }

    /// `bati!` ergonomic alias for [`pane`](Self::pane): a bare child in a
    /// `Splitter { ... }` block lowers to `.child(...)`.
    pub fn child(self, widget: impl Widget + 'static) -> Self {
        self.pane(widget)
    }

    /// Set an accessible region name for pane `index` (locale-reactive).
    /// Labeled panes become a named `Role::Group`; unlabeled panes stay
    /// AT-transparent (their content represents itself).
    pub fn pane_label(mut self, index: usize, label: impl Into<Prop<String>>) -> Self {
        if self.pane_labels.len() <= index {
            self.pane_labels.resize_with(index + 1, || None);
        }
        self.pane_labels[index] = Some(label.into());
        self
    }

    /// Override the active [`SplitterStyle`] for this instance only.
    pub fn style(mut self, style: impl SplitterStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Enable or disable handle dragging. When `false`, divider handles are
    /// rendered inert — the pane layout is still valid but the user cannot
    /// resize panes.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn resolved_style(&self, ctx: &BuildContext) -> SharedSplitterStyle {
        self.style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.splitter.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeSplitterStyle::default()))
    }

    /// Main-axis extent of `bounds` for this splitter's orientation.
    fn main_extent(&self, bounds: Rect) -> f32 {
        match self.model.orientation() {
            Orientation::Horizontal => bounds.width,
            Orientation::Vertical => bounds.height,
        }
    }
}

impl std::fmt::Debug for Splitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Splitter")
            .field("panes", &self.model.pane_count())
            .field("orientation", &self.model.orientation())
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for Splitter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        // Sizes + collapse changes reflow without a rebuild.
        self.model
            .version()
            .bind_to(self_id, registry, BindingLevel::Relayout);

        let style = self.resolved_style(ctx);
        let gutter = self.model.gutter_thickness();
        let orientation = self.model.orientation();
        let n = self.model.pane_count();

        // --- Per-pane collapse progress (1 = expanded, 0 = collapsed) ---
        // Created first so each pane clip can gate its content's
        // visibility on it.
        self.progress.clear();
        let mut initial_collapsed = Vec::with_capacity(n);
        for i in 0..n {
            let collapsed = self.model.is_collapsed(i);
            initial_collapsed.push(collapsed);
            let prog = ctx.animated_signal(if collapsed { 0.0 } else { 1.0 });
            let registry = ctx.binding_registry();
            prog.bind_to(self_id, registry, BindingLevel::Relayout);
            self.progress.push(prog);
        }
        *self.prev_collapsed.borrow_mut() = initial_collapsed;

        // --- Per-pane visibility tween (1 = visible, 0 = hidden) --------
        self.visible_progress.clear();
        let mut initial_visible = Vec::with_capacity(n);
        for i in 0..n {
            let visible = self.model.is_pane_visible(i);
            initial_visible.push(visible);
            let prog = ctx.animated_signal(if visible { 1.0 } else { 0.0 });
            let registry = ctx.binding_registry();
            prog.bind_to(self_id, registry, BindingLevel::Relayout);
            self.visible_progress.push(prog);
        }
        *self.prev_visible.borrow_mut() = initial_visible;

        // --- Pane clips (each user pane clipped to its placement) -------
        // A collapsed pane's content is parked *dormant* — excluded from
        // paint, the focus order, hit-test, and the a11y tree — via
        // `visible_when` on the collapse progress. A folded-away sidebar
        // must not be Tab-focusable or announced, and its animations must
        // pause. The gate tracks progress (not the raw collapsed flag) so
        // the content stays live through the shrink animation and only
        // drops out once it's effectively gone.
        self.pane_clip_ids.clear();
        self.pane_inner_ids.clear();
        self.pane_full_main.clear();
        for i in 0..n {
            let content = self.pane_content.get_mut(i).and_then(|c| c.take());
            let child_id = content.map(|pending| match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
            self.pane_inner_ids.push(child_id);
            // Effective visibility = collapse × visible. Drives the ClipPane's
            // shrink.
            let effective = self.progress[i]
                .zip(&self.visible_progress[i])
                .map(|(c, v)| c * v);
            // Content goes dormant when the pane is collapsed OR hidden —
            // EXCEPT a pane with a non-zero `collapsed_size` keeps a visible
            // sliver while collapsed (e.g. an accordion header), so it must stay
            // live then and only drop out when truly hidden.
            let keeps_sliver = self.model.collapsed_size(i) > COLLAPSED_VISIBLE_EPSILON;
            if let Some(inner) = child_id {
                let vis = if keeps_sliver {
                    self.visible_progress[i].map(|v| *v > COLLAPSED_VISIBLE_EPSILON)
                } else {
                    effective.map(|p| *p > COLLAPSED_VISIBLE_EPSILON)
                };
                ctx.visible_when(inner, vis);
            }
            let full_main = Rc::new(Cell::new(0.0_f32));
            self.pane_full_main.push(full_main.clone());
            let label = self.pane_labels.get(i).and_then(|l| l.clone());
            let clip = ClipPane {
                child_id,
                labeled: label.is_some(),
                effective_progress: Some(effective),
                full_main,
                orientation,
            };
            let clip_id = match label {
                Some(lbl) => ctx.add(clip.access_label(lbl)),
                None => ctx.add(clip),
            };
            self.pane_clip_ids.push(clip_id);
        }

        // --- Handles (one per gap), wired to control the adjacent panes -
        // A gap's handle is "active" only while both its panes are visible;
        // when a neighbor is hidden the gutter shrinks to 0 and the handle is
        // disabled (Tab-skipped, event-gated) and AT-hidden.
        self.handle_ids.clear();
        for i in 0..n.saturating_sub(1) {
            let left = self.pane_clip_ids[i];
            let right = self.pane_clip_ids[i + 1];
            let vis_l = self.visible_progress[i].map(|p| *p > COLLAPSED_VISIBLE_EPSILON);
            let vis_r = self.visible_progress[i + 1].map(|p| *p > COLLAPSED_VISIBLE_EPSILON);
            let active = vis_l.and(&vis_r);
            let handle = SplitterHandle::new(SplitterHandleConfig {
                model: self.model.clone(),
                index: i,
                enabled: self.enabled,
                gutter_thickness: gutter,
                style: style.clone(),
                container_bounds: self.container_bounds.clone(),
                is_rtl: self.is_rtl.clone(),
                layout_sizes: self.layout_sizes.clone(),
                layout_gutters: self.layout_gutters.clone(),
                active: active.clone(),
            });
            let handle_id = ctx.add(handle.access_controls(left).access_controls(right));
            ctx.enabled_when(handle_id, active);
            self.handle_ids.push(handle_id);
        }

        // --- Effect: drive each pane's progress on collapse changes -----
        // Animated for programmatic / double-click / keyboard triggers;
        // snapped for drag (the pointer is already the motion). Only the
        // panes whose collapsed flag actually changed are touched, so an
        // unrelated drag never clobbers an in-flight collapse tween.
        let anim = ctx.animate().collapse().standard();
        let model = self.model.clone();
        let progress = self.progress.clone();
        let visible_progress = self.visible_progress.clone();
        let prev_c = self.prev_collapsed.clone();
        let prev_v = self.prev_visible.clone();
        let layout_sizes = self.layout_sizes.clone();
        ctx.effect(&self.model.version(), move |_| {
            let animate = model.consume_animate_flag();
            // Collapse changes.
            {
                let mut prev = prev_c.borrow_mut();
                let count = progress.len().min(model.pane_count());
                for i in 0..count {
                    let now = model.is_collapsed(i);
                    if prev.get(i).copied() != Some(now) {
                        if i < prev.len() {
                            prev[i] = now;
                        }
                        if now {
                            // Capture the pane's current *displayed* size as the
                            // stored size, so the tween animates from where it
                            // actually is (and restores there) — independent of
                            // any tiny fallback stored size from a stretch-grown
                            // pane that was never dragged.
                            if let Some(&disp) = layout_sizes.borrow().get(i)
                                && disp > model.collapsed_size(i)
                            {
                                model.set_stored_size_silent(i, disp);
                            }
                        }
                        let target = if now { 0.0 } else { 1.0 };
                        if animate {
                            anim.to_or_snap(&progress[i], target);
                        } else {
                            progress[i].set(target);
                        }
                    }
                }
            }
            // Visibility changes.
            {
                let mut prev = prev_v.borrow_mut();
                let count = visible_progress.len().min(model.pane_count());
                for i in 0..count {
                    let now = model.is_pane_visible(i);
                    if prev.get(i).copied() != Some(now) {
                        if i < prev.len() {
                            prev[i] = now;
                        }
                        let target = if now { 1.0 } else { 0.0 };
                        if animate {
                            anim.to_or_snap(&visible_progress[i], target);
                        } else {
                            visible_progress[i].set(target);
                        }
                    }
                }
            }
        });

        self.children()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let n = self.pane_clip_ids.len();
        let gutter = self.model.gutter_thickness();
        let orientation = self.model.orientation();
        let total_gutter = (n.saturating_sub(1)) as f32 * gutter;

        // Query each pane's intrinsic size with an unbounded main axis.
        let child_proposal = match orientation {
            Orientation::Horizontal => SizeProposal {
                width: None,
                height: proposal.height,
            },
            Orientation::Vertical => SizeProposal {
                width: proposal.width,
                height: None,
            },
        };
        let mut sum_main = 0.0;
        let mut max_cross = 0.0_f32;
        for id in &self.pane_clip_ids {
            if let Some(sz) = ctx.child_size(*id, child_proposal) {
                match orientation {
                    Orientation::Horizontal => {
                        sum_main += sz.width;
                        max_cross = max_cross.max(sz.height);
                    }
                    Orientation::Vertical => {
                        sum_main += sz.height;
                        max_cross = max_cross.max(sz.width);
                    }
                }
            }
        }
        let min_main: f32 = (0..n).map(|i| self.model.min_size(i)).sum::<f32>() + total_gutter;
        let intrinsic_main = sum_main + total_gutter;

        match orientation {
            Orientation::Horizontal => Size::new(
                proposal.width.unwrap_or(intrinsic_main).max(min_main),
                proposal.height.unwrap_or(max_cross),
            ),
            Orientation::Vertical => Size::new(
                proposal.width.unwrap_or(max_cross),
                proposal.height.unwrap_or(intrinsic_main).max(min_main),
            ),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.container_bounds.set(bounds);
        let orientation = self.model.orientation();
        let rtl = ctx.is_rtl() && matches!(orientation, Orientation::Horizontal);
        self.is_rtl.set(rtl);

        let n = self.pane_clip_ids.len();
        if n == 0 || children.len() != 2 * n - 1 {
            return;
        }

        let gutter = self.model.gutter_thickness();
        // Each gap's gutter shrinks with the visibility of its two panes, so a
        // hidden pane takes its adjacent gutter with it.
        let vis_p: Vec<f32> = self
            .visible_progress
            .iter()
            .map(|s| s.get().clamp(0.0, 1.0))
            .collect();
        let mut gutter_w = vec![0.0_f32; n - 1];
        for k in 0..n - 1 {
            let l = vis_p.get(k).copied().unwrap_or(1.0);
            let r = vis_p.get(k + 1).copied().unwrap_or(1.0);
            gutter_w[k] = gutter * l.min(r);
        }
        *self.layout_gutters.borrow_mut() = gutter_w.clone();
        let total_gutter: f32 = gutter_w.iter().sum();
        let available = (self.main_extent(bounds) - total_gutter).max(0.0);

        let collapse_p: Vec<f32> = self.progress.iter().map(|s| s.get()).collect();
        // A hidden pane shrinks like a collapsed one; combine both tweens. A
        // pane mid-tween (`progress < 1`) keeps using the collapse path even
        // after its flag flips back to expanded, so **expanding animates** too
        // (otherwise `distribute` would jump straight to `stored_size`).
        let mut snapshots = self.model.pane_snapshots();
        for (i, s) in snapshots.iter_mut().enumerate() {
            let vis = vis_p.get(i).copied().unwrap_or(1.0);
            let prog = collapse_p.get(i).copied().unwrap_or(1.0);
            // Hidden (or mid-hide) panes fold *fully to zero* — the visibility
            // tween removes the pane and its gutter. Collapse, by contrast,
            // folds only to `collapsed_size` (e.g. an accordion-header sliver).
            // So when a pane is being hidden its `collapsed_size` floor must be
            // dropped, otherwise a pane that is both collapse-floored and hidden
            // would stop at the sliver instead of disappearing.
            let hiding = !s.visible || vis < 1.0 - 0.001;
            if hiding || prog < 1.0 - 0.001 {
                s.collapsed = true;
            }
            if hiding {
                s.collapsed_size = 0.0;
            }
        }
        let combined: Vec<f32> = (0..n)
            .map(|i| {
                collapse_p.get(i).copied().unwrap_or(1.0) * vis_p.get(i).copied().unwrap_or(1.0)
            })
            .collect();
        let sizes = distribute(available, &snapshots, &combined);
        *self.layout_sizes.borrow_mut() = sizes.clone();

        // Each pane's *full* size (what it would be fully expanded). The
        // clips lay their content at this width and clip the overflow, so
        // content doesn't reflow as the pane collapses/hides — it's
        // progressively revealed/hidden, the `Collapse` trick.
        let ones = vec![1.0_f32; snapshots.len()];
        let full_sizes = distribute(available, &snapshots, &ones);
        for (k, cell) in self.pane_full_main.iter().enumerate() {
            cell.set(full_sizes.get(k).copied().unwrap_or(0.0));
        }

        // Place panes + handles, walking a local main-axis cursor.
        let place = |child: &mut WidgetPlacement, local_start: f32, extent: f32| match orientation {
            Orientation::Horizontal => {
                let x = if rtl {
                    bounds.x + bounds.width - local_start - extent
                } else {
                    bounds.x + local_start
                };
                child.origin = Point::new(x, bounds.y);
                child.size = Size::new(extent, bounds.height);
            }
            Orientation::Vertical => {
                child.origin = Point::new(bounds.x, bounds.y + local_start);
                child.size = Size::new(bounds.width, extent);
            }
        };

        let mut local = 0.0;
        for k in 0..n {
            let pane_size = sizes.get(k).copied().unwrap_or(0.0);
            place(&mut children[2 * k], local, pane_size);
            local += pane_size;
            if k < n - 1 {
                let gw = gutter_w.get(k).copied().unwrap_or(gutter);
                place(&mut children[2 * k + 1], local, gw);
                local += gw;
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        let n = self.pane_clip_ids.len();
        let mut v = Vec::with_capacity(2 * n);
        for k in 0..n {
            v.push(self.pane_clip_ids[k]);
            if k + 1 < n
                && let Some(h) = self.handle_ids.get(k)
            {
                v.push(*h);
            }
        }
        v
    }
}

/// Single-child clip wrapper for one pane. Clips overflowing content to
/// the pane's placement so it can't bleed into a gutter or sibling pane.
/// When `labeled`, it becomes a named `Role::Group` region (the name is
/// supplied via the builder-level `access_label` override); otherwise it
/// is hidden from the AT tree (its content represents itself).
#[derive(Debug)]
struct ClipPane {
    child_id: Option<WidgetId>,
    labeled: bool,
    /// Effective visibility (collapse × visible) of this pane, `1` = shown.
    /// When ~0 the clip's region is hidden from the a11y tree so a
    /// folded-away or hidden labeled pane doesn't linger as an empty group.
    effective_progress: Option<Signal<f32>>,
    /// The pane's full (uncollapsed) main-axis size, set by the parent each
    /// layout. The content is laid out at this size and clipped to the
    /// (smaller, collapsing) bounds, so it doesn't reflow mid-animation.
    full_main: Rc<Cell<f32>>,
    orientation: Orientation,
}

impl Widget for ClipPane {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
                .into(),
            None => proposal.resolve(0.0, 0.0).into(),
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Lay the content at the pane's *full* main extent (never smaller
        // than the current bounds) and let `clips_children` crop the
        // overflow. While collapsing, bounds shrink but the content keeps
        // its full layout — it's clipped, not reflowed. Anchored at the
        // leading edge so it's revealed/hidden from the gutter side.
        let full = self.full_main.get();
        let size = match self.orientation {
            Orientation::Horizontal => Size::new(full.max(bounds.width), bounds.height),
            Orientation::Vertical => Size::new(bounds.width, full.max(bounds.height)),
        };
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = size;
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        let collapsed = self
            .effective_progress
            .as_ref()
            .map(|p| p.get() <= COLLAPSED_VISIBLE_EPSILON)
            .unwrap_or(false);
        if collapsed || !self.labeled {
            builder.set_hidden();
        } else {
            builder.set_role(bastyde_core::accesskit::Role::Group);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
