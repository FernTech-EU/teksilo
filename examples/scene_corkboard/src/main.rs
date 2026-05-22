//! `scene-corkboard` — `bastyde-scene` demo.
//!
//! Renders a grid of story cards on a bastyde-scene `SceneView`. Each card is
//! a real heavyweight widget — `Panel { VStack { TextWidget, TextWidget } }` —
//! placed at a fixed scene-coordinate rectangle. The cards sit on top of a
//! **lightweight** background grid (`RectItem`) and are stitched together by
//! **lightweight** connector lines (`PathItem`) tracing the reading order.
//!
//! This demo mixes the two content tiers under one `SceneView`:
//!
//! - **Heavyweight** cards (real widgets, focus + keyboard + a11y).
//! - **Lightweight** items (RectItem cells, PathItem connectors).
//! - **Same view transform** projects both tiers; pan / zoom keep the
//!   connectors glued to the card edges.
//! - **Same spatial index** culls both tiers.
//!
//! **Runtime mutation (the headline of this revision).** The toolbar's
//! **"Add Act"** button mutates the *live* scene from a handler:
//!
//! ```ignore
//! ctx.with_widget_mut::<SceneView>(view_id, BindingLevel::Rebuild, |view| {
//!     let scene = view.scene_mut();
//!     let act = scene.add_a11y_group(A11yGroup::builder().label(lit!("Act N")));
//!     scene.set_a11y_live(A11yNode::Group(act), Live::Polite);
//!     let card = scene.add_widget(build_card(...), rect);
//!     scene.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(act)));
//!     view.ensure_visible(rect, 40.0);
//! });
//! ```
//!
//! The framework reconciles both trees: the new card materialises into the
//! arena, the spatial index re-buckets, and — because the AccessKit tree is
//! *separate* from the visual scene — the new card is grafted under its new
//! Act group and a `Live::Polite` region announces the addition. View state
//! (pan / zoom) is app-owned via `bind_view_state`, so the viewport doesn't
//! reset; **"Reset View"** snaps it home.
//!
//! Earlier exercises (still in effect): `Scene::add_widget` round-trip, pan /
//! zoom / pinch gestures, trackpad + wheel pan, reduced-motion, spatial-index
//! culling, marquee box-select over the connectors, an over-band accent tag.
//!
//! Run with: `cargo run -p scene-corkboard`

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::Live;
use bastyde::canvas::{Path, Point, Rect};
use bastyde::core::BindingLevel;
use bastyde::prelude::*;
use bastyde::widgets::{Button, Expand, HStack, Panel, Spacer, TextWidget, Toolbar, VStack};
use bastyde_scene::{
    A11yGroup, A11yGroupId, A11yNode, ItemFlags, ItemId, PathItem, RectItem, Scene, SceneLayer,
    SceneSelectionMode, SceneView,
};

const CARDS_PER_ROW: usize = 3;
const ROWS: usize = 3;
const CARD_WIDTH: f32 = 220.0;
const CARD_HEIGHT: f32 = 140.0;
const CARD_GAP: f32 = 24.0;
const SCENE_MARGIN: f32 = 32.0;

/// Initial story cards in reading order (top-leading to bottom-trailing).
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
            .child(TextWidget::new(lit!(title)).style(TextStyleRole::BodyBold))
            .child(TextWidget::new(lit!(body)).style(TextStyleRole::Body)),
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

/// Add a lightweight step-shaped connector from `from_rect`'s trailing-mid to
/// `to_rect`'s leading-mid. Returns the connector's `ItemId` so the caller can
/// parent it into the right Act group. Shared by the initial build and the
/// runtime "Add Act" path.
fn add_connector(scene: &mut Scene, from_rect: Rect, to_rect: Rect, beat: usize) -> ItemId {
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
    // AABB enclosing the connector, padded by stroke half-width so partial
    // intersections aren't dropped at the viewport edge.
    let stroke_w = 2.0_f32;
    let pad = stroke_w * 0.5;
    let min_x = from.x.min(to.x).min(mid_x) - pad;
    let max_x = from.x.max(to.x).max(mid_x) + pad;
    let min_y = from.y.min(to.y) - pad;
    let max_y = from.y.max(to.y) + pad;
    let bounds = Rect::new(min_x, min_y, max_x - min_x, max_y - min_y);
    let connector_color = Color::new(0.40, 0.55, 0.85, 0.9);
    scene.add_item(
        PathItem::new(path, bounds)
            // Cosmetic stroke: constant device-pixel width at any zoom.
            .stroke_cosmetic(connector_color, stroke_w)
            .access_label(lit!(format!("connector to beat {beat}"))),
        Point::ZERO,
    )
}

