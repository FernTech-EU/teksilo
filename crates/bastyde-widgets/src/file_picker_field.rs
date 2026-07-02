// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `FilePickerField` — a text-input preset for path entry with a Browse button.
//!
//! Combines a `TextInput` with a trailing `IconButton` (the folder/browse glyph)
//! that opens a native file dialog and writes the chosen path back into the bound
//! `Signal<String>`. The three [`FilePickerKind`] variants map to the three
//! single-result dialog modes: open a file, pick a folder, or save a file.
//! Multi-file selection does not fit the "one editable line" pattern; use the
//! file-dialog API directly for that.
//!
//! ```ignore
//! // Requires ctx.signal() — shown as ignore per convention.
//! let path = ctx.signal(String::new());
//! let _f = FilePickerField::new(path.clone())
//!     .kind(FilePickerKind::OpenFile)
//!     .add_filter("Images", &["png", "jpg"])
//!     .placeholder(lit!("Choose a file…"));
//! ```

use std::path::PathBuf;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_platform::file_dialog::{
    EventContextFileDialogExt, FileDialogRequest, FileDialogResult,
};

use crate::icon_button::IconButton;
use crate::text_input::{TextInput, ValidationState};
use bastyde_i18n::LocalizedString;

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

/// A single-line path entry field with a trailing Browse button that invokes the
/// native file dialog and writes the chosen path back into the bound `Signal<String>`.
pub struct FilePickerField {
    text: Signal<String>,
    kind: FilePickerKind,
    title: Option<LocalizedString>,
    starting_dir: Option<PathBuf>,
    default_file_name: Option<String>,
    filters: Vec<FilterEntry>,
    on_pick: Option<Box<dyn Fn(&FileDialogResult, &mut EventContext)>>,
    placeholder: Option<LocalizedString>,
    label: Option<LocalizedString>,
    /// Optional external validation state, forwarded to the inner `TextInput`
    /// (renders the same inline error/warning strip + border tint as a plain
    /// text field).
    validation: Option<Prop<ValidationState>>,
    /// Initial enabled-state; forwarded to the arena at build time.
    enabled: Prop<bool>,
    root_child_id: Option<WidgetId>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
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
            validation: None,
            enabled: Prop::Static(true),
            root_child_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Pick the dialog kind opened by the Browse button.
    pub fn kind(mut self, kind: FilePickerKind) -> Self {
        self.kind = kind;
        self
    }

    /// Title shown in the file-dialog window caption.
    pub fn dialog_title(mut self, title: impl Into<LocalizedString>) -> Self {
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
    pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.placeholder = Some(ls);
        self
    }

    /// Accessible name for the path field.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Bind an external [`ValidationState`] signal — shown as the same inline
    /// error/warning strip and border tint the inner [`TextInput`] renders (e.g.
    /// "the chosen folder does not exist / is not writable").
    pub fn validation(mut self, validation: impl Into<Prop<ValidationState>>) -> Self {
        self.validation = Some(validation.into());
        self
    }

    /// Set the initial enabled state for the text field and Browse button.
    /// Forwarded to the arena at build time.
    pub fn enabled(mut self, on: impl Into<Prop<bool>>) -> Self {
        self.enabled = on.into();
        self
    }

    /// Attach a plain single-line tooltip shown after the hover delay.
    /// Clears any previously set rich or composite tooltip (last call wins).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip by registry key.
    /// Clears any previously set plain or composite tooltip (last call wins).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from inline [`crate::tooltip::TooltipContent`].
    /// Clears any previously set plain or composite tooltip (last call wins).
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    /// Clears any previously set plain or rich tooltip (last call wins).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
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
    title: Option<LocalizedString>,
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
        req = req.title(title.resolve_now());
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
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());

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
            .enabled(self.enabled.clone())
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
            .enabled(self.enabled.clone())
            .trailing_slot(browse);
        if let Some(ph) = self.placeholder.clone() {
            input = input.placeholder(ph);
        }
        if let Some(label) = self.label.clone() {
            input = input.label(label);
        }
        if let Some(validation) = self.validation.clone() {
            input = input.validation(validation);
        }
        let root_id = ctx.add(input);
        self.root_child_id = Some(root_id);

        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        self.children()
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
        // The inner TextInput owns the text-edit role + value. The
        // outer container is a layout shell.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
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
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[test]
    fn file_picker_builds() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let path = Signal::new(String::new());
        let id = tree.add(
            FilePickerField::new(path)
                .placeholder(lit!("Choose a file…"))
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

    #[test]
    fn tooltip_appears_on_hover() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let path = Signal::new(String::new());
        let id = tree.add(FilePickerField::new(path).tooltip(lit!("Tip")));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: Some(200.0),
        });
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("Tip").is_some());
    }
}
