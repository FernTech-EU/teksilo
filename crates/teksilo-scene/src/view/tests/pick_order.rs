// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One paint order, one answer — measured through the real router.
//!
//! The scene used to decide "what did the pointer hit?" in five places with
//! five different rules, and its own two entry points were wired to *opposite*
//! halves of the dispatch pipeline: tap ran in the preview pass and so beat
//! every heavyweight card, drag ran in the bubble and so lost to every one of
//! them. Everything here is driven through `WidgetTree`'s real pointer path
//! rather than through a picker in isolation, because the thing under test is
//! precisely that the two paths now agree.
//!
//! What each section pins:
//!
//! * the banded total order, across both tiers, through an actual tap;
//! * the equal-`z` tie, which paint and hit used to resolve in *opposite*
//!   directions;
//! * the `Over`-band veto — that an interactive overlay takes the press, the
//!   focus and the drag as well as the tap, and that a **decorative** one takes
//!   none of them, which is the half the crate's per-item AT tree depends on;
//! * that an irregular card which refuses a point hands it to the item behind,
//!   instead of the press vanishing between two pickers that answered
//!   independently.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget::{CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

use teksilo_i18n::lit;

use crate::flags::ItemFlags;
use crate::item::ItemId;
use crate::items::RectItem;
use crate::scene::{Scene, SceneLayer};
use crate::selection::SceneSelectionMode;
use crate::view::SceneView;

// ---------------------------------------------------------------------- rig

fn click(tree: &mut WidgetTree, at: Point) {
    tree.pointer_move(at);
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Primary,
        Modifiers::default(),
    ));
}

fn counter() -> Rc<Cell<u32>> {
    Rc::new(Cell::new(0))
}

/// A lightweight tile with a tap handler, at `pos`, `size` across.
fn tappable_item(
    scene: &mut Scene,
    pos: Point,
    size: f32,
    layer: SceneLayer,
    z: f32,
    taps: &Rc<Cell<u32>>,
) -> ItemId {
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, size, size)).fill(teksilo_tokens::Color::RED),
        pos,
    );
    scene.set_layer(id, layer);
    scene.set_z(id, z);
    let t = taps.clone();
    scene
        .handlers_mut(id)
        .unwrap()
        .on_tap(move |_pt, _ctx| t.set(t.get() + 1));
    id
}

/// A heavyweight card that counts its own taps.
#[derive(Debug)]
struct TappableCard(Rc<Cell<u32>>);

impl Widget for TappableCard {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let c = self.0.clone();
        ctx.apply_self_handlers(
            teksilo_core::widget_builder::HandlerSet::new()
                .on_tap(move |_e, _ctx| c.set(c.get() + 1))
                .focusable(true)
                .cursor(CursorIcon::Text),
        );
        vec![]
    }
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        p.resolve(100.0, 100.0).into()
    }
}

/// A card whose visible silhouette is an inscribed disc — the pattern
/// `docs/teksilo-scene.md` tells scene-node authors to write, and the one that
/// makes "narrow-phase the heavyweight tier by its scene rect" a wrong answer.
#[derive(Debug)]
struct RoundCard(Rc<Cell<u32>>);

impl Widget for RoundCard {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let c = self.0.clone();
        ctx.apply_self_handlers(
            teksilo_core::widget_builder::HandlerSet::new()
                .on_tap(move |_e, _ctx| c.set(c.get() + 1)),
        );
        vec![]
    }
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        p.resolve(100.0, 100.0).into()
    }
    fn hit_shape(&self, local: Point, bounds: Rect) -> bool {
        let r = bounds.width.min(bounds.height) / 2.0;
        let c = bounds.center();
        (local.x - c.x).powi(2) + (local.y - c.y).powi(2) <= r * r
    }
    fn paint(&self, _b: Rect, _canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {}
}

// ------------------------------------------------------- the banded order

/// The whole contract in one fixture: an `Under` item at `z = 100`, a card at
/// `z = 0`, an `Over` item at `z = -100`, all three stacked on the same point.
/// Paint puts the `Over` item on top whatever the numbers say, so the pointer
/// must go there too — and when it is taken away, to the card, and only then to
/// the `Under` item.
#[test]
fn the_total_order_spans_both_tiers() {
    let build = |keep_over: bool, keep_card: bool| {
        let under_taps = counter();
        let over_taps = counter();
        let card_taps = counter();
        let mut scene = Scene::new();
        tappable_item(
            &mut scene,
            Point::new(50.0, 50.0),
            100.0,
            SceneLayer::Under,
            100.0,
            &under_taps,
        );
        if keep_card {
            let card = scene.add_widget(
                TappableCard(card_taps.clone()),
                Rect::new(50.0, 50.0, 100.0, 100.0),
            );
            scene.set_z(card, 0.0);
        }
        if keep_over {
            tappable_item(
                &mut scene,
                Point::new(50.0, 50.0),
                100.0,
                SceneLayer::Over,
                -100.0,
                &over_taps,
            );
        }
        let mut tree = WidgetTree::new();
        tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        click(&mut tree, Point::new(100.0, 100.0));
        (under_taps.get(), card_taps.get(), over_taps.get())
    };

    assert_eq!(
        build(true, true),
        (0, 0, 1),
        "the Over item is painted last, so it takes the tap — even at z = -100 \
         against an Under item at z = 100",
    );
    assert_eq!(
        build(false, true),
        (0, 1, 0),
        "with the Over item gone the card is topmost, and an Under item with a \
         handler must not steal from the card painted over it",
    );
    assert_eq!(
        build(false, false),
        (1, 0, 0),
        "with nothing above it the Under item finally gets its tap",
    );
}

