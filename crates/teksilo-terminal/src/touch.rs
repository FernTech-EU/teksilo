// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch text selection for the terminal: the [`TextHitSource`] the cell grid
//! answers with, and the controller mount that owns its affordances.
//!
//! # The document is the visible screen, and its offsets are cells
//!
//! [`teksilo_core::text_touch`] is stated over a flat offset space. A terminal
//! has no document — it has a ring of lines of which `rows` are on screen — so
//! this module defines one: **`offset = line * columns + column`**, over the
//! visible viewport only, `document_len() == columns * rows`. Two consequences
//! are worth stating rather than discovering:
//!
//! * an offset is a **cell boundary**, not a character. `offset_at` *rounds* to
//!   the nearest boundary where a proportional editor floors to a glyph, which
//!   is what makes a dragged handle snap to the cell grid instead of hovering
//!   between two columns. In a monospace grid that is the only snapping there
//!   is to do, and doing it here means the controller, the painted handle and
//!   the engine's selection all agree on one number;
//! * the offsets are **viewport** offsets, so they name different text after
//!   the ring scrolls. Anything that scrolls the view therefore retires the
//!   affordances rather than trying to follow them (see
//!   [`TerminalTouch::dismiss`]); new *output* does not — the engine keeps its
//!   selection in buffer coordinates and the snapshot re-projects it, so a
//!   content change is a [`refresh`](TerminalTouch::refresh).
//!
//! # What the terminal does not take from the contract
//!
//! [`is_editable`](TextHitSource::is_editable) is `false`. A terminal has no
//! caret of its own to place — the cursor belongs to the child program, which
//! moves it — so there is no caret handle and no Cut. It does accept a paste,
//! which the derived [`clipboard_actions`](TextHitSource::clipboard_actions)
//! would refuse for a non-editable surface, so that one method is overridden.
//!
//! No selection **toolbar** is raised either. The terminal has one menu, and it
//! is the context menu ([`crate::menu`]): a hold opens it, a right-click opens
//! it, and it carries Copy / Paste / Select all / Clear. Raising a second,
//! nearly-identical bar at the end of a handle drag would be two mechanisms for
//! one job — `TouchSelection::toolbar()` is simply not consumed, which the
//! contract allows in as many words ("whether to *raise* one is the host's").

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::{Rc, Weak};

use teksilo_canvas::{Canvas, Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::event::{EventResponse, WidgetEvent};
use teksilo_core::overlay::{
    DismissBehavior, OverlayBand, OverlayLayer, OverlayPlacement, OverlayRequest,
    SelectionHandleKind,
};
use teksilo_core::styles::density::dp;
use teksilo_core::styles::{
    RecipeColor, TextMagnifierRecipe, TextSelectionHandleRecipe, TextSelectionStyle, Theme,
};
use teksilo_core::text_touch::{
    ClipboardActions, HANDLE_DIAMETER, HANDLE_HIT_SIZE, HANDLE_STEM_WIDTH, HandleDragPhase,
    HandleMetrics, TextAffordanceDelegate, TextAffordanceLayer, TextHitSource, TouchSelection,
    magnifier,
};
use teksilo_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, InputTokens, SurfaceRole, TargetRole};

use crate::engine::{CellSide, SelectionKind};
use crate::state::TerminalState;

/// The closure that re-emits the grid layer into the magnifier lens.
///
/// Named because the type is the same in three places — the host's field, its
/// constructor's parameter, and the widget method that builds one — and because
/// spelling it out three times is what `clippy::type_complexity` is for.
pub(crate) type MagnifierPainter = Rc<dyn Fn(&mut Canvas, &PaintContext<'_>)>;

/// One terminal's visible grid, as the touch controller needs to see it.
///
/// Borrows the state rather than living on it: every controller entry point
/// wants `&mut TouchSelection` and `&mut dyn TextHitSource` at once, so a
/// controller stored *inside* [`TerminalState`] would need two overlapping
/// borrows of one `RefCell` on every call. The controller is a sibling.
pub(crate) struct TerminalHitSource<'a> {
    st: &'a mut TerminalState,
}

impl<'a> TerminalHitSource<'a> {
    pub(crate) fn new(st: &'a mut TerminalState) -> Self {
        Self { st }
    }

    fn cols(&self) -> usize {
        self.st.cols.max(1)
    }

    fn rows(&self) -> usize {
        self.st.rows.max(1)
    }

    /// `(line, column)` for a flat offset. The column may be `columns`, which
    /// is the boundary past the last cell of that line.
    fn cell_of(&self, offset: usize) -> (usize, usize) {
        let cols = self.cols();
        let line = (offset / cols).min(self.rows() - 1);
        (line, offset - line * cols)
    }

