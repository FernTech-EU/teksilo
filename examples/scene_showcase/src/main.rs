// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `scene-showcase` — comprehensive demo of every `bastyde-scene` capability.
//!
//! One pannable / zoomable scene divided into eight labelled sections.
//! Pan with two-finger trackpad / mouse wheel; zoom with Ctrl+wheel or
//! pinch (where the OS supports it). Click a heavyweight card to focus
//! it; Ctrl-click to toggle. Drag in empty space to marquee. Drag a
//! "drag me" rectangle to move it — the new position persists.
//!
//! What this exercises:
//!
//! - **Heavyweight tier:** `Panel` cards with real `Button` and
//!   `ComboBox` widgets inside (Section 4).
//! - **Lightweight tier:** `RectItem`, `PathItem`, `TextItem`,
//!   `GroupItem` painted from the SceneView without arena overhead.
//! - **`GroupItem` chrome:** fill + stroke + corner-radius + inline label.
//! - **Selective drag:** items default to NOT draggable. Apps opt in via
//!   `.draggable(true)`. Section 5 shows the difference: only the three
//!   labelled rects move; everything else stays put.
//! - **Z-order:** explicit `Scene::set_z` for overlapping items.
//! - **Selection:** `SceneSelectionMode::Multi` — click + Ctrl-click +
//!   marquee box-select.
//! - **Movement policy:** the outer view clamps pan to the scene
//!   extent (`Scene::set_pan_bounds`) and zoom to a readable band
//!   (`Scene::set_zoom_range`); the toolbar toggles
//!   `DragMode::RubberBand` ⇄ `ScrollHandDrag`; the §7 inner view is
//!   locked to horizontal pan (`Scene::pan_axes`).
//! - **Nested SceneView:** an inner SceneView inside the outer one.
//! - **Logical a11y groups:** `Scene::add_a11y_group` + `set_a11y_parent`
//!   place cards under named "Acts" so screen readers announce a logical
//!   structure independent of visual placement.
//! - **Custom SceneItem:** Section 8 ships a `PulsingDot` impl that
//!   owns a `Signal<f32>` and uses `register_animated_item_signal` +
//!   `animate_looping` to drive a continuous opacity pulse.
//! - **Reactive view-transform signals:** `pan_x_signal` /
//!   `pan_y_signal` / `zoom_signal` drive the live readout in the header.
//!
//! Run with: `cargo run -p scene-showcase`

use std::time::Duration;

use bastyde::canvas::{Canvas, Path, Point, Rect, StrokeStyle};
use bastyde::core::binding::BindingLevel;
use bastyde::prelude::*;
use bastyde::tokens::{Alignment, Easing};
use bastyde::widgets::{
    Button, ComboBox, Expand, HStack, Padding, Panel, ScrollArea, Spacer, TextWidget, Toolbar,
    VStack, ZStack,
};
use bastyde_scene::{
    A11yGroup, A11yNode, DragMode, GroupItem, ItemId, PanAxes, PathItem, RectItem, Scene,
    SceneItem, SceneItemPaintContext, SceneMinimap, SceneSelectionMode, SceneView, ScrollBarMode,
    ScrollBarPolicy, TextItem, register_animated_item_signal,
};

// ---------------------------------------------------------------------------
// Scene layout: tight 4×2 grid that fits naturally in a 1500×950 window
// without forcing the user to pan to find content.
// ---------------------------------------------------------------------------

const SECTION_W: f32 = 320.0;
const SECTION_H: f32 = 280.0;
const GUTTER: f32 = 50.0;
const HEADER_H: f32 = 110.0;
const SCENE_PAD: f32 = 30.0;

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
    Color::new(0.20, 0.45, 0.85, 1.0)
}

// ---------------------------------------------------------------------------
// PulsingDot — a custom SceneItem demonstrating item-level animations.
// Shows the full pattern an app would use to ship its own animated items:
//   1. Own a `Signal<f32>` (here: opacity).
//   2. In `register_bindings`: register the signal with the SceneView's
//      animation scheduler, bind it at `RepaintOnly` so changes dirty
//      paint, then kick off `animate_looping`.
//   3. In `paint`: read the current signal value and paint accordingly.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct PulsingDot {
    bounds: Rect,
    base_color: Color,
    /// Goes 0.0 → 1.0 → 0.0 → … via `animate_looping`. We map it to
    /// an opacity range of [0.25, 1.0] so the dot never fully
    /// disappears (visibility tracking).
    phase: Signal<f32>,
}

