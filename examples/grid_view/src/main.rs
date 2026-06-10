//! `grid_view` — virtualized 2D tile grid showcase.
//!
//! Run with: `cargo run -p grid-view`
//!
//! Demonstrates:
//! - An adaptive `GridView<Photo>` (tiles ≥ 140 px wide, reflowing on resize).
//! - `Multi` selection: click, Ctrl+click, Shift+click, and rubber-band
//!   marquee (drag on the background). Ctrl+A selects all.
//! - 2D keyboard navigation: arrows / Home / End / Ctrl+Home/End / PageUp /
//!   PageDown, type-ahead (start typing a caption), Enter to "open".
//! - Drag-to-reorder (and Alt+Arrow) with a live insertion bar.
//! - Sections grouped by album, with sticky pinned headers.
//! - A live selection-count status line.

use bastyde::canvas::EdgeInsets;
use bastyde::data::{ListModel, SelectionMode, SelectionModel};
use bastyde::prelude::*;
use bastyde::widgets::{
    Center, Expand, GridSizing, GridView, Padding, Panel, RectWidget, TextWidget, VStack, ZStack,
    grouping_sections,
};

#[derive(Clone, Debug)]
struct Photo {
    caption: String,
    album: &'static str,
}

fn make_photos() -> Vec<Photo> {
    let albums = ["Travel", "Family", "Work", "Nature"];
    let words = [
        "Sunset", "Harbor", "Trail", "Picnic", "Summit", "Garden", "Market", "Bridge", "Cabin",
        "Meadow", "Canyon", "Festival", "Skyline", "Lantern", "Orchard", "Pier",
    ];
    let mut photos = Vec::new();
    for (a, album) in albums.iter().enumerate() {
        for i in 0..15 {
            photos.push(Photo {
                caption: format!("{} {}", words[(a * 4 + i) % words.len()], i + 1),
                album,
            });
        }
    }
    photos
}

fn main() {
    let photos = make_photos();
    let captions: Vec<String> = photos.iter().map(|p| p.caption.clone()).collect();
    let model = ListModel::from_vec(photos);
    let selection = SelectionModel::new(SelectionMode::Multi);
    let status = Signal::new(String::from("0 selected"));

    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("GridView — Photo Library")
                .size(900, 680)
                .root(move |tree, _| {
                    let sections = grouping_sections(&model, |p: &Photo| p.album);
                    let cap_for_type = captions.clone();
                    let status_sig = status.clone();

                    let grid = GridView::new(model.clone(), move |tc| {
                        let bg = if tc.is_selected {
                            SurfaceRole::AccentSubtle
                        } else {
                            SurfaceRole::Raised
                        };
                        Box::new(
                            ZStack::new().child(RectWidget::new().background(bg)).child(
                                Center::new().child(
                                    TextWidget::new(lit!(tc.item.caption.clone()))
                                        .color(TextRole::Primary),
                                ),
                            ),
                        ) as Box<dyn Widget>
                    })
                    .sizing(GridSizing::Adaptive {
                        min_width: 140.0,
                        max_width: Some(220.0),
                        height: 110.0,
                    })
                    .spacing(10.0)
                    .content_inset(EdgeInsets::uniform(12.0))
                    .selection(selection.clone())
                    .marquee_selection(true)
                    .reorderable(true)
                    .type_ahead_label(move |i| cap_for_type.get(i).cloned().unwrap_or_default())
                    .on_tile_activate(|idx, _ctx| println!("activate tile {idx}"))
                    .sections(sections)
                    .pinned_section_headers(true)
                    .a11y_label("Photo library")
                    .on_selection_changed(move |set| {
                        status_sig.set(format!("{} selected", set.len()));
                    });

                    let grid_id = tree.add(grid);
                    let status_line = TextWidget::new(lit!("0 selected")).bind_text(status.clone());

                    tree.add(
                        VStack::new()
                            .child(Expand::new().child(Panel::new().child_id(grid_id)))
                            .child(
                                Panel::new()
                                    .child(Padding::symmetric(8.0_f32, 6.0_f32).child(status_line)),
                            ),
                    )
                }),
        )
        .run();
}