    fn offset_of(&self, line: usize, column: usize) -> usize {
        line * self.cols() + column
    }

    /// The text of one visible row, one `String` per column so a column index
    /// is an index. A wide glyph's spacer half contributes an empty string, so
    /// it is classified as blank and never splits a word on its own.
    fn row_cells(&self, line: usize) -> Vec<String> {
        (0..self.cols())
            .map(|col| match self.st.snapshot.cell(line, col) {
                Some(cell) if !cell.attrs.wide_spacer => cell.text(),
                _ => String::new(),
            })
            .collect()
    }
}

/// Whether a cell's text counts as part of a word.
fn is_word_cell(text: &str) -> bool {
    !text.is_empty() && !text.chars().all(|c| c.is_whitespace() || c == '\0')
}

impl TextHitSource for TerminalHitSource<'_> {
    /// The nearest **cell boundary** to `point`.
    ///
    /// Rounds on the horizontal axis and floors on the vertical: a boundary is
    /// between two columns but a line has no half. This is the snapping the
    /// handles inherit.
    fn offset_at(&self, point: Point) -> usize {
        let cw = self.st.metrics.width.max(1.0);
        let ch = self.st.metrics.height.max(1.0);
        let x = ((point.x - self.st.origin.x) / cw).round();
        let y = ((point.y - self.st.origin.y) / ch).floor();
        let col = (x.max(0.0) as usize).min(self.cols());
        let row = (y.max(0.0) as usize).min(self.rows() - 1);
        self.offset_of(row, col)
    }

    /// A one-pixel-wide, line-tall rectangle **centred on** the cell boundary,
    /// so the handle that hangs off it (`caret.x + caret.width / 2.0`) sits
    /// exactly on the grid line. A zero-width rectangle would put the handle in
    /// the same place but would fail `rects_intersect` against a viewport whose
    /// left edge it touches, and the handle would vanish at column 0.
    fn caret_rect(&self, offset: usize) -> Rect {
        let cw = self.st.metrics.width.max(1.0);
        let ch = self.st.metrics.height.max(1.0);
        let (line, col) = self.cell_of(offset);
        Rect::new(
            self.st.origin.x + col as f32 * cw - 0.5,
            self.st.origin.y + line as f32 * ch,
            1.0,
            ch,
        )
    }

    /// The run of non-blank cells around `offset`, or the run of blank ones when
    /// the offset sits on whitespace.
    ///
    /// Deliberately simpler than the word the **mouse's** double-click selects:
    /// that goes through `SelectionKind::Word` and so through the engine's own
    /// separator set (alacritty's), which is not exposed as a range. The two
    /// cannot be seen to disagree in the shipped terminal, because nothing here
    /// calls `TouchSelection::on_long_press` — a hold opens the context menu.
    /// This is the contract's method, for a host that drives the controller
    /// itself.
    fn word_range_at(&self, offset: usize) -> Range<usize> {
        let cols = self.cols();
        let (line, col) = self.cell_of(offset);
        let cells = self.row_cells(line);
        let col = col.min(cols - 1);
        let want = is_word_cell(&cells[col]);
        let mut start = col;
        while start > 0 && is_word_cell(&cells[start - 1]) == want {
            start -= 1;
        }
        let mut end = col;
        while end + 1 < cols && is_word_cell(&cells[end + 1]) == want {
            end += 1;
        }
        self.offset_of(line, start)..self.offset_of(line, end + 1)
    }

    fn line_range_at(&self, offset: usize) -> Range<usize> {
        let (line, _) = self.cell_of(offset);
        self.offset_of(line, 0)..self.offset_of(line, self.cols())
    }

    /// The engine's selection span, projected into this offset space.
    ///
    /// The span's endpoints are **inclusive** cells, so the half-open range ends
    /// one past the last one. A block (Alt-drag) selection reports the same
    /// flowing range as its bounding cells describe — a handle drag that starts
    /// from one therefore continues as a flowing selection, which is what
    /// `set_selection` can express.
    fn selection(&self) -> Range<usize> {
        let Some(span) = self.st.snapshot.selection else {
            return 0..0;
        };
        let a = self.offset_of(span.start.0, span.start.1);
        let b = self.offset_of(span.end.0, span.end.1);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        lo..hi + 1
    }

    fn set_selection(&mut self, range: Range<usize>) {
        if range.is_empty() {
            if let Some(engine) = self.st.engine.as_mut() {
                engine.selection_clear();
            }
            self.st.refresh_snapshot();
            return;
        }
        let len = self.cols() * self.rows();
        let start = range.start.min(len.saturating_sub(1));
        let last = range.end.clamp(start + 1, len) - 1;
        let last_col = self.cols() - 1;
        let (sl, sc) = self.cell_of(start);
        let (el, ec) = self.cell_of(last);
        let (sc, ec) = (sc.min(last_col), ec.min(last_col));
        // `Left` on the anchor includes the cell it names; `Right` on the head
        // includes the cell *it* names. That is what makes a half-open range of
        // cell boundaries the same set of cells the engine highlights.
        if let Some(engine) = self.st.engine.as_mut() {
            engine.selection_start(sl, sc, CellSide::Left, SelectionKind::Simple);
            engine.selection_update(el, ec, CellSide::Right);
        }
        self.st.refresh_snapshot();
    }

    /// The union of the selected rows — a single rectangle, which is all the
    /// contract wants (see [`TextHitSource::selection_bounds`]).
    fn selection_bounds(&self) -> Option<Rect> {
        let span = self.st.snapshot.selection?;
        let cw = self.st.metrics.width.max(1.0);
        let ch = self.st.metrics.height.max(1.0);
        let (sl, sc) = span.start;
        let (el, ec) = span.end;
        let ((sl, sc), (el, ec)) = if (sl, sc) <= (el, ec) {
            ((sl, sc), (el, ec))
        } else {
            ((el, ec), (sl, sc))
        };
        let (x0, x1) = if sl == el {
            (sc.min(ec) as f32, ec.max(sc) as f32 + 1.0)
        } else {
            (0.0, self.cols() as f32)
        };
        Some(Rect::new(
            self.st.origin.x + x0 * cw,
            self.st.origin.y + sl as f32 * ch,
            (x1 - x0) * cw,
            (el.saturating_sub(sl) + 1) as f32 * ch,
        ))
    }

    /// The widget's **own** bounds, not the grid's. The grid ends exactly at the
    /// last row's baseline box, and a handle for a selection ending there hangs
    /// below it; the chrome inset is the room it hangs in.
    fn viewport(&self) -> Rect {
        self.st.bounds
    }

    fn document_len(&self) -> usize {
        self.cols() * self.rows()
    }

    /// A terminal has no caret of its own — the child program owns the cursor —
    /// so there is no caret handle to offer and nothing a Cut could remove.
    fn is_editable(&self) -> bool {
        false
    }

    /// Overridden because the derivation from `is_editable` is wrong here in
    /// both directions: a non-editable surface would be offered no Paste, and a
    /// terminal accepts one (it writes to the child); and `select_all` is
    /// offered *with* a selection up, because a terminal's Select all means the
    /// whole scrollback rather than a wider run of the same line.
    fn clipboard_actions(&self) -> ClipboardActions {
        ClipboardActions {
            cut: false,
            copy: !self.selection().is_empty(),
            paste: !self.st.read_only,
            select_all: true,
        }
    }
}

