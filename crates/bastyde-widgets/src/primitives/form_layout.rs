//! FormLayout — a two-column form layout where children are added as
//! label/field pairs, with support for full-width rows.

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// A resolved form row after `build()`.
#[derive(Debug, Clone)]
enum FormRow {
    /// A label/field pair occupying two columns.
    Pair(WidgetId, WidgetId),
    /// A single widget spanning the full width.
    FullWidth(WidgetId),
}

/// A pending form row before `build()`.
enum PendingFormRow {
    Pair(PendingChild, PendingChild),
    FullWidth(PendingChild),
}

impl std::fmt::Debug for PendingFormRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pair(..) => f.write_str("PendingFormRow::Pair(..)"),
            Self::FullWidth(..) => f.write_str("PendingFormRow::FullWidth(..)"),
        }
    }
}

/// A two-column form layout with auto-sized label column.
///
/// Children are added as label/field pairs via [`line()`](Self::line) or as
/// full-width rows via [`full_width()`](Self::full_width). The label column
/// auto-sizes to the widest label; the field column takes the remaining
/// space.
///
/// ```text
/// ┌─ label col ─┐ gap ┌── field col ──────────────┐
/// │ Name:       │     │ [___________________]      │
/// │ Email:      │     │ [___________________]      │
/// ├─────────────┴─────┴────────────────────────────┤
/// │ ── Advanced ──────────────────────────────────  │  ← full_width
/// ├─ label col ─┐ gap ┌── field col ──────────────┐
/// │ Port:       │     │ [____]                     │
/// └─────────────┘     └────────────────────────────┘
/// ```
#[derive(Debug)]
pub struct FormLayout {
    rows: Vec<FormRow>,
    pending_rows: Vec<PendingFormRow>,
    label_gap: f32,
    row_spacing: f32,
    a11y_label: Option<String>,
}

impl FormLayout {
    /// Create an empty form layout.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            pending_rows: Vec::new(),
            label_gap: 0.0,
            row_spacing: 0.0,
            a11y_label: None,
        }
    }

    /// Horizontal gap between the label column and the field column.
    pub fn label_gap(mut self, gap: f32) -> Self {
        self.label_gap = gap;
        self
    }

    /// Vertical gap between rows.
    pub fn row_spacing(mut self, spacing: f32) -> Self {
        self.row_spacing = spacing;
        self
    }

    /// Set an accessible name for this form. When set, the widget emits
    /// the `Role::Form` landmark so assistive-technology users can
    /// navigate directly to it and distinguish it from other forms on
    /// the page. When unset, the widget demotes to a presentational
    /// `GenericContainer` — an unnamed landmark is worse than no
    /// landmark for AT users.
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.a11y_label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(bastyde_i18n::LocalizedString::literal(label))
    }

    /// Add a label/field pair row.
    pub fn line(mut self, label: impl Widget + 'static, field: impl Widget + 'static) -> Self {
        self.pending_rows.push(PendingFormRow::Pair(
            PendingChild::Deferred(Box::new(label)),
            PendingChild::Deferred(Box::new(field)),
        ));
        self
    }

    /// Add a label/field pair row with pre-registered widget IDs.
    pub fn line_ids(mut self, label_id: WidgetId, field_id: WidgetId) -> Self {
        self.pending_rows.push(PendingFormRow::Pair(
            PendingChild::Id(label_id),
            PendingChild::Id(field_id),
        ));
        self
    }

    /// Add a full-width row spanning both columns.
    pub fn full_width(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_rows
            .push(PendingFormRow::FullWidth(PendingChild::Deferred(Box::new(
                widget,
            ))));
        self
    }

    /// Add a full-width row with a pre-registered widget ID.
    pub fn full_width_id(mut self, id: WidgetId) -> Self {
        self.pending_rows
            .push(PendingFormRow::FullWidth(PendingChild::Id(id)));
        self
    }

    /// Flatten all rows into a child ID list.
    fn all_child_ids(&self) -> Vec<WidgetId> {
        let mut ids = Vec::new();
        for row in &self.rows {
            match row {
                FormRow::Pair(l, f) => {
                    ids.push(*l);
                    ids.push(*f);
                }
                FormRow::FullWidth(id) => ids.push(*id),
            }
        }
        ids
    }

    /// Width of the label column (max intrinsic width of all pair labels).
    fn compute_label_width(&self, ctx: &LayoutContext) -> f32 {
        let mut max_w = 0.0_f32;
        for row in &self.rows {
            if let FormRow::Pair(label_id, _) = row
                && let Some(s) = ctx.child_size(*label_id, SizeProposal::unspecified())
            {
                max_w = max_w.max(s.width);
            }
        }
        max_w
    }
}

