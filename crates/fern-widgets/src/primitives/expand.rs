use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::Alignment;

/// Layout modifier that claims all available space on one or both axes.
/// The child is positioned within the expanded bounds according to
/// `content_alignment`.
#[derive(Debug)]
pub struct Expand {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    horizontal: bool,
    vertical: bool,
    content_alignment: Alignment,
    /// When true, reports `is_spacer` to the parent stack so this widget
    /// receives remaining space, and fills the child to its full bounds
    /// instead of centering it.
    fills_stack: bool,
}

impl Expand {
    /// Expand on both axes.
    pub fn new() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            horizontal: true,
            vertical: true,
            content_alignment: Alignment::CENTER,
            fills_stack: false,
        }
    }

    /// Expand on the horizontal axis only.
    pub fn horizontal() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            horizontal: true,
            vertical: false,
            content_alignment: Alignment::CENTER,
            fills_stack: false,
        }
    }

    /// Expand on the vertical axis only.
    pub fn vertical() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            horizontal: false,
            vertical: true,
            content_alignment: Alignment::CENTER,
            fills_stack: false,
        }
    }

    /// Make this Expand absorb remaining space in a VStack/HStack
    /// (acts like a Spacer) and fill the child to its full bounds.
    pub fn fills_stack(mut self) -> Self {
        self.fills_stack = true;
        self
    }

    pub fn content_alignment(mut self, alignment: Alignment) -> Self {
        self.content_alignment = alignment;
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

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        // On expanded axes, claim all offered space. On non-expanded axes, use child's size.
        let child_size = self
            .child_id
            .and_then(|id| ctx.child_size(id, SizeProposal::unspecified()))
            .unwrap_or(Size::ZERO);

        let w = if self.horizontal {
            proposal.width.unwrap_or(child_size.width)
        } else {
            child_size.width
        };
        let h = if self.vertical {
            proposal.height.unwrap_or(child_size.height)
        } else {
            child_size.height
        };
        Size::new(w, h)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            if self.fills_stack {
                // Fill mode: child takes the full Expand bounds
                child.origin = bounds.origin();
                child.size = bounds.size();
            } else {
                let child_size = ctx
                    .child_size(child.id, SizeProposal::unspecified())
                    .unwrap_or(bounds.size());
                let rtl = ctx.is_rtl();
                let (dx, dy) = self.content_alignment.resolve(
                    (child_size.width, child_size.height),
                    (bounds.width, bounds.height),
                    rtl,
                );
                child.origin = Point::new(bounds.x + dx, bounds.y + dy);
                child.size = child_size;
            }
        }
    }

    fn is_spacer(&self) -> bool {
        self.fills_stack
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
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn expand_both_axes() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let expand = tree.add(Expand::new().child_id(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Expand claims 200x100, child centered within
        let eb = tree.bounds(expand);
        assert!((eb.width - 200.0).abs() < 0.01);
        assert!((eb.height - 100.0).abs() < 0.01);

        let cb = tree.bounds(child);
        assert!((cb.x - 80.0).abs() < 0.01); // (200-40)/2
        assert!((cb.y - 40.0).abs() < 0.01); // (100-20)/2
    }

    #[test]
    fn expand_horizontal_only() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let expand = tree.add(Expand::horizontal().child_id(child));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        let eb = tree.bounds(expand);
        assert!((eb.width - 200.0).abs() < 0.01);
        assert!((eb.height - 20.0).abs() < 0.01); // child's natural height
    }

    #[test]
    fn content_alignment_top_trailing() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let _expand = tree.add(
            Expand::new()
                .content_alignment(Alignment::TOP_TRAILING)
                .child_id(child),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(child);
        assert!((cb.x - 160.0).abs() < 0.01); // 200-40
        assert!((cb.y - 0.0).abs() < 0.01); // top
    }
}