// ---------------------------------------------------------------------------
// The controller mount
// ---------------------------------------------------------------------------

/// The terminal's half of the touch-selection contract: the controller, the
/// overlay it raises, and the two conversions the host owes.
pub(crate) struct TerminalTouch {
    controller: RefCell<TouchSelection>,
    /// The terminal node, the overlay's anchor.
    terminal: Cell<Option<WidgetId>>,
    /// The affordance overlay's content root (this build's).
    layer: Cell<Option<WidgetId>>,
}

// There is deliberately no `raised` flag here. `TouchSelection` keeps one of its
// own — `refresh` is a no-op until `raise` has been called and `dismiss` clears
// it — so a mirror on this side would be a second copy of the same fact, able to
// disagree with the authority (the framework can take the affordance band down
// on paths neither would hear about, such as the context-menu mount's
// `dismiss_except`). Measured: with the mirror in place, deleting its write left
// every test green, because the controller's own flag was doing the work.

impl std::fmt::Debug for TerminalTouch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalTouch").finish_non_exhaustive()
    }
}

impl TerminalTouch {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            controller: RefCell::new(TouchSelection::new()),
            terminal: Cell::new(None),
            layer: Cell::new(None),
        })
    }

    /// Publish the handles for the selection the engine now holds, and raise the
    /// overlay that paints them.
    ///
    /// The caller owns the pointer-kind decision: this is only ever reached from
    /// a direct pointer's multi-tap or from a handle drag, because a cursor gets
    /// no touch chrome.
    pub(crate) fn raise(&self, state: &Rc<RefCell<TerminalState>>, ctx: &mut EventContext<'_>) {
        {
            let mut st = state.borrow_mut();
            let source = TerminalHitSource::new(&mut st);
            self.controller
                .borrow_mut()
                .raise(ctx.layout_direction(), &source);
        }
        self.mount(ctx);
    }

    /// Recompute the published geometry after something else moved the
    /// selection — new output re-projecting the engine's span, a `select_all`.
    /// A no-op while nothing is raised.
    pub(crate) fn refresh(&self, st: &mut TerminalState, direction: LayoutDirection) {
        let source = TerminalHitSource::new(st);
        self.controller.borrow_mut().refresh(direction, &source);
    }

    /// Retire the chrome. Publishing empty geometry rather than tearing the
    /// overlay down, because the paths that need it — a keystroke, a scroll, a
    /// window deactivation — have no `EventContext` to dismiss an overlay from.
    pub(crate) fn dismiss(&self) {
        self.controller.borrow_mut().dismiss();
    }

    fn mount(&self, ctx: &mut EventContext<'_>) {
        let (Some(anchor), Some(layer)) = (self.terminal.get(), self.layer.get()) else {
            return;
        };
        ctx.show_overlay_in_band(
            OverlayRequest {
                content_id: layer,
                anchor,
                // `FullViewport`, the placement the affordance band was written
                // for: the layer positions each handle in window coordinates, so
                // the overlay only has to *contain* them for the hit-test walk
                // to descend — and it never has to be re-placed as a handle
                // moves.
                placement: OverlayPlacement::FullViewport,
                // The band is exempt from outside-press dismissal (every cell a
                // press lands on is "outside" a handle), so the lifetime is the
                // controller's published state.
                dismiss: DismissBehavior::Manual,
                layer: OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            },
            OverlayBand::TextAffordance,
        );
    }
}

