// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A node's description, computed from what it is `described_by`
//! (`accessibility_description_impl`).
//!
//! The shape is a form field and the line under it that says what is wrong
//! with it. [`Ears`] hears the updates the way the readers do, from what their
//! sources say (see the module docs of `accessibility_description_impl`):
//! every reader speaks an announcement, and reads the focused node's name and
//! description as focus arrives on it; Orca also speaks a change to the
//! description of the node it holds as focus, which is still the old one while
//! it hears the changes of the update focus leaves in. What a reader does with
//! an announcement made as focus moves differs, Orca cutting it and NVDA
//! keeping it, and the tree's rule differs with it, so a test that meets that
//! moment holds the tree and the ears to each reader in turn. Otherwise "said
//! once" is asserted against the strictest ear.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use accesskit::{Live, NodeId, Role};
use teksilo_canvas::SizeProposal;

use crate::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use crate::announcer::Politeness;
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget::{LayoutContext, LayoutResponse, Widget};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;
use crate::widget_tree::accessibility_description_impl::AnnouncementOnFocusMove;

use AnnouncementOnFocusMove::{Cut, Kept};

/// Both readers' handling of an announcement made as focus moves.
const READERS: [AnnouncementOnFocusMove; 2] = [Kept, Cut];

/// Hold `tree` to `reader`'s rule rather than the host's.
fn for_reader(tree: &mut WidgetTree, reader: AnnouncementOnFocusMove) {
    tree.description_memory.on_focus_move = reader;
}

/// The line under a field: silent while empty, and when it holds a message a
/// `Role::Status` named with it, live or not, the way `ValidationStrip` and an
/// application's own silenced line publish themselves.
#[derive(Debug)]
struct Line {
    text: Rc<RefCell<String>>,
    live: Rc<Cell<Live>>,
}

impl Widget for Line {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Status);
        let text = self.text.borrow();
        if text.is_empty() {
            builder.set_live(Live::Off);
        } else {
            builder.set_name(text.as_str());
            builder.set_live(self.live.get());
        }
    }
}

/// A form: two fields, the first described by a line.
struct Form {
    tree: WidgetTree,
    field: WidgetId,
    other: WidgetId,
    line: WidgetId,
    message: Rc<RefCell<String>>,
    live: Rc<Cell<Live>>,
}

impl Form {
    fn new(live: Live) -> Self {
        Self::for_reader(live, AnnouncementOnFocusMove::PLATFORM)
    }

    fn for_reader(live: Live, reader: AnnouncementOnFocusMove) -> Self {
        let mut tree = WidgetTree::new();
        for_reader(&mut tree, reader);
        let message = Rc::new(RefCell::new(String::new()));
        let live = Rc::new(Cell::new(live));
        let line = tree.add(Line {
            text: message.clone(),
            live: live.clone(),
        });
        let field = tree.add(
            FillWidget::new()
                .focusable()
                .access_role(Role::TextInput)
                .access_label("Title")
                .access_described_by(line),
        );
        let other = tree.add(
            FillWidget::new()
                .focusable()
                .access_role(Role::Button)
                .access_label("Save"),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        Self {
            tree,
            field,
            other,
            line,
            message,
            live,
        }
    }

    /// Put `text` in the line, as a validator or an application does.
    fn say(&mut self, text: &str) {
        *self.message.borrow_mut() = text.to_owned();
        self.tree.request_accessibility_update();
    }

    fn description(&mut self) -> Option<String> {
        let update = self.tree.sync_accessibility();
        description_of(&update, self.field)
    }
}

fn description_of(update: &accesskit::TreeUpdate, id: WidgetId) -> Option<String> {
    let nid = widget_id_to_node_id(id);
    // Through the consumer every adapter is built on, so what is asserted is
    // what an adapter hands the platform, not a builder's intermediate.
    fn find(node: accesskit_consumer::NodeRef<'_>, nid: NodeId) -> Option<String> {
        if node.locate().0 == nid {
            return node.description();
        }
        node.children().find_map(|child| find(child, nid))
    }
    let tree = accesskit_consumer::Tree::new(update.clone(), true);
    find(tree.state().root(), nid)
}

/// What the readers say, update by update.
struct Ears {
    reader: AnnouncementOnFocusMove,
    heard: Vec<String>,
    /// Begun, then cut by the read of a new focus: never whole, and a
    /// fragment of a message is noise, not the message.
    cut: Vec<String>,
    seq: u64,
    focus: Option<NodeId>,
    focus_description: Option<String>,
}

impl Ears {
    fn new(reader: AnnouncementOnFocusMove) -> Self {
        Self {
            reader,
            heard: Vec::new(),
            cut: Vec::new(),
            seq: 0,
            focus: None,
            focus_description: None,
        }
    }

