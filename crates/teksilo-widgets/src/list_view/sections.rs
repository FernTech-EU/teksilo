// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Section headers for `ListView`: where they sit, the row that draws one, and
//! the pinned copy of the current one.
//!
//! A header is not an item. It has no model index, selection never reaches it,
//! and the arrow keys step from item to item over it. So the headers are a
//! layer over the item geometry rather than rows inside it: [`RowMetrics`]
//! stays indexed by item, with its three height modes and its measured prefixes
//! untouched, and item `i` is drawn `(section_of(i) + 1) × header_height` below
//! where the metrics put it. Every geometric question the view asks goes
//! through [`ListGeometry`], which answers from the metrics alone when the view
//! has no sections, and converts between the two spaces when it has.
//!
//! The vertical layout follows `GridView`'s: a section's header, its rows
//! directly under it, then the list's `spacing` before the next header. An
//! empty section is a header with nothing under it, and no spacing after it.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::SurfaceRole;

use crate::common::row_metrics::{RowMetrics, SharedRowMetrics};
use crate::grid_view::sections::SectionProvider;

/// Builds the header widget of a section, from its index.
pub(crate) type HeaderFactory = Rc<dyn Fn(usize) -> Box<dyn Widget>>;

/// Height of a section header row when the application sets none: the same
/// 28 dp `GridView` uses, so the two views group alike by default.
pub(crate) const DEFAULT_SECTION_HEADER_HEIGHT: f32 = 28.0;

/// The application's section provider, and the first item of each section as
/// of the last time it was read.
pub(crate) struct SectionTable {
    provider: Box<dyn SectionProvider>,
    state: RefCell<SectionState>,
}

struct SectionState {
    /// Read the provider again before the next answer. The view's model
    /// observer sets it: a provider's counts are derived from the model, so a
    /// change the model reports may be a change to them.
    stale: bool,
    /// The model length `firsts` was clamped to.
    len: usize,
    /// The first item of each section, non-decreasing and clamped to `len`,
    /// so a provider whose counts disagree with the model never puts a header
    /// past the end. Empty when the model is: an empty list draws no headers.
    firsts: Vec<usize>,
}

impl SectionTable {
    pub(crate) fn new(provider: impl SectionProvider) -> Self {
        Self {
            provider: Box::new(provider),
            state: RefCell::new(SectionState {
                stale: true,
                len: 0,
                firsts: Vec::new(),
            }),
        }
    }

    /// Re-read the provider before the next geometric answer.
    pub(crate) fn invalidate(&self) {
        self.state.borrow_mut().stale = true;
    }

    /// The provider's title for `section`.
    pub(crate) fn title(&self, section: usize) -> String {
        self.provider.section_title(section)
    }

    /// How many sections a model of `len` items is drawn with.
    pub(crate) fn section_count(&self, len: usize) -> usize {
        self.with_firsts(len, |firsts| firsts.len())
    }

    fn with_firsts<R>(&self, len: usize, f: impl FnOnce(&[usize]) -> R) -> R {
        self.sync(len);
        f(&self.state.borrow().firsts)
    }

    fn sync(&self, len: usize) {
        {
            let state = self.state.borrow();
            // A length change re-reads too, so the table follows a model
            // that changed before the view's observer was installed.
            if !state.stale && state.len == len {
                return;
            }
        }
        // Read with no borrow held: the provider is application code.
        let counts = if len == 0 {
            Vec::new()
        } else {
            self.provider.section_counts()
        };
        let mut firsts = Vec::with_capacity(counts.len());
        let mut next = 0_usize;
        for count in counts {
            firsts.push(next.min(len));
            next = next.saturating_add(count);
        }
        *self.state.borrow_mut() = SectionState {
            stale: false,
            len,
            firsts,
        };
    }
}

/// The section half of a [`ListGeometry`].
#[derive(Clone)]
pub(crate) struct SectionLayer {
    pub(crate) table: Rc<SectionTable>,
    pub(crate) header_height: f32,
    /// The list's inter-row spacing, which also separates a section's last
    /// row from the next header.
    pub(crate) spacing: f32,
    /// The current section's header is pinned over the top of the viewport,
    /// so a row revealed by the keyboard must clear it.
    pub(crate) pinned: bool,
}

