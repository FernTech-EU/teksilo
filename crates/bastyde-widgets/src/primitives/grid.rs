//! Grid — a 2D layout container with row and column tracks.
//!
//! Supports fixed, fractional, and auto-sized tracks. Children are placed
//! in row-major order: child 0 at (row=0, col=0), child 1 at (row=0, col=1), etc.

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::signal::Prop;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

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
    column_gap: Prop<f32>,
    row_gap: Prop<f32>,
    child_ids: Vec<WidgetId>,
    pending: Vec<PendingChild>,
}

impl Grid {
    /// Create a new grid. Columns and rows default to a single Auto track each.
    pub fn new() -> Self {
        Self {
            columns: vec![TrackSize::Auto],
            rows: vec![TrackSize::Auto],
            column_gap: Prop::Static(0.0),
            row_gap: Prop::Static(0.0),
            child_ids: Vec::new(),
            pending: Vec::new(),
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

    /// Set the inter-column gap. Accepts static `f32` or `Signal<f32>`.
    pub fn column_gap(mut self, gap: impl Into<Prop<f32>>) -> Self {
        self.column_gap = gap.into();
        self
    }

    /// Set the inter-row gap. Accepts static `f32` or `Signal<f32>`.
    pub fn row_gap(mut self, gap: impl Into<Prop<f32>>) -> Self {
        self.row_gap = gap.into();
        self
    }

    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.pending.push(PendingChild::Id(id));
        self
    }

    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending.push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self {
        for widget in iter {
            self.pending.push(PendingChild::Deferred(Box::new(widget)));
        }
        self
    }

    pub fn child_opt(mut self, widget: Option<impl Widget + 'static>) -> Self {
        if let Some(w) = widget {
            self.pending.push(PendingChild::Deferred(Box::new(w)));
        }
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
                    let size = if i < child_sizes.len() {
                        child_sizes[i]
                    } else {
                        0.0
                    };
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
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let num_cols = self.columns.len().max(1);
        let num_rows = self.rows.len().max(1);

        // Pass 1: measure children with unspecified width so Auto
        // columns can size to content. Wrapping children (TextWidget
        // with markup, etc.) report their single-line width — that's
        // fine for column resolution because Auto picks the largest
        // single-line width, and Fractional gets re-measured in pass 2.
        let intrinsic_proposal = SizeProposal::unspecified();
        let mut col_max = vec![0.0_f32; num_cols];
        let mut row_max = vec![0.0_f32; num_rows];

        for (i, &child_id) in self.child_ids.iter().enumerate() {
            let (row, col) = self.cell_for(i);
            if row >= num_rows || col >= num_cols {
                continue;
            }
            if let Some(child_size) = ctx.child_size(child_id, intrinsic_proposal) {
                col_max[col] = col_max[col].max(child_size.width);
                row_max[row] = row_max[row].max(child_size.height);
            }
        }

        // Resolve column tracks against the parent's width proposal.
        let col_gap = self.column_gap.get();
        let row_gap = self.row_gap.get();
        let col_sizes = Self::resolve_tracks(&self.columns, col_gap, proposal.width, &col_max);

        // Pass 2: for every child whose column is *narrower* than its
        // intrinsic width (typical of Fractional columns receiving the
        // remainder after Auto columns claim their natural sizes),
        // re-measure with the resolved column width as the proposal so
        // wrapping content reports its real wrapped height. Without
        // this, a TextWidget-with-markup inside a Fractional column
        // reports a 1-line height in pass 1 but actually paints
        // multi-line, bleeding outside its assigned cell.
        for (i, &child_id) in self.child_ids.iter().enumerate() {
            let (row, col) = self.cell_for(i);
            if row >= num_rows || col >= num_cols {
                continue;
            }
            let needs_remeasure = matches!(self.columns[col], TrackSize::Fractional(_))
                && col_sizes[col] + 0.5 < col_max[col];
            if needs_remeasure {
                let constrained = SizeProposal {
                    width: Some(col_sizes[col]),
                    height: None,
                };
                if let Some(s) = ctx.child_size(child_id, constrained) {
                    row_max[row] = row_max[row].max(s.height);
                }
            }
        }

        let row_sizes = Self::resolve_tracks(&self.rows, row_gap, proposal.height, &row_max);

        let total_col_gap = col_gap * (num_cols as f32 - 1.0).max(0.0);
        let total_row_gap = row_gap * (num_rows as f32 - 1.0).max(0.0);
        let width = col_sizes.iter().sum::<f32>() + total_col_gap;
        let height = row_sizes.iter().sum::<f32>() + total_row_gap;

        Size::new(width, height).into()
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

        // Map each active child to its original cell index. The
        // framework filters dormant children out of `children`, so we
        // look up each child's position in `self.child_ids` (which
        // retains all children) to keep cell assignments stable.
        let original_indices: Vec<usize> = children
            .iter()
            .map(|c| {
                self.child_ids
                    .iter()
                    .position(|&id| id == c.id)
                    .unwrap_or(0)
            })
            .collect();

        // Pass 1: intrinsic sizes — same as size_that_fits.
        let intrinsic_proposal = SizeProposal::unspecified();
        let mut col_max = vec![0.0_f32; num_cols];
        let mut row_max = vec![0.0_f32; num_rows];

        for (i, child) in children.iter().enumerate() {
            let (row, col) = self.cell_for(original_indices[i]);
            if row >= num_rows || col >= num_cols {
                continue;
            }
            if let Some(child_size) = ctx.child_size(child.id, intrinsic_proposal) {
                col_max[col] = col_max[col].max(child_size.width);
                row_max[row] = row_max[row].max(child_size.height);
            }
        }

        let col_gap = self.column_gap.get();
        let row_gap = self.row_gap.get();
        let col_sizes = Self::resolve_tracks(&self.columns, col_gap, Some(bounds.width), &col_max);

        // Pass 2: re-measure Fractional cells whose column shrank, so
        // their row height reflects wrap-induced growth.
        for (i, child) in children.iter().enumerate() {
            let (row, col) = self.cell_for(original_indices[i]);
            if row >= num_rows || col >= num_cols {
                continue;
            }
            let needs_remeasure = matches!(self.columns[col], TrackSize::Fractional(_))
                && col_sizes[col] + 0.5 < col_max[col];
            if needs_remeasure {
                let constrained = SizeProposal {
                    width: Some(col_sizes[col]),
                    height: None,
                };
                if let Some(s) = ctx.child_size(child.id, constrained) {
                    row_max[row] = row_max[row].max(s.height);
                }
            }
        }

        let row_sizes = Self::resolve_tracks(&self.rows, row_gap, Some(bounds.height), &row_max);

        // Compute cell origins
        let mut col_origins = Vec::with_capacity(num_cols);
        let mut x = bounds.x;
        for (i, &w) in col_sizes.iter().enumerate() {
            col_origins.push(x);
            x += w;
            if i < num_cols - 1 {
                x += col_gap;
            }
        }

        let mut row_origins = Vec::with_capacity(num_rows);
        let mut y = bounds.y;
        for (i, &h) in row_sizes.iter().enumerate() {
            row_origins.push(y);
            y += h;
            if i < num_rows - 1 {
                y += row_gap;
            }
        }

        // Place each child in its cell
        for (i, child) in children.iter_mut().enumerate() {
            let (row, col) = self.cell_for(original_indices[i]);
            if row >= num_rows || col >= num_cols {
                child.size = Size::ZERO;
                continue;
            }
            child.origin = Point::new(col_origins[col], row_origins[row]);
            child.size = Size::new(col_sizes[col], row_sizes[row]);
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            self.child_ids = pending
                .into_iter()
                .map(|child| match child {
                    PendingChild::Id(id) => id,
                    PendingChild::Deferred(w) => ctx.add_boxed(w),
                })
                .collect();
        }
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.column_gap.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::Relayout,
        );
        self.row_gap.register_if_bound(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::Relayout,
        );
        self.child_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
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
                .columns(vec![
                    TrackSize::Fixed(50.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(2.0),
                ])
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

    #[test]
    fn dormant_child_preserves_cell_positions() {
        // 2x2 grid: a(0,0) b(0,1) c(1,0) d(1,1)
        // Making a dormant should leave b at (0,1), c at (1,0), d at (1,1).
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(50.0, 30.0));
        let b = tree.add(FixedLeaf(50.0, 30.0));
        let c = tree.add(FixedLeaf(50.0, 30.0));
        let d = tree.add(FixedLeaf(50.0, 30.0));
        let _grid = tree.add(
            Grid::new()
                .columns(vec![TrackSize::Fixed(80.0), TrackSize::Fixed(80.0)])
                .rows(vec![TrackSize::Fixed(40.0), TrackSize::Fixed(40.0)])
                .column_gap(10.0)
                .row_gap(5.0)
                .add_child(a)
                .add_child(b)
                .add_child(c)
                .add_child(d),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // Before dormant: b at col 1 → x=90 (80+10), c at row 1 → y=45 (40+5)
        assert!((tree.bounds(b).x - 90.0).abs() < 0.01);
        assert!((tree.bounds(c).y - 45.0).abs() < 0.01);
        assert!((tree.bounds(d).x - 90.0).abs() < 0.01);
        assert!((tree.bounds(d).y - 45.0).abs() < 0.01);

        // Make a dormant — b, c, d keep their original cells
        tree.set_dormant(a);
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // b stays at cell (0,1)
        assert!(
            (tree.bounds(b).x - 90.0).abs() < 0.01,
            "b.x should stay at 90, got {}",
            tree.bounds(b).x
        );
        assert!(
            (tree.bounds(b).y - 0.0).abs() < 0.01,
            "b.y should stay at 0, got {}",
            tree.bounds(b).y
        );
        // c stays at cell (1,0)
        assert!(
            (tree.bounds(c).x - 0.0).abs() < 0.01,
            "c.x should stay at 0, got {}",
            tree.bounds(c).x
        );
        assert!(
            (tree.bounds(c).y - 45.0).abs() < 0.01,
            "c.y should stay at 45, got {}",
            tree.bounds(c).y
        );
        // d stays at cell (1,1)
        assert!(
            (tree.bounds(d).x - 90.0).abs() < 0.01,
            "d.x should stay at 90, got {}",
            tree.bounds(d).x
        );
        assert!(
            (tree.bounds(d).y - 45.0).abs() < 0.01,
            "d.y should stay at 45, got {}",
            tree.bounds(d).y
        );
    }
}
