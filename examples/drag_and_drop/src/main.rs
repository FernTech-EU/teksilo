// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag-and-drop showcase — milestone 6, §14.
//!
//! Three panels demonstrating both *in-view reorder* and *cross-widget export*
//! (see [docs/drag-and-drop.md §12](../../docs/drag-and-drop.md)):
//! - **Library** `ListView` of songs — `.exportable(Copy)`. Drag rows to
//!   reorder; multi-select (Ctrl/Shift) and drag the whole set out to the
//!   playlist (a *copy*); Alt+↑/↓ reorders the selected row.
//! - **Playlist** `ListView` — `.accept_foreign_rows(true)`: receives songs
//!   dragged from the library. It is itself `.exportable(Move)`, so dragging a
//!   playlist row onto the **Trash** `DropTarget` removes it.
//! - **Folders** `TreeView` — drag a row onto another's top third to drop
//!   *before*, middle third to drop *into* (reparent), bottom third *after*.
//!
//! The drag payload is the public [`RowDragData<T>`](bastyde::widgets::RowDragData);
//! `.export_external` also advertises `text/plain`, so a song can be dropped on
//! another application.
//!
//! Run with: `cargo run -p drag-and-drop`

use bastyde::core::WidgetPlacement;
use bastyde::data::{ListModel, SelectionMode, SelectionModel, TreeModel};
use bastyde::prelude::*;
use bastyde::widgets::{
    Divider, DragTransferMode, DropTarget, DropTargetVariant, Expand, HStack, ListView, Padding,
    Panel, RowDragData, Spacer, StandardListItem, StandardTreeItem, TextWidget, Toolbar, TreeView,
    VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Drag and Drop")
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
    tree.insert_child(projects, 0, "bastyde".to_string());
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
    playlist: ListModel<String>,
    folders: TreeModel<String>,
    song_selection: SelectionModel,
    playlist_selection: SelectionModel,
    folder_selection: SelectionModel,
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new(songs: ListModel<String>, folders: TreeModel<String>) -> Self {
        Self {
            songs,
            playlist: ListModel::from_vec(Vec::new()),
            folders,
            song_selection: SelectionModel::new(SelectionMode::Multi),
            playlist_selection: SelectionModel::new(SelectionMode::Multi),
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
                            TextWidget::new(lit!("Songs"))
                                .style(body_bold)
                                .color(text_primary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "Reorder within, or drag row(s) \u{2192} Playlist (copy). Multi-select with Ctrl/Shift."
                            ))
                            .style(small)
                            .color(text_muted),
                        ),
                ),
            )
            .child(
                Expand::vertical().child(
                    ListView::new(songs, move |index, title, selected| {
                        Box::new(
                            StandardListItem::new(lit!(title.clone()))
                                .selected(selected)
                                .leading_slot(
                                    TextWidget::new(lit!(
                                        format!("{:>2}.", index + 1).leak() as &str
                                    ))
                                    .color(TextRole::Secondary),
                                ),
                        )
                    })
                    .item_height(32.0)
                    .selection(selection)
                    .reorderable(true)
                    // Rows are droppable outside the library — a *copy* goes to
                    // the playlist, and `text/plain` lets a song drop on another
                    // application.
                    .exportable(DragTransferMode::Copy)
                    .export_external(|items| {
                        vec![("text/plain".to_string(), items.join("\n").into_bytes())]
                    }),
                ),
            )
    }

    fn build_playlist_panel(&self, theme: &Theme) -> impl Widget + 'static {
        let playlist = self.playlist.clone();
        let playlist_for_recv = self.playlist.clone();
        let selection = self.playlist_selection.clone();
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
                            TextWidget::new(lit!("Playlist"))
                                .style(body_bold)
                                .color(text_primary),
                        )
                        .child(
                            TextWidget::new(lit!(
                                "Drop songs here from the Library. Drag a row onto Trash to remove it."
                            ))
                            .style(small)
                            .color(text_muted),
                        ),
                ),
            )
            .child(
                Expand::vertical().child(
                    ListView::new(playlist, move |_i, title, selected| {
                        Box::new(StandardListItem::new(lit!(title.clone())).selected(selected))
                    })
                    .item_height(32.0)
                    .selection(selection)
                    .reorderable(true)
                    // Receive songs dragged from the Library …
                    .accept_foreign_rows(true)
                    .on_rows_received(move |items, at, _ctx| {
                        for (offset, song) in items.into_iter().enumerate() {
                            playlist_for_recv.insert(at + offset, song);
                        }
                    })
                    // … and let a playlist row be *moved* out (e.g. to Trash).
                    .exportable(DragTransferMode::Move),
                ),
            )
            .child(
                Padding::uniform(12.0).child(
                    DropTarget::new()
                        .variant(DropTargetVariant::Prominent)
                        .accept_when(|p| {
                            p.get_typed::<RowDragData<String>>()
                                .is_some_and(|d| d.is_export())
                        })
                        .on_drop(|mut p, _pos, _ctx| {
                            // The playlist is `.exportable(Move)`, so accepting
                            // here makes the source view remove the rows.
                            p.take_typed::<RowDragData<String>>().is_some()
                        })
                        .child(
                            Padding::uniform(18.0).child(
                                TextWidget::new(lit!("\u{1F5D1}  Trash — drop to remove"))
                                    .color(TextRole::Secondary),
                            ),
                        ),
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
                            TextWidget::new(lit!("Folders"))
                                .style(body_bold)
                                .color(text_primary),
                        )
                        .child(
                            TextWidget::new(lit!("Drag a row onto another: top third = before, middle = into, bottom = after."),
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
                            StandardTreeItem::new(lit!(name.clone()))
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
    fn build(&mut self, ctx: &mut bastyde::core::BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let root = ctx.add(
            Panel::new().child(
                HStack::new()
                    .spacing(0.0)
                    .child(Expand::horizontal().child(self.build_songs_panel(&theme)))
                    .child(Divider::vertical())
                    .child(Expand::horizontal().child(self.build_playlist_panel(&theme)))
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
        ctx: &bastyde::core::LayoutContext,
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
        _ctx: &bastyde::core::LayoutContext,
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