/// Where the pinned header goes this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PinnedPlacement {
    /// The section holding the top of the viewport.
    pub(crate) section: usize,
    /// Its own header has scrolled above the top, so the copy is needed.
    pub(crate) visible: bool,
    /// How far the next header has pushed the copy up, `-header_height..=0`.
    pub(crate) offset: f32,
}

/// Every geometric question a `ListView` asks: the item rows from the shared
/// [`RowMetrics`], with the section headers between them when there are any.
///
/// All indices are item indices and all `y` values are content coordinates
/// (the top of the content is 0, whatever the scroll). Each method is one
/// self-contained operation, like [`RowMetrics`]'s own: no borrow outlives it.
#[derive(Clone)]
pub(crate) struct ListGeometry {
    metrics: SharedRowMetrics,
    sections: Option<SectionLayer>,
}

impl ListGeometry {
    pub(crate) fn new(metrics: SharedRowMetrics, sections: Option<SectionLayer>) -> Self {
        Self { metrics, sections }
    }

    pub(crate) fn metrics(&self) -> &SharedRowMetrics {
        &self.metrics
    }

    pub(crate) fn section_table(&self) -> Option<&Rc<SectionTable>> {
        self.sections.as_ref().map(|layer| &layer.table)
    }

    /// The height of a header row; 0 without sections.
    pub(crate) fn header_height(&self) -> f32 {
        self.sections
            .as_ref()
            .map_or(0.0, |layer| layer.header_height)
    }

