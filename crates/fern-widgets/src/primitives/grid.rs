//! Grid — a 2D layout container with row and column tracks.
//!
//! Supports fixed, fractional, and auto-sized tracks. Children are placed
//! in row-major order: child 0 at (row=0, col=0), child 1 at (row=0, col=1), etc.

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::state::State;
use fern_core::widget::{IntoWidgetTree, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// How a grid track (row or column) is sized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackSize {
    /// Fixed size in logical pixels.
    Fixed(f32),
    /// Fraction of remaining space after fixed and auto tracks are allocated.
    Fractional(f32),
    /// Sized to fit content (uses the largest child in that track).
    Auto,
}

/// A 2D grid layout container.
#[derive(Debug)]
pub struct Grid {
    columns: Vec<TrackSize>,
    rows: Vec<TrackSize>,
    column_gap: f32,
    row_gap: f32,
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
    visible_when_state: Option<State<bool>>,
    enabled_when_state: Option<State<bool>>,
}

impl Grid {
    /// Create a new grid. Columns and rows default to a single Auto track each.
    pub fn new() -> Self {
        Self {
            columns: vec![TrackSize::Auto],
            rows: vec![TrackSize::Auto],
            column_gap: 0.0,
            row_gap: 0.0,
            child_ids: Vec::new(),
            pending: Vec::new(),
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    pub fn columns(mut self, columns: Vec<TrackSize>) -> Self {
        self.columns = columns;
        self
    }

    pub fn rows(mut self, rows: Vec<TrackSize>) -> Self {
        self.rows = rows;
        self
    }

    pub fn column_gap(mut self, gap: f32) -> Self {
        self.column_gap = gap;
        self
    }

    pub fn row_gap(mut self, gap: f32) -> Self {
        self.row_gap = gap;
        self
    }

    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.pending.push(PendingChild::Id(id));
        self
    }

    pub fn child(mut self, widget: impl IntoWidgetTree) -> Self {
        self.pending.push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn children(mut self, iter: impl IntoIterator<Item = impl IntoWidgetTree>) -> Self {
        for widget in iter {
            self.pending.push(PendingChild::Deferred(Box::new(widget)));
        }
        self
    }

    pub fn child_opt(mut self, widget: Option<impl IntoWidgetTree>) -> Self {
        if let Some(w) = widget {
            self.pending.push(PendingChild::Deferred(Box::new(w)));
        }
        self
    }

    pub fn visible_when(mut self, state: State<bool>) -> Self {
        self.visible_when_state = Some(state);
        self
    }

    pub fn enabled_when(mut self, state: State<bool>) -> Self {
        self.enabled_when_state = Some(state);
        self
    }

    /// Resolve track sizes given available space, child sizes, and track definitions.
    fn resolve_tracks(
        tracks: &[TrackSize],
        gap: f32,
        available: Option<f32>,
        child_sizes: &[f32], // max child size per track
    ) -> Vec<f32> {
        let n = tracks.len();
        let total_gap = gap * (n as f32 - 1.0).max(0.0);

        // Phase 1: resolve Fixed and Auto tracks
        let mut resolved = vec![0.0_f32; n];
        let mut used = 0.0_f32;
        let mut total_fr = 0.0_f32;

        for (i, track) in tracks.iter().enumerate() {
            match *track {
                TrackSize::Fixed(px) => {
                    resolved[i] = px;
                    used += px;
                }
                TrackSize::Auto => {
                    let size = if i < child_sizes.len() { child_sizes[i] } else { 0.0 };
                    resolved[i] = size;
                    used += size;
                }
                TrackSize::Fractional(fr) => {
                    total_fr += fr;
                }
            }
        }

        // Phase 2: distribute remaining space to Fractional tracks
        let remaining = available
            .map(|a| (a - total_gap - used).max(0.0))
            .unwrap_or(0.0);

        if total_fr > 0.0 {
            for (i, track) in tracks.iter().enumerate() {
                if let TrackSize::Fractional(fr) = *track {
                    resolved[i] = remaining * fr / total_fr;
                }
            }
        }

        resolved
    }

    /// Get the (row, col) cell index for a given child index.
    fn cell_for(&self, child_index: usize) -> (usize, usize) {
        let cols = self.columns.len().max(1);
        (child_index / cols, child_index % cols)
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Grid {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let num_cols = self.columns.len().max(1);
        let num_rows = self.rows.len().max(1);

        // Query each child's intrinsic size
        let child_proposal = SizeProposal::unspecified();
        let mut col_max = vec![0.0_f32; num_cols];
        let mut row_max = vec![0.0_f32; num_rows];

        for (i, &child_id) in self.child_ids.iter().enumerate() {
            let (row, col) = self.cell_for(i);
            if row >= num_rows || col >= num_cols {
                continue;
            }
            if let Some(child_size) = ctx.child_size(child_id, child_proposal) {
                col_max[col] = col_max[col].max(child_size.width);
                row_max[row] = row_max[row].max(child_size.height);
            }
        }

        let col_sizes = Self::resolve_tracks(&self.columns, self.column_gap, proposal.width, &col_max);
        let row_sizes = Self::resolve_tracks(&self.rows, self.row_gap, proposal.height, &row_max);

        let total_col_gap = self.column_gap * (num_cols as f32 - 1.0).max(0.0);
        let total_row_gap = self.row_gap * (num_rows as f32 - 1.0).max(0.0);
        let width = col_sizes.iter().sum::<f32>() + total_col_gap;
        let height = row_sizes.iter().sum::<f32>() + total_row_gap;

        Size::new(width, height)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let num_cols = self.columns.len().max(1);
        let num_rows = self.rows.len().max(1);

        // Query intrinsic sizes for Auto track resolution
        let child_proposal = SizeProposal::unspecified();
        let mut col_max = vec![0.0_f32; num_cols];
        let mut row_max = vec![0.0_f32; num_rows];

        for (i, child) in children.iter().enumerate() {
            let (row, col) = self.cell_for(i);
            if row >= num_rows || col >= num_cols {
                continue;
            }
            if let Some(child_size) = ctx.child_size(child.id, child_proposal) {
                col_max[col] = col_max[col].max(child_size.width);
                row_max[row] = row_max[row].max(child_size.height);
            }
        }

        let col_sizes = Self::resolve_tracks(&self.columns, self.column_gap, Some(bounds.width), &col_max);
        let row_sizes = Self::resolve_tracks(&self.rows, self.row_gap, Some(bounds.height), &row_max);

        // Compute cell origins
        let mut col_origins = Vec::with_capacity(num_cols);
        let mut x = bounds.x;
        for (i, &w) in col_sizes.iter().enumerate() {
            col_origins.push(x);
            x += w;
            if i < num_cols - 1 {
                x += self.column_gap;
            }
        }

        let mut row_origins = Vec::with_capacity(num_rows);
        let mut y = bounds.y;
        for (i, &h) in row_sizes.iter().enumerate() {
            row_origins.push(y);
            y += h;
            if i < num_rows - 1 {
                y += self.row_gap;
            }
        }

        // Place each child in its cell
        for (i, child) in children.iter_mut().enumerate() {
            let (row, col) = self.cell_for(i);
            if row >= num_rows || col >= num_cols {
                child.size = Size::ZERO;
                continue;
            }
            child.origin = Point::new(col_origins[col], row_origins[row]);
            child.size = Size::new(col_sizes[col], row_sizes[row]);
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

    fn take_pending_children(&mut self) -> Vec<PendingChild> {
        std::mem::take(&mut self.pending)
    }

    fn set_resolved_children(&mut self, ids: Vec<WidgetId>) {
        self.child_ids = ids;
    }

    fn take_visible_when(&mut self) -> Option<State<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<State<bool>> {
        self.enabled_when_state.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn fixed_tracks_place_children_correctly() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 30.0));
        let b = tree.add(FixedLeaf(50.0, 30.0));
        let c = tree.add(FixedLeaf(50.0, 30.0));
        let d = tree.add(FixedLeaf(50.0, 30.0));
        let _grid = tree.add(
            Grid::new()
                .columns(vec![TrackSize::Fixed(100.0), TrackSize::Fixed(100.0)])
                .rows(vec![TrackSize::Fixed(50.0), TrackSize::Fixed(50.0)])
                .add_child(a)
                .add_child(b)
                .add_child(c)
                .add_child(d),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // a at (0,0), b at (0,1), c at (1,0), d at (1,1)
        assert!((tree.bounds(a).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(a).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(b).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(c).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(c).y - 50.0).abs() < 0.01);
        assert!((tree.bounds(d).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(d).y - 50.0).abs() < 0.01);
    }

    #[test]
    fn auto_tracks_size_to_content() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(60.0, 25.0));
        let b = tree.add(FixedLeaf(40.0, 35.0));
        let _grid = tree.add(
            Grid::new()
                .columns(vec![TrackSize::Auto, TrackSize::Auto])
                .rows(vec![TrackSize::Auto])
                .add_child(a)
                .add_child(b),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // Column 0 sized to 60 (widest child), column 1 to 40
        assert!((tree.bounds(a).width - 60.0).abs() < 0.01);
        assert!((tree.bounds(b).width - 40.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 60.0).abs() < 0.01);
    }

    #[test]
    fn fractional_tracks_distribute_remaining_space() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(10.0, 10.0));
        let b = tree.add(FixedLeaf(10.0, 10.0));
        let c = tree.add(FixedLeaf(10.0, 10.0));
        let _grid = tree.add(
            Grid::new()
                .columns(vec![TrackSize::Fixed(50.0), TrackSize::Fractional(1.0), TrackSize::Fractional(2.0)])
                .rows(vec![TrackSize::Fixed(40.0)])
                .add_child(a)
                .add_child(b)
                .add_child(c),
        );
        tree.layout(SizeProposal::exact(350.0, 100.0));

        // Remaining: 350 - 50 = 300, split 1:2 → 100, 200
        assert!((tree.bounds(a).width - 50.0).abs() < 0.01);
        assert!((tree.bounds(b).width - 100.0).abs() < 0.01);
        assert!((tree.bounds(c).width - 200.0).abs() < 0.01);
    }

    #[test]
    fn gaps_between_tracks() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(10.0, 10.0));
        let b = tree.add(FixedLeaf(10.0, 10.0));
        let c = tree.add(FixedLeaf(10.0, 10.0));
        let d = tree.add(FixedLeaf(10.0, 10.0));
        let _grid = tree.add(
            Grid::new()
                .columns(vec![TrackSize::Fixed(50.0), TrackSize::Fixed(50.0)])
                .rows(vec![TrackSize::Fixed(30.0), TrackSize::Fixed(30.0)])
                .column_gap(10.0)
                .row_gap(5.0)
                .add_child(a)
                .add_child(b)
                .add_child(c)
                .add_child(d),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert!((tree.bounds(b).x - 60.0).abs() < 0.01); // 50 + 10 gap
        assert!((tree.bounds(c).y - 35.0).abs() < 0.01); // 30 + 5 gap
    }

    #[test]
    fn grid_intrinsic_size() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(40.0, 20.0));
        let b = tree.add(FixedLeaf(60.0, 30.0));
        let grid = tree.add(
            Grid::new()
                .columns(vec![TrackSize::Auto, TrackSize::Auto])
                .rows(vec![TrackSize::Auto])
                .column_gap(10.0)
                .add_child(a)
                .add_child(b),
        );
        tree.layout(SizeProposal::unspecified());

        let gb = tree.bounds(grid);
        // Width: 40 + 10 + 60 = 110
        assert!((gb.width - 110.0).abs() < 0.01);
        // Height: max(20, 30) = 30
        assert!((gb.height - 30.0).abs() < 0.01);
    }
}
