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

/// The pinned copy as the root last placed it on screen: the section it
/// shows, and its top and bottom in viewport coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PinnedShown {
    pub(crate) section: usize,
    pub(crate) top: f32,
    pub(crate) bottom: f32,
}

/// The section headers a build realizes.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HeadersToRealize {
    /// Every header to build, ascending.
    pub(crate) sections: Vec<usize>,
    /// The part of `sections` around the viewport, which a scroll has to
    /// leave before a missing header can come into view.
    pub(crate) near_viewport: Range<usize>,
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

    /// The headers to realize beside the realized items `[start, end)`.
    ///
    /// Two sets: the header of every section a realized item belongs to,
    /// however far above the viewport it is, and the headers within `buffer`
    /// header heights of the viewport. The first keeps every row a screen
    /// reader can reach under a heading naming its section, deep in a long
    /// section too. The second is the rows' buffer, applied to the headers a
    /// run of empty sections scrolls through. Nothing between the two is
    /// built: that stretch can hold any number of empty sections, and building
    /// it would make hundreds of headers where ten are on screen.
    pub(crate) fn headers_to_realize(
        &self,
        items: (usize, usize),
        scroll: f32,
        viewport: f32,
        len: usize,
        buffer: usize,
    ) -> HeadersToRealize {
        self.dispatch(
            len,
            |_| HeadersToRealize::default(),
            |s| {
                let top = scroll.max(0.0);
                let pad = buffer as f32 * s.header;
                let near_viewport = s.headers_between(top - pad, top + viewport + pad);
                let (start, end) = items;
                let mut sections: Vec<usize> = Vec::new();
                for index in start..end.min(s.len) {
                    let section = s.section_of(index);
                    if sections.last() != Some(&section) {
                        sections.push(section);
                    }
                }
                sections.extend(near_viewport.clone());
                sections.sort_unstable();
                sections.dedup();
                HeadersToRealize {
                    sections,
                    near_viewport,
                }
            },
        )
    }

    /// The section whose in-flow header the pinned copy covers at `scroll`;
    /// `None` without pinning, or while that header is still in place.
    pub(crate) fn covered_by_pinned_header(&self, scroll: f32, len: usize) -> Option<usize> {
        let pinned = self.sections.as_ref().is_some_and(|layer| layer.pinned);
        if !pinned {
            return None;
        }
        self.pinned_header(scroll, len)
            .filter(|placement| placement.visible)
            .map(|placement| placement.section)
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

    /// The drop insertion point for a pointer at viewport `y`: the boundary
    /// index in `0..=len`, and the viewport `y` of the line that shows it.
    ///
    /// `dragged` are the rows of this list being moved, empty for rows coming
    /// from elsewhere; with sections, they decide on which side of a header
    /// the line goes (see [`Sectioned::insertion_line`]). `pinned` is the
    /// pinned copy as it is on screen: a pointer over it is over its
    /// section's header, and a line it would cover is moved to its edge.
    pub(crate) fn drop_at(
        &self,
        y: f32,
        scroll: f32,
        len: usize,
        dragged: &[usize],
        pinned: Option<PinnedShown>,
    ) -> (usize, f32) {
        self.dispatch(
            len,
            |m| {
                m.resize(len);
                let index = m.insertion_index(y + scroll);
                (index, m.row_top(index) - scroll)
            },
            |s| s.drop_at(y, scroll, dragged, pinned),
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

    /// The boundary a drop at content `y` inserts at. A header is the
    /// boundary before its section's first item.
    fn insertion_index(&mut self, y: f32) -> usize {
        match self.hit(y) {
            Hit::Header(section) => self.firsts[section],
            Hit::Item(index) => {
                let top = self.item_top(index);
                let height = self.m.row_height(index);
                // The threshold form `RowMetrics::insertion_index` uses: the
                // row plus its trailing gap, the midpoint snapping forward.
                if y - top < (height + self.spacing) * 0.5 {
                    index
                } else {
                    index + 1
                }
            }
        }
    }

    /// The content `y` of the line for boundary `b`: where the dropped rows
    /// will be drawn once the source has moved them.
    ///
    /// Inside a section, that is the top of item `b`. Between two sections the
    /// boundary is one index and two places on screen, above the header and
    /// under it, and the provider, not the view, decides which section the
    /// rows join. The line assumes its counts survive the move, as they do for
    /// [`grouping_sections`](crate::grouping_sections) and for any provider
    /// that does not read the items. The rows then land from position `b`
    /// minus the dragged rows above `b`, which the source takes out first:
    /// rows dragged down from above the boundary close the section before the
    /// header, and rows dragged up from below it, or dropped from another
    /// view, open the section after it. A provider that groups by the items'
    /// content puts a moved item in the section of its own key wherever it is
    /// dropped, and the view cannot know where that is.
    fn insertion_line(&mut self, b: usize, dragged: &[usize]) -> f32 {
        // The sections whose first item is `b` have their headers in the gap.
        let starting = self.firsts.partition_point(|&first| first < b)
            ..self.firsts.partition_point(|&first| first <= b);
        if starting.is_empty() && b < self.len {
            return self.item_top(b);
        }
        // Past the last row with no header after it, or closing the section
        // above the header. `b > 0` either way: section 0 starts at 0, and a
        // row dragged from above `b` is a row before it.
        if starting.is_empty() || dragged.iter().any(|&row| row < b) {
            let last = b - 1;
            return self.item_top(last) + self.m.row_height(last);
        }
        // Under the last of those headers: an item at `b` belongs to the last
        // section starting at or before it, past any empty one.
        self.header_top(starting.end - 1) + self.header
    }

    fn drop_at(
        &mut self,
        y: f32,
        scroll: f32,
        dragged: &[usize],
        pinned: Option<PinnedShown>,
    ) -> (usize, f32) {
        let pinned = pinned.filter(|p| p.section < self.firsts.len());
        let b = match pinned {
            // The copy stands for its section's header, wherever the
            // in-flow header has scrolled to.
            Some(p) if (p.top..p.bottom).contains(&y) => self.firsts[p.section],
            _ => self.insertion_index(y + scroll),
        };
        let line = self.insertion_line(b, dragged);
        let Some(p) = pinned else {
            return (b, line - scroll);
        };
        // The copy hides whatever is under it, and its own section's header
        // has scrolled away above it: a line before that header is drawn at
        // the copy's top edge, and a line the copy would cover at its bottom
        // edge, under it.
        let shown = if line <= self.header_top(p.section) {
            p.top
        } else {
            (line - scroll).max(p.bottom)
        };
        (b, shown)
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
/// The in-flow header is there to read: the body pane realizes the header of
/// every section a realized row belongs to, however far above it has
/// scrolled.
pub(crate) struct PinnedSectionHeader {
    /// The section to show, written by the root's scroll observer and its
    /// `place_children`.
    pub(crate) section: Signal<Option<usize>>,
    /// Bumped on every model change, which may retitle the section shown.
    pub(crate) refresh: Signal<u64>,
    /// The section this slot last built. The root shows the slot only while
    /// it matches the current section: a section decided during layout (an
    /// offset the layout pass itself wrote) is built a frame later, and for
    /// that frame the slot would show the previous section's title.
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
            // A picture of the header, not a second set of its controls.
            // Focus could not be announced here, in a subtree hidden from
            // assistive technology; the in-flow header's controls are the
            // Tab stops, once it is back in view.
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
    fn the_line_at_a_section_boundary_is_where_the_moved_rows_land() {
        let g = uniform(vec![2, 3, 1]);
        let drop = |y: f32, dragged: &[usize]| g.drop_at(y, 0.0, 6, dragged, None);
        // Header S1 and the lower half of item 1 are the same boundary, 2.
        // Item 5 dragged up to it becomes the first of S1: under the header.
        assert_eq!(drop(55.0, &[5]), (2, 60.0));
        assert_eq!(drop(45.0, &[5]), (2, 60.0));
        // Item 0 dragged down to it becomes the last of S0, since S0 keeps
        // its two items: above the header, under item 1.
        assert_eq!(drop(55.0, &[0]), (2, 50.0));
        assert_eq!(drop(45.0, &[0]), (2, 50.0));
        // A row from another view, which removes nothing, takes position 2.
        assert_eq!(drop(45.0, &[]), (2, 60.0));
        // Inside a section the line is the next row's top, either way.
        assert_eq!(drop(85.0, &[0]), (3, 80.0));
        assert_eq!(drop(85.0, &[5]), (3, 80.0));
        // Past the end, after the last row.
        assert_eq!(drop(500.0, &[0]), (6, 150.0));
    }

    #[test]
    fn a_line_under_a_run_of_empty_sections_goes_under_the_last_of_them() {
        // `[1, 0, 0, 1]`: header 0, item 0, headers 1 to 3, item 1. The
        // boundary 1 has three headers in it; an item landing at 1 belongs
        // to section 3, the last that starts there.
        let g = uniform(vec![1, 0, 0, 1]);
        assert_eq!(g.header_top(3, 2), Some(50.0));
        assert_eq!(g.drop_at(35.0, 0.0, 2, &[], None), (1, 60.0));
        assert_eq!(g.drop_at(35.0, 0.0, 2, &[0], None), (1, 30.0));
    }

    #[test]
    fn a_pointer_over_the_pinned_copy_is_over_its_sections_header() {
        // Sections of ten; scrolled to 295, S1's header (210..220) is off the
        // top and its copy covers 0..10. Item 14 (300..320) shows at 5..25,
        // the upper half of it under the copy.
        let g = uniform(vec![10; 10]);
        let shown = Some(PinnedShown {
            section: 1,
            top: 0.0,
            bottom: 10.0,
        });
        let drop = |y: f32, dragged: &[usize]| g.drop_at(y, 295.0, 100, dragged, shown);
        // Over the copy: boundary 10, the top of S1, with the line under the
        // copy rather than under the header scrolled away above it.
        assert_eq!(drop(5.0, &[15]), (10, 10.0));
        // Dragged down from S0 it closes S0, above S1's header: drawn at the
        // copy's top edge, the nearest the viewport has.
        assert_eq!(drop(5.0, &[3]), (10, 0.0));
        // Just under the copy, in the upper half of item 14: before it, at a
        // gap the copy hides, so the line moves down to the copy's bottom.
        assert_eq!(drop(12.0, &[50]), (14, 10.0));
        // Without the copy, the pointer at 5 is over item 14 itself.
        assert_eq!(g.drop_at(5.0, 295.0, 100, &[15], None), (14, 5.0));
    }

    #[test]
    fn only_the_headers_near_the_viewport_and_of_the_realized_rows_are_realized() {
        // Five items, then two hundred empty sections: header s ≥ 1 at
        // 100 + 10·s. Scrolled to 1500 the viewport holds S140..S149, and the
        // realized items 0..5 sit in S0.
        let mut counts = vec![5];
        counts.extend(std::iter::repeat_n(0, 200));
        let g = uniform(counts);
        let realized = g.headers_to_realize((0, 5), 1500.0, 100.0, 5, 2);
        // Two headers' height of buffer each side of the viewport (S137's
        // bottom touches the buffer's top), and S0; none of S1..S136.
        assert_eq!(realized.near_viewport, 137..152);
        let mut expected = vec![0];
        expected.extend(137..152);
        assert_eq!(realized.sections, expected);
    }

    #[test]
    fn the_header_of_a_long_sections_realized_rows_is_realized_however_far_above() {
        // One section of 100: its header is 1000 dp above a viewport at item
        // 50, and it is still the heading the realized rows are read under.
        let g = uniform(vec![100]);
        let realized = g.headers_to_realize((45, 60), 1010.0, 200.0, 100, 5);
        assert_eq!(realized.sections, vec![0]);
        assert!(realized.near_viewport.is_empty());
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
