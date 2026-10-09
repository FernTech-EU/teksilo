// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! LiveImage through the umbrella crate: the producer types the prelude
//! brings, beside the widget catalog's own re-exports of them (spec J.12),
//! and the three forms a `teksu!` tree builds a live picture in (C.16).

/// Both globs name `LiveImageSource`, `LiveImageWriter`, `LivePixelFormat`
/// and `PixelRect`. They are one item each, so naming them is no ambiguity.
mod prelude_beside_the_catalog {
    use teksilo::prelude::*;
    use teksilo::widgets::*;

    #[test]
    fn j12_the_live_image_types_resolve_through_both_globs() {
        let source = LiveImageSource::new(LivePixelFormat::Bgrx8);
        let writer: LiveImageWriter = source.writer();
        assert_eq!(PixelRect::full(1, 1).area(), 1);
        writer.write_frame(1, 1, &[0, 0, 0, 0], 4).unwrap();
        assert_eq!(source.size(), Some((1, 1)));
        // And the widget, from the catalog's glob, takes the prelude's source.
        let _view = LiveImage::new(source).alt("Screen");
    }
}

mod teksu_forms {
    use std::cell::RefCell;
    use std::rc::Rc;

    use teksilo::canvas::SizeProposal;
    use teksilo::core::widget::LayoutResponse;
    use teksilo::core::{LayoutContext, Widget, WidgetId, WidgetTree};
    use teksilo::prelude::*;
    use teksilo::widgets::{
        ImageFit, LiveImage, LiveImageHandle, LiveImageSource, LivePixelFormat, ScalingFilter,
        VStack,
    };

    /// Builds what its function returns, as an app's root does.
    struct Root(
        Box<dyn FnMut(&mut BuildContext) -> WidgetId>,
        Option<WidgetId>,
    );

    impl std::fmt::Debug for Root {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Root")
        }
    }

    impl Widget for Root {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let id = (self.0)(ctx);
            self.1 = Some(id);
            vec![id]
        }

        fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            p.resolve(200.0, 200.0).into()
        }

        fn children(&self) -> Vec<WidgetId> {
            self.1.into_iter().collect()
        }
    }

    fn source() -> LiveImageSource {
        let source = LiveImageSource::new(LivePixelFormat::Rgba8);
        let writer = source.writer();
        writer.write_frame(4, 2, &[255; 4 * 2 * 4], 4 * 4).unwrap();
        source
    }

    /// Build `root` in a tree and lay it out.
    fn mount(root: impl FnMut(&mut BuildContext) -> WidgetId + 'static) -> WidgetTree {
        let mut tree = WidgetTree::new().with_theme(intui::light());
        tree.add(Root(Box::new(root), None));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        tree
    }

    #[test]
    fn c16_properties_lower_to_the_builder() {
        let shown = source();
        let id = Rc::new(RefCell::new(None));
        let seen = id.clone();
        let tree = mount(move |ctx| {
            let src = shown.clone();
            let built = teksu!(ctx => LiveImage(src) {
                    fit: ImageFit::Cover
                    scaling: ScalingFilter::Nearest
                    alt: lit!("Screen")
                }
            );
            *seen.borrow_mut() = Some(built);
            built
        });
        let id = id.borrow().expect("built");
        let repr = tree.widget_debug_string(id).expect("a repr");
        assert!(repr.contains("fit: Cover"), "{repr}");
        assert!(repr.contains("scaling: Nearest"), "{repr}");
    }

    #[test]
    fn c16_a_handle_made_first_reaches_the_tree() {
        let shown = source();
        let handle = LiveImageHandle::new();
        let given = handle.clone();
        let _tree = mount(move |ctx| {
            let src = shown.clone();
            teksu!(ctx => VStack {
                    LiveImage(src) {
                        with_handle: &given
                        alt: lit!("Screen")
                    }
                }
            )
        });
        assert!(
            handle.widget_id().is_some(),
            "the handle follows the widget"
        );
        assert!(handle.geometry().is_some());
    }

    #[test]
    fn c16_an_escape_carries_a_widget_built_in_rust() {
        let shown = source();
        let mapped = Rc::new(RefCell::new(None));
        let record = mapped.clone();
        let handle_out: Rc<RefCell<Option<LiveImageHandle>>> = Rc::default();
        let keep = handle_out.clone();
        let mut tree = mount(move |ctx| {
            let live = LiveImage::new(shown.clone()).alt(lit!("Screen"));
            let handle = live.handle();
            *keep.borrow_mut() = Some(handle.clone());
            let record = record.clone();
            let view = live.on_pointer_event(move |event, _ctx| {
                if let WidgetEvent::PointerDown { position, .. } = event {
                    *record.borrow_mut() = handle.map_to_source(*position);
                }
                EventResponse::Handled
            });
            teksu!(ctx => VStack {
                #{ view }
            })
        });
        let handle = handle_out.borrow().clone().expect("the handle");
        let id = handle.widget_id().expect("mounted");
        // The 4 x 2 picture fills the column's 200 px width: a press at the
        // top-leading quarter's centre is pixel (0, 0).
        let b = tree.bounds(id);
        tree.pointer_down_button(
            teksilo::canvas::Point::new(b.x + b.width / 8.0, b.y + b.height / 4.0),
            PointerButton::Primary,
        );
        assert_eq!(*mapped.borrow(), Some((0, 0)));
    }
}
