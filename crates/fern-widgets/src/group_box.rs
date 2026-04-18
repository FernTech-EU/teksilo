//! GroupBox — titled cluster of controls in Int UI / Jewel style.
//!
//! A bold title (optionally preceded by a checkbox) sits above an indented
//! content area. No border, no frame — pure composition.
//!
//! In checkable mode, unchecking disables event dispatch to every descendant
//! of the content area (via `ctx.enabled_when` with ancestor propagation) AND
//! paints a translucent surface overlay over the content so it reads as
//! greyed-out. The title checkbox itself stays interactive.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::Checkbox;
use crate::primitives::{HStack, Padding, RectWidget, TextWidget, VStack, ZStack};

pub struct GroupBox {
    title: String,
    checked: Option<Signal<bool>>,
    pending_content: Option<Box<dyn Widget>>,
    content_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
}

impl GroupBox {
    pub fn new(title: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = title.into();
        Self {
            title: ls.resolve_now(),
            checked: None,
            pending_content: None,
            content_id: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw title in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(title: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(title))
    }

    /// Turn this into a checkable GroupBox. When the signal is `false`, events
    /// to descendants of the content area are blocked via effective-enabled
    /// ancestor propagation. The title checkbox itself stays interactive.
    pub fn checkable(mut self, checked: Signal<bool>) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Set the content widget inline (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_content = Some(Box::new(widget));
        self
    }

    /// Set the content widget by pre-registered ID.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.content_id = Some(id);
        self
    }
}

impl std::fmt::Debug for GroupBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupBox")
            .field("title", &self.title)
            .field("checkable", &self.checked.is_some())
            .finish()
    }
}

impl Widget for GroupBox {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(ctx.add_boxed(pending));
        }

        let theme = ctx.theme().clone();
        let style = theme.components.group_box;

        let title_label = TextWidget::new_literal(&self.title)
            .style(theme.typography.body_bold.clone())
            .color(theme.colors.text_primary)
            .single_line()
            .a11y_hidden();

        let title_row_id = if let Some(ref checked) = self.checked {
            let checkbox = Checkbox::new(checked.clone());
            ctx.add(
                HStack::new()
                    .spacing(style.checkbox_gap)
                    .child(checkbox)
                    .child(title_label),
            )
        } else {
            ctx.add(title_label)
        };

        let padded_content_id = if let Some(content_id) = self.content_id {
            ctx.add(Padding::new(0.0, 0.0, 0.0, style.content_indent).child_id(content_id))
        } else {
            ctx.add(Padding::new(0.0, 0.0, 0.0, style.content_indent))
        };

        // When checkable and unchecked, lay a translucent surface tint over
        // the padded content so it reads as greyed-out. The dispatcher-level
        // ancestor-enabled check already blocks interaction; this overlay is
        // purely a visual cue.
        let content_wrapper_id = if let Some(ref checked) = self.checked {
            let dim_color = theme.colors.surface_main.with_alpha(0.6);
            let dim_overlay_id = ctx.add(RectWidget::new().background(dim_color));
            ctx.visible_when(dim_overlay_id, checked.map(|v| !*v));
            ctx.enabled_when(padded_content_id, checked.clone());
            ctx.add(
                ZStack::new()
                    .add_child(padded_content_id)
                    .add_child(dim_overlay_id),
            )
        } else {
            padded_content_id
        };

        let root = ctx.add(
            VStack::new()
                .spacing(style.title_content_spacing)
                .add_child(title_row_id)
                .add_child(content_wrapper_id),
        );
        self.root_child_id = Some(root);

        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return size;
        }
        proposal.resolve(0.0, 0.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Group);
        builder.set_name(&self.title);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

