// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a screen reader can reach inside a wrapper, walked the way an
//! adapter walks.
//!
//! The AT-SPI, UIA and macOS adapters all build the tree they expose through
//! `accesskit_consumer::common_filter`, and that filter reads a hidden node as
//! `ExcludeSubtree`: the node goes, and so does everything under it, because
//! `NodeRef::is_hidden` is inherited from every ancestor (accesskit_consumer
//! 0.39 `filters.rs:22-24`, `node.rs:457-459`). Only the focused node escapes,
//! since the filter includes it before it looks at the flag. A wrapper that
//! meant "I am only chrome" and said so with `set_hidden()` therefore took its
//! content with it, and a reader could reach whichever control held focus and
//! nothing around it: not the rest of a dialog, not a card's body, not the
//! items of an open menu.
//!
//! The node that says "only chrome" is a `Role::GenericContainer` carrying
//! nothing else. The framework's presentational pass drops it from the update
//! and promotes its children, and if something keeps it there (an override, a
//! relation pointing at it) the filter still reads it as `ExcludeNode`, which
//! drops the node and keeps its children.
//!
//! Every test here mounts named content inside one wrapper, focuses nothing
//! unless it says so, and walks `filtered_children(common_filter)` down from
//! the window. That is the walk behind Orca's flat review (its
//! `getOnScreenObjects` gathers AT-SPI children, which atspi_common lists
//! through the filter, node.rs:84), and the children the Windows adapter
//! hands UIA navigation (accesskit_windows node.rs:1076-1083); what NVDA's
//! object navigation does with those is not observed here. The raw
//! `TreeUpdate` is not enough to see this: a hidden subtree is still in it,
//! node for node.

#![cfg(test)]

use accesskit_consumer::{NodeRef, Tree, common_filter};
use teksilo_canvas::SizeProposal;
use teksilo_core::accessibility::widget_id_to_node_id;
use teksilo_core::accesskit::{NodeId, Role, TreeUpdate};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::button::Button;
use crate::primitives::{HStack, TextWidget, VStack};

/// One node the filtered walk visited: its role, and the name an adapter
/// announces for it (a `Role::Label` carries its text in `value`).
#[derive(Debug, Clone, PartialEq)]
struct Reached {
    role: Role,
    name: Option<String>,
    /// Whether an adapter announces the node when it enters the tree.
    live: teksilo_core::accesskit::Live,
}

fn spoken(node: &NodeRef<'_>) -> Option<String> {
    let text = if node.role() == Role::Label {
        node.value()
    } else {
        node.label()
    }?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn walk(node: NodeRef<'_>, out: &mut Vec<Reached>) {
    for child in node.filtered_children(&common_filter) {
        out.push(Reached {
            role: child.role(),
            name: spoken(&child),
            live: child.live(),
        });
        walk(child, out);
    }
}

/// Every node an adapter reaches from the window, in reading order.
fn reached(update: &TreeUpdate) -> Vec<Reached> {
    let tree = Tree::new(update.clone(), false);
    let mut out = Vec::new();
    walk(tree.state().root(), &mut out);
    out
}

/// Every node an adapter reaches below `widget`'s node, which the walk
/// itself has to reach first.
#[track_caller]
fn reached_below(update: &TreeUpdate, widget: WidgetId) -> Vec<Reached> {
    fn find<'a>(node: NodeRef<'a>, target: NodeId) -> Option<NodeRef<'a>> {
        if node.locate().0 == target {
            return Some(node);
        }
        node.filtered_children(&common_filter)
            .find_map(|child| find(child, target))
    }
    let tree = Tree::new(update.clone(), false);
    let Some(node) = find(tree.state().root(), widget_id_to_node_id(widget)) else {
        panic!("the filtered walk never reaches {widget:?}, so nothing under it can be tested");
    };
    let mut out = Vec::new();
    walk(node, &mut out);
    out
}

#[track_caller]
fn assert_reaches(what: &str, reached: &[Reached], role: Role, name: &str) {
    assert!(
        reached
            .iter()
            .any(|r| r.role == role && r.name.as_deref() == Some(name)),
        "{what}: a screen reader cannot reach the {role:?} \"{name}\"; the filtered walk \
         reaches only {reached:?}"
    );
}

