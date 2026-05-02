//! `scene-corkboard` — Phase 1 demo of `fern-scene`.
//!
//! Renders a 3×3 grid of story cards on a fern-scene `SceneView`. Each
//! card is a real heavyweight widget — `Panel { VStack { TextWidget,
//! TextWidget } }` — placed at a fixed scene-coordinate rectangle.
//!
//! This Phase 1 example exercises:
//!
//! - `Scene::add_widget` round-trip (heavyweight widgets at scene
//!   rects).
//! - `SceneView` placement (parent-local origin = bounds.origin +
//!   scene_rect.origin under identity view transform).
//! - Real interactivity through fern-scene — clicks on card titles
//!   focus, keyboard navigation works between cards.
//!
//! Phase 2 will add pan/zoom (the cards stay where they are; the view
//! transforms). Phase 6 will add drag-to-move, marquee select, and
//! group-move.
//!
//! Run with: `cargo run -p scene-corkboard`

use fern_scene::{Scene, SceneView};
use fern_ui::canvas::Rect;
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
    ("Act I — Opening", "An ordinary morning. The protagonist discovers a strange letter."),
    ("Inciting Incident", "The letter names a place that shouldn't exist."),
    ("Reluctant Departure", "After a brief argument, the protagonist sets out."),
    ("Crossing", "The journey begins. New companions, new costs."),
    ("Trials", "A series of escalating obstacles tests the team."),
    ("Midpoint Reversal", "What seemed like progress turns out to be a trap."),
    ("Dark Night", "The protagonist confronts what they've been avoiding."),
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

fn build_corkboard() -> SceneView {
    let mut scene = Scene::new();
    for (i, (title, body)) in CARDS.iter().enumerate() {
        let row = (i / CARDS_PER_ROW) as f32;
        let col = (i % CARDS_PER_ROW) as f32;
        let x = SCENE_MARGIN + col * (CARD_WIDTH + CARD_GAP);
        let y = SCENE_MARGIN + row * (CARD_HEIGHT + CARD_GAP);
        scene.add_widget(build_card(title, body), Rect::new(x, y, CARD_WIDTH, CARD_HEIGHT));
    }

    // Total scene size for a sensible default viewport.
    let width = SCENE_MARGIN * 2.0 + CARDS_PER_ROW as f32 * CARD_WIDTH
        + (CARDS_PER_ROW - 1) as f32 * CARD_GAP;
    let height = SCENE_MARGIN * 2.0 + ROWS as f32 * CARD_HEIGHT + (ROWS - 1) as f32 * CARD_GAP;
    SceneView::new(scene).default_size(width, height)
}

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Scene Corkboard (Phase 1)")
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
