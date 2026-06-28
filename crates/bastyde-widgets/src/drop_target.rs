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
//! # Styling
//!
//! The highlight overlay + popup chrome is a Tier-3 [`DropTargetStyle`]; the
//! default [`RecipeDropTargetStyle`](crate::styles::RecipeDropTargetStyle)
//! tracks the interaction state. Override per-call with [`DropTarget::style`] or
//! theme-wide via `theme.style_slots.drop_target`.
//!
//! # Accessibility
//!
//! The wrapper is a `Role::Group`. `Live` is intentionally **not** set on the
//! group (that would announce every change to the wrapped child); instead the
//! recipe scopes `Live::Polite` to the hint card so a screen reader announces
//! the hint *appearing*. The hint is gated by `visible_when`, so it leaves the
//! AT tree entirely while idle.
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

use std::rc::Rc;

use bastyde_canvas::{Point, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::Role;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    DropTargetDragState, DropTargetStyle, DropTargetStyleConfig, DropTargetVariant,
    SharedDropTargetStyle,
};
use bastyde_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_core::{DragPayload, DropFeedback};

type AcceptPredicate = Rc<dyn Fn(&DragPayload) -> bool>;
type DropCallback = Box<dyn FnMut(DragPayload, Point, &mut EventContext) -> bool>;
type LeaveCallback = Box<dyn FnMut(&mut EventContext)>;

/// A transparent container that turns its child into a drop target. See the
/// module docs.
pub struct DropTarget {
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    pending_hint: Option<PendingChild>,
    hint_id: Option<WidgetId>,
    accept_predicate: Option<AcceptPredicate>,
    on_drop_callback: Option<DropCallback>,
    on_drag_leave_callback: Option<LeaveCallback>,
    bind_is_targeted: Option<Signal<bool>>,
    bind_drag_state: Option<Signal<DropTargetDragState>>,
    variant: DropTargetVariant,
    style_override: Option<SharedDropTargetStyle>,
    root_child_id: Option<WidgetId>,
}

