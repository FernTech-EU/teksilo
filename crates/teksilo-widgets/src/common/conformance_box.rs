// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The conformance box: the one way a control whose chrome is deliberately
//! smaller than the WCAG 2.2 SC 2.5.8 floor still reaches it.
//!
//! [`InputTokens::min_target_conformance`] is 24 dp at **every** density and
//! never scales, so a recipe that pins a dimension below it is below it at
//! Touch as much as at Compact. The bargain struck here is that the chrome
//! stays whatever the recipe asked for — a preset is entitled to paint under
//! the floor, and macOS does — while the **node** around it grows to the floor
//! with the chrome centred inside: the box, not the chrome, is the target.
//!
//! Neither hit mechanism can substitute for the box. An outset cannot escape
//! its parent, so a control sitting flush against its container's edge has
//! nowhere to grow into; and the miss-only slop pass gives a control nothing
//! when a neighbour of its own kind sits flush beside it, because the
//! neighbour owns the press at distance zero. A row of icon buttons, or the
//! calendar header's arrow pair, is exactly that geometry.
//!
//! **The identity case adds no nodes.** It is also the common one — a recipe
//! that routes its sizes through the density ladder (`dp(.., Target, ..)`)
//! already clears the floor — and it must cost nothing: wrapping
//! unconditionally would add two arena nodes to every such control in every
//! tree, for a `Center` that centres nothing inside a `FixedSize` the size of
//! its child. Only a recipe pinned below the floor gets the box, and only its
//! node moves.
//!
//! The wrappers are rigid (`flex = 0`, `shrink = 0`), so hand this helper a
//! rigid painted node — both shipped callers hand it a `FixedSize`. Wrapping
//! a flexible child would flatten its `flex`/`shrink` on the way to the
//! parent, the same trap [`DeadZone`](crate::primitives::DeadZone)'s
//! layout-transparency doc describes.
//!
//! [`InputTokens::min_target_conformance`]: teksilo_tokens::InputTokens::min_target_conformance

use teksilo_canvas::Size;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget_id::WidgetId;

use crate::primitives::{Center, FixedSize};

/// The node extent the conformance box guarantees over a `painted` chrome:
/// each axis floored independently at `floor`.
///
/// Per-axis on purpose — a non-square chrome can clear the floor on one axis
/// and sit under it on the other, and flooring only one axis would leave the
/// other one a raised-floor theme could still fail. The pure half of
/// [`conformance_box`], split out so a `layout_response` fallback (which holds
/// a `LayoutContext`, not a `BuildContext`) computes the same number from the
/// same arithmetic.
pub(crate) fn conformance_box_size(painted: Size, floor: f32) -> Size {
    Size::new(painted.width.max(floor), painted.height.max(floor))
}

