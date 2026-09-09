// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch text editing: the contract a text surface implements, and the
//! controller that turns a finger into a selection.
//!
//! # Why this lives in `teksilo-core`
//!
//! Several independent surfaces select text: the rich-text editor, the
//! single-line field, the code editor — and the terminal, whose grid selection
//! is read-only. The terminal deliberately does not depend on
//! `teksilo-widgets`, so a controller that lived there could not serve it. (An
//! embedded `WebView` is not on this list: its engine owns the page's selection
//! and paints its own handles.)
//! Everything a surface needs therefore ships here: the [`TextHitSource`]
//! contract, the [`TouchSelection`] controller, the published affordance
//! geometry, and the affordance widgets. What does *not* ship here is which
//! colours they use — that is a Tier-3 recipe
//! ([`TextSelectionStyle`](crate::styles::TextSelectionStyle)) whose shipped
//! default lives in `teksilo-widgets` with every other `Recipe*Style`.
//!
//! # The mouse is untouched
//!
//! Every **pointer** entry point begins by asking
//! [`PointerKind::is_direct`](teksilo_tokens::PointerKind::is_direct) and
//! returns [`EventResponse::Ignored`](crate::event::EventResponse)
//! without touching any state when the answer is no. A mouse — and a legacy
//! event, which the router reports as the mouse at the tree epoch — therefore
//! reaches none of this code, and an editor that installs the controller edits
//! byte for byte as it did before. A stylus **is** direct and gets the full
//! affordance set: a pen selects text the way a finger does, and its own
//! precision is expressed through its gesture profile rather than by hiding the
//! handles.
//!
//! # Coordinates
//!
//! Every point and rectangle crossing this contract is in **window** logical
//! coordinates — the space `WidgetEvent::PointerDown::position` arrives in and
//! the space an overlay is positioned in. A host whose engine works in
//! document coordinates converts on the way in and on the way out; doing it
//! anywhere else means the handles and the caret disagree the moment the
//! editor is scrolled.
//!
//! # What a host owes
//!
//! See `docs/text-touch-editing.md` for the checklist. In outline: implement
//! [`TextHitSource`], own a [`TouchSelection`], forward pointer events and the
//! long press to it, mount a [`TextAffordanceLayer`] in the
//! [`TextAffordance`](crate::overlay::OverlayBand::TextAffordance) band, and
//! dismiss the controller when focus, content or read-only status changes.

pub mod affordance_layer;
pub mod magnifier;

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_tokens::{InputTokens, TargetRole};

use crate::environment::LayoutDirection;
use crate::event::{EventResponse, WidgetEvent};
use crate::overlay::direction::HorizontalSide;
use crate::overlay::{OverlayPlacement, SelectionHandleKind};
use crate::signal::Signal;
use crate::styles::TextSelectionHandleRecipe;
use crate::styles::density::dp;
use crate::widget::EventContext;

pub use affordance_layer::{
    SelectionHandle, TextAffordanceDelegate, TextAffordanceLayer, TextMagnifier,
};
pub use magnifier::MagnifierRequest;

/// Diameter of a selection handle's painted disc, in dp.
///
/// A `Decoration` dimension: the same at every density. Conformance is carried
/// by [`HANDLE_HIT_SIZE`], which is what the pointer actually meets.
pub const HANDLE_DIAMETER: f32 = 24.0;

/// Extent of a selection handle's square hit rectangle, in dp — a `Target`
/// dimension, so a denser ladder can only widen it.
pub const HANDLE_HIT_SIZE: f32 = 44.0;

/// Width of the stem drawn from a handle's disc to the caret it marks, in dp.
pub const HANDLE_STEM_WIDTH: f32 = 2.0;

/// One command a selection toolbar may offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextAction {
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// A host-defined command, named by a stable key the host resolves to a
    /// label and a handler — "Look Up", "Translate", "Add to dictionary".
    Custom(&'static str),
}

/// Which clipboard commands a surface will honour **right now**.
///
/// Derived rather than declared: the default
/// [`TextHitSource::clipboard_actions`] computes every field from
/// `is_editable`, `allows_copy` and the current selection, so a surface cannot
/// offer a Cut it would refuse. Override it only to add or remove a command for
/// a reason those three do not capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClipboardActions {
    pub cut: bool,
    pub copy: bool,
    pub paste: bool,
    pub select_all: bool,
}