/// The delegate the affordance layer services its handles through.
struct TerminalDelegate {
    touch: Rc<TerminalTouch>,
    state: Rc<RefCell<TerminalState>>,
}

impl TerminalDelegate {
    fn with_source<R>(
        &self,
        f: impl FnOnce(&mut TouchSelection, &mut TerminalHitSource<'_>) -> R,
    ) -> R {
        let mut st = self.state.borrow_mut();
        let mut source = TerminalHitSource::new(&mut st);
        let mut controller = self.touch.controller.borrow_mut();
        f(&mut controller, &mut source)
    }
}

impl TextAffordanceDelegate for TerminalDelegate {
    fn handle_drag(
        &self,
        kind: SelectionHandleKind,
        phase: HandleDragPhase,
        point: Point,
        ctx: &mut EventContext<'_>,
    ) {
        // The handle nodes move as they are dragged, so the position the router
        // localized against is not one this closure can reconstruct.
        // `pointer_position` is the sample's own window position.
        let window = ctx.pointer_position().unwrap_or(point);
        // No pointer-kind guard: `TouchSelection::drag_handle` refuses an
        // indirect pointer as its first statement, and the handle's own node
        // refuses one before that.
        self.with_source(|controller, source| {
            controller.drag_handle(kind, phase, window, ctx, source);
        });
        ctx.request_frame();
    }

    fn set_handle_offset(
        &self,
        kind: SelectionHandleKind,
        offset: usize,
        ctx: &mut EventContext<'_>,
    ) {
        let direction = ctx.layout_direction();
        self.with_source(|controller, source| {
            let selection = source.selection();
            let range = match kind {
                SelectionHandleKind::Start => offset..selection.end.max(offset),
                SelectionHandleKind::End | SelectionHandleKind::Caret => {
                    selection.start.min(offset)..offset
                }
            };
            source.set_selection(range);
            controller.refresh(direction, source);
        });
        ctx.request_frame();
    }
}

// ---------------------------------------------------------------------------
// The overlay content root
// ---------------------------------------------------------------------------

/// The affordance overlay's content root: a pass-through wrapper around one
/// [`TextAffordanceLayer`].
///
/// The layer alone would do for a finger. The wrapper exists for exactly one
/// press a finger never makes: a **cursor's** click on a selection handle. A
/// handle refuses an indirect pointer and answers `Ignored`, and an `Ignored`
/// bubbles up the handle's *own* path — the overlay's content, not the terminal,
/// which is a different arena root. Without a node above the handle to answer,
/// that click is swallowed and the touch chrome stands with nothing able to
/// remove it. `event_pass_through` removes a node from **hit-testing**, not from
/// the bubble path of a descendant that *was* hit, so this root hears presses on
/// its own handles and nothing else.
pub(crate) struct AffordanceHost {
    affordances: teksilo_core::text_touch::TextAffordances,
    handle_recipe: TextSelectionHandleRecipe,
    magnifier_recipe: TextMagnifierRecipe,
    delegate: Rc<dyn TextAffordanceDelegate>,
    painter: Option<MagnifierPainter>,
    touch: Weak<TerminalTouch>,
    layer: Option<WidgetId>,
}

impl std::fmt::Debug for AffordanceHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AffordanceHost").finish_non_exhaustive()
    }
}