    /// Sync the tree and hear what the delivered update says.
    fn hear(&mut self, tree: &mut WidgetTree) {
        let update = tree.sync_accessibility();
        let description_of_node = |id: NodeId| {
            update
                .nodes
                .iter()
                .find(|(node_id, _)| *node_id == id)
                .and_then(|(_, node)| node.description())
                .map(str::to_owned)
        };
        // Everything the adapters send before the focus move
        // (`accesskit_consumer` `tree.rs:640-673`): the announcements, and a
        // change to the description of the node the reader still holds as
        // its focus, which Orca speaks if it is not empty.
        let mut before_focus: Vec<String> = Vec::new();
        for announcement in tree.announcements_since(self.seq) {
            self.seq = announcement.seq;
            before_focus.push(announcement.text);
        }
        if let Some(held) = self.focus {
            let description = description_of_node(held);
            if description != self.focus_description {
                before_focus.extend(description);
            }
        }
        if self.focus != Some(update.focus) {
            match self.reader {
                Kept => self.heard.append(&mut before_focus),
                Cut => self.cut.append(&mut before_focus),
            }
            // Arrival: every reader reads the name, then the description.
            let node = update
                .nodes
                .iter()
                .find(|(id, _)| *id == update.focus)
                .map(|(_, node)| node);
            if let Some(name) = node.and_then(|node| node.label()) {
                self.heard.push(name.to_owned());
            }
            self.heard.extend(description_of_node(update.focus));
        } else {
            self.heard.append(&mut before_focus);
        }
        self.focus = Some(update.focus);
        self.focus_description = description_of_node(update.focus);
    }

    fn times(&self, text: &str) -> usize {
        self.heard.iter().filter(|said| said.contains(text)).count()
    }