/// Runtime-growable corkboard state, tracked across "Add Act" clicks.
struct CorkboardModel {
    /// Next grid slot for an appended card.
    next_index: usize,
    /// Act groups created so far (initial three + any runtime additions).
    acts: Vec<A11yGroupId>,
    /// The last card in reading order, for the connector + flow.
    last_card_rect: Rect,
}

/// Build the initial 9-card / 3-Act corkboard, returning the scene and the
/// runtime model seeded to continue from card 9.
fn build_initial_scene() -> (Scene, CorkboardModel) {
    let mut scene = Scene::new();

    // Background tile grid (lightweight tier) — one RectItem per cell.
    let (scene_width, scene_height) = scene_size();
    let tile = 40.0_f32;
    let cols = (scene_width / tile).ceil() as i32;
    let rows = (scene_height / tile).ceil() as i32;
    let grid_color = Color::new(0.85, 0.85, 0.88, 0.6);
    for r in 0..rows {
        for c in 0..cols {
            let cell = Rect::new(c as f32 * tile, r as f32 * tile, tile, tile);
            let id = scene.add_item(
                RectItem::new(cell).stroke_cosmetic(grid_color, 1.0),
                Point::ZERO,
            );
            // Decorative backdrop: keep it out of marquee box-select.
            scene.set_flag(id, ItemFlags::IS_SELECTABLE, false);
        }
    }

    // Three logical Act groups. The screen reader announces "Act I — Setup,
    // contains: …" before reaching Act II, regardless of visual placement.
    let act1 = scene.add_a11y_group(A11yGroup::builder().label(lit!("Act I — Setup")));
    let act2 = scene.add_a11y_group(A11yGroup::builder().label(lit!("Act II — Confrontation")));
    let act3 = scene.add_a11y_group(A11yGroup::builder().label(lit!("Act III — Resolution")));
    let acts = [act1, act2, act3];

    // Heavyweight cards, auto-grafted under their declared Act group.
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

    // Connector lines wiring each card to the next in reading order.
    for (i, pair) in card_rects.windows(2).enumerate() {
        let connector_id = add_connector(&mut scene, pair[0], pair[1], i + 2);
        let act_index = i / 3;
        scene.set_a11y_parent(
            A11yNode::Item(connector_id),
            Some(A11yNode::Group(acts[act_index])),
        );
    }

    // Over-band accent tag pinned to the first card's top-right corner —
    // raised to `SceneLayer::Over` so it paints on top of the heavyweight card.
    if let Some(&first) = card_rects.first() {
        let tag =
            RectItem::new(Rect::new(0.0, 0.0, 16.0, 16.0)).fill(Color::new(0.95, 0.55, 0.15, 0.95));
        let tag_id = scene.add_item(tag, Point::new(first.x + first.width - 12.0, first.y - 4.0));
        scene.set_layer(tag_id, SceneLayer::Over);
        scene.set_flag(tag_id, ItemFlags::IS_SELECTABLE, false);
    }

    let model = CorkboardModel {
        next_index: CARDS.len(),
        acts: acts.to_vec(),
        last_card_rect: card_rect(CARDS.len() - 1),
    };
    (scene, model)
}

/// Append a new Act: a fresh logical group (announced via a polite live
/// region), one card under it at the next grid slot, and a connector from the
/// previous last card. Returns the new card's scene rect for `ensure_visible`.
fn add_act(scene: &mut Scene, model: &mut CorkboardModel) -> Rect {
    let index = model.next_index;
    let r = card_rect(index);
    let act_number = model.acts.len() + 1;

    let group =
        scene.add_a11y_group(A11yGroup::builder().label(lit!(format!("Act {act_number}"))));
    // Live region: a screen reader announces the runtime addition.
    scene.set_a11y_live(A11yNode::Group(group), Live::Polite);

    let card = scene.add_widget(
        build_card(
            &format!("Act {act_number} — new beat"),
            "Added live via with_widget_mut → scene_mut().",
        ),
        r,
    );
    scene.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(group)));

    // Connector from the previous last card into this new beat.
    let connector = add_connector(scene, model.last_card_rect, r, index + 1);
    scene.set_a11y_parent(A11yNode::Item(connector), Some(A11yNode::Group(group)));

    model.acts.push(group);
    model.last_card_rect = r;
    model.next_index += 1;
    r
}

