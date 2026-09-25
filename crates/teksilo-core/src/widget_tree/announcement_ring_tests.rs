// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The announcement ring against the platform adapters' own rules.
//!
//! Each test builds a live region the way a widget or an application does,
//! changes it, and reads what [`WidgetTree::announcements_since`] recorded.
//! What the ring must record is what all three AccessKit adapters would
//! announce, read out of their sources; see
//! [`crate::accessibility::announcements`]. A test here that expects silence
//! is a place where the ring used to record something no platform says.

use std::cell::Cell;
use std::rc::Rc;

use accesskit::{Live, Role};
use teksilo_canvas::SizeProposal;

use crate::accessibility::AccessNodeBuilder;
use crate::signal::Signal;
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget::{LayoutContext, LayoutResponse, Widget};
use crate::widget_builder::WidgetBuilder;
use crate::widget_tree::WidgetTree;

/// A tree holding what `build` adds, laid out, with its first update already
/// taken. Returns the tree and the sequence number to read from, so a test
/// sees only what its own change produced.
fn settled(build: impl FnOnce(&mut WidgetTree)) -> (WidgetTree, u64) {
    let mut tree = WidgetTree::new();
    build(&mut tree);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.sync_accessibility();
    let seen = tree.announcements_since(0).last().map_or(0, |a| a.seq);
    (tree, seen)
}

/// Run one frame's layout and sync, as `teksilo-app` does, and return what
/// was recorded since `seen`, as `(text, assertive)`, moving `seen` past it.
fn heard(tree: &mut WidgetTree, seen: &mut u64) -> Vec<(String, bool)> {
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.sync_accessibility();
    let got = tree.announcements_since(*seen);
    if let Some(last) = got.last() {
        *seen = last.seq;
    }
    got.into_iter().map(|a| (a.text, a.assertive)).collect()
}

fn polite(text: &str) -> Vec<(String, bool)> {
    vec![(text.to_string(), false)]
}

/// A status line named the way `set_name` names one is heard when the name
/// changes. The control for every silence below: the same node, with its
/// text where the adapters read it.
#[test]
fn a_named_status_is_heard_when_its_name_changes() {
    let text = Signal::new("Prêt".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_label(text.clone())
                .access_live(Live::Polite),
        );
    });
    text.set("Évènement enregistré".to_string());
    assert_eq!(heard(&mut tree, &mut seen), polite("Évènement enregistré"));
}

/// The defect this ring was rebuilt for. A `Status` names itself from its
/// label; a `value` is not a name for any role but `Label`, so the adapters
/// have nothing to announce and stay silent. The ring used to read the value
/// first and record it, so a probe passed over a status nobody heard.
#[test]
fn a_status_that_carries_its_text_only_as_a_value_is_not_heard() {
    let text = Signal::new(String::new());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_value(text.clone())
                .access_live(Live::Polite),
        );
    });
    text.set("Évènement enregistré".to_string());
    assert_eq!(
        heard(&mut tree, &mut seen),
        Vec::new(),
        "no adapter names a Status from its value, so none announces it"
    );
}

/// The same for an `Alert`, whose politeness is assertive: the rule is about
/// where the name is read from, not about the level.
#[test]
fn an_alert_that_carries_its_text_only_as_a_value_is_not_heard() {
    let text = Signal::new(String::new());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Alert)
                .access_value(text.clone())
                .access_live(Live::Assertive),
        );
    });
    text.set("Impossible d'enregistrer".to_string());
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());
}

/// A `Label` is the one role whose name is its value, and a live one is heard
/// through it.
#[test]
fn a_live_label_is_heard_through_its_value() {
    let text = Signal::new("3 évènements".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Label)
                .access_value(text.clone())
                .access_live(Live::Polite),
        );
    });
    text.set("4 évènements".to_string());
    assert_eq!(heard(&mut tree, &mut seen), polite("4 évènements"));
}

/// `common_filter` excludes a hidden node, and no adapter announces a node it
/// has filtered out, whatever its name does.
#[test]
fn a_hidden_live_node_is_not_heard() {
    let text = Signal::new("Prêt".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_label(text.clone())
                .access_live(Live::Polite)
                .access_hidden(true),
        );
    });
    text.set("Évènement enregistré".to_string());
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());
}

/// Hiding reaches the whole subtree: the consumer excludes a node whose
/// ancestor is hidden, even when the node itself says nothing about it.
#[test]
fn a_live_node_inside_a_hidden_subtree_is_not_heard() {
    let text = Signal::new("Prêt".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        let panel = tree.add(
            StackWidget::new()
                .access_role(Role::Group)
                .access_label("Panneau")
                .access_hidden(true),
        );
        tree.add_child(
            panel,
            FillWidget::new()
                .access_role(Role::Status)
                .access_label(text.clone())
                .access_live(Live::Polite),
        );
    });
    text.set("Évènement enregistré".to_string());
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());
}

