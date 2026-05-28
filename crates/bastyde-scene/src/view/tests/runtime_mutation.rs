//! Runtime add / remove / move / pure-a11y reconciliation for `SceneView`.
//!
//! Exercises the visual + (separate) AccessKit self-reconciliation wired in
//! Parts 2a–2d: a post-mount `scene_mut()` mutation must materialise / destroy
//! / reposition heavyweight content AND request an AccessKit re-walk
//! (a relayout no longer re-walks AT on its own).
//!
//! Rebuilds are driven the headless way — a second `tree.layout(...)` re-runs
//! `process_state_changes` + `build()`. The public `a11y_request_handle()`
//! cell proves the re-walk was requested; `sync_accessibility()` drains it, so
//! we drain after the initial layout to distinguish the mutation's request
//! from the initial build's.

use super::*;
use crate::a11y::{A11yGroup, A11yNode};
use bastyde_canvas::Point;

fn viewport() -> SizeProposal {
    SizeProposal::exact(800.0, 600.0)
}

/// Borrow the mounted `SceneView` mutably and run `f` against its `Scene`.
fn with_scene_mut<R>(tree: &mut WidgetTree, view_id: WidgetId, f: impl FnOnce(&mut Scene) -> R) -> R {
    let view = tree
        .widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("view is a SceneView");
    f(&mut view.scene_mut())
}

fn view_ref(tree: &WidgetTree, view_id: WidgetId) -> &SceneView {
    tree.widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("view is a SceneView")
}

#[test]
fn runtime_add_materialises_and_requests_at() {
    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 100.0, 50.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(viewport());
    assert_eq!(tree.children(view_id).len(), 1);

    // Drain the initial build's AT request so we can prove the *add* re-requests.
    let _ = tree.sync_accessibility();
    assert!(!tree.a11y_request_handle().get());

    // Runtime add via the live scene.
    let added = with_scene_mut(&mut tree, view_id, |s| {
        s.add_widget(FillWidget::new(), Rect::new(200.0, 0.0, 120.0, 60.0))
    });
    tree.layout(viewport());

    assert_eq!(
        tree.children(view_id).len(),
        2,
        "the runtime-added widget must materialise into the arena"
    );
    let wid = view_ref(&tree, view_id)
        .widget_id_for(added)
        .expect("added item must be materialised");
    assert_eq!(tree.bounds(wid), Rect::new(200.0, 0.0, 120.0, 60.0));
    assert!(
        tree.a11y_request_handle().get(),
        "a runtime add must request an AccessKit re-walk"
    );
}

#[test]
fn runtime_remove_destroys_and_cleans() {
    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 100.0, 50.0)); // kept
    let group = scene.add_a11y_group(A11yGroup::builder().label(lit!("Act")));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));

    // Add a second widget at runtime and graft it under the group.
    let doomed = with_scene_mut(&mut tree, view_id, |s| {
        let id = s.add_widget(FillWidget::new(), Rect::new(200.0, 0.0, 120.0, 60.0));
        s.set_a11y_parent(A11yNode::Item(id), Some(A11yNode::Group(group)));
        id
    });
    tree.layout(viewport());
    assert_eq!(tree.children(view_id).len(), 2);
    {
        let view = view_ref(&tree, view_id);
        assert!(view.widget_id_for(doomed).is_some(), "doomed must materialise");
        assert_eq!(
            view.scene().a11y_parent_of(A11yNode::Item(doomed)),
            Some(A11yNode::Group(group)),
        );
    }
    let _ = tree.sync_accessibility();

    // Remove it.
    with_scene_mut(&mut tree, view_id, |s| s.remove(doomed));
    tree.layout(viewport());

    let view = view_ref(&tree, view_id);
    assert_eq!(
        tree.children(view_id).len(),
        1,
        "removed widget's arena child must be reaped (no leak)"
    );
    assert_eq!(
        view.widget_id_for(doomed),
        None,
        "materialized / widget_to_item maps must drop the removed item"
    );
    assert_eq!(
        view.scene().a11y_parent_of(A11yNode::Item(doomed)),
        None,
        "Scene::remove must clean the a11y parent map for the removed item"
    );
    assert!(
        tree.a11y_request_handle().get(),
        "a runtime remove must request an AccessKit re-walk"
    );
}