fn themed_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn laid_out(tree: &mut WidgetTree) -> TreeUpdate {
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.sync_accessibility()
}

/// A label and a button, the two things every wrapper below has to let
/// through: something to read and something to press.
fn content() -> VStack {
    VStack::new()
        .child(TextWidget::new(lit!("Inner text")))
        .child(Button::new(lit!("Inner action")))
}

#[track_caller]
fn assert_content_reachable(what: &str, reached: &[Reached]) {
    assert_reaches(what, reached, Role::Label, "Inner text");
    assert_reaches(what, reached, Role::Button, "Inner action");
}

// ── Dialogs ─────────────────────────────────────────────────────────────

mod dialogs {
    use super::*;
    use crate::dialog::{DialogContent, ModalContainer};

    fn confirmation() -> DialogContent {
        DialogContent::new()
            .title(lit!("Delete file?"))
            .supporting_text(lit!("It cannot be recovered."))
            .body(content())
            .footer(
                HStack::new()
                    .child(Button::new(lit!("Cancel")))
                    .child(Button::new(lit!("Delete"))),
            )
    }

    #[test]
    fn everything_in_a_dialog_is_reachable_inside_the_dialog() {
        // The dialog's own node is the one a reader is told they are in, so
        // the content has to be under it, not merely somewhere in the window.
        let mut tree = themed_tree();
        let dialog = tree.add(ModalContainer::new(confirmation()));
        let update = laid_out(&mut tree);
        let inside = reached_below(&update, dialog);

        let what = "a ModalContainer on the default DialogStyle";
        assert_reaches(what, &inside, Role::Label, "Delete file?");
        assert_reaches(what, &inside, Role::Label, "It cannot be recovered.");
        assert_content_reachable(what, &inside);
        assert_reaches(what, &inside, Role::Button, "Cancel");
        assert_reaches(what, &inside, Role::Button, "Delete");
    }

    #[test]
    fn focus_on_one_control_leaves_the_rest_of_the_dialog_reachable() {
        // How a dialog is actually met: focus lands on one control, and the
        // reader walks from there to find out what it is being asked. An
        // adapter still reports a focused node inside a hidden subtree, since
        // the filter includes it before it looks at the flag, which is how a
        // dialog could be answered with Tab and Enter while reading as empty
        // to anything that looked around. It is not even among the dialog's
        // children: the walk down skips the hidden subtree whole.
        let mut tree = themed_tree();
        let dialog = tree.add(ModalContainer::new(
            DialogContent::new()
                .title(lit!("Delete file?"))
                .supporting_text(lit!("It cannot be recovered."))
                .footer(Button::new(lit!("Cancel"))),
        ));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        // The only button in this dialog.
        let Some(cancel) = tree.find_by_role(Role::Button) else {
            panic!("the footer button was never built");
        };
        tree.focus(cancel);
        let update = laid_out(&mut tree);
        assert_eq!(
            update.focus,
            widget_id_to_node_id(cancel),
            "the test has to hold focus on the button it says it does"
        );
        let inside = reached_below(&update, dialog);

        let what = "a dialog with focus on its Cancel button";
        assert_reaches(what, &inside, Role::Button, "Cancel");
        assert_reaches(what, &inside, Role::Label, "Delete file?");
        assert_reaches(what, &inside, Role::Label, "It cannot be recovered.");
    }
}

/// Mounts content inside whatever `wrap` builds around it, the way a widget
/// mounts the body its style makes. For the style surfaces whose widget is
/// private or needs a presentation pipeline to show.
struct Wrapped {
    content: Option<Box<dyn teksilo_core::widget::Widget>>,
    wrap: Box<dyn Fn(&mut teksilo_core::build_context::BuildContext, WidgetId) -> WidgetId>,
    root: Option<WidgetId>,
}

impl Wrapped {
    fn new(
        content: impl teksilo_core::widget::Widget + 'static,
        wrap: impl Fn(&mut teksilo_core::build_context::BuildContext, WidgetId) -> WidgetId + 'static,
    ) -> Self {
        Self {
            content: Some(Box::new(content)),
            wrap: Box::new(wrap),
            root: None,
        }
    }
}