impl ClipboardActions {
    /// The commands as a toolbar orders them: destructive first, then the two
    /// that add, then the one that widens.
    pub fn to_actions(self) -> Vec<TextAction> {
        let mut actions = Vec::new();
        if self.cut {
            actions.push(TextAction::Cut);
        }
        if self.copy {
            actions.push(TextAction::Copy);
        }
        if self.paste {
            actions.push(TextAction::Paste);
        }
        if self.select_all {
            actions.push(TextAction::SelectAll);
        }
        actions
    }
}

/// The geometry and selection state of one text surface, as the touch
/// controller needs to see it.
///
/// # Relationship to [`TextSurface`](crate::text_surface::TextSurface)
///
/// The two do not overlap and neither subsumes the other. `TextSurface` answers
/// *"perform this command"* — undo, cut, paste — for a host that routes a
/// chord or a menu row into whatever has focus. This trait answers *"where is
/// the text"*, which is what a finger needs. The one place they meet is
/// [`clipboard_actions`](Self::clipboard_actions), which says which commands to
/// **offer**; invoking them stays with `TextSurface`, so there is exactly one
/// implementation of each command.
pub trait TextHitSource {
    /// The text offset nearest `point`.
    fn offset_at(&self, point: Point) -> usize;

    /// The caret rectangle at `offset` — a thin, line-height-tall rectangle.
    /// Handles hang off it, so a host that returns a zero-height rectangle gets
    /// handles that sit on the baseline.
    fn caret_rect(&self, offset: usize) -> Rect;

    /// The word containing `offset`, for the long-press selection.
    fn word_range_at(&self, offset: usize) -> Range<usize>;

    /// The line containing `offset`.
    fn line_range_at(&self, offset: usize) -> Range<usize>;

    /// The current selection, as a half-open offset range. An empty range is a
    /// collapsed caret.
    fn selection(&self) -> Range<usize>;

    /// Move the selection. Called on every sample of a handle drag, so an
    /// implementation that rebuilds the document here will be felt.
    fn set_selection(&mut self, range: Range<usize>);

    /// The selection's bounding rectangle, or `None` when nothing is selected.
    ///
    /// Deliberately one rectangle and not a list of per-line rectangles: the
    /// only consumer is the toolbar's
    /// [`AboveSelection`](crate::overlay::OverlayPlacement::AboveSelection)
    /// placement, which wants the block to hang above, and the engines behind
    /// the shipped editors expose a union box rather than per-line geometry. A
    /// surface that has per-line rectangles should still return their union.
    fn selection_bounds(&self) -> Option<Rect>;

    /// The visible rectangle of this surface. Affordances are clamped into it,
    /// and a caret outside it has no handle.
    fn viewport(&self) -> Rect;

    /// The largest offset a caret may take — the length of the text.
    ///
    /// Not geometry, and not in the original contract: a handle is exposed to
    /// assistive technology as a slider, and a slider without a maximum
    /// announces a position out of nothing. Every engine behind the shipped
    /// editors knows this number.
    fn document_len(&self) -> usize;

    /// Does this surface accept edits? A read-only surface shows no caret
    /// handle and offers only what it can honour.
    fn is_editable(&self) -> bool;

    /// May its contents leave the process at all? A password field says no, and
    /// then neither Cut nor Copy is offered.
    fn allows_copy(&self) -> bool {
        true
    }

    /// Which clipboard commands to offer for the current state. Derived from
    /// the three questions above; see [`ClipboardActions`].
    fn clipboard_actions(&self) -> ClipboardActions {
        let editable = self.is_editable();
        let has_selection = !self.selection().is_empty();
        ClipboardActions {
            cut: editable && has_selection && self.allows_copy(),
            copy: has_selection && self.allows_copy(),
            paste: editable,
            // Widening a selection is only an offer while there is something
            // left to widen to; with a range already up, the toolbar's other
            // commands are what the user came for.
            select_all: !has_selection,
        }
    }
}