/// Wrap an already-added `painted` node in `FixedSize(box) > Center > painted`
/// when any axis of `painted_size` sits under the theme's
/// `input.min_target_conformance`. The identity case — the chrome already
/// clears the floor on both axes — returns `painted` unchanged and adds no
/// nodes (see the module doc for why that is the design, not a shortcut).
///
/// Returns the node to hand the parent and the box extent actually in force —
/// equal to `painted_size` in the identity case — so a caller that parks the
/// extent for a later theme-less read (`Widget::hit_outset`) stores the same
/// number the tree lays out.
pub(crate) fn conformance_box(
    ctx: &mut BuildContext,
    painted: WidgetId,
    painted_size: Size,
) -> (WidgetId, Size) {
    let boxed = conformance_box_size(painted_size, ctx.theme().input.min_target_conformance);
    if boxed.width <= painted_size.width && boxed.height <= painted_size.height {
        return (painted, painted_size);
    }
    let centred = ctx.add(Center::new().child(painted));
    let id = ctx.add(
        FixedSize::new()
            .width(boxed.width)
            .height(boxed.height)
            .child(centred),
    );
    (id, boxed)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use teksilo_canvas::{Rect, SizeProposal};
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
    use teksilo_core::widget_tree::WidgetTree;

    use super::*;
    use crate::primitives::RectWidget;

    #[test]
    fn the_box_size_floors_each_axis_independently() {
        // Non-square, one axis under: only that axis rises (the macOS switch
        // track's shape).
        assert_eq!(
            conformance_box_size(Size::new(38.0, 22.0), 24.0),
            Size::new(38.0, 24.0),
        );
        // Both under.
        assert_eq!(
            conformance_box_size(Size::new(18.0, 18.0), 24.0),
            Size::new(24.0, 24.0),
        );
        // Both clear: the identity.
        assert_eq!(
            conformance_box_size(Size::new(30.0, 26.0), 24.0),
            Size::new(30.0, 26.0),
        );
    }

    /// Minimal consumer: a `FixedSize` chrome of `painted` dp handed to
    /// [`conformance_box`], publishing the extent that came back.
    #[derive(Debug)]
    struct Subject {
        painted: f32,
        reported: Rc<Cell<(f32, f32)>>,
        root: Option<WidgetId>,
    }

    impl Widget for Subject {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let rect = ctx.add(RectWidget::new());
            let chrome = ctx.add(
                FixedSize::new()
                    .width(self.painted)
                    .height(self.painted)
                    .child(rect),
            );
            let (root, extent) =
                conformance_box(ctx, chrome, Size::new(self.painted, self.painted));
            self.reported.set((extent.width, extent.height));
            self.root = Some(root);
            vec![root]
        }

        fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
            match self.root {
                Some(id) => ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
                None => proposal.resolve(0.0, 0.0),
            }
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

        fn children(&self) -> Vec<WidgetId> {
            self.root.into_iter().collect()
        }
    }

    fn mounted(painted: f32) -> (WidgetTree, WidgetId, Rc<Cell<(f32, f32)>>) {
        // Inside a stack, not bare at the root: a root child is stretched to
        // the window proposal, which would hide the squares under measurement.
        let reported = Rc::new(Cell::new((0.0, 0.0)));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(crate::primitives::HStack::new().child(Subject {
            painted,
            reported: reported.clone(),
            root: None,
        }));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        (tree, id, reported)
    }

    fn fixed_sizes(tree: &WidgetTree, root: WidgetId) -> Vec<(f32, f32, f32, f32)> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if tree
                .widget_type_name(id)
                .is_some_and(|t| t.rsplit("::").next() == Some("FixedSize"))
            {
                let b = tree.bounds(id);
                out.push((b.x, b.y, b.width, b.height));
            }
            for c in tree.children(id).into_iter().rev() {
                stack.push(c);
            }
        }
        out
    }

    /// The identity contract: a chrome already at the floor gains NO wrapper.
    /// Reddens if the early return is deleted (a second `FixedSize` appears)
    /// or if the returned extent stops being the painted size.
    #[test]
    fn a_chrome_at_the_floor_gains_no_wrapper_nodes() {
        let (tree, id, reported) = mounted(24.0);
        let squares = fixed_sizes(&tree, id);
        assert_eq!(
            squares.len(),
            1,
            "the painted square is the only FixedSize — no box was built: {squares:?}",
        );
        assert_eq!(squares[0].2, 24.0);
        assert_eq!(squares[0].3, 24.0);
        assert_eq!(
            reported.get(),
            (24.0, 24.0),
            "the identity arm reports the painted size as the extent in force",
        );
    }

    /// The box contract: a sub-floor chrome gains exactly one `FixedSize` at
    /// the floor with the chrome centred inside. Reddens if the floor read is
    /// neutered (outer square shrinks to the chrome) or the `Center` goes
    /// (the inner square's origin lands at the box's corner, not offset).
    #[test]
    fn a_sub_floor_chrome_is_centred_in_a_box_at_the_floor() {
        let (tree, id, reported) = mounted(18.0);
        let mut squares = fixed_sizes(&tree, id);
        squares.sort_by(|a, b| a.2.total_cmp(&b.2));
        assert_eq!(squares.len(), 2, "chrome + box: {squares:?}");
        let (inner, outer) = (squares[0], squares[1]);
        assert_eq!((outer.2, outer.3), (24.0, 24.0), "the box is the floor");
        assert_eq!((inner.2, inner.3), (18.0, 18.0), "the chrome is untouched");
        assert_eq!(
            (inner.0 - outer.0, inner.1 - outer.1),
            (3.0, 3.0),
            "the chrome sits centred in the box",
        );
        assert_eq!(
            reported.get(),
            (24.0, 24.0),
            "the extent handed back is the box, which is what a caller parks \
             for `hit_outset`",
        );
    }
}
