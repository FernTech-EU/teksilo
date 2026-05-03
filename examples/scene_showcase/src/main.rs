//! `scene-showcase` — comprehensive demo of every `fern-scene` capability.
//!
//! Six labelled sections inside one big pannable / zoomable scene, plus a
//! reactive header showing live pan / zoom / selection state. Pan with two-
//! finger trackpad or mouse wheel; zoom with `Ctrl + wheel` or pinch.
//! Click a card to select it, `Ctrl-click` to toggle, drag in empty space
//! to marquee-box-select. Drag any lightweight rectangle to move it.
//!
//! This example exercises:
//!
//! - **Heavyweight tier:** `Panel` cards placed at `scene_rect`s via
//!   `Scene::add_widget` — full focus / keyboard / a11y.
//! - **Lightweight tier:** `RectItem`, `PathItem`, `TextItem`, `GroupItem`
//!   painted from the SceneView without arena overhead.
//! - **`GroupItem` visual chrome:** fill + rounded stroke + inline label.
//! - **Z-order:** explicit `Scene::set_z` ordering for overlapping items.
//! - **Selection model:** `SceneSelectionMode::Multi`, click-to-select,
//!   marquee box-select, live selection signal.
//! - **Drag-to-move:** dragging any lightweight item translates it; the
//!   spatial index re-buckets at drag end.
//! - **Logical a11y groups:** `Scene::add_a11y_group` + `set_a11y_parent`
//!   place cards under named "Acts" so screen readers announce the
//!   logical structure independent of visual placement.
//! - **Reactive view-transform signals:** `pan_x_signal` / `pan_y_signal`
//!   / `zoom_signal` drive the live readout in the header.
//! - **Animation system:** pan / zoom / inertial fling are animated
//!   `Signal<f32>`s; the framework's idle scheduler handles them.
//!
//! Run with: `cargo run -p scene-showcase`

use fern_scene::{
    A11yGroup, A11yNode, GroupItem, ItemId, PathItem, RectItem, Scene,
    SceneSelectionMode, SceneView, TextItem,
};
use fern_ui::canvas::{Path, Point, Rect};
use fern_ui::prelude::*;
use fern_ui::widgets::{HStack, Panel, TextWidget, VStack};

// ---------------------------------------------------------------------------
// Scene layout constants
// ---------------------------------------------------------------------------
//
// The scene is divided into a 3 × 2 grid of sections, each ~500 px × 380 px,
// with generous gutters so empty-space marquee dragging works without
// constantly hitting an item.

const SECTION_W: f32 = 500.0;
const SECTION_H: f32 = 380.0;
const GUTTER: f32 = 80.0;
const HEADER_H: f32 = 110.0;
const SCENE_PAD: f32 = 40.0;

fn section_origin(col: usize, row: usize) -> Point {
    Point::new(
        SCENE_PAD + col as f32 * (SECTION_W + GUTTER),
        SCENE_PAD + HEADER_H + row as f32 * (SECTION_H + GUTTER),
    )
}

fn section_rect(col: usize, row: usize) -> Rect {
    let o = section_origin(col, row);
    Rect::new(o.x, o.y, SECTION_W, SECTION_H)
}

fn scene_extent() -> (f32, f32) {
    let cols = 3;
    let rows = 2;
    let w = SCENE_PAD * 2.0 + cols as f32 * SECTION_W + (cols - 1) as f32 * GUTTER;
    let h = SCENE_PAD * 2.0 + HEADER_H + rows as f32 * SECTION_H + (rows - 1) as f32 * GUTTER;
    (w, h)
}

// ---------------------------------------------------------------------------
// Theme palette — soft pastels so overlapping z-order is visually obvious.
// ---------------------------------------------------------------------------