#[test]
fn runtime_move_updates_bounds_and_requests_at() {
    let mut scene = Scene::new();
    let item = scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 100.0, 50.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(viewport());
    let _ = tree.sync_accessibility();

    with_scene_mut(&mut tree, view_id, |s| {
        s.set_local_pos(item, Point::new(300.0, 200.0));
    });
    tree.layout(viewport());

    let wid = view_ref(&tree, view_id)
        .widget_id_for(item)
        .expect("item still materialised");
    assert_eq!(
        tree.bounds(wid),
        Rect::new(300.0, 200.0, 100.0, 50.0),
        "a runtime move must reposition the materialised child"
    );
    assert!(
        tree.a11y_request_handle().get(),
        "a runtime move must request an AccessKit re-walk (screen-projected AT bounds changed)"
    );
}

#[test]
fn pure_a11y_mutation_requests_at_without_visual_change() {
    let mut scene = Scene::new();
    let item = scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 100.0, 50.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(viewport());
    let _ = tree.sync_accessibility();
    assert!(!tree.a11y_request_handle().get());

    // Pure logical-AT mutation: a new group + reparent, NO item geometry change.
    let group = with_scene_mut(&mut tree, view_id, |s| {
        let g = s.add_a11y_group(A11yGroup::builder().label(lit!("Act II")));
        s.set_a11y_parent(A11yNode::Item(item), Some(A11yNode::Group(g)));
        g
    });
    tree.layout(viewport());

    assert_eq!(
        tree.children(view_id).len(),
        1,
        "a pure-a11y mutation must not change the visual child set"
    );
    assert_eq!(
        view_ref(&tree, view_id)
            .scene()
            .a11y_parent_of(A11yNode::Item(item)),
        Some(A11yNode::Group(group)),
    );
    assert!(
        tree.a11y_request_handle().get(),
        "a pure-a11y mutation must still request an AccessKit re-walk (separate AT tree)"
    );
}

#[test]
fn bind_view_state_uses_app_owned_signals() {
    use bastyde_core::signal::Signal;
    let pan_x = Signal::new_animated(0.0_f32);
    let pan_y = Signal::new_animated(0.0_f32);
    let zoom = Signal::new_animated(1.0_f32);
    let rotation = Signal::new_animated(0.0_f32);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).bind_view_state(
        pan_x.clone(),
        pan_y.clone(),
        zoom.clone(),
        rotation.clone(),
    ));
    tree.layout(viewport());

    // The app drives zoom through its own handle; the view reads it (so view
    // state survives across a rebuild-from-state).
    zoom.set(2.0);
    assert_eq!(view_ref(&tree, view_id).zoom(), 2.0);
}

#[test]
fn with_widget_mut_rebuild_materialises_runtime_added_widget() {
    // Drive the EXACT real-app handler path (the corkboard "Add Act" button):
    // ctx.with_widget_mut::<SceneView>(id, Rebuild, |v| v.scene_mut().add_widget(...)).
    // This must (a) schedule a frame and (b) materialise the new card on the
    // next layout. The headless add_act test mutates the scene directly, so it
    // never exercised this deferred path.
    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 100.0, 50.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(viewport());
    assert_eq!(tree.children(view_id).len(), 1);

    let mut noop = bastyde_core::window::NoopWindowOps;
    tree.run_with_event_context(&mut noop, |ctx| {
        ctx.with_widget_mut::<SceneView>(view_id, BindingLevel::Rebuild, |view| {
            view.scene_mut()
                .add_widget(FillWidget::new(), Rect::new(200.0, 0.0, 120.0, 60.0));
        });
    });

    assert!(
        tree.needs_redraw(),
        "with_widget_mut(Rebuild) must schedule a frame (else the app sleeps and never rebuilds)"
    );

    tree.layout(viewport());
    assert_eq!(
        tree.children(view_id).len(),
        2,
        "the runtime-added card must materialise via the with_widget_mut path"
    );
}