/// Equal `z`, and the defect runs the opposite way from the obvious guess.
///
/// `Scene::ids()` is ascending, and the hit snapshots used to apply a *stable
/// descending* sort of `z` alone over it — which leaves equal-`z` ties in
/// **ascending** order, so the first match was the oldest, i.e. the bottom-most
/// item. Paint's stable ascending sort makes the newest paint last, on top.
/// They were exactly inverted.
#[test]
fn equal_z_resolves_to_the_item_paint_puts_on_top() {
    let first = counter();
    let second = counter();
    let mut scene = Scene::new();
    tappable_item(
        &mut scene,
        Point::new(50.0, 50.0),
        100.0,
        SceneLayer::Under,
        0.0,
        &first,
    );
    tappable_item(
        &mut scene,
        Point::new(50.0, 50.0),
        100.0,
        SceneLayer::Under,
        0.0,
        &second,
    );

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    click(&mut tree, Point::new(100.0, 100.0));

    assert_eq!(
        (first.get(), second.get()),
        (0, 1),
        "the later-inserted item paints last and must therefore take the tap",
    );
}

/// The same tie, asked of the eager query, which had the same inversion.
#[test]
fn scene_item_at_agrees_with_dispatch_on_an_equal_z_tie() {
    let mut scene = Scene::new();
    let older = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let newer = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(50.0, 50.0),
    );
    assert_eq!(scene.item_at(Point::new(100.0, 100.0)), Some(newer));
    assert_eq!(
        scene.items_at(Point::new(100.0, 100.0)),
        vec![newer, older],
        "topmost first",
    );
}

/// `Scene::item_at` respects the band too, not only `z`.
#[test]
fn scene_item_at_puts_the_over_band_above_a_higher_z_under_item() {
    let mut scene = Scene::new();
    let under = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(50.0, 50.0),
    );
    scene.set_z(under, 100.0);
    let over = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    scene.set_layer(over, SceneLayer::Over);
    scene.set_z(over, -100.0);
    assert_eq!(scene.item_at(Point::new(100.0, 100.0)), Some(over));
}

/// The flag contracts, on the eager query as well as on dispatch.
#[test]
fn scene_item_at_skips_hidden_and_disabled_entries() {
    let make = |hide: bool, disable: bool| {
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::RED),
            Point::new(50.0, 50.0),
        );
        if hide {
            scene.set_visible(id, false);
        }
        if disable {
            scene.set_flag(id, ItemFlags::IS_ENABLED, false);
        }
        (scene.item_at(Point::new(100.0, 100.0)), id)
    };
    let (hit, id) = make(false, false);
    assert_eq!(hit, Some(id), "precondition");
    assert_eq!(make(true, false).0, None, "IS_VISIBLE: not hit-tested");
    assert_eq!(make(false, true).0, None, "IS_ENABLED: clicks pass through");
}

// ----------------------------------------------- the irregular-card blocker

/// A card whose `hit_shape` refuses the point must hand it to the item behind,
/// not drop it.
///
/// This is the shape the design had to get right: had the lightweight tier
/// yielded on the *guess* that a card's scene rectangle means a card was hit,
/// the preview would have stood aside, the arena would then have rejected the
/// card through `hit_shape`, the view would have become the target, and the
/// view's own handler would have re-run the same guess and stood aside again.
/// Nothing would have handled the press. Reading the router's verdict instead
/// of re-deriving it cannot fail that way — by the time this view is asked, the
/// arena has already applied `hit_shape`.
#[test]
fn a_cards_transparent_corner_falls_through_to_the_item_behind_it() {
    let build = |at: Point| {
        let behind = counter();
        let card = counter();
        let mut scene = Scene::new();
        tappable_item(
            &mut scene,
            Point::new(50.0, 50.0),
            100.0,
            SceneLayer::Under,
            0.0,
            &behind,
        );
        scene.add_widget(RoundCard(card.clone()), Rect::new(50.0, 50.0, 100.0, 100.0));
        let mut tree = WidgetTree::new();
        tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        click(&mut tree, at);
        (behind.get(), card.get())
    };

    assert_eq!(
        build(Point::new(100.0, 100.0)),
        (0, 1),
        "the centre is inside the disc: the card takes it",
    );
    assert_eq!(
        build(Point::new(56.0, 56.0)),
        (1, 0),
        "the corner is inside the card's box but outside its shape, so the tap \
         must reach the item behind — a tap that reaches neither is the failure \
         mode this is here to rule out",
    );
}

