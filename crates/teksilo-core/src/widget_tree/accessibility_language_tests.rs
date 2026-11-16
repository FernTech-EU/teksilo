// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `content_language`: the declaration, how it is inherited, and what reaches
//! the accessibility tree and the widgets.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use accesskit::Role;
use teksilo_canvas::{Canvas, Rect, SizeProposal};

use crate::accessibility::text_runs::{TextRunSource, push_text_runs};
use crate::accessibility::{AccessNodeBuilder, root_node_id, widget_id_to_node_id};
use crate::language::LanguageTag;
use crate::overlay::{DismissBehavior, OverlayPlacement, OverlayRequest};
use crate::signal::Signal;
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

fn tag(s: &str) -> LanguageTag {
    LanguageTag::parse(s).unwrap()
}

/// A window in French.
fn french_tree() -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.set_locale("fr-FR".to_owned());
    tree
}

fn lay_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

fn node(update: &accesskit::TreeUpdate, id: WidgetId) -> Option<&accesskit::Node> {
    let nid = widget_id_to_node_id(id);
    update.nodes.iter().find(|(n, _)| *n == nid).map(|(_, n)| n)
}

fn language_of(update: &accesskit::TreeUpdate, id: WidgetId) -> Option<String> {
    node(update, id)
        .expect("the node is in the update")
        .language()
        .map(str::to_owned)
}

/// The language a platform adapter reads for `id`, through the consumer.
fn effective(update: &accesskit::TreeUpdate, id: WidgetId) -> Option<String> {
    crate::accessibility::audit::effective_languages(update)
        .get(&widget_id_to_node_id(id))
        .cloned()
}

/// A named container: a node the walker keeps.
#[derive(Debug)]
struct Group {
    children: Vec<WidgetId>,
}

impl Group {
    fn new(children: Vec<WidgetId>) -> Self {
        Self { children }
    }
}

impl Widget for Group {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Group);
        builder.set_name("group");
    }
}

/// A label that emits text runs, as every visible label does.
#[derive(Debug)]
struct Text(&'static str);

impl Widget for Text {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Label);
        builder.set_name(self.0);
        push_text_runs(builder, None, &TextRunSource::flat(self.0, 0));
    }
}

/// Records the language every context it is handed reports.
#[derive(Debug, Default, Clone)]
struct Seen {
    layout: Rc<RefCell<Vec<Option<String>>>>,
    place: Rc<RefCell<Vec<Option<String>>>>,
    paint: Rc<RefCell<Vec<Option<String>>>>,
}

#[derive(Debug)]
struct Probe {
    seen: Seen,
}

impl Widget for Probe {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.seen
            .layout
            .borrow_mut()
            .push(ctx.content_language().map(|t| t.as_str().to_owned()));
        proposal.resolve(10.0, 10.0).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.seen
            .place
            .borrow_mut()
            .push(ctx.content_language().map(|t| t.as_str().to_owned()));
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, ctx: &PaintContext) {
        self.seen
            .paint
            .borrow_mut()
            .push(ctx.content_language.as_ref().map(|t| t.as_str().to_owned()));
    }
}

/// A container that measures its child in `layout_response`, as a stack
/// does, handing it its own context.
#[derive(Debug)]
struct Measuring {
    child: WidgetId,
}