impl AffordanceHost {
    pub(crate) fn new(
        affordances: teksilo_core::text_touch::TextAffordances,
        handle_recipe: TextSelectionHandleRecipe,
        magnifier_recipe: TextMagnifierRecipe,
        delegate: Rc<dyn TextAffordanceDelegate>,
        painter: Option<MagnifierPainter>,
        touch: Weak<TerminalTouch>,
    ) -> Self {
        Self {
            affordances,
            handle_recipe,
            magnifier_recipe,
            delegate,
            painter,
            touch,
            layer: None,
        }
    }
}

impl Widget for AffordanceHost {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let mut layer = TextAffordanceLayer::new(
            self.affordances.clone(),
            self.handle_recipe,
            self.magnifier_recipe,
            Rc::clone(&self.delegate),
        );
        if let Some(painter) = self.painter.as_ref() {
            layer = layer.magnifier_painter(Rc::clone(painter));
        }
        let id = ctx.add(layer);
        self.layer = Some(id);

        let touch = self.touch.clone();
        let handlers =
            HandlerSet::new()
                .event_pass_through(true)
                .on_pointer_event(move |event, ctx| {
                    if !matches!(event, WidgetEvent::PointerDown { .. })
                        || ctx.pointer_kind().is_direct()
                    {
                        return EventResponse::Ignored;
                    }
                    if let Some(touch) = touch.upgrade() {
                        touch.dismiss();
                    }
                    EventResponse::Handled
                });
        ctx.apply_self_handlers(handlers);
        vec![id]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for placement in children.iter_mut() {
            placement.origin = bounds.origin();
            placement.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.layer.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(accesskit::Role::GenericContainer);
    }
}

// ---------------------------------------------------------------------------
// The fallback selection style
// ---------------------------------------------------------------------------

/// The shipped handle / lens recipes for a theme that installs no
/// [`TextSelectionStyle`] of its own.
///
/// A near-copy of `teksilo_widgets::styles::RecipeTextSelectionStyle`, and
/// forced to be one: that impl is the default for every widget-tier text
/// surface, and `teksilo-terminal` cannot depend on `teksilo-widgets`. What is
/// *not* duplicated is the arithmetic — every dimension comes from
/// `teksilo_core::text_touch`'s own constants through the same
/// [`dp`] projection, so a density change moves both copies together and the
/// only thing stated twice is which token colours which part.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TerminalSelectionStyle;

impl TextSelectionStyle for TerminalSelectionStyle {
    fn handle(&self, theme: &Theme) -> TextSelectionHandleRecipe {
        let tokens: &InputTokens = &theme.input;
        TextSelectionHandleRecipe {
            diameter: dp(HANDLE_DIAMETER, TargetRole::Decoration, tokens),
            hit_size: dp(HANDLE_HIT_SIZE, TargetRole::Target, tokens),
            stem_width: HANDLE_STEM_WIDTH,
            fill: RecipeColor::Surface(SurfaceRole::Accent),
            outline: RecipeColor::Surface(SurfaceRole::EditorBg),
            outline_width: 1.0,
        }
    }

    fn magnifier(&self, _theme: &Theme) -> TextMagnifierRecipe {
        TextMagnifierRecipe {
            radius: magnifier::MAGNIFIER_RADIUS,
            half_height: magnifier::MAGNIFIER_HALF_HEIGHT,
            scale: magnifier::MAGNIFIER_SCALE,
            rise: magnifier::MAGNIFIER_RISE,
            // Zero, because the clip behind the frame is rectangular and cannot
            // be anything else: a rounded frame leaves magnified content
            // standing outside its corners with nothing able to remove it.
            corner_radius: 0.0,
            background: RecipeColor::Surface(SurfaceRole::EditorBg),
            border: RecipeColor::Border(BorderRole::Default),
            border_width: 1.0,
        }
    }
}

/// The metrics the controller hit-tests with, from whichever style is installed.
pub(crate) fn selection_style(theme: &Theme) -> Rc<dyn TextSelectionStyle> {
    match theme.style_slots.text_selection.clone() {
        Some(installed) => installed,
        None => Rc::new(TerminalSelectionStyle),
    }
}

