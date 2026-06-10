//! Scene tab — a compact `SceneView` showcasing both tiers: lightweight
//! paint-only items (RectItem / PathItem / TextItem / GroupItem) and a
//! heavyweight `Panel` card with a real `Button` placed at scene
//! coordinates. Pan with the scroll wheel, drag the "drag me" rects.
//! Cannibalized from the `scene-showcase` example. Lives in the
//! `bastyde-scene` crate.

use bastyde::canvas::{Path, Point, Rect};
use bastyde::prelude::*;
use bastyde::widgets::{Button, Divider, FixedSize, Panel, TextWidget, VStack};
use bastyde_scene::{
    GroupItem, PathItem, RectItem, Scene, SceneSelectionMode, SceneView, TextItem,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_scene_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_scene_refs())
}

fn ink() -> Color {
    Color::new(0.10, 0.10, 0.12, 1.0)
}

fn build_scene_view() -> SceneView {
    let mut scene = Scene::new();

    // ── Lightweight tier: a labelled GroupItem holding colored tiles
    //    and a cosmetic-stroke zigzag path.
    scene.add_item(
        GroupItem::new(Rect::new(20.0, 20.0, 250.0, 160.0))
            .label(lit!("Lightweight items"))
            .show_label(true)
            .label_inset(10.0, 5.0)
            .label_color(ink())
            .fill(Color::new(0.97, 0.97, 1.0, 1.0))
            .stroke(Color::new(0.55, 0.55, 0.65, 1.0), 1.5)
            .corner_radius(8.0),
        Point::ZERO,
    );

    let palette = [
        Color::new(0.95, 0.55, 0.55, 0.9),
        Color::new(0.55, 0.70, 0.95, 0.9),
        Color::new(0.55, 0.85, 0.65, 0.9),
        Color::new(0.95, 0.85, 0.55, 0.9),
    ];
    for (i, c) in palette.iter().enumerate() {
        let rect = Rect::new(36.0 + i as f32 * 54.0, 64.0, 42.0, 42.0);
        scene.add_item(
            RectItem::new(rect)
                .fill(*c)
                .stroke(ink(), 1.0)
                .access_label(lit!(format!("tile {}", i + 1))),
            Point::ZERO,
        );
    }

    let mut zigzag = Path::new();
    zigzag.move_to(Point::new(40.0, 140.0));
    zigzag.line_to(Point::new(85.0, 158.0));
    zigzag.line_to(Point::new(130.0, 134.0));
    zigzag.line_to(Point::new(175.0, 158.0));
    scene.add_item(
        PathItem::new(zigzag, Rect::new(38.0, 132.0, 140.0, 28.0))
            .stroke_cosmetic(Color::new(0.80, 0.60, 0.90, 1.0), 3.0)
            .access_label(lit!("decorative zigzag")),
        Point::ZERO,
    );

    // ── Draggable tier: rects opt into dragging; each carries a child
    //    TextItem label that cascades along when the rect moves.
    let drag_colors = [
        Color::new(0.55, 0.70, 0.95, 0.9),
        Color::new(0.55, 0.85, 0.65, 0.9),
    ];
    for (i, c) in drag_colors.iter().enumerate() {
        let rect = Rect::new(310.0 + i as f32 * 110.0, 30.0, 90.0, 64.0);
        let parent = scene.add_item(
            RectItem::new(rect)
                .fill(*c)
                .stroke(ink(), 1.5)
                .draggable(true)
                .access_label(lit!(format!("draggable {}", i + 1))),
            Point::ZERO,
        );
        let label = scene.add_item(
            TextItem::new(
                lit!("drag me"),
                Rect::new(rect.x + 8.0, rect.y + 22.0, rect.width - 16.0, 24.0),
            )
            .color(ink()),
            Point::ZERO,
        );
        scene.set_item_parent(label, Some(parent));
    }

    // ── Heavyweight tier: a real Widget (Panel + Button) placed at a
    //    scene rect — full focus / keyboard / a11y survive embedding.
    scene.add_widget(
        Panel::new().child(
            VStack::new()
                .spacing(6.0)
                .child(TextWidget::new(lit!("Heavyweight card")).style(TextStyleRole::BodyBold))
                .child(
                    TextWidget::new(lit!("A real Button at scene coordinates."))
                        .style(TextStyleRole::Small),
                )
                .child(Button::new(lit!("Click me")).on_activate_fn(|_| {
                    println!("[widget-catalog] scene card button clicked");
                })),
        ),
        Rect::new(300.0, 110.0, 240.0, 110.0),
    );

    SceneView::new(scene)
        .selection_mode(SceneSelectionMode::Multi)
        .default_size(560.0, 360.0)
}

fn sized_scene() -> FixedSize {
    FixedSize::new()
        .bind_width(560.0_f32)
        .bind_height(360.0_f32)
        .child(build_scene_view())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let scene = section(
        ctx,
        lit!("SceneView (pan / zoom · two tiers)"),
        sized_scene(),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(scene),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // The scene is assembled imperatively (Scene::add_item / add_widget
    // return ItemIds, and items carry closures) — pre-build and splice.
    let scene_id = ctx.add(sized_scene());

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_scene_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_scene_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("SceneView (pan / zoom · two tiers)")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ scene_id }
            }
        }
    )
}