    fn cut_times(&self, text: &str) -> usize {
        self.cut.iter().filter(|said| said.contains(text)).count()
    }
}

/// The relation is written into the description, which is the one property
/// every adapter reads. Before this, the field published the relation alone,
/// and the consumer answered `None`.
#[test]
fn the_line_a_field_points_at_is_its_description() {
    let mut form = Form::new(Live::Off);
    form.say("Must be unique");
    assert_eq!(form.description().as_deref(), Some("Must be unique"));

    // The relation stays: it is the right one, and the reader that follows it
    // (Orca) reads its targets only when the description is empty.
    let update = form.tree.sync_accessibility();
    let field = update
        .nodes
        .iter()
        .find(|(id, _)| *id == widget_id_to_node_id(form.field))
        .map(|(_, node)| node)
        .expect("the field is in the tree");
    assert_eq!(field.described_by(), &[widget_id_to_node_id(form.line)]);
}

/// Away from the field, the description follows the line as it changes, and
/// an empty line describes nothing.
#[test]
fn the_description_follows_the_line_while_focus_is_elsewhere() {
    let mut form = Form::new(Live::Assertive);
    form.tree.focus(form.other);
    assert_eq!(form.description(), None, "an empty line says nothing");

    form.say("Too short");
    assert_eq!(form.description().as_deref(), Some("Too short"));
    form.say("Already taken");
    assert_eq!(form.description().as_deref(), Some("Already taken"));
    form.say("");
    assert_eq!(form.description(), None);
}

/// Coming back to a field that was refused: the reason is read with it. This
/// is what WCAG 3.3.1 asks of the relation, and what nobody heard before.
#[test]
fn arriving_at_a_field_reads_what_its_line_says() {
    for reader in READERS {
        let mut form = Form::for_reader(Live::Assertive, reader);
        let mut ears = Ears::new(reader);
        form.tree.focus(form.other);
        ears.hear(&mut form.tree);
        form.say("Too short");
        ears.hear(&mut form.tree);
        ears.hear(&mut form.tree);
        assert_eq!(
            ears.times("Too short"),
            1,
            "the line announced itself, {reader:?}: {:?}",
            ears.heard
        );

        form.tree.focus(form.field);
        ears.hear(&mut form.tree);
        assert_eq!(
            ears.times("Too short"),
            2,
            "arriving at the field reads its reason, {reader:?}: {:?}",
            ears.heard
        );
    }
}

/// The field validates while the user is in it: the line announces, and the
/// field's description does not change under the user, which Orca would speak
/// a second time. It arrives with the next visit.
#[test]
fn a_message_that_appears_while_the_field_has_focus_is_said_once() {
    for reader in READERS {
        let mut form = Form::for_reader(Live::Assertive, reader);
        let mut ears = Ears::new(reader);
        form.tree.focus(form.field);
        ears.hear(&mut form.tree);

        form.say("Too short");
        ears.hear(&mut form.tree);
        // Anything else that rebuilds the tree while focus stays.
        form.tree.request_accessibility_update();
        ears.hear(&mut form.tree);
        form.tree.request_accessibility_update();
        ears.hear(&mut form.tree);
        assert_eq!(
            ears.times("Too short"),
            1,
            "{reader:?}: heard {:?}",
            ears.heard
        );
        assert_eq!(form.description(), None, "it waits for the next arrival");

        form.tree.focus(form.other);
        ears.hear(&mut form.tree);
        form.tree.focus(form.field);
        ears.hear(&mut form.tree);
        assert_eq!(
            ears.times("Too short"),
            2,
            "and is read on that arrival, {reader:?}: {:?}",
            ears.heard
        );
    }
}

/// Leaving the field is not a moment to say its message: the adapters send
/// the field's changes before the focus move, while Orca still holds the field
/// as its focus, so a description that took the held-back message on the way
/// out would be spoken, and cut by the next field's name. The field takes it
/// in the update after, which the tree asks for itself, by which time Orca's
/// focus is elsewhere.
#[test]
fn leaving_a_field_does_not_start_saying_its_message() {
    for reader in READERS {
        let mut form = Form::for_reader(Live::Assertive, reader);
        let mut ears = Ears::new(reader);
        form.tree.focus(form.field);
        ears.hear(&mut form.tree);
        form.say("Too short");
        ears.hear(&mut form.tree);

        form.tree.focus(form.other);
        ears.hear(&mut form.tree);
        assert_eq!(
            (ears.times("Too short"), ears.cut_times("Too short")),
            (1, 0),
            "said by the line, and not begun again on the way out, {reader:?}; \
             heard {:?}, cut {:?}",
            ears.heard,
            ears.cut
        );

        // Nothing asks for this update but the tree itself.
        assert_eq!(
            form.description().as_deref(),
            Some("Too short"),
            "the field has its message once focus is elsewhere, {reader:?}"
        );
    }
}

/// The message appears and focus is sent to the field in the same update, the
/// WCAG pattern of focusing the first error. Said once by either reader, and
/// by nothing after while focus stays: NVDA keeps the line's announcement, so
/// the arrival leaves the message out; Orca cuts the announcement to read the
/// field, so the arrival is what says it.
#[test]
fn a_message_announced_as_focus_arrives_is_said_once() {
    for reader in READERS {
        let mut form = Form::for_reader(Live::Assertive, reader);
        let mut ears = Ears::new(reader);
        form.tree.focus(form.other);
        ears.hear(&mut form.tree);

        form.say("Too short");
        form.tree.focus(form.field);
        ears.hear(&mut form.tree);
        form.tree.request_accessibility_update();
        ears.hear(&mut form.tree);
        assert_eq!(
            ears.times("Too short"),
            1,
            "{reader:?}: heard {:?}",
            ears.heard
        );
        assert_eq!(ears.times("Title"), 1, "the arrival itself was heard");
    }
}

/// The same, where the voice is not the line but the application's own
/// announcement: a line kept silent (`Live::Off`), and `announce_with` saying
/// the same sentence as focus is sent to the field. The announcer is a live
/// region like any other, so the arrival leaves its sentence out where the
/// reader keeps the announcement, and says it where the reader cuts it.
#[test]
fn an_announcement_saying_the_line_as_focus_arrives_is_said_once() {
    for reader in READERS {
        let mut form = Form::for_reader(Live::Off, reader);
        let mut ears = Ears::new(reader);
        form.tree.focus(form.other);
        ears.hear(&mut form.tree);

        form.say("Enter a title");
        form.tree
            .announce_with("Enter a title", Politeness::Assertive);
        form.tree.focus(form.field);
        // Expose, then retract.
        ears.hear(&mut form.tree);
        ears.hear(&mut form.tree);
        assert_eq!(
            ears.times("Enter a title"),
            1,
            "{reader:?}: heard {:?}",
            ears.heard
        );

        // The same refusal again, from the button: the line is unchanged, the
        // announcement is not, and the arrival says it once with it.
        form.tree.focus(form.other);
        ears.hear(&mut form.tree);
        form.tree
            .announce_with("Enter a title", Politeness::Assertive);
        form.tree.focus(form.field);
        ears.hear(&mut form.tree);
        ears.hear(&mut form.tree);
        assert_eq!(
            ears.times("Enter a title"),
            2,
            "{reader:?}: heard {:?}",
            ears.heard
        );

        // And coming back later, with nobody announcing, reads it.
        form.tree.focus(form.other);
        ears.hear(&mut form.tree);
        form.tree.focus(form.field);
        ears.hear(&mut form.tree);
        assert_eq!(
            ears.times("Enter a title"),
            3,
            "{reader:?}: heard {:?}",
            ears.heard
        );
    }
}

/// A description never says what is no longer true: a message that goes away
/// while focus stays is dropped at once.
#[test]
fn a_message_that_goes_away_leaves_the_description_while_focus_stays() {
    let mut form = Form::new(Live::Assertive);
    form.tree.focus(form.other);
    form.say("Too short");
    let _ = form.description();
    form.tree.focus(form.field);
    assert_eq!(form.description().as_deref(), Some("Too short"));

    form.say("");
    assert_eq!(form.description(), None);
}

/// A replaced message while focus stays: the new one is announced, the old one
/// leaves the description, and the new one waits for the next arrival.
#[test]
fn a_replaced_message_is_announced_and_not_described_while_focus_stays() {
    let mut form = Form::new(Live::Assertive);
    let mut ears = Ears::new(AnnouncementOnFocusMove::PLATFORM);
    form.tree.focus(form.other);
    form.say("Too short");
    ears.hear(&mut form.tree);
    form.tree.focus(form.field);
    ears.hear(&mut form.tree);

    form.say("Already taken");
    ears.hear(&mut form.tree);
    form.tree.request_accessibility_update();
    ears.hear(&mut form.tree);
    assert_eq!(ears.times("Already taken"), 1, "heard {:?}", ears.heard);
    assert_eq!(form.description(), None);
}

/// A description the node wrote itself comes first, and the line's text
/// after it; a line saying the node's own name is not said again.
#[test]
fn the_nodes_own_description_comes_first_and_its_name_is_not_repeated() {
    let mut tree = WidgetTree::new();
    let error = tree.add(FillWidget::new().label("Too short"));
    let name = tree.add(FillWidget::new().label("Password"));
    let field = tree.add(
        FillWidget::new()
            .focusable()
            .access_role(Role::PasswordInput)
            .access_label("Password")
            .access_description_literal("At least eight characters")
            .access_described_by(name)
            .access_described_by(error),
    );
    tree.layout(SizeProposal::exact(200.0, 100.0));
    let update = tree.sync_accessibility();
    assert_eq!(
        description_of(&update, field).as_deref(),
        Some("At least eight characters Too short")
    );
}

/// A target with no name of its own contributes the text it contains, in
/// reading order and without its hidden parts; a hidden target still counts,
/// since pointing at hidden text is how a description is kept off the reading
/// path.
#[test]
fn a_target_says_what_it_contains_and_counts_even_hidden() {
    let mut tree = WidgetTree::new();
    let first = tree.add(FillWidget::new().label("Between 1 and 31."));
    let secret = tree.add(FillWidget::new().label("never read").access_hidden(true));
    let second = tree.add(FillWidget::new().label("Check the month."));
    let group = tree.add(StackWidget::new().child(first).child(secret).child(second));
    let hidden = tree.add(
        FillWidget::new()
            .label("Day of the month")
            .access_hidden(true),
    );
    let field = tree.add(
        FillWidget::new()
            .focusable()
            .access_role(Role::SpinButton)
            .access_label("Day")
            .access_described_by(hidden)
            .access_described_by(group),
    );
    tree.layout(SizeProposal::exact(200.0, 100.0));
    let update = tree.sync_accessibility();
    assert_eq!(
        description_of(&update, field).as_deref(),
        Some("Day of the month Between 1 and 31. Check the month.")
    );
}

/// A surface that carries its text in runs and nothing else, as the code
/// editor and the log do.
#[derive(Debug)]
struct Document(&'static str);

impl Widget for Document {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Document);
        builder.push_text_run_child_on_self(0, self.0.to_owned(), vec![1; self.0.len()], None);
    }
}