/// The painted and hittable geometry of one selection handle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionHandleGeometry {
    /// Which handle this is, in **logical** order.
    pub kind: SelectionHandleKind,
    /// The text offset this handle marks — the slider value assistive
    /// technology reads and writes.
    pub offset: usize,
    /// The largest offset the surface has, so the slider node has a maximum.
    pub document_len: usize,
    /// Which physical side of the selection it is on, resolved once through
    /// [`SelectionHandleKind::side`] so a mirror can never be applied twice.
    /// `None` for the caret handle, which has no side.
    pub side: Option<HorizontalSide>,
    /// The caret rectangle this handle marks.
    pub caret: Rect,
    /// Centre of the painted disc.
    pub anchor: Point,
    /// The painted disc's bounding square.
    pub visual: Rect,
    /// The square that accepts the press. Never smaller than the disc, and
    /// nudged so as much of it as possible lies inside the viewport.
    pub hit: Rect,
}

/// The two dimensions a handle is built from. Taken from the active
/// [`TextSelectionStyle`](crate::styles::TextSelectionStyle) so the controller
/// that hit-tests a handle and the widget that paints it cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandleMetrics {
    /// Diameter of the painted disc.
    pub diameter: f32,
    /// Extent of the square hit rectangle.
    pub hit: f32,
}

impl HandleMetrics {
    /// The shipped dimensions projected onto a density ladder.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            diameter: dp(HANDLE_DIAMETER, TargetRole::Decoration, tokens),
            hit: dp(HANDLE_HIT_SIZE, TargetRole::Target, tokens),
        }
    }
}

impl Default for HandleMetrics {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

impl From<&TextSelectionHandleRecipe> for HandleMetrics {
    fn from(recipe: &TextSelectionHandleRecipe) -> Self {
        Self {
            diameter: recipe.diameter,
            hit: recipe.hit_size,
        }
    }
}

/// The selection toolbar the controller wants raised.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionToolbarRequest {
    /// The commands to offer, in toolbar order.
    pub actions: Vec<TextAction>,
    /// The rectangle to hang above — the selection's bounds, or the caret's
    /// when nothing is selected.
    pub anchor: Rect,
}

impl SelectionToolbarRequest {
    /// The placement to raise this toolbar with.
    ///
    /// [`AboveSelection`](OverlayPlacement::AboveSelection) already flips below
    /// when the selection is against the top of the usable area, so a host
    /// needs no fallback of its own.
    pub fn placement(&self) -> OverlayPlacement {
        OverlayPlacement::AboveSelection {
            selection: self.anchor,
        }
    }
}

// ---------------------------------------------------------------------------
// Published affordance state
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq)]
struct AffordanceState {
    handles: Vec<SelectionHandleGeometry>,
    magnifier: Option<MagnifierRequest>,
    toolbar: Option<SelectionToolbarRequest>,
}

/// A cloneable view of what a [`TouchSelection`] currently wants shown.
///
/// The handle and magnifier widgets read this rather than the controller, so
/// the affordance layer can be mounted once, in an overlay, while the
/// controller stays inside the editor that owns the text. Clone it to share;
/// every clone sees the same state.
#[derive(Clone)]
pub struct TextAffordances {
    inner: Rc<RefCell<AffordanceState>>,
    version: Signal<u64>,
}

impl Default for TextAffordances {
    fn default() -> Self {
        Self {
            inner: Rc::new(RefCell::new(AffordanceState::default())),
            version: Signal::new(0),
        }
    }
}

impl std::fmt::Debug for TextAffordances {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.inner.borrow();
        f.debug_struct("TextAffordances")
            .field("handles", &state.handles.len())
            .field("magnifier", &state.magnifier.is_some())
            .field("toolbar", &state.toolbar.is_some())
            .finish()
    }
}

impl TextAffordances {
    /// A view with nothing shown.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bumped whenever anything below changes. Bind it at
    /// [`Relayout`](crate::binding::BindingLevel::Relayout) — a handle that
    /// moved has to be re-placed, not merely repainted.
    pub fn version_signal(&self) -> Signal<u64> {
        self.version.clone()
    }

    /// Every handle currently wanted, in `Caret` / `Start` / `End` order.
    pub fn handles(&self) -> Vec<SelectionHandleGeometry> {
        self.inner.borrow().handles.clone()
    }

    /// The handle of a given kind, if it is wanted.
    pub fn handle(&self, kind: SelectionHandleKind) -> Option<SelectionHandleGeometry> {
        self.inner
            .borrow()
            .handles
            .iter()
            .find(|h| h.kind == kind)
            .copied()
    }