fn resolve_pending(p: PendingChild, ctx: &mut bastyde_core::build_context::BuildContext) -> WidgetId {
    match p {
        PendingChild::Id(id) => id,
        PendingChild::Deferred(w) => ctx.add_boxed(w),
    }
}

impl Default for FormLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for FormLayout {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let pending = std::mem::take(&mut self.pending_rows);
        if !pending.is_empty() {
            self.rows = pending
                .into_iter()
                .map(|row| match row {
                    PendingFormRow::Pair(label, field) => {
                        let l = resolve_pending(label, ctx);
                        let f = resolve_pending(field, ctx);
                        FormRow::Pair(l, f)
                    }
                    PendingFormRow::FullWidth(child) => {
                        FormRow::FullWidth(resolve_pending(child, ctx))
                    }
                })
                .collect();
        }
        self.all_child_ids()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        if self.rows.is_empty() {
            return (proposal.resolve(0.0, 0.0)).into();
        }

        let label_col_width = self.compute_label_width(ctx);

        // Determine available width and field column width.
        let (available_width, field_col_width) = if let Some(w) = proposal.width {
            let fcw = (w - label_col_width - self.label_gap).max(0.0);
            (w, fcw)
        } else {
            // Unbounded: compute intrinsic width.
            let mut max_field_w = 0.0_f32;
            let mut max_full_w = 0.0_f32;
            for row in &self.rows {
                match row {
                    FormRow::Pair(_, field_id) => {
                        if let Some(s) = ctx.child_size(*field_id, SizeProposal::unspecified()) {
                            max_field_w = max_field_w.max(s.width);
                        }
                    }
                    FormRow::FullWidth(id) => {
                        if let Some(s) = ctx.child_size(*id, SizeProposal::unspecified()) {
                            max_full_w = max_full_w.max(s.width);
                        }
                    }
                }
            }
            let pair_width = if label_col_width > 0.0 || max_field_w > 0.0 {
                label_col_width + self.label_gap + max_field_w
            } else {
                0.0
            };
            let total_w = pair_width.max(max_full_w);
            let fcw = (total_w - label_col_width - self.label_gap).max(0.0);
            (total_w, fcw)
        };

        // Compute total height from row heights.
        let label_proposal = SizeProposal::with_width(label_col_width);
        let field_proposal = SizeProposal::with_width(field_col_width);
        let full_proposal = SizeProposal::with_width(available_width);

        let mut total_height = 0.0_f32;
        let mut active_count = 0_usize;

        for row in &self.rows {
            let row_h = match row {
                FormRow::Pair(label_id, field_id) => {
                    let lh = ctx.child_size(*label_id, label_proposal).map(|s| s.height);
                    let fh = ctx.child_size(*field_id, field_proposal).map(|s| s.height);
                    match (lh, fh) {
                        (Some(l), Some(f)) => l.max(f),
                        (Some(l), None) => l,
                        (None, Some(f)) => f,
                        (None, None) => continue, // both dormant
                    }
                }
                FormRow::FullWidth(id) => match ctx.child_size(*id, full_proposal) {
                    Some(s) => s.height,
                    None => continue, // dormant
                },
            };
            total_height += row_h;
            active_count += 1;
        }

        if active_count > 1 {
            total_height += self.row_spacing * (active_count as f32 - 1.0);
        }

