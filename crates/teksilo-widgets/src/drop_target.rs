// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DropTarget` — a transparent wrapping drop container.
//!
//! Where [`DropZone`](crate::drop_zone::DropZone) is a *standalone* "drop files
//! here" placeholder with its own label / icon / Browse button, `DropTarget` is
//! a *wrapping* container: it turns any existing widget subtree into a drop
//! target without replacing its visual identity. The wrapped child fills the
//! bounds and is always visible; the widget adds a reactive highlight border +
//! tint while a drag hovers and, if a hint slot is set, fades in a centered
//! popup card ("Drop your image here").
//!
//! It reacts to **both** internal drags (typed [`DragPayload`]) and external
//! (OS) drops (files / text / URIs), through the framework's normal drag
//! pipeline (`on_drag_hover` / `on_drag_leave` / `on_drop`).
//!
//! ```ignore
//! // Wrap a panel; accept image files; show a hint while hovering.
//! DropTarget::new()
//!     .child(my_panel)
//!     .hint(TextWidget::new(lit!("Drop your image here")))
//!     .accept_external_extensions(["png", "jpg", "jpeg"])
//!     .on_drop(|payload, _pos, _ctx| { import(payload.files()); true });
//!
//! // Typed internal drag — recovers the value even after an OS round-trip
//! // or across windows (the framework's typed re-entry).
//! DropTarget::new()
//!     .child(project_card)
//!     .on_drop_typed::<ProjectRef>(|project, _pos, ctx| {
//!         ctx.send_intent(AppIntent::Link(project));
//!         true
//!     });
//! ```
//!
//! # Multi-zone drops
//!
//! Beyond the single whole-bounds target, a `DropTarget` can expose up to five
//! independently enable-able [`DropRegion`]s — `Center` / `Top` / `Bottom` /
//! `Leading` / `Trailing` — each with its own optional hint, and route the drop
//! by which zone the pointer released over. This is the VS Code-style
//! "drop on the centre to add, drop on an edge to split" affordance
//! (`DockingLayout` computes the same five zones by hand). Declare regions with
//! [`DropTarget::region`]; the side zones share one [`DropTarget::zone_size_factor`]
//! (`0.1..=1.0`, the fraction of the axis each edge strip occupies — `0.2` is the
//! default fifth, `0.5` bisects) so you size them to the context. Route with
//! [`DropTarget::on_region_drop`] (or observe [`DropTarget::active_region_signal`]).
//!
//! ```ignore
//! DropTarget::new()
//!     .child(editor_pane)
//!     .zone_size_factor(0.25)
//!     .region(DropRegion::Center,   |z| z.hint(TextWidget::new(lit!("Add as tab"))))
//!     .region(DropRegion::Leading,  |z| z.hint(TextWidget::new(lit!("Split left"))))
//!     .region(DropRegion::Trailing, |z| z.hint(TextWidget::new(lit!("Split right"))))
//!     .on_region_drop(|region, payload, _pos, ctx| { route(region, payload); true });
//! ```
//!
//! Declaring **any** region switches the target to exactly the declared regions;
//! declaring none keeps the `Center`-only whole-bounds default (`.hint(w)` is
//! sugar for `.region(DropRegion::Center, |z| z.hint(w))`). `Leading` / `Trailing`
//! map to left / right — the framework surfaces no writing direction on the
//! layout context yet, so RTL mirroring is a follow-up.
//!
//! Each zone can be **reactively enabled** with `z.enabled(signal)` (default
//! `true`): a bound `Signal<bool>` disables the zone live — no rebuild — and its
//! strip then falls through to the next-priority enabled zone (or `Center`, or
//! rejects). A drop landing in a middle covered by no *enabled* zone is rejected;
//! `on_region_drop` therefore only ever receives an enabled region.
//!
//! # Styling
//!
//! The per-zone highlight overlay + hint chrome is a Tier-3 [`DropTargetStyle`];
//! the default [`RecipeDropTargetStyle`](crate::styles::RecipeDropTargetStyle)
//! paints the active zone (centre → frame only, so the wrapped content shows
//! through; an edge strip → translucent fill + accent frame) and a full-bounds
//! error border on reject. Override per-call with [`DropTarget::style`] or
//! theme-wide via `theme.style_slots.drop_target`.
//!
//! # Accessibility
//!
//! The wrapper is a `Role::Group`. `Live` is intentionally **not** set on the
//! group (that would announce every change to the wrapped child); instead the
//! recipe scopes `Live::Polite` to each hint card so a screen reader announces
//! the active zone's hint *appearing*. Each hint is gated by `visible_when`, so a
//! non-active zone's hint leaves the AT tree entirely.
//!
//! ## Keyboard accessibility is the caller's responsibility
//!
//! An OS drag cannot be initiated from the keyboard, and — unlike
//! [`DropZone`](crate::drop_zone::DropZone), which ships a keyboard-operable
//! **Browse…** button as its WCAG 2.1.1 equivalent — `DropTarget` adds **no**
//! keyboard affordance of its own. That is by design: `DropTarget` *wraps*
//! existing content that is expected to already offer a keyboard path to the
//! same outcome (e.g. a card you can drop a project onto *or* open with a
//! context-menu "Link…" command). The drop is an **enhancement**, not the sole
//! path.
//!
//! If you use `DropTarget` for an action that has *no* other affordance, you
//! must add a keyboard equivalent yourself (a button, menu item, or shortcut) —
//! otherwise the action is unreachable for keyboard-only users, and entirely
//! unavailable on platforms with no external-DnD backend (e.g. X11, where OS
//! drag-and-drop is a no-op). `DropZone` is the better choice when the drop
//! *is* the primary action.

pub(crate) mod overlay;

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::Role;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{
    DropRegion, DropRegionSet, DropTargetDragState, DropTargetStyle, DropTargetStyleConfig,
    DropTargetVariant, SharedDropTargetStyle, region_at,
};
use teksilo_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::{DragPayload, DropFeedback};

type AcceptPredicate = Rc<dyn Fn(&DragPayload) -> bool>;
type DropCallback = Box<dyn FnMut(DragPayload, Point, &mut EventContext) -> bool>;
type RegionDropCallback = Box<dyn FnMut(DropRegion, DragPayload, Point, &mut EventContext) -> bool>;
type LeaveCallback = Box<dyn FnMut(&mut EventContext)>;