// --------------------------------------------------- the `Over`-band veto

/// A decorative `Over` item over a card: the card keeps everything.
///
/// This is the half that protects the crate's differentiator. Focus-on-release
/// and the touch hold route both resolve from the arena's hit target, so a veto
/// here would move focus out of an embedded note and make explore-by-touch
/// announce the viewport instead of the card. The rule is press-*claiming*, and
/// a halo claims nothing.
#[test]
fn a_decorative_over_item_leaves_the_card_its_press_and_its_focus() {
    let card_taps = counter();
    let mut scene = Scene::new();
    let card_item = scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );
    let halo = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(80.0, 80.0),
    );
    scene.set_layer(halo, SceneLayer::Over);
    scene.set_z(halo, 100.0);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let card_widget = tree.children(view_id)[0];
    let _ = card_item;

    let at = Point::new(100.0, 100.0);
    assert_eq!(
        tree.hit_test_for(
            at,
            &teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO)
        ),
        Some(card_widget),
        "a decorative overlay must not veto the card in the arena either — \
         this is where explore-by-touch and the touch hold route read from",
    );

    click(&mut tree, at);
    assert_eq!(card_taps.get(), 1, "the card gets its own tap");
    assert_eq!(
        tree.focused(),
        Some(card_widget),
        "and its focus: clicking a halo drawn over a note must not move focus \
         out of the note",
    );
}

/// What a decoration drawn `Over` an embedded card does to that card, measured
/// on all four channels at once.
///
/// [`claims_press`](crate::pick::claims_press) is not interesting as a set of
/// fields; it is interesting as an answer to "can I still click the note?", so
/// that is what this measures. Each channel is one of the six things the arena
/// hit target decides: the tap, focus-on-release, the raw hit (which is also
/// where the touch hold route and press feedback read from), and the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
struct OverlayVerdict {
    card_taps: u32,
    focus_is_card: bool,
    hit_is_card: bool,
    cursor: CursorIcon,
}

impl OverlayVerdict {
    /// The card is untouched: it takes the click, the focus and the arena's
    /// hit, and shows its own `Text` cursor.
    const fn card_keeps_everything() -> Self {
        Self {
            card_taps: 1,
            focus_is_card: true,
            hit_is_card: true,
            cursor: CursorIcon::Text,
        }
    }

    /// The overlay claimed the press: the card loses the click, the focus and
    /// the hit, and the view owns the cursor.
    const fn overlay_took_the_press(cursor: CursorIcon) -> Self {
        Self {
            card_taps: 0,
            focus_is_card: false,
            hit_is_card: false,
            cursor,
        }
    }
}

/// Build `card (focusable, on_tap, Text cursor) + optional Over decoration on
/// top of it`, click the middle, and report all four channels.
fn overlay_verdict(decorate: Option<&dyn Fn(&mut Scene, ItemId)>) -> OverlayVerdict {
    let card_taps = counter();
    let mut scene = Scene::new();
    scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );
    if let Some(decorate) = decorate {
        let deco = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
            Point::new(80.0, 80.0),
        );
        scene.set_layer(deco, SceneLayer::Over);
        scene.set_z(deco, 100.0);
        decorate(&mut scene, deco);
    }

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let card_widget = tree.children(view_id)[0];

    let at = Point::new(100.0, 100.0);
    let mouse = teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO);
    let hit_is_card = tree.hit_test_for(at, &mouse) == Some(card_widget);
    tree.pointer_move(at);
    let cursor = tree.current_cursor();
    click(&mut tree, at);
    OverlayVerdict {
        card_taps: card_taps.get(),
        focus_is_card: tree.focused() == Some(card_widget),
        hit_is_card,
        cursor,
    }
}

/// A **hover** affordance drawn over a card must not cost the card anything.
///
/// `on_hover`, `cursor`, `tooltip` and `accepts_drops` all used to count as
/// press claims, and the result was not a conservative over-approximation: the
/// veto took the card out of the arena's walk, the view became the target, and
/// the tap then resolved to an item with no `on_tap` — so the click reached
/// *nobody*. A corkboard that put a `.tooltip("drop here")` on an `Over` hint
/// region made every note under it un-clickable, un-focusable and un-editable
/// by mouse, with no error and no handler firing.
///
/// Measured against the bare card, which is the definition of "costs nothing".
#[test]
fn a_hover_affordance_over_a_card_costs_the_card_nothing() {
    let bare = overlay_verdict(None);
    assert_eq!(
        bare,
        OverlayVerdict::card_keeps_everything(),
        "control: with nothing over it the card owns all four channels",
    );

    assert_eq!(
        overlay_verdict(Some(&|_scene, _deco| {})),
        bare,
        "a plain decorative rect",
    );
    assert_eq!(
        overlay_verdict(Some(&|scene: &mut Scene, deco| {
            scene.handlers_mut(deco).unwrap().tooltip(lit!("drop here"));
        })),
        bare,
        "tooltip-only — the regression that swallowed clicks on a corkboard",
    );
    assert_eq!(
        overlay_verdict(Some(&|scene: &mut Scene, deco| {
            scene.handlers_mut(deco).unwrap().on_hover(|_, _| {});
        })),
        bare,
        "on_hover-only",
    );
    assert_eq!(
        overlay_verdict(Some(&|scene: &mut Scene, deco| {
            scene.handlers_mut(deco).unwrap().accepts_drops(true);
        })),
        bare,
        "accepts_drops — a drop is a release of someone else's drag, not a press",
    );

    // The one hover affordance with a visible answer of its own. It still costs
    // the card nothing on the three press channels; it is arbitrated on the
    // cursor channel alone, which is where it belongs.
    assert_eq!(
        overlay_verdict(Some(&|scene: &mut Scene, deco| {
            scene
                .handlers_mut(deco)
                .unwrap()
                .cursor(CursorIcon::Crosshair);
        })),
        OverlayVerdict {
            cursor: CursorIcon::Crosshair,
            ..bare
        },
        "cursor-only: the item's cursor shows, the card keeps its press",
    );
}

