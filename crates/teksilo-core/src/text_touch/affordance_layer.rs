// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The widgets that fill the text-affordance overlay band: the selection
//! handles and the magnifier.
//!
//! # Why they are overlay nodes
//!
//! Both hang *outside* the text they belong to — a handle under the last line,
//! a lens above the first — and every real editor sits inside something with
//! `clips_children`. Painted by the host they would be cut off, and a 44 dp hit
//! rectangle that reaches past the editor's own bounds would never be offered
//! the press. In the
//! [`TextAffordance`](crate::overlay::OverlayBand::TextAffordance) band they are
//! ordinary widgets with ordinary bounds, above the content, below every menu,
//! and exempt from the outside-press dismissal that every caret-moving tap
//! would otherwise trigger.
//!
//! # Focus
//!
//! Handles are **not** focusable. The band is anchor-independent, so the
//! framework never moves focus into it, and a Tab stop that appears in the
//! middle of a sentence the moment a finger touched it would be worse than
//! useless to a keyboard user — who has arrow keys and Shift for the same job.
//! Assistive technology reaches a handle through
//! [`Action::SetValue`](accesskit::Action) on its
//! [`Role::Slider`](accesskit::Role) node instead, which is a route a pointer
//! and a screen reader can both take.
//!
//! # Mounting
//!
//! One [`TextAffordanceLayer`] per text surface, built in the host's `build()`
//! and raised as the content of a
//! [`FullViewport`](crate::overlay::OverlayPlacement::FullViewport) overlay in
//! the affordance band. The layer passes events through, so the window beneath
//! it behaves normally everywhere its children are not.

use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::binding::BindingLevel;
use crate::build_context::BuildContext;
use crate::event::{EventResponse, WidgetEvent};
use crate::overlay::SelectionHandleKind;
use crate::styles::{TextMagnifierRecipe, TextSelectionHandleRecipe};
use crate::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;

use super::{HandleDragPhase, SelectionHandleGeometry, TextAffordances};

/// How a host services the affordance widgets.
///
/// The widgets know where they are; only the host knows what text is under
/// them, so every act that changes the selection comes back through here. A
/// typical implementation is three lines forwarding to
/// [`TouchSelection`](super::TouchSelection) with the host's own
/// [`TextHitSource`](super::TextHitSource).
pub trait TextAffordanceDelegate {
    /// One sample of a handle drag.
    fn handle_drag(
        &self,
        kind: SelectionHandleKind,
        phase: HandleDragPhase,
        point: Point,
        ctx: &mut EventContext<'_>,
    );

    /// Move a handle to a text offset — the assistive-technology route, from
    /// `Action::SetValue` on the handle's slider node.
    fn set_handle_offset(
        &self,
        kind: SelectionHandleKind,
        offset: usize,
        ctx: &mut EventContext<'_>,
    );
}

/// One selection handle: a disc on a stem, with a square hit rectangle.
pub struct SelectionHandle {
    kind: SelectionHandleKind,
    affordances: TextAffordances,
    recipe: TextSelectionHandleRecipe,
    delegate: Rc<dyn TextAffordanceDelegate>,
}

impl std::fmt::Debug for SelectionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionHandle")
            .field("kind", &self.kind)
            .finish()
    }
}

impl SelectionHandle {
    pub fn new(
        kind: SelectionHandleKind,
        affordances: TextAffordances,
        recipe: TextSelectionHandleRecipe,
        delegate: Rc<dyn TextAffordanceDelegate>,
    ) -> Self {
        Self {
            kind,
            affordances,
            recipe,
            delegate,
        }
    }

    fn geometry(&self) -> Option<SelectionHandleGeometry> {
        self.affordances.handle(self.kind)
    }

    /// The name a screen reader reads. Deliberately not localised here: the
    /// affordance layer lives in `teksilo-core`, which has no message bundle,
    /// so a host that ships translations overrides it with `.access_label`.
    fn default_label(&self) -> &'static str {
        match self.kind {
            SelectionHandleKind::Caret => "Text cursor",
            SelectionHandleKind::Start => "Selection start",
            SelectionHandleKind::End => "Selection end",
        }
    }
}