    /// Whether a handle of `kind` is wanted — for a `visible_when` gate.
    pub fn handle_visible_signal(&self, kind: SelectionHandleKind) -> Signal<bool> {
        let inner = Rc::clone(&self.inner);
        self.version
            .map(move |_| inner.borrow().handles.iter().any(|h| h.kind == kind))
    }

    /// The magnifier, while one is raised.
    pub fn magnifier(&self) -> Option<MagnifierRequest> {
        self.inner.borrow().magnifier
    }

    /// Whether a magnifier is raised — for a `visible_when` gate.
    pub fn magnifier_visible_signal(&self) -> Signal<bool> {
        let inner = Rc::clone(&self.inner);
        self.version
            .map(move |_| inner.borrow().magnifier.is_some())
    }

    /// The toolbar the controller wants raised, if any.
    pub fn toolbar(&self) -> Option<SelectionToolbarRequest> {
        self.inner.borrow().toolbar.clone()
    }

    /// Whether anything at all is shown.
    pub fn is_empty(&self) -> bool {
        let state = self.inner.borrow();
        state.handles.is_empty() && state.magnifier.is_none() && state.toolbar.is_none()
    }

    /// Replace the published state and notify, dropping the mutable borrow
    /// **before** the notification so an observer may read it back.
    fn publish(&self, next: AffordanceState) {
        {
            let mut state = self.inner.borrow_mut();
            if *state == next {
                return;
            }
            *state = next;
        }
        let version = self.version.get();
        self.version.set(version.wrapping_add(1));
    }
}

// ---------------------------------------------------------------------------
// The controller
// ---------------------------------------------------------------------------

/// Which end of a handle drag a sample is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleDragPhase {
    Begin,
    Move,
    End,
    Cancel,
}

#[derive(Debug, Clone, Copy)]
struct HandleDrag {
    /// Which handle the finger grabbed. Read only to tell a caret drag from a
    /// range drag — the two branches of [`TouchSelection::update_drag`] — so it
    /// is *not* re-pointed when the drag crosses the other end: which end the
    /// finger then holds is derived from the resulting range, not recorded here.
    kind: SelectionHandleKind,
    /// The offset that stays put — the *other* end of the selection.
    fixed: usize,
}

/// Turns a direct pointer into a text selection: long press to select a word,
/// handles to adjust it, a magnifier while adjusting, a toolbar when done.
///
/// A host owns one of these per text surface, forwards pointer events to it,
/// and mounts a [`TextAffordanceLayer`] fed by [`affordances`](Self::affordances).
pub struct TouchSelection {
    metrics: HandleMetrics,
    magnifier_radius: f32,
    magnifier_half_height: f32,
    magnifier_rise: f32,
    magnifier_scale: f32,
    magnifier_enabled: bool,
    reduced_motion: bool,
    affordances: TextAffordances,
    drag: Option<HandleDrag>,
    /// Whether affordances have been raised at all. A surface that has never
    /// been touched shows nothing, even though it has a caret.
    raised: bool,
}

impl std::fmt::Debug for TouchSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TouchSelection")
            .field("metrics", &self.metrics)
            .field("magnifier_enabled", &self.magnifier_enabled)
            .field("reduced_motion", &self.reduced_motion)
            .field("dragging", &self.drag.is_some())
            .field("raised", &self.raised)
            .finish()
    }
}

impl Default for TouchSelection {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchSelection {
    /// A controller with the shipped Compact metrics, the magnifier on, and
    /// motion unrestricted.
    ///
    /// Build it in `build()` and refine it there:
    /// [`reduced_motion`](Self::reduced_motion) has no accessor on
    /// [`EventContext`], so the value must be captured while a
    /// [`BuildContext`](crate::build_context::BuildContext) is in hand.
    pub fn new() -> Self {
        Self {
            metrics: HandleMetrics::default(),
            magnifier_radius: magnifier::MAGNIFIER_RADIUS,
            magnifier_half_height: magnifier::MAGNIFIER_HALF_HEIGHT,
            magnifier_rise: magnifier::MAGNIFIER_RISE,
            magnifier_scale: magnifier::MAGNIFIER_SCALE,
            magnifier_enabled: true,
            reduced_motion: false,
            affordances: TextAffordances::new(),
            drag: None,
            raised: false,
        }
    }

