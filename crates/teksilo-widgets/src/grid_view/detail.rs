// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The full-width detail band under a disclosed tile.
//!
//! The "album expansion" of a desktop music library: activating a tile opens
//! a band right under the row that holds it, as wide as the grid, and the rows
//! after it move down to make room. Three pieces:
//!
//! * [`DetailState`], shared by everything that has to agree on which tile is
//!   open and how tall its band is: the layout, the tiles' accessibility, the
//!   keys, and the band itself.
//! * [`DetailRowStrategy`], a layout strategy wrapped around the grid's own.
//!   It leaves the tiles' rows and columns alone and moves every rect below
//!   the open row down by the band, so virtualization, `max_scroll_y`, the
//!   focus ring, the drop indicator, reveal-on-focus and the rubber band all
//!   see the band without knowing about it. Wrapping works for any strategy
//!   with rows (uniform, variable row height, sections); a waterfall has no
//!   rows, so `GridView` does not wrap it and the band is inert there.
//! * [`DetailBand`], the band widget. It is a child of the grid itself, not
//!   of the body pane: the pane rebuilds whenever a scroll leaves its buffer
//!   or a resize changes the column count, and the band's content (a track
//!   list with focus and a scroll offset of its own) must survive both. To a
//!   screen reader it is a child of the pane all the same, read right after
//!   its tile's row (`GridBodyPane::accessibility_children`).
//!
//! Opening, closing or moving the band rebuilds the band alone. The pane
//! realizes the tiles a viewport would hold with no band at all
//! ([`DetailRowStrategy::visible_range`]), and the grid keeps the content
//! under the viewport's top edge still when the band changes above it
//! ([`DetailRowStrategy::settle_placement`]), so the realized window does
//! not change and no tile node is replaced. A scroll the change causes, the
//! reveal of a band a user opened, realizes tiles as any scroll does.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{EdgeInsets, Point, Rect, SizeProposal};
use teksilo_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

use super::TileContext;
use super::layout::GridLayoutStrategy;
use super::layout::strategy::{TileRect, VisibleTileRange};

/// Builds a band's content for the disclosed tile. `None`: the tile has
/// nothing to disclose, and the band takes no space.
pub(crate) type DetailBuilder<T> = Rc<dyn Fn(&TileContext<'_, T>) -> Option<Box<dyn Widget>>>;

/// The exact height of the band for a tile.
pub(crate) type DetailHeightFn = Rc<dyn Fn(usize) -> f32>;

/// The band's content for a tile, erased from the grid's item type: the
/// widget, `None` when the tile has nothing to disclose, and whether the
/// tile's item was resident when it was asked. A tile of a lazy source that
/// has not loaded yet gets a placeholder and `false`.
pub(crate) type BandContentFn = Rc<dyn Fn(usize) -> (Option<Box<dyn Widget>>, bool)>;

/// Below this, a band height is unchanged: measurements wobble in the last
/// bits, and a wobble must not move the scroll offset.
const HEIGHT_EPSILON: f32 = 0.01;

/// What activating a tile did to its band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disclosure {
    /// The tile's band opened, or moved to it from another tile.
    Opened,
    /// The tile's band was open and closed.
    Closed,
    /// Nothing: the tile has nothing to disclose, or its band was open
    /// already.
    Nothing,
}

/// The band as a layout placed it: the open tile and its band's height, at
/// a viewport width.
#[derive(Clone, Copy, PartialEq)]
struct Placed {
    width: f32,
    band: Option<(usize, f32)>,
}