impl Widget for SelectionHandle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let kind = self.kind;
        let drag_delegate = Rc::clone(&self.delegate);
        let action_delegate = Rc::clone(&self.delegate);
        let handlers = crate::widget_builder::HandlerSet::new()
            .on_pointer_event(move |event, ctx| {
                // A mouse's click is not this node's. Handles are only ever
                // raised by a direct pointer, but on a hybrid machine a mouse can
                // arrive afterwards and click one — and swallowing that press
                // would cost the caret placement it was asking for. The
                // controller would refuse the drag anyway; without this the press
                // is consumed before it gets there.
                //
                // `Ignored` sends it up this node's own bubble path, not to the
                // editor: a handle is a node in the overlay's content, and the
                // editor is a different root. So a host that wants a cursor's
                // click on a handle answered puts that arm on the content root
                // above these nodes — see the checklist in
                // `docs/text-touch-editing.md`.
                if !ctx.pointer_kind().is_direct() {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::PointerDown { position, .. } => {
                        ctx.capture_pointer();
                        drag_delegate.handle_drag(kind, HandleDragPhase::Begin, *position, ctx);
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerMove { position, .. } => {
                        drag_delegate.handle_drag(kind, HandleDragPhase::Move, *position, ctx);
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerUp { position, .. } => {
                        drag_delegate.handle_drag(kind, HandleDragPhase::End, *position, ctx);
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerCancel { .. } => {
                        // Deliberately **not** `window_position`. The three arms
                        // above hand `handle_drag` points in this handle's own
                        // space — the router localises them — while a cancel's
                        // position is window-space by contract, so forwarding it
                        // would credit the same sink a point from a different
                        // frame. `HandleDragPhase::Cancel` discards the point
                        // (`TouchSelection::drag_handle` routes it to
                        // `cancel_drag`, which takes none), so the honest value
                        // is the origin: nothing in the wrong frame flows, and
                        // no reader is deprived of one it could have used.
                        drag_delegate.handle_drag(
                            kind,
                            HandleDragPhase::Cancel,
                            Point::new(0.0, 0.0),
                            ctx,
                        );
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            })
            .on_access_action_request(move |action, _node, data, ctx| {
                if action != accesskit::Action::SetValue {
                    return EventResponse::Ignored;
                }
                let offset = match data {
                    Some(accesskit::ActionData::NumericValue(v)) => v.max(0.0) as usize,
                    Some(accesskit::ActionData::Value(v)) => match v.parse::<usize>() {
                        Ok(parsed) => parsed,
                        Err(_) => return EventResponse::Ignored,
                    },
                    _ => return EventResponse::Ignored,
                };
                action_delegate.set_handle_offset(kind, offset, ctx);
                EventResponse::Handled
            });
        ctx.apply_self_handlers(handlers);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let extent = self.recipe.hit_size;
        proposal.resolve(extent, extent).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some(geometry) = self.geometry() else {
            return;
        };
        let fill = self.recipe.fill.resolve(ctx.theme);
        let radius = self.recipe.diameter / 2.0;
        // The disc is centred in the node, and the node was placed centred on
        // the anchor — but the hit square may have been nudged to stay on
        // screen, so the disc follows the *anchor*, not the node's middle.
        let centre = Point::new(
            geometry
                .anchor
                .x
                .clamp(bounds.x + radius, bounds.right() - radius),
            geometry
                .anchor
                .y
                .clamp(bounds.y + radius, bounds.bottom() - radius),
        );
        if self.recipe.stem_width > 0.0 {
            let stem_x = centre.x - self.recipe.stem_width / 2.0;
            let caret = geometry.caret;
            let (top, bottom) = if centre.y < caret.y {
                (centre.y, caret.y)
            } else {
                (caret.bottom(), centre.y)
            };
            if bottom > top {
                canvas.fill_rect(
                    Rect::new(stem_x, top, self.recipe.stem_width, bottom - top),
                    fill,
                );
            }
        }
        if self.recipe.outline_width > 0.0 {
            canvas.stroke_circle(
                centre,
                radius,
                self.recipe.outline.resolve(ctx.theme),
                self.recipe.outline_width,
            );
        }
        canvas.fill_circle(centre, radius, fill);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(accesskit::Role::Slider);
        builder.set_name(self.default_label());
        if let Some(geometry) = self.geometry() {
            builder.set_numeric_value(geometry.offset as f64);
            builder.set_min_numeric_value(0.0);
            builder.set_max_numeric_value(geometry.document_len as f64);
            builder.set_numeric_value_step(1.0);
        }
        // Not `Focus`: the handle is deliberately outside the Tab ring (see the
        // module docs), so advertising a focus action would promise a stop that
        // does not exist. `SetValue` is the whole AT contract — move this end
        // of the selection to a character offset.
        builder.add_action(accesskit::Action::SetValue);
    }
}

/// The magnifier lens: a framed window onto a magnified replay of the host's
/// own text layer.
pub struct TextMagnifier {
    affordances: TextAffordances,
    recipe: TextMagnifierRecipe,
    painter: Rc<dyn Fn(&mut Canvas, &PaintContext<'_>)>,
}

impl std::fmt::Debug for TextMagnifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextMagnifier").finish()
    }
}

impl TextMagnifier {
    /// `painter` re-emits the host's **text layer only**, in window
    /// coordinates. It is re-entered during the same frame; the contract it
    /// must meet, and what happens when it does not, are in
    /// [`super::magnifier`].
    pub fn new(
        affordances: TextAffordances,
        recipe: TextMagnifierRecipe,
        painter: Rc<dyn Fn(&mut Canvas, &PaintContext<'_>)>,
    ) -> Self {
        Self {
            affordances,
            recipe,
            painter,
        }
    }
}

impl Widget for TextMagnifier {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal
            .resolve(self.recipe.radius * 2.0, self.recipe.half_height * 2.0)
            .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some(request) = self.affordances.magnifier() else {
            return;
        };
        let corner = self.recipe.corner_radius;
        canvas.fill_rounded_rect(
            bounds,
            teksilo_tokens::CornerRadius::uniform(corner),
            self.recipe.background.resolve(ctx.theme),
        );
        // Content first, frame second: the frame's ink is what covers the
        // corners a rectangular clip cannot remove.
        //
        // The transform comes from the request rather than being recomposed
        // here, so there is one copy of the rule to be wrong about. The node is
        // placed *on* `request.lens`, so taking the centre from the request
        // rather than from `bounds` cannot disagree with where the lens sits.
        ctx.replay(canvas, &*self.painter, request.transform(), bounds);
        if self.recipe.border_width > 0.0 {
            canvas.stroke_rounded_rect(
                bounds,
                teksilo_tokens::CornerRadius::uniform(corner),
                self.recipe.border.resolve(ctx.theme),
                self.recipe.border_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A lens shows what is already in the tree. Announcing it would make a
        // screen reader read the same sentence twice.
        builder.set_hidden();
    }
}

/// The overlay content root: every affordance for one text surface.
///
/// Built once, in the host's `build()`. Each child is gated by its own
/// `visible_when`, so a selection that gains or loses a handle is a dormancy
/// flip and a re-place — never a rebuild, which would tear down the node under
/// the finger mid-drag.
pub struct TextAffordanceLayer {
    affordances: TextAffordances,
    handle_recipe: TextSelectionHandleRecipe,
    magnifier_recipe: TextMagnifierRecipe,
    delegate: Rc<dyn TextAffordanceDelegate>,
    painter: Option<Rc<dyn Fn(&mut Canvas, &PaintContext<'_>)>>,
    handles: Vec<(SelectionHandleKind, WidgetId)>,
    magnifier: Option<WidgetId>,
}

impl std::fmt::Debug for TextAffordanceLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextAffordanceLayer")
            .field("handles", &self.handles.len())
            .field("magnifier", &self.magnifier.is_some())
            .finish()
    }
}

impl TextAffordanceLayer {
    pub fn new(
        affordances: TextAffordances,
        handle_recipe: TextSelectionHandleRecipe,
        magnifier_recipe: TextMagnifierRecipe,
        delegate: Rc<dyn TextAffordanceDelegate>,
    ) -> Self {
        Self {
            affordances,
            handle_recipe,
            magnifier_recipe,
            delegate,
            painter: None,
            handles: Vec::new(),
            magnifier: None,
        }
    }

    /// Supply the text-layer painter that fills the lens. Without one there is
    /// no magnifier node at all — the per-surface opt-out, for a host whose
    /// paint cannot be re-entered.
    pub fn magnifier_painter(
        mut self,
        painter: Rc<dyn Fn(&mut Canvas, &PaintContext<'_>)>,
    ) -> Self {
        self.painter = Some(painter);
        self
    }
}

impl Widget for TextAffordanceLayer {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // A handle that moved has to be re-placed, not merely repainted, so the
        // geometry version binds at `Relayout`. It is bumped once per published
        // change, which during a drag is once per pointer sample.
        self.affordances.version_signal().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        self.handles.clear();
        for kind in [
            SelectionHandleKind::Caret,
            SelectionHandleKind::Start,
            SelectionHandleKind::End,
        ] {
            let id = ctx.add(
                SelectionHandle::new(
                    kind,
                    self.affordances.clone(),
                    self.handle_recipe,
                    Rc::clone(&self.delegate),
                )
                .visible_when(self.affordances.handle_visible_signal(kind)),
            );
            self.handles.push((kind, id));
        }
        self.magnifier = self.painter.as_ref().map(|painter| {
            ctx.add(
                TextMagnifier::new(
                    self.affordances.clone(),
                    self.magnifier_recipe,
                    Rc::clone(painter),
                )
                .visible_when(self.affordances.magnifier_visible_signal()),
            )
        });

        let handlers = crate::widget_builder::HandlerSet::new().event_pass_through(true);
        ctx.apply_self_handlers(handlers);

        self.children()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // The layer covers whatever the overlay gives it — a full viewport —
        // and positions its children in window coordinates inside that.
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for placement in children.iter_mut() {
            if let Some((kind, _)) = self.handles.iter().find(|(_, id)| *id == placement.id) {
                if let Some(geometry) = self.affordances.handle(*kind) {
                    placement.origin = geometry.hit.origin();
                    placement.size = geometry.hit.size();
                }
            } else if Some(placement.id) == self.magnifier
                && let Some(request) = self.affordances.magnifier()
            {
                placement.origin = request.lens.origin();
                placement.size = request.lens.size();
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.handles
            .iter()
            .map(|(_, id)| *id)
            .chain(self.magnifier)
            .collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A bare container. Its children carry the semantics; giving it a role
        // would add a traversal stop between the editor and its handles.
        builder.set_role(accesskit::Role::GenericContainer);
    }
}
