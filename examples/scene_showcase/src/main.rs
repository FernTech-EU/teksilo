//! `scene-showcase` — comprehensive demo of every `fern-scene` capability.
//!
//! One big pannable / zoomable scene divided into seven labelled sections.
//! Pan with two-finger trackpad / mouse wheel; zoom with Ctrl+wheel or
//! pinch (where supported by the OS). Click a heavyweight card to focus
//! it; Ctrl-click to toggle. Drag in empty space to marquee box-select.
//! Drag any lightweight rectangle to move it — the new position persists
//! (no snap-back).
//!
//! What this exercises:
//!
//! - **Heavyweight tier:** `Panel` cards with real `Button` and
//!   `ComboBox` widgets inside (Section 4).
//! - **Lightweight tier:** `RectItem`, `PathItem`, `TextItem`,
//!   `GroupItem` painted from the SceneView without arena overhead.
//! - **`GroupItem` visual chrome:** fill + stroke + corner-radius +
//!   inline label.
//! - **Z-order:** explicit `Scene::set_z` ordering for overlapping items.
//! - **Selection:** `SceneSelectionMode::Multi`, click + Ctrl-click +
//!   marquee.
//! - **Drag-to-move:** dragging any lightweight rect translates it; the
//!   spatial index re-buckets at drag end (no snap-back).
//! - **Nested SceneView:** an inner read-only SceneView placed inside the
//!   outer one (Section 7) — chart-style pattern, mini-map style pattern.
//! - **Logical a11y groups:** `Scene::add_a11y_group` + `set_a11y_parent`
//!   place cards under named "Acts" so screen readers announce the
//!   logical structure independent of visual placement.
//! - **Reactive view-transform signals:** `pan_x_signal` /
//!   `pan_y_signal` / `zoom_signal` drive the live readout in the header.
//! - **Animations:** pan / zoom / inertial fling all flow through
//!   animated `Signal<f32>`s — the framework's idle scheduler handles
//!   them automatically.
//!
//! Run with: `cargo run -p scene-showcase`

use fern_scene::{
    A11yGroup, A11yNode, GroupItem, ItemId, PathItem, RectItem, Scene,
    SceneSelectionMode, SceneView, TextItem,
};
use fern_ui::canvas::{Path, Point, Rect};
use fern_ui::prelude::*;
use fern_ui::widgets::{Button, ComboBox, HStack, Panel, TextWidget, VStack};

// ---------------------------------------------------------------------------
// Scene layout: 4 columns × 2 rows of sections, with generous gutters so
// marquee dragging in empty space is easy.
// ---------------------------------------------------------------------------

const SECTION_W: f32 = 460.0;
const SECTION_H: f32 = 360.0;
const GUTTER: f32 = 70.0;
const HEADER_H: f32 = 130.0;
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
    let cols = 4;
    let rows = 2;
    let w = SCENE_PAD * 2.0 + cols as f32 * SECTION_W + (cols - 1) as f32 * GUTTER;
    let h = SCENE_PAD * 2.0 + HEADER_H + rows as f32 * SECTION_H + (rows - 1) as f32 * GUTTER;
    (w, h)
}

// ---------------------------------------------------------------------------
// Theme palette
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
fn connector_color() -> Color {
    // Saturated blue — must contrast strongly with the faint
    // background grid so connectors are obviously visible.
    Color::new(0.20, 0.45, 0.85, 1.0)
}

// ---------------------------------------------------------------------------
// Scene-internal header — one big paragraph at the top spanning all columns.
// ---------------------------------------------------------------------------