/// Which tile is disclosed, and what is known about its band.
pub(crate) struct DetailState {
    /// The tile the application or the user disclosed. Read live by every
    /// party, so a write from outside opens and closes the band.
    pub(crate) expanded: Signal<Option<usize>>,
    /// The tile the band holds content for, written by the band's build.
    /// Differs from `expanded` while the builder returned `None`, and for the
    /// moment between a write and the band's rebuild.
    pub(crate) shown: Cell<Option<usize>>,
    /// The application's exact height, or `None` to measure the band.
    height_fn: Option<DetailHeightFn>,
    /// The band's measured height and the tile it was measured for.
    measured: Cell<Option<(usize, f32)>>,
    /// The band widget, once built: the target of a tile's `controls`
    /// relation and of ↓ from the open tile.
    pub(crate) band_id: Cell<Option<WidgetId>>,
    /// Whether the band's content has something to take focus. ↓ moves into
    /// the band only then, and is plain navigation otherwise.
    pub(crate) enterable: Cell<bool>,
    /// `true` while keyboard focus is inside the band. The grid's own keys
    /// stand aside then: a key the band's content did not use must not move
    /// the tile cursor behind it.
    pub(crate) focus_within: Signal<bool>,
    /// The band's content for a tile, set by every `GridView::build` because
    /// it reads the layout strategy in force. Activation asks it too, to
    /// learn whether a tile has anything to disclose.
    content: RefCell<Option<BandContentFn>>,
    /// A tile a user has just opened: the next layout scrolls its row and its
    /// band into view.
    pub(crate) reveal: Cell<Option<usize>>,
    /// The tile whose band held keyboard focus when `expanded` was written:
    /// the band's rebuild hands focus back to it when the new content has
    /// nothing to take it.
    focus_return: Cell<Option<usize>>,
    /// Rebuilds the band alone: bumped when the window holding a tile whose
    /// band was built before its item arrived loads.
    pub(crate) version: Signal<u64>,
    /// Whether the band's tile had no resident item when its content was
    /// built, so it shows a placeholder for content still to come.
    pub(crate) awaiting_item: Cell<bool>,
    /// Where the last layout put the band, for the next one to keep the
    /// content under the viewport's top edge still when it changes. `None`
    /// after a change to the data, which renumbers the tiles it refers to.
    placed: Cell<Option<Placed>>,
    /// Bumped whenever the band's placement changes. The focus ring and the
    /// drop indicator are painted at tile positions that move with it, by a
    /// widget whose own bounds do not change.
    pub(crate) moved: Signal<u64>,
}

impl DetailState {
    pub(crate) fn new(expanded: Signal<Option<usize>>, height_fn: Option<DetailHeightFn>) -> Self {
        Self {
            expanded,
            shown: Cell::new(None),
            height_fn,
            measured: Cell::new(None),
            band_id: Cell::new(None),
            enterable: Cell::new(false),
            focus_within: Signal::new(false),
            content: RefCell::new(None),
            reveal: Cell::new(None),
            focus_return: Cell::new(None),
            version: Signal::new(0),
            awaiting_item: Cell::new(false),
            placed: Cell::new(None),
            moved: Signal::new(0),
        }
    }

    /// The tile whose band is open, among `len` tiles: disclosed, and with
    /// content.
    pub(crate) fn open(&self, len: usize) -> Option<usize> {
        self.shown
            .get()
            .filter(|&i| i < len && self.expanded.get() == Some(i))
    }

    /// Whether `index` is the tile whose band is open.
    pub(crate) fn is_open(&self, index: usize) -> bool {
        self.shown.get() == Some(index) && self.expanded.get() == Some(index)
    }

    /// Install the band's content, built over the layout strategy in force.
    pub(crate) fn set_content(&self, content: BandContentFn) {
        *self.content.borrow_mut() = Some(content);
    }

    fn content_for(&self, index: usize) -> (Option<Box<dyn Widget>>, bool) {
        // Cloned out, so the application's builder runs with no borrow held.
        let content = self.content.borrow().clone();
        match content {
            Some(content) => content(index),
            None => (None, true),
        }
    }

    /// Activating `index`, by the keyboard or a click: close its band when it
    /// is the disclosed tile, open it otherwise.
    pub(crate) fn activate(&self, index: usize) -> Disclosure {
        if self.expanded.get() == Some(index) {
            self.expanded.set(None);
            return Disclosure::Closed;
        }
        self.expand(index)
    }

