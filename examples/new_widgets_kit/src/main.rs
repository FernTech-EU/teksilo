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

use fern_ui::core::widget::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Banner, Button, ButtonVariant, Card, CommandLinkButton, FilePickerField, FilePickerKind,
    GroupHeader, HStack, Panel, SearchField, Spacer, TextWidget, VStack,
};

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

        let info_banner = Banner::info_literal("Welcome to FernUI")
            .description_literal("Persistent inline status strips for app-level conditions.");
        let warn_banner = Banner::warning_literal("Unsaved changes")
            .description_literal("Closing the document now will discard your edits.")
            .action(
                Button::new_literal("Save now")
                    .style(ButtonVariant::Regular)
                    .on_activate_fn(|_| println!("Save now clicked")),
            )
            .on_dismiss({
                let s = warn_visible.clone();
                move |_| s.set(false)
            });
        let error_banner = Banner::error_literal("Disk almost full")
            .description_literal("Less than 200 MB remaining on /Users/you.")
            .on_dismiss({
                let s = error_visible.clone();
                move |_| s.set(false)
            });

        let info_id = ctx.add(info_banner);
        let warn_id = ctx.add(warn_banner);
        let error_id = ctx.add(error_banner);
        ctx.visible_when(info_id, info_visible);
        ctx.visible_when(warn_id, warn_visible);
        ctx.visible_when(error_id, error_visible);

        let restore = Button::new_literal("Restore banners")
            .style(ButtonVariant::Flat)
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
                .child(GroupHeader::new_literal("Banner"))
                .add_child(info_id)
                .add_child(warn_id)
                .add_child(error_id)
                .child(HStack::new().child(Spacer::new()).child(restore)),
        )
    }

    fn search_and_picker_section(&self, ctx: &mut BuildContext) -> WidgetId {
        let search = SearchField::new(self.search_text.clone())
            .placeholder("Filter library…")
            .on_submit_fn({
                let s = self.search_text.clone();
                move |_| println!("submit search: {:?}", s.get())
            });

        let picker = FilePickerField::new(self.path_text.clone())
            .kind(FilePickerKind::OpenFile)
            .placeholder("No file selected")
            .add_filter("Text", &["txt", "md"])
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .dialog_title("Choose a file");

        ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(GroupHeader::new_literal("SearchField & FilePickerField"))
                .child(search)
                .child(picker),
        )
    }

    fn input_dialog_section(&self, ctx: &mut BuildContext) -> WidgetId {
        let rename_text = self.rename_text.clone();
        let preview = TextWidget::new_literal("")
            .bind_text(rename_text.map(|s| format!("Current name: {}", s)))
            .color(TextRole::Secondary);

        let trigger = Button::new_literal("Rename…")
            .style(ButtonVariant::Regular)
            .on_activate_fn({
                let rename_text = rename_text.clone();
                move |ctx| {
                    let rename_text = rename_text.clone();
                    use fern_ui::widgets::InputDialog;
                    InputDialog::new_literal("Rename document")
                        .prompt_literal("Enter the new file name:")
                        .default_text(rename_text.get())
                        .placeholder("filename.ext")
                        .on_result(move |result, _ctx| match result {
                            Some(name) => rename_text.set(name),
                            None => println!("Rename cancelled"),
                        })
                        .present(ctx);
                }
            });

        ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(GroupHeader::new_literal("InputDialog"))
                .child(preview)
                .child(HStack::new().child(trigger).child(Spacer::new())),
        )
    }

    fn command_link_section(&self, _ctx: &mut BuildContext) -> impl Widget + 'static {
        let new_project = CommandLinkButton::new_literal("Create new project")
            .description_literal("Start with a blank workspace.")
            .on_activate_fn(|_| println!("New project clicked"));
        let open_project = CommandLinkButton::new_literal("Open existing project")
            .description_literal("Browse to a folder on disk.")
            .on_activate_fn(|_| println!("Open project clicked"));

        Card::new()
            .header(TextWidget::new_literal("CommandLinkButton").style(TextStyleRole::BodyBold))
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
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .install_file_dialog()
        .initial_window(
            WindowConfig::new()
                .title("New widgets kit")
                .size(720, 720)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