/// The converse row of the same matrix: a handler that a press is the beginning
/// of *does* claim, and takes all three press channels with it.
#[test]
fn a_press_acting_overlay_takes_the_cards_press() {
    let taken = OverlayVerdict::overlay_took_the_press(CursorIcon::Default);
    assert_eq!(
        overlay_verdict(Some(&|scene: &mut Scene, deco| {
            scene.handlers_mut(deco).unwrap().on_tap(|_, _| {});
        })),
        taken,
        "on_tap",
    );
    assert_eq!(
        overlay_verdict(Some(&|scene: &mut Scene, deco| {
            scene.handlers_mut(deco).unwrap().on_double_tap(|_, _| {});
        })),
        taken,
        "on_double_tap",
    );
    assert_eq!(
        overlay_verdict(Some(&|scene: &mut Scene, deco| {
            scene.handlers_mut(deco).unwrap().on_context_menu(|_, _| {});
        })),
        taken,
        "on_context_menu",
    );
    assert_eq!(
        overlay_verdict(Some(&|scene: &mut Scene, deco| {
            scene.set_flag(deco, ItemFlags::IS_DRAGGABLE, true);
        })),
        // A draggable item under the pointer is owed its `Grab`, and the view
        // is the target, so the view says so.
        OverlayVerdict::overlay_took_the_press(CursorIcon::Grab),
        "IS_DRAGGABLE with no handler at all",
    );
}

/// The other half of D1's principle: hover, cursor and tooltip keep working
/// while the card is the arena's target.
///
/// They never needed the veto. The view's hover seam is registered on
/// `on_pointer_event`, which fires on every **strict ancestor** of the target
/// during the preview pass — so it runs whether a card won the walk or not,
/// resolves the topmost `Over` entry itself, and drives `on_hover`, the hovered
/// item and (from the same `new_hit`) the item tooltip.
#[test]
fn the_hover_seam_reaches_an_over_item_while_the_card_holds_the_target() {
    let hovered = counter();
    let card_taps = counter();
    let mut scene = Scene::new();
    scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );
    let hint = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(80.0, 80.0),
    );
    scene.set_layer(hint, SceneLayer::Over);
    scene.set_z(hint, 100.0);
    {
        let h = hovered.clone();
        let handlers = scene.handlers_mut(hint).unwrap();
        handlers.tooltip(lit!("drop here"));
        handlers.on_hover(move |entered, _ctx| {
            if entered {
                h.set(h.get() + 1);
            }
        });
    }

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let card_widget = tree.children(view_id)[0];
    let mouse = teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO);

    let at = Point::new(100.0, 100.0);
    assert_eq!(
        tree.hit_test_for(at, &mouse),
        Some(card_widget),
        "precondition: the card is the arena's target — nothing vetoed it",
    );
    tree.pointer_move(at);
    assert_eq!(
        hovered.get(),
        1,
        "and the item still gets its on_hover, from the preview pass",
    );
    assert_eq!(
        super::view_handle(&tree, view_id).hovered_item.get(),
        Some(hint),
        "the same `new_hit` the item tooltip is scheduled from",
    );

    // Leaving it retracts, so the pairing an `on_hover(true)` is entitled to
    // still closes while the card holds the target.
    tree.pointer_move(Point::new(60.0, 60.0));
    assert_eq!(
        super::view_handle(&tree, view_id).hovered_item.get(),
        None,
        "and the retract half runs too",
    );
}

/// The converse: an `Over` item that *does* claim the press takes the press,
/// the focus and the tap.
#[test]
fn an_over_claimant_takes_the_press_and_the_focus_from_the_card() {
    let card_taps = counter();
    let item_taps = counter();
    let mut scene = Scene::new();
    scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );
    tappable_item(
        &mut scene,
        Point::new(80.0, 80.0),
        40.0,
        SceneLayer::Over,
        100.0,
        &item_taps,
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let card_widget = tree.children(view_id)[0];

    let at = Point::new(100.0, 100.0);
    assert_eq!(
        tree.hit_test_for(
            at,
            &teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO)
        ),
        Some(view_id),
        "the claimant vetoes the card, so press / focus / hold / cursor land \
         where the tap does instead of being split across the two",
    );

    click(&mut tree, at);
    assert_eq!(item_taps.get(), 1, "the item painted on top gets the tap");
    assert_eq!(card_taps.get(), 0);
    assert_ne!(
        tree.focused(),
        Some(card_widget),
        "the card under an interactive overlay must not silently take focus",
    );
}