    /// Open `index`'s band, and have the next layout scroll it into view.
    ///
    /// A tile with nothing to disclose opens nothing, and leaves a band open
    /// on another tile as it is. Whether it has anything is learned by
    /// building its band's content once and dropping it, so the builder runs
    /// twice for a band that opens: once here, once when the band is built.
    /// Asking it of every tile, to tell a reader which tiles are expandable,
    /// would build the content of every tile the grid realizes, which is why
    /// every tile is offered as one (`TileA11y::accessibility`).
    pub(crate) fn expand(&self, index: usize) -> Disclosure {
        if self.expanded.get() == Some(index) {
            return Disclosure::Nothing;
        }
        if self.content_for(index).0.is_none() {
            return Disclosure::Nothing;
        }
        self.reveal.set(Some(index));
        self.expanded.set(Some(index));
        Disclosure::Opened
    }

    /// Whether the band's height comes from measuring it.
    pub(crate) fn measures(&self) -> bool {
        self.height_fn.is_none()
    }

    /// Forget where the last layout put the band. Called on a change to the
    /// data: the tile it was recorded under may be another one now, and the
    /// rows around it have moved anyway.
    pub(crate) fn forget_placement(&self) {
        self.placed.set(None);
        // Focus inside a band the data change closes or moves goes where
        // the framework puts it after the grid's rebuild: onto the grid.
        self.focus_return.set(None);
    }

    fn height(&self, index: usize, estimate: f32) -> f32 {
        if let Some(f) = &self.height_fn {
            return f(index).max(0.0);
        }
        match self.measured.get() {
            Some((i, h)) if i == index => h,
            _ => estimate,
        }
    }
}

/// Where the band sits, in the wrapped strategy's coordinates.
#[derive(Clone, Copy)]
struct Insert {
    index: usize,
    /// Top of the open row: every rect strictly below it moves down.
    row_top: f32,
    /// Bottom of the open row.
    row_bottom: f32,
    /// Top of the band, one row gap under the open row.
    band_top: f32,
    band_height: f32,
    /// How far everything below the open row moves: a row gap and the band.
    extra: f32,
}

/// The scroll correction that keeps still the content the viewport's top
/// edge is on, when the band goes from `old` to `new` at `scroll_y`.
///
/// The content is the grid without its band, and the viewport is pinned to
/// a point of it: the one at its top edge, or, when that edge is inside the
/// band, the row under the band, at the distance it is on screen. Where that
/// point lands with the new band is the new top. A band that opens, closes
/// or moves entirely below the top moves nothing; one above it moves the
/// offset by what it adds or takes away; a band growing under the top edge
/// keeps the rows under it still, as a measured row does
/// (`VariableRowGrid::observe_measured`).
fn anchor_shift(old: Option<Insert>, new: Option<Insert>, scroll_y: f32) -> f32 {
    // The pinned point, whether it lies past the old band, and how far
    // under the viewport's top edge it is on screen.
    let (point, past, below_top) = match old {
        Some(o) if scroll_y >= o.band_top + o.extra => (scroll_y - o.extra, true, 0.0),
        Some(o) if scroll_y >= o.band_top => (o.band_top, true, o.band_top + o.extra - scroll_y),
        _ => (scroll_y, false, 0.0),
    };
    let landed = match new {
        Some(n) if point > n.band_top || (past && point >= n.band_top) => point + n.extra,
        _ => point,
    };
    landed - below_top - scroll_y
}

/// A layout strategy with the detail band inserted under the open row.
pub(crate) struct DetailRowStrategy {
    inner: Rc<dyn GridLayoutStrategy>,
    state: Rc<DetailState>,
    len_fn: Rc<dyn Fn() -> usize>,
    row_gap: f32,
    inset: EdgeInsets,
}

impl std::fmt::Debug for DetailRowStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DetailRowStrategy")
            .field("inner", &self.inner)
            .field("shown", &self.state.shown.get())
            .finish()
    }
}

impl DetailRowStrategy {
    pub(crate) fn new(
        inner: Rc<dyn GridLayoutStrategy>,
        state: Rc<DetailState>,
        len_fn: Rc<dyn Fn() -> usize>,
        row_gap: f32,
        inset: EdgeInsets,
    ) -> Self {
        Self {
            inner,
            state,
            len_fn,
            row_gap: row_gap.max(0.0),
            inset,
        }
    }

