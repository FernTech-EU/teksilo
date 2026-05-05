//! Native file-dialog showcase.
//!
//! Demonstrates all four operations exposed by
//! [`EventContextFileDialogExt`]:
//!
//! - Open a single file
//! - Open multiple files
//! - Pick a folder
//! - Save a file
//!
//! Each button writes the result into a `Signal<String>` that drives a
//! status line below — confirming the event loop kept ticking while
//! the OS dialog was up. Run with: `cargo run -p file-dialogs`.

use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonVariant, HStack, Panel, Spinner, Switcher, TextWidget, VStack,
};

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .install_inspector_in_debug()
        .install_file_dialog()
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Native File Dialogs")
                .size(720, 420)
                .root(|tree, _state| tree.add(FileDialogShowcase::new())),
        )
        .run();
}

#[derive(Debug)]
struct FileDialogShowcase {
    status: Signal<String>,
    /// Pulses the spinner whenever a dialog is in flight to prove the
    /// event loop is still ticking.
    spinning: Signal<bool>,
    root: Option<WidgetId>,
}

impl FileDialogShowcase {
    fn new() -> Self {
        Self {
            status: Signal::new(String::from("Click any button to open a native dialog.")),
            spinning: Signal::new(false),
            root: None,
        }
    }
}

impl Widget for FileDialogShowcase {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let status_for_open = self.status.clone();
        let spinning_for_open = self.spinning.clone();
        let open_btn = Button::new_literal("Open file…")
            .style(ButtonVariant::Default)
            .on_activate_fn(move |ctx| {
                spinning_for_open.set(true);
                let status = status_for_open.clone();
                let spinning = spinning_for_open.clone();
                let req = FileDialogRequest::pick_file().title("Pick any file");
                let _ = ctx.pick_file(req, move |result, _ctx| {
                    spinning.set(false);
                    let msg = match result {
                        FileDialogResult::File(Some(p)) => format!("Opened: {}", p.display()),
                        FileDialogResult::File(None) => "Open cancelled.".into(),
                        FileDialogResult::Error(e) => format!("Error: {e}"),
                        _ => "Unexpected result.".into(),
                    };
                    status.set(msg);
                });
            });

        let status_for_multi = self.status.clone();
        let spinning_for_multi = self.spinning.clone();
        let multi_btn = Button::new_literal("Open multiple files…").on_activate_fn(move |ctx| {
            spinning_for_multi.set(true);
            let status = status_for_multi.clone();
            let spinning = spinning_for_multi.clone();
            let req = FileDialogRequest::pick_files()
                .title("Pick one or more files")
                .add_filter("Text", &["txt", "md"])
                .add_filter("All files", &["*"]);
            let _ = ctx.pick_files(req, move |result, _ctx| {
                spinning.set(false);
                let msg = match result {
                    FileDialogResult::Files(paths) if paths.is_empty() => "Open cancelled.".into(),
                    FileDialogResult::Files(paths) => format!(
                        "Opened {}: {}",
                        paths.len(),
                        paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    FileDialogResult::Error(e) => format!("Error: {e}"),
                    _ => "Unexpected result.".into(),
                };
                status.set(msg);
            });
        });

        let status_for_folder = self.status.clone();
        let spinning_for_folder = self.spinning.clone();
        let folder_btn = Button::new_literal("Pick folder…").on_activate_fn(move |ctx| {
            spinning_for_folder.set(true);
            let status = status_for_folder.clone();
            let spinning = spinning_for_folder.clone();
            let req = FileDialogRequest::pick_folder().title("Pick a folder");
            let _ = ctx.pick_folder(req, move |result, _ctx| {
                spinning.set(false);
                let msg = match result {
                    FileDialogResult::Folder(Some(p)) => format!("Folder: {}", p.display()),
                    FileDialogResult::Folder(None) => "Folder pick cancelled.".into(),
                    FileDialogResult::Error(e) => format!("Error: {e}"),
                    _ => "Unexpected result.".into(),
                };
                status.set(msg);
            });
        });

        let status_for_save = self.status.clone();
        let spinning_for_save = self.spinning.clone();
        let save_btn = Button::new_literal("Save file…").on_activate_fn(move |ctx| {
            spinning_for_save.set(true);
            let status = status_for_save.clone();
            let spinning = spinning_for_save.clone();
            let req = FileDialogRequest::save_file()
                .title("Save a sample file")
                .default_file_name("sample.txt")
                .add_filter("Text", &["txt"]);
            let _ = ctx.save_file(req, move |result, _ctx| {
                spinning.set(false);
                let msg = match result {
                    FileDialogResult::Saved(Some(p)) => {
                        // Write a tiny placeholder so the user sees
                        // a real file appear at the chosen path.
                        match std::fs::write(&p, b"Hello from FernUI's file-dialog demo.\n") {
                            Ok(()) => format!("Saved to: {}", p.display()),
                            Err(e) => format!("Save failed: {e}"),
                        }
                    }
                    FileDialogResult::Saved(None) => "Save cancelled.".into(),
                    FileDialogResult::Error(e) => format!("Error: {e}"),
                    _ => "Unexpected result.".into(),
                };
                status.set(msg);
            });
        });

        let header = TextWidget::new_literal("Native File Dialogs").style(TextStyleRole::BodyBold);
        let intro = TextWidget::new_literal(
            "Each button opens a real OS dialog. The spinner keeps animating \
             while the dialog is up — the event loop is not blocked.",
        )
        .style(TextStyleRole::Body);

        let buttons = HStack::new()
            .spacing(8.0)
            .child(open_btn)
            .child(multi_btn)
            .child(folder_btn)
            .child(save_btn);

        let status_text = TextWidget::new_literal("status")
            .bind_text(self.status.clone())
            .style(TextStyleRole::Small);

        // Spinner pulses while a dialog is in flight, proving the
        // FernUI event loop keeps ticking. A Switcher driven by
        // `spinning` swaps an empty placeholder for the live spinner.
        let switch = self.spinning.map(|b| if *b { 1_usize } else { 0_usize });
        let spinner = Switcher::new(switch)
            .child(TextWidget::new_literal(""))
            .child(Spinner::new(20.0));

        let id = ctx.add(
            Panel::new().padding(20.0).child(
                VStack::new()
                    .spacing(14.0)
                    .child(header)
                    .child(intro)
                    .child(buttons)
                    .child(
                        HStack::new()
                            .spacing(10.0)
                            .child(spinner)
                            .child(status_text),
                    ),
            ),
        );
        self.root = Some(id);
        vec![id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}