/// A document's text is not a description: a target that holds its text in
/// runs alone is a surface like the log, whose whole visible text would be
/// read on every arrival. What the field is pointed at beside it still counts.
#[test]
fn a_document_a_field_points_at_is_not_read_out_as_its_description() {
    let mut tree = WidgetTree::new();
    let log = tree.add(Document("error: expected one of `;` or `}`"));
    let hint = tree.add(FillWidget::new().label("See the build log."));
    let field = tree.add(
        FillWidget::new()
            .focusable()
            .access_role(Role::TextInput)
            .access_label("Command")
            .access_described_by(log)
            .access_described_by(hint),
    );
    tree.layout(SizeProposal::exact(200.0, 100.0));
    let update = tree.sync_accessibility();
    assert_eq!(
        description_of(&update, field).as_deref(),
        Some("See the build log.")
    );
}

/// A live region under a hidden node is out of the tree every adapter reads
/// (`common_filter` drops the hidden node's whole subtree), so it announces
/// nothing, and a text only it says is not kept out of the arrival, even where
/// the reader keeps an announcement.
#[test]
fn a_live_region_nobody_can_hear_does_not_keep_its_text_out_of_the_arrival() {
    let mut tree = WidgetTree::new();
    for_reader(&mut tree, Kept);
    let message = Rc::new(RefCell::new(String::new()));
    let line = tree.add(Line {
        text: message.clone(),
        live: Rc::new(Cell::new(Live::Assertive)),
    });
    let panel = tree.add(StackWidget::new().child(line).access_hidden(true));
    let field = tree.add(
        FillWidget::new()
            .focusable()
            .access_role(Role::TextInput)
            .access_label("Title")
            .access_described_by(line),
    );
    let other = tree.add(FillWidget::new().focusable().access_label("Save"));
    let _ = panel;
    tree.layout(SizeProposal::exact(200.0, 100.0));
    tree.focus(other);
    let _ = tree.sync_accessibility();

    *message.borrow_mut() = "Too short".to_owned();
    tree.focus(field);
    let update = tree.sync_accessibility();
    assert_eq!(description_of(&update, field).as_deref(), Some("Too short"));
}