    /// The wrapped strategy. The band's content reads a tile's row and
    /// column from it rather than from this wrapper: the wrapper holds the
    /// state the content is kept in, and a reference back would be a cycle.
    pub(crate) fn inner(&self) -> Rc<dyn GridLayoutStrategy> {
        self.inner.clone()
    }

    fn insert_for(&self, index: usize, band_height: f32, viewport_width: f32) -> Insert {
        let row = self.inner.tile_rect(index, viewport_width);
        let row_bottom = row.y + row.height;
        Insert {
            index,
            row_top: row.y,
            row_bottom,
            band_top: row_bottom + self.row_gap,
            band_height,
            extra: self.row_gap + band_height,
        }
    }

    fn open_band(&self) -> Option<(usize, f32)> {
        let index = self.state.open((self.len_fn)())?;
        Some((
            index,
            self.state.height(index, self.inner.estimated_row_height()),
        ))
    }

    fn insert(&self, viewport_width: f32) -> Option<Insert> {
        let (index, height) = self.open_band()?;
        Some(self.insert_for(index, height, viewport_width))
    }

    /// A y in this strategy's coordinates, in the wrapped one's. A y inside
    /// the inserted space maps to the open row's bottom.
    fn to_inner(y: f32, ins: &Insert) -> f32 {
        if y < ins.row_bottom {
            y
        } else if y < ins.row_bottom + ins.extra {
            ins.row_bottom
        } else {
            y - ins.extra
        }
    }

    fn shifted(mut r: TileRect, ins: &Insert) -> TileRect {
        if r.y > ins.row_top + HEIGHT_EPSILON {
            r.y += ins.extra;
        }
        r
    }

    /// The open tile and its band's rect in content coordinates: as wide as
    /// the content (the viewport less its side insets), one row gap under
    /// the open row.
    pub(crate) fn band_rect(&self, viewport_width: f32) -> Option<(usize, TileRect)> {
        let ins = self.insert(viewport_width)?;
        Some((
            ins.index,
            TileRect {
                x: self.inset.leading,
                y: ins.band_top,
                width: (viewport_width - self.inset.horizontal()).max(0.0),
                height: ins.band_height,
            },
        ))
    }

    /// Record the band's measured `height` for tile `index`. The scroll
    /// correction a change of height calls for comes from
    /// [`settle_placement`](Self::settle_placement), with every other change
    /// of the band.
    pub(crate) fn record_band_height(&self, index: usize, height: f32) {
        let Some((open, before)) = self.open_band() else {
            return;
        };
        if open == index && (height - before).abs() > HEIGHT_EPSILON {
            self.state.measured.set(Some((index, height)));
        }
    }

    /// Record where the band is now, and return the scroll correction that
    /// keeps the content under the viewport's top edge still across whatever
    /// changed since the last layout: the band opening, closing, moving to
    /// another tile, or taking a new height. See [`anchor_shift`].
    ///
    /// Both placements are read from the wrapped strategy as it is now, so a
    /// row above that was measured again in between, which the body pane
    /// anchors on its own, is not counted twice. Nothing is corrected across
    /// a change of width, which lays every row out again, nor after a change
    /// to the data (`DetailState::forget_placement`).
    pub(crate) fn settle_placement(&self, scroll_y: f32, viewport_width: f32) -> f32 {
        let now = Placed {
            width: viewport_width,
            band: self.open_band(),
        };
        let Some(before) = self.state.placed.replace(Some(now)) else {
            self.state.moved.set(self.state.moved.get().wrapping_add(1));
            return 0.0;
        };
        if before == now {
            return 0.0;
        }
        self.state.moved.set(self.state.moved.get().wrapping_add(1));
        if (before.width - viewport_width).abs() > HEIGHT_EPSILON {
            return 0.0;
        }
        let old = before
            .band
            .map(|(index, height)| self.insert_for(index, height, viewport_width));
        let new = now
            .band
            .map(|(index, height)| self.insert_for(index, height, viewport_width));
        anchor_shift(old, new, scroll_y)
    }