/// The toolbar: Add Act, Reset View, dark-mode toggle. Handlers capture the
/// SceneView's `WidgetId`, the shared model, and the app-owned view-state
/// signals.
fn build_toolbar(
    view_id: WidgetId,
    model: Rc<RefCell<CorkboardModel>>,
    pan_x: Signal<f32>,
    pan_y: Signal<f32>,
    zoom: Signal<f32>,
    rotation: Signal<f32>,
) -> impl Widget + 'static {
    let is_dark = Signal::new(false);
    Toolbar::new().child(
        HStack::new()
            .spacing(8.0)
            .child(Button::new(lit!("Add Act")).on_activate_fn(move |ctx| {
                let model = model.clone();
                // Reach the live SceneView from the handler and mutate its
                // scene; the framework reconciles visual + AccessKit trees.
                ctx.with_widget_mut::<SceneView>(view_id, BindingLevel::Rebuild, move |view| {
                    let rect = {
                        let mut m = model.borrow_mut();
                        add_act(view.scene_mut(), &mut m)
                    };
                    // View state is app-owned, so panning to the new card
                    // doesn't fight the rebuild.
                    view.ensure_visible(rect, 40.0);
                });
            }))
            .child(
                Button::new(lit!("Reset View")).on_activate_fn(move |_ctx| {
                    // Animate home rather than `set(...)`. A plain set does NOT
                    // cancel an in-flight pan animation (the scroll handler pans
                    // via `animate_to`), so a set mid-scroll-animation gets
                    // overridden on the next scheduler tick and Reset appears
                    // dead. `animate_to` installs a fresh target, overriding any
                    // running animation. (On a pristine view this is a no-op —
                    // there is nothing to reset.)
                    let dur = std::time::Duration::from_millis(220);
                    pan_x.animate_to(0.0, dur, bastyde::tokens::Easing::EaseOut);
                    pan_y.animate_to(0.0, dur, bastyde::tokens::Easing::EaseOut);
                    zoom.animate_to(1.0, dur, bastyde::tokens::Easing::EaseOut);
                    rotation.animate_to(0.0, dur, bastyde::tokens::Easing::EaseOut);
                }),
            )
            .child(Spacer::new())
            .child(
                Button::new(lit!("Toggle Dark Mode")).on_activate_fn(move |ctx| {
                    let next = !is_dark.get();
                    is_dark.set(next);
                    ctx.set_theme(if next {
                        bastyde::presets::intui::dark()
                    } else {
                        bastyde::presets::intui::light()
                    });
                }),
            ),
    )
}

