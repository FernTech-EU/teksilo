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
//!   list with focus and a scroll offset of its own) must survive both.
//!   Opening, closing or moving the band rebuilds the band alone, so no tile
//!   node is replaced either.

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
use teksilo_data::RowState;

use super::TileContext;
use super::body_pane::ReadItemFn;
use super::layout::GridLayoutStrategy;
use super::layout::strategy::{TileRect, VisibleTileRange};
use crate::data_views::{RowSelection, default_placeholder};

/// Builds a band's content for the disclosed tile. `None`: the tile has
/// nothing to disclose, and the band takes no space.
pub(crate) type DetailBuilder<T> = Rc<dyn Fn(&TileContext<'_, T>) -> Option<Box<dyn Widget>>>;

/// The exact height of the band for a tile.
pub(crate) type DetailHeightFn = Rc<dyn Fn(usize) -> f32>;

/// Below this, a band height is unchanged: measurements wobble in the last
/// bits, and a wobble must not move the scroll offset.
const HEIGHT_EPSILON: f32 = 0.01;

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

    /// Disclose `index`, or close it when it is the disclosed tile.
    pub(crate) fn toggle(&self, index: usize) {
        if self.expanded.get() == Some(index) {
            self.expanded.set(None);
        } else {
            self.expanded.set(Some(index));
        }
    }

    /// Whether the band's height comes from measuring it.
    pub(crate) fn measures(&self) -> bool {
        self.height_fn.is_none()
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
    /// Bottom of the open row: the band starts one row gap under it.
    row_bottom: f32,
    band_height: f32,
    /// How far everything below the open row moves: a row gap and the band.
    extra: f32,
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

    fn insert(&self, viewport_width: f32) -> Option<Insert> {
        let index = self.state.open((self.len_fn)())?;
        let row = self.inner.tile_rect(index, viewport_width);
        let band_height = self.state.height(index, self.inner.estimated_row_height());
        Some(Insert {
            index,
            row_top: row.y,
            row_bottom: row.y + row.height,
            band_height,
            extra: self.row_gap + band_height,
        })
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
                y: ins.row_bottom + self.row_gap,
                width: (viewport_width - self.inset.horizontal()).max(0.0),
                height: ins.band_height,
            },
        ))
    }

    /// Record the band's measured `height` and return the scroll correction
    /// that keeps the content under the viewport top still: the band's change
    /// in height when the band starts above the top, nothing otherwise. The
    /// rule a measured row follows (`VariableRowGrid::observe_measured`).
    pub(crate) fn observe_band_measured(
        &self,
        index: usize,
        height: f32,
        scroll_y: f32,
        viewport_width: f32,
    ) -> f32 {
        let Some((open, before)) = self.band_rect(viewport_width) else {
            return 0.0;
        };
        if open != index || (height - before.height).abs() <= HEIGHT_EPSILON {
            return 0.0;
        }
        self.state.measured.set(Some((index, height)));
        if before.y < scroll_y {
            height - before.height
        } else {
            0.0
        }
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

    fn visible_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        viewport_width: f32,
        item_count: usize,
    ) -> VisibleTileRange {
        let Some(ins) = self.insert(viewport_width) else {
            return self
                .inner
                .visible_range(scroll_y, viewport_height, viewport_width, item_count);
        };
        // The viewport with the band cut out of it. Both ends map through
        // the same monotone function, so the tiles between them are exactly
        // the tiles on screen.
        let top = Self::to_inner(scroll_y, &ins);
        let bottom = Self::to_inner(scroll_y + viewport_height, &ins);
        self.inner
            .visible_range(top, (bottom - top).max(0.0), viewport_width, item_count)
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
        let top = Self::to_inner(scroll_y, &ins);
        let bottom = Self::to_inner(scroll_y + viewport_height, &ins);
        self.inner
            .headers_in_range(top, (bottom - top).max(0.0), viewport_width)
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

/// The band widget: the disclosed tile's detail, laid out by `GridView`
/// under the open row.
pub(crate) struct DetailBand<T: 'static> {
    pub(crate) state: Rc<DetailState>,
    pub(crate) builder: DetailBuilder<T>,
    pub(crate) read_item_fn: ReadItemFn<T>,
    pub(crate) row_state_fn: Rc<dyn Fn(usize) -> RowState>,
    pub(crate) len_fn: Rc<dyn Fn() -> usize>,
    /// For the content's `TileContext`: the grid coordinates of the tile.
    pub(crate) strategy: Rc<dyn GridLayoutStrategy>,
    pub(crate) viewport_width: Rc<Cell<f32>>,
    pub(crate) selection: Option<RowSelection>,
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
    pub(crate) child: Option<WidgetId>,
}

impl<T: 'static> std::fmt::Debug for DetailBand<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DetailBand")
            .field("shown", &self.state.shown.get())
            .finish_non_exhaustive()
    }
}

impl<T: 'static> DetailBand<T> {
    fn content(&self, index: usize) -> Option<Box<dyn Widget>> {
        let width = self.viewport_width.get();
        let (row, col) = self.strategy.tile_row_col(index, width);
        let is_selected = self
            .selection
            .as_ref()
            .is_some_and(|s| s.is_selected(index));
        let is_focused = self.focused_index.get() == Some(index);
        let mut content = None;
        let resident = (self.read_item_fn)(index, &mut |item| {
            content = (self.builder)(&TileContext {
                index,
                row,
                col,
                item,
                is_selected,
                is_focused,
            });
        });
        if resident {
            content
        } else {
            // A tile of a lazy source whose item has not arrived yet: hold the
            // band's place, as a tile does, rather than collapse it.
            ((self.row_state_fn)(index) == RowState::Loading).then(default_placeholder)
        }
    }
}

impl<T: 'static> Widget for DetailBand<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();
        state
            .expanded
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        state.band_id.set(Some(ctx.self_id()));

        self.child = None;
        state.shown.set(None);
        state.enterable.set(false);
        if let Some(index) = state.expanded.get().filter(|&i| i < (self.len_fn)())
            && let Some(content) = self.content(index)
        {
            let id = ctx.add_boxed(content);
            self.child = Some(id);
            state.shown.set(Some(index));
            state
                .enterable
                .set(ctx.first_focusable_descendant(id).is_some());
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
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn clips_children(&self) -> bool {
        true
    }
}