    /// Use `metrics` for handle geometry. Pass the ones derived from the same
    /// [`TextSelectionHandleRecipe`] the affordance layer paints with.
    pub fn metrics(mut self, metrics: HandleMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Lens geometry, from the active style's
    /// [`TextMagnifierRecipe`](crate::styles::TextMagnifierRecipe).
    pub fn magnifier_metrics(
        mut self,
        radius: f32,
        half_height: f32,
        rise: f32,
        scale: f32,
    ) -> Self {
        self.magnifier_radius = radius;
        self.magnifier_half_height = half_height;
        self.magnifier_rise = rise;
        self.magnifier_scale = scale;
        self
    }

    /// Turn the magnifier off for this surface — the per-widget opt-out.
    ///
    /// A surface whose text layer cannot be replayed purely (see
    /// [`magnifier`]) must set this, because there is no way for the framework
    /// to detect that it could not.
    pub fn magnifier(mut self, enabled: bool) -> Self {
        self.magnifier_enabled = enabled;
        self
    }

    /// Whether the user asked for reduced motion. Gates the magnifier
    /// entirely: a lens that appears and then chases the finger is exactly the
    /// unbidden movement the preference is about, and the selection works
    /// without it.
    pub fn reduced_motion(mut self, reduced: bool) -> Self {
        self.reduced_motion = reduced;
        self
    }

    /// The published geometry. Clone it into the affordance layer.
    pub fn affordances(&self) -> TextAffordances {
        self.affordances.clone()
    }

    /// Every handle currently wanted.
    pub fn handles(&self) -> Vec<SelectionHandleGeometry> {
        self.affordances.handles()
    }

    /// The magnifier, while one is raised.
    pub fn magnifier_request(&self) -> Option<MagnifierRequest> {
        self.affordances.magnifier()
    }

    /// The toolbar the controller wants raised, if any.
    pub fn toolbar(&self) -> Option<SelectionToolbarRequest> {
        self.affordances.toolbar()
    }

    /// Whether a handle drag is in progress.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Retract every affordance. The controller owns its own lifetime: the
    /// text-affordance band is exempt from outside-press dismissal, so nothing
    /// else will do this. Call it when focus leaves, the content changes, the
    /// surface becomes read-only, or the window deactivates.
    pub fn dismiss(&mut self) {
        self.drag = None;
        self.raised = false;
        self.affordances.publish(AffordanceState::default());
    }

    /// Tell the platform where the caret is, so an on-screen keyboard's
    /// candidate window does not cover it.
    ///
    /// Reports the area only; it deliberately does not ask for the keyboard.
    /// Re-asserting IME allowance cancels a live composition, and a touch
    /// caret placement during composition must preserve the preedit.
    pub fn report_ime_area(&self, ctx: &mut EventContext<'_>, source: &dyn TextHitSource) {
        ctx.set_ime_cursor_area(source.caret_rect(source.selection().end));
    }

    /// Recompute every affordance from `source`.
    ///
    /// Call it after any change the controller did not make — an arrow key, an
    /// undo, a scroll that moved the caret — or the handles keep the position
    /// the text used to be at.
    pub fn refresh(&mut self, direction: LayoutDirection, source: &dyn TextHitSource) {
        if !self.raised {
            return;
        }
        self.affordances.publish(AffordanceState {
            handles: self.compute_handles(direction, source),
            toolbar: self.compute_toolbar(source),
            // A lens exists only while a finger is on a handle, and every path
            // into `refresh` is a path out of that.
            magnifier: None,
        });
    }

    /// Raise the affordances for the current selection.
    ///
    /// The host calls this when a direct pointer finishes placing a caret: the
    /// caret placement itself stays with the host's own code, which is what
    /// keeps a mouse's path unchanged.
    pub fn raise(&mut self, direction: LayoutDirection, source: &dyn TextHitSource) {
        self.raised = true;
        self.refresh(direction, source);
    }

    /// Select the word under `point` and raise the affordances — the long-press
    /// gesture.
    ///
    /// Returns [`Ignored`](EventResponse::Ignored) untouched for an indirect
    /// pointer. That branch is load-bearing: the gesture arena installs a
    /// long-press recognizer on the presence of the handler alone, with no
    /// pointer-kind condition, so without it a half-second mouse hold inside an
    /// editor would select a word.
    ///
    /// # Why the device is a parameter here and not read off `ctx`
    ///
    /// [`handle_pointer`](Self::handle_pointer) and
    /// [`drag_handle`](Self::drag_handle) ask `ctx.pointer_kind()`, and they are
    /// right to: both serve a **sample**, and the tree installs that sample's
    /// pointer for the length of the dispatch. A hold serves no sample — it is a
    /// deadline coming due — and the answer a context can give for it is only as
    /// good as the tree's bookkeeping at tick time. It was wrong for the whole
    /// of this method's first life: `current_input` is saved-and-restored around
    /// every dispatch, so the timer path read back `InputSnapshot::default()`
    /// and this guard refused **every** genuine touch hold, which made the entry
    /// point dead code and cost its first host a duplicate guard of its own.
    /// The tree now installs the holding contact
    /// (`InputSnapshot::for_recognized_gesture`), so `ctx` answers correctly
    /// too — but the gesture already carries the truth on
    /// [`TapEvent::pointer`](crate::gesture::TapEvent::pointer), and a host that
    /// drives this from anywhere else — an assistive-technology action, its own
    /// hold timer — has no snapshot behind it at all. So the caller names the
    /// device.
    ///
    /// `point` is in **window** coordinates, like every other point this type
    /// takes — deliberately *not* `TapEvent::position`, which the router has
    /// already rewritten into the target's local space.
    pub fn on_long_press(
        &mut self,
        pointer: crate::pointer::PointerInfo,
        point: Point,
        ctx: &mut EventContext<'_>,
        source: &mut dyn TextHitSource,
    ) -> EventResponse {
        if !pointer.kind.is_direct() {
            return EventResponse::Ignored;
        }
        let offset = source.offset_at(point);
        let word = source.word_range_at(offset);
        source.set_selection(word);
        self.raised = true;
        self.refresh(ctx.layout_direction(), source);
        EventResponse::Handled
    }

    /// Route one pointer event.
    ///
    /// The host installs this on the editor itself. It claims only what belongs
    /// to the affordances: a press that lands on a handle, and the samples of a
    /// drag it started. Everything else — including every event from an
    /// indirect pointer — is [`Ignored`](EventResponse::Ignored), so the host's
    /// own caret placement runs exactly as it did.
    ///
    /// A host that mounts the [`TextAffordanceLayer`] gets handle presses
    /// through the layer's own nodes, which sit above the editor and are
    /// offered the press first; the handle branch here is what serves a host
    /// that paints its own handles instead.
    pub fn handle_pointer(
        &mut self,
        event: &WidgetEvent,
        ctx: &mut EventContext<'_>,
        source: &mut dyn TextHitSource,
    ) -> EventResponse {
        if !ctx.pointer_kind().is_direct() {
            return EventResponse::Ignored;
        }
        let direction = ctx.layout_direction();
        match event {
            WidgetEvent::PointerDown { position, .. } => {
                match self.handle_at(*position) {
                    Some(kind) => {
                        self.begin_drag(kind, *position, direction, source);
                        ctx.capture_pointer();
                        EventResponse::Handled
                    }
                    // Not on a handle: the host places the caret, and the
                    // affordances go away until the press resolves.
                    None => EventResponse::Ignored,
                }
            }
            WidgetEvent::PointerMove { position } if self.drag.is_some() => {
                self.update_drag(*position, direction, source);
                EventResponse::Handled
            }
            WidgetEvent::PointerUp { position, .. } => {
                if self.drag.is_some() {
                    self.end_drag(*position, direction, source);
                    EventResponse::Handled
                } else {
                    self.raise(direction, source);
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerCancel { .. } if self.drag.is_some() => {
                self.cancel_drag(direction, source);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    /// Drive a handle drag from the affordance layer's own node.
    ///
    /// The layer knows which handle was pressed — it is a widget in its own
    /// right — so it names the kind instead of making the controller hit-test
    /// for it.
    ///
    /// An indirect pointer is refused here as well as at the layer's node and in
    /// [`handle_pointer`](Self::handle_pointer). The three guards cover three
    /// different ways in, and this is the one a caller of the public API reaches
    /// without passing either of the others.
    pub fn drag_handle(
        &mut self,
        kind: SelectionHandleKind,
        phase: HandleDragPhase,
        point: Point,
        ctx: &mut EventContext<'_>,
        source: &mut dyn TextHitSource,
    ) -> EventResponse {
        if !ctx.pointer_kind().is_direct() {
            return EventResponse::Ignored;
        }
        let direction = ctx.layout_direction();
        match phase {
            HandleDragPhase::Begin => self.begin_drag(kind, point, direction, source),
            HandleDragPhase::Move => self.update_drag(point, direction, source),
            HandleDragPhase::End => self.end_drag(point, direction, source),
            HandleDragPhase::Cancel => self.cancel_drag(direction, source),
        }
        EventResponse::Handled
    }

    /// Which handle, if any, accepts a press at `point`.
    ///
    /// Handles overlap when a selection is short, so this answers with the one
    /// whose centre is nearest rather than the first in the list — the same
    /// tie-break the hit-test slop pass uses between adjacent grips.
    pub fn handle_at(&self, point: Point) -> Option<SelectionHandleKind> {
        self.affordances
            .handles()
            .into_iter()
            .filter(|h| h.hit.contains(point))
            .min_by(|a, b| {
                distance_squared(a.anchor, point).total_cmp(&distance_squared(b.anchor, point))
            })
            .map(|h| h.kind)
    }

    // -- drag -------------------------------------------------------------

    fn begin_drag(
        &mut self,
        kind: SelectionHandleKind,
        point: Point,
        direction: LayoutDirection,
        source: &mut dyn TextHitSource,
    ) {
        let selection = source.selection();
        let fixed = match kind {
            SelectionHandleKind::Start => selection.end,
            SelectionHandleKind::End => selection.start,
            SelectionHandleKind::Caret => selection.start,
        };
        self.drag = Some(HandleDrag { kind, fixed });
        self.raised = true;
        self.update_drag(point, direction, source);
    }

    fn update_drag(
        &mut self,
        point: Point,
        direction: LayoutDirection,
        source: &mut dyn TextHitSource,
    ) {
        let Some(drag) = self.drag else {
            return;
        };
        let moving = source.offset_at(point);
        let dragging_caret = drag.kind == SelectionHandleKind::Caret;
        if dragging_caret {
            source.set_selection(moving..moving);
        } else {
            // The selection is whatever lies between the end that stays put and
            // the finger, so dragging one end past the other grows the range on
            // the far side instead of collapsing it. Which *kind* of handle is
            // then under the finger is symmetric in the crossing direction and
            // follows from the range alone — `compute_handles` reads the offsets
            // back out, so the finger holds the start once its offset is below
            // the fixed end's and the end once it is above. Nothing records the
            // crossing: `drag.kind` is read only to tell these two branches
            // apart, and crossing over cannot turn a range drag into a caret
            // drag.
            source.set_selection(drag.fixed.min(moving)..drag.fixed.max(moving));
        }
        let mut handles = self.compute_handles(direction, source);
        if !dragging_caret {
            // Dragging one end exactly onto the other empties the selection,
            // and an empty selection would otherwise be published as a caret
            // handle — a third target appearing under the finger mid-gesture.
            // The two ends stay up until the finger lifts.
            handles.retain(|h| h.kind != SelectionHandleKind::Caret);
        }
        self.affordances.publish(AffordanceState {
            handles,
            magnifier: self.compute_magnifier(point, source),
            // A toolbar over the text being adjusted is in the way, and its
            // commands would be aimed at a selection that is still moving.
            toolbar: None,
        });
    }

    fn end_drag(
        &mut self,
        point: Point,
        direction: LayoutDirection,
        source: &mut dyn TextHitSource,
    ) {
        if self.drag.is_some() {
            self.update_drag(point, direction, source);
        }
        self.drag = None;
        self.refresh(direction, source);
    }

    fn cancel_drag(&mut self, direction: LayoutDirection, source: &mut dyn TextHitSource) {
        self.drag = None;
        self.refresh(direction, source);
    }

    // -- geometry ---------------------------------------------------------

    fn compute_handles(
        &self,
        direction: LayoutDirection,
        source: &dyn TextHitSource,
    ) -> Vec<SelectionHandleGeometry> {
        let viewport = source.viewport();
        let document_len = source.document_len();
        let selection = source.selection();
        let kinds: &[(SelectionHandleKind, usize)] = &if selection.is_empty() {
            // A read-only surface has no caret to place, so it gets no caret
            // handle — only the two that adjust a selection.
            if source.is_editable() {
                vec![(SelectionHandleKind::Caret, selection.start)]
            } else {
                vec![]
            }
        } else {
            vec![
                (SelectionHandleKind::Start, selection.start),
                (SelectionHandleKind::End, selection.end),
            ]
        };
        kinds
            .iter()
            .filter_map(|&(kind, offset)| {
                handle_geometry(
                    kind,
                    offset,
                    document_len,
                    source.caret_rect(offset),
                    direction,
                    viewport,
                    self.metrics,
                )
            })
            .collect()
    }

    fn compute_toolbar(&self, source: &dyn TextHitSource) -> Option<SelectionToolbarRequest> {
        let actions = source.clipboard_actions().to_actions();
        if actions.is_empty() {
            return None;
        }
        let anchor = source
            .selection_bounds()
            .unwrap_or_else(|| source.caret_rect(source.selection().end));
        Some(SelectionToolbarRequest { actions, anchor })
    }

    fn compute_magnifier(
        &self,
        point: Point,
        source: &dyn TextHitSource,
    ) -> Option<MagnifierRequest> {
        if !self.magnifier_enabled || self.reduced_motion {
            return None;
        }
        Some(MagnifierRequest::new(
            point,
            source.viewport(),
            self.magnifier_radius,
            self.magnifier_half_height,
            self.magnifier_rise,
            self.magnifier_scale,
        ))
    }
}

fn distance_squared(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

/// The geometry of one handle hanging off `caret`, or `None` when the caret is
/// not visible in `viewport` at all.
///
/// The disc hangs **above** the line for a `Start` handle and **below** it for
/// `End` and `Caret`, so the two ends of a one-line selection do not sit on top
/// of each other. When the preferred side does not fit in the viewport the
/// handle takes the other one — a handle under the last line of a surface whose
/// bottom is the window's bottom would otherwise be off-screen, which is the
/// case this rule exists for.
pub fn handle_geometry(
    kind: SelectionHandleKind,
    offset: usize,
    document_len: usize,
    caret: Rect,
    direction: LayoutDirection,
    viewport: Rect,
    metrics: HandleMetrics,
) -> Option<SelectionHandleGeometry> {
    if !rects_intersect(caret, viewport) {
        return None;
    }
    let radius = metrics.diameter / 2.0;
    let cx = caret.x + caret.width / 2.0;
    let above = Point::new(cx, caret.y - radius);
    let below = Point::new(cx, caret.bottom() + radius);
    let (preferred, alternate) = match kind {
        SelectionHandleKind::Start => (above, below),
        SelectionHandleKind::End | SelectionHandleKind::Caret => (below, above),
    };
    let fits = |p: Point| p.y - radius >= viewport.y && p.y + radius <= viewport.bottom();
    let anchor = if fits(preferred) {
        preferred
    } else if fits(alternate) {
        alternate
    } else {
        preferred
    };
    let visual = square_centred_on(anchor, metrics.diameter);
    // The hit square may slide, but never so far that the disc leaves it: a
    // press on the ink the user aimed at must always land.
    let slack = ((metrics.hit - metrics.diameter) / 2.0).max(0.0);
    let hit = nudge_into(square_centred_on(anchor, metrics.hit), viewport, slack);
    Some(SelectionHandleGeometry {
        kind,
        offset,
        document_len,
        side: kind.side(direction),
        caret,
        anchor,
        visual,
        hit,
    })
}

fn square_centred_on(centre: Point, extent: f32) -> Rect {
    Rect::new(
        centre.x - extent / 2.0,
        centre.y - extent / 2.0,
        extent,
        extent,
    )
}

/// `rect` moved toward `bounds` by at most `slack` on each axis.
fn nudge_into(rect: Rect, bounds: Rect, slack: f32) -> Rect {
    let dx = if rect.x < bounds.x {
        (bounds.x - rect.x).min(slack)
    } else if rect.right() > bounds.right() {
        -((rect.right() - bounds.right()).min(slack))
    } else {
        0.0
    };
    let dy = if rect.y < bounds.y {
        (bounds.y - rect.y).min(slack)
    } else if rect.bottom() > bounds.bottom() {
        -((rect.bottom() - bounds.bottom()).min(slack))
    } else {
        0.0
    };
    Rect::new(rect.x + dx, rect.y + dy, rect.width, rect.height)
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

#[cfg(test)]
mod tests;
