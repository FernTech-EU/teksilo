//! SplitButton — a button split into two regions sharing a single frame.
//!
//! The left region is the **default action**: it shows the label of the
//! currently-selected item and, on click, fires that item's command
//! (behaving like a regular [`Button`](crate::button::Button)). The right
//! region is a narrow chevron zone that, on click, opens a
//! [`MenuList`](crate::menu_list::MenuList) of related actions. Picking an
//! action from the dropdown fires it and promotes its index to become the
//! new default for the session (IntelliJ's "remember last used"
//! convention).
//!
//! SplitButton reuses [`MenuItem`](crate::menu_item::MenuItem) verbatim
//! for the dropdown rows — the caller passes real `MenuItem` values via
//! `.item(...)`, so icons, shortcut labels, enabled flags, and separators
//! all come for free.
//!
//! ```ignore
//! SplitButton::new()
//!     .item(MenuItem::new_literal("Run").on_activate(Cmd::Run))
//!     .item(MenuItem::new_literal("Run Tests").on_activate(Cmd::RunTests))
//!     .separator()
//!     .item(MenuItem::new_literal("Debug").on_activate(Cmd::Debug))
//!     .style(ButtonVariant::Regular)
//! ```

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::{HandlerSet, WidgetBuilder};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, ColorTokens, CornerRadius};

use crate::button::{ButtonVariant, InteractionState};
use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;
use crate::primitives::{
    Center, FixedSize, FocusRing, HStack, IconWidget, MinSize, Padding, RectWidget, TextWidget,
    ZStack,
};

/// One row of the SplitButton's dropdown: either a real MenuItem or a
/// separator. Stored unbuilt until `build()` hands the items to a MenuList.
/// MenuItem is boxed because it is substantially larger than `Separator`,
/// which would otherwise bloat every `Row::Separator` slot.
enum Row {
    Item(Box<MenuItem>),
    Separator,
}

pub struct SplitButton {
    rows: Vec<Row>,
    style: ButtonVariant,
    enabled: bool,
    initial_selected: usize,
    // Build state
    interaction: Signal<InteractionState>,
    selected: Signal<usize>,
    labels: Rc<Vec<String>>,
    menu_content_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
}

impl SplitButton {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            style: ButtonVariant::Regular,
            enabled: true,
            initial_selected: 0,
            interaction: Signal::new(InteractionState::Idle),
            selected: Signal::new(0),
            labels: Rc::new(Vec::new()),
            menu_content_id: None,
            root_child_id: None,
        }
    }

    /// Add a menu item. The item is reused verbatim as a row of the
    /// dropdown, and its label + action are also used to drive the main
    /// region (when its index is the current default).
    pub fn item(mut self, item: MenuItem) -> Self {
        self.rows.push(Row::Item(Box::new(item)));
        self
    }

    /// Add a separator row in the dropdown. Separators are skipped when
    /// computing item indices for `initial_selected`.
    pub fn separator(mut self) -> Self {
        self.rows.push(Row::Separator);
        self
    }

    pub fn style(mut self, variant: ButtonVariant) -> Self {
        self.style = variant;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Which item index (counting only items, not separators) should be
    /// the initial default. Defaults to 0.
    pub fn initial_selected(mut self, index: usize) -> Self {
        self.initial_selected = index;
        self
    }
}

impl Default for SplitButton {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SplitButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitButton")
            .field("rows", &self.rows.len())
            .field("style", &self.style)
            .field("enabled", &self.enabled)
            .finish()
    }
}

// --- Color resolution (variant × state × theme) ---
//
// Mirrors Button::resolve_bg / resolve_text / resolve_border so a Button
// and a SplitButton with the same variant look identical. If Button's
// color tables ever diverge from these, update both sides.

fn resolve_bg(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    match (style, state) {
        (ButtonVariant::Default, InteractionState::Disabled) => colors.accent_disabled,
        (ButtonVariant::Default, InteractionState::Pressed) => colors.accent_pressed,
        (ButtonVariant::Default, InteractionState::Hovered) => colors.accent_hover,
        (ButtonVariant::Default, _) => colors.accent,

        (ButtonVariant::Regular, InteractionState::Pressed) => colors.surface_pressed,
        (ButtonVariant::Regular, InteractionState::Hovered) => colors.surface_hover,
        (ButtonVariant::Regular, _) => colors.surface_main,

        (ButtonVariant::Flat, InteractionState::Pressed) => colors.surface_pressed,
        (ButtonVariant::Flat, InteractionState::Hovered) => colors.surface_hover,
        (ButtonVariant::Flat, _) => Color::TRANSPARENT,
    }
}

