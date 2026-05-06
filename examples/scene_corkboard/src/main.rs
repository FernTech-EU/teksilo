//! `scene-corkboard` — Phase 4 demo of `fern-scene`.
//!
//! Renders a 3×3 grid of story cards on a fern-scene `SceneView`. Each
//! card is a real heavyweight widget — `Panel { VStack { TextWidget,
//! TextWidget } }` — placed at a fixed scene-coordinate rectangle. The
//! cards sit on top of a **lightweight** background grid (`RectItem`)
//! and are stitched together by **lightweight** connector lines
//! (`PathItem`) tracing the reading-order through the story beats.
//!
//! Phase 4 mixes the two content tiers under one `SceneView`:
//!
//! - **Heavyweight** cards (real widgets, focus + keyboard + a11y).
//! - **Lightweight** items (RectItem cells, PathItem connectors)
//!   painted from `SceneView::paint` without arena overhead.
//! - **Same view transform** projects both tiers identically — pan /
//!   zoom / pinch keep the connector lines glued to the card edges.
//! - **Same spatial index** culls both tiers — off-screen connectors
//!   never enter the paint walk.
//!
//! Phase 1 + 2 + 3 exercises (still in effect):
//!
//! - `Scene::add_widget` round-trip (heavyweight widgets at scene
//!   rects).
//! - `SceneView` placement (each child at scene_rect under the view
//!   transform; identity at rest).
//! - Real interactivity — clicks on cards focus, keyboard navigation
//!   works between cards.
//! - **View transform** — pan / zoom / rotate as four animated
//!   `Signal<f32>`s composed into a `set_transform` scope on the
//!   viewport. The cards stay at their scene coordinates; the
//!   transform applies on top.
//! - **Trackpad two-finger pan** — `ScrollDelta::Pixels` events
//!   animate the pan signals via `Easing::EaseOut`. Trackpad
//!   momentum after release arrives as more `Pixels` events,
//!   producing inertial fling for free.
//! - **Mouse wheel pan** — `ScrollDelta::Lines` events scaled by
//!   `line_height` (default 16 px / notch).
//! - **Pinch-to-zoom** — OS trackpad pinch (`PinchPhase::Changed`)
//!   adjusts zoom + pan together so the scene point under the
//!   gesture center stays put.
//! - **Reduced motion** — at build time SceneView captures
//!   `BuildContext::prefers_reduced_motion()`; when set, scroll
//!   handlers `set` the pan signals directly instead of animating.
//! - **Spatial index + viewport culling** — every `Scene` carries a
//!   `GridHashIndex` (default cell size 256 px). `place_children`
//!   queries the visible scene region and collapses off-screen
//!   children to zero size, so layout / paint walks short-circuit
//!   on them. The 9-card demo is too small to demonstrate this
//!   visibly, but the same machinery scales to thousands of items;
//!   see `crates/fern-scene/src/view.rs::tests::off_screen_items_*`
//!   and `crates/fern-scene/src/index.rs::tests` for the
//!   correctness pins.
//!
//! Phase 6 will add drag-to-move, marquee select, and group-move on
//! the same demo.
//!
//! Run with: `cargo run -p scene-corkboard`

use fern_scene::{A11yGroup, A11yNode, PathItem, RectItem, Scene, SceneView};
use fern_ui::canvas::{Path, Point, Rect};
use fern_ui::prelude::*;
use fern_ui::widgets::{Panel, TextWidget, VStack};

const CARDS_PER_ROW: usize = 3;
const ROWS: usize = 3;
const CARD_WIDTH: f32 = 220.0;
const CARD_HEIGHT: f32 = 140.0;
const CARD_GAP: f32 = 24.0;
const SCENE_MARGIN: f32 = 32.0;

