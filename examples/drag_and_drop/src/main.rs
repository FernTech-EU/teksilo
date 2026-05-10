//! Drag-and-drop showcase — milestone 6, §14.
//!
//! Two side-by-side panels:
//! - **ListView** of songs. Drag any row to reorder; Alt+ArrowUp / Alt+ArrowDown
//!   reorders the currently-selected row via the keyboard contract.
//! - **TreeView** of a folder hierarchy. Drag a row onto another row's top
//!   third to drop *before*, middle third to drop *into* (reparent), or
//!   bottom third to drop *after*.
//!
//! Both widgets are drag sources and drop targets. The payloads are typed
//! and self-contained — no MIME/cross-app transfer yet (see milestones §6
//! for what's pending).
//!
//! Run with: `cargo run -p drag-and-drop`

use fern_ui::core::WidgetPlacement;
use fern_ui::data::{ListModel, SelectionMode, SelectionModel, TreeModel};
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, Divider, Expand, HStack, ListView, Padding, Panel, Spacer, StandardListItem,
    StandardTreeItem, TextWidget, Toolbar, TreeView, VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new_literal("Toggle Dark Mode").on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                fern_ui::presets::intui::dark()
            } else {
                fern_ui::presets::intui::light()
            });
        }),
    ))
}

fn main() {
    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(fern_ui::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Drag and Drop")
                .size(960, 640)
                .root(|tree, _state| {
                    let songs = ListModel::from_vec(
                        [
                            "Hyperballad",
                            "Unravel",
                            "Black Cow",
                            "Chan Chan",
                            "The Chauffeur",
                            "Teardrop",
                            "Bachelorette",
                            "Lamento Borincano",
                            "Space Oddity",
                            "Paranoid Android",
                        ]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                    );
                    let folders = build_folder_tree();
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(Root::new(songs, folders))),
                    )
                }),
        )
        .run();
}

fn build_folder_tree() -> TreeModel<String> {
    let tree = TreeModel::new();
    let documents = tree.insert_root(0, "Documents".to_string());
    tree.insert_child(documents, 0, "notes.md".to_string());
    tree.insert_child(documents, 1, "taxes.pdf".to_string());
    let projects = tree.insert_child(documents, 2, "Projects".to_string());
    tree.insert_child(projects, 0, "fern-ui".to_string());
    tree.insert_child(projects, 1, "skribisto".to_string());

    let downloads = tree.insert_root(1, "Downloads".to_string());
    tree.insert_child(downloads, 0, "image.png".to_string());
    tree.insert_child(downloads, 1, "report.docx".to_string());

    tree.insert_root(2, "README.txt".to_string());
    tree
}

#[derive(Debug)]
struct Root {
    songs: ListModel<String>,
    folders: TreeModel<String>,
    song_selection: SelectionModel,
    folder_selection: SelectionModel,
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new(songs: ListModel<String>, folders: TreeModel<String>) -> Self {
        Self {
            songs,
            folders,
            song_selection: SelectionModel::new(SelectionMode::Single),
            folder_selection: SelectionModel::new(SelectionMode::Single),
            root_child_id: None,
        }
    }

    fn build_songs_panel(&self, theme: &Theme) -> impl Widget + 'static {
        let songs = self.songs.clone();
        let selection = self.song_selection.clone();
        let body_bold = theme.typography.body_bold.clone();
        let small = theme.typography.small.clone();
        let text_primary = theme.colors.text_primary;
        let text_muted = theme.colors.text_secondary;

        VStack::new()
            .spacing(0.0)
            .child(
                Padding::uniform(16.0).child(
                    VStack::new()
                        .spacing(6.0)
                        .child(
                            TextWidget::new_literal("Songs")
                                .style(body_bold)
                                .color(text_primary),
                        )
                        .child(
                            TextWidget::new_literal(
                                "Drag to reorder. Alt+\u{2191}/\u{2193} reorders the selected row.",
                            )
                            .style(small)
                            .color(text_muted),
                        ),
                ),
            )
            .child(
                Expand::vertical().child(
                    ListView::new(songs, move |index, title, selected| {
                        Box::new(
                            StandardListItem::new_literal(title.clone())
                                .selected(selected)
                                .leading_slot(
                                    TextWidget::new_literal(
                                        format!("{:>2}.", index + 1).leak() as &str,
                                    )
                                    .color(TextRole::Secondary),
                                ),
                        )
                    })
                    .item_height(32.0)
                    .selection(selection)
                    .reorderable(true),
                ),
            )
    }

    fn build_folders_panel(&self, theme: &Theme) -> impl Widget + 'static {
        let folders = self.folders.clone();
        let selection = self.folder_selection.clone();
        let body_bold = theme.typography.body_bold.clone();
        let small = theme.typography.small.clone();
        let text_primary = theme.colors.text_primary;
        let text_muted = theme.colors.text_secondary;

        VStack::new()
            .spacing(0.0)
            .child(
                Padding::uniform(16.0).child(
                    VStack::new()
                        .spacing(6.0)
                        .child(
                            TextWidget::new_literal("Folders")
                                .style(body_bold)
                                .color(text_primary),
                        )
                        .child(
                            TextWidget::new_literal(
                                "Drag a row onto another: top third = before, middle = into, bottom = after.",
                            )
                            .style(small)
                            .color(text_muted),
                        ),
                ),
            )
            .child(
                Expand::vertical().child(
                    TreeView::new_with_context(folders, move |name, entry, selected, ctx| {
                        Box::new(
                            StandardTreeItem::new_literal(name.clone())
                                .from_entry(entry)
                                .selected(selected)
                                .on_toggle_rc(ctx.toggle_callback()),
                        )
                    })
                    .item_height(28.0)
                    .selection(selection)
                    .reorderable(true)
                    .row_click_expands(false),
                ),
            )
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut fern_ui::core::BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let root = ctx.add(
            Panel::new().child(
                HStack::new()
                    .spacing(0.0)
                    .child(Expand::horizontal().child(self.build_songs_panel(&theme)))
                    .child(Divider::vertical())
                    .child(Expand::horizontal().child(self.build_folders_panel(&theme))),
            ),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &fern_ui::core::LayoutContext,
    ) -> LayoutResponse {
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
        _ctx: &fern_ui::core::LayoutContext,
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