/// A text node that speaks only through the politeness it inherits: a label
/// named by its value, setting no politeness of its own.
#[derive(Debug)]
struct Quiet {
    text: Rc<RefCell<String>>,
    role: Role,
}

impl Widget for Quiet {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(self.role);
        let text = self.text.borrow();
        if !text.is_empty() {
            builder.inner_mut().set_value(text.as_str());
        }
    }
}

/// A field described by `target`, with a second control holding focus first.
/// Returns the tree, the field, and that other control.
fn field_described_by(tree: &mut WidgetTree, target: WidgetId) -> (WidgetId, WidgetId) {
    let field = tree.add(
        FillWidget::new()
            .focusable()
            .access_role(Role::TextInput)
            .access_label("Title")
            .access_described_by(target),
    );
    let other = tree.add(FillWidget::new().focusable().access_label("Save"));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    tree.focus(other);
    let _ = tree.sync_accessibility();
    (field, other)
}

/// A live `Status` that holds its text only as a value has no name, so no
/// adapter announces it: the text is not being said to anybody, and the
/// arrival must say it, whether the reader keeps an announcement or cuts it.
#[test]
fn a_status_holding_its_text_as_a_value_does_not_keep_it_out_of_the_arrival() {
    for reader in READERS {
        let mut tree = WidgetTree::new();
        for_reader(&mut tree, reader);
        let message = Rc::new(RefCell::new(String::new()));
        let line = tree.add(
            Quiet {
                text: message.clone(),
                role: Role::Status,
            }
            .access_live(Live::Assertive),
        );
        let (field, _) = field_described_by(&mut tree, line);

        *message.borrow_mut() = "Too short".to_owned();
        tree.focus(field);
        let update = tree.sync_accessibility();
        assert_eq!(
            description_of(&update, field).as_deref(),
            Some("Too short"),
            "{reader:?}"
        );
    }
}