/// Story cards in reading order (top-leading to bottom-trailing).
const CARDS: [(&str, &str); 9] = [
    (
        "Act I — Opening",
        "An ordinary morning. The protagonist discovers a strange letter.",
    ),
    (
        "Inciting Incident",
        "The letter names a place that shouldn't exist.",
    ),
    (
        "Reluctant Departure",
        "After a brief argument, the protagonist sets out.",
    ),
    ("Crossing", "The journey begins. New companions, new costs."),
    ("Trials", "A series of escalating obstacles tests the team."),
    (
        "Midpoint Reversal",
        "What seemed like progress turns out to be a trap.",
    ),
    (
        "Dark Night",
        "The protagonist confronts what they've been avoiding.",
    ),
    ("Resolution", "A choice. Hard, but right."),
    ("Coda", "Months later. Quiet evidence of change."),
];

fn build_card(title: &str, body: &str) -> impl Widget + 'static {
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new_literal(title).style(TextStyleRole::BodyBold))
            .child(TextWidget::new_literal(body).style(TextStyleRole::Body)),
    )
}

fn card_rect(index: usize) -> Rect {
    let row = (index / CARDS_PER_ROW) as f32;
    let col = (index % CARDS_PER_ROW) as f32;
    let x = SCENE_MARGIN + col * (CARD_WIDTH + CARD_GAP);
    let y = SCENE_MARGIN + row * (CARD_HEIGHT + CARD_GAP);
    Rect::new(x, y, CARD_WIDTH, CARD_HEIGHT)
}

fn scene_size() -> (f32, f32) {
    let width = SCENE_MARGIN * 2.0
        + CARDS_PER_ROW as f32 * CARD_WIDTH
        + (CARDS_PER_ROW - 1) as f32 * CARD_GAP;
    let height = SCENE_MARGIN * 2.0 + ROWS as f32 * CARD_HEIGHT + (ROWS - 1) as f32 * CARD_GAP;
    (width, height)
}

