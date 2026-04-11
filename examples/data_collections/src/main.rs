//! Data Collections example — Milestone 6 showcase.
//!
//! Demonstrates `Repeater`, `ListView` (with virtualization and selection),
//! and `TreeView` (with expand/collapse).
//!
//! Buttons use `on_activate_fn` to mutate shared models directly via closures,
//! which is the natural pattern for data-driven UIs (architecture Section 9.2.6).
//!
//! Run with: `cargo run -p data-collections`

use std::cell::Cell;
use std::rc::Rc;

use fern_ui::core::widget::WidgetPlacement;
use fern_ui::data::{ListModel, SelectionMode, SelectionModel, TreeModel};
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonStyle, Card, HStack, ListView, Padding, Panel, Repeater, Spacer, TabWidget,
    TextWidget, TreeView, VStack,
};

// ---------------------------------------------------------------------------
// Root widget
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Root {
    root_child_id: Option<WidgetId>,
    tags: ListModel<String>,
    list_items: ListModel<String>,
    tree_model: TreeModel<String>,
    selection: SelectionModel,
    tag_counter: Rc<Cell<usize>>,
    list_counter: Rc<Cell<usize>>,
    tree_counter: Rc<Cell<usize>>,
}

impl Root {
    fn new(
        tags: ListModel<String>,
        list_items: ListModel<String>,
        tree_model: TreeModel<String>,
        selection: SelectionModel,
    ) -> Self {
        Self {
            root_child_id: None,
            tags,
            list_items,
            tree_model,
            selection,
            tag_counter: Rc::new(Cell::new(5)),
            list_counter: Rc::new(Cell::new(201)),
            tree_counter: Rc::new(Cell::new(1)),
        }
    }

    fn build_repeater_tab(&self, theme: &Theme) -> impl Widget + 'static {
        let tags = self.tags.clone();
        let tags_add = self.tags.clone();
        let tags_remove = self.tags.clone();
        let counter = self.tag_counter.clone();

