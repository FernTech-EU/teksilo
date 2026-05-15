//! Default `ListContainerStyle` impl.
//!
//! IntUI's list/tree container chrome is just the drag-insertion
//! indicator. The recipe ships a `RectWidget`-leaf factory for
//! custom recipes that want a composed indicator, and a
//! `ListInsertionRecipe` (`BorderRole::Accent`, 2 dp thickness) for
//! the inline paint path the widgets use today.

use fern_core::build_context::BuildContext;
use fern_core::styles::{
    ListContainerStyle, ListInsertionConfig, ListInsertionRecipe, SharedListContainerStyle,
};
use fern_core::widget_id::WidgetId;
use fern_tokens::BorderRole;

use crate::primitives::RectWidget;

/// Default `ListContainerStyle` shipped with FernUI.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeListContainerStyle;

impl ListContainerStyle for RecipeListContainerStyle {
    fn make_insertion_indicator(
        &self,
        cfg: &ListInsertionConfig,
        ctx: &mut BuildContext,
    ) -> WidgetId {
        // Reference shape: a `width × 2` accent-coloured rect.
        // Custom container styles may install a different leaf.
        let _ = cfg.axis_offset; // positioned by the container's place_children
        let _ = cfg.width;
        ctx.add(
            RectWidget::new()
                .background(BorderRole::Accent),
        )
    }

    fn insertion(&self) -> ListInsertionRecipe {
        ListInsertionRecipe::default()
    }
}

pub fn resolve_list_container_style(
    override_: &Option<SharedListContainerStyle>,
    ctx: &BuildContext,
) -> SharedListContainerStyle {
    if let Some(s) = override_.clone() {
        return s;
    }
    ctx.theme_signal()
        .get()
        .style_slots
        .list_container
        .clone()
        .unwrap_or_else(|| std::rc::Rc::new(RecipeListContainerStyle) as SharedListContainerStyle)
}