impl Widget for Measuring {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        ctx.child_layout_response(self.child, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

#[test]
fn a_declared_node_carries_its_language_and_the_root_keeps_the_locale() {
    let mut tree = french_tree();
    let label = tree.add(
        FillWidget::new()
            .label("日本語")
            .content_language(tag("ja")),
    );
    let plain = tree.add(FillWidget::new().label("Bonjour"));
    tree.add(StackWidget::new().child(label).child(plain));
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    let root = update
        .nodes
        .iter()
        .find(|(id, _)| *id == root_node_id())
        .map(|(_, n)| n)
        .unwrap();
    assert_eq!(root.language(), Some("fr-FR"));
    assert_eq!(language_of(&update, label).as_deref(), Some("ja"));
    assert_eq!(
        language_of(&update, plain),
        None,
        "undeclared content inherits the window's locale; nothing is written on it"
    );
    assert_eq!(effective(&update, plain).as_deref(), Some("fr-FR"));
}

#[test]
fn an_inner_declaration_overrides_and_a_sibling_inherits_without_a_write() {
    let mut tree = french_tree();
    let inner = tree.add(
        FillWidget::new()
            .label("日本語")
            .content_language(tag("ja")),
    );
    let sibling = tree.add(FillWidget::new().label("مرحبا"));
    let group = tree.add(Group::new(vec![inner, sibling]).content_language(tag("ar")));
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    assert_eq!(language_of(&update, group).as_deref(), Some("ar"));
    assert_eq!(language_of(&update, inner).as_deref(), Some("ja"));
    assert_eq!(language_of(&update, sibling), None);
    assert_eq!(effective(&update, sibling).as_deref(), Some("ar"));
}

#[test]
fn a_declaration_equal_to_the_inherited_language_writes_nothing() {
    let mut tree = french_tree();
    let child = tree.add(FillWidget::new().label("نص").content_language(tag("ar")));
    let group = tree.add(Group::new(vec![child]).content_language(tag("ar")));
    let same_as_window = tree.add(
        FillWidget::new()
            .label("Texte")
            .content_language(tag("fr-fr")),
    );
    tree.add(StackWidget::new().child(group).child(same_as_window));
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    assert_eq!(language_of(&update, group).as_deref(), Some("ar"));
    assert_eq!(language_of(&update, child), None);
    assert_eq!(
        language_of(&update, same_as_window),
        None,
        "a declaration of the window's own locale changes nothing"
    );
}

#[test]
fn a_pruned_container_stays_pruned_and_its_children_carry_its_language() {
    let mut tree = french_tree();
    let a = tree.add(FillWidget::new().label("一"));
    let b = tree.add(FillWidget::new().label("二"));
    let stack = tree.add(
        StackWidget::new()
            .child(a)
            .child(b)
            .content_language(tag("ja")),
    );
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    assert!(
        node(&update, stack).is_none(),
        "a layout container is pruned whatever language it declares"
    );
    assert_eq!(language_of(&update, a).as_deref(), Some("ja"));
    assert_eq!(language_of(&update, b).as_deref(), Some("ja"));
}

#[test]
fn a_bound_declaration_follows_its_signal() {
    let mut tree = french_tree();
    let language: Signal<Option<LanguageTag>> = Signal::new(None);
    let field = tree.add(
        FillWidget::new()
            .label("suggestion")
            .content_language(language.clone()),
    );
    lay_out(&mut tree);
    let update = tree.sync_accessibility();
    assert_eq!(language_of(&update, field), None);

    language.set(Some(tag("ar")));
    lay_out(&mut tree);
    assert!(tree.a11y_dirty, "a language change re-walks the tree");
    let update = tree.sync_accessibility();
    assert_eq!(language_of(&update, field).as_deref(), Some("ar"));

    language.set(Some(tag("ja-JP")));
    lay_out(&mut tree);
    let update = tree.sync_accessibility();
    assert_eq!(language_of(&update, field).as_deref(), Some("ja-JP"));

    language.set(None);
    lay_out(&mut tree);
    let update = tree.sync_accessibility();
    assert_eq!(language_of(&update, field), None);
    assert_eq!(effective(&update, field).as_deref(), Some("fr-FR"));
}

#[test]
fn a_signal_of_a_tag_is_always_a_declaration() {
    let mut tree = french_tree();
    let language = Signal::new(tag("he"));
    let field = tree.add(
        FillWidget::new()
            .label("שלום")
            .content_language(language.clone()),
    );
    lay_out(&mut tree);
    assert_eq!(
        language_of(&tree.sync_accessibility(), field).as_deref(),
        Some("he")
    );
    language.set(tag("yi"));
    lay_out(&mut tree);
    assert_eq!(
        language_of(&tree.sync_accessibility(), field).as_deref(),
        Some("yi")
    );
}

#[test]
fn overlay_content_inherits_its_anchors_language() {
    let mut tree = french_tree();
    let anchor = tree.add(FillWidget::new().label("champ").content_language(tag("ja")));
    tree.add(StackWidget::new().child(anchor));
    let row = tree.add(FillWidget::new().label("候補"));
    let content = tree.add(StackWidget::new().child(row));
    tree.show_overlay(OverlayRequest::new(
        content,
        anchor,
        OverlayPlacement::Below,
        DismissBehavior::Manual,
    ));
    lay_out(&mut tree);

    assert_eq!(tree.content_language(row), Some(tag("ja")));
    let update = tree.sync_accessibility();
    assert_eq!(
        language_of(&update, row).as_deref(),
        Some("ja"),
        "overlay content hangs off the window root, so it is written there"
    );
}

#[test]
fn an_overlay_whose_content_declares_keeps_its_own() {
    let mut tree = french_tree();
    let anchor = tree.add(FillWidget::new().label("champ").content_language(tag("ja")));
    tree.add(StackWidget::new().child(anchor));
    let row = tree.add(FillWidget::new().label("العربية"));
    let content = tree.add(StackWidget::new().child(row).content_language(tag("ar")));
    tree.show_overlay(OverlayRequest::new(
        content,
        anchor,
        OverlayPlacement::Below,
        DismissBehavior::Manual,
    ));
    lay_out(&mut tree);
    assert_eq!(tree.content_language(row), Some(tag("ar")));
}

#[test]
fn an_overlay_anchored_inside_its_own_content_does_not_loop() {
    let mut tree = french_tree();
    let row = tree.add(FillWidget::new().label("row"));
    let content = tree.add(StackWidget::new().child(row));
    tree.show_overlay(OverlayRequest::new(
        content,
        row,
        OverlayPlacement::Below,
        DismissBehavior::Manual,
    ));
    // Something else declares, so the walk runs at all.
    tree.add(FillWidget::new().content_language(tag("de")));
    lay_out(&mut tree);
    assert_eq!(tree.content_language(row), None);
    let _ = tree.sync_accessibility();
}

#[test]
fn a_merged_node_carries_the_merging_widgets_language() {
    let mut tree = french_tree();
    let inner = tree.add(
        FillWidget::new()
            .label("日本語")
            .content_language(tag("ja")),
    );
    let card = tree.add(
        Group::new(vec![inner])
            .content_language(tag("ar"))
            .access_merge_subtree(),
    );
    lay_out(&mut tree);
    let update = tree.sync_accessibility();
    assert!(node(&update, inner).is_none(), "merged away");
    assert_eq!(language_of(&update, card).as_deref(), Some("ar"));
}

#[test]
fn a_text_run_reports_the_declared_language_through_the_consumer() {
    // `NodeRef::language()` is what every platform adapter reads, so this
    // stands in for the three of them.
    let mut tree = french_tree();
    let label = tree.add(Text("日本語のテキスト").content_language(tag("ja")));
    let plain = tree.add(Text("texte"));
    tree.add(StackWidget::new().child(label).child(plain));
    lay_out(&mut tree);
    let update = tree.sync_accessibility();

    let infos = crate::accessibility::audit::text_infos(&update);
    let info = infos.get(&widget_id_to_node_id(label)).expect("runs");
    assert_eq!(info.language.as_deref(), Some("ja"));
    assert_eq!(
        infos
            .get(&widget_id_to_node_id(plain))
            .and_then(|i| i.language.as_deref()),
        Some("fr-FR")
    );

    let runs: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == Role::TextRun)
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(runs.len(), 2);
    let languages = crate::accessibility::audit::effective_languages(&update);
    let run_languages: Vec<_> = runs.iter().map(|r| languages.get(r).cloned()).collect();
    assert!(run_languages.contains(&Some("ja".to_owned())));
    assert!(run_languages.contains(&Some("fr-FR".to_owned())));
}

#[test]
fn a_tree_with_no_declaration_resolves_without_a_walk_and_counts_removals() {
    let mut tree = french_tree();
    let a = tree.add(FillWidget::new().label("a"));
    tree.add(StackWidget::new().child(a));
    lay_out(&mut tree);
    assert_eq!(tree.arena.content_language_count, 0);
    assert_eq!(tree.content_language(a), None);

    let declared = tree.add(FillWidget::new().content_language(tag("ja")));
    assert_eq!(tree.arena.content_language_count, 1);
    tree.destroy_subtree(declared);
    assert_eq!(
        tree.arena.content_language_count, 0,
        "removing a declaring node gives the fast path back"
    );
}

#[test]
fn withdrawing_and_redeclaring_keeps_the_count() {
    let mut tree = french_tree();
    let a = tree.add(FillWidget::new().label("a"));
    tree.set_content_language(a, Some(tag("ja").into()));
    tree.set_content_language(a, Some(tag("ko").into()));
    assert_eq!(tree.arena.content_language_count, 1);
    assert_eq!(tree.content_language(a), Some(tag("ko")));
    tree.set_content_language(a, None);
    assert_eq!(tree.arena.content_language_count, 0);
    assert_eq!(tree.content_language(a), None);
}

#[test]
fn widgets_read_their_language_in_layout_and_paint() {
    let mut tree = french_tree();
    let seen = Seen::default();
    let inside = tree.add(Probe { seen: seen.clone() });
    let outside_seen = Seen::default();
    let outside = tree.add(Probe {
        seen: outside_seen.clone(),
    });
    let measuring = tree.add(Measuring { child: inside }.content_language(tag("ja")));
    tree.add(StackWidget::new().child(measuring).child(outside));
    lay_out(&mut tree);
    tree.render();

    let ja = Some("ja".to_owned());
    assert!(!seen.layout.borrow().is_empty());
    assert!(
        seen.layout.borrow().iter().all(|l| *l == ja),
        "measured through its parent's context or its own, the child reads ja: {:?}",
        seen.layout.borrow()
    );
    assert_eq!(*seen.place.borrow(), vec![ja.clone()]);
    assert_eq!(*seen.paint.borrow(), vec![ja]);
    assert!(outside_seen.layout.borrow().iter().all(Option::is_none));
    assert_eq!(*outside_seen.paint.borrow(), vec![None]);
}

#[test]
fn a_language_change_repaints_the_subtree() {
    let mut tree = french_tree();
    let language: Signal<Option<LanguageTag>> = Signal::new(Some(tag("ja")));
    let seen = Seen::default();
    let leaf = tree.add(Probe { seen: seen.clone() });
    let middle = tree.add(StackWidget::new().child(leaf));
    tree.add(
        StackWidget::new()
            .child(middle)
            .content_language(language.clone()),
    );
    lay_out(&mut tree);
    tree.render();
    tree.render();
    assert_eq!(
        seen.paint.borrow().len(),
        1,
        "a clean leaf paints from cache"
    );

    language.set(Some(tag("zh-Hant")));
    lay_out(&mut tree);
    tree.render();
    assert_eq!(
        *seen.paint.borrow(),
        vec![Some("ja".to_owned()), Some("zh-Hant".to_owned())],
        "a leaf two levels down repaints with the new language"
    );
    // `StackWidget` never measures its children, so the leaf hears of the
    // change through `place_children`, which every widget gets each pass.
    assert_eq!(
        seen.place.borrow().last().cloned().flatten().as_deref(),
        Some("zh-Hant")
    );
}

#[test]
fn overlay_content_lays_out_with_its_anchors_language() {
    let mut tree = french_tree();
    let anchor = tree.add(FillWidget::new().content_language(tag("ar")));
    tree.add(StackWidget::new().child(anchor));
    let seen = Seen::default();
    let probe = tree.add(Probe { seen: seen.clone() });
    let content = tree.add(StackWidget::new().child(probe));
    tree.show_overlay(OverlayRequest::new(
        content,
        anchor,
        OverlayPlacement::Below,
        DismissBehavior::Manual,
    ));
    lay_out(&mut tree);
    tree.render();
    assert_eq!(
        seen.place.borrow().last().cloned().flatten().as_deref(),
        Some("ar")
    );
    assert_eq!(
        seen.paint.borrow().last().cloned().flatten().as_deref(),
        Some("ar")
    );
}

#[test]
fn a_handler_reads_its_widgets_language() {
    let mut tree = french_tree();
    let read: Rc<RefCell<Option<Option<String>>>> = Rc::new(RefCell::new(None));
    let sink = read.clone();
    let button = tree.add(FillWidget::new().on_tap(move |_event, ctx| {
        *sink.borrow_mut() = Some(ctx.content_language().map(|t| t.as_str().to_owned()));
    }));
    tree.add(StackWidget::new().child(button).content_language(tag("ko")));
    lay_out(&mut tree);
    tree.click(button);
    assert_eq!(*read.borrow(), Some(Some("ko".to_owned())));
}

#[test]
fn a_declaration_applied_from_build_reaches_the_node() {
    #[derive(Debug)]
    struct SelfDeclaring {
        built: Rc<Cell<u32>>,
    }
    impl Widget for SelfDeclaring {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            self.built.set(self.built.get() + 1);
            ctx.apply_self_handlers(
                crate::widget_builder::HandlerSet::new().content_language(tag("el")),
            );
            Vec::new()
        }
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(Role::Label);
            builder.set_name("Καλημέρα");
        }
    }
    let built = Rc::new(Cell::new(0));
    let mut tree = french_tree();
    let id = tree.add(SelfDeclaring {
        built: built.clone(),
    });
    lay_out(&mut tree);
    assert_eq!(built.get(), 1);
    assert_eq!(tree.content_language(id), Some(tag("el")));
    assert_eq!(
        language_of(&tree.sync_accessibility(), id).as_deref(),
        Some("el")
    );
    assert_eq!(tree.arena.content_language_count, 1);
}