    /// The span a reveal of tile `index`'s band brings into view: from the
    /// top of the tile's row to the bottom of the band. `None` when the band
    /// open is not that tile's.
    pub(crate) fn reveal_span(&self, index: usize, viewport_width: f32) -> Option<(f32, f32)> {
        let ins = self
            .insert(viewport_width)
            .filter(|ins| ins.index == index)?;
        Some((ins.row_top, ins.band_top + ins.band_height))
    }
}

impl GridLayoutStrategy for DetailRowStrategy {
    fn column_count(&self, viewport_width: f32) -> usize {
        self.inner.column_count(viewport_width)
    }

    fn column_x(&self, col: usize, viewport_width: f32) -> (f32, f32) {
        self.inner.column_x(col, viewport_width)
    }

    fn total_content_height(&self, item_count: usize, viewport_width: f32) -> f32 {
        let total = self.inner.total_content_height(item_count, viewport_width);
        total + self.insert(viewport_width).map_or(0.0, |ins| ins.extra)
    }

    /// The tiles a viewport whose top edge is at `scroll_y` would hold with
    /// no band at all: a superset of those on screen with it, by the rows the
    /// band covers.
    ///
    /// The band takes nothing out of the window, so opening, closing or
    /// moving it at the same place in the content needs no tile the pane has
    /// not realized already, and no tile node is replaced. When the band
    /// changes above the viewport, the grid moves the offset so the content
    /// under the top edge stays put (`settle_placement`), which keeps the
    /// window's start where it was too.
    ///
    /// The window starts at the content under the top edge, which after a
    /// reset (the offset back at 0) is 0 wherever the band is: the stale row
    /// table a reset leaves until the next call that passes a count cannot
    /// shorten it.
    fn visible_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
        item_count: usize,
    ) -> VisibleTileRange {
        let top = self
            .insert(viewport_width)
            .map_or(scroll_y, |ins| Self::to_inner(scroll_y, &ins));
        self.inner
            .visible_range(top, viewport_height, viewport_width, item_count)
    }

    fn tile_rect(&self, index: usize, viewport_width: f32) -> TileRect {
        let r = self.inner.tile_rect(index, viewport_width);
        match self.insert(viewport_width) {
            Some(ins) => Self::shifted(r, &ins),
            None => r,
        }
    }

    fn estimated_row_height(&self) -> f32 {
        self.inner.estimated_row_height()
    }

    fn measures_tiles(&self) -> bool {
        self.inner.measures_tiles()
    }

    fn observe_measured(
        &self,
        measured: &[(usize, f32)],
        scroll_y: f32,
        viewport_width: f32,
    ) -> f32 {
        // The wrapped strategy anchors against the viewport top in its own
        // coordinates. A row above it moves the band and everything under it
        // by the same amount, so its correction holds here unchanged.
        let scroll_y = self
            .insert(viewport_width)
            .map_or(scroll_y, |ins| Self::to_inner(scroll_y, &ins));
        self.inner
            .observe_measured(measured, scroll_y, viewport_width)
    }

    fn invalidate_rows(&self, item_range: std::ops::Range<usize>) {
        self.inner.invalidate_rows(item_range);
    }

    fn resize(&self, item_count: usize) {
        self.inner.resize(item_count);
    }

    fn index_at_point(
        &self,
        content_point: Point,
        item_count: usize,
        viewport_width: f32,
    ) -> Option<usize> {
        let Some(ins) = self.insert(viewport_width) else {
            return self
                .inner
                .index_at_point(content_point, item_count, viewport_width);
        };
        let y = content_point.y;
        if y >= ins.row_bottom && y < ins.row_bottom + ins.extra {
            // The band, and the gap above it: no tile.
            return None;
        }
        self.inner.index_at_point(
            Point::new(content_point.x, Self::to_inner(y, &ins)),
            item_count,
            viewport_width,
        )
    }

    fn tile_row_col(&self, index: usize, viewport_width: f32) -> (usize, usize) {
        // The band is not a row of the grid: tiles keep their coordinates.
        self.inner.tile_row_col(index, viewport_width)
    }

    fn headers_in_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
    ) -> Vec<(usize, TileRect)> {
        let Some(ins) = self.insert(viewport_width) else {
            return self
                .inner
                .headers_in_range(scroll_y, viewport_height, viewport_width);
        };
        // The same window as `visible_range`, for the same reason.
        let top = Self::to_inner(scroll_y, &ins);
        self.inner
            .headers_in_range(top, viewport_height, viewport_width)
            .into_iter()
            .map(|(section, r)| (section, Self::shifted(r, &ins)))
            .collect()
    }

    fn current_section(&self, scroll_y: f32, viewport_width: f32) -> Option<usize> {
        let scroll_y = self
            .insert(viewport_width)
            .map_or(scroll_y, |ins| Self::to_inner(scroll_y, &ins));
        self.inner.current_section(scroll_y, viewport_width)
    }

    fn header_rect(&self, section: usize, viewport_width: f32) -> Option<TileRect> {
        let r = self.inner.header_rect(section, viewport_width)?;
        Some(match self.insert(viewport_width) {
            Some(ins) => Self::shifted(r, &ins),
            None => r,
        })
    }
}