impl PulsingDot {
    fn new(bounds: Rect, base_color: Color) -> Self {
        Self {
            bounds,
            base_color,
            phase: Signal::new_animated(0.0),
        }
    }
}

impl SceneItem for PulsingDot {
    fn local_bounds(&self) -> Rect {
        self.bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        // Phase signal goes 0..1 looped. Triangle-wave-shape it so
        // the dot pulses smoothly: dim → bright → dim → …
        let phase = self.phase.get().clamp(0.0, 1.0);
        let triangle = 1.0 - (2.0 * phase - 1.0).abs();
        let alpha = (0.25 + 0.75 * triangle) * self.base_color.a();
        let c = self.base_color.with_alpha(alpha);
        canvas.fill_rect(self.bounds, c);
        canvas.stroke_rect(self.bounds, ink(), StrokeStyle::solid(1.0));
    }

    fn register_bindings(&self, ctx: &mut BuildContext, scene_view_id: WidgetId) {
        // Hook into the scheduler so the framework's idle gates apply.
        register_animated_item_signal(ctx, &self.phase);
        // Repaint on each phase tick.
        self.phase.bind_to(
            scene_view_id,
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        // Kick off the loop. Idempotent — re-registering on rebuild
        // doesn't restart, the framework dedups via signal id.
        self.phase
            .animate_looping(1.0, Duration::from_millis(1400), Easing::Linear, None);
    }
}

// ---------------------------------------------------------------------------
// Scene-internal header
// ---------------------------------------------------------------------------

fn add_scene_header(scene: &mut Scene) {
    let (w, _h) = scene_extent();
    let usable_w = w - 2.0 * SCENE_PAD;

    scene.add_item(
        TextItem::new(
            lit!("bastyde-scene showcase — eight labelled sections, all visible at zoom 1.0"),
            Rect::new(SCENE_PAD, SCENE_PAD, usable_w, 30.0),
        )
        .color(ink()),
        Point::ZERO,
    );
    scene.add_item(
        TextItem::new(
            lit!("Scroll wheel / two-finger trackpad: PAN.   Ctrl+wheel: ZOOM about viewport center.   \
             Pinch (macOS / Win precision touchpad): ZOOM about gesture center.   \
             Click card: SELECT.   Ctrl-click: TOGGLE.   Drag empty space: MARQUEE.   \
             Drag a 'drag me' rect: MOVE.   Other items stay put — drag is opt-in via .draggable(true)."),
            Rect::new(SCENE_PAD, SCENE_PAD + 36.0, usable_w, 70.0),
        )
        .color(dim_ink()), Point::ZERO
    );
}

// ---------------------------------------------------------------------------
// Section helpers
// ---------------------------------------------------------------------------

fn add_section_frame(scene: &mut Scene, col: usize, row: usize, title: &str) {
    let r = section_rect(col, row);
    scene.add_item(
        GroupItem::new(r)
            .label(lit!(title))
            .show_label(true)
            .label_inset(12.0, 6.0)
            .label_color(ink())
            .fill(Color::new(0.99, 0.99, 1.00, 1.0))
            .stroke(Color::new(0.55, 0.55, 0.65, 1.0), 1.5)
            .corner_radius(10.0),
        Point::ZERO,
    );
}

fn add_section_caption(scene: &mut Scene, col: usize, row: usize, body: &str) {
    let r = section_rect(col, row);
    scene.add_item(
        TextItem::new(
            lit!(body),
            Rect::new(r.x + 12.0, r.y + 30.0, r.width - 24.0, 90.0),
        )
        .color(dim_ink()),
        Point::ZERO,
    );
}

// ---------------------------------------------------------------------------
// Section 1 — Lightweight items
// ---------------------------------------------------------------------------

fn build_lightweight_items_section(scene: &mut Scene) {
    add_section_frame(scene, 0, 0, "1. Lightweight items");
    add_section_caption(
        scene,
        0,
        0,
        "RectItem, PathItem, TextItem, GroupItem paint from \
         SceneView without arena overhead. The spatial-index culls \
         off-screen items before paint.",
    );

    let r = section_rect(0, 0);
    let area_y = r.y + 130.0;

    // 5×2 colored tile grid.
    let cols = 5;
    let rows = 2;
    let cell_w = 30.0;
    let cell_h = 30.0;
    let cell_gap = 6.0;
    let palette = [
        pastel_red(),
        pastel_blue(),
        pastel_green(),
        pastel_yellow(),
        pastel_purple(),
    ];
    let grid_x = r.x + 16.0;
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
                    .access_label(lit!(format!("tile {}", i + 1))),
                Point::ZERO,
            );
        }
    }

    // Decorative zigzag PathItem on the right half.
    let path_x = r.x + 200.0;
    let path_y = area_y + 4.0;
    let mut zigzag = Path::new();
    zigzag.move_to(Point::new(path_x, path_y));
    let segments = 4;
    for s in 0..segments {
        let dx = (s as f32 + 1.0) * 22.0;
        let dy = if s % 2 == 0 { 50.0 } else { 0.0 };
        zigzag.line_to(Point::new(path_x + dx, path_y + dy));
    }
    let path_bounds = Rect::new(path_x - 2.0, path_y - 2.0, 22.0 * 5.0, 54.0);
    scene.add_item(
        PathItem::new(zigzag, path_bounds)
            // Cosmetic stroke: crisp constant-width zigzag at any zoom.
            .stroke_cosmetic(pastel_purple(), 3.0)
            .access_label(lit!("decorative zigzag")),
        Point::ZERO,
    );
    scene.add_item(
        TextItem::new(
            lit!("Stroke-only paths get per-segment hit-test."),
            Rect::new(path_x - 4.0, path_y + 60.0, 130.0, 60.0),
        )
        .color(dim_ink()),
        Point::ZERO,
    );
}