/// A row or cell wrapper, as a data view builds one around the app's
/// delegate: a role and no name of its own.
#[derive(Debug)]
struct Wrapper {
    role: Role,
    child: WidgetId,
}

impl Widget for Wrapper {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(self.role);
    }
}

/// A delegate's row widget as `StandardListItem` is one: a name and no role
/// of its own.
#[derive(Debug)]
struct RowContent(&'static str);

impl Widget for RowContent {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_name(self.0);
    }
}

#[test]
fn a_row_named_from_its_content_is_read_in_that_contents_language() {
    let mut tree = french_tree();
    let endonym = tree.add(RowContent("日本語").content_language(tag("ja")));
    let row = tree.add(Wrapper {
        role: Role::ListBoxOption,
        child: endonym,
    });
    let plain = tree.add(RowContent("français"));
    let plain_row = tree.add(Wrapper {
        role: Role::ListBoxOption,
        child: plain,
    });
    tree.add(Group::new(vec![row, plain_row]));
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    assert_eq!(
        node(&update, row).and_then(|n| n.label()),
        Some("日本語"),
        "the row is named from its content"
    );
    assert_eq!(
        language_of(&update, row).as_deref(),
        Some("ja"),
        "and its name is read in the content's language"
    );
    assert_eq!(
        language_of(&update, endonym),
        None,
        "the content inherits it from the row"
    );
    assert_eq!(language_of(&update, plain_row), None);
}

#[test]
fn a_cell_named_after_its_label_is_read_in_the_labels_language() {
    let _on = super::accessibility_cell_name_impl::CellNamingOverride::new(true);
    let mut tree = french_tree();
    let text = tree.add(FillWidget::new().label("مرحبا").content_language(tag("ar")));
    let cell = tree.add(Wrapper {
        role: Role::Cell,
        child: text,
    });
    tree.add(Group::new(vec![cell]));
    lay_out(&mut tree);

    let update = tree.sync_accessibility();
    assert_eq!(
        node(&update, cell).map(|n| n.labelled_by().to_vec()),
        Some(vec![widget_id_to_node_id(text)])
    );
    assert_eq!(language_of(&update, cell).as_deref(), Some("ar"));
}