/// Moves the keyboard back to the open tile from inside its band.
pub(crate) type ReturnToTile = Rc<dyn Fn(usize, &mut EventContext)>;

/// The band's accessible name when nothing else names it: "Details of item
/// 5", for the tile at flat `index` 4.
fn band_name(index: usize) -> String {
    let position = i64::try_from(index.saturating_add(1)).unwrap_or(i64::MAX);
    teksilo_i18n::tr_widget!(grid_view_detail_name(position = position))
        .resolve_now()
        .to_string()
}

/// The band widget: the disclosed tile's detail, laid out by `GridView`
/// under the open row.
pub(crate) struct DetailBand {
    pub(crate) state: Rc<DetailState>,
    pub(crate) len_fn: Rc<dyn Fn() -> usize>,
    pub(crate) focused_index: Signal<Option<usize>>,
    /// The tiles' accessible names (`GridView::tile_a11y_label`); the band
    /// is named after its tile.
    #[allow(clippy::type_complexity)]
    pub(crate) name_fn: Option<Rc<dyn Fn(usize) -> String>>,
    /// The realized tiles, for a band whose tile has no name of its own to
    /// lend: it is then labelled by the tile's node.
    pub(crate) tile_map: Rc<RefCell<Vec<(usize, WidgetId)>>>,
    /// ↑ from the band's top: focus back on the grid, cursor on the tile.
    pub(crate) return_to_tile: ReturnToTile,
    /// The grid, which takes focus back when the band closes around it.
    pub(crate) grid: WidgetId,
    pub(crate) child: Option<WidgetId>,
}

impl std::fmt::Debug for DetailBand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DetailBand")
            .field("shown", &self.state.shown.get())
            .finish_non_exhaustive()
    }
}

impl Widget for DetailBand {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();
        state
            .expanded
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        state
            .version
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        state.band_id.set(Some(ctx.self_id()));
        let returning = state.focus_return.take();

        self.child = None;
        state.shown.set(None);
        state.enterable.set(false);
        state.awaiting_item.set(false);
        if let Some(index) = state.expanded.get().filter(|&i| i < (self.len_fn)()) {
            let (content, resident) = state.content_for(index);
            state.awaiting_item.set(!resident);
            if let Some(content) = content {
                let id = ctx.add_boxed(content);
                self.child = Some(id);
                state.shown.set(Some(index));
                state
                    .enterable
                    .set(ctx.first_focusable_descendant(id).is_some());
            }
        }

        // The band closed, or moved to content with nothing to take focus,
        // while focus was inside it. Destroying the old content left focus
        // nowhere, and the framework's restore looks for somewhere to put it
        // inside this band only, so the grid takes it back, with the cursor
        // on the tile the band belonged to. Content that can take focus gets
        // it from that restore.
        if let Some(tile) = returning
            && !state.enterable.get()
        {
            self.focused_index.set(Some(tile));
            ctx.focus(self.grid);
        }
        {
            // Read at the write, while the old content still holds focus:
            // by the rebuild the write causes, it has been destroyed.
            let watched = state.clone();
            let handle = state.expanded.observe(move |_| {
                if watched.focus_within.get() {
                    watched.focus_return.set(watched.shown.get());
                }
            });
            ctx.own_handle(handle);
        }