// ---------------------------------------------------------------------------
// Section 2 — GroupItem chrome
// ---------------------------------------------------------------------------

fn build_groupitem_section(scene: &mut Scene) {
    add_section_frame(scene, 1, 0, "2. GroupItem chrome");
    add_section_caption(
        scene,
        1,
        0,
        "GroupItem can paint fill + stroke + inline label. With no \
         chrome it stays logical-only — invisible, but still a parent \
         in the a11y tree.",
    );

    let r = section_rect(1, 0);

    let inner = Rect::new(r.x + 16.0, r.y + 130.0, 140.0, 130.0);
    scene.add_item(
        GroupItem::new(inner)
            .label(lit!("Visible group"))
            .show_label(true)
            .label_inset(8.0, 4.0)
            .label_color(ink())
            .fill(Color::new(0.96, 0.96, 1.0, 1.0))
            .stroke(pastel_blue(), 2.0)
            .corner_radius(10.0),
        Point::ZERO,
    );
    let items_y = inner.y + 28.0;
    for i in 0..3 {
        let dot = Rect::new(inner.x + 10.0 + i as f32 * 42.0, items_y, 32.0, 32.0);
        scene.add_item(
            RectItem::new(dot)
                .fill(pastel_red())
                .stroke(ink(), 1.0)
                .access_label(lit!(format!("inner item {}", i + 1))),
            Point::ZERO,
        );
    }
    let dot2 = Rect::new(inner.x + 10.0, items_y + 50.0, 90.0, 26.0);
    scene.add_item(
        RectItem::new(dot2).fill(pastel_yellow()).stroke(ink(), 1.0),
        Point::ZERO,
    );

    let invisible = Rect::new(r.x + 168.0, r.y + 130.0, 138.0, 130.0);
    scene.add_item(
        GroupItem::new(invisible).label(lit!("Logical-only group")),
        Point::ZERO,
    );
    scene.add_item(
        TextItem::new(
            lit!(
                "Same Rect as a group with NO chrome. Paints nothing, but \
             set_a11y_parent still works."
            ),
            Rect::new(
                invisible.x + 6.0,
                invisible.y + 6.0,
                invisible.width - 12.0,
                invisible.height - 12.0,
            ),
        )
        .color(dim_ink()),
        Point::ZERO,
    );
}

// ---------------------------------------------------------------------------
// Section 3 — Z-order
// ---------------------------------------------------------------------------

fn build_zorder_section(scene: &mut Scene) {
    add_section_frame(scene, 2, 0, "3. Z-order");
    add_section_caption(
        scene,
        2,
        0,
        "Scene::set_z(item, z) controls paint and hit-test ordering. \
         Higher z paints on top. Equal z preserves insertion order.",
    );

    let r = section_rect(2, 0);
    let base_x = r.x + 60.0;
    let base_y = r.y + 130.0;
    let size = 110.0;
    let stagger = 30.0;

    let entries = [
        (0, pastel_red(), "z=0"),
        (1, pastel_green(), "z=1"),
        (2, pastel_blue(), "z=2"),
    ];
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
                .access_label(lit!(format!("z-stack rect at z={}", z))),
            Point::ZERO,
        );
        scene.set_z(id, *z as f32);
        scene.add_item(
            TextItem::new(
                lit!(*label),
                Rect::new(rect.x + 8.0, rect.y + 6.0, rect.width - 16.0, 22.0),
            )
            .color(ink()),
            Point::ZERO,
        );
    }
}