#[test]
fn runtime_added_widget_emits_draw_commands() {
    // Decisive render-walk check: after a runtime add, the new heavyweight
    // child's paint must reach the RenderFrame's draw_order (not just be
    // materialised + placed). If the count doesn't grow, the walk skips it.
    // (The crate-local test `FillWidget` paints nothing, so use a real
    // painting leaf here.)
    #[derive(Debug)]
    struct PaintCard;
    impl Widget for PaintCard {
        fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            p.resolve(50.0, 50.0).into()
        }
        fn paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, _: &PaintContext) {
            canvas.fill_rect(bounds, bastyde_tokens::Color::RED);
        }
    }

    let mut scene = Scene::new();
    scene.add_widget(PaintCard, Rect::new(10.0, 10.0, 100.0, 50.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(viewport());
    let before = tree.render().draw_order.len();

    let mut noop = bastyde_core::window::NoopWindowOps;
    tree.run_with_event_context(&mut noop, |ctx| {
        ctx.with_widget_mut::<SceneView>(view_id, BindingLevel::Rebuild, |view| {
            // In view at pan=0 in an 800×600 viewport.
            view.scene_mut()
                .add_widget(PaintCard, Rect::new(200.0, 100.0, 120.0, 60.0));
        });
    });
    tree.layout(viewport());
    let after = tree.render().draw_order.len();

    assert!(
        after > before,
        "runtime-added widget must emit draw commands (before={before}, after={after})"
    );
}

#[test]
fn pan_via_bound_signal_moves_heavyweight_content() {
    // Both corkboard bugs reduce to "do heavyweight cards follow the view
    // transform driven by app-owned (bind_view_state) signals?". A pan must
    // produce a content-transform PushTransform around the cards in the frame.
    use bastyde_canvas::DrawCommand;
    use bastyde_core::signal::Signal;

    #[derive(Debug)]
    struct PaintCard;
    impl Widget for PaintCard {
        fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            p.resolve(50.0, 50.0).into()
        }
        fn paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, _: &PaintContext) {
            canvas.fill_rect(bounds, bastyde_tokens::Color::RED);
        }
    }

    let pan_x = Signal::new_animated(0.0_f32);
    let pan_y = Signal::new_animated(0.0_f32);
    let zoom = Signal::new_animated(1.0_f32);
    let rotation = Signal::new_animated(0.0_f32);
    let mut scene = Scene::new();
    scene.add_widget(PaintCard, Rect::new(100.0, 100.0, 50.0, 50.0));
    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene).bind_view_state(
        pan_x.clone(),
        pan_y.clone(),
        zoom.clone(),
        rotation.clone(),
    ));
    tree.layout(viewport());
    let _ = tree.render();

    // Pan the view through the app-owned signal (what "Reset View" / a scroll do).
    pan_x.set(120.0);
    tree.layout(viewport());
    let frame = tree.render();

    let max_tx = frame
        .draw_order
        .iter()
        .filter_map(|c| match c {
            DrawCommand::PushTransform(t) | DrawCommand::SetTransform(t) => Some(t.m[4]),
            _ => None,
        })
        .fold(0.0_f32, |acc, tx| acc.max(tx));

    assert!(
        (max_tx - 120.0).abs() < 0.5,
        "a pan via the app-owned signal must move heavyweight content (expected a \
         content-transform tx≈120 in the frame, got max tx={max_tx})"
    );
}