    /// Run `sectioned` when the list draws at least one header, `plain`
    /// otherwise. `plain` is handed the metrics as the view used them before
    /// sections existed, so a list without sections takes exactly that path.
    fn dispatch<R>(
        &self,
        len: usize,
        plain: impl FnOnce(&mut RowMetrics) -> R,
        sectioned: impl FnOnce(&mut Sectioned<'_>) -> R,
    ) -> R {
        match &self.sections {
            Some(layer) if len > 0 => layer.table.with_firsts(len, |firsts| {
                let mut metrics = self.metrics.borrow_mut();
                if firsts.is_empty() {
                    return plain(&mut metrics);
                }
                metrics.resize(len);
                sectioned(&mut Sectioned {
                    m: &mut metrics,
                    firsts,
                    len,
                    header: layer.header_height,
                    spacing: layer.spacing,
                    pinned: layer.pinned,
                })
            }),
            _ => plain(&mut self.metrics.borrow_mut()),
        }
    }

    /// Content height: every row, every header.
    pub(crate) fn total_height(&self, len: usize) -> f32 {
        self.dispatch(len, |m| m.total_height(len), |s| s.total())
    }

    /// Items `[start, end)` to realize at `scroll`, padded by `buffer` items
    /// each side.
    pub(crate) fn visible_range(
        &self,
        scroll: f32,
        viewport: f32,
        len: usize,
        buffer: usize,
    ) -> (usize, usize) {
        self.dispatch(
            len,
            |m| m.visible_range(scroll, viewport, len, buffer),
            |s| s.visible_range(scroll, viewport, buffer),
        )
    }

    /// `(top, height)` of item `index`.
    pub(crate) fn item_span(&self, index: usize, len: usize) -> (f32, f32) {
        self.dispatch(
            len,
            |m| (m.row_top(index), m.row_height(index)),
            |s| (s.item_top(index), s.m.row_height(index)),
        )
    }

    /// Top of `section`'s header; `None` when the list draws no headers.
    pub(crate) fn header_top(&self, section: usize, len: usize) -> Option<f32> {
        self.dispatch(
            len,
            |_| None,
            |s| (section < s.firsts.len()).then(|| s.header_top(section)),
        )
    }

    /// The first item of each section; empty when the list draws no headers.
    pub(crate) fn section_firsts(&self, len: usize) -> Vec<usize> {
        self.dispatch(len, |_| Vec::new(), |s| s.firsts.to_vec())
    }

    /// The headers to realize beside the realized items `[start, end)`: those
    /// between them, and those in the viewport.
    pub(crate) fn headers_to_realize(
        &self,
        items: (usize, usize),
        scroll: f32,
        viewport: f32,
        len: usize,
    ) -> Range<usize> {
        self.dispatch(
            len,
            |_| 0..0,
            |s| {
                let top = scroll.max(0.0);
                let bottom = top + viewport;
                let (start, end) = items;
                let (from, to) = if start < end {
                    let first = s.item_top(start);
                    let last = s.item_top(end - 1) + s.m.row_height(end - 1);
                    (first.min(top), last.max(bottom))
                } else {
                    (top, bottom)
                };
                s.headers_between(from, to)
            },
        )
    }

    /// The headers that intersect the viewport.
    pub(crate) fn visible_headers(&self, scroll: f32, viewport: f32, len: usize) -> Range<usize> {
        self.dispatch(
            len,
            |_| 0..0,
            |s| {
                let top = scroll.max(0.0);
                s.headers_between(top, top + viewport)
            },
        )
    }

    /// The scroll offset that brings item `index` into view, unchanged when it
    /// already is.
    pub(crate) fn scroll_for_ensure_visible(
        &self,
        index: usize,
        scroll: f32,
        viewport: f32,
        max_scroll: f32,
        len: usize,
    ) -> f32 {
        self.dispatch(
            len,
            |m| m.scroll_for_ensure_visible(index, scroll, viewport, max_scroll),
            |s| s.scroll_for_ensure_visible(index, scroll, viewport, max_scroll),
        )
    }

    /// The scroll offset that puts item `index` at the top of the viewport,
    /// before clamping.
    pub(crate) fn scroll_to_index_target(&self, index: usize, len: usize) -> f32 {
        self.dispatch(len, |m| m.row_top(index), |s| s.reveal_top(index))
    }

    /// The drop insertion point for content `y`: the boundary index in
    /// `0..=len`, and the content `y` of the line that shows it.
    pub(crate) fn insertion(&self, y: f32, len: usize) -> (usize, f32) {
        self.dispatch(
            len,
            |m| {
                m.resize(len);
                let index = m.insertion_index(y);
                (index, m.row_top(index))
            },
            |s| s.insertion(y),
        )
    }

    /// The item a page key lands on from `current`, before the caller's
    /// guarantee of progress.
    pub(crate) fn page_target(
        &self,
        current: usize,
        down: bool,
        viewport: f32,
        len: usize,
    ) -> usize {
        self.dispatch(
            len,
            |m| {
                m.resize(len);
                let target = if down {
                    m.row_top(current) + viewport
                } else {
                    (m.row_top(current) - viewport).max(0.0)
                };
                m.row_at(target)
            },
            |s| {
                let top = s.item_top(current);
                let target = if down {
                    top + viewport
                } else {
                    (top - viewport).max(0.0)
                };
                s.item_at(target)
            },
        )
    }

    /// Feed measured heights back, and return how far `scroll` must move so
    /// that what is on screen stays put. See [`RowMetrics::observe_measured`].
    pub(crate) fn observe_measured(
        &self,
        measured: &[(usize, f32)],
        scroll: f32,
        len: usize,
    ) -> f32 {
        self.dispatch(
            len,
            |m| m.observe_measured(measured, scroll),
            |s| {
                // The metrics decide "above the viewport" by comparing a row's
                // top with the offset they are handed, in their own space,
                // where no header exists. Handing them the content offset
                // would count every row within `(sections above) ×
                // header_height` of the top as above it, and the anchor
                // would then shift the rows the user is looking at.
                let in_metrics = s.metrics_offset(scroll);
                s.m.observe_measured(measured, in_metrics)
            },
        )
    }

    /// Where the pinned header goes at `scroll`; `None` when there is no
    /// header to pin.
    pub(crate) fn pinned_header(&self, scroll: f32, len: usize) -> Option<PinnedPlacement> {
        self.dispatch(len, |_| None, |s| Some(s.pinned(scroll.max(0.0))))
    }

    /// Reveal item `index` inside every enclosing scroll container, as
    /// [`chase_row_into_outer_view`](crate::common::row_metrics::chase_row_into_outer_view)
    /// does for a list without sections. `viewport` is the view's own absolute
    /// bounds and `scroll` its just-applied offset.
    pub(crate) fn chase_into_outer_view(
        &self,
        ctx: &mut EventContext,
        viewport: Rect,
        index: usize,
        scroll: f32,
        len: usize,
    ) {
        let span = self.dispatch(
            len,
            |_| None,
            |s| {
                let top = s.reveal_top(index);
                let bottom = s.item_top(index) + s.m.row_height(index);
                Some((top, bottom - top))
            },
        );
        match span {
            None => crate::common::row_metrics::chase_row_into_outer_view(
                ctx,
                &self.metrics,
                viewport,
                index,
                scroll,
            ),
            Some((top, height)) => ctx.ensure_visible(Rect::new(
                viewport.x,
                viewport.y + top - scroll,
                viewport.width,
                height,
            )),
        }
    }
}

/// What lies at a content `y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Header(usize),
    Item(usize),
}