// ---------------------------------------------------------------------------
// Section 4 — Heavyweight cards (with Button + ComboBox inside)
// ---------------------------------------------------------------------------

fn build_card_with_button() -> impl Widget + 'static {
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new(lit!("Card with Button")).style(TextStyleRole::BodyBold))
            .child(
                TextWidget::new(lit!("Real widget machinery: focus, keyboard, a11y."))
                    .style(TextStyleRole::Body),
            )
            .child(Button::new(lit!("Click me")).on_activate_fn(|_ctx| {
                eprintln!("[scene-showcase] button clicked");
            })),
    )
}

fn build_card_with_combo() -> impl Widget + 'static {
    let selected: Signal<Option<String>> = Signal::new(Some("Apple".to_string()));
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new(lit!("Card with ComboBox")).style(TextStyleRole::BodyBold))
            .child(TextWidget::new(lit!("Pick a fruit from the list:")).style(TextStyleRole::Body))
            .child(ComboBox::new(
                vec!["Apple", "Banana", "Cherry", "Date"],
                selected,
            )),
    )
}

fn build_card_plain(title: &str, body: &str) -> impl Widget + 'static {
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new(lit!(title)).style(TextStyleRole::BodyBold))
            .child(TextWidget::new(lit!(body)).style(TextStyleRole::Body)),
    )
}

/// Cards are wrapped in a `ScrollArea` so each one gets a comfortable
/// height (Button / ComboBox stay easy to click) without being
/// crammed into the section's vertical budget. The whole ScrollArea
/// goes into the scene as ONE heavyweight widget; the cards inside
/// it are children of the ScrollArea, not direct scene entries.
///
/// Returns the ScrollArea's `ItemId` so Section 6 can parent its
/// AT-tree under the Acts.
fn build_heavyweight_section(scene: &mut Scene) -> ItemId {
    add_section_frame(scene, 3, 0, "4. Heavyweight cards");
    add_section_caption(
        scene,
        3,
        0,
        "Scene::add_widget puts a real Widget at a scene_rect. Wrap \
         several cards in a ScrollArea so each card stays a \
         comfortable size — scroll inside this section.",
    );

    let r = section_rect(3, 0);
    let area_x = r.x + 10.0;
    let area_y = r.y + 130.0;
    let area_w = r.width - 20.0;
    let area_h = r.height - 140.0;

    // ScrollArea contains a tall VStack of cards. The total content
    // height (~360 px) exceeds the area's height (~140 px), so the
    // user scrolls inside this section to reveal each card.
    let scroll_area = ScrollArea::new().child(
        VStack::new()
            .spacing(10.0)
            .child(build_card_with_button())
            .child(build_card_with_combo())
            .child(build_card_plain(
                "Plain card",
                "Click to focus, Ctrl-click to toggle selection. Drag empty space outside this section for marquee.",
            )),
    );

    scene.add_widget(scroll_area, Rect::new(area_x, area_y, area_w, area_h))
}

// ---------------------------------------------------------------------------
// Section 5 — Drag-to-move (only here are items draggable)
// ---------------------------------------------------------------------------

fn build_drag_section(scene: &mut Scene) {
    add_section_frame(scene, 0, 1, "5. Drag-to-move");
    add_section_caption(
        scene,
        0,
        1,
        "Items default to NOT draggable. Apps opt in with \
         .draggable(true). Drag end calls Scene::set_local_pos which \
         re-buckets the spatial index — the new position persists.",
    );

    let r = section_rect(0, 1);
    let labels = ["drag me", "and me", "and me too"];
    let colors = [pastel_blue(), pastel_green(), pastel_yellow()];
    for (i, (label, color)) in labels.iter().zip(colors.iter()).enumerate() {
        let rect = Rect::new(r.x + 16.0 + i as f32 * 100.0, r.y + 160.0, 85.0, 70.0);
        // QGraphicsScene-style parent/child: the label is a child
        // of the rect. The rect is `.draggable(true)`; the label
        // isn't, but `Scene::set_item_parent` makes it cascade
        // along when the rect is dragged.
        let parent = scene.add_item(
            RectItem::new(rect)
                .fill(*color)
                .stroke(ink(), 1.5)
                .draggable(true)
                .access_label(lit!(format!("draggable {}", i + 1))),
            Point::ZERO,
        );
        let label_id = scene.add_item(
            TextItem::new(
                lit!(*label),
                Rect::new(rect.x + 8.0, rect.y + 24.0, rect.width - 16.0, 28.0),
            )
            .color(ink()),
            Point::ZERO,
        );
        scene.set_item_parent(label_id, Some(parent));
    }
}