#[test]
fn heavyweight_child_far_in_scene_renders_when_panned_into_view() {
    // The corkboard "Add Act draws lines but no card" bug. The per-child render
    // cull (paint_widget_cached) must be pan-aware for a content-transform node
    // (SceneView): a card far down in scene coords, panned into the viewport,
    // must still emit draw commands. The buggy cull compares the child's
    // *scene*-space bounds against the *screen*-space clip and drops it.
    use bastyde_core::signal::Signal;

    #[derive(Debug)]
    struct PaintCard;
    impl Widget for PaintCard {
        fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            p.resolve(50.0, 50.0).into()
        }
        fn paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, _: &PaintContext) {
            canvas.fill_rect(bounds, bastyde_tokens::Color::RED);
        }
    }

    fn red_fill_count(frame: &bastyde_canvas::RenderFrame) -> usize {
        frame
            .decorations
            .iter()
            .filter(|d| d.color == bastyde_tokens::Color::RED.to_array())
            .count()
    }

    let pan_y = Signal::new_animated(0.0_f32);
    let mut scene = Scene::new();
    // A card far below the 600px-tall viewport.
    scene.add_widget(PaintCard, Rect::new(100.0, 1000.0, 50.0, 50.0));
    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene).bind_view_state(
        Signal::new_animated(0.0),
        pan_y.clone(),
        Signal::new_animated(1.0),
        Signal::new_animated(0.0),
    ));
    tree.layout(viewport());
    assert_eq!(red_fill_count(&tree.render()), 0, "card off-screen at pan=0");

    // Pan up by 900 so scene-y 1000 maps to screen-y ~100 — squarely in view.
    pan_y.set(-900.0);
    tree.layout(viewport());
    assert_eq!(
        red_fill_count(&tree.render()),
        1,
        "a card panned into view must render (pan-aware per-child cull)"
    );
}

#[test]
fn reset_animate_to_overrides_in_flight_pan() {
    // Why "Reset View" animates home instead of `set(0)`: the scroll handler
    // pans via `animate_to`, and a plain `set` does NOT cancel that in-flight
    // animation — the scheduler drags pan back toward the scroll target on the
    // next tick, so Reset looks dead. `animate_to(0)` installs a fresh target,
    // overriding the running animation.
    use bastyde_core::signal::Signal;

    let pan_x = Signal::new_animated(0.0_f32);
    let mut tree = WidgetTree::new();
    let _v = tree.add(SceneView::new(Scene::new()).bind_view_state(
        pan_x.clone(),
        Signal::new_animated(0.0),
        Signal::new_animated(1.0),
        Signal::new_animated(0.0),
    ));
    tree.layout(viewport());

    // Simulate a scroll: pan animates toward 300 (in-flight).
    pan_x.animate_to(300.0, std::time::Duration::from_millis(200), bastyde_tokens::Easing::EaseOut);
    assert_eq!(pan_x.animation_target(), Some(300.0));

    // Reset must replace that target with 0.
    pan_x.animate_to(0.0, std::time::Duration::from_millis(220), bastyde_tokens::Easing::EaseOut);
    assert_eq!(
        pan_x.animation_target(),
        Some(0.0),
        "Reset's animate_to must override the in-flight scroll target"
    );
}

#[test]
fn initial_zoom_seeds_the_view() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()).initial_zoom(2.5));
    tree.layout(viewport());
    assert_eq!(view_ref(&tree, view_id).zoom(), 2.5);
}

/// A lightweight scene item whose bounds derive from a `Signal<f32>` size,
/// with that signal bound at `BindingLevel::Rebuild` — the realistic shape of a
/// signal-animated `add_item_dynamic` item: a size tick marks the view
/// `needs_rebuild`, the next `layout()` re-runs `build()`, and
/// `refresh_dynamic_bounds` reads the new bounds back. `set_local_bounds` is a
/// no-op because `local_bounds()` (the signal) is the source of truth.
#[derive(Debug)]
struct DynRect {
    size: bastyde_core::signal::Signal<f32>,
}
impl crate::item::SceneItem for DynRect {
    fn local_bounds(&self) -> Rect {
        let s = self.size.get();
        Rect::new(0.0, 0.0, s, s)
    }
    fn set_local_bounds(&mut self, _b: Rect) {}
    fn paint(&self, _: &mut bastyde_canvas::Canvas, _: &crate::item::SceneItemPaintContext) {}
    fn register_bindings(&self, ctx: &mut BuildContext, view_id: WidgetId) {
        self.size
            .bind_to(view_id, ctx.binding_registry(), BindingLevel::Rebuild);
    }
}