fn resolve_text(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    match (style, state) {
        (ButtonVariant::Default, InteractionState::Disabled) => colors.text_disabled,
        (ButtonVariant::Default, _) => colors.text_on_accent,

        (ButtonVariant::Regular | ButtonVariant::Flat, InteractionState::Disabled) => {
            colors.text_disabled
        }
        (ButtonVariant::Regular | ButtonVariant::Flat, _) => colors.text_primary,
    }
}

fn resolve_border(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    match style {
        ButtonVariant::Default | ButtonVariant::Flat => Color::TRANSPARENT,
        ButtonVariant::Regular => match state {
            InteractionState::Disabled => colors.border,
            InteractionState::Hovered | InteractionState::Pressed => colors.border_strong,
            _ => colors.border,
        },
    }
}

impl Widget for SplitButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let sb_style = theme.components.split_button;
        let style = self.style;
        let enabled = self.enabled;

        // ---- Extract label / action for each MenuItem and wrap each item's
        // activation so selecting it from the menu also promotes its index
        // to the current default. ----

        let mut labels_vec: Vec<String> = Vec::new();
        let mut actions_vec: Vec<Option<Rc<dyn Fn(&mut EventContext)>>> = Vec::new();
        let mut menu = MenuList::new();

        // Create the `selected` signal early so the wrap closures can
        // capture it.
        let initial = self.initial_selected;
        let selected: Signal<usize> = ctx.signal(initial);

        for row in self.rows.drain(..) {
            match row {
                Row::Item(boxed_item) => {
                    let mut item = *boxed_item;
                    let label = item.label().to_string();
                    let action = item.action();
                    let my_index = labels_vec.len();
                    labels_vec.push(label);
                    actions_vec.push(action.clone());

                    let prev_action = action.clone();
                    let promote = selected.clone();
                    item = item.on_activate_fn(move |ctx: &mut EventContext| {
                        if let Some(ref a) = prev_action {
                            a(ctx);
                        }
                        promote.set(my_index);
                    });
                    menu = menu.item(item);
                }
                Row::Separator => {
                    menu = menu.separator();
                }
            }
        }

        // Clamp initial_selected to a valid range now that we know the count.
        let item_count = labels_vec.len();
        if item_count == 0 || selected.get() >= item_count {
            selected.set(0);
        }

        let labels_rc = Rc::new(labels_vec);
        let actions_rc: Rc<Vec<Option<Rc<dyn Fn(&mut EventContext)>>>> = Rc::new(actions_vec);

        self.labels = labels_rc.clone();
        self.selected = selected.clone();

        // ---- Interaction state signal ----
        let interaction = ctx.signal(if enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        self.interaction = interaction.clone();

        // ---- Derived reactive colors ----
        let bg_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_bg(style, *s, &colors))
        };
        let text_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_text(style, *s, &colors))
        };
        let border_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_border(style, *s, &colors))
        };
        let divider_color = theme.colors.border;

        // ---- Main-region label bound to `selected` ----
        let main_label_text = {
            let labels = labels_rc.clone();
            selected.map(move |i| {
                if labels.is_empty() {
                    String::new()
                } else {
                    labels[(*i).min(labels.len() - 1)].clone()
                }
            })
        };

        // ---- Pre-register the menu overlay (dormant until opened) ----
        let menu_id = ctx.add(menu);
        ctx.set_dormant(menu_id);
        self.menu_content_id = Some(menu_id);

        let self_id = ctx.self_id();

        // ---- Main region subtree ----
        let label_widget = TextWidget::new_literal("")
            .bind_text(main_label_text)
            .bind_color(text_color.clone())
            .single_line();
        let label_id = ctx.add(label_widget);

        let main_padding_id = ctx.add(
            Padding::symmetric(sb_style.padding_vertical, sb_style.padding_horizontal)
                .set_child(label_id),
        );

        let main_region = {
            let actions_for_tap = actions_rc.clone();
            let selected_for_tap = selected.clone();
            let int_for_tap = interaction.clone();
            let int_for_hover = interaction.clone();
            MinSize::new(sb_style.min_width, sb_style.height)
                .set_child(main_padding_id)
                .on_tap(move |_pos, ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    let idx = selected_for_tap.get();
                    if let Some(Some(action)) = actions_for_tap.get(idx) {
                        action(ctx);
                    }
                    int_for_tap.set(InteractionState::Hovered);
                })
                .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    int_for_hover.set(if entered {
                        InteractionState::Hovered
                    } else {
                        InteractionState::Idle
                    });
                })
                .cursor(CursorIcon::Pointer)
        };
        let main_region_id = ctx.add(main_region);

        // ---- Divider between main and chevron regions ----
        let divider_fill_id = ctx.add(RectWidget::new().background(divider_color));
        let divider_id = ctx.add(
            FixedSize::new()
                .bind_width(sb_style.divider_width)
                .bind_height(sb_style.height)
                .set_child(divider_fill_id),
        );

        // ---- Chevron region ----
        let chevron_icon_id = ctx.add(
            IconWidget::chevron_down(sb_style.chevron_icon_size).bind_color(text_color.clone()),
        );
        let chevron_centered_id = ctx.add(Center::new().set_child(chevron_icon_id));

        let chevron_region = {
            let int_for_tap = interaction.clone();
            let int_for_hover = interaction.clone();
            FixedSize::new()
                .bind_width(sb_style.chevron_width)
                .bind_height(sb_style.height)
                .set_child(chevron_centered_id)
                .on_tap(move |_pos, ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    int_for_tap.set(InteractionState::Pressed);
                    ctx.activate(menu_id);
                    ctx.show_overlay(OverlayRequest {
                        content_id: menu_id,
                        anchor: self_id,
                        placement: OverlayPlacement::BelowPreferred,
                        dismiss: DismissBehavior::EscapeOrClickOutside,
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                    });
                    // The MenuList owns the keyboard-navigation handler
                    // (ArrowUp/ArrowDown/Enter/Escape) and that handler
                    // only fires when the MenuList is focused. Hand focus
                    // over so the user can immediately keyboard-walk the
                    // items they just opened.
                    ctx.request_focus(menu_id);
                })
                .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    int_for_hover.set(if entered {
                        InteractionState::Hovered
                    } else {
                        InteractionState::Idle
                    });
                })
                .cursor(CursorIcon::Pointer)
        };
        let chevron_region_id = ctx.add(chevron_region);

        // ---- Row: main | divider | chevron ----
        let row_id = ctx.add(
            HStack::new()
                .spacing(0.0)
                .add_child(main_region_id)
                .add_child(divider_id)
                .add_child(chevron_region_id),
        );

        // ---- Shared frame (single RectWidget behind the row) ----
        let bg_rect = RectWidget::new()
            .bind_background(bg_color)
            .bind_border_color(border_color)
            .border_width(sb_style.border_width)
            .corner_radius(CornerRadius::uniform(sb_style.corner_radius));
        let bg_id = ctx.add(bg_rect);

        let frame_id = ctx.add(ZStack::new().add_child(bg_id).add_child(row_id));

        // Enforce an overall minimum size: main min_width + divider + chevron.
        let total_min_width = sb_style.min_width + sb_style.divider_width + sb_style.chevron_width;
        let sized_id = ctx.add(
            MinSize::new(total_min_width, sb_style.height).set_child(frame_id),
        );

        // Focus ring is drawn outside the frame on keyboard focus only.
        let focused = interaction.map(|s| *s == InteractionState::Focused);
        let root_id = ctx.add(
            FocusRing::new(focused)
                .corner_radius(sb_style.corner_radius)
                .set_child(sized_id),
        );
        self.root_child_id = Some(root_id);

        // ---- Self handlers: the SplitButton is the single focus stop.
        // Space/Enter fires the current default; ArrowDown opens the menu.
        let actions_for_key = actions_rc.clone();
        let selected_for_key = selected.clone();
        let int_for_key = interaction.clone();
        let int_for_focus = interaction.clone();

        let handler_set = HandlerSet::new()
            .on_key(
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            int_for_key.set(InteractionState::Pressed);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyUp {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            let idx = selected_for_key.get();
                            if let Some(Some(action)) = actions_for_key.get(idx) {
                                action(ctx);
                            }
                            int_for_key.set(InteractionState::Focused);
                            EventResponse::Handled
                        }
                        // ArrowDown alone, or Alt+ArrowDown (the native
                        // "open dropdown" shortcut) both open the menu.
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown,
                            ..
                        } => {
                            ctx.activate(menu_id);
                            ctx.show_overlay(OverlayRequest {
                                content_id: menu_id,
                                anchor: self_id,
                                placement: OverlayPlacement::BelowPreferred,
                                dismiss: DismissBehavior::EscapeOrClickOutside,
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                            });
                            ctx.request_focus(menu_id);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                },
            )
            .on_focus(move |gained: bool, _ctx: &mut EventContext| {
                if gained {
                    if int_for_focus.get() == InteractionState::Idle {
                        int_for_focus.set(InteractionState::Focused);
                    }
                } else {
                    int_for_focus.set(InteractionState::Idle);
                }
            })
            .focusable(enabled);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
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
        builder.set_role(fern_core::accesskit::Role::Button);
        if !self.labels.is_empty() {
            let idx = self.selected.get().min(self.labels.len() - 1);
            builder.set_name(self.labels[idx].as_str());
        }
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::app_command::AppCommand;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::RefCell as StdRefCell;
    use std::rc::Rc as StdRc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Run,
        RunTests,
        Debug,
    }
    impl AppCommand for TestCmd {}

    fn capture_commands(tree: &mut WidgetTree) -> StdRc<StdRefCell<Vec<TestCmd>>> {
        let commands = StdRc::new(StdRefCell::new(Vec::new()));
        let captured = commands.clone();
        tree.on_command(move |cmd: &TestCmd| {
            captured.borrow_mut().push(cmd.clone());
        });
        commands
    }

    fn collect_descendants(tree: &WidgetTree, root: WidgetId, out: &mut Vec<WidgetId>) {
        out.push(root);
        for child in tree.children(root) {
            collect_descendants(tree, child, out);
        }
    }

    fn descendants(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
        let mut out = Vec::new();
        collect_descendants(tree, root, &mut out);
        out
    }

    fn setup() -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add(
            SplitButton::new()
                .item(MenuItem::new_literal("Run").on_activate(TestCmd::Run))
                .item(MenuItem::new_literal("Run Tests").on_activate(TestCmd::RunTests))
                .item(MenuItem::new_literal("Debug").on_activate(TestCmd::Debug))
                .style(ButtonVariant::Regular),
        );
        // Unspecified proposal so the SplitButton reports its natural size;
        // its outer bounds then coincide with the inner frame, and region
        // hit-testing via bounds centers lands where expected.
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        (tree, sb)
    }

    /// Click position for the "main" region — a couple of pixels inside
    /// the leading edge of the SplitButton's natural bounds, which is
    /// inside the main region as long as it is at least ~8 px wide.
    fn main_region_point(tree: &WidgetTree, sb: WidgetId) -> fern_canvas::Point {
        let b = tree.bounds(sb);
        fern_canvas::Point::new(b.x + 8.0, b.y + b.height / 2.0)
    }

    /// Click position for the "chevron" region — a couple of pixels
    /// inside the trailing edge.
    fn chevron_region_point(tree: &WidgetTree, sb: WidgetId) -> fern_canvas::Point {
        let b = tree.bounds(sb);
        fern_canvas::Point::new(b.x + b.width - 6.0, b.y + b.height / 2.0)
    }

    #[test]
    fn default_action_fires_on_main_click() {
        let (mut tree, sb) = setup();
        let commands = capture_commands(&mut tree);
        let p = main_region_point(&tree, sb);
        tree.pointer_move(p);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(&*commands.borrow(), &[TestCmd::Run]);
    }

    #[test]
    fn chevron_opens_menu() {
        let (mut tree, sb) = setup();
        assert!(tree.active_overlays().is_empty());
        let p = chevron_region_point(&tree, sb);
        tree.pointer_move(p);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn menu_selection_promotes_default() {
        let (mut tree, sb) = setup();
        let commands = capture_commands(&mut tree);

        // Open the dropdown via chevron.
        let cp = chevron_region_point(&tree, sb);
        tree.pointer_move(cp);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: cp,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: cp,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        // Find the MenuItem with label "Debug" in the overlay tree and click it.
        let overlay_roots = tree.overlay_manager().active_content_ids();
        assert_eq!(overlay_roots.len(), 1);
        let overlay_root = overlay_roots[0];
        let debug_id = descendants(&tree, overlay_root)
            .into_iter()
            .find(|&id| {
                let info = tree.accessibility_node(id);
                info.role() == fern_core::accesskit::Role::MenuItem && info.name() == Some("Debug")
            })
            .expect("Debug item must exist in the open menu");
        tree.click(debug_id);
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        // Assert: (a) Debug's command was emitted from the menu selection,
        // (b) subsequent main-click fires Debug (the new default), not Run.
        let main_point = main_region_point(&tree, sb);
        tree.pointer_move(main_point);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: main_point,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: main_point,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        let cmds = commands.borrow();
        assert_eq!(&*cmds, &[TestCmd::Debug, TestCmd::Debug]);
    }

    #[test]
    fn disabled_split_button_ignores_clicks() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add(
            SplitButton::new()
                .item(MenuItem::new_literal("Run").on_activate(TestCmd::Run))
                .item(MenuItem::new_literal("Debug").on_activate(TestCmd::Debug))
                .enabled(false),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let commands = capture_commands(&mut tree);

        let mp = main_region_point(&tree, sb);
        tree.pointer_move(mp);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: mp,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: mp,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        let cp = chevron_region_point(&tree, sb);
        tree.pointer_move(cp);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: cp,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: cp,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        assert!(commands.borrow().is_empty());
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn initial_selected_respected() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add(
            SplitButton::new()
                .item(MenuItem::new_literal("Run").on_activate(TestCmd::Run))
                .item(MenuItem::new_literal("Run Tests").on_activate(TestCmd::RunTests))
                .item(MenuItem::new_literal("Debug").on_activate(TestCmd::Debug))
                .initial_selected(1),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let commands = capture_commands(&mut tree);

        let p = main_region_point(&tree, sb);
        tree.pointer_move(p);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(&*commands.borrow(), &[TestCmd::RunTests]);
    }

    #[test]
    fn separator_does_not_shift_indices() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add(
            SplitButton::new()
                .item(MenuItem::new_literal("Run").on_activate(TestCmd::Run))
                .separator()
                .item(MenuItem::new_literal("Debug").on_activate(TestCmd::Debug))
                .initial_selected(1),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let commands = capture_commands(&mut tree);

        // initial_selected(1) should pick "Debug" (second item), not be
        // shifted by the separator.
        let p = main_region_point(&tree, sb);
        tree.pointer_move(p);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(&*commands.borrow(), &[TestCmd::Debug]);
    }

    #[test]
    fn accessibility_role_and_default_name() {
        let (tree, sb) = setup();
        let info = tree.accessibility_node(sb);
        assert_eq!(info.role(), fern_core::accesskit::Role::Button);
        assert_eq!(info.name(), Some("Run"));
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }

    #[test]
    fn no_items_renders_without_panic() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sb = tree.add(SplitButton::new());
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let bounds = tree.bounds(sb);
        assert!(bounds.width > 0.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn default_variant_renders_accent_background() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            SplitButton::new()
                .item(MenuItem::new_literal("Run").on_activate(TestCmd::Run))
                .style(ButtonVariant::Default),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let frame = tree.render();
        let accent = Theme::light_default().colors.accent.to_array();
        assert!(frame.shapes.iter().any(|s| s.color == accent));
    }

    #[test]
    fn chevron_open_transfers_focus_to_menu() {
        let (mut tree, sb) = setup();
        let p = chevron_region_point(&tree, sb);
        tree.pointer_move(p);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: p,
            button: fern_core::event::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        // The menu is open and the MenuList owns keyboard focus so its
        // ArrowUp/ArrowDown handler is live.
        let overlay_roots = tree.overlay_manager().active_content_ids();
        assert_eq!(overlay_roots.len(), 1);
        assert_eq!(
            tree.focused(),
            Some(overlay_roots[0]),
            "the open menu should own keyboard focus after the chevron click"
        );
    }

    #[test]
    fn arrow_down_on_split_button_opens_menu_and_focuses_it() {
        let (mut tree, sb) = setup();
        tree.focus(sb);
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let overlay_roots = tree.overlay_manager().active_content_ids();
        assert_eq!(overlay_roots.len(), 1);
        assert_eq!(tree.focused(), Some(overlay_roots[0]));
    }

    #[test]
    fn alt_arrow_down_also_opens_menu() {
        let (mut tree, sb) = setup();
        tree.focus(sb);
        tree.press_key(Key::ArrowDown, Modifiers::ALT);
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn space_fires_default_action() {
        let (mut tree, sb) = setup();
        let commands = capture_commands(&mut tree);
        tree.focus(sb);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(&*commands.borrow(), &[TestCmd::Run]);
    }
}
