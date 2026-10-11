// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `child_opt` takes anything `child` takes: a widget or a pre-registered
//! `WidgetId`.
//!
//! The seven stack-like containers once asked for `Option<impl Widget>`, so
//! `child_opt(Some(widget_id))` failed to compile while `child(widget_id)` did
//! not. Each case here passes an `Option<WidgetId>` (and an `Option` widget),
//! so the file stops compiling if one of them narrows again, and checks that
//! `None` adds nothing.

use teksilo_canvas::SizeProposal;
use teksilo_core::presets::intui;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_widgets::*;

fn leaf() -> impl Widget + 'static {
    FixedSize::new()
        .width(10.0)
        .height(10.0)
        .child(RectWidget::new())
}

macro_rules! child_opt_case {
    ($name:ident, $ctor:expr) => {
        #[test]
        fn $name() {
            let mut tree = WidgetTree::new().with_theme(intui::light());
            let by_id: Option<WidgetId> = Some(tree.add(leaf()));
            let none_id: Option<WidgetId> = None;
            let root = tree.add(
                $ctor
                    .child_opt(by_id)
                    .child_opt(none_id)
                    .child_opt(Some(leaf())),
            );
            tree.layout(SizeProposal::exact(200.0, 200.0));
            assert_eq!(
                tree.children(root).len(),
                2,
                "the id and the widget land; the `None` adds nothing"
            );
            let first = tree.children(root)[0];
            assert_eq!(Some(first), by_id, "the pre-registered id is adopted");
        }
    };
}

child_opt_case!(vstack_child_opt, VStack::new());
child_opt_case!(hstack_child_opt, HStack::new());
child_opt_case!(zstack_child_opt, ZStack::new());
child_opt_case!(grid_child_opt, Grid::new());
child_opt_case!(column_flow_child_opt, ColumnFlow::new());
child_opt_case!(masonry_child_opt, MasonryLayout::new(2));
child_opt_case!(wrap_child_opt, Wrap::new());
