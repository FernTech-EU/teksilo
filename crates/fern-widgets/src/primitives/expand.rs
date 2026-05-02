use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_id::WidgetId;
use fern_tokens::Alignment;

/// Layout modifier that claims space along one or both axes from its parent
/// and stretches its child to fill it.
///
/// In an `HStack` / `VStack`, `Expand` participates in flex slack
/// distribution: it returns a `LayoutResponse` with `flex` (default `1.0`),
/// so the parent stack hands it a share of the leftover space proportional
/// to flex. Default basis is **zero** — the wrapped child's natural size
/// does NOT count in the rigid pool, which gives clean ratio layouts. Call
/// [`Expand::respect_intrinsic`] to switch to **auto** basis (CSS
/// flex-basis: auto), where the child's natural size acts as a floor and
/// flex adds slack on top.
///
/// `Expand::new()` is the common case: claim space, fill the child.
/// Use `.flex(n)` to change the ratio (e.g. 1:2 by pairing `flex(1)` with
/// `flex(2)`). Use `.align_child(...)` to opt out of fill and align the
/// child at its natural size within the claimed bounds.
#[derive(Debug)]
pub struct Expand {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    horizontal: bool,
    vertical: bool,
    flex: f32,
    /// When `Some`, the child is laid out at its natural size and aligned;
    /// when `None`, the child is stretched to the full Expand bounds.
    child_alignment: Option<Alignment>,
    /// When `true`, the wrapped child's natural size acts as a floor on the
    /// flex axis (CSS flex-basis: auto). When `false` (default), the
    /// wanted size on flex axes is `0` so the parent stack divides bounds
    /// purely by flex weight (CSS flex-basis: 0).
    respect_intrinsic: bool,
}

impl Expand {
    /// Expand on both axes. Default `flex(1)`, child fills bounds.
    pub fn new() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            horizontal: true,
            vertical: true,
            flex: 1.0,
            child_alignment: None,
            respect_intrinsic: false,
        }
    }

    /// Expand on the horizontal axis only.
    pub fn horizontal() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            horizontal: true,
            vertical: false,
            flex: 1.0,
            child_alignment: None,
            respect_intrinsic: false,
        }
    }

    /// Expand on the vertical axis only.
    pub fn vertical() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            horizontal: false,
            vertical: true,
            flex: 1.0,
            child_alignment: None,
            respect_intrinsic: false,
        }
    }

    /// Override the flex weight reported to a parent stack. `flex(0)` opts
    /// out of slack distribution (the wrapper still claims any offered
    /// proposal, useful inside non-stack containers). Default: `1.0`.
    pub fn flex(mut self, flex: f32) -> Self {
        self.flex = flex.max(0.0);
        self
    }

    /// Opt out of stretching the child. The child is laid out at its
    /// natural size and positioned within the Expand's bounds according
    /// to `alignment`.
    pub fn align_child(mut self, alignment: Alignment) -> Self {
        self.child_alignment = Some(alignment);
        self
    }

    /// Switch to **auto** flex basis — the wrapped child's natural size
    /// acts as a floor on each flex axis, and the parent stack adds slack
    /// on top via the flex weight. Useful when the wrapper sits inside an
    /// unconstrained parent (e.g. an outer `VStack` with `height = None`),
    /// where the default zero-basis would let the child overflow because
    /// the parent has no bound to share.
    ///
    /// Trade-off: with `respect_intrinsic`, exact ratios bend by content
    /// width — `[Expand::flex(1).child(60), Expand::flex(2).child(40)]` in
    /// 300 px gives `60 + 66 = 126` and `40 + 133 = 173` rather than
    /// `100 / 200`. Without it (the default), the same layout splits
    /// exactly `100 / 200`.
    pub fn respect_intrinsic(mut self) -> Self {
        self.respect_intrinsic = true;
        self
    }

    /// Set child by pre-registered ID.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Default for Expand {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Expand {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let child_size = self
            .child_id
            .and_then(|id| ctx.child_size(id, SizeProposal::unspecified()))
            .unwrap_or(Size::ZERO);

        // On a flex axis, the wanted size is either:
        //   - 0 (default, zero-basis): give us pure slack.
        //   - child's natural size (auto-basis): respect the wrapped child
        //     as a floor; parent adds slack on top.
        // If the parent gave us a concrete proposal on that axis, claim it
        // (this matters for non-stack parents — ZStack, Padding, FixedSize).
        let basis_w = if self.respect_intrinsic {
            child_size.width
        } else {
            0.0
        };
        let basis_h = if self.respect_intrinsic {
            child_size.height
        } else {
            0.0
        };

        let w = if self.horizontal {
            proposal.width.unwrap_or(basis_w)
        } else {
            child_size.width
        };
        let h = if self.vertical {
            proposal.height.unwrap_or(basis_h)
        } else {
            child_size.height
        };
        LayoutResponse::flexible(Size::new(w, h), self.flex)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            if let Some(alignment) = self.child_alignment {
                // Align mode: child takes its natural size; we position it.
                let child_size = ctx
                    .child_size(child.id, SizeProposal::unspecified())
                    .unwrap_or(bounds.size());
                let rtl = ctx.is_rtl();
                let (dx, dy) = alignment.resolve(
                    (child_size.width, child_size.height),
                    (bounds.width, bounds.height),
                    rtl,
                );
                child.origin = Point::new(bounds.x + dx, bounds.y + dy);
                child.size = child_size;
            } else {
                // Fill mode (default): child takes the full Expand bounds.
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn expand_at_root_fills_proposal() {
        // At the tree root, the proposal IS the bounds. Expand claims it
        // and fills its child to those bounds.
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let expand = tree.add(Expand::new().child_id(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let eb = tree.bounds(expand);
        assert!((eb.width - 200.0).abs() < 0.01);
        assert!((eb.height - 100.0).abs() < 0.01);

        // Default fill mode: child stretches to full Expand bounds.
        let cb = tree.bounds(child);
        assert!((cb.width - 200.0).abs() < 0.01);
        assert!((cb.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn align_child_top_trailing() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let _expand = tree.add(
            Expand::new()
                .align_child(Alignment::TOP_TRAILING)
                .child_id(child),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(child);
        // Child stays at natural 40x20, placed top-trailing.
        assert!((cb.width - 40.0).abs() < 0.01);
        assert!((cb.height - 20.0).abs() < 0.01);
        assert!((cb.x - 160.0).abs() < 0.01); // 200 - 40
        assert!((cb.y - 0.0).abs() < 0.01); // top
    }

    #[test]
    fn flex_default_is_one() {
        let theme = fern_tokens::Theme::light_default();
        let ctx = LayoutContext::for_testing(&theme);
        let r = Expand::new().layout_response(SizeProposal::unspecified(), &ctx);
        assert_eq!(r.flex, 1.0);
    }

    #[test]
    fn flex_zero_opts_out() {
        let theme = fern_tokens::Theme::light_default();
        let ctx = LayoutContext::for_testing(&theme);
        let r = Expand::new()
            .flex(0.0)
            .layout_response(SizeProposal::unspecified(), &ctx);
        assert_eq!(r.flex, 0.0);
    }
}