// ---------------------------------------------------------------------------
// Section 6 — Logical a11y groups
// ---------------------------------------------------------------------------

fn build_a11y_groups_section(scene: &mut Scene, scroll_area_id: ItemId) {
    add_section_frame(scene, 1, 1, "6. Logical a11y groups");
    add_section_caption(
        scene,
        1,
        1,
        "Three virtual A11yGroups (no on-screen counterpart). Section \
         4's ScrollArea is parented under Act I so screen readers \
         announce 'Act I, contains: ScrollArea' regardless of visual \
         placement. Add more cards and parent each individually.",
    );

    let r = section_rect(1, 1);

    let act1 = scene.add_a11y_group(A11yGroup::builder().label(lit!("Act I — Setup")));
    let act2 = scene.add_a11y_group(A11yGroup::builder().label(lit!("Act II — Confrontation")));
    let _act3 = scene.add_a11y_group(A11yGroup::builder().label(lit!("Act III — Resolution")));

    // Parent the ScrollArea (and thereby its cards) under Act I.
    scene.set_a11y_parent(A11yNode::Item(scroll_area_id), Some(A11yNode::Group(act1)));
    let _ = act2;

    // Visual hint stripes (decoration only — not in the AT tree).
    let stripe_w = (r.width - 32.0) / 3.0;
    for (i, color) in [pastel_red(), pastel_yellow(), pastel_green()]
        .iter()
        .enumerate()
    {
        let stripe = Rect::new(
            r.x + 16.0 + i as f32 * stripe_w,
            r.y + 160.0,
            stripe_w - 6.0,
            44.0,
        );
        scene.add_item(
            RectItem::new(stripe).fill(*color).stroke(ink(), 1.0),
            Point::ZERO,
        );
        let label_text = ["Act I", "Act II", "Act III"][i];
        scene.add_item(
            TextItem::new(
                lit!(label_text),
                Rect::new(stripe.x + 8.0, stripe.y + 12.0, stripe.width - 16.0, 24.0),
            )
            .color(ink()),
            Point::ZERO,
        );
    }

    scene.add_item(
        TextItem::new(
            lit!(
                "Visual placement vs logical AT tree: the cards stay in §4; \
             the AT walker reports them under these Acts."
            ),
            Rect::new(r.x + 12.0, r.y + 215.0, r.width - 24.0, 60.0),
        )
        .color(dim_ink()),
        Point::ZERO,
    );
}

// ---------------------------------------------------------------------------
// Section 7 — Nested SceneView
// ---------------------------------------------------------------------------

fn build_inner_scene() -> impl Widget + 'static {
    let mut inner = Scene::new();
    let palette = [
        pastel_red(),
        pastel_blue(),
        pastel_green(),
        pastel_yellow(),
        pastel_purple(),
    ];
    for row in 0..3 {
        for col in 0..8 {
            let color = palette[(row * 8 + col) % palette.len()];
            let dot = Rect::new(
                15.0 + col as f32 * 28.0,
                14.0 + row as f32 * 26.0,
                22.0,
                22.0,
            );
            inner.add_item(
                RectItem::new(dot).fill(color).stroke(ink(), 1.0),
                Point::ZERO,
            );
        }
    }
    let mut path = Path::new();
    path.move_to(Point::new(26.0, 25.0))
        .line_to(Point::new(110.0, 25.0))
        .line_to(Point::new(110.0, 77.0))
        .line_to(Point::new(26.0, 77.0))
        .close();
    inner.add_item(
        PathItem::new(path, Rect::new(24.0, 22.0, 90.0, 60.0))
            .stroke_cosmetic(connector_color(), 2.0),
        Point::ZERO,
    );
    // Movement policy demo: this nested view is locked to horizontal
    // pan. The dot grid overflows its 135 px width, so left/right scroll
    // pans it; vertical scroll deltas fall through to the outer view.
    inner.pan_axes(PanAxes::Horizontal);
    SceneView::new(inner)
        .nested_a11y(true)
        .a11y_label(lit!("Inner scene"))
        .default_size(135.0, 105.0)
}