impl std::fmt::Debug for Wrapped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wrapped").finish_non_exhaustive()
    }
}

impl teksilo_core::widget::Widget for Wrapped {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(content) = self.content.take() {
            let content_id = ctx.add_boxed(content);
            self.root = Some((self.wrap)(ctx, content_id));
        }
        self.root.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [teksilo_core::widget::WidgetPlacement],
        _ctx: &teksilo_core::widget::LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        builder.set_role(Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

// ── Surfaces: the chrome a style draws around content ───────────────────

mod surfaces {
    use super::*;
    use crate::card::Card;
    use crate::panel::Panel;
    use crate::radio_tile::RadioTile;

    #[test]
    fn a_cards_slots_are_reachable_inside_the_card() {
        // `Card` is the `Role::Group`; the frame `RecipeCardStyle` draws is
        // chrome under it, and the macOS, Fluent and Material 3 card styles
        // all delegate to that frame.
        let mut tree = themed_tree();
        let card = tree.add(
            Card::new()
                .header(TextWidget::new(lit!("Card title")))
                .content(content()),
        );
        let update = laid_out(&mut tree);
        let inside = reached_below(&update, card);
        assert_reaches("a Card", &inside, Role::Label, "Card title");
        assert_content_reachable("a Card", &inside);
    }

    #[test]
    fn a_panels_child_is_reachable_inside_the_panel() {
        let mut tree = themed_tree();
        let panel = tree.add(Panel::new().child(content()));
        let update = laid_out(&mut tree);
        assert_content_reachable("a Panel", &reached_below(&update, panel));
    }

    #[test]
    fn a_presentational_panels_child_is_reachable() {
        // `a11y_presentational` promises to drop the panel's own group and
        // keep its children, which is what a toolbar and a status bar build
        // on.
        let mut tree = themed_tree();
        tree.add(Panel::new().a11y_presentational().child(content()));
        let update = laid_out(&mut tree);
        assert_content_reachable("an a11y_presentational Panel", &reached(&update));
    }

    #[test]
    fn a_toolbars_items_are_reachable_inside_the_toolbar() {
        let mut tree = themed_tree();
        let toolbar = tree.add(crate::toolbar::Toolbar::new().child(content()));
        let update = laid_out(&mut tree);
        assert_content_reachable("a Toolbar", &reached_below(&update, toolbar));
    }

    #[test]
    fn a_status_bars_items_are_reachable_inside_the_status_bar() {
        let mut tree = themed_tree();
        let status = tree.add(crate::status_bar::StatusBar::new().child(content()));
        let update = laid_out(&mut tree);
        assert_content_reachable("a StatusBar", &reached_below(&update, status));
    }

    #[test]
    fn a_snackbars_message_and_action_are_reachable() {
        // The frame `RecipeSnackbarStyle` draws inside the snackbar's
        // `Role::Alert`, reached here without the overlay that shows it.
        use teksilo_core::styles::{SnackbarStyle, SnackbarStyleConfig};
        let mut tree = themed_tree();
        tree.add(Wrapped::new(content(), |ctx, content| {
            crate::styles::RecipeSnackbarStyle::default()
                .make_body(&SnackbarStyleConfig { content }, ctx)
        }));
        let update = laid_out(&mut tree);
        assert_content_reachable("a RecipeSnackbarStyle frame", &reached(&update));
    }

    #[test]
    fn a_radio_tiles_body_is_reachable() {
        // A tile's typed title and description are folded into its own
        // name and description and hidden one by one. A `body` is not: it
        // is documented as exposed as it is, and only the frame around it
        // stood in the way.
        let mut tree = themed_tree();
        tree.add(RadioTile::new().title(lit!("Tile")).body(content()));
        let update = laid_out(&mut tree);
        let all = reached(&update);
        assert_reaches("a RadioTile", &all, Role::RadioButton, "Tile");
        assert_content_reachable("a RadioTile body", &all);
    }
}

// ── Popovers ────────────────────────────────────────────────────────────

mod popovers {
    use super::*;
    use crate::menu_item::MenuItem;
    use crate::menu_list::MenuList;

    #[test]
    fn a_menus_items_are_reachable_inside_the_menu() {
        // The `Menu` variant of `RecipePopoverStyle` draws the panel of every
        // `MenuList`, `ComboBox` drop-down and search suggestion list. Its
        // owner carries the container role, so the surface is chrome.
        let mut tree = themed_tree();
        let menu = tree.add(
            MenuList::new()
                .item(MenuItem::new(lit!("Open")))
                .item(MenuItem::new(lit!("Save"))),
        );
        let update = laid_out(&mut tree);
        let inside = reached_below(&update, menu);
        assert_reaches("a MenuList", &inside, Role::MenuItem, "Open");
        assert_reaches("a MenuList", &inside, Role::MenuItem, "Save");
    }

    #[test]
    fn a_presentational_popover_surface_lets_its_content_through() {
        // The surface itself, as any `PopoverStyle` caller builds it.
        use crate::popover_surface::PopoverSurface;
        let mut tree = themed_tree();
        tree.add(PopoverSurface::new(
            teksilo_core::widget::PendingChild::Deferred(Box::new(content())),
            teksilo_core::overlay::OverlayPlacement::Below,
            false,
            0.0,
            String::new(),
            teksilo_canvas::EdgeInsets::ZERO,
            teksilo_tokens::SurfaceRole::Raised,
            4.0,
            true,
        ));
        let update = laid_out(&mut tree);
        assert_content_reachable("a presentational PopoverSurface", &reached(&update));
    }
}

// ── Toasts ──────────────────────────────────────────────────────────────

mod toasts {
    use super::*;
    use crate::toast::{Toast, ToastAction, ToastHost, ToastInstallOptions, ToastRegistry};
    use teksilo_core::accesskit::Live;

    #[test]
    fn a_toast_is_reachable_and_live() {
        // A toast is a live `Role::Status` (or `Alert`), and AT-SPI announces
        // a live node when it enters the filtered tree
        // (accesskit_atspi_common adapter.rs:72-77). A toast under a hidden
        // host never entered it, so it was neither announced nor reachable.
        let options = ToastInstallOptions {
            archive: None,
            ..ToastInstallOptions::default()
        };
        let registry = ToastRegistry::new(options.clone());
        registry.enqueue(Toast::info(lit!("Saved")).action(ToastAction::new(lit!("Undo"), |_| {})));
        let mut tree = themed_tree();
        tree.add(ToastHost::new(registry.clone(), options));
        let update = laid_out(&mut tree);
        let all = reached(&update);

        assert_reaches("a toast", &all, Role::Status, "Saved");
        // A toast action is drawn and exposed as a link.
        assert_reaches("a toast", &all, Role::Link, "Undo");
        assert!(
            all.iter()
                .any(|r| r.name.as_deref() == Some("Saved") && r.live == Live::Polite),
            "an information toast must reach the filtered tree as a polite live region: {all:?}"
        );
    }
}

// ── Layout primitives ───────────────────────────────────────────────────

mod primitives {
    use super::*;
    use crate::primitives::{AspectRatio, MaxSize, Switcher};
    use teksilo_core::signal::Signal;

    #[test]
    fn a_max_size_child_is_reachable() {
        let mut tree = themed_tree();
        tree.add(MaxSize::width(640.0).child(content()));
        let update = laid_out(&mut tree);
        assert_content_reachable("a MaxSize", &reached(&update));
    }

    #[test]
    fn an_aspect_ratio_child_is_reachable() {
        let mut tree = themed_tree();
        tree.add(AspectRatio::new(1.5).child(content()));
        let update = laid_out(&mut tree);
        assert_content_reachable("an AspectRatio", &reached(&update));
    }

    #[test]
    fn a_switchers_shown_page_is_reachable() {
        // The pages not shown are dormant, which is what keeps them out of
        // the tree; the shown one has to stay in it.
        let mut tree = themed_tree();
        tree.add(
            Switcher::new(Signal::new(0_usize))
                .child(content())
                .child(TextWidget::new(lit!("Other page"))),
        );
        let update = laid_out(&mut tree);
        let all = reached(&update);
        assert_content_reachable("a Switcher", &all);
        assert!(
            !all.iter().any(|r| r.name.as_deref() == Some("Other page")),
            "a page the Switcher is not showing must stay out of the tree: {all:?}"
        );
    }

    #[test]
    fn a_tab_panel_and_its_content_are_reachable() {
        // `TabWidget` shows its panels through a `Switcher`, so the selected
        // `Role::TabPanel` went with it, content and all.
        let mut tree = themed_tree();
        tree.add(
            crate::tab_widget::TabWidget::new(Signal::new(None)).tab(lit!("General"), content()),
        );
        let update = laid_out(&mut tree);
        let all = reached(&update);
        assert_reaches("a TabWidget", &all, Role::TabPanel, "General");
        assert_content_reachable("a TabWidget's panel", &all);
    }

    #[test]
    fn a_capped_menus_items_are_reachable() {
        // Past `max_visible_items` a menu wraps its rows in a `MaxSize`.
        use crate::menu_item::MenuItem;
        use crate::menu_list::MenuList;
        let mut tree = themed_tree();
        let menu = tree.add(
            MenuList::new()
                .max_visible_items(1)
                .item(MenuItem::new(lit!("Open")))
                .item(MenuItem::new(lit!("Save")))
                .item(MenuItem::new(lit!("Close"))),
        );
        let update = laid_out(&mut tree);
        let inside = reached_below(&update, menu);
        assert_reaches("a capped MenuList", &inside, Role::MenuItem, "Open");
        assert_reaches("a capped MenuList", &inside, Role::MenuItem, "Save");
    }
}

// ── Composite shells ────────────────────────────────────────────────────

mod shells {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn a_snackbars_trigger_is_reachable() {
        // `Snackbar` is a layout shell around the button that shows it.
        let mut tree = themed_tree();
        tree.add(
            crate::snackbar::Snackbar::new(lit!("Show message"))
                .content(TextWidget::new(lit!("Saved"))),
        );
        let update = laid_out(&mut tree);
        assert_reaches(
            "a Snackbar",
            &reached(&update),
            Role::Button,
            "Show message",
        );
    }

    #[test]
    fn the_notification_bell_is_reachable() {
        use crate::notification::{NotificationArchiveModel, NotificationCenterButton};
        let mut tree = themed_tree();
        tree.add(NotificationCenterButton::new(Rc::new(
            NotificationArchiveModel::in_memory(),
        )));
        let update = laid_out(&mut tree);
        let bell = teksilo_i18n::tr_widget!(a11y_builtin_bell()).resolve_now();
        assert_reaches(
            "a NotificationCenterButton",
            &reached(&update),
            Role::Button,
            &bell,
        );
    }

    #[test]
    fn a_title_bars_centre_content_is_reachable() {
        // `TitleBar::center` is documented for a search box or breadcrumbs,
        // and it is mounted inside the drag region.
        use teksilo_canvas::{Point, Size};
        use teksilo_core::{HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge};

        #[derive(Debug)]
        struct Host;
        impl PlatformTitleBarHost for Host {
            fn reserved_leading_inset(&self) -> Size {
                Size::ZERO
            }
            fn reserved_trailing_inset(&self) -> Size {
                Size::ZERO
            }
            fn renders_custom_controls(&self) -> bool {
                false
            }
            fn needs_custom_resize_handles(&self) -> bool {
                false
            }
            fn begin_drag(&self) -> Result<(), PlatformError> {
                Ok(())
            }
            fn begin_resize(&self, _edge: ResizeEdge) -> Result<(), PlatformError> {
                Ok(())
            }
            fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
                Ok(())
            }
            fn update_hit_regions(&self, _regions: &HitRegions) {}
        }

        let mut tree = themed_tree();
        tree.add(crate::title_bar::TitleBar::new(Rc::new(Host)).center(content()));
        let update = laid_out(&mut tree);
        assert_content_reachable("a TitleBar's centre", &reached(&update));
    }

    #[test]
    fn an_unlabelled_splitter_panes_content_is_reachable() {
        // An unlabelled pane is documented as transparent, its content
        // representing itself. A labelled one is a named group.
        use crate::splitter::Splitter;
        use crate::splitter::SplitterModel;
        let mut tree = themed_tree();
        tree.add(
            Splitter::new(SplitterModel::new(
                2,
                teksilo_tokens::Orientation::Horizontal,
            ))
            .pane(content())
            .pane(TextWidget::new(lit!("Second pane")))
            .pane_label(1, "Details".to_string()),
        );
        let update = laid_out(&mut tree);
        let all = reached(&update);
        assert_content_reachable("an unlabelled Splitter pane", &all);
        assert_reaches("a labelled Splitter pane", &all, Role::Group, "Details");
        assert_reaches("a labelled Splitter pane", &all, Role::Label, "Second pane");
    }

    #[test]
    fn a_collapsed_panes_sliver_is_reachable() {
        // A `collapsed_size` keeps a sliver of a collapsed pane on screen, an
        // accordion's header say, and its content stays live to fill it. The
        // pane dropped its group when it collapsed by hiding itself, and took
        // the sliver with it.
        use crate::splitter::{Splitter, SplitterModel};
        let model = SplitterModel::new(2, teksilo_tokens::Orientation::Vertical);
        model.set_collapsed_size(0, 24.0);
        model.set_collapsed_immediate(0, true);
        let mut tree = themed_tree();
        tree.add(
            Splitter::new(model.clone())
                .pane(content())
                .pane(TextWidget::new(lit!("Second pane")))
                .pane_label(0, "Header pane".to_string()),
        );
        let update = laid_out(&mut tree);
        assert!(
            model.is_collapsed(0),
            "the test has to hold the pane collapsed"
        );
        assert_content_reachable("a collapsed Splitter pane's sliver", &reached(&update));
    }

    #[test]
    fn a_tree_table_rows_cells_are_reachable() {
        // Each row is a `TreeRowA11y` carrying `Role::Row`, around a
        // `BodyRow` built not to announce a second row.
        use crate::table_view::column::{CellContext, Column, ColumnWidth};
        use teksilo_data::{SortFilterTreeModel, TreeModel};
        let model = TreeModel::new();
        model.insert_root(0, "readme");
        model.insert_root(1, "guide");
        let mut tree = themed_tree();
        tree.add(
            crate::TreeTableView::from_projection(SortFilterTreeModel::new(model))
                .add_column(
                    Column::<&str>::new("name", lit!("Name"), |row, _: &CellContext| {
                        Box::new(TextWidget::new(lit!(*row)))
                    })
                    .width(ColumnWidth::Flex(1.0)),
                )
                .row_height(20.0),
        );
        let update = laid_out(&mut tree);
        let all = reached(&update);
        assert_reaches("a TreeTableView row", &all, Role::Label, "readme");
        assert_reaches("a TreeTableView row", &all, Role::Label, "guide");
    }
}

// ── Calendar ────────────────────────────────────────────────────────────

mod calendar {
    use super::*;
    use crate::calendar::Calendar;
    use teksilo_core::signal::Signal;

    fn published(update: &TreeUpdate, role: Role) -> usize {
        update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == role)
            .count()
    }

    #[test]
    fn every_day_cell_and_header_button_is_reachable_inside_the_grid() {
        // A day cell names its date in full, says whether it is selected
        // and whether it is today, and the header holds the arrows and the
        // title button. The grid's own value carries the cursor; the cells
        // and buttons are what a reader reviews around it.
        let mut tree = themed_tree();
        let calendar = tree.add(Calendar::single(Signal::new(None)));
        let update = laid_out(&mut tree);
        let inside = reached_below(&update, calendar);

        let cells = published(&update, Role::GridCell);
        assert!(cells >= 28, "a month grid publishes its days: {cells}");
        let reached_cells = inside.iter().filter(|r| r.role == Role::GridCell).count();
        assert_eq!(
            reached_cells, cells,
            "every day cell the calendar publishes must be reachable inside it"
        );

        let buttons = published(&update, Role::Button);
        assert!(buttons >= 4, "the header publishes its arrows: {buttons}");
        let reached_buttons = inside.iter().filter(|r| r.role == Role::Button).count();
        assert_eq!(
            reached_buttons, buttons,
            "every header button the calendar publishes must be reachable inside it"
        );
    }
}