/// Coming back into the filtered tree is an arrival, and every adapter
/// announces a live node as it arrives, with the name it has had all along.
/// The ring used to wait for the text to change, which it never does here.
#[test]
fn a_live_node_is_heard_again_when_it_is_shown_again() {
    let hidden = Signal::new(false);
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_label("Hors ligne")
                .access_live(Live::Polite)
                .access_hidden(hidden.clone()),
        );
    });
    hidden.set(true);
    assert_eq!(
        heard(&mut tree, &mut seen),
        Vec::new(),
        "leaving says nothing"
    );
    hidden.set(false);
    assert_eq!(heard(&mut tree, &mut seen), polite("Hors ligne"));
}

/// The name is the consumer's, so a live region named after a visible title
/// through `labelled_by` is announced with that title as it arrives.
#[test]
fn a_live_region_named_through_labelled_by_is_heard_by_that_name() {
    let hidden = Signal::new(true);
    let (mut tree, mut seen) = settled(|tree| {
        let title = tree.add(FillWidget::new().label("Erreur de saisie"));
        tree.add(
            FillWidget::new()
                .access_role(Role::Alert)
                .access_live(Live::Assertive)
                .access_labelled_by(title)
                .access_hidden(hidden.clone()),
        );
    });
    hidden.set(false);
    assert_eq!(
        heard(&mut tree, &mut seen),
        vec![("Erreur de saisie".to_string(), true)]
    );
}

/// `live` is inherited: the consumer gives a node without a setting of its own
/// its nearest ancestor's, so a line inside a live region is itself live, at
/// the region's level.
#[test]
fn a_node_inside_a_live_region_is_heard_at_the_regions_level() {
    let text = Signal::new("Aucun résultat".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        let region = tree.add(
            StackWidget::new()
                .access_role(Role::Group)
                .access_label("Recherche")
                .access_live(Live::Assertive),
        );
        tree.add_child(
            region,
            FillWidget::new()
                .access_role(Role::Label)
                .access_value(text.clone()),
        );
    });
    text.set("2 résultats".to_string());
    assert_eq!(
        heard(&mut tree, &mut seen),
        vec![("2 résultats".to_string(), true)]
    );
}

/// A name cleared to nothing leaves nothing to say. AT-SPI sends an empty
/// announcement for it, which no screen reader can speak.
#[test]
fn a_cleared_name_is_not_heard() {
    let text = Signal::new("Prêt".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_label(text.clone())
                .access_live(Live::Polite),
        );
    });
    text.set(String::new());
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());
}

/// The same name twice is not a change, and no adapter speaks it again.
#[test]
fn an_unchanged_name_is_not_heard_twice() {
    let text = Signal::new("Prêt".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_label(text.clone())
                .access_live(Live::Polite),
        );
    });
    text.set("Enregistré".to_string());
    assert_eq!(heard(&mut tree, &mut seen), polite("Enregistré"));
    tree.request_accessibility_update();
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());
}

/// A screen reader already running when the window opens is handed a bare
/// window first, so everything live and named in the first real update
/// arrives, and is announced as it does.
#[test]
fn a_live_node_in_the_first_update_is_heard() {
    let mut tree = WidgetTree::new();
    tree.add(
        FillWidget::new()
            .access_role(Role::Status)
            .access_label("Prêt")
            .access_live(Live::Polite),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let mut seen = 0;
    assert_eq!(heard(&mut tree, &mut seen), polite("Prêt"));
}

/// A live region whose name is set by a widget and whose politeness changes on
/// its own. Only the politeness moves between the two updates.
#[derive(Debug)]
struct Escalating {
    live: Rc<Cell<Live>>,
}

impl Widget for Escalating {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(120.0, 20.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Status);
        builder.set_name("Connexion perdue");
        builder.set_live(self.live.get());
    }
}

/// Windows and macOS announce a live node whose politeness alone changed;
/// AT-SPI does not. The ring records only what every platform says, so a
/// user on Linux is not assumed to have heard it.
#[test]
fn a_change_of_politeness_alone_is_not_heard() {
    let live = Rc::new(Cell::new(Live::Polite));
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(Escalating { live: live.clone() });
    });
    live.set(Live::Assertive);
    tree.request_accessibility_update();
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());
}

/// Several announcements in one update come out in the tree's reading order,
/// whatever order the consumer reported the changes in.
#[test]
fn several_announcements_in_one_update_keep_the_trees_order() {
    let lines: Vec<Signal<String>> = (0..6).map(|_| Signal::new(String::new())).collect();
    let (mut tree, mut seen) = settled(|tree| {
        for line in &lines {
            tree.add(
                FillWidget::new()
                    .access_role(Role::Status)
                    .access_label(line.clone())
                    .access_live(Live::Polite),
            );
        }
    });
    for (index, line) in lines.iter().enumerate() {
        line.set(format!("ligne {index}"));
    }
    let texts: Vec<String> = heard(&mut tree, &mut seen)
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    let expected: Vec<String> = (0..6).map(|index| format!("ligne {index}")).collect();
    assert_eq!(texts, expected);
}