        Size::new(available_width, total_height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }

        let label_col_width = self.compute_label_width(ctx);
        let field_col_width = (bounds.width - label_col_width - self.label_gap).max(0.0);

        let rtl = ctx.is_rtl();
        let (label_x, field_x) = if rtl {
            (bounds.x + field_col_width + self.label_gap, bounds.x)
        } else {
            (bounds.x, bounds.x + label_col_width + self.label_gap)
        };

        let label_proposal = SizeProposal::with_width(label_col_width);
        let field_proposal = SizeProposal::with_width(field_col_width);
        let full_proposal = SizeProposal::with_width(bounds.width);

        let mut child_idx = 0;
        let mut y = bounds.y;
        let mut first_active = true;

        for row in &self.rows {
            match row {
                FormRow::Pair(label_id, field_id) => {
                    let label_active =
                        child_idx < children.len() && children[child_idx].id == *label_id;
                    let field_check_idx = if label_active {
                        child_idx + 1
                    } else {
                        child_idx
                    };
                    let field_active = field_check_idx < children.len()
                        && children[field_check_idx].id == *field_id;

                    if !label_active && !field_active {
                        continue; // entire row dormant
                    }

                    if !first_active {
                        y += self.row_spacing;
                    }
                    first_active = false;

                    let label_h = if label_active {
                        ctx.child_size(*label_id, label_proposal)
                            .map(|s| s.height)
                            .unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    let field_h = if field_active {
                        ctx.child_size(*field_id, field_proposal)
                            .map(|s| s.height)
                            .unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    let row_h = label_h.max(field_h);

                    if label_active {
                        children[child_idx].origin = Point::new(label_x, y);
                        children[child_idx].size = Size::new(label_col_width, row_h);
                        child_idx += 1;
                    }
                    if field_active {
                        children[child_idx].origin = Point::new(field_x, y);
                        children[child_idx].size = Size::new(field_col_width, row_h);
                        child_idx += 1;
                    }

                    y += row_h;
                }
                FormRow::FullWidth(id) => {
                    if child_idx >= children.len() || children[child_idx].id != *id {
                        continue; // dormant
                    }

                    if !first_active {
                        y += self.row_spacing;
                    }
                    first_active = false;

                    let child_h = ctx
                        .child_size(*id, full_proposal)
                        .map(|s| s.height)
                        .unwrap_or(0.0);

                    children[child_idx].origin = Point::new(bounds.x, y);
                    children[child_idx].size = Size::new(bounds.width, child_h);
                    child_idx += 1;

                    y += child_h;
                }
            }
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        match self.a11y_label.as_deref() {
            Some(name) => {
                builder.set_role(bastyde_core::accesskit::Role::Form);
                builder.set_name(name);
            }
            None => {
                builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.all_child_ids()
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
    fn basic_form_places_label_and_field() {
        let mut tree = WidgetTree::new();
        let label = tree.add(FixedLeaf(60.0, 20.0));
        let field = tree.add(FixedLeaf(100.0, 25.0));
        let _form = tree.add(FormLayout::new().line_ids(label, field));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        assert!((tree.bounds(label).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(label).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(field).x - 60.0).abs() < 0.01);
        assert!((tree.bounds(field).y - 0.0).abs() < 0.01);
        // Row height = max(20, 25) = 25
        assert!((tree.bounds(label).height - 25.0).abs() < 0.01);
        assert!((tree.bounds(field).height - 25.0).abs() < 0.01);
    }

    #[test]
    fn label_column_auto_sizes_to_widest() {
        let mut tree = WidgetTree::new();
        let l1 = tree.add(FixedLeaf(60.0, 20.0));
        let f1 = tree.add(FixedLeaf(100.0, 20.0));
        let l2 = tree.add(FixedLeaf(100.0, 20.0)); // wider label
        let f2 = tree.add(FixedLeaf(80.0, 20.0));
        let _form = tree.add(FormLayout::new().line_ids(l1, f1).line_ids(l2, f2));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // Label column = 100 (widest label). Both fields start at x=100.
        assert!((tree.bounds(f1).x - 100.0).abs() < 0.01);
        assert!((tree.bounds(f2).x - 100.0).abs() < 0.01);
        // Label column width = 100 for both labels
        assert!((tree.bounds(l1).width - 100.0).abs() < 0.01);
        assert!((tree.bounds(l2).width - 100.0).abs() < 0.01);
    }

    #[test]
    fn full_width_row_spans_entire_width() {
        let mut tree = WidgetTree::new();
        let fw = tree.add(FixedLeaf(50.0, 30.0));
        let _form = tree.add(FormLayout::new().full_width_id(fw));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert!((tree.bounds(fw).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(fw).width - 400.0).abs() < 0.01);
        assert!((tree.bounds(fw).height - 30.0).abs() < 0.01);
    }

    #[test]
    fn mixed_pair_and_full_width_rows() {
        let mut tree = WidgetTree::new();
        let l1 = tree.add(FixedLeaf(80.0, 20.0));
        let f1 = tree.add(FixedLeaf(100.0, 25.0));
        let fw = tree.add(FixedLeaf(200.0, 30.0));
        let l2 = tree.add(FixedLeaf(60.0, 20.0));
        let f2 = tree.add(FixedLeaf(100.0, 20.0));
        let _form = tree.add(
            FormLayout::new()
                .line_ids(l1, f1)
                .full_width_id(fw)
                .line_ids(l2, f2),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // Row 0 (Pair): height 25, y=0
        assert!((tree.bounds(l1).y - 0.0).abs() < 0.01);
        // Row 1 (FullWidth): height 30, y=25
        assert!((tree.bounds(fw).y - 25.0).abs() < 0.01);
        // Row 2 (Pair): height 20, y=55
        assert!((tree.bounds(l2).y - 55.0).abs() < 0.01);
    }

    #[test]
    fn label_gap_applied() {
        let mut tree = WidgetTree::new();
        let label = tree.add(FixedLeaf(80.0, 20.0));
        let field = tree.add(FixedLeaf(100.0, 20.0));
        let _form = tree.add(FormLayout::new().label_gap(12.0).line_ids(label, field));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // Field starts at label_width + gap = 80 + 12 = 92
        assert!((tree.bounds(field).x - 92.0).abs() < 0.01);
    }

    #[test]
    fn row_spacing_applied() {
        let mut tree = WidgetTree::new();
        let l1 = tree.add(FixedLeaf(60.0, 20.0));
        let f1 = tree.add(FixedLeaf(100.0, 25.0));
        let l2 = tree.add(FixedLeaf(60.0, 20.0));
        let f2 = tree.add(FixedLeaf(100.0, 20.0));
        let _form = tree.add(
            FormLayout::new()
                .row_spacing(10.0)
                .line_ids(l1, f1)
                .line_ids(l2, f2),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // Row 0 at y=0, height=25. Row 1 at y=25+10=35.
        assert!((tree.bounds(l1).y - 0.0).abs() < 0.01);
        assert!((tree.bounds(l2).y - 35.0).abs() < 0.01);
    }

    #[test]
    fn intrinsic_height_sums_rows() {
        let mut tree = WidgetTree::new();
        let l1 = tree.add(FixedLeaf(60.0, 20.0));
        let f1 = tree.add(FixedLeaf(100.0, 25.0));
        let l2 = tree.add(FixedLeaf(60.0, 30.0));
        let f2 = tree.add(FixedLeaf(100.0, 20.0));
        let form = tree.add(
            FormLayout::new()
                .row_spacing(5.0)
                .line_ids(l1, f1)
                .line_ids(l2, f2),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });

        // Row 0: max(20,25)=25. Row 1: max(30,20)=30. Total: 25+5+30=60
        assert!((tree.bounds(form).height - 60.0).abs() < 0.01);
    }

    #[test]
    fn field_column_gets_remaining_width() {
        let mut tree = WidgetTree::new();
        let label = tree.add(FixedLeaf(80.0, 20.0));
        let field = tree.add(FixedLeaf(100.0, 20.0));
        let _form = tree.add(FormLayout::new().label_gap(10.0).line_ids(label, field));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // Field width = 400 - 80 - 10 = 310
        assert!((tree.bounds(field).width - 310.0).abs() < 0.01);
    }

    #[test]
    fn single_pair_row() {
        let mut tree = WidgetTree::new();
        let label = tree.add(FixedLeaf(70.0, 20.0));
        let field = tree.add(FixedLeaf(100.0, 20.0));
        let _form = tree.add(FormLayout::new().line_ids(label, field));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        assert!((tree.bounds(label).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(field).x - 70.0).abs() < 0.01);
    }

    #[test]
    fn empty_form() {
        let mut tree = WidgetTree::new();
        let form = tree.add(FormLayout::new());
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });

        assert!((tree.bounds(form).height - 0.0).abs() < 0.01);
    }

    #[test]
    fn dormant_row_excluded_from_layout() {
        let mut tree = WidgetTree::new();
        let l1 = tree.add(FixedLeaf(60.0, 20.0));
        let f1 = tree.add(FixedLeaf(100.0, 25.0));
        let l2 = tree.add(FixedLeaf(60.0, 20.0));
        let f2 = tree.add(FixedLeaf(100.0, 30.0));
        let l3 = tree.add(FixedLeaf(60.0, 20.0));
        let f3 = tree.add(FixedLeaf(100.0, 20.0));
        let form = tree.add(
            FormLayout::new()
                .row_spacing(10.0)
                .line_ids(l1, f1)
                .line_ids(l2, f2)
                .line_ids(l3, f3),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Before dormant: row 0 at y=0 (h=25), row 1 at y=35 (h=30), row 2 at y=75
        assert!((tree.bounds(l3).y - 75.0).abs() < 0.01);

        // Make row 1 dormant
        tree.set_dormant(l2);
        tree.set_dormant(f2);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });

        // Row 2 should move up: y = 25 + 10 = 35
        assert!((tree.bounds(l3).y - 35.0).abs() < 0.01);
        assert!((tree.bounds(f3).y - 35.0).abs() < 0.01);
        // Form height = 25 + 10 + 20 = 55
        assert!((tree.bounds(form).height - 55.0).abs() < 0.01);
    }

    #[test]
    fn unbounded_width_uses_intrinsic() {
        let mut tree = WidgetTree::new();
        let l1 = tree.add(FixedLeaf(80.0, 20.0));
        let f1 = tree.add(FixedLeaf(200.0, 20.0));
        let form = tree.add(FormLayout::new().label_gap(10.0).line_ids(l1, f1));
        tree.layout(SizeProposal {
            width: None,
            height: Some(200.0),
        });

        // Intrinsic width = 80 + 10 + 200 = 290
        assert!((tree.bounds(form).width - 290.0).abs() < 0.01);
    }

    #[test]
    fn deferred_line_api_works() {
        let mut tree = WidgetTree::new();
        let form = tree.add(
            FormLayout::new()
                .label_gap(10.0)
                .line(FixedLeaf(70.0, 20.0), FixedLeaf(150.0, 25.0)),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });

        // Form should have 2 children, height = max(20, 25) = 25
        assert!((tree.bounds(form).height - 25.0).abs() < 0.01);
    }

    #[test]
    fn rtl_swaps_label_and_field_columns() {
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(bastyde_core::environment::LayoutDirection::RightToLeft);
        let label = tree.add(FixedLeaf(80.0, 20.0));
        let field = tree.add(FixedLeaf(100.0, 20.0));
        let _form = tree.add(FormLayout::new().label_gap(10.0).line_ids(label, field));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // field_col_width = 400 - 80 - 10 = 310
        // RTL: field at x=0, label at x=310+10=320
        assert!((tree.bounds(field).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(label).x - 320.0).abs() < 0.01);
    }
}
