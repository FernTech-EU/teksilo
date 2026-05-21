//! Data tab — Repeater, ListView, StandardListItem, StandardTreeItem
//! plus inline notes for TreeView/TableView/TreeTable (the heavyweights
//! live in dedicated examples).

use bastyde::data::ListModel;
use bastyde::prelude::*;
use bastyde::widgets::{
    Divider, FixedSize, ListView, Repeater, StandardListItem, StandardTreeItem, TextWidget, VStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_data_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_data_refs())
}

fn make_repeater_model() -> ListModel<String> {
    ListModel::from_vec(vec![
        tr!(dat_fruit_apple()).resolve_now(),
        tr!(dat_fruit_banana()).resolve_now(),
        tr!(dat_fruit_cherry()).resolve_now(),
        tr!(dat_fruit_date()).resolve_now(),
    ])
}

fn make_list_model() -> ListModel<String> {
    ListModel::from_vec(
        (1..=10)
            .map(|i| format!("{} {}", tr!(dat_list_row()).resolve_now(), i))
            .collect(),
    )
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let repeater = section(
        ctx,
        "Repeater",
        Repeater::new(make_repeater_model(), |_idx, item: &String| {
            Box::new(
                TextWidget::new_literal(format!("• {item}"))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
            )
        })
        .spacing(2.0),
    );
    let list_view = section(
        ctx,
        "ListView",
        FixedSize::new()
            .bind_width(280.0_f32)
            .bind_height(180.0_f32)
            .child(ListView::new(
                make_list_model(),
                |_idx, item: &String, _sel| Box::new(StandardListItem::new_literal(item.clone())),
            )),
    );
    let standard_list_item = section(
        ctx,
        tr!(dat_standard_list_item_standalone()),
        VStack::new()
            .spacing(2.0)
            .child(StandardListItem::new(tr!(data_first_item())))
            .child(StandardListItem::new(tr!(data_second_item())))
            .child(StandardListItem::new(tr!(data_third_item()))),
    );
    let standard_tree_item = section(
        ctx,
        tr!(dat_standard_tree_item_standalone()),
        VStack::new()
            .spacing(2.0)
            .child(StandardTreeItem::new(tr!(dat_tree_root())).depth(0))
            .child(StandardTreeItem::new(tr!(data_child_a())).depth(1))
            .child(StandardTreeItem::new(tr!(data_child_b())).depth(1))
            .child(StandardTreeItem::new(tr!(dat_tree_grandchild())).depth(2)),
    );
    let tree_view_note = section(
        ctx,
        "TreeView",
        TextWidget::new(tr!(dat_tree_note()))
            .style(TextStyleRole::Small)
            .color(TextRole::Secondary),
    );
    let table_view_note = section(
        ctx,
        "TableView",
        TextWidget::new(tr!(dat_table_note()))
            .style(TextStyleRole::Small)
            .color(TextRole::Secondary),
    );
    let tree_table_note = section(
        ctx,
        "TreeTable",
        TextWidget::new(tr!(dat_treetable_note()))
            .style(TextStyleRole::Small)
            .color(TextRole::Secondary),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(repeater)
            .add_child(list_view)
            .add_child(standard_list_item)
            .add_child(standard_tree_item)
            .add_child(tree_view_note)
            .add_child(table_view_note)
            .add_child(tree_table_note),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Repeater + ListView take a closure delegate as a constructor
    // arg. bati! can carry constructor args, but the delegate needs
    // to be quoted as a single expression — pre-register both.
    let repeater_widget = ctx.add(
        Repeater::new(make_repeater_model(), |_idx, item: &String| {
            Box::new(
                TextWidget::new_literal(format!("• {item}"))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
            )
        })
        .spacing(2.0),
    );
    let list_view_widget = ctx.add(ListView::new(
        make_list_model(),
        |_idx, item: &String, _sel| Box::new(StandardListItem::new_literal(item.clone())),
    ));

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_data_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_data_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new_literal("Repeater") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ repeater_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("ListView") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 280.0_f32
                    bind_height: 180.0_f32
                    child_id: list_view_widget
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dat_standard_list_item_standalone())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 2.0
                    StandardListItem::new(tr!(data_first_item()))
                    StandardListItem::new(tr!(data_second_item()))
                    StandardListItem::new(tr!(data_third_item()))
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dat_standard_tree_item_standalone())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 2.0
                    StandardTreeItem::new(tr!(dat_tree_root())) {
                        depth: 0
                    }
                    StandardTreeItem::new(tr!(data_child_a())) {
                        depth: 1
                    }
                    StandardTreeItem::new(tr!(data_child_b())) {
                        depth: 1
                    }
                    StandardTreeItem::new(tr!(dat_tree_grandchild())) {
                        depth: 2
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("TreeView") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                TextWidget::new(tr!(dat_tree_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("TableView") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                TextWidget::new(tr!(dat_table_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new_literal("TreeTable") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                TextWidget::new(tr!(dat_treetable_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
        }
    )
}
