//! Data Collections example — Milestone 6 showcase.
//!
//! Demonstrates `Repeater`, `ListView` (with virtualization and selection),
//! and `TreeView` (with expand/collapse).
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
// Commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum Cmd {
    ToggleTheme,
    AddTag,
    RemoveTag,
    AddListItem,
    RemoveListItem,
    AddTreeNode,
    RemoveTreeNode,
}

impl AppCommand for Cmd {}

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
        }
    }

    fn build_repeater_tab(&self, theme: &Theme) -> impl Widget + 'static {
        let tags = self.tags.clone();

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
                                    .on_click(Cmd::AddTag),
                            )
                            .child(
                                Button::new("- Remove Last")
                                    .style(ButtonStyle::Outlined)
                                    .on_click(Cmd::RemoveTag),
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
        let selection = self.selection.clone();

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
                                        .on_click(Cmd::AddListItem),
                                )
                                .child(
                                    Button::new("- Remove First")
                                        .style(ButtonStyle::Outlined)
                                        .on_click(Cmd::RemoveListItem),
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
                                        .on_click(Cmd::AddTreeNode),
                                )
                                .child(
                                    Button::new("- Remove Last Root")
                                        .style(ButtonStyle::Outlined)
                                        .on_click(Cmd::RemoveTreeNode),
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
    // Create shared data models in main so the command handler can access them.
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

    // Counters for generating unique names.
    let tag_counter = Rc::new(Cell::new(5_usize));
    let list_counter = Rc::new(Cell::new(201_usize));
    let tree_counter = Rc::new(Cell::new(1_usize));

    // Clones for the command handler.
    let tags_cmd = tags.clone();
    let list_items_cmd = list_items.clone();
    let tree_model_cmd = tree_model.clone();
    let tag_ctr = tag_counter.clone();
    let list_ctr = list_counter.clone();
    let tree_ctr = tree_counter.clone();

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("Data Collections — Milestone 6")
        .window_size(960, 680)
        .on_command(move |cmd: &Cmd, ctx| match cmd {
            Cmd::ToggleTheme => {
                let next = if ctx.theme().colors.surface == Theme::light_default().colors.surface {
                    Theme::dark_default()
                } else {
                    Theme::light_default()
                };
                ctx.set_theme(next);
            }
            Cmd::AddTag => {
                let n = tag_ctr.get();
                tag_ctr.set(n + 1);
                tags_cmd.push(format!("Tag {}", n));
            }
            Cmd::RemoveTag => {
                if !tags_cmd.is_empty() {
                    tags_cmd.remove(tags_cmd.len() - 1);
                }
            }
            Cmd::AddListItem => {
                let n = list_ctr.get();
                list_ctr.set(n + 1);
                list_items_cmd.push(format!("Item {}", n));
            }
            Cmd::RemoveListItem => {
                if !list_items_cmd.is_empty() {
                    list_items_cmd.remove(0);
                }
            }
            Cmd::AddTreeNode => {
                let n = tree_ctr.get();
                tree_ctr.set(n + 1);
                let count = tree_model_cmd.root_count();
                tree_model_cmd.insert_root(count, format!("New Folder {}", n));
            }
            Cmd::RemoveTreeNode => {
                let count = tree_model_cmd.root_count();
                if count > 0 {
                    let last = tree_model_cmd.root(count - 1);
                    tree_model_cmd.remove(last);
                }
            }
        })
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
