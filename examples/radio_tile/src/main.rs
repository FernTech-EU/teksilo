// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! RadioTile / RadioTileGroup showcase.
//!
//! Run with: `cargo run -p radio-tile`
//!
//! Three arrangements of the selectable-card radio:
//!   1. `TileLayout::Row`      — equal-size horizontal cards (icon + title +
//!      corner radio + wrapping description).
//!   2. `TileLayout::Vertical` — a compact settings list (leading radio + icon
//!      + title + right-aligned meta that tints on selection).
//!   3. `TileLayout::Grid`     — an adaptive wrapping grid, with one disabled
//!      tile.
//!
//! Keyboard: Tab focuses a group; Arrow keys move selection (2-D in the grid),
//! Home / End jump. The whole group is a single WAI-ARIA roving radiogroup.

use teksilo::prelude::*;
use teksilo::widgets::{
    IconWidget, Padding, RadioTile, RadioTileGroup, TextWidget, TileLayout, VStack,
};

// Minimal tintable glyphs (from_svg extracts geometry; the fill is our color).
const FILE_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M6 2h8l4 4v16H6z'/></svg>";
const FOLDER_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M3 6h6l2 2h10v11H3z'/></svg>";
const BAN_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M12 3a9 9 0 100 18 9 9 0 000-18zm0 2a7 7 0 015.7 11L7 6.3A7 7 0 0112 5z'/></svg>";
const BOOK_SVG: &str =
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M4 4h13v16H4z'/></svg>";
const LAYERS_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M12 3l9 5-9 5-9-5z'/></svg>";
const NOTE_SVG: &str =
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path d='M5 3h14v18H5z'/></svg>";

/// A leading icon whose tint follows selection (accent when this tile is the
/// selected one, muted otherwise) — the reference "cyan when picked" cue.
fn tile_icon(svg: &'static str, index: usize, selected: &Signal<usize>) -> IconWidget {
    let color = selected.map(move |s| {
        if *s == index {
            TextRole::Accent
        } else {
            TextRole::Secondary
        }
    });
    IconWidget::from_svg(svg).icon_size(22.0).color(color)
}

fn section(title: &'static str, content: impl Widget + 'static) -> impl Widget {
    VStack::new()
        .spacing(10.0)
        .child(
            TextWidget::new(lit!(title))
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary),
        )
        .child(content)
}

/// Screenshot 1 — equal-size horizontal cards.
fn format_cards(selected: Signal<usize>) -> impl Widget {
    RadioTileGroup::new(selected.clone())
        .layout(TileLayout::Row)
        .label(lit!("Project format"))
        .tile(
            RadioTile::new()
                .icon(tile_icon(FILE_SVG, 0, &selected))
                .title(lit!("Single file"))
                .description(lit!("One .skrib archive (zip). Portable, easy to back up.")),
        )
        .tile(
            RadioTile::new()
                .icon(tile_icon(FOLDER_SVG, 1, &selected))
                .title(lit!("Bundle"))
                .description(lit!(
                    "A folder holding every text & asset. Friendlier to version control."
                )),
        )
}

/// Screenshot 2 — a compact vertical settings list with trailing meta.
fn project_list(selected: Signal<usize>) -> impl Widget {
    RadioTileGroup::new(selected.clone())
        .layout(TileLayout::Vertical)
        .label(lit!("Template"))
        .tile(
            RadioTile::new()
                .icon(tile_icon(BAN_SVG, 0, &selected))
                .title(lit!("None"))
                .trailing(lit!("empty binder")),
        )
        .tile(
            RadioTile::new()
                .icon(tile_icon(BOOK_SVG, 1, &selected))
                .title(lit!("Empty Novel"))
                .trailing(lit!("binders, no chapters")),
        )
        .tile(
            RadioTile::new()
                .icon(tile_icon(LAYERS_SVG, 2, &selected))
                .title(lit!("Light Novel"))
                .trailing(lit!("15 chapters")),
        )
        .tile(
            RadioTile::new()
                .icon(tile_icon(LAYERS_SVG, 3, &selected))
                .title(lit!("Novel"))
                .trailing(lit!("20 chapters")),
        )
        .tile(
            RadioTile::new()
                .icon(tile_icon(NOTE_SVG, 4, &selected))
                .title(lit!("Notebook"))
                .trailing(lit!("free-form notes")),
        )
}

/// An adaptive grid, with one disabled tile.
fn stage_grid(selected: Signal<usize>) -> impl Widget {
    RadioTileGroup::new(selected)
        .layout(TileLayout::Grid {
            min_tile_width: 200.0,
        })
        .label(lit!("Publication stage"))
        .tile(
            RadioTile::new()
                .title(lit!("Draft"))
                .description(lit!("Work in progress.")),
        )
        .tile(
            RadioTile::new()
                .title(lit!("Review"))
                .description(lit!("Ready for feedback.")),
        )
        .tile(
            RadioTile::new()
                .title(lit!("Published"))
                .description(lit!("Live for readers.")),
        )
        .tile(
            RadioTile::new()
                .title(lit!("Archived"))
                .description(lit!("Read-only — disabled."))
                .enabled(false),
        )
}

