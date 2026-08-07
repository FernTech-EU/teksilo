// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Data Collections example — Milestone 6 showcase.
//!
//! Demonstrates all Milestone 6 features:
//! - **Repeater** — non-virtualized dynamic collection
//! - **ListView** — virtualized list with selection, drag reordering,
//!   and **variable row heights** (exact `item_height_fn` — two-line
//!   rows are taller than single-line ones)
//! - **Auto Feed** — message-feed ListView under `auto_item_height`:
//!   rows self-measure (1–4 content lines each), scroll anchoring
//!   keeps content steady as estimates are corrected
//! - **TreeView** — hierarchical tree with expand/collapse, drag
//!   reparenting, and auto-measured rows (branches carry a subtitle
//!   line; measured heights above a toggle survive expand/collapse)
//!
//! Uses the `on_activate_fn(|ctx| …)` handler for button activation.
//! Handlers can fire typed intents via `ctx.send_intent(AppIntent::X)`
//! for source → root dispatch, or run closures directly for local
//! effects. See `docs/shortcut-intent-action.md`.
//!
//! Run with: `cargo run -p data-collections`

use std::cell::Cell;
use std::rc::Rc;

use teksilo::core::widget::WidgetPlacement;
use teksilo::data::{
    CheckedModel, ListModel, SelectionMode, SelectionModel, TreeCheckedModel, TreeModel,
};
use teksilo::prelude::*;
use teksilo::widgets::{
    Button, ButtonVariant, Card, Expand, HStack, ListView, Padding, Panel, Repeater, Spacer,
    StandardListItem, StandardTreeItem, TabId, TabInfo, TabWidget, TextWidget, Toolbar, TreeView,
    VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(teksilo::widgets::ThemeSwitcher::new()),
    )
}

// ---------------------------------------------------------------------------
// Root widget
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Root {
    root_child_id: Option<WidgetId>,
    tags: ListModel<String>,
    list_items: ListModel<String>,
    feed_items: ListModel<String>,
    tree_model: TreeModel<String>,
    list_selection: SelectionModel,
    feed_selection: SelectionModel,
    tree_selection: SelectionModel,
    list_checks: CheckedModel,
    tree_checks: TreeCheckedModel<String>,
    tag_counter: Rc<Cell<usize>>,
    list_counter: Rc<Cell<usize>>,
    feed_counter: Rc<Cell<usize>>,
    tree_counter: Rc<Cell<usize>>,
}