        VStack::new().spacing(16.0).child(
            Padding::uniform(16.0).child(
                VStack::new()
                    .spacing(12.0)
                    .child(
                        TextWidget::new("Dynamic Tags (Repeater)")
                            .style(theme.typography.heading_2.clone())
                            .color(theme.colors.on_surface),
                    )
                    .child(
                        TextWidget::new(
                            "The Repeater creates one widget per item. \
                             Click the buttons to add/remove tags.",
                        )
                        .style(theme.typography.body.clone())
                        .color(theme.colors.on_surface),
                    )
                    .child(
                        HStack::new()
                            .spacing(8.0)
                            .child(
                                Button::new("+ Add Tag")
                                    .style(ButtonStyle::Filled)
                                    .on_activate_fn(move |_ctx| {
                                        let n = counter.get();
                                        counter.set(n + 1);
                                        tags_add.push(format!("Tag {}", n));
                                    }),
                            )
                            .child(
                                Button::new("- Remove Last")
                                    .style(ButtonStyle::Outlined)
                                    .on_activate_fn(move |_ctx| {
                                        if !tags_remove.is_empty() {
                                            tags_remove.remove(tags_remove.len() - 1);
                                        }
                                    }),
                            ),
                    )
                    .child(
                        Repeater::new(tags, move |_i, tag| {
                            Box::new(Padding::uniform(4.0).child(Card::new().content(
                                Padding::uniform(8.0).child(TextWidget::new(tag.as_str())),
                            )))
                        })
                        .spacing(4.0),
                    ),
            ),
        )
    }

    fn build_listview_tab(&self, theme: &Theme) -> impl Widget + 'static {
        let items = self.list_items.clone();
        let items_add = self.list_items.clone();
        let items_remove = self.list_items.clone();
        let selection = self.selection.clone();
        let counter = self.list_counter.clone();

        VStack::new()
            .spacing(0.0)
            .child(
                Padding::uniform(16.0).child(
                    VStack::new()
                        .spacing(12.0)
                        .child(
                            TextWidget::new("Virtualized List (ListView)")
                                .style(theme.typography.heading_2.clone())
                                .color(theme.colors.on_surface),
                        )
                        .child(
                            TextWidget::new(
                                "200 items, but only the visible ones have widgets. \
                                 Click buttons to add/remove items.",
                            )
                            .style(theme.typography.body.clone())
                            .color(theme.colors.on_surface),
                        )
                        .child(
                            HStack::new()
                                .spacing(8.0)
                                .child(
                                    Button::new("+ Add Item")
                                        .style(ButtonStyle::Filled)
                                        .on_activate_fn(move |_ctx| {
                                            let n = counter.get();
                                            counter.set(n + 1);
                                            items_add.push(format!("Item {}", n));
                                        }),
                                )
                                .child(
                                    Button::new("- Remove First")
                                        .style(ButtonStyle::Outlined)
                                        .on_activate_fn(move |_ctx| {
                                            if !items_remove.is_empty() {
                                                items_remove.remove(0);
                                            }
                                        }),
                                ),
                        ),
                ),
            )
            .child(
                ListView::new(items, move |_index, item, _selected| {
                    Box::new(
                        Padding::symmetric(4.0, 16.0).child(
                            HStack::new()
                                .spacing(12.0)
                                .child(TextWidget::new(item.as_str()))
                                .child(Spacer::new()),
                        ),
                    )
                })
                .item_height(32.0)
                .selection(selection),
            )
    }

    fn build_treeview_tab(&self, theme: &Theme) -> impl Widget + 'static {
        let tree = self.tree_model.clone();
        let tree_add = self.tree_model.clone();
        let tree_remove = self.tree_model.clone();
        let counter = self.tree_counter.clone();

        VStack::new()
            .spacing(0.0)
            .child(
                Padding::uniform(16.0).child(
                    VStack::new()
                        .spacing(12.0)
                        .child(
                            TextWidget::new("File Tree (TreeView)")
                                .style(theme.typography.heading_2.clone())
                                .color(theme.colors.on_surface),
                        )
                        .child(
                            TextWidget::new(
                                "Hierarchical data with expand/collapse. \
                                 Click buttons to add/remove root nodes.",
                            )
                            .style(theme.typography.body.clone())
                            .color(theme.colors.on_surface),
                        )
                        .child(
                            HStack::new()
                                .spacing(8.0)
                                .child(
                                    Button::new("+ Add Root Node")
                                        .style(ButtonStyle::Filled)
                                        .on_activate_fn(move |_ctx| {
                                            let n = counter.get();
                                            counter.set(n + 1);
                                            let count = tree_add.root_count();
                                            tree_add
                                                .insert_root(count, format!("New Folder {}", n));
                                        }),
                                )
                                .child(
                                    Button::new("- Remove Last Root")
                                        .style(ButtonStyle::Outlined)
                                        .on_activate_fn(move |_ctx| {
                                            let count = tree_remove.root_count();
                                            if count > 0 {
                                                let last = tree_remove.root(count - 1);
                                                tree_remove.remove(last);
                                            }
                                        }),
                                ),
                        ),
                ),
            )
            .child(
                TreeView::new(tree, move |item, entry, _selected| {
                    let indent = entry.depth as f32 * 24.0;
                    let prefix = if entry.has_children {
                        if entry.is_expanded { "v " } else { "> " }
                    } else {
                        "  "
                    };
                    let label = format!("{}{}", prefix, item);
                    Box::new(
                        Padding::new(0.0, 8.0, 0.0, indent + 8.0)
                            .child(TextWidget::new(label.leak() as &str)),
                    )
                })
                .item_height(28.0),
            )
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let selected_tab = ctx.signal(0_usize);

        let repeater_tab = self.build_repeater_tab(&theme);
        let listview_tab = self.build_listview_tab(&theme);
        let treeview_tab = self.build_treeview_tab(&theme);

        let root = ctx.add(
            Panel::new().child(
                TabWidget::new(selected_tab)
                    .tab("Repeater", repeater_tab)
                    .tab("ListView", listview_tab)
                    .tab("TreeView", treeview_tab),
            ),
        );

        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let tags = ListModel::from_vec(vec![
        "Rust".into(),
        "GUI".into(),
        "FernUI".into(),
        "Desktop".into(),
    ]);
    let list_items = ListModel::from_vec((1..=200).map(|i| format!("Item {}", i)).collect());

    let tree_model = TreeModel::new();
    let docs = tree_model.insert_root(0, "Documents".into());
    let proj = tree_model.insert_child(docs, 0, "Projects".into());
    tree_model.insert_child(proj, 0, "FernUI".into());
    tree_model.insert_child(proj, 1, "Skribisto".into());
    let notes = tree_model.insert_child(docs, 1, "Notes".into());
    tree_model.insert_child(notes, 0, "Meeting 2026-04-01".into());
    tree_model.insert_child(notes, 1, "Ideas".into());
    let pics = tree_model.insert_root(1, "Pictures".into());
    tree_model.insert_child(pics, 0, "Vacation".into());
    tree_model.insert_child(pics, 1, "Screenshots".into());
    tree_model.insert_root(2, "Downloads".into());

    let selection = SelectionModel::new(SelectionMode::Multi);

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("Data Collections — Milestone 6")
        .window_size(960, 680)
        .root(move |tree| {
            tree.add(Root::new(
                tags.clone(),
                list_items.clone(),
                tree_model.clone(),
                selection.clone(),
            ))
        })
        .run();
}