fn build_nested_scene_section(scene: &mut Scene) {
    add_section_frame(scene, 2, 1, "7. Nested SceneView");
    add_section_caption(
        scene,
        2,
        1,
        "A nested SceneView with its own movement policy: locked to \
         horizontal pan via Scene::pan_axes(Horizontal). Vertical \
         scroll falls through to the outer view. nested_a11y(true) \
         flips Pane → Region.",
    );

    let r = section_rect(2, 1);
    let inner_rect = Rect::new(r.x + 16.0, r.y + 140.0, 135.0, 105.0);
    scene.add_widget(build_inner_scene(), inner_rect);

    scene.add_item(
        TextItem::new(
            lit!(
                "← This inner viewport pans horizontally only; vertical \
             two-finger scroll passes through to the outer scene. \
             Per-view movement policy."
            ),
            Rect::new(
                inner_rect.x + inner_rect.width + 8.0,
                inner_rect.y,
                145.0,
                inner_rect.height,
            ),
        )
        .color(dim_ink()),
        Point::ZERO,
    );
}

// ---------------------------------------------------------------------------
// Section 8 — Animated PulsingDot (custom SceneItem)
// ---------------------------------------------------------------------------

fn build_animation_section(scene: &mut Scene) {
    add_section_frame(scene, 3, 1, "8. Animated items");
    add_section_caption(
        scene,
        3,
        1,
        "PulsingDot is a custom SceneItem that owns a Signal<f32> \
         and uses register_animated_item_signal + animate_looping in \
         register_bindings. Watch them pulse independently.",
    );

    let r = section_rect(3, 1);
    // Three dots that all pulse — each one is its own SceneItem
    // with its own animated signal, scheduled by the framework.
    for (i, color) in [pastel_red(), pastel_yellow(), pastel_green()]
        .iter()
        .enumerate()
    {
        let dot = Rect::new(r.x + 30.0 + i as f32 * 90.0, r.y + 160.0, 70.0, 70.0);
        scene.add_item(PulsingDot::new(dot, *color), Point::ZERO);
    }
    scene.add_item(
        TextItem::new(
            lit!(
                "Idle scheduler still applies: tab-out the window, \
             ticks pause; no CPU/GPU drain when not visible."
            ),
            Rect::new(r.x + 12.0, r.y + 240.0, r.width - 24.0, 50.0),
        )
        .color(dim_ink()),
        Point::ZERO,
    );
}

// ---------------------------------------------------------------------------
// Decorative connectors
// ---------------------------------------------------------------------------

fn add_section_connector(
    scene: &mut Scene,
    from_section: (usize, usize),
    to_section: (usize, usize),
) {
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
    scene.add_item(
        PathItem::new(path, bounds).stroke_cosmetic(connector_color(), stroke_w),
        Point::ZERO,
    );
}

// ---------------------------------------------------------------------------
// Whole-scene assembly
// ---------------------------------------------------------------------------

