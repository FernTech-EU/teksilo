//! FilePickerField — a [`TextInput`](crate::text_input::TextInput) preset
//! with a trailing **Browse…** button that opens a native file dialog
//! and writes the chosen path back into the bound text signal.
//!
//! Three modes mirror the three single-result file-dialog kinds:
//! [`FilePickerKind::OpenFile`], [`FilePickerKind::PickFolder`], and
//! [`FilePickerKind::SaveFile`]. (Multi-file selection doesn't fit the
//! "one editable line" pattern; use the file-dialog API directly for
//! that.)
//!
//! ```ignore
//! let path = ctx.signal(String::new());
//! FilePickerField::new(path.clone())
//!     .kind(FilePickerKind::OpenFile)
//!     .add_filter("Images", &["png", "jpg"])
//!     .placeholder("Choose a file…")
//! ```

use std::path::PathBuf;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_platform::file_dialog::{EventContextFileDialogExt, FileDialogRequest, FileDialogResult};

use crate::icon_button::IconButton;
use crate::text_input::TextInput;

/// Which file-dialog kind the trailing button opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilePickerKind {
    /// Open an existing file. Default.
    #[default]
    OpenFile,
    /// Pick an existing folder.
    PickFolder,
    /// Pick a new or existing file location for saving.
    SaveFile,
}

type FilterEntry = (String, Vec<String>);

/// Convenience wrapper around [`TextInput`] preset for path entry.
pub struct FilePickerField {
    text: Signal<String>,
    kind: FilePickerKind,
    title: Option<String>,
    starting_dir: Option<PathBuf>,
    default_file_name: Option<String>,
    filters: Vec<FilterEntry>,
    on_pick: Option<Box<dyn Fn(&FileDialogResult, &mut EventContext)>>,
    placeholder: Option<String>,
    label: Option<String>,
    enabled: bool,
    root_child_id: Option<WidgetId>,
}

impl FilePickerField {
    /// Construct a `FilePickerField` bound to `text`. The visible string
    /// is updated on a successful pick; existing content is shown as-is.
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            kind: FilePickerKind::OpenFile,
            title: None,
            starting_dir: None,
            default_file_name: None,
            filters: Vec::new(),
            on_pick: None,
            placeholder: None,
            label: None,
            enabled: true,
            root_child_id: None,
        }
    }

    /// Pick the dialog kind opened by the Browse button.
    pub fn kind(mut self, kind: FilePickerKind) -> Self {
        self.kind = kind;
        self
    }

    /// Title shown in the file-dialog window caption.
    pub fn dialog_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Directory the dialog opens in. If not set, the OS default is used.
    pub fn starting_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.starting_dir = Some(path.into());
        self
    }

    /// Pre-filled file name for the [`FilePickerKind::SaveFile`] dialog.
    /// No-op for `OpenFile` / `PickFolder`.
    pub fn default_file_name(mut self, name: impl Into<String>) -> Self {
        self.default_file_name = Some(name.into());
        self
    }

    /// Append an extension filter (label + extensions without leading dots).
    /// Repeat to add multiple rows.
    pub fn add_filter(mut self, label: impl Into<String>, extensions: &[&str]) -> Self {
        self.filters.push((
            label.into(),
            extensions.iter().map(|s| (*s).to_string()).collect(),
        ));
        self
    }

    /// Hook invoked with the raw [`FileDialogResult`] after the dialog
    /// closes — useful when the caller needs to react to cancellation
    /// or backend errors. The bound text signal is already updated by
    /// the time this fires (on success).
    pub fn on_pick(mut self, f: impl Fn(&FileDialogResult, &mut EventContext) + 'static) -> Self {
        self.on_pick = Some(Box::new(f));
        self
    }

    /// Placeholder text shown when the field is empty.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// Accessible name for the path field.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Disable / re-enable the field (and the browse button).
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

impl std::fmt::Debug for FilePickerField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePickerField")
            .field("kind", &self.kind)
            .field("filters", &self.filters)
            .finish_non_exhaustive()
    }
}

