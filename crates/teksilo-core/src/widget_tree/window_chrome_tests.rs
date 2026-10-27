// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A modal leaves the window's own chrome live.
//!
//! An in-window modal's scrim covers the whole window, a title bar the
//! window draws for itself included. The window system's frame stays live
//! above a dialog (WinUI keeps its caption buttons, libadwaita makes the dim
//! a window handle), so the chrome a window draws stays live too: its drag
//! surface, its window buttons, its resize edges. What the app puts in a
//! title bar stays behind the modal, as the rest of the page does.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};

use crate::overlay::{DismissBehavior, OverlayId, OverlayPlacement, OverlayRequest};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_builder::{HandlerSet, WidgetBuilder};
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

/// Children side by side in equal columns, so a point at a child's centre
/// lands on that child and no other.
#[derive(Debug)]
struct Row(Vec<WidgetId>);

impl crate::widget::Widget for Row {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &crate::widget::LayoutContext,
    ) -> crate::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        _ctx: &crate::widget::LayoutContext,
    ) {
        let width = bounds.width / children.len().max(1) as f32;
        for (i, child) in children.iter_mut().enumerate() {
            child.origin = Point::new(bounds.x + width * i as f32, bounds.y);
            child.size = teksilo_canvas::Size::new(width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.0.clone()
    }
}

/// A piece of window chrome, as the title bar's own widgets mark
/// themselves, filling its bounds with its one child if it has one, and
/// counting its taps.
#[derive(Debug)]
struct Chrome {
    child: Option<WidgetId>,
    taps: Rc<Cell<u32>>,
}

impl crate::widget::Widget for Chrome {
    fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
        let taps = self.taps.clone();
        ctx.apply_self_handlers(
            HandlerSet::new()
                .window_chrome()
                .on_tap(move |_, _| taps.set(taps.get() + 1)),
        );
        self.child.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &crate::widget::LayoutContext,
    ) -> crate::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        _ctx: &crate::widget::LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

/// A window drawing its own title bar, and a modal to put over it.
struct Window {
    tree: WidgetTree,
    /// The drag surface, holding the app's title.
    surface: WidgetId,
    surface_taps: Rc<Cell<u32>>,
    /// A drag surface holding an app control behind a gesture dead zone, as
    /// an app puts a control in a title bar.
    app_control: WidgetId,
    app_control_taps: Rc<Cell<u32>>,
    /// A window button.
    close: WidgetId,
    close_taps: Rc<Cell<u32>>,
    /// A control on the page.
    page_control: WidgetId,
    page_taps: Rc<Cell<u32>>,
    panel: WidgetId,
    scrim: WidgetId,
}

impl Window {
    fn new() -> Self {
        let mut tree = WidgetTree::new();
        let counter = || Rc::new(Cell::new(0));
        let (surface_taps, app_control_taps, close_taps, page_taps) =
            (counter(), counter(), counter(), counter());
        let tapped = |taps: &Rc<Cell<u32>>| {
            let taps = taps.clone();
            move |_: &crate::gesture::TapEvent, _: &mut crate::widget::EventContext| {
                taps.set(taps.get() + 1)
            }
        };

        let title = tree.add(FillWidget::new().label("Untitled"));
        let surface = tree.add(Chrome {
            child: Some(title),
            taps: surface_taps.clone(),
        });
        let app_control = tree.add(FillWidget::new().on_tap(tapped(&app_control_taps)));
        let dead_zone = tree.add(
            StackWidget::new()
                .child(app_control)
                .gesture_dead_zone(true),
        );
        let second_surface = tree.add(Chrome {
            child: Some(dead_zone),
            taps: Rc::new(Cell::new(0)),
        });
        let close = tree.add(Chrome {
            child: None,
            taps: close_taps.clone(),
        });
        let page_control = tree.add(FillWidget::new().on_tap(tapped(&page_taps)));
        tree.add(Row(vec![surface, second_surface, close, page_control]));

        let panel = tree.add(FillWidget::new());
        tree.set_dormant(panel);
        let scrim = tree.add(FillWidget::new());
        tree.set_dormant(scrim);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        Self {
            tree,
            surface,
            surface_taps,
            app_control,
            app_control_taps,
            close,
            close_taps,
            page_control,
            page_taps,
            panel,
            scrim,
        }
    }

    fn overlay(&mut self, content: WidgetId, placement: OverlayPlacement) -> OverlayId {
        self.tree.activate(content);
        self.tree.show_overlay(OverlayRequest::new(
            content,
            self.page_control,
            placement,
            DismissBehavior::Manual,
        ))
    }

    /// The modal as the app presents one: a scrim, then the centred panel,
    /// the scrim's parent.
    fn open_modal(&mut self) {
        let scrim = self.overlay(self.scrim, OverlayPlacement::FullViewport);
        let modal = self.overlay(self.panel, OverlayPlacement::Centered);
        self.tree
            .overlay_manager_mut()
            .set_parent_overlay(scrim, Some(modal));
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
    }

    fn centre(&self, id: WidgetId) -> Point {
        self.tree.bounds(id).center()
    }
}

#[test]
fn a_window_button_takes_a_press_through_a_modal_scrim() {
    let mut window = Window::new();
    window.open_modal();

    assert_eq!(
        window.tree.hit_test(window.centre(window.close)),
        Some(window.close)
    );
    window.tree.click(window.close);
    assert_eq!(window.close_taps.get(), 1, "the window button must work");
}

#[test]
fn the_page_stays_behind_the_scrim() {
    let mut window = Window::new();
    window.open_modal();

    assert_eq!(
        window.tree.hit_test(window.centre(window.page_control)),
        Some(window.scrim)
    );
    window.tree.click(window.page_control);
    assert_eq!(window.page_taps.get(), 0);
}

/// A press on the app's title is the drag surface's: the window moves, and
/// the title never sees the press.
#[test]
fn app_content_on_a_drag_surface_is_the_surface() {
    let mut window = Window::new();
    window.open_modal();

    assert_eq!(
        window.tree.hit_test(window.centre(window.surface)),
        Some(window.surface)
    );
    window.tree.click(window.surface);
    assert_eq!(window.surface_taps.get(), 1);
}

/// An app control in the title bar is the app's, behind a dead zone, and
/// stays behind the modal.
#[test]
fn an_app_control_in_the_title_bar_stays_blocked() {
    let mut window = Window::new();
    window.open_modal();

    assert_eq!(
        window.tree.hit_test(window.centre(window.app_control)),
        Some(window.scrim)
    );
    window.tree.click(window.app_control);
    assert_eq!(window.app_control_taps.get(), 0);
}

/// A modal presented without a scrim blocks the page all the same, and
/// leaves the chrome live all the same.
#[test]
fn a_modal_without_a_scrim_leaves_the_chrome_live() {
    let mut window = Window::new();
    window.overlay(window.panel, OverlayPlacement::Centered);
    window.tree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        window.tree.hit_test(window.centre(window.close)),
        Some(window.close)
    );
    assert_eq!(
        window.tree.hit_test(window.centre(window.page_control)),
        None
    );
    assert_eq!(
        window.tree.hit_test(window.centre(window.app_control)),
        None
    );
}