/// The veto is per point: one pixel outside the claimant's shape, the card is
/// itself again.
#[test]
fn the_veto_is_per_point_not_per_card() {
    let mut scene = Scene::new();
    scene.add_widget(TappableCard(counter()), Rect::new(0.0, 0.0, 200.0, 200.0));
    tappable_item(
        &mut scene,
        Point::new(0.0, 0.0),
        40.0,
        SceneLayer::Over,
        100.0,
        &counter(),
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let card_widget = tree.children(view_id)[0];
    let mouse = teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO);

    assert_eq!(
        tree.hit_test_for(Point::new(20.0, 20.0), &mouse),
        Some(view_id)
    );
    assert_eq!(
        tree.hit_test_for(Point::new(100.0, 100.0), &mouse),
        Some(card_widget),
        "outside the claimant's 40×40 shape the card is untouched",
    );
}

/// A screen-pinned (`IGNORES_TRANSFORMATIONS`) `Over` claimant is hit-tested in
/// **screen** space, and the veto has to use that space too.
///
/// The arena hands the parent one point, in scene coordinates. The view owns
/// the transform, so it projects it back — and at a zoom of 1 the two spaces
/// coincide, which is exactly why this test zooms.
#[test]
fn a_screen_pinned_over_claimant_vetoes_at_a_zoom_other_than_one() {
    let mut scene = Scene::new();
    scene.add_widget(TappableCard(counter()), Rect::new(0.0, 0.0, 400.0, 400.0));
    let badge = tappable_item(
        &mut scene,
        // Scene anchor (100, 100) → screen (200, 200) at zoom 2.
        Point::new(100.0, 100.0),
        40.0,
        SceneLayer::Over,
        100.0,
        &counter(),
    );
    scene.set_flag(badge, ItemFlags::IGNORES_TRANSFORMATIONS, true);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).initial_zoom(2.0));
    tree.layout(SizeProposal::exact(600.0, 600.0));
    let card_widget = tree.children(view_id)[0];
    let mouse = teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO);

    // The badge draws as a fixed 40×40 at screen (200, 200)..(240, 240).
    assert_eq!(
        tree.hit_test_for(Point::new(220.0, 220.0), &mouse),
        Some(view_id),
        "the badge is pinned in screen space; the veto must test it there",
    );
    // Where the badge would be if it scaled with the view — it does not.
    assert_eq!(
        tree.hit_test_for(Point::new(260.0, 260.0), &mouse),
        Some(card_widget),
        "and only there: a scene-space comparison would veto this point too",
    );
}

/// Intra-lightweight occlusion is **unchanged**. A handler-less item painted on
/// top of a claiming one still blocks it — the hit test resolves the topmost
/// *entry* and only then looks for handlers, and `claims_press` decides one
/// thing only: whether the lightweight tier may veto a card.
#[test]
fn a_handlerless_item_still_blocks_the_claimant_beneath_it() {
    let lower = counter();
    let mut scene = Scene::new();
    tappable_item(
        &mut scene,
        Point::new(50.0, 50.0),
        100.0,
        SceneLayer::Over,
        0.0,
        &lower,
    );
    let blocker = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(50.0, 50.0),
    );
    scene.set_layer(blocker, SceneLayer::Over);
    scene.set_z(blocker, 10.0);

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    click(&mut tree, Point::new(100.0, 100.0));
    assert_eq!(
        lower.get(),
        0,
        "the decorative item on top absorbs the tap, as it always has",
    );
}

/// …and the same arrangement over a card does not veto it: the topmost `Over`
/// entry is the decorative one, so the card keeps its press.
#[test]
fn a_handlerless_item_on_top_of_a_claimant_does_not_veto_the_card() {
    let card = counter();
    let mut scene = Scene::new();
    scene.add_widget(
        TappableCard(card.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );
    tappable_item(
        &mut scene,
        Point::new(50.0, 50.0),
        100.0,
        SceneLayer::Over,
        0.0,
        &counter(),
    );
    let blocker = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(50.0, 50.0),
    );
    scene.set_layer(blocker, SceneLayer::Over);
    scene.set_z(blocker, 10.0);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let card_widget = tree.children(view_id)[0];
    let mouse = teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO);
    assert_eq!(
        tree.hit_test_for(Point::new(100.0, 100.0), &mouse),
        Some(card_widget),
        "veto and dispatch must agree: both resolve the topmost Over ENTRY, \
         and that entry claims nothing",
    );
    click(&mut tree, Point::new(100.0, 100.0));
    assert_eq!(card.get(), 1);
}

// ------------------------------------------------------------ the drag floor