/// Default side-zone size factor (fraction of the axis each edge zone occupies)
/// when the caller doesn't set one — matches docking's historical 20 %.
const DEFAULT_ZONE_SIZE_FACTOR: f32 = 0.2;

/// Per-region configuration for a multi-zone [`DropTarget`]: an optional hint
/// plus a reactive enabled flag. Kept as a struct so more per-zone knobs can
/// land without a signature churn.
pub struct DropRegionSpec {
    hint: Option<PendingChild>,
    enabled: Prop<bool>,
}

impl DropRegionSpec {
    /// An enabled spec with no hint.
    pub fn new() -> Self {
        Self {
            hint: None,
            enabled: Prop::Static(true),
        }
    }

    /// Widget shown (centered in this region's rect, inside a popup card) while
    /// a drag with an accepted payload hovers **this** region.
    pub fn hint(mut self, widget: impl Widget + 'static) -> Self {
        self.hint = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// This region's hint content by pre-registered `WidgetId`.
    pub fn hint_id(mut self, id: WidgetId) -> Self {
        self.hint = Some(PendingChild::Id(id));
        self
    }

    /// Whether this zone is active — static or signal-bound (default `true`). A
    /// bound `Signal<bool>` enables/disables the zone **live, without a rebuild**:
    /// while disabled the zone stops hit-testing (its area falls through to the
    /// next-priority enabled zone, or `Center`, or rejects), never highlights,
    /// and never shows its hint. The enabled state is resolved on every drag
    /// tick, so a `.set(false)` mid-drag takes effect on the next hover.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }
}

impl Default for DropRegionSpec {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DropRegionSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropRegionSpec")
            .field("has_hint", &self.hint.is_some())
            .finish()
    }
}

/// Resolve the currently-**enabled** [`DropRegionSet`] from the declared
/// per-region enable props (evaluated live each drag tick). An empty list means
/// no `.region(...)` was declared → the implicit `Center`-only default.
fn resolve_region_set(specs: &[(DropRegion, Prop<bool>)]) -> DropRegionSet {
    if specs.is_empty() {
        DropRegionSet::default()
    } else {
        specs
            .iter()
            .fold(DropRegionSet::none(), |set, (region, enabled)| {
                if enabled.get() {
                    set.with(*region)
                } else {
                    set
                }
            })
    }
}

/// A transparent container that turns its child into a drop target. See the
/// module docs.
pub struct DropTarget {
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// Declared regions in call order (each with its optional per-zone hint).
    /// Empty → the implicit `Center`-only whole-bounds default.
    regions: Vec<(DropRegion, DropRegionSpec)>,
    size_factor: f32,
    accept_predicate: Option<AcceptPredicate>,
    on_drop_callback: Option<DropCallback>,
    on_region_drop_callback: Option<RegionDropCallback>,
    on_drag_leave_callback: Option<LeaveCallback>,
    out_targeted: Option<Signal<bool>>,
    out_drag_state: Option<Signal<DropTargetDragState>>,
    out_active_region: Option<Signal<Option<DropRegion>>>,
    variant: DropTargetVariant,
    style_override: Option<SharedDropTargetStyle>,
    /// Written every layout pass so the hover/drop handlers can classify the
    /// target-local pointer into a region (the `DockPanePane` idiom).
    self_size: Rc<Cell<Size>>,
    root_child_id: Option<WidgetId>,
}

impl DropTarget {
    /// A drop target with no child yet — call [`Self::child`] (required).
    pub fn new() -> Self {
        Self {
            pending_child: None,
            child_id: None,
            regions: Vec::new(),
            size_factor: DEFAULT_ZONE_SIZE_FACTOR,
            accept_predicate: None,
            on_drop_callback: None,
            on_region_drop_callback: None,
            on_drag_leave_callback: None,
            out_targeted: None,
            out_drag_state: None,
            out_active_region: None,
            variant: DropTargetVariant::Default,
            style_override: None,
            self_size: Rc::new(Cell::new(Size::ZERO)),
            root_child_id: None,
        }
    }

    /// Upsert a region's spec (last-call-wins per region).
    fn set_region(&mut self, region: DropRegion, spec: DropRegionSpec) {
        if let Some(slot) = self.regions.iter_mut().find(|(r, _)| *r == region) {
            slot.1 = spec;
        } else {
            self.regions.push((region, spec));
        }
    }

    // ── Child slot (required) ───────────────────────────────────────────────

    /// The wrapped content — fills the bounds and is always visible.
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// The wrapped content by pre-registered `WidgetId`.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    // ── Zones (optional multi-region) ────────────────────────────────────────

    /// Enable and configure a drop [`DropRegion`]. Declaring **any** region
    /// switches the target to exactly the declared regions; declaring none
    /// leaves the implicit `Center`-only whole-bounds default. The spec closure
    /// configures the region (currently: an optional hint).
    ///
    /// ```ignore
    /// DropTarget::new()
    ///     .child(editor)
    ///     .zone_size_factor(0.25)
    ///     .region(DropRegion::Center,   |z| z.hint(TextWidget::new(lit!("Add tab"))))
    ///     .region(DropRegion::Leading,  |z| z.hint(TextWidget::new(lit!("Split left"))))
    ///     .region(DropRegion::Trailing, |z| z.hint(TextWidget::new(lit!("Split right"))))
    ///     .on_region_drop(|region, payload, _pos, ctx| { route(region, payload); true });
    /// ```
    pub fn region(
        mut self,
        region: DropRegion,
        f: impl FnOnce(DropRegionSpec) -> DropRegionSpec,
    ) -> Self {
        self.set_region(region, f(DropRegionSpec::new()));
        self
    }

    /// The fraction of the axis each **side** zone occupies (clamped to
    /// `0.1..=1.0`). `0.2` is the default fifth; `0.5` bisects. Applies to all
    /// four edge zones in common; `Center` takes the leftover middle.
    pub fn zone_size_factor(mut self, factor: f32) -> Self {
        self.size_factor = factor.clamp(0.1, 1.0);
        self
    }

    // ── Hint slot (single-zone sugar) ─────────────────────────────────────────