fn add_scene_header(scene: &mut Scene) {
    let (w, _h) = scene_extent();
    let usable_w = w - 2.0 * SCENE_PAD;

    scene.add_item(
        TextItem::new(
            "fern-scene showcase",
            Rect::new(SCENE_PAD, SCENE_PAD, usable_w, 38.0),
        )
        .color(ink()),
    );
    scene.add_item(
        TextItem::new(
            "Scroll wheel / two-finger trackpad: PAN.   Ctrl+wheel: ZOOM about viewport center.   \
             Pinch (macOS / Win precision touchpad): ZOOM about gesture center.   \
             Click a card: SELECT.   Ctrl-click: TOGGLE.   Drag empty space: MARQUEE.   \
             Drag any rect: MOVE (and the new position persists).",
            Rect::new(SCENE_PAD, SCENE_PAD + 44.0, usable_w, 80.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Per-section helpers — visible chrome (GroupItem) + wrapped caption.
// ---------------------------------------------------------------------------

fn add_section_frame(scene: &mut Scene, col: usize, row: usize, title: &str) {
    let r = section_rect(col, row);
    scene.add_item(
        GroupItem::new(r)
            .label(title)
            .show_label(true)
            .label_inset(16.0, 8.0)
            .label_color(ink())
            .fill(Color::new(0.99, 0.99, 1.00, 1.0))
            .stroke(Color::new(0.55, 0.55, 0.65, 1.0), 1.5)
            .corner_radius(12.0),
    );
}

fn add_section_caption(scene: &mut Scene, col: usize, row: usize, body: &str) {
    let r = section_rect(col, row);
    // Caption rect is wide and ~110 px tall — TextItem now uses
    // `draw_paragraph` which wraps to fit, so multi-line captions
    // render correctly.
    scene.add_item(
        TextItem::new(
            body,
            Rect::new(r.x + 16.0, r.y + 38.0, r.width - 32.0, 110.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Section 1 — Lightweight items (RectItem grid + decorative PathItem)
// ---------------------------------------------------------------------------

fn build_lightweight_items_section(scene: &mut Scene) {
    add_section_frame(scene, 0, 0, "1. Lightweight items");
    add_section_caption(
        scene,
        0,
        0,
        "RectItem, PathItem, TextItem, GroupItem are painted from \
         SceneView::paint without arena overhead. The spatial-index \
         culls off-screen items before they enter the paint walk, so \
         thousands per scene render cheaply.",
    );

    let r = section_rect(0, 0);
    let area_y = r.y + 160.0;

    let cols = 4;
    let rows = 2;
    let cell_w = 36.0;
    let cell_h = 36.0;
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

    // Decorative zigzag PathItem.
    let path_x = r.x + 240.0;
    let path_y = area_y + 8.0;
    let mut zigzag = Path::new();
    zigzag.move_to(Point::new(path_x, path_y));
    let segments = 5;
    for s in 0..segments {
        let dx = (s as f32 + 1.0) * 28.0;
        let dy = if s % 2 == 0 { 60.0 } else { 0.0 };
        zigzag.line_to(Point::new(path_x + dx, path_y + dy));
    }
    let path_bounds = Rect::new(path_x - 2.0, path_y - 2.0, 28.0 * 6.0, 64.0);
    scene.add_item(
        PathItem::new(zigzag, path_bounds)
            .stroke(pastel_purple(), 3.0)
            .access_label("decorative zigzag"),
    );

    scene.add_item(
        TextItem::new(
            "Stroke-only PathItems get per-segment hit-test with \
             stroke-width tolerance. Filled paths use AABB.",
            Rect::new(path_x, path_y + 80.0, 200.0, 60.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Section 2 — Visual GroupItem with nested content
// ---------------------------------------------------------------------------

fn build_groupitem_section(scene: &mut Scene) {
    add_section_frame(scene, 1, 0, "2. GroupItem chrome");
    add_section_caption(
        scene,
        1,
        0,
        "GroupItem can paint its own fill / rounded stroke / inline \
         label, giving you a labelled box around a set of items. \
         With no chrome it stays logical-only — invisible but still \
         a parent in the a11y tree.",
    );

    let r = section_rect(1, 0);

    let inner = Rect::new(r.x + 24.0, r.y + 170.0, 200.0, 160.0);
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
    let dot2 = Rect::new(inner.x + 12.0, items_y + 60.0, 100.0, 30.0);
    scene.add_item(RectItem::new(dot2).fill(pastel_yellow()).stroke(ink(), 1.0));

    let invisible = Rect::new(r.x + 240.0, r.y + 170.0, 200.0, 160.0);
    scene.add_item(GroupItem::new(invisible).label("Logical-only group"));
    scene.add_item(
        TextItem::new(
            "Same Rect → group with NO chrome. Paints nothing, but \
             Scene::set_a11y_parent(_, this) still works for declaring \
             AT structure independent of visuals.",
            Rect::new(invisible.x + 6.0, invisible.y + 6.0, invisible.width - 12.0, invisible.height - 12.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Section 3 — Z-order with overlapping items
// ---------------------------------------------------------------------------

fn build_zorder_section(scene: &mut Scene) {
    add_section_frame(scene, 2, 0, "3. Z-order");
    add_section_caption(
        scene,
        2,
        0,
        "Scene::set_z(item, z) controls paint and hit-test ordering. \
         Higher z paints on top (and hit-tests first). Equal z preserves \
         insertion order — stable sort.",
    );

    let r = section_rect(2, 0);
    let base_x = r.x + 80.0;
    let base_y = r.y + 175.0;
    let size = 130.0;
    let stagger = 38.0;

    let entries = [(0, pastel_red(), "z=0"), (1, pastel_green(), "z=1"), (2, pastel_blue(), "z=2")];
    for (i, (z, color, label)) in entries.iter().enumerate() {
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
                Rect::new(rect.x + 10.0, rect.y + 8.0, rect.width - 20.0, 24.0),
            )
            .color(ink()),
        );
    }
}

// ---------------------------------------------------------------------------
// Section 4 — Heavyweight cards with real interactive widgets inside
// ---------------------------------------------------------------------------

fn build_card_with_button() -> impl Widget + 'static {
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new_literal("Card with Button").style(TextStyleRole::BodyBold))
            .child(TextWidget::new_literal("Real widget machinery — focus, keyboard, a11y.").style(TextStyleRole::Body))
            .child(
                Button::new_literal("Click me")
                    .on_activate_fn(|_ctx| {
                        // Demo button — just prove it's interactive.
                        eprintln!("[scene-showcase] Card-A button clicked");
                    }),
            ),
    )
}

fn build_card_with_combo() -> impl Widget + 'static {
    let selected: Signal<Option<String>> = Signal::new(Some("Apple".to_string()));
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new_literal("Card with ComboBox").style(TextStyleRole::BodyBold))
            .child(TextWidget::new_literal("Heavyweight widgets work normally inside scene_rect.").style(TextStyleRole::Body))
            .child(ComboBox::new(
                vec!["Apple", "Banana", "Cherry", "Date"],
                selected,
            )),
    )
}

fn build_card_plain(title: &str, body: &str) -> impl Widget + 'static {
    Panel::new().child(
        VStack::new()
            .spacing(6.0)
            .child(TextWidget::new_literal(title).style(TextStyleRole::BodyBold))
            .child(TextWidget::new_literal(body).style(TextStyleRole::Body)),
    )
}

/// Returns the heavyweight card item-ids so caller can wire them
/// into logical AT groups in Section 6.
fn build_heavyweight_section(scene: &mut Scene) -> Vec<ItemId> {
    add_section_frame(scene, 3, 0, "4. Heavyweight cards");
    add_section_caption(
        scene,
        3,
        0,
        "Scene::add_widget puts a real Widget at a scene_rect — \
         here, three Panel cards. The first holds an interactive \
         Button; the second a ComboBox. Click to focus, Tab to \
         move focus, Ctrl-click to toggle selection.",
    );

    let r = section_rect(3, 0);
    let card_w = 380.0;
    let card_h = 60.0;
    let card_x = r.x + 20.0;

    let id_a = scene.add_widget(
        build_card_with_button(),
        Rect::new(card_x, r.y + 160.0, card_w, card_h + 30.0),
    );
    let id_b = scene.add_widget(
        build_card_with_combo(),
        Rect::new(card_x, r.y + 160.0 + (card_h + 40.0), card_w, card_h + 30.0),
    );
    let id_c = scene.add_widget(
        build_card_plain("Plain Card", "Drag empty space below for marquee box-select."),
        Rect::new(card_x, r.y + 160.0 + 2.0 * (card_h + 40.0), card_w, card_h),
    );

    vec![id_a, id_b, id_c]
}

// ---------------------------------------------------------------------------
// Section 5 — Drag-to-move sandbox
// ---------------------------------------------------------------------------

fn build_drag_section(scene: &mut Scene) {
    add_section_frame(scene, 0, 1, "5. Drag-to-move");
    add_section_caption(
        scene,
        0,
        1,
        "Lightweight items are draggable when selection is on. Drag \
         end calls Scene::move_item which re-buckets the spatial \
         index — the new position persists, no snap-back.",
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
                Rect::new(rect.x + 12.0, rect.y + 26.0, rect.width - 24.0, 30.0),
            )
            .color(ink()),
        );
    }
}

// ---------------------------------------------------------------------------
// Section 6 — Logical a11y groups (declared here, point at cards in §4)
// ---------------------------------------------------------------------------

fn build_a11y_groups_section(scene: &mut Scene, card_ids: &[ItemId]) {
    add_section_frame(scene, 1, 1, "6. Logical a11y groups");
    add_section_caption(
        scene,
        1,
        1,
        "Three Acts as virtual A11yGroups. The cards from Section 4 \
         are parented under each Act so screen readers announce \
         'Act I, contains: Card A' regardless of visual position.",
    );

    let r = section_rect(1, 1);

    let act1 = scene.add_a11y_group(A11yGroup::builder().label("Act I — Setup"));
    let act2 = scene.add_a11y_group(A11yGroup::builder().label("Act II — Confrontation"));
    let act3 = scene.add_a11y_group(A11yGroup::builder().label("Act III — Resolution"));
    let acts = [act1, act2, act3];

    for (i, card_id) in card_ids.iter().enumerate() {
        let act = acts[i.min(acts.len() - 1)];
        scene.set_a11y_parent(A11yNode::Item(*card_id), Some(A11yNode::Group(act)));
    }

    // Visual hint stripes (not part of the AT tree — pure decoration).
    let stripe_w = (r.width - 40.0) / 3.0;
    for (i, color) in [pastel_red(), pastel_yellow(), pastel_green()].iter().enumerate() {
        let stripe = Rect::new(
            r.x + 20.0 + i as f32 * stripe_w,
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

    scene.add_item(
        TextItem::new(
            "Visually the cards stay in Section 4; logically the AT \
             tree shows them under Act I/II/III declared here. Open \
             with NVDA / VoiceOver / Orca to hear it.",
            Rect::new(r.x + 16.0, r.y + 285.0, r.width - 32.0, 80.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Section 7 — Nested SceneView (scene-in-a-scene)
// ---------------------------------------------------------------------------
//
// A second SceneView placed inside the outer one as a heavyweight widget.
// The inner view runs its own pan/zoom/selection — the user can scroll
// over the inner area to pan it independently. We disable interactivity
// to keep it as a static preview here, but `interactive(true)` works
// (useful for chart-style nested-scene patterns).

fn build_inner_scene() -> impl Widget + 'static {
    let mut inner = Scene::new();
    let palette = [pastel_red(), pastel_blue(), pastel_green(), pastel_yellow(), pastel_purple()];
    // 5×4 dot grid.
    for row in 0..4 {
        for col in 0..5 {
            let color = palette[(row * 5 + col) % palette.len()];
            let dot = Rect::new(20.0 + col as f32 * 30.0, 20.0 + row as f32 * 30.0, 22.0, 22.0);
            inner.add_item(
                RectItem::new(dot)
                    .fill(color)
                    .stroke(ink(), 1.0),
            );
        }
    }
    // A connector through the dots.
    let mut path = Path::new();
    path.move_to(Point::new(31.0, 31.0))
        .line_to(Point::new(151.0, 31.0))
        .line_to(Point::new(151.0, 121.0))
        .line_to(Point::new(31.0, 121.0))
        .close();
    inner.add_item(
        PathItem::new(path, Rect::new(28.0, 28.0, 130.0, 100.0))
            .stroke(connector_color(), 2.5),
    );
    // Big label at top of inner scene.
    inner.add_item(
        TextItem::new(
            "INNER SCENE",
            Rect::new(20.0, 0.0, 200.0, 18.0),
        )
        .color(ink()),
    );
    SceneView::new(inner)
        .nested_a11y(true)
        .a11y_label("Inner scene")
        .default_size(220.0, 160.0)
}

fn build_nested_scene_section(scene: &mut Scene) {
    add_section_frame(scene, 2, 1, "7. Nested SceneView");
    add_section_caption(
        scene,
        2,
        1,
        "A second SceneView placed inside this one via Scene::add_widget. \
         Each inner view runs its own pan / zoom independently. \
         `nested_a11y(true)` flips the inner from Pane to Region for \
         screen readers, plus `a11y_label(...)` names it.",
    );

    let r = section_rect(2, 1);
    let inner_rect = Rect::new(r.x + 30.0, r.y + 170.0, 220.0, 160.0);
    scene.add_widget(build_inner_scene(), inner_rect);

    // Caption to the right of the inner scene.
    scene.add_item(
        TextItem::new(
            "← Pan-zoom this inner scene independently. \
             The chart-style pattern: outer SceneView holds axis \
             chrome (TextItems reading the inner's pan/zoom signals); \
             inner SceneView holds the data and accepts user input.",
            Rect::new(inner_rect.x + inner_rect.width + 10.0, inner_rect.y, 165.0, inner_rect.height),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Section 8 — Animation hint
// ---------------------------------------------------------------------------

fn build_animation_section(scene: &mut Scene) {
    add_section_frame(scene, 3, 1, "8. Animations & idle gates");
    add_section_caption(
        scene,
        3,
        1,
        "Pan / zoom / inertial fling are animated Signal<f32>s. The \
         framework's idle scheduler handles them: animations stop \
         when paused, reduced-motion preference snaps instead of \
         tweens, no CPU/GPU drain at rest.",
    );

    let r = section_rect(3, 1);

    // A trio of static rects — the "animation" lives entirely in
    // the user-driven view-transform changes (pan/zoom).
    for i in 0..3 {
        let dot = Rect::new(
            r.x + 30.0 + i as f32 * 80.0,
            r.y + 200.0,
            60.0,
            60.0,
        );
        let color = [pastel_red(), pastel_yellow(), pastel_green()][i];
        scene.add_item(RectItem::new(dot).fill(color).stroke(ink(), 1.5));
    }

    scene.add_item(
        TextItem::new(
            "Try Ctrl+wheel — zoom animates with EaseOut and respects \
             min/max clamps. Apps drive item-level animations via \
             register_animated_item_signal + pulse_once helpers.",
            Rect::new(r.x + 16.0, r.y + 280.0, r.width - 32.0, 70.0),
        )
        .color(dim_ink()),
    );
}

// ---------------------------------------------------------------------------
// Decorative connectors — saturated blue, NOT background-grid color.
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
    let stroke_w = 3.0;
    let pad = stroke_w * 0.5 + 2.0;
    let bounds = Rect::new(
        from.x.min(to.x).min(mid_x) - pad,
        from.y.min(to.y) - pad,
        (from.x.max(to.x).max(mid_x) - from.x.min(to.x).min(mid_x)).max(stroke_w) + 2.0 * pad,
        (from.y.max(to.y) - from.y.min(to.y)).max(stroke_w) + 2.0 * pad,
    );
    scene.add_item(PathItem::new(path, bounds).stroke(connector_color(), stroke_w));
}

// ---------------------------------------------------------------------------
// Background grid (z = -100 keeps it behind everything else).
// ---------------------------------------------------------------------------

fn add_background_grid(scene: &mut Scene) {
    let (w, h) = scene_extent();
    let tile = 60.0;
    let cols = (w / tile).ceil() as i32;
    let rows = (h / tile).ceil() as i32;
    for r in 0..rows {
        for c in 0..cols {
            let cell = Rect::new(c as f32 * tile, r as f32 * tile, tile, tile);
            let id = scene.add_item(RectItem::new(cell).stroke(faint_grid(), 1.0));
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
    build_zorder_section(&mut scene);
    let card_ids = build_heavyweight_section(&mut scene);
    build_drag_section(&mut scene);
    build_a11y_groups_section(&mut scene, &card_ids);
    build_nested_scene_section(&mut scene);
    build_animation_section(&mut scene);

    // Connectors weaving each row together.
    add_section_connector(&mut scene, (0, 0), (1, 0));
    add_section_connector(&mut scene, (1, 0), (2, 0));
    add_section_connector(&mut scene, (2, 0), (3, 0));
    add_section_connector(&mut scene, (0, 1), (1, 1));
    add_section_connector(&mut scene, (1, 1), (2, 1));
    add_section_connector(&mut scene, (2, 1), (3, 1));

    let (w, h) = scene_extent();
    SceneView::new(scene)
        .selection_mode(SceneSelectionMode::Multi)
        .default_size(w, h)
}

// ---------------------------------------------------------------------------
// Reactive header bar — live pan/zoom/selection readout.
// ---------------------------------------------------------------------------

fn build_status_row(view: &SceneView) -> impl Widget + 'static {
    let pan_x = view.pan_x_signal();
    let pan_y = view.pan_y_signal();
    let zoom = view.zoom_signal();
    let selection = view.selection().selection_signal();

    let pan_text = pan_x
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
// Root composition
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
                .size(1500, 950)
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
        tree.layout(SizeProposal::exact(1500.0, 950.0));
    }

    #[test]
    fn outer_scene_has_one_inner_scene_widget() {
        // The outer SceneView contains: 3 cards (§4) + 1 inner
        // SceneView (§7) = 4 heavyweight children.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let view_id = tree.add(build_showcase_view());
        tree.layout(SizeProposal::exact(1500.0, 950.0));
        let kids = tree.children(view_id);
        assert_eq!(
            kids.len(),
            4,
            "outer scene must have 3 cards + 1 nested SceneView"
        );
    }
}