/// A pull that starts where a card is on top must not grab a lightweight item
/// the card was covering.
///
/// This is the half of the inversion that was easiest to miss. A `SceneView`
/// receives the press **even when a card is the arena's target** — press and
/// release is a tap on the card, press and pull is an ancestor drag on the view
/// (click a card to select it, pull away from it to marquee), which is a
/// documented and wanted behaviour. So the drag path cannot infer "no card was
/// hit" from the fact that it is running: it reads the floor the press
/// recorded, which is the same verdict the tap read.
///
/// Three arrangements, one pull, one question each.
#[test]
fn a_pull_over_a_card_does_not_grab_what_the_card_covers() {
    let build = |with_card: bool, with_claimant: bool| {
        let mut scene = Scene::new();
        let wire = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .fill(teksilo_tokens::Color::BLUE)
                .draggable(true),
            Point::new(50.0, 50.0),
        );
        if with_card {
            scene.add_widget(TappableCard(counter()), Rect::new(50.0, 50.0, 100.0, 100.0));
        }
        if with_claimant {
            tappable_item(
                &mut scene,
                Point::new(50.0, 50.0),
                100.0,
                SceneLayer::Over,
                0.0,
                &counter(),
            );
        }
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene).selection_mode(SceneSelectionMode::Multi));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let from = Point::new(100.0, 100.0);
        tree.pointer_move(from);
        tree.dispatch_event(WidgetEvent::pointer_down(
            from,
            PointerButton::Primary,
            Modifiers::default(),
        ));
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(100.0, 200.0)));
        let grabbed = super::view_handle(&tree, view_id)
            .drag_target
            .get()
            .map(|t| t.item_id);
        (grabbed, wire)
    };

    let (bare, wire) = build(false, false);
    assert_eq!(
        bare,
        Some(wire),
        "control: with nothing over it the draggable item is grabbed",
    );
    let (under_card, wire) = build(true, false);
    assert_ne!(
        under_card,
        Some(wire),
        "the card is painted on top of the item, so a pull that starts there \
         must not grab it — the ancestor drag marquees instead",
    );
    let (under_claimant, wire) = build(true, true);
    assert_ne!(
        under_claimant,
        Some(wire),
        "and the same when an Over claimant vetoed the card: the press only \
         reached this view because something on top of the card claimed it",
    );
}

// ------------------------------------------------------------- the cursor

/// When the view yields to a card it must leave the cursor alone. The preview
/// pass runs *before* the target's own node cursor resolves, so setting
/// `Default` there silently overwrites whatever the card asked for.
///
/// The fixture carries a **decorative `Over` rect** over the card, which is the
/// only arrangement that tells the two candidate guards apart. The snapshot
/// deliberately holds every hit-testable entry, so that rect is a `new_hit`;
/// keying the yield off `new_hit.is_none()` therefore stops yielding exactly
/// where a yield matters most and stomps the card's `Text` with `Default`. A
/// bare card passes either way, which is why this one is not bare.
#[test]
fn the_view_does_not_reset_the_cursor_when_it_yields_to_a_card() {
    let mut scene = Scene::new();
    scene.add_widget(TappableCard(counter()), Rect::new(50.0, 50.0, 100.0, 100.0));
    let deco = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(80.0, 80.0),
    );
    scene.set_layer(deco, SceneLayer::Over);
    scene.set_z(deco, 100.0);

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(Point::new(100.0, 100.0));
    assert_eq!(
        tree.current_cursor(),
        CursorIcon::Text,
        "the card declares a Text cursor and the overlay declares none, so the          scene has nothing to say and must not stomp it",
    );
    tree.pointer_move(Point::new(300.0, 250.0));
    assert_eq!(
        tree.current_cursor(),
        CursorIcon::Default,
        "off the card the view is the target again and owns the cursor",
    );
}

/// The other side of the same rule: an `Over` item that *does* declare a cursor
/// is painted on top of the card, so its cursor is the right answer — and it
/// gets there **without** the veto. The card keeps its press and its focus.
///
/// This is what makes `cursor` safe to drop from
/// [`claims_press`](crate::pick::claims_press): the affordance is arbitrated
/// here, where it costs the card nothing, instead of by taking the card out of
/// the arena's walk and stranding the click.
#[test]
fn an_over_items_declared_cursor_wins_without_taking_the_cards_press() {
    let card_taps = counter();
    let mut scene = Scene::new();
    scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );
    let hint = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(80.0, 80.0),
    );
    scene.set_layer(hint, SceneLayer::Over);
    scene.set_z(hint, 100.0);
    scene
        .handlers_mut(hint)
        .unwrap()
        .cursor(CursorIcon::Crosshair);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let card_widget = tree.children(view_id)[0];

    let at = Point::new(100.0, 100.0);
    tree.pointer_move(at);
    assert_eq!(
        tree.current_cursor(),
        CursorIcon::Crosshair,
        "the item is painted over the card, so its cursor is the one to show",
    );
    click(&mut tree, at);
    assert_eq!(
        card_taps.get(),
        1,
        "and the card still gets the press: a cursor is not a press claim",
    );
    assert_eq!(tree.focused(), Some(card_widget));
}