/// The geometry of a list that draws at least one header: `firsts` is never
/// empty, `firsts[0] == 0`, and `len > 0`.
struct Sectioned<'a> {
    m: &'a mut RowMetrics,
    firsts: &'a [usize],
    len: usize,
    header: f32,
    spacing: f32,
    pinned: bool,
}

/// The first index in `0..n` for which `pred` is false, `pred` being true on a
/// prefix of the range (the contract of `slice::partition_point`).
fn partition(n: usize, mut pred: impl FnMut(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if pred(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

impl Sectioned<'_> {
    /// The section holding item `index`. An empty section shares its first
    /// item with the next one, so the last section starting at or before the
    /// item is the one that holds it.
    fn section_of(&self, index: usize) -> usize {
        self.firsts
            .partition_point(|&first| first <= index)
            .saturating_sub(1)
    }

    fn section_end(&self, section: usize) -> usize {
        self.firsts.get(section + 1).copied().unwrap_or(self.len)
    }

    /// Where item `first` starts in metrics space — or, for a section that
    /// starts past the last item, where an item after the last would.
    fn items_start(&mut self, first: usize) -> f32 {
        if first < self.len {
            self.m.row_top(first)
        } else {
            let last = self.len - 1;
            self.m.row_top(last) + self.m.row_height(last) + self.spacing
        }
    }

    fn header_top(&mut self, section: usize) -> f32 {
        self.items_start(self.firsts[section]) + section as f32 * self.header
    }

    fn item_top(&mut self, index: usize) -> f32 {
        self.m.row_top(index) + (self.section_of(index) + 1) as f32 * self.header
    }

    fn total(&mut self) -> f32 {
        let last_item = self.len - 1;
        let rows =
            self.m.total_height(self.len) + (self.section_of(last_item) + 1) as f32 * self.header;
        // Trailing empty sections draw headers past the last row.
        let headers = self.header_top(self.firsts.len() - 1) + self.header;
        rows.max(headers)
    }

    /// The last section whose header starts at or above `y`, or the first.
    fn section_at(&mut self, y: f32) -> usize {
        let sections = self.firsts.len();
        partition(sections, |s| self.header_top(s) <= y).saturating_sub(1)
    }

    fn hit(&mut self, y: f32) -> Hit {
        let section = self.section_at(y);
        if y < self.header_top(section) + self.header {
            return Hit::Header(section);
        }
        let (first, end) = (self.firsts[section], self.section_end(section));
        if first >= end {
            // Below the header of an empty last section: past the content.
            return Hit::Item(self.len - 1);
        }
        let in_metrics = y - (section + 1) as f32 * self.header;
        Hit::Item(self.m.row_at(in_metrics).clamp(first, end - 1))
    }

    /// The item at `y`; a header answers with the first item under it.
    fn item_at(&mut self, y: f32) -> usize {
        match self.hit(y) {
            Hit::Header(section) => self.firsts[section].min(self.len - 1),
            Hit::Item(index) => index,
        }
    }

    fn visible_range(&mut self, scroll: f32, viewport: f32, buffer: usize) -> (usize, usize) {
        let top = scroll.max(0.0);
        // As in `RowMetrics::visible_range`: a row whose top lands exactly on
        // the viewport bottom contributes no pixels.
        let bottom = (top + viewport - 0.01).max(top);
        let start = match self.hit(top) {
            Hit::Header(section) => self.firsts[section],
            Hit::Item(index) => index,
        };
        let end = match self.hit(bottom) {
            Hit::Header(section) => self.firsts[section],
            Hit::Item(index) => index + 1,
        };
        let end = end.max(start).min(self.len);
        (
            start.min(self.len).saturating_sub(buffer),
            (end + buffer).min(self.len),
        )
    }

    /// The sections whose header intersects `[from, to)`. A header whose
    /// bottom touches `from` counts, so the header right above the first
    /// realized row is realized with it.
    fn headers_between(&mut self, from: f32, to: f32) -> Range<usize> {
        let sections = self.firsts.len();
        let header = self.header;
        let start = partition(sections, |s| self.header_top(s) + header < from);
        let end = partition(sections, |s| self.header_top(s) < to);
        start..end.max(start)
    }

    /// The content `y` at which item `index` counts as revealed: its own top,
    /// or its header's when it is the first of its section — arriving at a
    /// section from below shows the header that names it — or the bottom of
    /// the pinned header, which covers the top of the viewport.
    fn reveal_top(&mut self, index: usize) -> f32 {
        let top = self.item_top(index);
        let section = self.section_of(index);
        if self.pinned || self.firsts[section] == index {
            top - self.header
        } else {
            top
        }
    }

    fn scroll_for_ensure_visible(
        &mut self,
        index: usize,
        scroll: f32,
        viewport: f32,
        max_scroll: f32,
    ) -> f32 {
        let reveal = self.reveal_top(index);
        let bottom = self.item_top(index) + self.m.row_height(index);
        if reveal < scroll {
            reveal.max(0.0)
        } else if bottom > scroll + viewport {
            (bottom - viewport).clamp(0.0, max_scroll.max(0.0))
        } else {
            scroll
        }
    }

    fn insertion(&mut self, y: f32) -> (usize, f32) {
        match self.hit(y) {
            // Dropped on a header: at the top of its section, which is where
            // the line is drawn, under the header.
            Hit::Header(section) => (self.firsts[section], self.header_top(section) + self.header),
            Hit::Item(index) => {
                let top = self.item_top(index);
                let height = self.m.row_height(index);
                // The threshold form `RowMetrics::insertion_index` uses: the
                // row plus its trailing gap, the midpoint snapping forward.
                if y - top < (height + self.spacing) * 0.5 {
                    return (index, top);
                }
                let next = index + 1;
                if next < self.len && self.section_of(next) == self.section_of(index) {
                    (next, self.item_top(next))
                } else {
                    // After the last row of a section: under that row, not
                    // under the next header.
                    (next, top + height)
                }
            }
        }
    }

    /// The offset in metrics space that has the same rows strictly above it
    /// as content offset `scroll` has above it.
    fn metrics_offset(&mut self, scroll: f32) -> f32 {
        match self.hit(scroll) {
            Hit::Header(section) => {
                let first = self.firsts[section];
                if first < self.len {
                    self.m.row_top(first)
                } else {
                    f32::INFINITY
                }
            }
            Hit::Item(index) => scroll - (self.section_of(index) + 1) as f32 * self.header,
        }
    }

    fn pinned(&mut self, scroll: f32) -> PinnedPlacement {
        let section = self.section_at(scroll);
        // Shown once its own header is off the top; while the header is
        // still there, the header is what the user sees.
        let visible = self.header_top(section) < scroll - 0.5;
        let offset = if section + 1 < self.firsts.len() {
            let next = self.header_top(section + 1) - scroll;
            (next - self.header).clamp(-self.header, 0.0)
        } else {
            0.0
        };
        PinnedPlacement {
            section,
            visible,
            offset,
        }
    }
}

/// An in-flow section header: the application's header widget, published as a
/// `Role::Heading` named by the section's title.
///
/// A heading because that is what a section title is to every platform, and
/// it is how a screen reader jumps from one group to the next. Not
/// `Role::RowHeader`, `GridView`'s choice for the same header: a row header
/// belongs to a row of a grid or a table, and a list has neither. The node
/// sits between the `Role::ListBoxOption` rows; it is not an item, so
/// AccessKit's item walks (the selection a platform reports) pass over it,
/// and every option keeps its position in the model.
#[derive(Debug)]
pub(crate) struct SectionHeaderRow {
    child: WidgetId,
    title: String,
    presentational: bool,
}

impl SectionHeaderRow {
    pub(crate) fn new(child: WidgetId, title: String) -> Self {
        Self {
            child,
            title,
            presentational: false,
        }
    }

    /// Publish nothing, for a view that is
    /// [`presentational`](super::ListView::presentational).
    pub(crate) fn presentational(mut self, presentational: bool) -> Self {
        self.presentational = presentational;
        self
    }
}

impl Widget for SectionHeaderRow {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        ctx.child_size(self.child, proposal)
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
        if self.presentational {
            builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
            return;
        }
        builder.set_role(teksilo_core::accesskit::Role::Heading);
        builder.set_name(self.title.clone());
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// The pinned copy of the current section's header, laid over the top of the
/// viewport by the `ListView` root.
///
/// One slot, rebuilt with the header of whichever section the root reports as
/// current. It paints an opaque surface so the rows scrolling beneath do not
/// show through.
///
/// Hidden from assistive technology, subtree included: the in-flow header is
/// the section's heading, and a second node with the same name would be read
/// twice, at a place in the reading order where the section does not begin.
pub(crate) struct PinnedSectionHeader {
    /// The section to show, written by the root's `place_children`.
    pub(crate) section: Signal<Option<usize>>,
    /// Bumped on every model change, which may retitle the section shown.
    pub(crate) refresh: Signal<u64>,
    /// The section this slot last built. The root shows the slot only while
    /// it matches the current section: a change of section lands one frame
    /// after the scroll that caused it, and for that frame the slot would
    /// show the previous section's title.
    pub(crate) built: Rc<Cell<Option<usize>>>,
    pub(crate) factory: HeaderFactory,
    pub(crate) table: Rc<SectionTable>,
    pub(crate) len_fn: Rc<dyn Fn() -> usize>,
    pub(crate) child: Option<WidgetId>,
}

impl std::fmt::Debug for PinnedSectionHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedSectionHeader")
            .field("built", &self.built.get())
            .finish_non_exhaustive()
    }
}

impl Widget for PinnedSectionHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.section
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        self.refresh
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        // Re-checked here rather than trusted: the section was current when
        // the root wrote it, and the model may have lost sections since. A
        // provider is free to index its own list without a bounds check.
        let count = self.table.section_count((self.len_fn)());
        let section = self.section.get().filter(|&s| s < count);
        self.built.set(section);
        self.child = section.map(|s| {
            let id = ctx.add_boxed((self.factory)(s));
            // A copy, not a stop: the listbox stays one Tab stop.
            ctx.set_tab_stop(id, false);
            id
        });
        self.child.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
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

    fn paint(&self, bounds: Rect, canvas: &mut teksilo_canvas::Canvas, ctx: &PaintContext) {
        if bounds.height > 0.5 {
            // `GridView`'s default pinned-header surface.
            canvas.fill_rect(bounds, SurfaceRole::Raised.resolve(&ctx.theme.colors));
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    //! The geometry, without a widget tree. Sections `[2, 3, 1]`, 20 dp rows,
    //! 10 dp headers, no spacing:
    //!
    //! ```text
    //!   0..10  header 0      60..80  item 2       120..130 header 2
    //!  10..30  item 0        80..100 item 3       130..150 item 5
    //!  30..50  item 1       100..120 item 4
    //!  50..60  header 1
    //! ```
    use super::*;

    struct Counts(Vec<usize>);
    impl SectionProvider for Counts {
        fn section_count(&self) -> usize {
            self.0.len()
        }
        fn items_in_section(&self, section: usize) -> usize {
            self.0[section]
        }
        fn section_title(&self, section: usize) -> String {
            format!("S{section}")
        }
    }

    fn geometry(
        counts: Vec<usize>,
        metrics: RowMetrics,
        spacing: f32,
        pinned: bool,
    ) -> ListGeometry {
        ListGeometry::new(
            Rc::new(RefCell::new(metrics)),
            Some(SectionLayer {
                table: Rc::new(SectionTable::new(Counts(counts))),
                header_height: 10.0,
                spacing,
                pinned,
            }),
        )
    }

    fn uniform(counts: Vec<usize>) -> ListGeometry {
        geometry(counts, RowMetrics::uniform(20.0, 0.0), 0.0, false)
    }

    #[test]
    fn rows_sit_below_every_header_above_them() {
        let g = uniform(vec![2, 3, 1]);
        let tops: Vec<f32> = (0..6).map(|i| g.item_span(i, 6).0).collect();
        assert_eq!(tops, vec![10.0, 30.0, 60.0, 80.0, 100.0, 130.0]);
        assert_eq!(g.header_top(1, 6), Some(50.0));
        assert_eq!(g.header_top(2, 6), Some(120.0));
        assert_eq!(g.total_height(6), 150.0);
    }

    #[test]
    fn spacing_separates_a_section_from_the_next_header_not_a_header_from_its_rows() {
        let g = geometry(vec![2, 1], RowMetrics::uniform(20.0, 4.0), 4.0, false);
        // Header 0, then items 0 and 1 four apart, then four before header 1,
        // then item 2 straight under it.
        assert_eq!(g.item_span(0, 3).0, 10.0);
        assert_eq!(g.item_span(1, 3).0, 34.0);
        assert_eq!(g.header_top(1, 3), Some(58.0));
        assert_eq!(g.item_span(2, 3).0, 68.0);
        assert_eq!(g.total_height(3), 88.0);
    }

    #[test]
    fn empty_sections_are_headers_with_nothing_under_them() {
        // A leading, a middle and a trailing empty section.
        let g = uniform(vec![0, 1, 0, 1, 0]);
        assert_eq!(g.header_top(0, 2), Some(0.0));
        assert_eq!(g.header_top(1, 2), Some(10.0));
        assert_eq!(g.item_span(0, 2).0, 20.0);
        assert_eq!(g.header_top(2, 2), Some(40.0));
        assert_eq!(g.header_top(3, 2), Some(50.0));
        assert_eq!(g.item_span(1, 2).0, 60.0);
        assert_eq!(g.header_top(4, 2), Some(80.0));
        assert_eq!(g.total_height(2), 90.0);
    }

    #[test]
    fn counts_that_disagree_with_the_model_are_clamped_to_it() {
        // More counted than there are: the second section starts past the
        // end and draws an empty header after the last row.
        let g = uniform(vec![5, 3]);
        assert_eq!(g.section_firsts(4), vec![0, 4]);
        // Fewer counted than there are: the rest belong to the last section.
        let g = uniform(vec![1, 1]);
        assert_eq!(g.item_span(3, 4).0, 20.0 + 3.0 * 20.0);
    }

    #[test]
    fn a_provider_with_no_sections_is_a_list_without_sections() {
        let g = uniform(vec![]);
        let plain = ListGeometry::new(Rc::new(RefCell::new(RowMetrics::uniform(20.0, 0.0))), None);
        assert_eq!(g.total_height(6), plain.total_height(6));
        assert_eq!(g.item_span(3, 6), plain.item_span(3, 6));
        assert_eq!(
            g.visible_range(25.0, 50.0, 6, 0),
            plain.visible_range(25.0, 50.0, 6, 0)
        );
        assert!(g.pinned_header(30.0, 6).is_none());
    }

    #[test]
    fn the_visible_range_counts_items_only() {
        let g = uniform(vec![2, 3, 1]);
        // 45..105: the bottom of item 1, header 1, items 2 to 4.
        assert_eq!(g.visible_range(45.0, 60.0, 6, 0), (1, 5));
        // 50..60 is header 1 alone: no item is on screen.
        assert_eq!(g.visible_range(50.0, 10.0, 6, 0), (2, 2));
        assert_eq!(g.visible_headers(45.0, 60.0, 6), 1..2);
    }

    #[test]
    fn a_drop_on_a_header_lands_at_the_top_of_its_section() {
        let g = uniform(vec![2, 3, 1]);
        assert_eq!(g.insertion(55.0, 6), (2, 60.0));
        // The lower half of a section's last row: after it, under it.
        assert_eq!(g.insertion(45.0, 6), (2, 50.0));
        // The upper half of a row: before it.
        assert_eq!(g.insertion(85.0, 6), (3, 80.0));
        // Past the end.
        assert_eq!(g.insertion(500.0, 6).0, 6);
    }

    #[test]
    fn revealing_the_first_row_of_a_section_reveals_its_header() {
        let g = uniform(vec![2, 3, 1]);
        // Scrolled down to 70: item 2 (60..80) is half off the top. Its
        // header, 50..60, comes back with it.
        assert_eq!(g.scroll_for_ensure_visible(2, 70.0, 40.0, 110.0, 6), 50.0);
        // Item 3 is not first; only its own top must clear.
        assert_eq!(g.scroll_for_ensure_visible(3, 90.0, 40.0, 110.0, 6), 80.0);
        // With the header pinned, every row must clear the pinned copy.
        let pinned = geometry(vec![2, 3, 1], RowMetrics::uniform(20.0, 0.0), 0.0, true);
        assert_eq!(
            pinned.scroll_for_ensure_visible(3, 75.0, 40.0, 110.0, 6),
            70.0
        );
        assert_eq!(pinned.scroll_to_index_target(4, 6), 90.0);
    }

    #[test]
    fn the_next_header_pushes_the_pinned_one_up() {
        let g = uniform(vec![2, 3, 1]);
        // At the very top header 0 is in place: nothing to pin.
        let p = g.pinned_header(0.0, 6).unwrap();
        assert_eq!((p.section, p.visible), (0, false));
        // At 20 header 0 is off the top and header 1 is 30 below it.
        let p = g.pinned_header(20.0, 6).unwrap();
        assert_eq!((p.section, p.visible, p.offset), (0, true, 0.0));
        // At 45 header 1 is 5 below the top: the copy is pushed up by 5.
        let p = g.pinned_header(45.0, 6).unwrap();
        assert_eq!((p.section, p.visible, p.offset), (0, true, -5.0));
        // At 55 section 1 is current, and its own header half off the top.
        let p = g.pinned_header(55.0, 6).unwrap();
        assert_eq!((p.section, p.visible, p.offset), (1, true, 0.0));
    }

    #[test]
    fn measured_rows_are_anchored_by_the_rows_above_the_viewport_only() {
        // Rows estimated at 50, scrolled so item 3 (the second of section 1)
        // is exactly at the top: content 10 + 2·50 + 10 + 50 = 170. Measuring
        // items 2 and 3 at 30 must shift the offset by item 2's change only;
        // item 3 starts at the top, so it is not above it.
        let g = geometry(
            vec![2, 3, 1],
            RowMetrics::auto_measure(50.0, 0.0),
            0.0,
            false,
        );
        assert_eq!(g.item_span(3, 6).0, 170.0);
        let delta = g.observe_measured(&[(2, 30.0), (3, 30.0)], 170.0, 6);
        assert!((delta + 20.0).abs() < 0.01, "delta {delta}");
    }
}