/// The controller, configured from `theme`'s selection style so the rectangles
/// it hit-tests are the ones the layer paints.
pub(crate) fn configure_controller(
    touch: &Rc<TerminalTouch>,
    theme: &Theme,
    reduced_motion: bool,
) -> (
    teksilo_core::text_touch::TextAffordances,
    TextSelectionHandleRecipe,
    TextMagnifierRecipe,
) {
    let style = selection_style(theme);
    let handle = style.handle(theme);
    let lens = style.magnifier(theme);
    let controller = TouchSelection::new()
        .metrics(HandleMetrics::from(&handle))
        .magnifier_metrics(lens.radius, lens.half_height, lens.rise, lens.scale)
        .reduced_motion(reduced_motion);
    let affordances = controller.affordances();
    *touch.controller.borrow_mut() = controller;
    (affordances, handle, lens)
}

/// Record the terminal node and the overlay content this build produced.
pub(crate) fn attach(touch: &Rc<TerminalTouch>, terminal: WidgetId, layer: WidgetId) {
    touch.terminal.set(Some(terminal));
    touch.layer.set(Some(layer));
}

/// The delegate handle for a mounted terminal.
pub(crate) fn delegate(
    touch: &Rc<TerminalTouch>,
    state: &Rc<RefCell<TerminalState>>,
) -> Rc<dyn TextAffordanceDelegate> {
    Rc::new(TerminalDelegate {
        touch: Rc::clone(touch),
        state: Rc::clone(state),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color_scheme::ColorScheme;
    use crate::engine::SelectionSpan;
    use crate::render::CellMetrics;
    use crate::state::blank_snapshot;

    const COLS: usize = 40;
    const ROWS: usize = 20;
    const CW: f32 = 8.0;
    const CH: f32 = 16.0;
    /// The grid's top-left, deliberately not the window's: everything this
    /// contract answers is in window coordinates, so a fixture at the origin
    /// could not tell a correct answer from one missing the offset.
    const OX: f32 = 10.0;
    const OY: f32 = 20.0;

    fn state() -> TerminalState {
        let mut st = TerminalState::new(ColorScheme::default());
        st.metrics = CellMetrics {
            width: CW,
            height: CH,
        };
        st.cols = COLS;
        st.rows = ROWS;
        st.origin = teksilo_canvas::Point::new(OX, OY);
        st.bounds = Rect::new(
            OX - 4.0,
            OY - 4.0,
            COLS as f32 * CW + 8.0,
            ROWS as f32 * CH + 8.0,
        );
        st.snapshot = blank_snapshot(COLS, ROWS);
        st
    }

    /// Write `text` into row `line` starting at column 0.
    fn write_row(st: &mut TerminalState, line: usize, text: &str) {
        for (col, ch) in text.chars().enumerate() {
            if let Some(cell) = st.snapshot.cells.get_mut(line * COLS + col) {
                cell.ch = ch;
            }
        }
    }

    #[test]
    fn an_offset_is_a_cell_boundary_in_the_row_the_point_is_over() {
        let mut st = state();
        let source = TerminalHitSource::new(&mut st);
        // Dead centre of the cell at (row 2, column 3): the nearest boundary is
        // the one on its left.
        assert_eq!(
            source.offset_at(Point::new(OX + 3.4 * CW, OY + 2.5 * CH)),
            2 * COLS + 3
        );
        // Four fifths across it: the boundary on its right.
        assert_eq!(
            source.offset_at(Point::new(OX + 3.8 * CW, OY + 2.5 * CH)),
            2 * COLS + 4
        );
    }

    #[test]
    fn a_point_outside_the_grid_clamps_rather_than_vanishing() {
        let mut st = state();
        let source = TerminalHitSource::new(&mut st);
        assert_eq!(source.offset_at(Point::new(-500.0, -500.0)), 0);
        assert_eq!(
            source.offset_at(Point::new(10_000.0, 10_000.0)),
            (ROWS - 1) * COLS + COLS,
            "the far corner is the boundary past the last cell of the last row"
        );
        assert_eq!(source.document_len(), COLS * ROWS);
    }

    /// The rectangle a handle hangs off is centred **on** the grid line, not
    /// beside it, and it is a full line tall.
    #[test]
    fn a_caret_rect_straddles_the_boundary_it_names() {
        let mut st = state();
        let source = TerminalHitSource::new(&mut st);
        let rect = source.caret_rect(3 * COLS + 7);
        assert!((rect.x + rect.width / 2.0 - (OX + 7.0 * CW)).abs() < 0.001);
        assert_eq!(rect.y, OY + 3.0 * CH);
        assert_eq!(rect.height, CH);
        assert!(
            rect.width > 0.0,
            "a zero-width rectangle fails `rects_intersect` against a viewport \
             edge it touches, and the handle at column 0 would disappear"
        );
    }

    #[test]
    fn a_word_is_a_run_of_non_blank_cells_within_one_row() {
        let mut st = state();
        write_row(&mut st, 4, "  cargo build  ");
        let source = TerminalHitSource::new(&mut st);
        assert_eq!(
            source.word_range_at(4 * COLS + 4),
            4 * COLS + 2..4 * COLS + 7
        );
        // On the space between the two words: the run of blanks, which is one
        // cell wide here.
        assert_eq!(
            source.word_range_at(4 * COLS + 7),
            4 * COLS + 7..4 * COLS + 8
        );
        assert_eq!(
            source.word_range_at(4 * COLS + 9),
            4 * COLS + 8..4 * COLS + 13
        );
    }

    #[test]
    fn a_line_is_the_whole_row_and_never_reaches_the_next() {
        let mut st = state();
        let source = TerminalHitSource::new(&mut st);
        assert_eq!(source.line_range_at(6 * COLS + 11), 6 * COLS..7 * COLS);
    }

    /// The span's endpoints are inclusive cells and the contract's range is
    /// half-open, so the round trip has to add and drop exactly one.
    #[test]
    fn the_selection_is_the_span_one_past_its_last_cell() {
        let mut st = state();
        st.snapshot.selection = Some(SelectionSpan {
            start: (2, 5),
            end: (2, 9),
            block: false,
        });
        let source = TerminalHitSource::new(&mut st);
        assert_eq!(source.selection(), 2 * COLS + 5..2 * COLS + 10);
    }

    /// A backwards drag reports its endpoints in the order they were made; the
    /// contract wants a range, which is ordered.
    #[test]
    fn a_backwards_span_still_reads_as_an_ordered_range() {
        let mut st = state();
        st.snapshot.selection = Some(SelectionSpan {
            start: (2, 9),
            end: (2, 5),
            block: false,
        });
        let source = TerminalHitSource::new(&mut st);
        assert_eq!(source.selection(), 2 * COLS + 5..2 * COLS + 10);
    }

    #[test]
    fn a_multi_line_selections_bounds_span_every_row_it_touches() {
        let mut st = state();
        st.snapshot.selection = Some(SelectionSpan {
            start: (1, 3),
            end: (3, 8),
            block: false,
        });
        let source = TerminalHitSource::new(&mut st);
        let bounds = source.selection_bounds().expect("a selection has bounds");
        assert_eq!(bounds.x, OX, "a flowing selection over rows is full-width");
        assert_eq!(bounds.width, COLS as f32 * CW);
        assert_eq!(bounds.y, OY + CH);
        assert_eq!(bounds.height, 3.0 * CH);
    }

    #[test]
    fn a_single_line_selections_bounds_are_the_cells_it_covers() {
        let mut st = state();
        st.snapshot.selection = Some(SelectionSpan {
            start: (1, 3),
            end: (1, 6),
            block: false,
        });
        let source = TerminalHitSource::new(&mut st);
        let bounds = source.selection_bounds().expect("a selection has bounds");
        assert_eq!(bounds.x, OX + 3.0 * CW);
        assert_eq!(bounds.width, 4.0 * CW);
        assert_eq!(bounds.height, CH);
    }

    #[test]
    fn nothing_selected_has_no_bounds_and_an_empty_range() {
        let mut st = state();
        let source = TerminalHitSource::new(&mut st);
        assert!(source.selection().is_empty());
        assert!(source.selection_bounds().is_none());
    }

    /// The viewport is the **widget's** bounds, wider than the grid by the chrome
    /// inset — which is the room a handle under the last row hangs in. Clamping
    /// affordances into the grid instead would flip that handle above the line.
    #[test]
    fn the_viewport_is_the_widget_not_the_grid() {
        let mut st = state();
        let grid_bottom = OY + ROWS as f32 * CH;
        let source = TerminalHitSource::new(&mut st);
        assert!(source.viewport().bottom() > grid_bottom);
    }

    /// A terminal has no caret of its own, and nothing a Cut could remove.
    #[test]
    fn the_offered_commands_are_a_terminals_and_not_an_editors() {
        let mut st = state();
        st.snapshot.selection = Some(SelectionSpan {
            start: (0, 0),
            end: (0, 4),
            block: false,
        });
        let actions = TerminalHitSource::new(&mut st).clipboard_actions();
        assert!(
            !actions.cut,
            "terminal output is not the caller's to remove"
        );
        assert!(actions.copy);
        assert!(actions.paste, "a paste goes to the child");
        assert!(
            actions.select_all,
            "…and Select all stays on offer with a selection up, because it \
             means the whole scrollback rather than a wider run of one line"
        );
        assert!(!TerminalHitSource::new(&mut st).is_editable());
    }

    #[test]
    fn a_read_only_terminal_offers_no_paste() {
        let mut st = state();
        st.read_only = true;
        assert!(!TerminalHitSource::new(&mut st).clipboard_actions().paste);
    }

    /// Setting a selection anchors on the first cell's left half and heads to the
    /// last cell's right half, which is the pair that reproduces the same set of
    /// cells the half-open range describes.
    #[test]
    fn setting_a_selection_names_the_cells_the_range_covers() {
        let factory = crate::memory::MemoryEngineFactory::new();
        let shared = factory.shared();
        let spawned = crate::engine::TerminalEngineFactory::spawn(
            &factory,
            &crate::engine::TerminalCommand::shell(),
            crate::engine::PtyGeom::new(COLS as u16, ROWS as u16, 0, 0),
            0,
        )
        .expect("the memory engine always spawns");
        let mut st = state();
        st.engine = Some(spawned.engine);

        TerminalHitSource::new(&mut st).set_selection(2 * COLS + 5..2 * COLS + 10);

        let s = shared.borrow();
        assert_eq!(
            s.selections,
            vec![(2, 5, SelectionKind::Simple)],
            "anchored on the range's first cell"
        );
        assert_eq!(
            s.selection_updates,
            vec![(2, 9, CellSide::Right)],
            "…and headed to the cell before its end, on that cell's right half"
        );
    }

    /// The controller's published geometry **follows** the selection.
    ///
    /// This is the rule the four context-menu commands lean on instead of each
    /// deciding whether to retire the chrome: Select all re-marks the buffer and
    /// the handles move to the ends of its visible part, Clear empties the
    /// selection and there is nothing left to mark. Asserted here rather than
    /// through the tree because the framework's own context-menu mount dismisses
    /// the affordance band before a command from that menu ever runs
    /// (`OverlayManager::dismiss_except`), which makes the tree blind to this.
    #[test]
    fn the_published_geometry_follows_the_selection() {
        let touch = TerminalTouch::new();
        let theme = teksilo_core::presets::intui::light();
        let (affordances, _, _) = configure_controller(&touch, &theme, false);

        let mut st = state();
        st.snapshot.selection = Some(SelectionSpan {
            start: (3, 5),
            end: (3, 11),
            block: false,
        });
        {
            let source = TerminalHitSource::new(&mut st);
            touch
                .controller
                .borrow_mut()
                .raise(LayoutDirection::LeftToRight, &source);
        }
        assert_eq!(
            affordances.handles().len(),
            2,
            "a selection publishes its two ends"
        );

        st.snapshot.selection = None;
        touch.refresh(&mut st, LayoutDirection::LeftToRight);
        assert!(
            affordances.handles().is_empty(),
            "a terminal has no caret handle, so an empty selection publishes nothing"
        );
    }

    /// Retiring publishes empty geometry rather than tearing an overlay down:
    /// the paths that need it (a keystroke, a scroll, a window deactivation) have
    /// no `EventContext` to dismiss an overlay from.
    #[test]
    fn dismissing_publishes_empty_geometry() {
        let touch = TerminalTouch::new();
        let theme = teksilo_core::presets::intui::light();
        let (affordances, _, _) = configure_controller(&touch, &theme, false);

        let mut st = state();
        st.snapshot.selection = Some(SelectionSpan {
            start: (3, 5),
            end: (3, 11),
            block: false,
        });
        {
            let source = TerminalHitSource::new(&mut st);
            touch
                .controller
                .borrow_mut()
                .raise(LayoutDirection::LeftToRight, &source);
        }
        assert_eq!(affordances.handles().len(), 2);

        touch.dismiss();
        assert!(affordances.handles().is_empty());
        // …and a refresh does not bring them back: retirement is a decision, not
        // a stale cache.
        touch.refresh(&mut st, LayoutDirection::LeftToRight);
        assert!(affordances.handles().is_empty());
    }

    #[test]
    fn setting_an_empty_selection_clears_it() {
        let factory = crate::memory::MemoryEngineFactory::new();
        let shared = factory.shared();
        let spawned = crate::engine::TerminalEngineFactory::spawn(
            &factory,
            &crate::engine::TerminalCommand::shell(),
            crate::engine::PtyGeom::new(COLS as u16, ROWS as u16, 0, 0),
            0,
        )
        .expect("the memory engine always spawns");
        let mut st = state();
        st.engine = Some(spawned.engine);
        shared.borrow_mut().selection_text = Some("gone".into());

        TerminalHitSource::new(&mut st).set_selection(7..7);

        assert!(shared.borrow().selection_text.is_none());
        assert!(shared.borrow().selections.is_empty());
    }
}
