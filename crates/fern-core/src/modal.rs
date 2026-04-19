use crate::widget_id::WidgetId;

/// How the framework should present a modal surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModalPresentation {
    /// Let the framework pick the most appropriate backend for the runtime.
    #[default]
    Auto,
    /// Present inside the current widget tree using the overlay system.
    InTree,
    /// Present in a separate native OS window.
    NativeWindow,
}

/// How a presented modal can be closed by framework-managed interactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalCloseBehavior {
    /// Close when clicking outside the modal surface.
    ClickOutside,
    /// Close when pressing Escape.
    EscapeKey,
    /// Close on either Escape or an outside click.
    EscapeOrClickOutside,
    /// Only close through explicit application logic.
    Manual,
}

impl Default for ModalCloseBehavior {
    fn default() -> Self {
        Self::EscapeOrClickOutside
    }
}

/// Builder used to create modal content in a target widget tree later.
pub type ModalBuilder = Box<dyn FnOnce(&mut crate::widget_tree::WidgetTree) -> WidgetId>;

/// Modal content source.
pub enum ModalContent {
    /// Reuse an already-inserted widget subtree in the current tree.
    ExistingWidget(WidgetId),
    /// Build the modal content into a target widget tree on demand.
    Deferred(ModalBuilder),
}

impl std::fmt::Debug for ModalContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExistingWidget(id) => f.debug_tuple("ExistingWidget").field(id).finish(),
            Self::Deferred(_) => f.write_str("Deferred(..)"),
        }
    }
}

/// A framework-level request to present a modal.
#[derive(Debug)]
pub struct ModalRequest {
    pub content: ModalContent,
    pub presentation: ModalPresentation,
    pub close_behavior: ModalCloseBehavior,
    pub title: Option<String>,
    pub size: Option<(u32, u32)>,
    /// Optional explicit initial-focus target inside the modal content
    /// subtree. When `None`, the framework falls back to
    /// `first_focusable_descendant(content_id)`. When `Some`, the id
    /// is consulted first and the framework focuses it if the widget
    /// exists and is still active in the target tree; otherwise it
    /// falls back to `first_focusable_descendant`. Required for
    /// `MessageBox`-style alerts where the default button may not be
    /// the first focusable descendant in tree-walk order.
    pub focus_target: Option<WidgetId>,
}

impl ModalRequest {
    /// Present an existing widget subtree as modal content.
    pub fn in_tree(content_id: WidgetId) -> Self {
        Self {
            content: ModalContent::ExistingWidget(content_id),
            presentation: ModalPresentation::Auto,
            close_behavior: ModalCloseBehavior::default(),
            title: None,
            size: None,
            focus_target: None,
        }
    }

    /// Build modal content on demand in the presentation target tree.
    pub fn deferred(
        builder: impl FnOnce(&mut crate::widget_tree::WidgetTree) -> WidgetId + 'static,
    ) -> Self {
        Self {
            content: ModalContent::Deferred(Box::new(builder)),
            presentation: ModalPresentation::Auto,
            close_behavior: ModalCloseBehavior::default(),
            title: None,
            size: None,
            focus_target: None,
        }
    }

    pub fn presentation(mut self, presentation: ModalPresentation) -> Self {
        self.presentation = presentation;
        self
    }

    pub fn close_behavior(mut self, close_behavior: ModalCloseBehavior) -> Self {
        self.close_behavior = close_behavior;
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.size = Some((width, height));
        self
    }

    /// Direct initial focus to a specific widget inside the modal
    /// content subtree. The id must resolve to a widget that exists
    /// and is active at the time the modal is presented; if it does
    /// not, the framework falls back to
    /// `first_focusable_descendant(content_id)`.
    pub fn focus_target(mut self, id: WidgetId) -> Self {
        self.focus_target = Some(id);
        self
    }
}

/// A modal request drained from a widget tree with its originating widget.
#[derive(Debug)]
pub struct QueuedModalRequest {
    pub source_widget: WidgetId,
    pub request: ModalRequest,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;

    #[test]
    fn in_tree_request_defaults_to_auto() {
        let mut tree = crate::WidgetTree::new();
        let content_id = tree.add(FillWidget::new());
        let request = ModalRequest::in_tree(content_id);
        assert_eq!(request.presentation, ModalPresentation::Auto);
        assert_eq!(
            request.close_behavior,
            ModalCloseBehavior::EscapeOrClickOutside
        );
        match request.content {
            ModalContent::ExistingWidget(id) => assert_eq!(id, content_id),
            ModalContent::Deferred(_) => panic!("expected ExistingWidget content"),
        }
    }

    #[test]
    fn deferred_request_can_override_metadata() {
        let request =
            ModalRequest::deferred(|tree| tree.add(crate::test_widgets::FillWidget::new()))
                .presentation(ModalPresentation::NativeWindow)
                .close_behavior(ModalCloseBehavior::Manual)
                .title("Preferences")
                .size(640, 480);

        assert_eq!(request.presentation, ModalPresentation::NativeWindow);
        assert_eq!(request.close_behavior, ModalCloseBehavior::Manual);
        assert_eq!(request.title.as_deref(), Some("Preferences"));
        assert_eq!(request.size, Some((640, 480)));
        assert!(matches!(request.content, ModalContent::Deferred(_)));
    }

    #[test]
    fn focus_target_defaults_to_none() {
        let mut tree = crate::WidgetTree::new();
        let content_id = tree.add(FillWidget::new());
        let in_tree = ModalRequest::in_tree(content_id);
        assert!(in_tree.focus_target.is_none());

        let deferred =
            ModalRequest::deferred(|tree| tree.add(crate::test_widgets::FillWidget::new()));
        assert!(deferred.focus_target.is_none());
    }

    #[test]
    fn focus_target_builder_sets_field() {
        let mut tree = crate::WidgetTree::new();
        let content_id = tree.add(FillWidget::new());
        let target_id = tree.add(FillWidget::new());
        let request = ModalRequest::in_tree(content_id).focus_target(target_id);
        assert_eq!(request.focus_target, Some(target_id));
    }
}