impl Root {
    fn new(
        tags: ListModel<String>,
        list_items: ListModel<String>,
        tree_model: TreeModel<String>,
    ) -> Self {
        let tree_checks = TreeCheckedModel::new(tree_model.clone());
        // Message-feed content for the auto-measure tab: each entry
        // carries 1–4 lines, so realized rows genuinely differ in
        // measured height.
        let feed_items = ListModel::from_vec(
            (1..=300)
                .map(|i| {
                    let lines = 1 + (i * 7) % 4;
                    (0..lines)
                        .map(|l| {
                            if l == 0 {
                                format!("Message {i}")
                            } else {
                                format!("· detail line {} of message {i}", l + 1)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect(),
        );
        Self {
            root_child_id: None,
            tags,
            list_items,
            feed_items,
            tree_model,
            list_selection: SelectionModel::new(SelectionMode::Multi),
            feed_selection: SelectionModel::new(SelectionMode::Single),
            tree_selection: SelectionModel::new(SelectionMode::Single),
            list_checks: CheckedModel::new(),
            tree_checks,
            tag_counter: Rc::new(Cell::new(5)),
            list_counter: Rc::new(Cell::new(201)),
            feed_counter: Rc::new(Cell::new(301)),
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
                        TextWidget::new(lit!("Dynamic Tags (Repeater)"))
                            .style(theme.typography.body_bold.clone())
                            .color(theme.colors.text_primary),
                    )
                    .child(
                        TextWidget::new(lit!(
                            "Non-virtualized: one widget per item. \
                             Add uses on_activate_fn, Remove uses on_activate."
                        ))
                        .style(theme.typography.body.clone())
                        .color(theme.colors.text_primary),
                    )
                    .child(
                        HStack::new()
                            .spacing(8.0)
                            .child(
                                Button::new(lit!("+ Add Tag"))
                                    .variant(ButtonVariant::Filled)
                                    .on_activate_fn(move |_ctx| {
                                        let n = counter.get();
                                        counter.set(n + 1);
                                        tags_add.push(format!("Tag {}", n));
                                    }),
                            )
                            .child({
                                let tags = tags_remove.clone();
                                Button::new(lit!("- Remove Last"))
                                    .variant(ButtonVariant::Plain)
                                    .on_activate_fn(move |_ctx| {
                                        if !tags.is_empty() {
                                            tags.remove(tags.len() - 1);
                                        }
                                    })
                            }),
                    )
                    .child(
                        Repeater::indexed(tags, move |i, tag| {
                            Box::new(
                                Padding::uniform(2.0).child(
                                    Card::new().content(
                                        Padding::symmetric(8.0, 12.0).child(
                                            HStack::new()
                                                .spacing(8.0)
                                                .child(
                                                    TextWidget::new(lit!(
                                                        format!("{}.", i + 1).leak() as &str
                                                    ))
                                                    .color(Color::from_rgba(0.5, 0.5, 0.5, 1.0)),
                                                )
                                                .child(TextWidget::new(lit!(tag.as_str()))),
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
                            TextWidget::new(lit!("Virtualized List (ListView)"))
                                .style(theme.typography.body_bold.clone())
                                .color(theme.colors.text_primary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "200 items, only visible ones have widgets. \
                                 Variable row heights via item_height_fn: \
                                 two-line rows are 64 px, single-line 40 px. \
                                 Multi-select: click, Ctrl+click, Shift+click. \
                                 Drag to reorder. Alt+Arrow to reorder via keyboard."
                            ))
                            .style(theme.typography.body.clone())
                            .color(theme.colors.text_primary),
                        )
                        .child(
                            HStack::new()
                                .spacing(8.0)
                                .child(
                                    Button::new(lit!("+ Add Item"))
                                        .variant(ButtonVariant::Filled)
                                        .on_activate_fn(move |_ctx| {
                                            let n = counter.get();
                                            counter.set(n + 1);
                                            items_add.push(format!("Item {}", n));
                                        }),
                                )
                                .child(
                                    Button::new(lit!("- Remove First"))
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
                    let mut row = StandardListItem::new(lit!(item.clone()))
                        .selected(selected)
                        .leading_slot(
                            TextWidget::new(lit!(format!("{:>4}", index + 1).leak() as &str))
                                .color(TextRole::Secondary),
                        );
                    if is_two_line {
                        row = row
                            .subtitle(lit!(format!("Item #{} · category", index + 1)))
                            .subtitle_leading_slot(
                                TextWidget::new(lit!("•")).color(TextRole::Accent),
                            )
                            .subtitle_trailing_slot(
                                TextWidget::new(lit!("just now")).color(TextRole::Secondary),
                            );
                    } else {
                        row = row.checkbox(checks.signal_for(index));
                    }
                    Box::new(row)
                })
                // Exact variable heights: the callback mirrors the
                // delegate's structure (even rows are two-line).
                .item_height_fn(|i| if i % 2 == 0 { 64.0 } else { 40.0 })
                .selection(selection)
                .reorderable(true),
            )
    }

    // ---- Tab 3: Auto-measured message feed ----

    fn build_feed_tab(&self, theme: &Theme) -> impl Widget + 'static {
        let items = self.feed_items.clone();
        let items_add = self.feed_items.clone();
        let selection = self.feed_selection.clone();
        let counter = self.feed_counter.clone();

        VStack::new()
            .spacing(0.0)
            .child(
                Padding::uniform(16.0).child(
                    VStack::new()
                        .spacing(12.0)
                        .child(
                            TextWidget::new(lit!("Message Feed (auto_item_height)"))
                                .style(theme.typography.body_bold.clone())
                                .color(theme.colors.text_primary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "300 messages, 1–4 lines each. Rows self-measure \
                                 (height-for-width); unrealized rows assume the \
                                 estimate and scroll anchoring keeps content steady \
                                 as measurements correct it. Appending keeps the \
                                 measured prefix — scroll position doesn't jump."
                            ))
                            .style(theme.typography.body.clone())
                            .color(theme.colors.text_primary),
                        )
                        .child(
                            Button::new(lit!("+ Append Message"))
                                .variant(ButtonVariant::Filled)
                                .on_activate_fn(move |_ctx| {
                                    let n = counter.get();
                                    counter.set(n + 1);
                                    items_add.push(format!("Message {n}\n· appended at runtime"));
                                }),
                        ),
                ),
            )
            .child(
                ListView::new(items, move |index, item, selected| {
                    let mut lines = VStack::new().spacing(2.0);
                    for (l, line) in item.lines().enumerate() {
                        let text = TextWidget::new(lit!(line.to_string()));
                        lines = lines.child(if l == 0 {
                            text
                        } else {
                            text.color(TextRole::Secondary)
                        });
                    }
                    let mut row =
                        StandardListItem::new(lit!(format!("#{}", index + 1))).selected(selected);
                    row = row.trailing_slot(Padding::symmetric(6.0, 0.0).child(lines));
                    Box::new(row)
                })
                // Auto-measure: 48 px is just an estimate — real heights
                // come from measuring each realized row's content.
                .auto_item_height(48.0)
                .spacing(2.0)
                .selection(selection),
            )
    }

    // ---- Tab 4: TreeView ----

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
                            TextWidget::new(lit!("File Tree (TreeView)"))
                                .style(theme.typography.body_bold.clone())
                                .color(theme.colors.text_primary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "Hierarchical tree with expand/collapse and \
                                 auto-measured rows: branches carry a subtitle \
                                 line (taller), leaves are single-line. Expanding \
                                 a node keeps the measured heights above it — no \
                                 scroll jump. Click to select, Right/Left \
                                 expand/collapse, drag to reparent."
                            ))
                            .style(theme.typography.body.clone())
                            .color(theme.colors.text_primary),
                        )
                        .child(
                            HStack::new()
                                .spacing(8.0)
                                .child(
                                    Button::new(lit!("+ Add Root"))
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
                                    Button::new(lit!("- Remove Last Root"))
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
                    let mut row = StandardTreeItem::new(lit!(item.clone()))
                        .from_entry(entry)
                        .selected(selected)
                        .on_toggle_rc(ctx.toggle_callback());
                    if entry.has_children {
                        // Branches: tristate so `Indeterminate` is
                        // visible when descendants are mixed — plus a
                        // subtitle, making branch rows measurably
                        // taller than leaves.
                        row = row
                            .tristate_checkbox(tree_checks.signal_for(entry.node_id))
                            .subtitle(lit!(format!("folder · depth {}", entry.depth)));
                    } else {
                        // Leaves: two-state via the bidirectional
                        // bool bridge. The model still recomputes
                        // ancestors on writes through this signal.
                        row = row.checkbox(tree_checks.bool_signal_for(entry.node_id));
                    }
                    Box::new(row)
                })
                // Auto-measure: two-line branches and one-line leaves
                // get their real heights from measurement.
                .auto_item_height(28.0)
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
        let feed_tab = self.build_feed_tab(&theme);
        let treeview_tab = self.build_treeview_tab(&theme);

        let root = ctx.add(
            Panel::new().child(
                TabWidget::new(selected_tab)
                    .static_tab(TabInfo::new().title(lit!("Repeater")), repeater_tab)
                    .static_tab(TabInfo::new().title(lit!("ListView")), listview_tab)
                    .static_tab(TabInfo::new().title(lit!("Auto Feed")), feed_tab)
                    .static_tab(TabInfo::new().title(lit!("TreeView")), treeview_tab),
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
        "Teksilo".into(),
        "Desktop".into(),
    ]);

    let list_items = ListModel::from_vec((1..=200).map(|i| format!("Item {}", i)).collect());

    let tree_model = TreeModel::new();
    let docs = tree_model.insert_root(0, "Documents".into());
    let proj = tree_model.insert_child(docs, 0, "Projects".into());
    tree_model.insert_child(proj, 0, "Teksilo".into());
    tree_model.insert_child(proj, 1, "Skribisto".into());
    let notes = tree_model.insert_child(docs, 1, "Notes".into());
    tree_model.insert_child(notes, 0, "Meeting 2026-04-01".into());
    tree_model.insert_child(notes, 1, "Ideas".into());
    let pics = tree_model.insert_root(1, "Pictures".into());
    tree_model.insert_child(pics, 0, "Vacation".into());
    tree_model.insert_child(pics, 1, "Screenshots".into());
    tree_model.insert_root(2, "Downloads".into());

    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::light())
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
