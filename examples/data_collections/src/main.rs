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

use bastyde::core::widget::WidgetPlacement;
use bastyde::data::{
    CheckedModel, ListModel, SelectionMode, SelectionModel, TreeCheckedModel, TreeModel,
};
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Card, Expand, HStack, ListView, Padding, Panel, Repeater, Spacer,
    StandardListItem, StandardTreeItem, TabId, TabInfo, TabWidget, TextWidget, Toolbar, TreeView,
    VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new_literal("Toggle Dark Mode").on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                bastyde::presets::intui::dark()
            } else {
                bastyde::presets::intui::light()
            });
        }),
    ))
}

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
    list_checks: CheckedModel,
    tree_checks: TreeCheckedModel<String>,
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
        let tree_checks = TreeCheckedModel::new(tree_model.clone());
        Self {
            root_child_id: None,
            tags,
            list_items,
            tree_model,
            list_selection: SelectionModel::new(SelectionMode::Multi),
            tree_selection: SelectionModel::new(SelectionMode::Single),
            list_checks: CheckedModel::new(),
            tree_checks,
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
                                    .variant(ButtonVariant::Filled)
                                    .on_activate_fn(move |_ctx| {
                                        let n = counter.get();
                                        counter.set(n + 1);
                                        tags_add.push(format!("Tag {}", n));
                                    }),
                            )
                            .child({
                                let tags = tags_remove.clone();
                                Button::new_literal("- Remove Last")
                                    .variant(ButtonVariant::Plain)
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
                                                        format!("{}.", i + 1).leak() as &str,
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
        let checks = self.list_checks.clone();

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
                                        .variant(ButtonVariant::Filled)
                                        .on_activate_fn(move |_ctx| {
                                            let n = counter.get();
                                            counter.set(n + 1);
                                            items_add.push(format!("Item {}", n));
                                        }),
                                )
                                .child(
                                    Button::new_literal("- Remove First")
                                        .variant(ButtonVariant::Plain)
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
                    // Half the rows show subtitle + subtitle slots so
                    // the two-line / subtitle-slot path is exercised
                    // visually. The remaining rows are single-line
                    // with a checkbox so the two-state checkbox path
                    // and selection-highlight rendering are both
                    // visible at once.
                    let is_two_line = index % 2 == 0;
                    let mut row = StandardListItem::new_literal(item.clone())
                        .selected(selected)
                        .leading_slot(
                            TextWidget::new_literal(format!("{:>4}", index + 1).leak() as &str)
                                .color(TextRole::Secondary),
                        );
                    if is_two_line {
                        row = row
                            .subtitle_literal(format!("Item #{} · category", index + 1))
                            .subtitle_leading_slot(
                                TextWidget::new_literal("•").color(TextRole::Accent),
                            )
                            .subtitle_trailing_slot(
                                TextWidget::new_literal("just now").color(TextRole::Secondary),
                            );
                    } else {
                        row = row.checkbox(checks.signal_for(index));
                    }
                    Box::new(row)
                })
                .item_height(48.0)
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
        let tree_checks = self.tree_checks.clone();

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
                                        .variant(ButtonVariant::Filled)
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
                                        .variant(ButtonVariant::Plain)
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
                TreeView::new_with_context(tree, move |item, entry, selected, ctx| {
                    let mut row = StandardTreeItem::new_literal(item.clone())
                        .from_entry(entry)
                        .selected(selected)
                        .on_toggle_rc(ctx.toggle_callback());
                    if entry.has_children {
                        // Branches: tristate so `Indeterminate` is
                        // visible when descendants are mixed.
                        row = row.tristate_checkbox(tree_checks.signal_for(entry.node_id));
                    } else {
                        // Leaves: two-state via the bidirectional
                        // bool bridge. The model still recomputes
                        // ancestors on writes through this signal.
                        row = row.checkbox(tree_checks.bool_signal_for(entry.node_id));
                    }
                    Box::new(row)
                })
                .item_height(28.0)
                .selection(selection)
                .reorderable(true)
                .row_click_expands(false),
            )
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme_signal().get();
        let selected_tab: Signal<Option<TabId>> = ctx.signal(None);

        let repeater_tab = self.build_repeater_tab(&theme);
        let listview_tab = self.build_listview_tab(&theme);
        let treeview_tab = self.build_treeview_tab(&theme);

        let root = ctx.add(
            Panel::new().child(
                TabWidget::new(selected_tab)
                    .static_tab(
                        TabInfo::new().title(bastyde::i18n::LocalizedString::literal("Repeater")),
                        repeater_tab,
                    )
                    .static_tab(
                        TabInfo::new().title(bastyde::i18n::LocalizedString::literal("ListView")),
                        listview_tab,
                    )
                    .static_tab(
                        TabInfo::new().title(bastyde::i18n::LocalizedString::literal("TreeView")),
                        treeview_tab,
                    ),
            ),
        );

        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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
        "Bastyde".into(),
        "Desktop".into(),
    ]);

    let list_items = ListModel::from_vec((1..=200).map(|i| format!("Item {}", i)).collect());

    let tree_model = TreeModel::new();
    let docs = tree_model.insert_root(0, "Documents".into());
    let proj = tree_model.insert_child(docs, 0, "Projects".into());
    tree_model.insert_child(proj, 0, "Bastyde".into());
    tree_model.insert_child(proj, 1, "Skribisto".into());
    let notes = tree_model.insert_child(docs, 1, "Notes".into());
    tree_model.insert_child(notes, 0, "Meeting 2026-04-01".into());
    tree_model.insert_child(notes, 1, "Ideas".into());
    let pics = tree_model.insert_root(1, "Pictures".into());
    tree_model.insert_child(pics, 0, "Vacation".into());
    tree_model.insert_child(pics, 1, "Screenshots".into());
    tree_model.insert_root(2, "Downloads".into());

    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Data Collections — Milestone 6")
                .size(960, 680)
                .root(move |tree, _state| {
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(Root::new(
                                tags.clone(),
                                list_items.clone(),
                                tree_model.clone(),
                            ))),
                    )
                }),
        )
        .run();
}