/// The framework's announcers are the root's last children, after the
/// application's content in reading order. A message said through them in the
/// same update as a change of the application's own live region comes after
/// it.
#[test]
fn the_frameworks_announcement_comes_after_the_applications_own() {
    let text = Signal::new("Prêt".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_label(text.clone())
                .access_live(Live::Polite),
        );
    });
    text.set("Enregistré".to_string());
    tree.announce("Évènement enregistré");
    assert_eq!(
        heard(&mut tree, &mut seen),
        vec![
            ("Enregistré".to_string(), false),
            ("Évènement enregistré".to_string(), false),
        ]
    );
}

/// A snapshot is a look at the tree, not a delivery, so it records nothing:
/// a change first seen by a snapshot is still announced by the next sync.
#[test]
fn a_snapshot_records_nothing() {
    let text = Signal::new("Prêt".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_label(text.clone())
                .access_live(Live::Polite),
        );
    });
    text.set("Enregistré".to_string());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.accessibility_tree_snapshot();
    assert!(tree.announcements_since(seen).is_empty());
    assert_eq!(heard(&mut tree, &mut seen), polite("Enregistré"));
}

/// A viewport that clips what it holds, the way a scroll area does, and
/// places its one child `offset` below its top edge. The offset is bound at
/// `Relayout`, so changing it moves the child without re-walking the
/// accessibility tree: the moves-only path a scroll takes.
#[derive(Debug)]
struct Viewport {
    offset: Signal<f32>,
    child: Option<crate::widget_id::WidgetId>,
}

impl Widget for Viewport {
    fn build(
        &mut self,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Vec<crate::widget_id::WidgetId> {
        self.offset.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            crate::binding::BindingLevel::Relayout,
        );
        let child = ctx.add(
            FillWidget::new()
                .access_role(Role::Status)
                .access_label("Nouveau message")
                .access_live(Live::Polite),
        );
        self.child = Some(child);
        vec![child]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        teksilo_canvas::Size::new(100.0, 50.0).into()
    }

    fn children(&self) -> Vec<crate::widget_id::WidgetId> {
        self.child.into_iter().collect()
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y + self.offset.get());
            child.size = teksilo_canvas::Size::new(80.0, 20.0);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::ScrollView);
        builder.set_name("Messages");
        builder.inner_mut().set_clips_children();
    }
}

/// `common_filter` excludes a node its clipping parent has scrolled out of
/// view, and a scroll that brings it back is an arrival every adapter
/// announces. A scroll only moves nodes, and the tree hands the adapters a
/// re-placed copy of its last update rather than a new walk, so the ring has
/// to replay that copy as well, or it misses what the adapters hear.
#[test]
fn a_live_node_scrolled_into_view_is_heard() {
    // The viewport fills the 400 by 300 tree, so 400 is below its bottom edge.
    let offset = Signal::new(400.0_f32);
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(Viewport {
            offset: offset.clone(),
            child: None,
        });
    });
    assert_eq!(
        heard(&mut tree, &mut seen),
        Vec::new(),
        "out of view, the status is filtered out"
    );
    offset.set(10.0);
    assert_eq!(heard(&mut tree, &mut seen), polite("Nouveau message"));
}

/// A named polite status reading `text`.
fn status(text: &Signal<String>) -> impl Widget {
    FillWidget::new()
        .access_role(Role::Status)
        .access_label(text.clone())
        .access_live(Live::Polite)
}

/// A tree that does not record announcements replays nothing and keeps no
/// copy of the tree: the window of an application nobody drives pays for no
/// reader. What it would have said is simply not recorded.
#[test]
fn a_tree_that_does_not_record_replays_nothing() {
    let text = Signal::new("Prêt".to_string());
    let mut tree = WidgetTree::new();
    assert!(tree.records_announcements(), "a new tree records");
    tree.set_records_announcements(false);
    tree.add(status(&text));
    let mut seen = 0;
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());
    text.set("Enregistré".to_string());
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());
    assert!(!tree.announcement_ring.holds_a_replay());
}

/// Recording switched on part way records the changes from then on. What the
/// tree already held when it was switched on was announced as it arrived, to
/// whoever was listening then, and is not recorded as arriving now.
#[test]
fn recording_switched_on_hears_what_changes_after() {
    let text = Signal::new("Prêt".to_string());
    let mut tree = WidgetTree::new();
    tree.set_records_announcements(false);
    tree.add(status(&text));
    let mut seen = 0;
    assert_eq!(heard(&mut tree, &mut seen), Vec::new());

    tree.set_records_announcements(true);
    tree.request_accessibility_update();
    assert_eq!(
        heard(&mut tree, &mut seen),
        Vec::new(),
        "no arrival replayed"
    );
    text.set("Enregistré".to_string());
    assert_eq!(heard(&mut tree, &mut seen), polite("Enregistré"));
}

/// Switched off and on again with no update in between, the replay still
/// matches the adapters, and the next change is heard.
#[test]
fn recording_switched_off_and_on_between_updates_misses_nothing() {
    let text = Signal::new("Prêt".to_string());
    let (mut tree, mut seen) = settled(|tree| {
        tree.add(status(&text));
    });
    tree.set_records_announcements(false);
    tree.set_records_announcements(true);
    text.set("Enregistré".to_string());
    assert_eq!(heard(&mut tree, &mut seen), polite("Enregistré"));
}
