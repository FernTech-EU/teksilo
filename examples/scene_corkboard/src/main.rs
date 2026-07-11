// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `scene-corkboard` — `bastyde-scene` multi-view + delegate demo.
//!
//! **Two `SceneView` panes share one `SceneModel`.** A story corkboard is shown
//! in a wide "editor" pane and a zoomed-out "overview" pane side by side; both
//! render the *same* scene through the same handle, each with its own camera.
//!
//! Cards are stored as `CardData` **payloads** (`SceneModel::add_widget_item`),
//! not widget instances — so each pane materialises its **own** card widgets via
//! a per-view `delegate_typed::<CardData>`. Mutating the shared model once
//! ("Add Act") reconciles **both** panes. A shared [`SceneSelection`] (passed to
//! both views via `selection_model`) makes selection reactive across panes:
//! clicking a card in either pane highlights it in both, with **no rebuild** —
//! each card's border binds the shared selection signal.
//!
//! Two content tiers coexist under each pane's view transform:
//! - **Heavyweight** cards (real widgets, focus + keyboard + a11y), one instance
//!   per pane, built by that pane's delegate from the shared `CardData`.
//! - **Lightweight** items (RectItem grid, PathItem connectors, an Over-band
//!   accent tag) — shared automatically; no per-view instance needed.
//!
//! **Runtime mutation.** "Add Act" calls `model.add_widget_item(..)` directly
//! from the handler (every `SceneModel` mutator is `&self` — no
//! `with_widget_mut` needed for the mutation); both panes rebuild via the shared
//! change signal, and — because the AccessKit tree is *separate* from the visual
//! scene — the new card is grafted under a fresh Act group announced by a
//! `Live::Polite` region. Each pane's camera is app-owned via `bind_view_state`,
//! so the per-pane "Reset" buttons snap only their own viewport home.
//!
//! **List-driven pins (`SceneListAdapter`).** A small `ListModel<PinTag>` backs
//! colour-coded corner "pins" on the cards, kept in sync with the shared scene
//! by a `SceneListAdapter` — the scene-tier counterpart of `ListView`'s
//! automatic data-to-widget sync, but for lightweight items. The adapter is
//! stored inside `CorkboardModel` (alongside the runtime Act-growth state) so
//! it stays alive for the app's lifetime; dropping it would stop the sync.
//! "Pin card" / "Unpin last" push/remove rows on `pins` — each reconciles the
//! *whole* pin set (the adapter's insert/remove policy); "Recolour pins" calls
//! `ListModel::set` per row instead, exercising the adapter's single-row
//! `ItemUpdated` rebuild path. Both panes show the same pins — one adapter,
//! one shared `SceneModel`.
//!
//! Run with: `cargo run -p scene_corkboard`

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::Live;
use bastyde::canvas::{Path, Point, Rect};
use bastyde::core::BindingLevel;
use bastyde::data::ListModel;
use bastyde::prelude::*;
use bastyde::widgets::{Button, Expand, HStack, Panel, Spacer, TextWidget, Toolbar, VStack};
use bastyde_scene::{
    A11yGroup, A11yGroupId, A11yNode, ItemFlags, ItemId, PathItem, RectItem, SceneItem, SceneLayer,
    SceneListAdapter, SceneModel, SceneSelection, SceneSelectionMode, SceneView,
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

/// The per-card data stored in the shared [`SceneModel`]. Each pane's delegate
/// builds its own card widget from a borrow of this — so one logical card can
/// be rendered (independently) in any number of views.
#[derive(Clone)]
struct CardData {
    title: String,
    body: String,
}

/// Build a card widget for one pane from shared `CardData`. The border binds the
/// shared selection signal (reactive — selecting in one pane repaints the border
/// in every pane, with no rebuild), and a tap toggles the shared selection.
fn build_card(card: &CardData, selection: SceneSelection, id: ItemId) -> Box<dyn Widget> {
    let border = selection.selection_signal().map(move |sel| {
        if sel.contains(&id) {
            Color::new(0.40, 0.55, 0.85, 1.0) // selected: accent
        } else {
            Color::new(0.80, 0.80, 0.85, 0.4) // idle: faint
        }
    });
    let tap_selection = selection.clone();
    Box::new(
        Panel::new()
            .border_color(border)
            .border_width(2.0)
            .child(
                VStack::new()
                    .spacing(8.0)
                    .child(TextWidget::new(lit!(card.title.clone())).style(TextStyleRole::BodyBold))
                    .child(TextWidget::new(lit!(card.body.clone())).style(TextStyleRole::Body)),
            )
            .on_tap(move |_ev, _ctx| {
                tap_selection.toggle(id);
            }),
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
/// runtime "Add Act" path. Connectors are lightweight, so both panes share the
/// one instance — no per-view delegate involved.
fn add_connector(model: &SceneModel, from_rect: Rect, to_rect: Rect, beat: usize) -> ItemId {
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
    model.add_item(
        PathItem::new(path, bounds)
            // Cosmetic stroke: constant device-pixel width at any zoom.
            .stroke_cosmetic(connector_color, stroke_w)
            .access_label(lit!(format!("connector to beat {beat}"))),
        Point::ZERO,
    )
}

// ---------------------------------------------------------------------------
// List-driven pins — `ListModel<PinTag>` kept in sync via `SceneListAdapter`.
// ---------------------------------------------------------------------------

const PIN_SIZE: f32 = 14.0;

fn pin_palette() -> [Color; 4] {
    [
        Color::new(0.90, 0.30, 0.30, 0.95), // red
        Color::new(0.95, 0.75, 0.20, 0.95), // amber
        Color::new(0.30, 0.75, 0.45, 0.95), // green
        Color::new(0.35, 0.55, 0.90, 0.95), // blue
    ]
}

fn pin_ink() -> Color {
    Color::new(0.15, 0.15, 0.18, 1.0)
}

/// One row of the corkboard's pin `ListModel`: which card it decorates, and a
/// palette index (rather than a `Color` directly) so "recolour" is a plain
/// integer bump — no colour comparison needed to find "the next one".
#[derive(Debug, Clone, Copy)]
struct PinTag {
    card_index: usize,
    color_index: usize,
}

fn pin_color(tag: &PinTag) -> Color {
    let palette = pin_palette();
    palette[tag.color_index % palette.len()]
}

/// A pin's local rect: a small rounded square in the card's top-leading
/// corner (the top-trailing corner already hosts the static "over-band"
/// accent tag on card 0). Positions are absolute scene coordinates —
/// `SceneListAdapter` always inserts its delegate's item at `Point::ZERO`, so
/// the delegate itself computes each row's placement (see the module docs on
/// `SceneListAdapter`).
fn pin_rect(card_index: usize) -> Rect {
    let card = card_rect(card_index);
    Rect::new(card.x + 6.0, card.y + 6.0, PIN_SIZE, PIN_SIZE)
}

/// The `SceneListAdapter` delegate: one `RectItem` "pin" per `PinTag` row,
/// corner-radius'd to a circle. Re-run for every row on an insert/remove
/// (whole-set rebuild) and for the single changed row on a `ListModel::set`
/// (targeted `ItemUpdated` rebuild) — see the module docs on
/// `SceneListAdapter`'s reconciliation policy.
fn build_pin_item(tag: &PinTag, _index: usize) -> Box<dyn SceneItem> {
    Box::new(
        RectItem::new(pin_rect(tag.card_index))
            .fill(pin_color(tag))
            .corner_radius(PIN_SIZE * 0.5)
            .stroke(pin_ink(), 1.0)
            .access_label(lit!(format!("pin on card {}", tag.card_index + 1))),
    )
}

/// Runtime-growable corkboard state, tracked across "Add Act" clicks.
struct CorkboardModel {
    /// Next grid slot for an appended card.
    next_index: usize,
    /// Act groups created so far (initial three + any runtime additions).
    acts: Vec<A11yGroupId>,
    /// The last card in reading order (item + rect), for the connector + flow.
    last_card_item: Option<(ItemId, Rect)>,
    /// Backing list model for the pin markers — the toolbar's Pin/Unpin/
    /// Recolour buttons mutate this directly; `pins_adapter` reconciles it
    /// into the scene automatically.
    pins: ListModel<PinTag>,
    /// Kept alive purely so the pin sync keeps running — dropping it would
    /// stop `pins`' changes from reaching the scene (see the module docs).
    /// Never read: its entire job is done by staying alive, not by its value.
    #[allow(dead_code)]
    pins_adapter: SceneListAdapter<PinTag>,
}

/// Build the initial 9-card / 3-Act corkboard, returning a shared [`SceneModel`]
/// handle and the runtime model seeded to continue from card 9.
fn build_initial_scene() -> (SceneModel, CorkboardModel) {
    let model = SceneModel::new();

    // Background tile grid (lightweight tier) — one RectItem per cell.
    let (scene_width, scene_height) = scene_size();
    let tile = 40.0_f32;
    let cols = (scene_width / tile).ceil() as i32;
    let rows = (scene_height / tile).ceil() as i32;
    let grid_color = Color::new(0.85, 0.85, 0.88, 0.6);
    for r in 0..rows {
        for c in 0..cols {
            let cell = Rect::new(c as f32 * tile, r as f32 * tile, tile, tile);
            let id = model.add_item(
                RectItem::new(cell).stroke_cosmetic(grid_color, 1.0),
                Point::ZERO,
            );
            // Decorative backdrop: keep it out of marquee box-select.
            model.set_flag(id, ItemFlags::IS_SELECTABLE, false);
        }
    }

    // Three logical Act groups. The screen reader announces "Act I — Setup,
    // contains: …" before reaching Act II, regardless of visual placement.
    let act1 = model.add_a11y_group(A11yGroup::builder().label(lit!("Act I — Setup")));
    let act2 = model.add_a11y_group(A11yGroup::builder().label(lit!("Act II — Confrontation")));
    let act3 = model.add_a11y_group(A11yGroup::builder().label(lit!("Act III — Resolution")));
    let acts = [act1, act2, act3];

    // Heavyweight cards stored as `CardData` payloads, auto-grafted under their
    // declared Act group. Each pane's delegate builds its own widget per card.
    let mut last: Option<(ItemId, Rect)> = None;
    let mut prev: Option<(ItemId, Rect)> = None;
    for (i, (title, body)) in CARDS.iter().enumerate() {
        let r = card_rect(i);
        let card_item = model.add_widget_item(
            CardData {
                title: (*title).to_string(),
                body: (*body).to_string(),
            },
            r,
        );
        let act_index = i / 3;
        model.set_a11y_parent(
            A11yNode::Item(card_item),
            Some(A11yNode::Group(acts[act_index])),
        );

        // Connector from the previous card to this one, parented in this card's
        // Act group.
        if let Some((_, prev_rect)) = prev {
            let connector_id = add_connector(&model, prev_rect, r, i + 1);
            model.set_a11y_parent(
                A11yNode::Item(connector_id),
                Some(A11yNode::Group(acts[act_index])),
            );
        }
        prev = Some((card_item, r));
        last = Some((card_item, r));
    }

    // Over-band accent tag pinned to the first card's top-right corner —
    // raised to `SceneLayer::Over` so it paints on top of the heavyweight card.
    let first_rect = card_rect(0);
    let tag =
        RectItem::new(Rect::new(0.0, 0.0, 16.0, 16.0)).fill(Color::new(0.95, 0.55, 0.15, 0.95));
    let tag_id = model.add_item(
        tag,
        Point::new(first_rect.x + first_rect.width - 12.0, first_rect.y - 4.0),
    );
    model.set_layer(tag_id, SceneLayer::Over);
    model.set_flag(tag_id, ItemFlags::IS_SELECTABLE, false);

    // Colour-coded pins: a `ListModel<PinTag>` synced into the scene via
    // `SceneListAdapter` — the list-driven counterpart to every other item in
    // this scene, which is added by hand through `SceneModel`. Seed two pins
    // on the first two cards; the toolbar grows/shrinks/recolours `pins` from
    // here on, and the adapter keeps the scene in step automatically.
    let pins: ListModel<PinTag> = ListModel::from_vec(vec![
        PinTag {
            card_index: 0,
            color_index: 0,
        },
        PinTag {
            card_index: 1,
            color_index: 1,
        },
    ]);
    let pins_adapter = SceneListAdapter::from_model(&pins, model.clone(), build_pin_item);

    let cork = CorkboardModel {
        next_index: CARDS.len(),
        acts: acts.to_vec(),
        last_card_item: last,
        pins,
        pins_adapter,
    };
    (model, cork)
}

/// Append a new Act: a fresh logical group (announced via a polite live
/// region), one card under it at the next grid slot, and a connector from the
/// previous last card. Returns the new card's `(ItemId, Rect)`. Drives the
/// shared model directly (`&self`), so every attached pane reconciles.
fn add_act(model: &SceneModel, state: &mut CorkboardModel) -> (ItemId, Rect) {
    let index = state.next_index;
    let r = card_rect(index);
    let act_number = state.acts.len() + 1;

    let group = model.add_a11y_group(A11yGroup::builder().label(lit!(format!("Act {act_number}"))));
    // Live region: a screen reader announces the runtime addition.
    model.set_a11y_live(A11yNode::Group(group), Live::Polite);

    let card = model.add_widget_item(
        CardData {
            title: format!("Act {act_number} — new beat"),
            body: "Added live via the shared SceneModel; both panes show it.".to_string(),
        },
        r,
    );
    model.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(group)));

    // Connector from the previous last card into this new beat.
    if let Some((_, prev_rect)) = state.last_card_item {
        let connector = add_connector(model, prev_rect, r, index + 1);
        model.set_a11y_parent(A11yNode::Item(connector), Some(A11yNode::Group(group)));
    }

    state.acts.push(group);
    state.last_card_item = Some((card, r));
    state.next_index += 1;
    (card, r)
}

/// Per-pane camera signals (pan x/y, zoom, rotation), app-owned so the toolbar
/// can reset each pane independently.
#[derive(Clone)]
struct Camera {
    pan_x: Signal<f32>,
    pan_y: Signal<f32>,
    zoom: Signal<f32>,
    rotation: Signal<f32>,
}

impl Camera {
    fn new(zoom: f32) -> Self {
        Self {
            pan_x: Signal::new_animated(0.0),
            pan_y: Signal::new_animated(0.0),
            zoom: Signal::new_animated(zoom),
            rotation: Signal::new_animated(0.0),
        }
    }

    /// Animate this camera home (to the given resting zoom).
    fn reset(&self, rest_zoom: f32) {
        let dur = std::time::Duration::from_millis(220);
        // Animate rather than `set`: a plain set does NOT cancel an in-flight
        // pan animation (the scroll handler pans via `animate_to`), so a reset
        // mid-scroll would be overridden on the next scheduler tick.
        self.pan_x
            .animate_to(0.0, dur, bastyde::tokens::Easing::EaseOut);
        self.pan_y
            .animate_to(0.0, dur, bastyde::tokens::Easing::EaseOut);
        self.zoom
            .animate_to(rest_zoom, dur, bastyde::tokens::Easing::EaseOut);
        self.rotation
            .animate_to(0.0, dur, bastyde::tokens::Easing::EaseOut);
    }
}

/// Configure a pane over the shared model: same content, own camera + delegate,
/// shared selection (so both panes highlight together).
fn build_pane(model: &SceneModel, selection: &SceneSelection, camera: &Camera) -> SceneView {
    let delegate_selection = selection.clone();
    let (sw, sh) = scene_size();
    SceneView::with_model(model.clone())
        .selection_mode(SceneSelectionMode::Multi)
        .selection_model(selection.clone())
        .delegate_typed::<CardData>(move |card, id| {
            build_card(card, delegate_selection.clone(), id)
        })
        .default_size(sw, sh)
        .view_state(
            camera.pan_x.clone(),
            camera.pan_y.clone(),
            camera.zoom.clone(),
            camera.rotation.clone(),
        )
}

/// The toolbar: Add Act, pin controls, per-pane Reset, dark-mode toggle.
/// Captures the shared model (mutated directly) + main pane id (for
/// `ensure_visible`) + both cameras. Each button gets its own `cork` clone —
/// `Rc<RefCell<_>>` is cheap to clone and every closure needs to own its
/// capture.
fn build_toolbar(
    main_view_id: WidgetId,
    model: SceneModel,
    cork: Rc<RefCell<CorkboardModel>>,
    main_cam: Camera,
    overview_cam: Camera,
) -> impl Widget + 'static {
    let pin_cork = cork.clone();
    let unpin_cork = cork.clone();
    let recolor_cork = cork.clone();
    Toolbar::new().child(
        HStack::new()
            .spacing(8.0)
            .child(Button::new(lit!("Add Act")).on_activate_fn(move |ctx| {
                // Mutate the shared model directly — every mutator is `&self`,
                // so both panes' observers fire and both rebuild. No
                // `with_widget_mut` needed for the mutation itself.
                let (_card, rect) = {
                    let mut state = cork.borrow_mut();
                    add_act(&model, &mut state)
                };
                // Pan the *main* pane to the new card. ensure_visible animates
                // the (app-owned) main camera; the overview pane is untouched.
                ctx.with_widget_mut::<SceneView>(
                    main_view_id,
                    BindingLevel::Relayout,
                    move |view| {
                        view.ensure_visible(rect, 40.0);
                    },
                );
            }))
            .child(Button::new(lit!("Pin card")).on_activate_fn(move |_ctx| {
                // `ListModel::push` — the SceneListAdapter rebuilds the whole
                // pin set (insert/remove reconciliation policy) and every pane
                // (sharing the one SceneModel) picks up the new pin.
                let state = pin_cork.borrow();
                let count = state.pins.len();
                state.pins.push(PinTag {
                    card_index: count % CARDS.len(),
                    color_index: count,
                });
            }))
            .child(Button::new(lit!("Unpin last")).on_activate_fn(move |_ctx| {
                let state = unpin_cork.borrow();
                let len = state.pins.len();
                if len > 0 {
                    state.pins.remove(len - 1);
                }
            }))
            .child(
                Button::new(lit!("Recolour pins")).on_activate_fn(move |_ctx| {
                    // `ListModel::set` per row — the adapter's targeted
                    // `ItemUpdated` path rebuilds only that one pin, not the set.
                    let state = recolor_cork.borrow();
                    for i in 0..state.pins.len() {
                        if let Some(next) = state.pins.with_item(i, |tag| PinTag {
                            card_index: tag.card_index,
                            color_index: tag.color_index + 1,
                        }) {
                            state.pins.set(i, next);
                        }
                    }
                }),
            )
            .child({
                let cam = main_cam.clone();
                Button::new(lit!("Reset Main")).on_activate_fn(move |_ctx| cam.reset(1.0))
            })
            .child({
                let cam = overview_cam.clone();
                Button::new(lit!("Reset Overview")).on_activate_fn(move |_ctx| cam.reset(0.5))
            })
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
                .title("Bastyde — Scene Corkboard (two panes, one shared SceneModel)")
                .size(1300, 640)
                .root(|tree, _state| {
                    let (model, cork) = build_initial_scene();
                    let cork = Rc::new(RefCell::new(cork));

                    // One shared selection across both panes: clicking a card in
                    // either pane highlights it in both.
                    let selection = SceneSelection::new(SceneSelectionMode::Multi);

                    // Independent cameras: the editor pane at 1×, the overview
                    // pane zoomed out to 0.5×.
                    let main_cam = Camera::new(1.0);
                    let overview_cam = Camera::new(0.5);

                    let main_id = tree.add(build_pane(&model, &selection, &main_cam));
                    let overview_id = tree.add(
                        build_pane(&model, &selection, &overview_cam)
                            .nested_a11y(true)
                            .a11y_label(lit!("Overview pane")),
                    );

                    let toolbar =
                        build_toolbar(main_id, model.clone(), cork, main_cam, overview_cam);

                    tree.add(
                        VStack::new().child(toolbar).child(
                            // Wrap the pane row in an `Expand` so it fills the
                            // VStack's remaining height (a bare HStack child is
                            // flex-0 and would collapse to ~0 px, culling both
                            // panes — only the toolbar would show).
                            Expand::new().child(
                                HStack::new()
                                    // Editor pane gets 2/3, overview 1/3.
                                    .child(Expand::new().flex(2.0).child_id(main_id))
                                    .child(Expand::new().child_id(overview_id)),
                            ),
                        ),
                    )
                }),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde::core::WidgetTree;
    use std::collections::BTreeSet;

    /// A `WidgetTree` + two panes over one shared model, the way `main()` wires
    /// them. Returns `(tree, model, selection, main_id, overview_id)`.
    fn two_pane_tree() -> (WidgetTree, SceneModel, SceneSelection, WidgetId, WidgetId) {
        let (model, _cork) = build_initial_scene();
        let selection = SceneSelection::new(SceneSelectionMode::Multi);
        let main_cam = Camera::new(1.0);
        let overview_cam = Camera::new(0.5);
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let main_id = tree.add(build_pane(&model, &selection, &main_cam));
        let overview_id = tree.add(build_pane(&model, &selection, &overview_cam));
        let _root = tree.add(
            VStack::new().child(
                HStack::new()
                    .child(Expand::new().child_id(main_id))
                    .child(Expand::new().child_id(overview_id)),
            ),
        );
        tree.layout(SizeProposal::exact(1200.0, 600.0));
        (tree, model, selection, main_id, overview_id)
    }

    #[test]
    fn both_panes_materialise_every_card_via_their_own_delegate() {
        let (tree, _model, _sel, main_id, overview_id) = two_pane_tree();
        assert_eq!(
            tree.children(main_id).len(),
            CARDS.len(),
            "main pane builds its own 9 card widgets"
        );
        assert_eq!(
            tree.children(overview_id).len(),
            CARDS.len(),
            "overview pane builds its OWN 9 card widgets from the same payloads"
        );
        // Distinct arenas → distinct WidgetIds for the same logical cards.
        let main_kids: BTreeSet<_> = tree.children(main_id).into_iter().collect();
        let ov_kids: BTreeSet<_> = tree.children(overview_id).into_iter().collect();
        assert!(
            main_kids.is_disjoint(&ov_kids),
            "the two panes own independent widget instances"
        );
    }

    #[test]
    fn add_act_via_shared_model_materialises_in_both_panes() {
        let (mut tree, model, _sel, main_id, overview_id) = two_pane_tree();
        let before_main = tree.children(main_id).len();
        let before_ov = tree.children(overview_id).len();

        // Mutate the shared model directly (the way the "Add Act" button does).
        let pins: ListModel<PinTag> = ListModel::from_vec(vec![]);
        let pins_adapter = SceneListAdapter::from_model(&pins, model.clone(), build_pin_item);
        let mut cork = CorkboardModel {
            next_index: CARDS.len(),
            acts: vec![],
            last_card_item: Some((model.ids()[0], card_rect(CARDS.len() - 1))),
            pins,
            pins_adapter,
        };
        let (_card, _rect) = add_act(&model, &mut cork);
        tree.layout(SizeProposal::exact(1200.0, 600.0));

        assert_eq!(
            tree.children(main_id).len(),
            before_main + 1,
            "main pane materialises the new card"
        );
        assert_eq!(
            tree.children(overview_id).len(),
            before_ov + 1,
            "overview pane materialises the new card too (shared model)"
        );
    }

    #[test]
    fn corkboard_lays_out_nine_cards_at_scene_coords() {
        let (model, _cork) = build_initial_scene();
        let selection = SceneSelection::new(SceneSelectionMode::Multi);
        let camera = Camera::new(1.0);
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let view_id = tree.add(build_pane(&model, &selection, &camera));
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

    /// Regression for "empty panes, only the toolbar shows": the two-pane row
    /// must fill the VStack's remaining height (via an `Expand`), or both panes
    /// collapse to ~0 px and every card is culled. Mirrors `main()`'s layout
    /// and asserts both panes *place* their first card at non-zero size — a
    /// child-count check alone (cards materialise regardless of viewport size)
    /// would not catch this.
    #[test]
    fn app_layout_places_cards_in_both_panes_with_nonzero_size() {
        let (model, _cork) = build_initial_scene();
        let selection = SceneSelection::new(SceneSelectionMode::Multi);
        let main_cam = Camera::new(1.0);
        let overview_cam = Camera::new(0.5);
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let main_id = tree.add(build_pane(&model, &selection, &main_cam));
        let overview_id = tree.add(build_pane(&model, &selection, &overview_cam));
        // Exactly main()'s structure: VStack { toolbar, Expand { HStack { … } } }.
        tree.add(
            VStack::new().child(TextWidget::new(lit!("toolbar"))).child(
                Expand::new().child(
                    HStack::new()
                        .child(Expand::new().flex(2.0).child_id(main_id))
                        .child(Expand::new().child_id(overview_id)),
                ),
            ),
        );
        tree.layout(SizeProposal::exact(1300.0, 640.0));

        let main_card = tree.bounds(tree.children(main_id)[0]);
        let ov_card = tree.bounds(tree.children(overview_id)[0]);
        assert!(
            main_card.width > 0.0 && main_card.height > 0.0,
            "main pane must place its card at non-zero size, got {main_card:?}"
        );
        assert!(
            ov_card.width > 0.0 && ov_card.height > 0.0,
            "overview pane must place its card at non-zero size, got {ov_card:?}"
        );
    }

    #[test]
    fn add_act_appends_a_card_and_group_and_requests_at_rewalk() {
        let (model, mut cork) = build_initial_scene();
        let acts_before = cork.acts.len();
        let selection = SceneSelection::new(SceneSelectionMode::Multi);
        let camera = Camera::new(1.0);
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let view_id = tree.add(build_pane(&model, &selection, &camera));
        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert_eq!(tree.children(view_id).len(), CARDS.len());

        // Mutate the shared model directly, then rebuild.
        let (_card, new_rect) = add_act(&model, &mut cork);
        tree.layout(SizeProposal::exact(900.0, 600.0));

        assert_eq!(
            tree.children(view_id).len(),
            CARDS.len() + 1,
            "Add Act must materialise one new card"
        );
        assert_eq!(
            cork.acts.len(),
            acts_before + 1,
            "a new Act group is created"
        );
        assert!(
            tree.a11y_request_handle().get(),
            "Add Act must request an AccessKit re-walk"
        );

        // The new card sits at the next grid slot and is placed (non-zero).
        assert_eq!(new_rect, card_rect(CARDS.len()));
        let new_wid = *tree.children(view_id).last().unwrap();
        let b = tree.bounds(new_wid);
        assert!(
            b.width > 0.0 && b.height > 0.0,
            "the runtime-added card must be placed at non-zero size, got {b:?}"
        );
    }

    #[test]
    fn shared_selection_is_observed_by_both_panes() {
        let (_tree, model, selection, _main_id, _overview_id) = two_pane_tree();
        // Pick a real heavyweight (Delegated) card — `payload(id).is_some()`
        // is true only for `add_widget_item` entries, not the grid cells.
        let card_id = model
            .ids()
            .into_iter()
            .find(|&id| model.payload(id).is_some())
            .expect("at least one Delegated card");
        assert!(selection.selection_signal().get().is_empty());
        selection.select_one(card_id);
        // Both panes were built with `.selection_model(selection.clone())`, so
        // their cards bind THIS same selection signal — selecting once is seen
        // by both panes' borders (verified visually; here we assert the shared
        // signal reflects it).
        assert!(selection.selection_signal().get().contains(&card_id));
    }
}