/// Only a modal's scrim lets the chrome through: a full-window layer of
/// any other kind covers it as it covers the page.
#[test]
fn an_ordinary_full_window_layer_covers_the_chrome() {
    let mut window = Window::new();
    window.overlay(window.scrim, OverlayPlacement::FullViewport);
    window.tree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        window.tree.hit_test(window.centre(window.close)),
        Some(window.scrim)
    );
}

/// What a title bar carves out of the caption it gives the OS: every
/// overlay but a modal's scrim, which would otherwise take the whole
/// caption, buttons and all, away from the window system.
#[test]
fn the_scrim_is_not_over_the_chrome() {
    let mut window = Window::new();
    window.open_modal();
    let full = Rect::new(0.0, 0.0, 400.0, 300.0);

    let rects = window.tree.overlay_rects_for_view();

    assert!(rects.all.contains(&full), "a web view still stands down");
    assert!(!rects.over_chrome.contains(&full));
}

/// A pass-through layer over the window (a text editor's touch handles)
/// covers only what in it takes a press. Counted whole, the layer an editor
/// mounted covered the whole caption (on Windows the title bar became client
/// area and offered no Snap layouts) and hid an embedded web view.
#[test]
fn a_pass_through_layer_covers_only_what_takes_a_press() {
    let mut window = Window::new();
    let handle = window.tree.add(FillWidget::new());
    let hidden = window.tree.add(FillWidget::new());
    let layer = window
        .tree
        .add(Row(vec![handle, hidden]).event_pass_through(true));
    window.tree.set_dormant(layer);
    window.overlay(layer, OverlayPlacement::FullViewport);
    window.tree.set_dormant(hidden);
    window.tree.layout(SizeProposal::exact(400.0, 300.0));

    let handle_bounds = window.tree.bounds(handle);
    assert!(handle_bounds.width > 0.0 && handle_bounds.height > 0.0);
    let rects = window.tree.overlay_rects_for_view();
    assert_eq!(rects.over_chrome, vec![handle_bounds]);
    assert_eq!(rects.all, vec![handle_bounds]);

    window.tree.set_dormant(handle);
    window.tree.layout(SizeProposal::exact(400.0, 300.0));
    let rects = window.tree.overlay_rects_for_view();
    assert_eq!(
        rects.over_chrome,
        Vec::<Rect>::new(),
        "an empty layer covers nothing"
    );
    assert_eq!(rects.all, Vec::<Rect>::new());
}