    /// Widget shown centered inside a popup card while a drag with an accepted
    /// payload hovers. Sugar for `.region(DropRegion::Center, |z| z.hint(w))` —
    /// the classic whole-bounds single-zone case.
    pub fn hint(mut self, widget: impl Widget + 'static) -> Self {
        self.set_region(DropRegion::Center, DropRegionSpec::new().hint(widget));
        self
    }

    /// Hint content by pre-registered `WidgetId` (Center region).
    pub fn hint_id(mut self, id: WidgetId) -> Self {
        self.set_region(DropRegion::Center, DropRegionSpec::new().hint_id(id));
        self
    }

    // ── Accept filtering (last-call-wins; default = accept all) ──────────────

    /// Accept any payload (internal or external). Explicit form of the default.
    pub fn accept_any(mut self) -> Self {
        self.accept_predicate = Some(Rc::new(|_| true));
        self
    }

    /// Accept any external (OS) drop, regardless of content.
    pub fn accept_external(mut self) -> Self {
        self.accept_predicate = Some(Rc::new(|p: &DragPayload| p.is_external()));
        self
    }

    /// Accept external drops that carry at least one file. Optimistic at hover
    /// on Wayland (where the file bytes only arrive at drop) if the source
    /// advertises a `text/uri-list`.
    pub fn accept_external_files(mut self) -> Self {
        self.accept_predicate = Some(Rc::new(|p: &DragPayload| {
            p.is_external() && (!p.files().is_empty() || offers_uri_list(p))
        }));
        self
    }

    /// Accept external text drops. Optimistic at hover on Wayland if the source
    /// advertises a text format.
    pub fn accept_external_text(mut self) -> Self {
        self.accept_predicate = Some(Rc::new(|p: &DragPayload| {
            p.is_external() && (p.text().is_some() || offers_text(p))
        }));
        self
    }

    /// Accept external file drops whose extension is in `extensions`
    /// (case-insensitive). At hover on Wayland the real check is deferred to
    /// drop (no file bytes yet); it is optimistic if a `text/uri-list` is
    /// advertised.
    pub fn accept_external_extensions<I, S>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let exts: Vec<String> = extensions
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        self.accept_predicate = Some(Rc::new(move |p: &DragPayload| {
            if !p.is_external() {
                return false;
            }
            let files = p.files();
            if !files.is_empty() {
                return files.iter().all(|path| {
                    path.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
                        .unwrap_or(false)
                });
            }
            // Hover with no concrete bytes yet (Wayland): optimistic.
            offers_uri_list(p)
        }));
        self
    }

    /// Accept internal drags whose payload carries a value of type `T`.
    /// Ergonomic companion to [`Self::on_drop_typed`].
    pub fn accept_typed<T: 'static>(mut self) -> Self {
        self.accept_predicate = Some(Rc::new(|p: &DragPayload| p.has_typed::<T>()));
        self
    }

    /// Custom predicate — full control over payload inspection.
    pub fn accept_when(mut self, f: impl Fn(&DragPayload) -> bool + 'static) -> Self {
        self.accept_predicate = Some(Rc::new(f));
        self
    }

    // ── Caller-observable state ──────────────────────────────────────────────

    /// The widget writes `true` while a drag with an *accepted* payload is over
    /// the target, `false` otherwise — SwiftUI's `isTargeted` pattern. Drive
    /// custom visuals off this signal.
    pub fn targeted_signal(mut self, signal: Signal<bool>) -> Self {
        self.out_targeted = Some(signal);
        self
    }

    /// Full three-state version of [`Self::targeted_signal`].
    pub fn drag_state_signal(mut self, signal: Signal<DropTargetDragState>) -> Self {
        self.out_drag_state = Some(signal);
        self
    }

    /// The widget writes which [`DropRegion`] an *accepted* drag is currently
    /// over (`None` when idle, rejecting, or over a disabled middle). Drive
    /// custom per-zone visuals off this.
    pub fn active_region_signal(mut self, signal: Signal<Option<DropRegion>>) -> Self {
        self.out_active_region = Some(signal);
        self
    }

    // ── Callbacks ──────────────────────────────────────────────────────────────

    /// Handle a drop. Return `true` to accept, `false` to reject. Invoked only
    /// when the accept filter passes.
    pub fn on_drop(
        mut self,
        f: impl FnMut(DragPayload, Point, &mut EventContext) -> bool + 'static,
    ) -> Self {
        self.on_drop_callback = Some(Box::new(f));
        self
    }

    /// Ergonomic typed drop: implicitly sets `accept_typed::<T>()` and extracts
    /// the typed value before invoking `f`. Last-call-wins with [`Self::on_drop`].
    pub fn on_drop_typed<T: 'static>(
        mut self,
        mut f: impl FnMut(T, Point, &mut EventContext) -> bool + 'static,
    ) -> Self {
        self.accept_predicate = Some(Rc::new(|p: &DragPayload| p.has_typed::<T>()));
        self.on_drop_callback = Some(Box::new(move |mut payload, pos, ctx| {
            match payload.take_typed::<T>() {
                Some(value) => f(value, pos, ctx),
                None => false,
            }
        }));
        self
    }

    /// Region-aware drop: receives which [`DropRegion`] the pointer released
    /// over, plus the payload. Last-call-wins with [`Self::on_drop`] — when set,
    /// it is used instead of the plain `on_drop`. Invoked only when the accept
    /// filter passes; return `true` to accept.
    pub fn on_region_drop(
        mut self,
        f: impl FnMut(DropRegion, DragPayload, Point, &mut EventContext) -> bool + 'static,
    ) -> Self {
        self.on_region_drop_callback = Some(Box::new(f));
        self
    }

    /// Called when a drag leaves the target (pointer exit, drop completion, or
    /// cancel).
    pub fn on_drag_leave(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.on_drag_leave_callback = Some(Box::new(f));
        self
    }

    // ── Style ────────────────────────────────────────────────────────────────

    /// Visual prominence of the hover indicator.
    pub fn variant(mut self, variant: DropTargetVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Per-call style override (Tier-3). Wins over the theme slot and the
    /// default recipe.
    pub fn style(mut self, style: impl DropTargetStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }
}