#[test]
fn animating_dynamic_item_does_not_rewalk_at_every_frame() {
    // The version-delta gate: a `build()` driven purely by per-frame
    // dynamic-bounds churn must NOT request an AccessKit re-walk (re-walking AT
    // 60×/s for sub-pixel drift is waste a screen reader can't use), but the
    // moment the animation settles, the final bounds must be walked into AT once.
    use bastyde_core::signal::Signal;

    let size = Signal::new(10.0_f32);
    let mut scene = Scene::new();
    // A heavyweight widget so the view has built children (a SceneView with only
    // lightweight items never re-runs build() — the realistic scene the gate
    // protects always has heavyweight cards driving the per-frame rebuild).
    scene.add_widget(FillWidget::new(), Rect::new(400.0, 0.0, 50.0, 50.0));
    scene.add_item_dynamic(DynRect { size: size.clone() }, Point::ZERO);

    let mut tree = WidgetTree::new();
    let _view_id = tree.add(SceneView::new(scene));
    tree.layout(viewport()); // build 1 — initial AT population
    let _ = tree.sync_accessibility(); // drain the initial request
    assert!(!tree.a11y_request_handle().get());

    // Frame 1: the size signal ticks (bound at Rebuild → arms a build).
    size.set(20.0);
    tree.layout(viewport()); // build 2 — dynamic-only churn
    assert!(
        !tree.a11y_request_handle().get(),
        "a build driven purely by dynamic-bounds churn must NOT re-walk AT"
    );

    // Frame 2: still growing.
    size.set(30.0);
    tree.layout(viewport()); // build 3 — still churning
    assert!(
        !tree.a11y_request_handle().get(),
        "mid-animation dynamic churn must NOT re-walk AT"
    );

    // Settle: the signal stops ticking. build 3's refresh changed the entry,
    // which fired the item-change observer → bumped `reconcile_dirty` (also
    // bound at Rebuild) → arms one more build whose refresh finds no change. The
    // was-churning → steady edge walks the final bounds into AT exactly once.
    tree.layout(viewport()); // build 4 — settle
    assert!(
        tree.a11y_request_handle().get(),
        "a settled dynamic animation must re-walk AT once for the final bounds"
    );
}

#[test]
fn discrete_mutation_during_animation_still_rewalks_at() {
    // A real model change interleaved with dynamic-bounds churn must still
    // re-walk AT immediately — the gate suppresses churn, never a structural
    // change. Proves the `mutation_version` delta sees through the churn.
    use crate::a11y::A11yGroup;
    use bastyde_core::signal::Signal;

    let size = Signal::new(10.0_f32);
    let mut scene = Scene::new();
    scene.add_widget(FillWidget::new(), Rect::new(400.0, 0.0, 50.0, 50.0));
    scene.add_item_dynamic(DynRect { size: size.clone() }, Point::ZERO);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(viewport());
    let _ = tree.sync_accessibility();

    // Mid-animation: the size ticks AND an app adds a logical-AT group. The
    // version advanced (a11y mutation) → must re-walk despite the churn.
    size.set(20.0);
    with_scene_mut(&mut tree, view_id, |s| {
        s.add_a11y_group(A11yGroup::builder().label(lit!("Act")));
    });
    tree.layout(viewport());
    assert!(
        tree.a11y_request_handle().get(),
        "a discrete a11y mutation must re-walk AT even while a dynamic item animates"
    );
}