        let back = self.return_to_tile.clone();
        let on_key_state = state.clone();
        ctx.apply_self_handlers(
            HandlerSet::new()
                .clips_children(true)
                .focus_within(state.focus_within.clone())
                .on_key(move |event, ctx| {
                    // Reached only by a key the content did not use: an ↑ a
                    // list at its first row let through, or one on a control
                    // with no use for it.
                    let WidgetEvent::KeyDown {
                        key: Key::ArrowUp,
                        modifiers,
                        ..
                    } = event
                    else {
                        return EventResponse::Ignored;
                    };
                    if *modifiers != Modifiers::NONE {
                        return EventResponse::Ignored;
                    }
                    let Some(index) = on_key_state.shown.get() else {
                        return EventResponse::Ignored;
                    };
                    back(index, ctx);
                    EventResponse::Handled
                }),
        );
        self.child.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.child
            .and_then(|child| ctx.child_size(child, proposal))
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
        let Some(index) = self.state.shown.get().filter(|_| self.child.is_some()) else {
            builder.set_hidden();
            return;
        };
        builder.set_role(teksilo_core::accesskit::Role::Group);
        if let Some(name) = &self.name_fn {
            builder.set_name(name(index));
        } else if let Some(&(_, tile)) = self.tile_map.borrow().iter().find(|(i, _)| *i == index) {
            builder.push_labelled_by(widget_id_to_node_id(tile));
        } else {
            // The tile is outside the realized window and has no node to
            // lend its name, which happens to a band left open while the
            // grid is scrolled far from it: a group with no name is just
            // "group" to a reader moving through the window by landmarks.
            builder.set_name(band_name(index));
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn clips_children(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Row 1 of 58 dp rows (58..108), its band of 80 dp from 116.
    fn band_under_row_1() -> Insert {
        Insert {
            index: 4,
            row_top: 58.0,
            row_bottom: 108.0,
            band_top: 116.0,
            band_height: 80.0,
            extra: 88.0,
        }
    }

    /// The same band under row 3 (174..224).
    fn band_under_row_3() -> Insert {
        Insert {
            index: 10,
            row_top: 174.0,
            row_bottom: 224.0,
            band_top: 232.0,
            band_height: 80.0,
            extra: 88.0,
        }
    }

    #[test]
    fn a_band_below_the_top_edge_moves_nothing() {
        let band = Some(band_under_row_1());
        assert_eq!(anchor_shift(None, band, 0.0), 0.0, "opened");
        assert_eq!(anchor_shift(band, None, 0.0), 0.0, "closed");
        assert_eq!(anchor_shift(band, Some(band_under_row_3()), 100.0), 0.0);
        assert_eq!(anchor_shift(None, band, 116.0), 0.0, "opened at the top");
    }

    #[test]
    fn a_band_above_the_top_edge_moves_the_offset_by_what_it_adds() {
        let band = Some(band_under_row_1());
        assert_eq!(anchor_shift(None, band, 580.0), 88.0, "opened");
        assert_eq!(anchor_shift(band, None, 668.0), -88.0, "closed");
        assert_eq!(
            anchor_shift(band, Some(band_under_row_3()), 668.0),
            0.0,
            "moved, still above"
        );
        let mut taller = band_under_row_1();
        taller.band_height = 100.0;
        taller.extra = 108.0;
        assert_eq!(anchor_shift(band, Some(taller), 668.0), 20.0, "grew");
    }

    #[test]
    fn a_band_under_the_top_edge_keeps_the_rows_under_it_still() {
        // The top edge 30 dp into the band: row 2 is 58 dp down the screen.
        let band = Some(band_under_row_1());
        let mut taller = band_under_row_1();
        taller.band_height = 100.0;
        taller.extra = 108.0;
        assert_eq!(anchor_shift(band, Some(taller), 146.0), 20.0, "grew");
        assert_eq!(
            anchor_shift(band, None, 146.0),
            -88.0,
            "closed: row 2 stays 58 dp down, row 1 comes back above it"
        );
    }
}