fn pastel_red() -> Color {
    Color::new(0.95, 0.55, 0.55, 0.85)
}
fn pastel_blue() -> Color {
    Color::new(0.55, 0.70, 0.95, 0.85)
}
fn pastel_green() -> Color {
    Color::new(0.55, 0.85, 0.65, 0.85)
}
fn pastel_yellow() -> Color {
    Color::new(0.95, 0.85, 0.55, 0.85)
}
fn pastel_purple() -> Color {
    Color::new(0.80, 0.60, 0.90, 0.85)
}
fn ink() -> Color {
    Color::new(0.10, 0.10, 0.12, 1.0)
}
fn dim_ink() -> Color {
    Color::new(0.30, 0.30, 0.35, 1.0)
}
fn faint_grid() -> Color {
    Color::new(0.85, 0.85, 0.88, 0.5)
}

// ---------------------------------------------------------------------------
// Header inside the scene — big title + four lines of explanation.
// ---------------------------------------------------------------------------

fn add_scene_header(scene: &mut Scene) {
    let (w, _h) = scene_extent();
    let usable_w = w - 2.0 * SCENE_PAD;

    scene.add_item(
        TextItem::new(
            "fern-scene showcase — pan, zoom, click, drag, marquee.",
            Rect::new(SCENE_PAD, SCENE_PAD, usable_w, 36.0),
        )
        .color(ink()),
    );
    scene.add_item(
        TextItem::new(
            "Two-finger trackpad / scroll wheel: pan.   Ctrl+wheel / pinch: zoom.   \
             Each section below demonstrates one capability of the crate.",
            Rect::new(SCENE_PAD, SCENE_PAD + 40.0, usable_w, 28.0),
        )
        .color(dim_ink()),
    );
    scene.add_item(
        TextItem::new(
            "Click a heavyweight card to select it. Ctrl-click to toggle. \
             Drag in empty space to marquee. Drag any rectangle to move it.",
            Rect::new(SCENE_PAD, SCENE_PAD + 70.0, usable_w, 28.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Per-section helpers
// ---------------------------------------------------------------------------

/// Outline a section with a `GroupItem` (fill + stroke + inline label).
/// Returns the GroupItem's id so callers can parent items / a11y groups
/// under the same logical anchor if they choose.
fn add_section_frame(scene: &mut Scene, col: usize, row: usize, title: &str) -> ItemId {
    let r = section_rect(col, row);
    scene.add_item(
        GroupItem::new(r)
            .label(title)
            .show_label(true)
            .label_inset(14.0, 8.0)
            .label_color(ink())
            .fill(Color::new(0.99, 0.99, 1.00, 1.0))
            .stroke(Color::new(0.55, 0.55, 0.65, 1.0), 1.5)
            .corner_radius(12.0),
    )
}

/// Drop a paragraph of explanation text inside a section, just below
/// the title.
fn add_section_caption(scene: &mut Scene, col: usize, row: usize, body: &str) {
    let r = section_rect(col, row);
    scene.add_item(
        TextItem::new(
            body,
            Rect::new(r.x + 14.0, r.y + 36.0, r.width - 28.0, 60.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Section 1 — Lightweight items: RectItem grid + decorative PathItem
// ---------------------------------------------------------------------------

fn build_lightweight_items_section(scene: &mut Scene) {
    add_section_frame(scene, 0, 0, "1. Lightweight items");
    add_section_caption(
        scene,
        0,
        0,
        "RectItem, PathItem, TextItem, GroupItem — painted from \
         SceneView::paint without arena overhead. Thousands per scene \
         render cheaply because the spatial-index culls off-screen \
         items before they enter the paint walk.",
    );

    let r = section_rect(0, 0);
    let area_y = r.y + 110.0;

    // 4×3 grid of small colored RectItems.
    let cols = 4;
    let rows = 3;
    let cell_w = 32.0;
    let cell_h = 32.0;
    let cell_gap = 10.0;
    let palette = [
        pastel_red(),
        pastel_blue(),
        pastel_green(),
        pastel_yellow(),
        pastel_purple(),
    ];
    let grid_x = r.x + 24.0;
    for row_idx in 0..rows {
        for col_idx in 0..cols {
            let i = row_idx * cols + col_idx;
            let color = palette[i % palette.len()];
            let rect = Rect::new(
                grid_x + col_idx as f32 * (cell_w + cell_gap),
                area_y + row_idx as f32 * (cell_h + cell_gap),
                cell_w,
                cell_h,
            );
            scene.add_item(
                RectItem::new(rect)
                    .fill(color)
                    .stroke(ink(), 1.0)
                    .access_label(format!("tile {}", i + 1)),
            );
        }
    }

    // Decorative zigzag PathItem on the right half.
    let path_x = r.x + 250.0;
    let path_y = area_y + 12.0;
    let mut zigzag = Path::new();
    zigzag.move_to(Point::new(path_x, path_y));
    let segments = 6;
    for s in 0..segments {
        let dx = (s as f32 + 1.0) * 26.0;
        let dy = if s % 2 == 0 { 60.0 } else { 0.0 };
        zigzag.line_to(Point::new(path_x + dx, path_y + dy));
    }
    let path_bounds = Rect::new(path_x - 2.0, path_y - 2.0, 26.0 * 7.0, 64.0);
    scene.add_item(
        PathItem::new(zigzag, path_bounds)
            .stroke(pastel_purple(), 3.0)
            .access_label("decorative zigzag"),
    );

    // TextItem caption near the path.
    scene.add_item(
        TextItem::new(
            "Stroke-only paths get per-segment hit-test (line→line)\n\
             with stroke-width tolerance. Filled paths use AABB.",
            Rect::new(path_x, path_y + 80.0, 240.0, 50.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Section 2 — Visual GroupItem with nested content
// ---------------------------------------------------------------------------

fn build_groupitem_section(scene: &mut Scene) {
    add_section_frame(scene, 1, 0, "2. GroupItem with visual chrome");
    add_section_caption(
        scene,
        1,
        0,
        "GroupItem can paint its own fill / stroke / inline label, \
         giving you a labeled box around a set of items. With no chrome \
         it's logical-only — invisible but still addressable from the \
         a11y tree.",
    );

    let r = section_rect(1, 0);

    // A visible group with chrome enclosing 4 small RectItems.
    let inner = Rect::new(r.x + 28.0, r.y + 130.0, 200.0, 200.0);
    scene.add_item(
        GroupItem::new(inner)
            .label("Visible group")
            .show_label(true)
            .label_inset(8.0, 4.0)
            .label_color(ink())
            .fill(Color::new(0.96, 0.96, 1.0, 1.0))
            .stroke(pastel_blue(), 2.0)
            .corner_radius(10.0),
    );
    // Items inside the group's bounds — they're not "members" in any
    // structural sense (GroupItem doesn't track membership), they
    // just visually overlap.
    let items_y = inner.y + 30.0;
    for i in 0..3 {
        let dot = Rect::new(inner.x + 12.0 + i as f32 * 56.0, items_y, 44.0, 44.0);
        scene.add_item(
            RectItem::new(dot)
                .fill(pastel_red())
                .stroke(ink(), 1.0)
                .access_label(format!("inner item {}", i + 1)),
        );
    }
    let dot2 = Rect::new(inner.x + 12.0, items_y + 64.0, 100.0, 30.0);
    scene.add_item(RectItem::new(dot2).fill(pastel_yellow()).stroke(ink(), 1.0));

    // An invisible (logical-only) sibling group, demonstrated by a tiny
    // hint box + caption.
    let invisible = Rect::new(r.x + 260.0, r.y + 130.0, 220.0, 100.0);
    scene.add_item(GroupItem::new(invisible).label("Logical-only group"));
    scene.add_item(
        TextItem::new(
            "← Same Rect as a group with NO chrome configured. Paints \
             nothing, but `Scene::set_a11y_parent(_, this)` still \
             works for declaring AT structure independent of visuals.",
            Rect::new(invisible.x, invisible.y, invisible.width, invisible.height),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Section 3 — Heavyweight cards + selection
// ---------------------------------------------------------------------------

fn build_card(title: &str, body: &str) -> impl Widget + 'static {
    Panel::new().child(
        VStack::new()
            .spacing(6.0)
            .child(TextWidget::new_literal(title).style(TextStyleRole::BodyBold))
            .child(TextWidget::new_literal(body).style(TextStyleRole::Body)),
    )
}

/// Returns the heavyweight card item-ids and the "Acts" a11y group
/// ids so the caller can wire logical AT-tree structure (Section 6).
fn build_heavyweight_section(scene: &mut Scene) -> Vec<ItemId> {
    add_section_frame(scene, 2, 0, "3. Heavyweight cards + selection");
    add_section_caption(
        scene,
        2,
        0,
        "Real `Panel` widgets placed via Scene::add_widget. Click \
         to select, Ctrl-click to toggle. Drag the empty area to \
         marquee box-select. Selection mode: Multi.",
    );

    let r = section_rect(2, 0);
    let card_w = 220.0;
    let card_h = 80.0;
    let card_x = r.x + 24.0;

    let cards = [
        ("Card A", "Heavyweight Panel — full focus, keyboard, a11y."),
        ("Card B", "Click me, then Ctrl-click another."),
        ("Card C", "Drag empty space below for marquee select."),
    ];
    let mut ids = Vec::new();
    for (i, (title, body)) in cards.iter().enumerate() {
        let rect = Rect::new(card_x, r.y + 130.0 + i as f32 * (card_h + 10.0), card_w, card_h);
        let id = scene.add_widget(build_card(title, body), rect);
        ids.push(id);
    }
    ids
}

// ---------------------------------------------------------------------------
// Section 4 — Drag-to-move sandbox
// ---------------------------------------------------------------------------

fn build_drag_section(scene: &mut Scene) {
    add_section_frame(scene, 0, 1, "4. Drag-to-move");
    add_section_caption(
        scene,
        0,
        1,
        "Any lightweight item is draggable when selection is enabled. \
         Drag end calls Scene::move_item which re-buckets the spatial \
         index — the new bounds are visible to subsequent hit-tests \
         and culling immediately.",
    );

    let r = section_rect(0, 1);
    let labels = ["drag me", "and me", "and me too"];
    let colors = [pastel_blue(), pastel_green(), pastel_yellow()];
    for (i, (label, color)) in labels.iter().zip(colors.iter()).enumerate() {
        let rect = Rect::new(
            r.x + 30.0 + i as f32 * 130.0,
            r.y + 200.0,
            110.0,
            70.0,
        );
        scene.add_item(
            RectItem::new(rect)
                .fill(*color)
                .stroke(ink(), 1.5)
                .access_label(format!("draggable {}", i + 1)),
        );
        scene.add_item(
            TextItem::new(
                *label,
                Rect::new(rect.x + 12.0, rect.y + 24.0, rect.width - 24.0, 30.0),
            )
            .color(ink()),
        );
    }
}

// ---------------------------------------------------------------------------
// Section 5 — Z-order with overlapping items
// ---------------------------------------------------------------------------

fn build_zorder_section(scene: &mut Scene) {
    add_section_frame(scene, 1, 1, "5. Z-order");
    add_section_caption(
        scene,
        1,
        1,
        "Scene::set_z(item, z) controls paint and hit-test ordering. \
         Higher z paints on top (and hit-tests first). Equal z preserves \
         insertion order — stable sort.",
    );

    let r = section_rect(1, 1);
    // Three overlapping rects with explicit z = 0, 1, 2.
    let base_x = r.x + 90.0;
    let base_y = r.y + 180.0;
    let size = 130.0;
    let stagger = 38.0;

    let labels = [(0, pastel_red(), "z=0"), (1, pastel_green(), "z=1"), (2, pastel_blue(), "z=2")];
    for (i, (z, color, label)) in labels.iter().enumerate() {
        let rect = Rect::new(
            base_x + i as f32 * stagger,
            base_y + i as f32 * stagger,
            size,
            size,
        );
        let id = scene.add_item(
            RectItem::new(rect)
                .fill(*color)
                .stroke(ink(), 1.5)
                .access_label(format!("z-stack rect at z={}", z)),
        );
        scene.set_z(id, *z as f32);
        scene.add_item(
            TextItem::new(
                *label,
                Rect::new(rect.x + 8.0, rect.y + 8.0, rect.width - 16.0, 24.0),
            )
            .color(ink()),
        );
    }
}

// ---------------------------------------------------------------------------
// Section 6 — Logical a11y groups
// ---------------------------------------------------------------------------

fn build_a11y_groups_section(scene: &mut Scene, card_ids: &[ItemId]) {
    add_section_frame(scene, 2, 1, "6. Logical a11y groups");
    add_section_caption(
        scene,
        2,
        1,
        "Three Acts as virtual A11yGroups. Each card from Section 3 \
         is parented under its Act so screen readers announce 'Act I, \
         contains: Card A' regardless of visual position.",
    );

    let r = section_rect(2, 1);

    // Add three logical Acts. They have NO visual counterpart — the
    // only on-screen hint is the explanatory text inside this section.
    let act1 = scene.add_a11y_group(A11yGroup::builder().label("Act I — Setup"));
    let act2 = scene.add_a11y_group(A11yGroup::builder().label("Act II — Confrontation"));
    let act3 = scene.add_a11y_group(A11yGroup::builder().label("Act III — Resolution"));
    let acts = [act1, act2, act3];

    // Parent the heavyweight cards from Section 3 under each Act.
    // The 3 cards in Section 3 map 1-1 to the 3 Acts here.
    for (i, card_id) in card_ids.iter().enumerate() {
        let act = acts[i.min(acts.len() - 1)];
        scene.set_a11y_parent(A11yNode::Item(*card_id), Some(A11yNode::Group(act)));
    }

    // Visual hint: three colored "Act" stripes that are NOT part of
    // the AT logical tree — pure decoration. The AT user only hears
    // the Acts when reaching the cards they wrap.
    let stripe_w = (r.width - 50.0) / 3.0;
    for (i, color) in [pastel_red(), pastel_yellow(), pastel_green()].iter().enumerate() {
        let stripe = Rect::new(
            r.x + 25.0 + i as f32 * stripe_w,
            r.y + 220.0,
            stripe_w - 8.0,
            50.0,
        );
        scene.add_item(RectItem::new(stripe).fill(*color).stroke(ink(), 1.0));
        let label_text = ["Act I", "Act II", "Act III"][i];
        scene.add_item(
            TextItem::new(
                label_text,
                Rect::new(stripe.x + 12.0, stripe.y + 14.0, stripe.width - 24.0, 28.0),
            )
            .color(ink()),
        );
    }

    // Footer note about cross-section parent declaration.
    scene.add_item(
        TextItem::new(
            "(Card→Act parent links are logical: visually the cards \
             stay in Section 3, but the AT tree shows them under \
             Act I/II/III here. Open with NVDA or VoiceOver to hear it.)",
            Rect::new(r.x + 14.0, r.y + 290.0, r.width - 28.0, 60.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Connector lines wiring sections together — decorative, lightweight tier.
// ---------------------------------------------------------------------------

fn add_section_connector(scene: &mut Scene, from_section: (usize, usize), to_section: (usize, usize)) {
    let from_rect = section_rect(from_section.0, from_section.1);
    let to_rect = section_rect(to_section.0, to_section.1);
    let from = Point::new(
        from_rect.x + from_rect.width,
        from_rect.y + from_rect.height * 0.5,
    );
    let to = Point::new(to_rect.x, to_rect.y + to_rect.height * 0.5);
    let mid_x = (from.x + to.x) * 0.5;
    let mut path = Path::new();
    path.move_to(from)
        .line_to(Point::new(mid_x, from.y))
        .line_to(Point::new(mid_x, to.y))
        .line_to(to);
    let stroke_w = 2.0;
    let pad = stroke_w * 0.5;
    let bounds = Rect::new(
        from.x.min(to.x).min(mid_x) - pad,
        from.y.min(to.y) - pad,
        (from.x.max(to.x).max(mid_x) - from.x.min(to.x).min(mid_x)).max(stroke_w) + 2.0 * pad,
        (from.y.max(to.y) - from.y.min(to.y)).max(stroke_w) + 2.0 * pad,
    );
    scene.add_item(PathItem::new(path, bounds).stroke(faint_grid(), stroke_w));
}

// ---------------------------------------------------------------------------
// Background tiled grid — gives the scene a sense of scale during pan/zoom.
// ---------------------------------------------------------------------------

fn add_background_grid(scene: &mut Scene) {
    let (w, h) = scene_extent();
    let tile = 50.0;
    let cols = (w / tile).ceil() as i32;
    let rows = (h / tile).ceil() as i32;
    for r in 0..rows {
        for c in 0..cols {
            let cell = Rect::new(c as f32 * tile, r as f32 * tile, tile, tile);
            // Stroke-only — keeps the grid airy.
            let id = scene.add_item(RectItem::new(cell).stroke(faint_grid(), 1.0));
            // Push the grid behind everything else by z = -100.
            scene.set_z(id, -100.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Whole-scene assembly
// ---------------------------------------------------------------------------

fn build_showcase_view() -> SceneView {
    let mut scene = Scene::new();

    add_background_grid(&mut scene);
    add_scene_header(&mut scene);

    build_lightweight_items_section(&mut scene);
    build_groupitem_section(&mut scene);
    let card_ids = build_heavyweight_section(&mut scene);
    build_drag_section(&mut scene);
    build_zorder_section(&mut scene);
    build_a11y_groups_section(&mut scene, &card_ids);

    // Connectors weaving the top row together (decorative).
    add_section_connector(&mut scene, (0, 0), (1, 0));
    add_section_connector(&mut scene, (1, 0), (2, 0));
    // And the bottom row.
    add_section_connector(&mut scene, (0, 1), (1, 1));
    add_section_connector(&mut scene, (1, 1), (2, 1));

    let (w, h) = scene_extent();
    SceneView::new(scene)
        .selection_mode(SceneSelectionMode::Multi)
        .default_size(w, h)
}

// ---------------------------------------------------------------------------
// Reactive header bar — live pan/zoom/selection readout above the scene.
// ---------------------------------------------------------------------------

fn build_status_row(view: &SceneView) -> impl Widget + 'static {
    let pan_x = view.pan_x_signal();
    let pan_y = view.pan_y_signal();
    let zoom = view.zoom_signal();
    let selection = view.selection().selection_signal();

    // pan_x and pan_y zip into one Signal<(f32, f32)>; map to a String.
    let pan_text =
        pan_x
            .zip(&pan_y)
            .map(|(x, y)| format!("Pan: ({:>6.1}, {:>6.1})", x, y));
    let zoom_text = zoom.map(|z| format!("Zoom: ×{:.2}", z));
    let sel_text = selection.map(|s| format!("Selected items: {}", s.len()));

    HStack::new()
        .spacing(28.0)
        .child(
            TextWidget::new_literal("")
                .bind_text(pan_text)
                .style(TextStyleRole::Body),
        )
        .child(
            TextWidget::new_literal("")
                .bind_text(zoom_text)
                .style(TextStyleRole::Body),
        )
        .child(
            TextWidget::new_literal("")
                .bind_text(sel_text)
                .style(TextStyleRole::Body),
        )
}

// ---------------------------------------------------------------------------
// Root composition: header + scene
// ---------------------------------------------------------------------------

fn build_root() -> impl Widget + 'static {
    let view = build_showcase_view();
    let status = build_status_row(&view);

    VStack::new()
        .spacing(8.0)
        .child(
            TextWidget::new_literal("fern-scene showcase")
                .style(TextStyleRole::BodyBold),
        )
        .child(status)
        .child(view)
}

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — fern-scene showcase")
                .size(1400, 900)
                .root(|tree, _state| tree.add(build_root())),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_ui::core::WidgetTree;

    #[test]
    fn showcase_root_lays_out_without_panicking() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let _root = tree.add(build_root());
        tree.layout(SizeProposal::exact(1400.0, 900.0));
    }

    #[test]
    fn scene_has_card_widgets() {
        // Spot-check: building the showcase scene materialises the
        // 3 heavyweight cards (rest of the scene is lightweight).
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let view_id = tree.add(build_showcase_view());
        tree.layout(SizeProposal::exact(1400.0, 900.0));
        let kids = tree.children(view_id);
        assert_eq!(kids.len(), 3, "exactly 3 heavyweight cards");
    }
}
