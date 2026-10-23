// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Intents between a window and the application tree.
//!
//! An intent goes from the widget that sent it to the root, then to the
//! global actions of its own tree. A command fired in a window that only the
//! application tree implements (an application menu's "New window" with no
//! window action for it) would stop there, and so would one the application
//! tree sends for a window to carry out. After every dispatch the event loop
//! takes what each tree left unclaimed
//! ([`WidgetTree::take_unclaimed_intents`](teksilo_core::WidgetTree::take_unclaimed_intents))
//! and offers it to the other side once:
//!
//! - a window's to the application tree;
//! - the application tree's to the window the user works with: the most
//!   recent one, at its frontmost native dialog when it has one.
//!
//! The receiving tree dispatches it from its focus
//! ([`WidgetTree::dispatch_routed_intent`](teksilo_core::WidgetTree::dispatch_routed_intent))
//! and does not keep it as unclaimed again, so it never travels back to its
//! origin or further. One nobody takes is dropped, and a debug build names it
//! once. A shortcut's intent is never offered: the tree that heard the key
//! hands it to the focused widget instead.

use std::collections::HashSet;

use teksilo_core::{Intent, WidgetTree};

/// Offer `intents` to `to`, the tree on the other side, oldest first.
/// Returns the names of those it did not take, or of all of them when there
/// is no tree to offer them to.
pub(crate) fn offer(
    intents: Vec<Intent>,
    to: Option<&mut WidgetTree>,
    ops: &mut dyn teksilo_core::WindowOps,
) -> Vec<&'static str> {
    let Some(to) = to else {
        return intents.into_iter().map(|intent| intent.name).collect();
    };
    intents
        .into_iter()
        .filter_map(|intent| {
            let name = intent.name;
            (!to.dispatch_routed_intent(intent, &mut *ops)).then_some(name)
        })
        .collect()
}

/// Names a debug build has already reported dropped, so each is said once.
#[derive(Debug, Default)]
pub(crate) struct DroppedIntents {
    reported: HashSet<&'static str>,
}

impl DroppedIntents {
    /// Report `names`, sent `from` one side, that the other side did not
    /// take either: once per name, in debug builds.
    pub(crate) fn report(&mut self, from: &str, names: Vec<&'static str>) {
        for name in names {
            if self.reported.insert(name) {
                #[cfg(debug_assertions)]
                eprintln!(
                    "teksilo-app: intent `{name}`, sent in {from}, was taken by no action there \
                     or on the other side (the application tree and the windows offer each \
                     other what they leave); it was dropped"
                );
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = from;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use teksilo_canvas::{Size, SizeProposal};
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::{Action, BuildContext, NoopWindowOps, WidgetId};

    use super::*;

    /// Takes `takes` with a global action, records it in `log` under
    /// `label`, and on it sends `then` if any.
    #[derive(Debug)]
    struct Taker {
        label: &'static str,
        takes: &'static str,
        then: Option<&'static str>,
        log: Rc<RefCell<Vec<String>>>,
    }

    impl Widget for Taker {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let (label, then, log) = (self.label, self.then, self.log.clone());
            ctx.register_action_global(Action::new(self.takes).on_invoke(move |intent, ctx| {
                log.borrow_mut().push(format!("{label}:{}", intent.name));
                if let Some(then) = then {
                    ctx.send_intent(Intent::new(then));
                }
            }));
            Vec::new()
        }

        fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            Size::new(0.0, 0.0).into()
        }
    }

    fn tree(
        label: &'static str,
        takes: &'static str,
        then: Option<&'static str>,
        log: &Rc<RefCell<Vec<String>>>,
    ) -> WidgetTree {
        let mut tree = WidgetTree::new();
        tree.set_records_unclaimed_intents(true);
        tree.add(Taker {
            label,
            takes,
            then,
            log: log.clone(),
        });
        tree
    }

    fn send(tree: &mut WidgetTree, name: &'static str) {
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| ctx.send_intent(Intent::new(name)));
    }

    #[test]
    fn a_windows_intent_reaches_the_application_tree() {
        let log = Rc::default();
        let mut window = tree("window", "doc.save", None, &log);
        let mut app = tree("app", "app.new_window", None, &log);
        send(&mut window, "app.new_window");
        let dropped = offer(
            window.take_unclaimed_intents(),
            Some(&mut app),
            &mut NoopWindowOps,
        );
        assert!(dropped.is_empty());
        assert_eq!(*log.borrow(), ["app:app.new_window"]);
    }

    #[test]
    fn the_application_trees_intent_reaches_the_window() {
        let log = Rc::default();
        let mut window = tree("window", "doc.save", None, &log);
        let mut app = tree("app", "app.new_window", None, &log);
        send(&mut app, "doc.save");
        let dropped = offer(
            app.take_unclaimed_intents(),
            Some(&mut window),
            &mut NoopWindowOps,
        );
        assert!(dropped.is_empty());
        assert_eq!(*log.borrow(), ["window:doc.save"]);
    }

    /// Nobody takes it: it is dropped, and does not come back to its origin
    /// to be offered again.
    #[test]
    fn an_intent_nobody_takes_is_dropped_after_one_offer() {
        let log = Rc::default();
        let mut window = tree("window", "doc.save", None, &log);
        let mut app = tree("app", "app.new_window", None, &log);
        send(&mut window, "nobody.takes");
        let dropped = offer(
            window.take_unclaimed_intents(),
            Some(&mut app),
            &mut NoopWindowOps,
        );
        assert_eq!(dropped, ["nobody.takes"]);
        assert!(
            app.take_unclaimed_intents().is_empty(),
            "not kept to travel on"
        );
        assert!(window.take_unclaimed_intents().is_empty());
    }

    /// What the receiving side's handler sends is its own, and makes its own
    /// way: here back to the window that sent the first one.
    #[test]
    fn what_a_routed_intents_handler_sends_is_the_receivers_own() {
        let log = Rc::default();
        let mut window = tree("window", "doc.refresh", None, &log);
        let mut app = tree("app", "app.reload", Some("doc.refresh"), &log);
        send(&mut window, "app.reload");
        offer(
            window.take_unclaimed_intents(),
            Some(&mut app),
            &mut NoopWindowOps,
        );
        let back = app.take_unclaimed_intents();
        assert_eq!(
            back.iter().map(|i| i.name).collect::<Vec<_>>(),
            ["doc.refresh"]
        );
        offer(back, Some(&mut window), &mut NoopWindowOps);
        assert_eq!(*log.borrow(), ["app:app.reload", "window:doc.refresh"]);
    }

    #[test]
    fn with_no_tree_on_the_other_side_everything_is_dropped() {
        let log = Rc::default();
        let mut app = tree("app", "app.new_window", None, &log);
        send(&mut app, "doc.save");
        assert_eq!(
            offer(app.take_unclaimed_intents(), None, &mut NoopWindowOps),
            ["doc.save"]
        );
    }

    #[test]
    fn a_dropped_name_is_reported_once() {
        let mut dropped = DroppedIntents::default();
        dropped.report("a window", vec!["x", "x", "y"]);
        dropped.report("the application tree", vec!["x"]);
        assert_eq!(dropped.reported.len(), 2);
    }
}