fn build_request_owned(
    kind: FilePickerKind,
    title: Option<String>,
    starting_dir: Option<PathBuf>,
    default_file_name: Option<String>,
    filters: &[FilterEntry],
) -> FileDialogRequest {
    let mut req = match kind {
        FilePickerKind::OpenFile => FileDialogRequest::pick_file(),
        FilePickerKind::PickFolder => FileDialogRequest::pick_folder(),
        FilePickerKind::SaveFile => FileDialogRequest::save_file(),
    };
    if let Some(title) = title {
        req = req.title(title);
    }
    if let Some(dir) = starting_dir {
        req = req.starting_dir(dir);
    }
    if let Some(name) = default_file_name {
        req = req.default_file_name(name);
    }
    for (label, extensions) in filters {
        let exts: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
        req = req.add_filter(label.clone(), &exts);
    }
    req
}

impl Widget for FilePickerField {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Snapshot dialog config + result writer for the Browse-button
        // closure (which can't borrow `self`).
        let kind = self.kind;
        let title = self.title.clone();
        let starting_dir = self.starting_dir.clone();
        let default_file_name = self.default_file_name.clone();
        let filters = self.filters.clone();
        // Convert Box<dyn Fn> into Rc<dyn Fn> once so the inner
        // callback can be cloned into each per-tap result closure
        // (which must be FnOnce).
        let on_pick: Option<std::rc::Rc<dyn Fn(&FileDialogResult, &mut EventContext)>> =
            self.on_pick.take().map(std::rc::Rc::from);
        let text_signal = self.text.clone();

        let browse = IconButton::browse()
            .embedded()
            .enabled(self.enabled)
            .on_activate_fn(move |ctx| {
                let request = build_request_owned(
                    kind,
                    title.clone(),
                    starting_dir.clone(),
                    default_file_name.clone(),
                    &filters,
                );
                let text_signal = text_signal.clone();
                let on_pick = on_pick.clone();
                let result_cb = move |result: FileDialogResult, ctx: &mut EventContext| {
                    apply_result(&result, &text_signal, kind);
                    if let Some(handler) = &on_pick {
                        handler(&result, ctx);
                    }
                };
                let _ = match kind {
                    FilePickerKind::OpenFile => ctx.pick_file(request, result_cb),
                    FilePickerKind::PickFolder => ctx.pick_folder(request, result_cb),
                    FilePickerKind::SaveFile => ctx.save_file(request, result_cb),
                };
            });

        // Build the TextInput inline (matching DateEdit / TimeEdit) —
        // no Option<TextInput> storage, no map_input plumbing, just
        // direct construction from the FilePickerField's own config.
        let mut input = TextInput::new(self.text.clone())
            .enabled(self.enabled)
            .trailing_slot(browse);
        if let Some(ph) = self.placeholder.clone() {
            input = input.placeholder(ph);
        }
        if let Some(label) = self.label.clone() {
            input = input.label(label);
        }
        self.root_child_id = Some(ctx.add(input));
        self.children()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
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
        // The inner TextInput owns the text-edit role + value. The
        // outer container is a layout shell.
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn apply_result(result: &FileDialogResult, text: &Signal<String>, kind: FilePickerKind) {
    let path = match result {
        FileDialogResult::File(Some(p)) if matches!(kind, FilePickerKind::OpenFile) => Some(p),
        FileDialogResult::Folder(Some(p)) if matches!(kind, FilePickerKind::PickFolder) => Some(p),
        FileDialogResult::Saved(Some(p)) if matches!(kind, FilePickerKind::SaveFile) => Some(p),
        _ => None,
    };
    if let Some(p) = path {
        text.set(p.to_string_lossy().into_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    #[test]
    fn file_picker_builds() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let path = Signal::new(String::new());
        let id = tree.add(
            FilePickerField::new(path)
                .placeholder("Choose a file…")
                .add_filter("Images", &["png", "jpg"]),
        );
        tree.layout(SizeProposal {
            width: Some(420.0),
            height: None,
        });
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }
}
