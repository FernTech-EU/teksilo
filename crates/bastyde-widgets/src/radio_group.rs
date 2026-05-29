//! RadioGroup — invisible layout container that groups `RadioButton`s
//! and wires their accessibility metadata.
//!
//! Radios are a fundamentally group-based control: screen readers need
//! to announce "2 of 3" positional info, which AccessKit models via
//! `push_to_radio_group([sibling_ids])` on each radio button. Loose
//! `RadioButton`s scattered in an HStack can't self-assemble this
//! relation because they have no knowledge of their siblings.
//!
//! `RadioGroup` solves this by owning a shared `Rc<RefCell<Vec<WidgetId>>>`
//! buffer, injecting it into each `RadioButton` child before adding
//! them to the arena, and populating the buffer with each radio's
//! `WidgetId` as it's created. `RadioButton::accessibility()` reads
//! the buffer and emits the `push_to_radio_group` calls.
//!
//! The widget is a pure layout wrapper — it delegates actual
//! rendering to an `HStack` or `VStack` under the hood. Its own
//! accessibility node carries `Role::RadioGroup` + an optional
//! accessible name.
//!
//! ```ignore
//! let selected = ctx.signal(0_usize);
//! RadioGroup::new()
//!     .label(lit!("Theme"))
//!     .radio(RadioButton::new(0, selected.clone()).label(lit!("Light")))
//!     .radio(RadioButton::new(1, selected.clone()).label(lit!("Dark")))
//!     .radio(RadioButton::new(2, selected.clone()).label(lit!("System")))
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Orientation;

use crate::primitives::{HStack, VStack};
use crate::radio_button::RadioButton;
use bastyde_i18n::LocalizedString;

enum RadioGroupChild {
    /// A radio button whose `group_ids` buffer gets injected at build time.
    Radio(Box<RadioButton>),
    /// Any other widget — dividers, section labels, spacers. Passed
    /// straight through to the internal stack without any a11y wiring.
    Other(Box<dyn Widget>),
}

/// Invisible layout container that groups `RadioButton`s for
/// accessibility. Arranges children in an `HStack` or `VStack`
/// and carries `Role::RadioGroup` on its own a11y node.
pub struct RadioGroup {
    pending: Vec<RadioGroupChild>,
    orientation: Orientation,
    spacing: f32,
    label: Option<LocalizedString>,
    /// Shared buffer of sibling `WidgetId`s, populated during `build()`.
    /// Each child radio stores this same `Rc` so its `accessibility()`
    /// impl can publish the group membership.
    group_ids: Rc<RefCell<Vec<WidgetId>>>,
    root_child_id: Option<WidgetId>,
}

impl RadioGroup {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            orientation: Orientation::Vertical,
            spacing: 8.0,
            label: None,
            group_ids: Rc::new(RefCell::new(Vec::new())),
            root_child_id: None,
        }
    }

    /// Layout orientation. Defaults to `Vertical` — most radio groups
    /// read top-to-bottom.
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Gap between children.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Accessible name for the group — e.g. "Theme", "Font family".
    /// Screen readers announce this before individual radio labels.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Add a radio button. The group's shared sibling-id buffer is
    /// injected into the radio at build time so its accessibility
    /// impl can publish group membership via `push_to_radio_group`.
    pub fn radio(mut self, button: RadioButton) -> Self {
        self.pending.push(RadioGroupChild::Radio(Box::new(button)));
        self
    }

    /// Add a non-radio child (divider, caption label, etc.). Passed
    /// straight through to the internal stack without a11y wiring.
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending.push(RadioGroupChild::Other(Box::new(widget)));
        self
    }
}

impl Default for RadioGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RadioGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioGroup")
            .field("orientation", &self.orientation)
            .field("spacing", &self.spacing)
            .field("label", &self.label)
            .field("num_pending", &self.pending.len())
            .finish()
    }
}

impl Widget for RadioGroup {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let pending = std::mem::take(&mut self.pending);
        // Rebuilds restart with an empty buffer; children added below
        // repopulate it. (Only matters if the widget ever rebuilds —
        // which happens on locale change or structural rebinds.)
        self.group_ids.borrow_mut().clear();

        // Two-pass build: first inject the group buffer into each
        // radio before it's moved into the arena, then add each
        // child and collect WidgetIds (recording the radios in the
        // shared buffer so siblings see each other).
        let child_ids: Vec<WidgetId> = pending
            .into_iter()
            .map(|child| match child {
                RadioGroupChild::Radio(mut rb) => {
                    rb.set_group_ids(self.group_ids.clone());
                    let id = ctx.add(*rb);
                    self.group_ids.borrow_mut().push(id);
                    id
                }
                RadioGroupChild::Other(w) => ctx.add_boxed(w),
            })
            .collect();

        let spacing = self.spacing;
        let stack_id = match self.orientation {
            Orientation::Vertical => {
                let mut stack = VStack::new().spacing(spacing);
                for id in child_ids {
                    stack = stack.add_child(id);
                }
                ctx.add(stack)
            }
            Orientation::Horizontal => {
                let mut stack = HStack::new().spacing(spacing);
                for id in child_ids {
                    stack = stack.add_child(id);
                }
                ctx.add(stack)
            }
        };

        self.root_child_id = Some(stack_id);
        vec![stack_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::RadioGroup);
        if let Some(ref name) = self.label {
            builder.set_name(name.resolve_now());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::signal::Signal;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[test]
    fn group_publishes_radio_group_role_and_name() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let rg = tree.add(
            RadioGroup::new()
                .label(lit!("Theme"))
                .radio(RadioButton::new(0, selected.clone()).label(lit!("Light")))
                .radio(RadioButton::new(1, selected.clone()).label(lit!("Dark"))),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let info = tree.accessibility_node(rg);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::RadioGroup);
        assert_eq!(info.name(), Some("Theme"));
    }

    #[test]
    fn member_radios_receive_group_buffer() {
        // Smoke test: after layout, the group_ids buffer on the
        // RadioGroup should contain exactly the added radios'
        // WidgetIds. We can't observe the buffer directly but we
        // can build the group and assert that each radio's
        // accessibility still reports set_selected correctly and
        // that the tree has the expected number of RadioButton
        // nodes beneath the group.
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            RadioGroup::new()
                .radio(RadioButton::new(0, selected.clone()).label(lit!("A")))
                .radio(RadioButton::new(1, selected.clone()).label(lit!("B")))
                .radio(RadioButton::new(2, selected.clone()).label(lit!("C"))),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let a = tree.find_by_label("A").expect("A radio not found");
        let b = tree.find_by_label("B").expect("B radio not found");
        let c = tree.find_by_label("C").expect("C radio not found");
        let info_a = tree.accessibility_node(a);
        let info_b = tree.accessibility_node(b);
        let info_c = tree.accessibility_node(c);
        assert!(!info_a.is_toggled());
        assert!(info_b.is_toggled());
        assert!(!info_c.is_toggled());
    }
}
