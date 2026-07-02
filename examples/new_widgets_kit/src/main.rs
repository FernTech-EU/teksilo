// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Showcase for the five widgets shipped together: Banner,
//! CommandLinkButton, SearchField, FilePickerField, and InputDialog.
//!
//! Run with: `cargo run -p new-widgets-kit`
//!
//! The window is laid out top-to-bottom:
//!
//! 1. A row of Banners (one per severity) — the dismiss action toggles
//!    a per-banner visibility signal.
//! 2. A SearchField — pretend filter input.
//! 3. A FilePickerField — opens the native file dialog.
//! 4. A "Rename…" button that opens an InputDialog modal.
//! 5. Two CommandLinkButtons in a "Welcome" landing pane.

use bastyde::core::widget::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::widgets::{
    Banner, Button, ButtonVariant, Card, Collapse, CommandLinkButton, Expand, FilePickerField,
    FilePickerKind, GroupHeader, HStack, IconWidget, Panel, SearchField, Spacer, TextWidget,
    Toolbar, VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

#[derive(Debug)]
struct Root {
    root_child_id: Option<WidgetId>,

    // Per-widget reactive state.
    show_info_banner: Signal<bool>,
    show_warn_banner: Signal<bool>,
    show_error_banner: Signal<bool>,
    search_text: Signal<String>,
    path_text: Signal<String>,
    rename_text: Signal<String>,
}

impl Root {
    fn new() -> Self {
        Self {
            root_child_id: None,
            show_info_banner: Signal::new(true),
            show_warn_banner: Signal::new(true),
            show_error_banner: Signal::new(true),
            search_text: Signal::new(String::new()),
            path_text: Signal::new(String::new()),
            rename_text: Signal::new("untitled.txt".to_string()),
        }
    }

    fn banner_section(&self, ctx: &mut BuildContext) -> WidgetId {
        let info_visible = self.show_info_banner.clone();
        let warn_visible = self.show_warn_banner.clone();
        let error_visible = self.show_error_banner.clone();

        let info_banner = Banner::info(lit!("Welcome to Bastyde")).description(lit!(
            "Persistent inline status strips for app-level conditions."
        ));
        let warn_banner = Banner::warning(lit!("Unsaved changes"))
            .description(lit!("Closing the document now will discard your edits."))
            .action(
                Button::new(lit!("Save now"))
                    .variant(ButtonVariant::Plain)
                    .on_activate_fn(|_| println!("Save now clicked")),
            )
            .on_dismiss({
                let s = warn_visible.clone();
                move |_| s.set(false)
            });
        let error_banner = Banner::error(lit!("Disk almost full"))
            .description(lit!("Less than 200 MB remaining on /Users/you."))
            .on_dismiss({
                let s = error_visible.clone();
                move |_| s.set(false)
            });

        // Wrap each banner in a Collapse so showing / dismissing
        // animates the height instead of snapping. The Collapse's
        // `expanded` signal is the same `Signal<bool>` we toggle from
        // the dismiss / restore handlers.
        let info_id = ctx.add(Collapse::new(info_visible).child(info_banner));
        let warn_id = ctx.add(Collapse::new(warn_visible).child(warn_banner));
        let error_id = ctx.add(Collapse::new(error_visible).child(error_banner));

        let restore = Button::new(lit!("Restore banners"))
            .variant(ButtonVariant::Ghost)
            .on_activate_fn({
                let info = self.show_info_banner.clone();
                let warn = self.show_warn_banner.clone();
                let err = self.show_error_banner.clone();
                move |_| {
                    info.set(true);
                    warn.set(true);
                    err.set(true);
                }
            });

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(GroupHeader::new(lit!("Banner")))
                .add_child(info_id)
                .add_child(warn_id)
                .add_child(error_id)
                .child(HStack::new().child(Spacer::new()).child(restore)),
        )
    }

    fn search_and_picker_section(&self, ctx: &mut BuildContext) -> WidgetId {
        // Live readout of what's typed in the search field — shows the
        // user that the bound signal updates on every keystroke.
        let search_readout = TextWidget::new(lit!(""))
            .text(self.search_text.map(|s| {
                if s.is_empty() {
                    "Type to filter — text mirrors here as you type. Press Enter to submit."
                        .to_string()
                } else {
                    format!("Filtering: \"{}\"", s)
                }
            }))
            .color(TextRole::Secondary);

        // Submit feedback line — flips to a confirmation when the user
        // presses Enter. Reset by typing again.
        let submit_count = ctx.signal(0_usize);
        let submit_count_for_label = submit_count.clone();
        let submit_readout = TextWidget::new(lit!(""))
            .text(submit_count_for_label.map(|n| {
                if *n == 0 {
                    String::new()
                } else {
                    format!("Submitted {n} time(s).")
                }
            }))
            .style(TextStyleRole::Small)
            .color(TextRole::Accent);

        // Static dictionary of fruits — the suggestion provider filters
        // case-insensitively by prefix. Try typing "ap" → Apple,
        // Apricot. Use ArrowDown / ArrowUp to navigate and Enter to
        // pick (or click the row).
        const FRUITS: &[&str] = &[
            "Apple",
            "Apricot",
            "Avocado",
            "Banana",
            "Blackberry",
            "Blueberry",
            "Cherry",
            "Coconut",
            "Cranberry",
            "Date",
            "Elderberry",
            "Fig",
            "Grape",
            "Grapefruit",
            "Guava",
            "Kiwi",
            "Lemon",
            "Lime",
            "Lychee",
            "Mango",
            "Melon",
            "Nectarine",
            "Olive",
            "Orange",
            "Papaya",
            "Passionfruit",
            "Peach",
            "Pear",
            "Persimmon",
            "Pineapple",
            "Plum",
            "Pomegranate",
            "Quince",
            "Raspberry",
            "Strawberry",
            "Tangerine",
            "Watermelon",
        ];
        let search = SearchField::new(self.search_text.clone())
            .placeholder(lit!("Type a fruit — Apple, Banana, …"))
            .with_suggestions(|prefix| {
                let p = prefix.to_lowercase();
                FRUITS
                    .iter()
                    .filter(|f| f.to_lowercase().starts_with(&p))
                    .map(|f| (*f).to_string())
                    .collect()
            })
            .on_select(|value, _ctx| println!("picked suggestion: {value}"))
            .on_submit_fn({
                let s = self.search_text.clone();
                let count = submit_count.clone();
                move |_| {
                    println!("submit search: {:?}", s.get());
                    count.set(count.get() + 1);
                }
            });

        // Live readout of the picker state.
        let path_readout = TextWidget::new(lit!(""))
            .text(self.path_text.map(|p| {
                if p.is_empty() {
                    "No file picked yet — click the trailing Browse button.".to_string()
                } else {
                    format!("Picked: {}", p)
                }
            }))
            .color(TextRole::Secondary);

        let picker = FilePickerField::new(self.path_text.clone())
            .kind(FilePickerKind::OpenFile)
            .placeholder(lit!("No file selected"))
            .add_filter("Text", &["txt", "md"])
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .dialog_title(lit!("Choose a file"));

        ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(GroupHeader::new(lit!("SearchField & FilePickerField")))
                .child(search)
                .child(search_readout)
                .child(submit_readout)
                .child(picker)
                .child(path_readout),
        )
    }

    fn input_dialog_section(&self, ctx: &mut BuildContext) -> WidgetId {
        let rename_text = self.rename_text.clone();
        // Last action: "accepted: …" / "cancelled" / "" — gives clear
        // feedback whether the modal accepted the rename or not.
        let last_action = ctx.signal::<Option<bool>>(None);

        let preview = TextWidget::new(lit!(""))
            .text(rename_text.map(|s| format!("Current name: {}", s)))
            .style(TextStyleRole::BodyBold);

        let action_readout = TextWidget::new(lit!(""))
            .text(last_action.map(|state| match state {
                None => String::new(),
                Some(true) => "Last result: accepted — name updated above.".to_string(),
                Some(false) => "Last result: cancelled — name unchanged.".to_string(),
            }))
            .color(TextRole::Secondary);

        let trigger = Button::new(lit!("Rename…"))
            .variant(ButtonVariant::Plain)
            .on_activate_fn({
                let rename_text = rename_text.clone();
                let last_action = last_action.clone();
                move |ctx| {
                    let rename_text = rename_text.clone();
                    let last_action = last_action.clone();
                    use bastyde::widgets::InputDialog;
                    InputDialog::new(lit!("Rename document"))
                        .prompt(lit!("Enter the new file name:"))
                        .default_text(rename_text.get())
                        .placeholder(lit!("filename.ext"))
                        .on_result(move |result, _ctx| match result {
                            Some(name) => {
                                rename_text.set(name);
                                last_action.set(Some(true));
                            }
                            None => last_action.set(Some(false)),
                        })
                        .present(ctx);
                }
            });

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(GroupHeader::new(lit!("InputDialog")))
                .child(preview)
                .child(action_readout)
                .child(HStack::new().child(trigger).child(Spacer::new())),
        )
    }

    fn command_link_section(&self, _ctx: &mut BuildContext) -> impl Widget + 'static {
        // Icon assets are embedded at compile time via the `res!`
        // macro — same pattern as widget_catalog. The SVG bytes are
        // inlined into the binary and parsed once into an SvgIcon
        // resource; `IconWidget::from_svg_icon` then produces a fresh
        // tintable IconWidget on each build call.
        let save_icon = bastyde::res!("resources/icons/save.svg");
        let home_icon = bastyde::res!("resources/icons/home.svg");

        let new_project = CommandLinkButton::new(lit!("Create new project"))
            .description(lit!("Start with a blank workspace."))
            .icon(IconWidget::from_svg_icon(save_icon))
            .on_activate_fn(|_| println!("New project clicked"));
        let open_project = CommandLinkButton::new(lit!("Open existing project"))
            .description(lit!("Browse to a folder on disk."))
            .icon(IconWidget::from_svg_icon(home_icon))
            .on_activate_fn(|_| println!("Open project clicked"));

        Card::new()
            .header(TextWidget::new(lit!("CommandLinkButton")).style(TextStyleRole::BodyBold))
            .content(
                VStack::new()
                    .spacing(8.0)
                    .child(new_project)
                    .child(open_project),
            )
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let banner_id = self.banner_section(ctx);
        let search_id = self.search_and_picker_section(ctx);
        let input_id = self.input_dialog_section(ctx);
        let command_link = self.command_link_section(ctx);

        let body = VStack::new()
            .spacing(20.0)
            .add_child(banner_id)
            .add_child(search_id)
            .add_child(input_id)
            .child(command_link);

        let root = ctx.add(Panel::new().padding(24.0).child(body));
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
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

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .install_file_dialog()
        .initial_window(
            WindowConfig::new()
                .title("New widgets kit")
                .size(720, 720)
                .root(|tree, _state| {
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(Root::new())),
                    )
                }),
        )
        .run();
}