impl Default for DropTarget {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DropTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropTarget")
            .field("variant", &self.variant)
            .field("regions", &self.regions.len())
            .field("size_factor", &self.size_factor)
            .field("has_accept_filter", &self.accept_predicate.is_some())
            .finish()
    }
}

/// Does the payload advertise a `text/uri-list` format? (Wayland hover, before
/// file bytes arrive.)
fn offers_uri_list(p: &DragPayload) -> bool {
    p.formats()
        .iter()
        .any(|f| f == "text/uri-list" || f.starts_with("text/uri-list"))
}

/// Does the payload advertise a text format? (Wayland hover.)
fn offers_text(p: &DragPayload) -> bool {
    p.formats().iter().any(|f| {
        f == "text/plain"
            || f.starts_with("text/plain")
            || f == "UTF8_STRING"
            || f == "STRING"
            || f == "TEXT"
    })
}

impl Widget for DropTarget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let drag_state = ctx.signal(DropTargetDragState::Idle);
        let active_region = ctx.signal(None::<DropRegion>);

        // Resolve the (required) child slot.
        let content_id = match self.pending_child.take() {
            Some(PendingChild::Id(id)) => id,
            Some(PendingChild::Deferred(w)) => ctx.add_boxed(w),
            None => panic!("DropTarget requires a child — call .child(...) or .child_id(...)"),
        };
        self.child_id = Some(content_id);

        // Declared set (structural — which zones this target exposes), for the
        // style config. The *live enabled* set (honouring each zone's reactive
        // `.enabled` prop) is resolved per drag tick in the handlers below.
        let declared_set = if self.regions.is_empty() {
            DropRegionSet::default()
        } else {
            self.regions
                .iter()
                .fold(DropRegionSet::none(), |set, (r, _)| set.with(*r))
        };

        // Resolve each region's optional hint into a WidgetId, and keep its
        // reactive enable prop for the live hit-test.
        let mut region_hints: Vec<(DropRegion, WidgetId)> = Vec::new();
        let mut enable_specs: Vec<(DropRegion, Prop<bool>)> = Vec::new();
        for (region, spec) in std::mem::take(&mut self.regions) {
            if let Some(hint) = spec.hint {
                let id = match hint {
                    PendingChild::Id(id) => id,
                    PendingChild::Deferred(w) => ctx.add_boxed(w),
                };
                region_hints.push((region, id));
            }
            enable_specs.push((region, spec.enabled));
        }

        // Tier-3 chrome: per-call > theme slot > default recipe.
        let style: SharedDropTargetStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.drop_target.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeDropTargetStyle::default()));

        let cfg = DropTargetStyleConfig {
            content_id,
            drag_state: drag_state.clone(),
            active_region: active_region.clone(),
            regions: declared_set,
            region_hints,
            size_factor: self.size_factor,
            variant: self.variant,
        };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);

        // Drag behaviour on the composite node (the drop target). Signals are
        // Clone (one per closure); each user callback is owned by exactly one
        // closure; only the accept predicate (an Rc) is shared.
        let ds_hover = drag_state.clone();
        let ds_leave = drag_state.clone();
        let ar_hover = active_region.clone();
        let ar_leave = active_region.clone();
        let tgt_hover = self.out_targeted.clone();
        let tgt_leave = self.out_targeted.clone();
        let st_hover = self.out_drag_state.clone();
        let st_leave = self.out_drag_state.clone();
        let out_ar_hover = self.out_active_region.clone();
        let out_ar_leave = self.out_active_region.clone();
        let accept_hover = self.accept_predicate.clone();
        let accept_drop = self.accept_predicate.clone();
        let size_hover = self.self_size.clone();
        let size_drop = self.self_size.clone();
        let specs_hover = enable_specs.clone();
        let specs_drop = enable_specs;
        let factor = self.size_factor;
        let mut on_leave_cb = self.on_drag_leave_callback.take();
        let mut on_drop_cb = self.on_drop_callback.take();
        let mut on_region_drop_cb = self.on_region_drop_callback.take();

        let handlers = HandlerSet::new()
            .clips_children(true)
            .on_drag_hover(move |payload, pos, _ctx| {
                let accepts = accept_hover.as_ref().is_none_or(|p| p(payload));
                // Which zone is under the pointer (only meaningful on accept).
                // `None` = the payload is rejected, OR it is accepted but the
                // pointer is over a middle with no enabled zone (a "dead middle"
                // when only side zones are declared with a small size_factor).
                let new_region = if accepts {
                    region_at(
                        pos,
                        size_hover.get(),
                        resolve_region_set(&specs_hover),
                        factor,
                    )
                } else {
                    None
                };
                // This target only *engages* (is a real drop target) when the
                // payload is accepted AND the pointer is over an enabled zone —
                // so a drop in a dead middle bubbles to an ancestor and is never
                // delivered here (honouring region_at's documented "no zone →
                // reject" contract). A rejected payload shows the reject tint; an
                // accepted-but-zoneless hover is treated as idle for this target.
                let engaged = accepts && new_region.is_some();
                let new_state = if !accepts {
                    DropTargetDragState::HoverReject
                } else if engaged {
                    DropTargetDragState::HoverAccept
                } else {
                    DropTargetDragState::Idle
                };
                // GUARD: Signal::set always notifies (no dirty-check), and
                // on_drag_hover fires every tick. Re-issuing the same target
                // each tick would restart hint tweens. Only write on a real
                // change of (state, region) — moving *within* a zone is a no-op,
                // crossing into a new zone repaints the overlay + swaps hints.
                if ds_hover.get() != new_state {
                    ds_hover.set(new_state);
                    if let Some(s) = &tgt_hover {
                        s.set(engaged);
                    }
                    if let Some(s) = &st_hover {
                        s.set(new_state);
                    }
                }
                if ar_hover.get() != new_region {
                    ar_hover.set(new_region);
                    if let Some(s) = &out_ar_hover {
                        s.set(new_region);
                    }
                }
                // Visuals are signal-driven, so engage with `Accept` (no
                // framework-drawn feedback) when this target accepts AND a zone is
                // under the pointer; otherwise `NoFeedback` so the drag bubbles to
                // the next drop target up (e.g. a reorderable list behind a
                // per-row DropTarget, or an ancestor for a dead-middle hover).
                if engaged {
                    DropFeedback::Accept
                } else {
                    DropFeedback::NoFeedback
                }
            })
            .on_drag_leave(move |ctx| {
                if ds_leave.get() != DropTargetDragState::Idle {
                    ds_leave.set(DropTargetDragState::Idle);
                    if let Some(s) = &tgt_leave {
                        s.set(false);
                    }
                    if let Some(s) = &st_leave {
                        s.set(DropTargetDragState::Idle);
                    }
                }
                if ar_leave.get().is_some() {
                    ar_leave.set(None);
                    if let Some(s) = &out_ar_leave {
                        s.set(None);
                    }
                }
                if let Some(cb) = &mut on_leave_cb {
                    cb(ctx);
                }
            })
            .on_drop(move |payload, pos, ctx| {
                // The hover predicate is only a visual gate; the framework still
                // routes the drop here. Re-check before accepting.
                let accepts = accept_drop.as_ref().is_none_or(|p| p(&payload));
                if !accepts {
                    return false;
                }
                // Region-aware callback wins over the plain one. A drop that
                // classifies to no enabled zone (a dead middle with no `Center`)
                // is REJECTED — `on_region_drop` only ever receives an enabled
                // region, matching region_at's contract and the hover path (which
                // never engages there). Normally the hover gate means such a drop
                // never routes here at all; this is the belt-and-suspenders.
                if let Some(cb) = &mut on_region_drop_cb {
                    match region_at(
                        pos,
                        size_drop.get(),
                        resolve_region_set(&specs_drop),
                        factor,
                    ) {
                        Some(region) => cb(region, payload, pos, ctx),
                        None => false,
                    }
                } else if let Some(cb) = &mut on_drop_cb {
                    cb(payload, pos, ctx)
                } else {
                    false
                }
            });
        ctx.apply_self_handlers(handlers);

        self.children()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Report the *content's* full response (grow / shrink / floor), not the
        // chrome wrapper's: a `DropTarget` is a transparent wrapper whose border
        // / hint are overlays that don't change size. Forwarding the wrapper's
        // response (a ZStack, which reports rigid) would flatten a flexible
        // child like `Expand` (flex-basis 0) to a rigid zero and collapse it
        // inside a flex/fill parent. `place_children` still fills the wrapper,
        // which then stretches the content to those bounds.
        self.child_id
            .or(self.root_child_id)
            .and_then(|id| ctx.child_layout_response(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Cache our own size so the hover/drop handlers can classify the
        // target-local pointer into a region.
        self.self_size.set(bounds.size());
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The composite node is the drop target and a semantic group. Live is
        // scoped to the hint card by the recipe, not set here — see module docs.
        builder.set_role(Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::RectWidget;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;
    use teksilo_canvas::Size;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_core::{ExternalDropData, NoopWindowOps};
    use teksilo_i18n::lit;

    /// Minimal fixed-size leaf so we can assert intrinsic-size delegation.
    #[derive(Debug)]
    struct Fixed(f32, f32);
    impl Widget for Fixed {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    /// Fixed-size leaf that paints a distinctive red fill — lets a test detect
    /// whether the hint subtree actually rendered.
    #[derive(Debug)]
    struct Marker;
    impl Widget for Marker {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(40.0, 20.0).into()
        }
        fn paint(
            &self,
            bounds: teksilo_canvas::Rect,
            canvas: &mut teksilo_canvas::Canvas,
            _ctx: &teksilo_core::widget::PaintContext,
        ) {
            canvas.fill_rounded_rect(
                bounds,
                teksilo_tokens::CornerRadius::uniform(4.0),
                teksilo_tokens::Color::RED,
            );
        }
    }

    fn themed_tree() -> WidgetTree {
        WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
    }

    /// `DropTarget` is layout-transparent: it reports exactly the wrapped
    /// child's natural size (the tint overlay + centered hint slot must not
    /// inflate it).
    #[test]
    fn reports_child_natural_size() {
        let mut tree = themed_tree();
        let target = tree.add(
            DropTarget::new()
                .child(Fixed(200.0, 100.0))
                .hint(Fixed(50.0, 20.0)),
        );
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(target);
        assert!(
            (b.width - 200.0).abs() < 0.01 && (b.height - 100.0).abs() < 0.01,
            "expected 200x100, got {}x{}",
            b.width,
            b.height
        );
    }

    /// The wrapped child fills the full bounds (always visible).
    #[test]
    fn child_fills_bounds() {
        let mut tree = themed_tree();
        let inner = tree.add(RectWidget::new());
        tree.add(DropTarget::new().child_id(inner));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let cb = tree.bounds(inner);
        assert!((cb.width - 300.0).abs() < 0.01 && (cb.height - 200.0).abs() < 0.01);
    }

    /// Regression: a flexible child (`Expand`, flex-basis 0) wrapped in a
    /// `DropTarget` must stay flexible so a flex/fill parent stretches it to
    /// fill. The drop target must forward the content's grow weight, not flatten
    /// it to a rigid zero (which centered it and collapsed it to nothing).
    #[test]
    fn forwards_flexible_child_through_flex_parent() {
        use crate::primitives::{Expand, Padding, ZStack};
        let mut tree = themed_tree();
        let inner = tree.add(RectWidget::new());
        let expand = tree.add(Expand::new().child_id(inner));
        let dt = tree.add(DropTarget::new().child_id(expand));
        let pad = tree.add(Padding::uniform(16.0).child_id(dt));
        let _z = tree.add(ZStack::new().child(RectWidget::new()).add_child(pad));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let b = tree.bounds(inner);
        assert!(
            b.width > 700.0 && b.height > 500.0,
            "flexible child collapsed inside DropTarget: {b:?}"
        );
    }

    /// Regression: the decorative highlight border must be `event_pass_through`
    /// so a tap reaches the wrapped (interactive) content — otherwise wrapping a
    /// tree row's expand chevron / a button in a `DropTarget` silently breaks it.
    #[test]
    fn border_overlay_does_not_block_taps_to_content() {
        use teksilo_core::event::PointerButton;
        use teksilo_core::widget_builder::WidgetBuilder;
        let tapped = Rc::new(Cell::new(false));
        let t = tapped.clone();
        let mut tree = themed_tree();
        let inner = tree.add(RectWidget::new().on_tap(move |_e, _ctx| t.set(true)));
        tree.add(DropTarget::new().child_id(inner));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let center = tree.bounds(inner).center();
        tree.pointer_down_button(center, PointerButton::Primary);
        tree.pointer_up_button(center, PointerButton::Primary);
        assert!(
            tapped.get(),
            "the DropTarget border overlay must not eat taps meant for the wrapped content"
        );
    }

    /// An accepted external file drop reaches `on_drop`.
    #[test]
    fn external_file_accepted_fires_on_drop() {
        let mut tree = themed_tree();
        let dropped = Rc::new(Cell::new(false));
        let d = dropped.clone();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .accept_external_files()
                .on_drop(move |_payload, _pos, _ctx| {
                    d.set(true);
                    true
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/photo.png")],
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data.clone(), &mut noop);
        tree.end_external_drag(p, data, &mut noop);

        assert!(dropped.get(), "accepted file drop should fire on_drop");
    }

    /// The headline feature: an **internal** typed drag flows through
    /// `accept_typed` (set implicitly by `on_drop_typed`) and the value is
    /// extracted via `take_typed` before the callback runs.
    #[test]
    fn internal_typed_drop_extracts_value() {
        #[derive(Debug, Clone, PartialEq)]
        struct ProjectRef(u32);

        // A source widget that starts a typed internal drag on drag-start.
        #[derive(Debug)]
        struct TypedDragSource;
        impl Widget for TypedDragSource {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                let self_id = ctx.self_id();
                let hs = HandlerSet::new().on_drag(move |phase, ctx| {
                    if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                        ctx.start_drag(self_id, DragPayload::typed(ProjectRef(7)));
                    }
                });
                ctx.apply_self_handlers(hs);
                Vec::new()
            }
            fn layout_response(
                &self,
                _proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> LayoutResponse {
                Size::new(100.0, 80.0).into()
            }
        }

        let mut tree = themed_tree();
        let got: Rc<RefCell<Option<ProjectRef>>> = Rc::new(RefCell::new(None));
        let g = got.clone();
        let target = DropTarget::new()
            .child(Fixed(100.0, 80.0))
            .on_drop_typed::<ProjectRef>(move |project, _pos, _ctx| {
                *g.borrow_mut() = Some(project);
                true
            });
        let source_id = tree.add(TypedDragSource);
        let target_id = tree.add(target);
        let es = tree.add(
            crate::primitives::Expand::new()
                .flex(1.0)
                .child_id(source_id),
        );
        let et = tree.add(
            crate::primitives::Expand::new()
                .flex(1.0)
                .child_id(target_id),
        );
        tree.add(crate::primitives::HStack::new().add_child(es).add_child(et));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let from = tree.bounds(source_id).center();
        let to = tree.bounds(target_id).center();
        tree.drag(from, to);

        assert_eq!(
            *got.borrow(),
            Some(ProjectRef(7)),
            "internal typed drop must extract and deliver the typed value",
        );
    }

    /// A typed drop target rejects a typed payload of the *wrong* type:
    /// `accept_typed::<T>` fails, so the user callback never runs.
    #[test]
    fn internal_typed_drop_rejects_other_type() {
        #[derive(Debug, Clone)]
        struct ProjectRef(u32);
        #[derive(Debug, Clone)]
        struct OtherRef(u32);

        #[derive(Debug)]
        struct OtherSource;
        impl Widget for OtherSource {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                let self_id = ctx.self_id();
                let hs = HandlerSet::new().on_drag(move |phase, ctx| {
                    if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                        ctx.start_drag(self_id, DragPayload::typed(OtherRef(1)));
                    }
                });
                ctx.apply_self_handlers(hs);
                Vec::new()
            }
            fn layout_response(
                &self,
                _proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> LayoutResponse {
                Size::new(100.0, 80.0).into()
            }
        }

        let mut tree = themed_tree();
        let fired = Rc::new(Cell::new(false));
        let f = fired.clone();
        let target = DropTarget::new()
            .child(Fixed(100.0, 80.0))
            .on_drop_typed::<ProjectRef>(move |_p, _pos, _ctx| {
                f.set(true);
                true
            });
        let source_id = tree.add(OtherSource);
        let target_id = tree.add(target);
        let es = tree.add(
            crate::primitives::Expand::new()
                .flex(1.0)
                .child_id(source_id),
        );
        let et = tree.add(
            crate::primitives::Expand::new()
                .flex(1.0)
                .child_id(target_id),
        );
        tree.add(crate::primitives::HStack::new().add_child(es).add_child(et));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let from = tree.bounds(source_id).center();
        let to = tree.bounds(target_id).center();
        tree.drag(from, to);

        assert!(!fired.get(), "a payload of the wrong type must be rejected");
    }

    /// The accept filter rejects non-matching extensions: `on_drop` re-checks
    /// the predicate and never invokes the user callback.
    #[test]
    fn extension_filter_rejects_wrong_type() {
        let mut tree = themed_tree();
        let dropped = Rc::new(Cell::new(false));
        let d = dropped.clone();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .accept_external_extensions(["png"])
                .on_drop(move |_payload, _pos, _ctx| {
                    d.set(true);
                    true
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/notes.txt")],
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data.clone(), &mut noop);
        tree.end_external_drag(p, data, &mut noop);

        assert!(!dropped.get(), "non-png drop must be rejected");
    }

    /// `out_targeted` is written `true` while an accepted drag hovers and
    /// reset to `false` once the drag ends.
    #[test]
    fn is_targeted_tracks_accepted_hover() {
        let mut tree = themed_tree();
        let targeted = Signal::new(false);
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .accept_external_files()
                .targeted_signal(targeted.clone())
                .on_drop(|_p, _pos, _ctx| true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png")],
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data.clone(), &mut noop);
        assert!(targeted.get(), "accepted hover sets is_targeted true");
        tree.end_external_drag(p, data, &mut noop);
        assert!(!targeted.get(), "drop/leave resets is_targeted");
    }

    /// A rejected drag drives `out_drag_state` to `HoverReject`, not
    /// `HoverAccept`.
    #[test]
    fn drag_state_reports_reject() {
        let mut tree = themed_tree();
        let state = Signal::new(DropTargetDragState::Idle);
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .accept_external_extensions(["png"])
                .drag_state_signal(state.clone())
                .on_drop(|_p, _pos, _ctx| true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/notes.txt")],
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data, &mut noop);
        assert_eq!(state.get(), DropTargetDragState::HoverReject);
    }

    /// The hint popup is culled at rest and paints only while an accepted drag
    /// hovers. Regression for "the popup never appears".
    #[test]
    fn hint_paints_only_on_accepted_hover() {
        let mut tree = themed_tree();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .hint(Marker)
                .accept_external_files()
                .on_drop(|_p, _pos, _ctx| true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let red = teksilo_tokens::Color::RED.to_array();
        let frame = tree.render();
        assert!(
            !frame.shapes.iter().any(|s| s.color == red),
            "hint must be hidden at rest"
        );

        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png")],
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data, &mut noop);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert!(
            frame.shapes.iter().any(|s| s.color == red),
            "hint must paint while an accepted drag hovers"
        );
    }

    /// Smoke test: builds and renders with a hint + Prominent variant without
    /// panicking, and still sizes to the child.
    #[test]
    fn builds_with_hint_and_prominent_variant() {
        let mut tree = themed_tree();
        let target = tree.add(
            DropTarget::new()
                .child(Fixed(160.0, 90.0))
                .hint(crate::primitives::TextWidget::new(lit!("Drop here")))
                .variant(DropTargetVariant::Prominent)
                .accept_any()
                .on_drop(|_p, _pos, _ctx| true),
        );
        tree.layout(SizeProposal::exact(160.0, 90.0));
        let _ = tree.render();
        let b = tree.bounds(target);
        assert!(b.width > 0.0 && b.height > 0.0);
    }

    // ── Multi-zone ────────────────────────────────────────────────────────────

    fn png_drop(tree: &mut WidgetTree, p: Point) {
        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png")],
            ..Default::default()
        };
        tree.begin_external_drag(p, data.clone(), &mut noop);
        tree.end_external_drag(p, data, &mut noop);
    }

    /// A drop landing in the leading edge strip reports `DropRegion::Leading`.
    #[test]
    fn region_drop_reports_leading() {
        let mut tree = themed_tree();
        let got: Rc<RefCell<Option<DropRegion>>> = Rc::new(RefCell::new(None));
        let g = got.clone();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .region(DropRegion::Center, |z| z)
                .region(DropRegion::Leading, |z| z)
                .accept_external_files()
                .on_region_drop(move |region, _p, _pos, _ctx| {
                    *g.borrow_mut() = Some(region);
                    true
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // 400 wide, factor 0.2 → leading strip is x < 80.
        png_drop(&mut tree, Point::new(20.0, 150.0));
        assert_eq!(*got.borrow(), Some(DropRegion::Leading));
    }

    /// A drop in the middle of a five-ish-zone target reports `Center`.
    #[test]
    fn region_drop_reports_center_in_middle() {
        let mut tree = themed_tree();
        let got: Rc<RefCell<Option<DropRegion>>> = Rc::new(RefCell::new(None));
        let g = got.clone();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .region(DropRegion::Center, |z| z)
                .region(DropRegion::Leading, |z| z)
                .region(DropRegion::Trailing, |z| z)
                .accept_external_files()
                .on_region_drop(move |region, _p, _pos, _ctx| {
                    *g.borrow_mut() = Some(region);
                    true
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        png_drop(&mut tree, Point::new(200.0, 150.0));
        assert_eq!(*got.borrow(), Some(DropRegion::Center));
    }

    /// The `size_factor` widens the side zones: at 0.5 the leading strip spans
    /// the left half, so a point that was `Center` at the default fifth is now
    /// `Leading`.
    #[test]
    fn zone_size_factor_widens_side_zones() {
        let mut tree = themed_tree();
        let got: Rc<RefCell<Option<DropRegion>>> = Rc::new(RefCell::new(None));
        let g = got.clone();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .zone_size_factor(0.5)
                .region(DropRegion::Center, |z| z)
                .region(DropRegion::Leading, |z| z)
                .accept_external_files()
                .on_region_drop(move |region, _p, _pos, _ctx| {
                    *g.borrow_mut() = Some(region);
                    true
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // x = 150 is > 80 (Center at 0.2) but < 200 (Leading at 0.5).
        png_drop(&mut tree, Point::new(150.0, 150.0));
        assert_eq!(*got.borrow(), Some(DropRegion::Leading));
    }

    /// Regression: with no region declared and no `on_region_drop`, the plain
    /// `on_drop` still fires (classic single-zone behaviour).
    #[test]
    fn center_only_default_uses_plain_on_drop() {
        let mut tree = themed_tree();
        let dropped = Rc::new(Cell::new(false));
        let d = dropped.clone();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .accept_external_files()
                .on_drop(move |_p, _pos, _ctx| {
                    d.set(true);
                    true
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        png_drop(&mut tree, Point::new(20.0, 150.0));
        assert!(
            dropped.get(),
            "center-only default must route to plain on_drop"
        );
    }

    /// `active_region_signal` tracks the hovered zone and resets on leave.
    #[test]
    fn active_region_signal_tracks_and_resets() {
        let mut tree = themed_tree();
        let region = Signal::new(None);
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .region(DropRegion::Center, |z| z)
                .region(DropRegion::Leading, |z| z)
                .accept_external_files()
                .active_region_signal(region.clone())
                .on_drop(|_p, _pos, _ctx| true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png")],
            ..Default::default()
        };
        let p = Point::new(20.0, 150.0);
        tree.begin_external_drag(p, data.clone(), &mut noop);
        assert_eq!(region.get(), Some(DropRegion::Leading));
        tree.end_external_drag(p, data, &mut noop);
        assert_eq!(region.get(), None, "leave resets the active region");
    }

    /// A per-region hint paints only while *its* region is the active hover:
    /// a Leading hint stays hidden over the centre and appears over the edge.
    #[test]
    fn per_region_hint_paints_only_for_its_zone() {
        let mut tree = themed_tree();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .region(DropRegion::Center, |z| z)
                .region(DropRegion::Leading, |z| z.hint(Marker))
                .accept_external_files()
                .on_drop(|_p, _pos, _ctx| true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let red = teksilo_tokens::Color::RED.to_array();

        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png")],
            ..Default::default()
        };

        // Hover the centre: the Leading hint must stay hidden.
        let center = Point::new(200.0, 150.0);
        tree.begin_external_drag(center, data.clone(), &mut noop);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(
            !tree.render().shapes.iter().any(|s| s.color == red),
            "Leading hint must not paint while hovering the centre"
        );
        tree.end_external_drag(center, data.clone(), &mut noop);

        // Hover the leading edge: the Leading hint appears.
        let lead = Point::new(20.0, 150.0);
        tree.begin_external_drag(lead, data.clone(), &mut noop);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(
            tree.render().shapes.iter().any(|s| s.color == red),
            "Leading hint must paint while hovering the leading zone"
        );
        tree.end_external_drag(lead, data, &mut noop);
    }

    /// A target with only side zones (no `Center`): a drop in the dead middle is
    /// **rejected** — `on_region_drop` is never invoked with a fabricated
    /// `Center`, the hover disengages (targeted → false), and the reported region
    /// clears to `None`. Regression for the `unwrap_or(Center)` contract bug.
    #[test]
    fn side_only_dead_middle_rejects_drop_and_hover() {
        let mut tree = themed_tree();
        let got: Rc<RefCell<Option<DropRegion>>> = Rc::new(RefCell::new(None));
        let g = got.clone();
        let region = Signal::new(None);
        let targeted = Signal::new(false);
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .zone_size_factor(0.2)
                .region(DropRegion::Leading, |z| z)
                .region(DropRegion::Trailing, |z| z)
                .accept_external_files()
                .active_region_signal(region.clone())
                .targeted_signal(targeted.clone())
                .on_region_drop(move |r, _p, _pos, _ctx| {
                    *g.borrow_mut() = Some(r);
                    true
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png")],
            ..Default::default()
        };
        // Hover an enabled zone first (leading, x < 80) → engages.
        tree.begin_external_drag(Point::new(20.0, 150.0), data.clone(), &mut noop);
        assert_eq!(region.get(), Some(DropRegion::Leading));
        assert!(targeted.get(), "hovering an enabled zone engages");
        // Move to the dead middle (x = 200, between the 80px side strips) → disengages.
        tree.update_external_drag(Point::new(200.0, 150.0), &mut noop);
        assert_eq!(region.get(), None, "dead middle reports no zone");
        assert!(!targeted.get(), "dead middle must not engage this target");
        // Drop in the dead middle → rejected, never delivered as a phantom Center.
        tree.end_external_drag(Point::new(200.0, 150.0), data, &mut noop);
        assert_eq!(
            *got.borrow(),
            None,
            "a dead-middle drop must be rejected, not fabricated as Center"
        );
    }

    /// The vertical axis routes too: `Top` / `Bottom` strips deliver their own
    /// region (only `Leading` / `Center` were widget-tested before).
    #[test]
    fn top_and_bottom_zones_route() {
        for (y, expected) in [(10.0_f32, DropRegion::Top), (290.0_f32, DropRegion::Bottom)] {
            let mut tree = themed_tree();
            let got: Rc<RefCell<Option<DropRegion>>> = Rc::new(RefCell::new(None));
            let g = got.clone();
            tree.add(
                DropTarget::new()
                    .child(RectWidget::new())
                    .region(DropRegion::Top, |z| z)
                    .region(DropRegion::Bottom, |z| z)
                    .region(DropRegion::Center, |z| z)
                    .accept_external_files()
                    .on_region_drop(move |r, _p, _pos, _ctx| {
                        *g.borrow_mut() = Some(r);
                        true
                    }),
            );
            tree.layout(SizeProposal::exact(400.0, 300.0));
            // 300 tall, factor 0.2 → ey = 60: y=10 → top strip, y=290 → bottom strip.
            png_drop(&mut tree, Point::new(200.0, y));
            assert_eq!(*got.borrow(), Some(expected));
        }
    }

    /// A **rejected** drag reports no active region even over a would-be zone
    /// strip (region is only meaningful for an accepted payload).
    #[test]
    fn rejected_hover_reports_no_region() {
        let mut tree = themed_tree();
        let region = Signal::new(None);
        let state = Signal::new(DropTargetDragState::Idle);
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .region(DropRegion::Leading, |z| z)
                .region(DropRegion::Center, |z| z)
                .accept_external_extensions(["png"])
                .active_region_signal(region.clone())
                .drag_state_signal(state.clone())
                .on_drop(|_p, _pos, _ctx| true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = NoopWindowOps;
        // A .txt over the leading strip: payload rejected by the extension filter.
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/notes.txt")],
            ..Default::default()
        };
        tree.begin_external_drag(Point::new(20.0, 150.0), data, &mut noop);
        assert_eq!(state.get(), DropTargetDragState::HoverReject);
        assert_eq!(
            region.get(),
            None,
            "a rejected hover must report no zone even inside a would-be strip"
        );
    }

    /// A zone's `.enabled(signal)` gates hit-testing **live**: disabling the
    /// leading zone makes its strip fall through to the next-priority enabled
    /// zone (`Center`) — no rebuild.
    #[test]
    fn reactive_zone_enabled_gates_hit_testing() {
        let mut tree = themed_tree();
        let leading_on = Signal::new(true);
        let got: Rc<RefCell<Option<DropRegion>>> = Rc::new(RefCell::new(None));
        let g = got.clone();
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .zone_size_factor(0.2)
                .region(DropRegion::Leading, |z| z.enabled(leading_on.clone()))
                .region(DropRegion::Center, |z| z)
                .accept_external_files()
                .on_region_drop(move |r, _p, _pos, _ctx| {
                    *g.borrow_mut() = Some(r);
                    true
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // Leading enabled: a drop in the leading strip (x < 80) → Leading.
        png_drop(&mut tree, Point::new(20.0, 150.0));
        assert_eq!(*got.borrow(), Some(DropRegion::Leading));
        // Disable leading live (no rebuild): the same position falls through to Center.
        leading_on.set(false);
        *got.borrow_mut() = None;
        png_drop(&mut tree, Point::new(20.0, 150.0));
        assert_eq!(
            *got.borrow(),
            Some(DropRegion::Center),
            "a live-disabled zone falls through to the next enabled zone"
        );
    }
}