fn build_corkboard() -> SceneView {
    let mut scene = Scene::new();

    // Background tile grid (Phase 4 lightweight tier). One RectItem
    // per cell — they share the spatial index with the cards and are
    // culled by viewport just like heavyweight children.
    let (scene_width, scene_height) = scene_size();
    let tile = 40.0_f32;
    let cols = (scene_width / tile).ceil() as i32;
    let rows = (scene_height / tile).ceil() as i32;
    let grid_color = Color::new(0.85, 0.85, 0.88, 0.6);
    for r in 0..rows {
        for c in 0..cols {
            // Draw only the cell border to keep the tile pattern airy.
            let cell = Rect::new(c as f32 * tile, r as f32 * tile, tile, tile);
            scene.add_item(RectItem::new(cell).stroke(grid_color, 1.0), Point::ZERO);
        }
    }

    // Phase 5b: declare three logical groups for the three Acts.
    // The screen reader announces "Act 1, Scene cards, 1 of 3" when
    // landing on a card, regardless of where the card is visually
    // placed in scene coordinates. Apps changing the visual layout
    // (drag-to-move in Phase 6) won't disturb the AT-shape.
    let act1 = scene.add_a11y_group(A11yGroup::builder().label("Act I — Setup"));
    let act2 = scene.add_a11y_group(A11yGroup::builder().label("Act II — Confrontation"));
    let act3 = scene.add_a11y_group(A11yGroup::builder().label("Act III — Resolution"));
    let acts = [act1, act2, act3];

    // Heavyweight cards. The cards themselves don't yet route
    // through the logical-tree (Phase 5b heavyweight grouping is
    // the deferred auto-graft work — see docs/fern-scene-a11y.md);
    // we still bookkeep their ids so the demo source documents the
    // intent.
    // Heavyweight cards. Auto-graft places each card under its
    // declared Act group: the screen reader announces "Act I —
    // Setup, contains: Card, Card, Card, connector 1 → 2,
    // connector 2 → 3" before reaching Act II. The framework
    // handles redirecting each card's WidgetId to the right
    // logical parent via `Widget::a11y_redirect_descendant`.
    let mut card_rects = Vec::with_capacity(CARDS.len());
    for (i, (title, body)) in CARDS.iter().enumerate() {
        let r = card_rect(i);
        let card_item = scene.add_widget(build_card(title, body), r);
        card_rects.push(r);
        let act_index = i / 3;
        scene.set_a11y_parent(
            A11yNode::Item(card_item),
            Some(A11yNode::Group(acts[act_index])),
        );
    }

    // Connector lines wiring each card to the next in reading order
    // (Phase 4 lightweight tier — PathItem). The path runs from the
    // trailing-mid of card N to the leading-mid of card N+1, with a
    // gentle horizontal-then-vertical bend so cards on different rows
    // get a step-shaped connector. Each connector also declares an
    // AT `FlowTo` relation between its source and target cards so
    // VoiceOver / NVDA users following data-flow get the right
    // direction.
    let connector_color = Color::new(0.40, 0.55, 0.85, 0.9);
    for (i, pair) in card_rects.windows(2).enumerate() {
        let a = pair[0];
        let b = pair[1];
        let from = Point::new(a.x + a.width, a.y + a.height * 0.5);
        let to = Point::new(b.x, b.y + b.height * 0.5);
        let mid_x = (from.x + to.x) * 0.5;
        let mut path = Path::new();
        path.move_to(from)
            .line_to(Point::new(mid_x, from.y))
            .line_to(Point::new(mid_x, to.y))
            .line_to(to);
        // AABB enclosing the connector — used by the spatial index
        // for culling. Padded by stroke half-width so partial
        // intersections aren't dropped at the viewport edge.
        let stroke_w = 2.0_f32;
        let pad = stroke_w * 0.5;
        let min_x = from.x.min(to.x).min(mid_x) - pad;
        let max_x = from.x.max(to.x).max(mid_x) + pad;
        let min_y = from.y.min(to.y) - pad;
        let max_y = from.y.max(to.y) + pad;
        let bounds = Rect::new(min_x, min_y, max_x - min_x, max_y - min_y);
        let connector_id = scene.add_item(
            PathItem::new(path, bounds)
                .stroke(connector_color, stroke_w)
                .access_label(format!("connector {} → {}", i + 1, i + 2)),
            Point::ZERO,
        );
        // Phase 5b: parent the connector under the act its source
        // card belongs to. Screen-reader users walking the AT tree
        // hear "Act I, contains: connector 1 → 2, connector 2 → 3"
        // before reaching Act II.
        let act_index = i / 3;
        scene.set_a11y_parent(
            A11yNode::Item(connector_id),
            Some(A11yNode::Group(acts[act_index])),
        );
    }

    SceneView::new(scene).default_size(scene_width, scene_height)
}

fn main() {
    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Scene Corkboard (Phase 5b: cards auto-grafted into Act groups)")
                .size(900, 600)
                .root(|tree, _state| tree.add(build_corkboard())),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_ui::core::WidgetTree;

    #[test]
    fn corkboard_lays_out_nine_cards_at_scene_coords() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let view_id = tree.add(build_corkboard());
        tree.layout(SizeProposal::exact(900.0, 600.0));

        let kids = tree.children(view_id);
        assert_eq!(kids.len(), CARDS.len(), "all 9 cards must materialise");

        // First card: row 0, col 0 → (32, 32) origin.
        let first = tree.bounds(kids[0]);
        assert_eq!(first.x, SCENE_MARGIN);
        assert_eq!(first.y, SCENE_MARGIN);
        assert_eq!(first.width, CARD_WIDTH);
        assert_eq!(first.height, CARD_HEIGHT);

        // Last card: row 2, col 2 → (32 + 2*(220+24), 32 + 2*(140+24)).
        let last = tree.bounds(kids[CARDS.len() - 1]);
        let expected_last_x = SCENE_MARGIN + 2.0 * (CARD_WIDTH + CARD_GAP);
        let expected_last_y = SCENE_MARGIN + 2.0 * (CARD_HEIGHT + CARD_GAP);
        assert_eq!(last.x, expected_last_x);
        assert_eq!(last.y, expected_last_y);
    }
}