fn build_showcase_view() -> SceneView {
    let mut scene = Scene::new();

    add_scene_header(&mut scene);

    build_lightweight_items_section(&mut scene);
    build_groupitem_section(&mut scene);
    build_zorder_section(&mut scene);
    let scroll_area_id = build_heavyweight_section(&mut scene);
    build_drag_section(&mut scene);
    build_a11y_groups_section(&mut scene, scroll_area_id);
    build_nested_scene_section(&mut scene);
    build_animation_section(&mut scene);

    add_section_connector(&mut scene, (0, 0), (1, 0));
    add_section_connector(&mut scene, (1, 0), (2, 0));
    add_section_connector(&mut scene, (2, 0), (3, 0));
    add_section_connector(&mut scene, (0, 1), (1, 1));
    add_section_connector(&mut scene, (1, 1), (2, 1));
    add_section_connector(&mut scene, (2, 1), (3, 1));

    let (w, h) = scene_extent();
    // Movement policy demo: clamp the visible viewport to the scene
    // extent so the user can't pan into empty space, and keep zoom in a
    // readable band. Both are reactive Scene-level signals.
    scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, w, h)));
    scene.set_zoom_range(Some(0.5_f32..=3.0_f32));
    SceneView::new(scene)
        .selection_mode(SceneSelectionMode::Multi)
        .default_size(w, h)
        // Start slightly zoomed in so the content overflows the viewport and
        // the scroll bars are active and draggable on launch. Zooming back out
        // to fit hides them again (the `AsNeeded` policy in `build_root`).
        .initial_zoom(1.4)
        // R5 background closure: paints a zoom-aware 50-unit grid in
        // scene coords. The closure receives the visible scene region,
        // so off-screen lines are skipped — at zoom 0.1× over a
        // 50,000-unit scene we still emit only the lines on screen.
        // The grid uses a HAIRLINE stroke (StrokeStyle::hairline): a
        // transform-invariant 1px line that stays crisp at every zoom
        // instead of thinning out and vanishing.
        .background(|canvas, _ctx, region| {
            let step = 50.0_f32;
            let color = faint_grid();
            let x0 = (region.x / step).floor() * step;
            let y0 = (region.y / step).floor() * step;
            let x_end = region.x + region.width;
            let y_end = region.y + region.height;
            let mut x = x0;
            while x <= x_end {
                canvas.draw_line(
                    bastyde::canvas::Point::new(x, region.y),
                    bastyde::canvas::Point::new(x, y_end),
                    color,
                    StrokeStyle::hairline(1.0),
                );
                x += step;
            }
            let mut y = y0;
            while y <= y_end {
                canvas.draw_line(
                    bastyde::canvas::Point::new(region.x, y),
                    bastyde::canvas::Point::new(x_end, y),
                    color,
                    StrokeStyle::hairline(1.0),
                );
                y += step;
            }
        })
}

// ---------------------------------------------------------------------------
// Reactive header
// ---------------------------------------------------------------------------

fn build_status_row(view: &SceneView) -> impl Widget + 'static {
    let pan_x = view.pan_x_signal();
    let pan_y = view.pan_y_signal();
    let zoom = view.zoom_signal();
    let selection = view.selection().selection_signal();
    let drag = view.drag_mode_signal();

    let pan_text = pan_x
        .zip(&pan_y)
        .map(|(x, y)| format!("Pan: ({:>6.1}, {:>6.1})", x, y));
    let zoom_text = zoom.map(|z| format!("Zoom: ×{:.2}", z));
    let sel_text = selection.map(|s| format!("Selected items: {}", s.len()));
    let drag_text = drag.map(|m| {
        let label = match m {
            DragMode::RubberBand => "Marquee",
            DragMode::ScrollHandDrag => "Pan",
            DragMode::NoDrag => "None",
        };
        format!("Drag: {label}")
    });

    HStack::new()
        .spacing(28.0)
        .child(
            TextWidget::new(lit!(""))
                .text(pan_text)
                .style(TextStyleRole::Body),
        )
        .child(
            TextWidget::new(lit!(""))
                .text(zoom_text)
                .style(TextStyleRole::Body),
        )
        .child(
            TextWidget::new(lit!(""))
                .text(sel_text)
                .style(TextStyleRole::Body),
        )
        .child(
            TextWidget::new(lit!(""))
                .text(drag_text)
                .style(TextStyleRole::Body),
        )
}

fn build_toolbar(drag_mode: Signal<DragMode>) -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .spacing(12.0)
            .child(
                Button::new(lit!("Drag mode: Marquee ⇄ Pan")).on_activate_fn(move |_ctx| {
                    // Flip between marquee box-select and scroll-hand pan.
                    let next = match drag_mode.get() {
                        DragMode::ScrollHandDrag => DragMode::RubberBand,
                        _ => DragMode::ScrollHandDrag,
                    };
                    drag_mode.set(next);
                }),
            )
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

