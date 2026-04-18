//! Data Collections example — Milestone 6 showcase.
//!
//! Demonstrates all Milestone 6 features:
//! - **Repeater** — non-virtualized dynamic collection
//! - **ListView** — virtualized list with selection and drag reordering
//! - **TreeView** — hierarchical tree with expand/collapse and drag reparenting
//!
//! Uses the `on_activate_fn(|ctx| …)` handler for button activation.
//! Handlers can fire typed intents via `ctx.send_intent(AppIntent::X)`
//! for source → root dispatch, or run closures directly for local
//! effects. See `docs/shortcut-intent-action.md`.
//!
//! Run with: `cargo run -p data-collections`

use std::cell::Cell;
use std::rc::Rc;

use fern_ui::core::widget::WidgetPlacement;
use fern_ui::data::{ListModel, SelectionMode, SelectionModel, TreeModel};
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonVariant, Card, HStack, ListView, Padding, Panel, RectWidget, Repeater, Spacer,
    TabWidget, TextWidget, TreeView, VStack, ZStack,
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
    list_selection: SelectionModel,
    tree_selection: SelectionModel,
    tag_counter: Rc<Cell<usize>>,
    list_counter: Rc<Cell<usize>>,
    tree_counter: Rc<Cell<usize>>,
}

impl Root {
    fn new(
        tags: ListModel<String>,
        list_items: ListModel<String>,
        tree_model: TreeModel<String>,
    ) -> Self {
        Self {
            root_child_id: None,
            tags,
            list_items,
            tree_model,
            list_selection: SelectionModel::new(SelectionMode::Multi),
            tree_selection: SelectionModel::new(SelectionMode::Single),
            tag_counter: Rc::new(Cell::new(5)),
            list_counter: Rc::new(Cell::new(201)),
            tree_counter: Rc::new(Cell::new(1)),
        }
    }