/// Yielding is not a state the view can enter and never leave. Once it has
/// **spoken** for a hover episode, silence no longer means "the card's cursor
/// shows" — it means "whatever I said last still shows", because
/// `current_cursor` only ever moves when somebody writes to it, and the node
/// cursor is written on `PointerEnter`/`PointerLeave` alone.
///
/// So the pointer sliding off an `Over` item onto the *same* card produces no
/// hover transition, and the item's `Crosshair` used to survive across the
/// whole rest of the note. The three positions below are the whole episode:
/// over the item, inside the card but off the item, and away. Only the first
/// and the last were ever right.
///
/// The fix is that the view **hands the cursor back** — `release_cursor`, the
/// withdrawal half of `set_cursor` — instead of going quiet.
#[test]
fn the_view_hands_the_cursor_back_when_it_runs_out_of_one() {
    /// Hover the item, slide inside the card off the item, then leave the
    /// card — reporting the cursor at each stop.
    fn episode(item_cursor: Option<CursorIcon>) -> [CursorIcon; 3] {
        let mut scene = Scene::new();
        scene.add_widget(TappableCard(counter()), Rect::new(50.0, 50.0, 100.0, 100.0));
        if let Some(c) = item_cursor {
            let hint = scene.add_item(
                RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
                Point::new(80.0, 80.0),
            );
            scene.set_layer(hint, SceneLayer::Over);
            scene.set_z(hint, 100.0);
            scene.handlers_mut(hint).unwrap().cursor(c);
        }

        let mut tree = WidgetTree::new();
        tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut seen = [CursorIcon::Default; 3];
        for (slot, at) in seen.iter_mut().zip([
            Point::new(100.0, 100.0), // on the item, which is on the card
            Point::new(60.0, 60.0),   // still on the card, off the item
            Point::new(300.0, 250.0), // off the card entirely
        ]) {
            tree.pointer_move(at);
            *slot = tree.current_cursor();
        }
        seen
    }

    assert_eq!(
        episode(Some(CursorIcon::Crosshair)),
        [CursorIcon::Crosshair, CursorIcon::Text, CursorIcon::Default],
        "the item's cursor while over it, the card's the moment the view has \
         none left to offer, and the view's own once the card is behind us",
    );

    // The view never speaks here, so the card's `Text` is never disturbed and
    // the withdrawal has nothing to undo. It must still be a no-op.
    assert_eq!(
        episode(None),
        [CursorIcon::Text, CursorIcon::Text, CursorIcon::Default],
        "a card with no Over item above it is unaffected",
    );

    // The degenerate case a memo could get wrong: the view sets the very
    // cursor the card declares, so nothing observable changes at the boundary
    // — and the card must still own it afterwards, not the view.
    assert_eq!(
        episode(Some(CursorIcon::Text)),
        [CursorIcon::Text, CursorIcon::Text, CursorIcon::Default],
        "item and card agreeing must not make the hand-back a no-op that \
         leaves the view's copy in place",
    );
}

// ------------------------------------------------------------- re-entrancy

/// The veto runs **inside the router**, where the scene's `RefCell` may already
/// be borrowed by whoever is mid-mutation, so it must answer from the
/// per-layout snapshot and never from the model.
///
/// Measured the only way that cannot be faked: hold the model's borrow and hit
/// test through it. The moment someone "just checks the scene here" instead,
/// this panics.
#[test]
fn the_veto_answers_without_re_entering_the_model() {
    let mut scene = Scene::new();
    scene.add_widget(TappableCard(counter()), Rect::new(50.0, 50.0, 100.0, 100.0));
    tappable_item(
        &mut scene,
        Point::new(80.0, 80.0),
        40.0,
        SceneLayer::Over,
        100.0,
        &counter(),
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let model = super::view_handle(&tree, view_id).model();
    // `borrow_mut`, not `borrow`: a shared borrow would let a re-entrant
    // *read* through, and reading the model from the hit path is exactly the
    // mistake this pins.
    let held = model.0.borrow_mut();
    let mouse = teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO);
    // Both branches of the veto: a point an `Over` claimant owns (which scans
    // the snapshot) and one it does not (which still consults the gate).
    assert_eq!(
        tree.hit_test_for(Point::new(100.0, 100.0), &mouse),
        Some(view_id),
    );
    assert!(tree.hit_test_for(Point::new(60.0, 60.0), &mouse).is_some());
    drop(held);
}

// ------------------------------------------------------------ accessibility

/// A scene entry named for the AT probe, so a failure reads as a sentence.
#[derive(Debug)]
struct NamedNote;

impl Widget for NamedNote {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(
            teksilo_core::widget_builder::HandlerSet::new()
                .on_tap(|_e, _c| {})
                .focusable(true),
        );
        vec![]
    }
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        p.resolve(100.0, 100.0).into()
    }
    fn accessibility(&self, b: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        b.set_role(accesskit::Role::TextInput);
        b.set_name("the note");
    }
}