/// A label inside a live region is announced by every adapter at the
/// region's politeness, as it appears (consumer `node.rs:906-910`). Where the
/// reader keeps that announcement through the focus move (NVDA), the arrival
/// leaves the text out, or it is said twice; where it cuts it (Orca), the
/// arrival says it.
#[test]
fn a_label_live_through_its_region_is_said_once_as_focus_arrives() {
    for (reader, arrival) in [(Kept, None), (Cut, Some("Too short"))] {
        let mut tree = WidgetTree::new();
        for_reader(&mut tree, reader);
        let message = Rc::new(RefCell::new(String::new()));
        let line = tree.add(Quiet {
            text: message.clone(),
            role: Role::Label,
        });
        let _region = tree.add(StackWidget::new().child(line).access_live(Live::Assertive));
        let (field, _) = field_described_by(&mut tree, line);

        *message.borrow_mut() = "Too short".to_owned();
        tree.request_accessibility_update();
        tree.focus(field);
        let update = tree.sync_accessibility();
        assert_eq!(
            description_of(&update, field).as_deref(),
            arrival,
            "{reader:?}"
        );
    }
}

/// A node that names itself with `text` once there is some, in `role`.
#[derive(Debug)]
struct Named {
    text: Rc<RefCell<String>>,
    role: Role,
}

impl Widget for Named {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(self.role);
        let text = self.text.borrow();
        if !text.is_empty() {
            builder.inner_mut().set_label(text.as_str());
        }
    }
}

/// `common_filter` drops a `GenericContainer` whatever its name (consumer
/// `filters.rs:30-33`), and an adapter announces only a node the filter
/// includes, so a named, live container is announced by none of them. Its
/// text is being said to nobody, and the arrival says it for every reader.
#[test]
fn a_live_container_no_adapter_announces_does_not_keep_its_text_out_of_the_arrival() {
    for reader in READERS {
        let mut tree = WidgetTree::new();
        for_reader(&mut tree, reader);
        let message = Rc::new(RefCell::new(String::new()));
        let line = tree.add(
            Named {
                text: message.clone(),
                role: Role::GenericContainer,
            }
            .access_live(Live::Assertive),
        );
        let (field, _) = field_described_by(&mut tree, line);

        *message.borrow_mut() = "Too short".to_owned();
        tree.request_accessibility_update();
        tree.focus(field);
        let update = tree.sync_accessibility();
        assert_eq!(
            description_of(&update, field).as_deref(),
            Some("Too short"),
            "{reader:?}"
        );
    }
}

/// A container that is a live region at a politeness that can change, and
/// holds one child.
#[derive(Debug)]
struct Region {
    live: Rc<Cell<Live>>,
    child: WidgetId,
}

impl Widget for Region {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_live(self.live.get());
    }
}