fn main() {
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Scene Corkboard (Add Act mutates the live scene + AccessKit)")
                .size(900, 600)
                .root(|tree, _state| {
                    let (scene, model) = build_initial_scene();
                    let model = Rc::new(RefCell::new(model));

                    // App-owned view state, injected into the SceneView so it
                    // survives runtime mutation and "Reset View" can snap it.
                    let pan_x = Signal::new_animated(0.0);
                    let pan_y = Signal::new_animated(0.0);
                    let zoom = Signal::new_animated(1.0);
                    let rotation = Signal::new_animated(0.0);

                    let (scene_width, scene_height) = scene_size();
                    let view_id = tree.add(
                        SceneView::new(scene)
                            .selection_mode(SceneSelectionMode::Multi)
                            .default_size(scene_width, scene_height)
                            .bind_view_state(
                                pan_x.clone(),
                                pan_y.clone(),
                                zoom.clone(),
                                rotation.clone(),
                            ),
                    );

                    let toolbar =
                        build_toolbar(view_id, model, pan_x, pan_y, zoom, rotation);
                    tree.add(
                        VStack::new()
                            .child(toolbar)
                            .child(Expand::new().child_id(view_id)),
                    )
                }),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde::core::WidgetTree;

    /// Reproduce the real-app topology + the exact "Add Act" handler path:
    /// view wrapped in `VStack → Expand`, app-owned view state via
    /// `bind_view_state`, and the mutation driven through
    /// `ctx.with_widget_mut(view_id, Rebuild, |v| { add_act; ensure_visible })`.
    #[test]
    fn add_act_via_handler_materialises_card_in_real_topology() {
        let (scene, model) = build_initial_scene();
        let model = Rc::new(RefCell::new(model));
        let pan_x = Signal::new_animated(0.0);
        let pan_y = Signal::new_animated(0.0);
        let zoom = Signal::new_animated(1.0);
        let rotation = Signal::new_animated(0.0);
        let (scene_width, scene_height) = scene_size();

        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(SceneSelectionMode::Multi)
                .default_size(scene_width, scene_height)
                .bind_view_state(pan_x.clone(), pan_y.clone(), zoom.clone(), rotation.clone()),
        );
        // Wrap exactly like main(): VStack → Expand → SceneView.
        let _root = tree.add(VStack::new().child(Expand::new().child_id(view_id)));
        tree.layout(SizeProposal::exact(900.0, 600.0));
        let before = tree.children(view_id).len();

        // Drive the handler path.
        let mut noop = bastyde::core::window::NoopWindowOps;
        tree.run_with_event_context(&mut noop, |ctx| {
            let model = model.clone();
            ctx.with_widget_mut::<SceneView>(view_id, BindingLevel::Rebuild, move |view| {
                let rect = {
                    let mut m = model.borrow_mut();
                    add_act(view.scene_mut(), &mut m)
                };
                view.ensure_visible(rect, 40.0);
            });
        });

        assert!(
            tree.needs_redraw(),
            "Add Act handler must schedule a frame"
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert_eq!(
            tree.children(view_id).len(),
            before + 1,
            "Add Act must materialise the new card in the real (wrapped) topology"
        );
        let new_wid = *tree.children(view_id).last().unwrap();
        let b = tree.bounds(new_wid);
        // It should be placed at its scene rect (card_rect(9) = 32, 524) and not
        // culled — its top is within the ~560px-tall view under the toolbar.
        assert!(
            b.width > 0.0 && b.height > 0.0,
            "runtime card must be placed (non-zero) in the wrapped topology, got {b:?}"
        );
    }

    #[test]
    fn corkboard_lays_out_nine_cards_at_scene_coords() {
        let (scene, _model) = build_initial_scene();
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(900.0, 600.0));

        let kids = tree.children(view_id);
        assert_eq!(kids.len(), CARDS.len(), "all 9 cards must materialise");

        let first = tree.bounds(kids[0]);
        assert_eq!(first.x, SCENE_MARGIN);
        assert_eq!(first.y, SCENE_MARGIN);
        assert_eq!(first.width, CARD_WIDTH);
        assert_eq!(first.height, CARD_HEIGHT);

        // Last card: row 2, col 2.
        let last = tree.bounds(kids[CARDS.len() - 1]);
        assert_eq!(last.x, SCENE_MARGIN + 2.0 * (CARD_WIDTH + CARD_GAP));
        assert_eq!(last.y, SCENE_MARGIN + 2.0 * (CARD_HEIGHT + CARD_GAP));
    }

    #[test]
    fn add_act_appends_a_card_and_group() {
        let (scene, mut model) = build_initial_scene();
        let acts_before = model.acts.len();
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert_eq!(tree.children(view_id).len(), CARDS.len());

        // Mutate the live scene the way the "Add Act" button does, then rebuild.
        let new_rect = {
            let view = tree
                .widget_as_any_mut(view_id)
                .and_then(|a| a.downcast_mut::<SceneView>())
                .expect("view is a SceneView");
            add_act(view.scene_mut(), &mut model)
        };
        tree.layout(SizeProposal::exact(900.0, 600.0));

        assert_eq!(
            tree.children(view_id).len(),
            CARDS.len() + 1,
            "Add Act must materialise one new card"
        );
        assert_eq!(model.acts.len(), acts_before + 1, "a new Act group is created");
        assert!(
            tree.a11y_request_handle().get(),
            "Add Act must request an AccessKit re-walk"
        );

        // The new card sits at the next grid slot.
        assert_eq!(new_rect, card_rect(CARDS.len()));

        // The new card must be PLACED (non-zero bounds) — not just present in
        // the child list. card_rect(9) is (32, 524, 220, 140); with pan=0 / zoom=1
        // and a 900×600 viewport its top is in view, so it must not be culled.
        let new_wid = *tree.children(view_id).last().unwrap();
        let b = tree.bounds(new_wid);
        assert!(
            b.width > 0.0 && b.height > 0.0,
            "the runtime-added card must be placed at non-zero size, got {b:?}"
        );
    }
}