/// Exactly what the press-claiming rule reconciles, and exactly what it does
/// not — measured on all three channels at once.
///
/// The rule governs the **pointer**: the arena's hit test, and therefore press,
/// focus-on-release, the touch hold route, the cursor and the drag. It changes
/// no AccessKit node, so it cannot move where a platform explore-by-touch probe
/// lands — that resolves through the AT tree, whose child list is in *emission*
/// order (groups, then lightweight items, then the framework's widget children)
/// while `node_at_point` walks children in reverse. The heavyweight tier
/// therefore wins an AT probe unconditionally, whatever band the overlay is in
/// and whatever it claims.
///
/// | overlay | tap | arena hit | AT probe |
/// |---------|-----|-----------|----------|
/// | decorative | the note | the note | the note |
/// | claiming   | the item | the view | the note |
///
/// The decorative row is the corkboard case and the one the rule was chosen
/// for; it agrees on all three. The claiming row is the documented gap, whose
/// fix is an AT-walker slot that emits a widget's own nodes after its children
/// (the accessibility analogue of `post_paint`) — a change to the AT tree's
/// *reading* order, and therefore not a picker fix. See
/// `docs/teksilo-scene-a11y.md`, "Where the pointer and the AT probe still
/// disagree".
///
/// This test exists so that gap cannot narrow or widen unnoticed: it pins both
/// rows, so the day the AT order is reconciled, it is reconciled deliberately.
#[test]
fn what_the_press_claiming_rule_does_and_does_not_reconcile() {
    // (tap went to the item, arena hit was the card, AT probe named the note)
    let measure = |claiming: bool| {
        let item_taps = counter();
        let mut scene = Scene::new();
        scene.add_widget(NamedNote, Rect::new(50.0, 50.0, 100.0, 100.0));
        let over = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::RED),
            Point::new(50.0, 50.0),
        );
        scene.set_layer(over, SceneLayer::Over);
        scene.set_z(over, 100.0);
        if claiming {
            let t = item_taps.clone();
            scene
                .handlers_mut(over)
                .unwrap()
                .on_tap(move |_pt, _ctx| t.set(t.get() + 1));
        }

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let card = tree.children(view_id)[0];
        let mouse =
            teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO);

        let at = Point::new(100.0, 100.0);
        let hit_is_card = tree.hit_test_for(at, &mouse) == Some(card);
        click(&mut tree, at);

        // Where a platform explore-by-touch probe lands, resolved through the
        // same consumer every platform adapter answers from — not through what
        // the scene believes it emitted.
        let update = tree.accessibility_tree_snapshot();
        let labels: std::collections::HashMap<_, _> = update
            .nodes
            .iter()
            .map(|(id, n)| (*id, n.label().map(|l| l.to_string()).unwrap_or_default()))
            .collect();
        let consumer = accesskit_consumer::Tree::new(update, false);
        let probe = consumer
            .state()
            .root()
            .node_at_point(accesskit::Point::new(at.x as f64, at.y as f64), &|_| {
                accesskit_consumer::FilterResult::Include
            })
            .map(|n| labels.get(&n.locate().0).cloned().unwrap_or_default());

        (item_taps.get(), hit_is_card, probe)
    };

    assert_eq!(
        measure(false),
        (0, true, Some(String::from("the note"))),
        "decorative overlay: the note takes the tap and the arena's hit, and \
         the AT probe names it too — all three agree, which is the case the \
         press-claiming rule was chosen to protect",
    );
    assert_eq!(
        measure(true),
        (1, false, Some(String::from("the note"))),
        "claiming overlay: the item takes the tap, the veto hands the arena's \
         hit to the view — and the AT probe still names the note, because the \
         rule moves no AccessKit node. This row is the DOCUMENTED GAP, not an \
         accident; see docs/teksilo-scene-a11y.md before changing it",
    );
}

/// The veto is a hit-test mechanism and nothing else: it must move no AT node
/// and no tab stop. Measured as a delta between a decorative overlay and a
/// claiming one over the same card — the two scenes differ only in whether the
/// item carries an `on_tap`.
#[test]
fn the_veto_changes_neither_the_at_tree_nor_the_tab_stops() {
    let build = |claiming: bool| {
        let mut scene = Scene::new();
        scene.add_widget(TappableCard(counter()), Rect::new(50.0, 50.0, 100.0, 100.0));
        let over = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
            Point::new(80.0, 80.0),
        );
        scene.set_layer(over, SceneLayer::Over);
        if claiming {
            scene.handlers_mut(over).unwrap().on_tap(|_pt, _ctx| {});
        }
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let stops = tree.tab_stops_within(view_id);
        let card = tree.children(view_id)[0];
        (
            tree.accessibility_tree_snapshot().nodes.len(),
            stops.len(),
            stops.contains(&card),
        )
    };
    let decorative = build(false);
    let claiming = build(true);
    assert_eq!(
        decorative, claiming,
        "AT node count and tab stops must be identical; only hit-testing moved",
    );
    assert!(
        claiming.2,
        "the card stays keyboard-reachable under a claiming overlay — a veto \
         refuses a POINT, it does not remove a node",
    );
}