/// Windows announces a node whose own politeness changes, but a region that
/// turns assertive changes nothing in the label it holds, and the consumer
/// reports a node as changed only when its own data changes: nobody says the
/// label again, so the arrival must.
#[test]
fn a_region_turning_assertive_does_not_say_its_label_again() {
    for reader in READERS {
        let mut tree = WidgetTree::new();
        for_reader(&mut tree, reader);
        let message = Rc::new(RefCell::new("Check the date".to_owned()));
        let live = Rc::new(Cell::new(Live::Polite));
        let line = tree.add(Quiet {
            text: message.clone(),
            role: Role::Label,
        });
        let _region = tree.add(Region {
            live: live.clone(),
            child: line,
        });
        let (field, _) = field_described_by(&mut tree, line);

        live.set(Live::Assertive);
        tree.request_accessibility_update();
        tree.focus(field);
        let update = tree.sync_accessibility();
        assert_eq!(
            description_of(&update, field).as_deref(),
            Some("Check the date"),
            "{reader:?}"
        );
    }
}

/// Windows announces a live region whose politeness alone changes
/// (`accesskit_windows` `adapter.rs:313-324`), so a message raised from polite
/// to assertive as focus arrives is being said to NVDA, and the arrival leaves
/// it out. AT-SPI announces no such change (`accesskit_atspi_common`
/// `node.rs:610-622` keys on the name), and Orca would cut it if it did, so
/// there the arrival says it.
#[test]
fn a_message_raised_to_assertive_as_focus_arrives_is_said_once() {
    for (reader, arrival) in [(Kept, None), (Cut, Some("Check the date"))] {
        let mut form = Form::for_reader(Live::Polite, reader);
        form.tree.focus(form.other);
        form.say("Check the date");
        let _ = form.description();

        form.live.set(Live::Assertive);
        form.tree.request_accessibility_update();
        form.tree.focus(form.field);
        assert_eq!(form.description().as_deref(), arrival, "{reader:?}");
    }
}

/// The rules hold for every node focus is inside, not only the one it is on:
/// Orca reads a dialog's description as focus enters it (`formatting.py`: a
/// `DIALOG` ancestor is spoken in its `focused` format, `labelOrName +
/// roleName + (unrelatedLabelsOrDescription)`). A dialog described by a line
/// that announces as focus lands inside it reads the line on entry exactly
/// where a field would (not where the reader keeps the announcement, yes where
/// it cuts it), gains nothing while focus moves around inside, and has it when
/// focus next enters.
#[test]
fn a_dialog_focus_enters_follows_the_same_rules_as_the_field() {
    for (reader, entry) in [(Kept, None), (Cut, Some("This cannot be undone"))] {
        let mut tree = WidgetTree::new();
        for_reader(&mut tree, reader);
        let message = Rc::new(RefCell::new(String::new()));
        let line = tree.add(Line {
            text: message.clone(),
            live: Rc::new(Cell::new(Live::Assertive)),
        });
        let first = tree.add(FillWidget::new().focusable().access_label("Yes"));
        let second = tree.add(FillWidget::new().focusable().access_label("No"));
        let dialog = tree.add(
            StackWidget::new()
                .child(first)
                .child(second)
                .access_role(Role::Dialog)
                .access_label("Delete?")
                .access_described_by(line),
        );
        let outside = tree.add(FillWidget::new().focusable().access_label("Delete"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(outside);
        let _ = tree.sync_accessibility();

        *message.borrow_mut() = "This cannot be undone".to_owned();
        tree.focus(first);
        let update = tree.sync_accessibility();
        assert_eq!(
            description_of(&update, dialog).as_deref(),
            entry,
            "announced as focus entered, {reader:?}"
        );
        tree.focus(second);
        let update = tree.sync_accessibility();
        assert_eq!(
            description_of(&update, dialog).as_deref(),
            entry,
            "focus stayed inside, {reader:?}"
        );

        tree.focus(outside);
        let _ = tree.sync_accessibility();
        tree.focus(first);
        let update = tree.sync_accessibility();
        assert_eq!(
            description_of(&update, dialog).as_deref(),
            Some("This cannot be undone"),
            "{reader:?}"
        );
    }
}