fn main() {
    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .theme(teksilo::presets::intui::dark())
        .install_inspector_in_debug()
        .initial_window(
            WindowConfig::new()
                .title("Teksilo — RadioTile")
                .size(700, 940)
                .root(|tree, _state| {
                    let format = Signal::new(0_usize);
                    let template = Signal::new(3_usize); // "Novel", like the screenshot
                    let stage = Signal::new(0_usize);
                    tree.add(
                        Padding::symmetric(24.0, 24.0).child(
                            VStack::new()
                                .spacing(26.0)
                                .child(section("Project format", format_cards(format)))
                                .child(section("Template", project_list(template)))
                                .child(section("Publication stage", stage_grid(stage))),
                        ),
                    )
                }),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo::automation::{
        AutomationOp, AutomationReply, RecordingWindowOps, SettleSpec, execute,
    };
    use teksilo::core::WidgetTree;
    use teksilo::core::accessibility::widget_id_to_node_id;

    /// Drive the same engine the `teksilo-automation` MCP `inspect_node` tool
    /// uses (`execute` + `AutomationOp::InspectNode`) to confirm every Vertical
    /// tile is laid out at the theme's fixed compact-row height (44 dp).
    #[test]
    fn automation_measures_vertical_tiles_at_theme_height() {
        let selected = Signal::new(3_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo::presets::intui::dark());
        tree.add(project_list(selected));
        tree.layout(SizeProposal::exact(560.0, 600.0));

        let mut ops = RecordingWindowOps::new();
        for label in ["None", "Empty Novel", "Light Novel", "Novel", "Notebook"] {
            let id = tree
                .find_by_label(label)
                .unwrap_or_else(|| panic!("tile {label:?} not found"));
            let node = widget_id_to_node_id(id).0;
            let reply = execute(
                &mut tree,
                &mut ops,
                &AutomationOp::InspectNode { node },
                &SettleSpec::default(),
            );
            let AutomationReply::Ok { data } = reply else {
                panic!("inspect_node failed for {label:?}: {reply:?}");
            };
            let height = data["bounds"]["height"]
                .as_f64()
                .expect("layout node bounds.height");
            assert!(
                (height - 44.0).abs() < 0.5,
                "vertical tile {label:?} measured {height} dp via automation (expected 44)"
            );
        }
    }

    /// Walk the `layout_tree` the MCP exposes and assert no widget spills past
    /// its parent — i.e. the fixed-height compact Vertical rows do not
    /// over-constrain their content (the content is centered at its natural
    /// height, not squeezed into a padded box shorter than it).
    #[test]
    fn automation_reports_no_overflow_in_vertical_mode() {
        use std::collections::HashMap;

        let selected = Signal::new(3_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo::presets::intui::dark());
        tree.add(project_list(selected));
        tree.layout(SizeProposal::exact(560.0, 600.0));

        let mut ops = RecordingWindowOps::new();
        let reply = execute(
            &mut tree,
            &mut ops,
            &AutomationOp::LayoutTree {
                max_depth: None,
                include_debug: false,
            },
            &SettleSpec::default(),
        );
        let AutomationReply::Ok { data } = reply else {
            panic!("layout_tree failed: {reply:?}");
        };
        let nodes = data["nodes"].as_array().expect("layout tree nodes");

        let mut bounds: HashMap<u64, (f64, f64, f64, f64)> = HashMap::new();
        for n in nodes {
            let id = n["id"].as_u64().expect("node id");
            let b = &n["bounds"];
            bounds.insert(
                id,
                (
                    b["x"].as_f64().unwrap(),
                    b["y"].as_f64().unwrap(),
                    b["width"].as_f64().unwrap(),
                    b["height"].as_f64().unwrap(),
                ),
            );
        }

        // Every widget must sit within its parent (an over-constraint pushes a
        // child past its parent's box and trips this).
        const TOL: f64 = 0.5;
        for n in nodes {
            let Some(parent) = n["parent"].as_u64() else {
                continue;
            };
            let id = n["id"].as_u64().unwrap();
            let (cx, cy, cw, ch) = bounds[&id];
            let (px, py, pw, ph) = bounds[&parent];
            assert!(
                cx >= px - TOL
                    && cy >= py - TOL
                    && cx + cw <= px + pw + TOL
                    && cy + ch <= py + ph + TOL,
                "overflow in vertical mode: node {id} ({cx},{cy} {cw}x{ch}) spills parent {parent} ({px},{py} {pw}x{ph})"
            );
        }
    }
}