impl DropTarget {
    /// A drop target with no child yet — call [`Self::child`] (required).
    pub fn new() -> Self {
        Self {
            pending_child: None,
            child_id: None,
            pending_hint: None,
            hint_id: None,
            accept_predicate: None,
            on_drop_callback: None,
            on_drag_leave_callback: None,
            bind_is_targeted: None,
            bind_drag_state: None,
            variant: DropTargetVariant::Default,
            style_override: None,
            root_child_id: None,
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

    // ── Hint slot (optional) ──────────────────────────────────────────────────

    /// Widget shown centered inside a popup card while a drag with an accepted
    /// payload hovers. Simple use: `TextWidget::new(lit!("Drop here"))`.
    pub fn hint(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_hint = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Hint content by pre-registered `WidgetId`.
    pub fn hint_id(mut self, id: WidgetId) -> Self {
        self.pending_hint = Some(PendingChild::Id(id));
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
    pub fn bind_is_targeted(mut self, signal: Signal<bool>) -> Self {
        self.bind_is_targeted = Some(signal);
        self
    }

    /// Full three-state version of [`Self::bind_is_targeted`].
    pub fn bind_drag_state(mut self, signal: Signal<DropTargetDragState>) -> Self {
        self.bind_drag_state = Some(signal);
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
            .field(
                "has_hint",
                &(self.pending_hint.is_some() || self.hint_id.is_some()),
            )
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

        // Resolve the (required) child slot.
        let content_id = match self.pending_child.take() {
            Some(PendingChild::Id(id)) => id,
            Some(PendingChild::Deferred(w)) => ctx.add_boxed(w),
            None => panic!("DropTarget requires a child — call .child(...) or .child_id(...)"),
        };
        self.child_id = Some(content_id);

        // Resolve the optional hint slot.
        let hint_id = match self.pending_hint.take() {
            Some(PendingChild::Id(id)) => Some(id),
            Some(PendingChild::Deferred(w)) => Some(ctx.add_boxed(w)),
            None => None,
        };
        self.hint_id = hint_id;

        // Tier-3 chrome: per-call > theme slot > default recipe.
        let style: SharedDropTargetStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.drop_target.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeDropTargetStyle::default()));

        let cfg = DropTargetStyleConfig {
            content_id,
            hint_id,
            drag_state: drag_state.clone(),
            variant: self.variant,
        };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);

        // Drag behaviour on the composite node (the drop target). Signals are
        // Clone (one per closure); each user callback is owned by exactly one
        // closure; only the accept predicate (an Rc) is shared.
        let ds_hover = drag_state.clone();
        let ds_leave = drag_state.clone();
        let tgt_hover = self.bind_is_targeted.clone();
        let tgt_leave = self.bind_is_targeted.clone();
        let st_hover = self.bind_drag_state.clone();
        let st_leave = self.bind_drag_state.clone();
        let accept_hover = self.accept_predicate.clone();
        let accept_drop = self.accept_predicate.clone();
        let mut on_leave_cb = self.on_drag_leave_callback.take();
        let mut on_drop_cb = self.on_drop_callback.take();

        let handlers = HandlerSet::new()
            .clips_children(true)
            .on_drag_hover(move |payload, _pos, _ctx| {
                let accepts = accept_hover.as_ref().is_none_or(|p| p(payload));
                let new = if accepts {
                    DropTargetDragState::HoverAccept
                } else {
                    DropTargetDragState::HoverReject
                };
                // GUARD: Signal::set always notifies (no dirty-check), and
                // on_drag_hover fires every tick. `new` drives the hint Fade
                // tween — re-issuing the same target each tick would restart
                // it. Only write on a real change.
                if ds_hover.get() != new {
                    ds_hover.set(new);
                    if let Some(s) = &tgt_hover {
                        s.set(accepts);
                    }
                    if let Some(s) = &st_hover {
                        s.set(new);
                    }
                }
                // Visuals are signal-driven, so engage with `Accept` (no
                // framework-drawn feedback) when this target accepts; otherwise
                // `NoFeedback` so the drag bubbles to the next drop target up
                // (e.g. a reorderable list behind a per-row DropTarget).
                if accepts {
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
                match &mut on_drop_cb {
                    Some(cb) => cb(payload, pos, ctx),
                    None => false,
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
    use bastyde_canvas::Size;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_core::{ExternalDropData, NoopWindowOps};
    use bastyde_i18n::lit;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;

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
            bounds: bastyde_canvas::Rect,
            canvas: &mut bastyde_canvas::Canvas,
            _ctx: &bastyde_core::widget::PaintContext,
        ) {
            canvas.fill_rounded_rect(
                bounds,
                bastyde_tokens::CornerRadius::uniform(4.0),
                bastyde_tokens::Color::RED,
            );
        }
    }

    fn themed_tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
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
        use bastyde_core::event::PointerButton;
        use bastyde_core::widget_builder::WidgetBuilder;
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
                    if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
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
                    if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
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

    /// `bind_is_targeted` is written `true` while an accepted drag hovers and
    /// reset to `false` once the drag ends.
    #[test]
    fn bind_is_targeted_tracks_accepted_hover() {
        let mut tree = themed_tree();
        let targeted = Signal::new(false);
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .accept_external_files()
                .bind_is_targeted(targeted.clone())
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

    /// A rejected drag drives `bind_drag_state` to `HoverReject`, not
    /// `HoverAccept`.
    #[test]
    fn bind_drag_state_reports_reject() {
        let mut tree = themed_tree();
        let state = Signal::new(DropTargetDragState::Idle);
        tree.add(
            DropTarget::new()
                .child(RectWidget::new())
                .accept_external_extensions(["png"])
                .bind_drag_state(state.clone())
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

        let red = bastyde_tokens::Color::RED.to_array();
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
}