fn build_root() -> impl Widget + 'static {
    let view = build_showcase_view();
    let drag_mode = view.drag_mode_signal();
    let status = build_status_row(&view);

    // Minimap: a bottom-trailing thumbnail of the whole scene with a live
    // viewport rectangle. Read its inputs from the view *before* moving `view`
    // into the ZStack. `item_thumbnails()` snapshots both tiers (cards + the
    // lightweight grid/connectors); the viewport rect is reactive on its own,
    // tracking pan / zoom with no extra plumbing. (A live "items as they move"
    // minimap would re-snapshot on mutation; this showcase scene is static.)
    let content = view.scene_content_bounds().unwrap_or_else(|| {
        let (w, h) = scene_extent();
        Rect::new(0.0, 0.0, w, h)
    });
    let minimap = SceneMinimap::new(content, view.viewport_in_scene_signal())
        .items(view.scene().item_thumbnails())
        .size(240.0, 160.0)
        .background(Color::new(0.10, 0.11, 0.15, 0.82))
        .border(Some((Color::new(1.0, 1.0, 1.0, 0.35), 1.0)))
        .content_outline(Some((Color::new(1.0, 1.0, 1.0, 0.18), 1.0)))
        .viewport_color(Color::new(0.40, 0.66, 1.0, 0.95));

    // Wrap the view in a SceneScrollView so the viewport gains overlay scroll
    // bars. They track pan/zoom and let the user scroll by dragging; native
    // wheel / hand-drag panning keeps working. `view` is consumed here — every
    // signal the minimap / status row needed was captured above first.
    let scrollable = view
        .with_scroll_bars()
        .scroll_bar_mode(ScrollBarMode::Overlay)
        .vertical_policy(ScrollBarPolicy::AsNeeded)
        .horizontal_policy(ScrollBarPolicy::AsNeeded);

    VStack::new()
        .spacing(8.0)
        .child(build_toolbar(drag_mode))
        .child(TextWidget::new(lit!("bastyde-scene showcase")).style(TextStyleRole::BodyBold))
        .child(status)
        .child(
            // Overlay the minimap on the viewport's bottom-trailing corner.
            Expand::new().child(
                ZStack::new()
                    .alignment(Alignment::BOTTOM_TRAILING)
                    .child(Expand::new().child(scrollable))
                    .child(Padding::new(12.0, 12.0, 12.0, 12.0).child(minimap)),
            ),
        )
}

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — bastyde-scene showcase")
                .size(1500, 950)
                .root(|tree, _state| tree.add(build_root())),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde::core::WidgetTree;

    #[test]
    fn showcase_root_lays_out_without_panicking() {
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let _root = tree.add(build_root());
        tree.layout(SizeProposal::exact(1500.0, 950.0));
    }

    #[test]
    fn outer_scene_has_two_heavyweight_children() {
        // 1 ScrollArea (§4, wraps 3 cards as ScrollArea descendants)
        // + 1 inner SceneView (§7) = 2 direct heavyweight children.
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let view_id = tree.add(build_showcase_view());
        tree.layout(SizeProposal::exact(1500.0, 950.0));
        let kids = tree.children(view_id);
        assert_eq!(kids.len(), 2);
    }

    #[test]
    fn scene_extent_fits_the_default_window() {
        // Showcase claim: at zoom 1.0 the entire scene fits in
        // ~1500×950 minus header. Verify the math.
        let (w, h) = scene_extent();
        assert!(w <= 1500.0, "scene width {} must fit in 1500 window", w);
        assert!(h <= 870.0, "scene height {} must fit below ~80px header", h);
    }

    #[test]
    fn showcase_scroll_bars_appear_when_zoomed_content_overflows() {
        // The showcase starts at initial_zoom(1.4), so the 1490×780 scene
        // overflows a typical scene-area viewport — both AsNeeded bars should
        // be placed with real (non-zero) bounds at the viewport edges.
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let scrollable = build_showcase_view()
            .with_scroll_bars()
            .scroll_bar_mode(ScrollBarMode::Overlay)
            .vertical_policy(ScrollBarPolicy::AsNeeded)
            .horizontal_policy(ScrollBarPolicy::AsNeeded);
        let id = tree.add(scrollable);
        // A realistic scene-area size (window minus toolbar/title/status chrome).
        tree.layout(SizeProposal::exact(1480.0, 800.0));

        let kids = tree.children(id);
        assert_eq!(kids.len(), 3, "scene_view + v_bar + h_bar");
        let v = tree.bounds(kids[1]);
        let h = tree.bounds(kids[2]);
        assert!(
            v.width > 0.0 && v.height > 0.0,
            "vertical scroll bar should be visible, got {:?}",
            v
        );
        assert!(
            h.width > 0.0 && h.height > 0.0,
            "horizontal scroll bar should be visible, got {:?}",
            h
        );
        // Vertical bar hugs the trailing (right) edge; horizontal bar the bottom.
        assert!(
            (v.x + v.width - 1480.0).abs() < 0.5,
            "v bar at right edge, got x={} w={}",
            v.x,
            v.width
        );
        assert!(
            (h.y + h.height - 800.0).abs() < 0.5,
            "h bar at bottom edge, got y={} h={}",
            h.y,
            h.height
        );
    }
}