    // ---- Tab 1: Repeater ----

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
                        TextWidget::new_literal("Dynamic Tags (Repeater)")
                            .style(theme.typography.body_bold.clone())
                            .color(theme.colors.text_primary),
                    )
                    .child(
                        TextWidget::new_literal(
                            "Non-virtualized: one widget per item. \
                             Add uses on_activate_fn, Remove uses on_activate.",
                        )
                        .style(theme.typography.body.clone())
                        .color(theme.colors.text_primary),
                    )
                    .child(
                        HStack::new()
                            .spacing(8.0)
                            .child(
                                Button::new_literal("+ Add Tag")
                                    .style(ButtonVariant::Default)
                                    .on_activate_fn(move |_ctx| {
                                        let n = counter.get();
                                        counter.set(n + 1);
                                        tags_add.push(format!("Tag {}", n));
                                    }),
                            )
                            .child({
                                let tags = tags_remove.clone();
                                Button::new_literal("- Remove Last")
                                    .style(ButtonVariant::Regular)
                                    .on_activate_fn(move |_ctx| {
                                        if !tags.is_empty() {
                                            tags.remove(tags.len() - 1);
                                        }
                                    })
                            }),
                    )
                    .child(
                        Repeater::new(tags, move |i, tag| {
                            Box::new(
                                Padding::uniform(2.0).child(
                                    Card::new().content(
                                        Padding::symmetric(8.0, 12.0).child(
                                            HStack::new()
                                                .spacing(8.0)
                                                .child(
                                                    TextWidget::new_literal(
                                                        format!("{}.", i + 1).leak() as &str
                                                    )
                                                    .color(Color::from_rgba(0.5, 0.5, 0.5, 1.0)),
                                                )
                                                .child(TextWidget::new_literal(tag.as_str())),
                                        ),
                                    ),
                                ),
                            )
                        })
                        .spacing(2.0),
                    ),
            ),
        )
    }

    // ---- Tab 2: ListView ----

    fn build_listview_tab(&self, theme: &Theme) -> impl Widget + 'static {
        let items = self.list_items.clone();
        let items_add = self.list_items.clone();
        let items_remove = self.list_items.clone();
        let selection = self.list_selection.clone();
        let counter = self.list_counter.clone();
        let on_surface = theme.colors.text_primary;
        let body_style = theme.typography.body.clone();

        VStack::new()
            .spacing(0.0)
            .child(
                Padding::uniform(16.0).child(
                    VStack::new()
                        .spacing(12.0)
                        .child(
                            TextWidget::new_literal("Virtualized List (ListView)")
                                .style(theme.typography.body_bold.clone())
                                .color(theme.colors.text_primary),
                        )
                        .child(
                            TextWidget::new_literal(
                                "200 items, only visible ones have widgets. \
                                 Multi-select: click, Ctrl+click, Shift+click. \
                                 Drag to reorder. Alt+Arrow to reorder via keyboard.",
                            )
                            .style(theme.typography.body.clone())
                            .color(theme.colors.text_primary),
                        )
                        .child(
                            HStack::new()
                                .spacing(8.0)
                                .child(
                                    Button::new_literal("+ Add Item")
                                        .style(ButtonVariant::Default)
                                        .on_activate_fn(move |_ctx| {
                                            let n = counter.get();
                                            counter.set(n + 1);
                                            items_add.push(format!("Item {}", n));
                                        }),
                                )
                                .child(
                                    Button::new_literal("- Remove First")
                                        .style(ButtonVariant::Regular)
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
                ListView::new(items, move |index, item, selected| {
                    // Alternating row background + selection highlight
                    let bg = if selected {
                        Color::from_rgba(0.25, 0.47, 0.85, 0.25)
                    } else if index % 2 == 0 {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.03)
                    } else {
                        Color::TRANSPARENT
                    };

                    Box::new(
                        ZStack::new().child(RectWidget::new().background(bg)).child(
                            Padding::symmetric(6.0, 16.0).child(
                                HStack::new()
                                    .spacing(12.0)
                                    .child(
                                        TextWidget::new_literal(format!("{:>4}", index + 1).leak() as &str)
                                            .color(Color::from_rgba(0.5, 0.5, 0.5, 1.0))
                                            .style(body_style.clone()),
                                    )
                                    .child(
                                        TextWidget::new_literal(item.as_str())
                                            .color(on_surface)
                                            .style(body_style.clone()),
                                    )
                                    .child(Spacer::new()),
                            ),
                        ),
                    )
                })
                .item_height(32.0)
                .selection(selection)
                .reorderable(true),
            )
    }

    // ---- Tab 3: TreeView ----

    fn build_treeview_tab(&self, theme: &Theme) -> impl Widget + 'static {
        let tree = self.tree_model.clone();
        let tree_add = self.tree_model.clone();
        let tree_remove = self.tree_model.clone();
        let selection = self.tree_selection.clone();
        let counter = self.tree_counter.clone();
        let on_surface = theme.colors.text_primary;
        let body_style = theme.typography.body.clone();
        let label_style = theme.typography.small.clone();

        VStack::new()
            .spacing(0.0)
            .child(
                Padding::uniform(16.0).child(
                    VStack::new()
                        .spacing(12.0)
                        .child(
                            TextWidget::new_literal("File Tree (TreeView)")
                                .style(theme.typography.body_bold.clone())
                                .color(theme.colors.text_primary),
                        )
                        .child(
                            TextWidget::new_literal(
                                "Hierarchical tree with expand/collapse. \
                                 Click to select, Right/Left expand/collapse. \
                                 Drag to reparent (top=before, middle=into, bottom=after).",
                            )
                            .style(theme.typography.body.clone())
                            .color(theme.colors.text_primary),
                        )
                        .child(
                            HStack::new()
                                .spacing(8.0)
                                .child(
                                    Button::new_literal("+ Add Root")
                                        .style(ButtonVariant::Default)
                                        .on_activate_fn(move |_ctx| {
                                            let n = counter.get();
                                            counter.set(n + 1);
                                            let count = tree_add.root_count();
                                            tree_add
                                                .insert_root(count, format!("New Folder {}", n));
                                        }),
                                )
                                .child(
                                    Button::new_literal("- Remove Last Root")
                                        .style(ButtonVariant::Regular)
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
                TreeView::new(tree, move |item, entry, selected| {
                    let indent = entry.depth as f32 * 20.0;
                    let arrow = if entry.has_children {
                        if entry.is_expanded { "v " } else { "> " }
                    } else {
                        "  "
                    };
                    let is_folder = entry.has_children;

                    let bg = if selected {
                        Color::from_rgba(0.25, 0.47, 0.85, 0.25)
                    } else {
                        Color::TRANSPARENT
                    };

                    Box::new(
                        ZStack::new()
                            .child(RectWidget::new().background(bg))
                            .child(
                                Padding::new(2.0, 8.0, 2.0, indent + 8.0).child(
                                    HStack::new()
                                        .spacing(4.0)
                                        .child(
                                            TextWidget::new_literal(arrow.to_string().leak() as &str)
                                                .color(Color::from_rgba(0.4, 0.4, 0.4, 1.0))
                                                .style(label_style.clone()),
                                        )
                                        .child(
                                            TextWidget::new_literal(item.as_str()).color(on_surface).style(
                                                if is_folder {
                                                    body_style.clone()
                                                } else {
                                                    label_style.clone()
                                                },
                                            ),
                                        )
                                        .child(Spacer::new()),
                                ),
                            ),
                    )
                })
                .item_height(28.0)
                .selection(selection)
                .reorderable(true),
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
                    .tab_literal("Repeater", repeater_tab)
                    .tab_literal("ListView", listview_tab)
                    .tab_literal("TreeView", treeview_tab),
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
    // --- Data models (shared between Root and command handler) ---

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

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("Data Collections — Milestone 6")
        .window_size(960, 680)
        .root(move |tree| {
            tree.add(Root::new(
                tags.clone(),
                list_items.clone(),
                tree_model.clone(),
            ))
        })
        .run();
}
