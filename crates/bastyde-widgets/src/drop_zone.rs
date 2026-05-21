//! `DropZone` — a "drop files here" target for external (OS) drag-and-drop.
//!
//! A bordered, tinted region that accepts files / text / URLs dragged in from
//! the operating system (Finder, Explorer, Nautilus) or another application.
//! It reacts to hover (accept / reject highlight) and fires typed callbacks on
//! drop. Because an OS drag cannot be initiated from the keyboard, the zone
//! also offers a keyboard-operable **Browse…** button (opening the native file
//! dialog) as the WCAG 2.1.1 equivalent.
//!
//! ```ignore
//! DropZone::new(tr!("drop_images_here"))
//!     .subtitle(tr!("png_or_jpeg"))
//!     .accept_extensions(["png", "jpg", "jpeg"])
//!     .allow_multiple(true)
//!     .on_files_dropped(|paths, _ctx| { /* import paths */ });
//! ```
//!
//! External drops are delivered through the framework's normal drag pipeline
//! (`on_drag_hover` / `on_drag_leave` / `on_drop`) once
//! [`install_external_dnd`](https://docs.rs/bastyde-app) is wired and a backend
//! is available; on platforms with no backend (e.g. X11) the Browse button
//! keeps the zone fully usable.
//!
//! # Styling
//!
//! The bordered, tinted chrome is a Tier-3 [`DropZoneStyle`]; the default
//! [`RecipeDropZoneStyle`](crate::styles::RecipeDropZoneStyle) tracks the
//! interaction state. Override per-call with [`DropZone::style`] or theme-wide
//! via `theme.style_slots.drop_zone`.
//!
//! # Accessibility
//!
//! The zone is a `Role::Group` labelled by its prompt, with a `Live::Polite`
//! status line that announces hover ("Drop to add 3 files"), success
//! ("3 files added"), and rejection. AccessKit models no drag/drop action and
//! ARIA's `aria-grabbed` / `aria-dropeffect` are deprecated, so live-region
//! announcements plus the Browse fallback are the supported pattern.

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::{Live, Role};
use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{
    DropZoneStyle, DropZoneStyleConfig, DropZoneVisualState, SharedDropZoneStyle,
};
use bastyde_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::{HandlerSet, WidgetBuilder};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::{DragPayload, DropFeedback};
use bastyde_platform::file_dialog::{
    EventContextFileDialogExt, FileDialogRequest, FileDialogResult,
};
use bastyde_tokens::{HAlignment, TextRole};

use crate::button::Button;
use crate::primitives::{TextWidget, VStack};

type FilesCallback = Box<dyn FnMut(Vec<PathBuf>, &mut EventContext)>;
type TextCallback = Box<dyn FnMut(String, &mut EventContext)>;
type UrlsCallback = Box<dyn FnMut(Vec<String>, &mut EventContext)>;

/// A drop target for external (OS) drag-and-drop. See the module docs.
pub struct DropZone {
    label: String,
    subtitle: Option<String>,
    browse_label: String,
    extensions: Vec<String>,
    allow_multiple: bool,
    show_browse_button: bool,
    icon: Option<Box<dyn Widget>>,
    on_files: Option<FilesCallback>,
    on_text: Option<TextCallback>,
    on_urls: Option<UrlsCallback>,
    style_override: Option<SharedDropZoneStyle>,
    root_child_id: Option<WidgetId>,
}

impl DropZone {
    /// Build a drop zone with the given prompt (e.g. `tr!("drop_files_here")`).
    /// The label may come from `tr!(...)` (translated) or
    /// `LocalizedString::literal(...)`; it is resolved eagerly at construction
    /// and stored as a `String`. Locale changes rebuild the composite parent,
    /// which re-creates the `DropZone` with a fresh translation — the same
    /// model as [`Button::new`](crate::button::Button::new).
    pub fn new(label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        Self {
            label: label.into().resolve_now(),
            subtitle: None,
            browse_label: bastyde_i18n::LocalizedString::literal("Browse…").resolve_now(),
            extensions: Vec::new(),
            allow_multiple: true,
            show_browse_button: true,
            icon: None,
            on_files: None,
            on_text: None,
            on_urls: None,
            style_override: None,
            root_child_id: None,
        }
    }

    /// Shim (`#[doc(hidden)]`) — wraps a raw label in
    /// `LocalizedString::literal` for tests and scaffolding. Production code
    /// uses `new(tr!(...))`; the `_literal` suffix is the grep marker for
    /// untranslated strings.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(bastyde_i18n::LocalizedString::literal(label))
    }

    /// Secondary line under the prompt (e.g. `tr!("png_or_jpeg")`).
    pub fn subtitle(mut self, text: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        self.subtitle = Some(text.into().resolve_now());
        self
    }

    /// `#[doc(hidden)]` untranslated twin of [`Self::subtitle`].
    #[doc(hidden)]
    pub fn subtitle_literal(self, text: impl Into<String>) -> Self {
        self.subtitle(bastyde_i18n::LocalizedString::literal(text))
    }

    /// Restrict accepted files to these extensions (without leading dots,
    /// case-insensitive). Empty (the default) accepts any file. Text and URL
    /// drops are unaffected.
    pub fn accept_extensions<I, S>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extensions = extensions
            .into_iter()
            .map(|e| e.into().trim_start_matches('.').to_ascii_lowercase())
            .collect();
        self
    }

    /// Whether more than one file may be dropped at once. Default `true`.
    /// When `false`, a multi-file drop is rejected.
    pub fn allow_multiple(mut self, allow: bool) -> Self {
        self.allow_multiple = allow;
        self
    }

    /// Show or hide the keyboard-operable Browse button. Default `true`.
    /// Keeping it visible is strongly recommended — it is the only
    /// keyboard-accessible path to the zone's action.
    pub fn show_browse_button(mut self, show: bool) -> Self {
        self.show_browse_button = show;
        self
    }

    /// Override the Browse button's label (e.g. `tr!("browse")`).
    pub fn browse_label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        self.browse_label = label.into().resolve_now();
        self
    }

    /// `#[doc(hidden)]` untranslated twin of [`Self::browse_label`].
    #[doc(hidden)]
    pub fn browse_label_literal(self, label: impl Into<String>) -> Self {
        self.browse_label(bastyde_i18n::LocalizedString::literal(label))
    }

    /// An icon widget shown above the prompt (any widget — typically an
    /// [`IconWidget`](crate::primitives::IconWidget)).
    pub fn icon(mut self, icon: impl Widget + 'static) -> Self {
        self.icon = Some(Box::new(icon));
        self
    }

    /// Override the Tier-3 [`DropZoneStyle`] for this instance only.
    pub fn style(mut self, style: impl DropZoneStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Called with the dropped (or browsed) file paths. Files are only
    /// accepted when this is set.
    pub fn on_files_dropped(
        mut self,
        f: impl FnMut(Vec<PathBuf>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_files = Some(Box::new(f));
        self
    }

    /// Called with dropped plain text. Text drops are only accepted when set.
    pub fn on_text_dropped(mut self, f: impl FnMut(String, &mut EventContext) + 'static) -> Self {
        self.on_text = Some(Box::new(f));
        self
    }

    /// Called with dropped non-file URLs. URL drops are only accepted when set.
    pub fn on_urls_dropped(
        mut self,
        f: impl FnMut(Vec<String>, &mut EventContext) + 'static,
    ) -> Self {
        self.on_urls = Some(Box::new(f));
        self
    }
}

impl std::fmt::Debug for DropZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropZone")
            .field("label", &self.label)
            .field("extensions", &self.extensions)
            .field("allow_multiple", &self.allow_multiple)
            .finish_non_exhaustive()
    }
}

/// Decide whether `payload` is acceptable given the zone's policy. Free
/// function so the drag closures don't need to borrow `self`.
fn payload_accepted(
    payload: &DragPayload,
    extensions: &[String],
    allow_multiple: bool,
    has_files_cb: bool,
    has_text_cb: bool,
    has_urls_cb: bool,
) -> bool {
    let files = payload.files();
    if !files.is_empty() {
        if !has_files_cb {
            return false;
        }
        if !allow_multiple && files.len() > 1 {
            return false;
        }
        if extensions.is_empty() {
            return true;
        }
        return files.iter().all(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
                .unwrap_or(false)
        });
    }
    if payload.text().is_some() {
        return has_text_cb;
    }
    if !payload.uris().is_empty() {
        return has_urls_cb;
    }
    // No concrete data yet — on Wayland the bytes only arrive at drop, so the
    // hover decision is made from the advertised formats. Optimistic: accept if
    // the zone handles a kind the source offers; the real extension check runs
    // at drop once `files()` is populated.
    if payload.is_external() {
        let formats = payload.formats();
        let offers = |needles: &[&str]| {
            formats
                .iter()
                .any(|f| needles.iter().any(|n| f == n || f.starts_with(n)))
        };
        if has_files_cb && offers(&["text/uri-list"]) {
            return true;
        }
        if has_text_cb && offers(&["text/plain", "UTF8_STRING", "STRING", "TEXT"]) {
            return true;
        }
        if has_urls_cb && offers(&["text/x-moz-url", "text/uri-list", "_NETSCAPE_URL"]) {
            return true;
        }
    }
    false
}

/// A short human description of what's being dragged, for announcements.
fn describe(payload: &DragPayload) -> String {
    let n = payload.files().len();
    if n == 1 {
        return "1 file".to_string();
    }
    if n > 1 {
        return format!("{n} files");
    }
    if payload.text().is_some() {
        return "text".to_string();
    }
    if !payload.uris().is_empty() {
        let n = payload.uris().len();
        return if n == 1 {
            "1 link".to_string()
        } else {
            format!("{n} links")
        };
    }
    "item".to_string()
}

impl Widget for DropZone {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = ctx.signal(DropZoneVisualState::Idle);
        let announce = ctx.signal(String::new());

        // Snapshots for the closures.
        let extensions = self.extensions.clone();
        let allow_multiple = self.allow_multiple;
        let has_files_cb = self.on_files.is_some();
        let has_text_cb = self.on_text.is_some();
        let has_urls_cb = self.on_urls.is_some();

        let on_files = self.on_files.take().map(|f| Rc::new(RefCell::new(f)));
        let on_text = self.on_text.take().map(|f| Rc::new(RefCell::new(f)));
        let on_urls = self.on_urls.take().map(|f| Rc::new(RefCell::new(f)));

        // --- Content column: [icon?] prompt [subtitle?] [status] [Browse?] ---
        let mut content = VStack::new().spacing(8.0).alignment(HAlignment::Center);

        if let Some(icon) = self.icon.take() {
            let icon_id = ctx.add_boxed(icon);
            content = content.add_child(icon_id);
        }

        content = content.child(TextWidget::new(lit!(self.label.clone())));

        if let Some(subtitle) = &self.subtitle {
            content =
                content.child(TextWidget::new(lit!(subtitle.clone())).color(TextRole::Secondary));
        }

        // Live-region status line: empty at rest, narrates hover / drop.
        content = content.child(
            TextWidget::new(lit!(String::new()))
                .bind_text(announce.clone())
                .color(TextRole::Secondary)
                .access_live(Live::Polite),
        );

        if self.show_browse_button {
            let browse_extensions = self.extensions.clone();
            let allow_multiple_browse = self.allow_multiple;
            let on_files_browse = on_files.clone();
            let announce_browse = announce.clone();
            let browse = Button::new(lit!(self.browse_label.clone())).on_activate_fn(
                move |ctx: &mut EventContext| {
                    let mut request = FileDialogRequest::pick_file();
                    if !browse_extensions.is_empty() {
                        let exts: Vec<&str> =
                            browse_extensions.iter().map(String::as_str).collect();
                        request = request.add_filter("Allowed", &exts);
                    }
                    let on_files_cb = on_files_browse.clone();
                    let announce_cb = announce_browse.clone();
                    let result_cb = move |result: FileDialogResult, ctx: &mut EventContext| {
                        let paths = match result {
                            FileDialogResult::File(Some(p)) => vec![p],
                            FileDialogResult::Files(v) => v,
                            _ => Vec::new(),
                        };
                        if paths.is_empty() {
                            return;
                        }
                        let count = paths.len();
                        if let Some(cb) = &on_files_cb {
                            (cb.borrow_mut())(paths, ctx);
                        }
                        announce_cb.set(format!("{count} added"));
                    };
                    // Multi vs single picker per policy. Errors (no dialog
                    // installed) are ignored — the zone stays usable.
                    let _ = if allow_multiple_browse {
                        ctx.pick_files(request, result_cb)
                    } else {
                        ctx.pick_file(request, result_cb)
                    };
                },
            );
            content = content.child(browse);
        }

        let content_id = ctx.add(content);

        // --- Tier-3 chrome: resolve style (per-call > theme slot > default) ---
        let style = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.drop_zone.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeDropZoneStyle));
        let body = style.make_body(
            &DropZoneStyleConfig {
                state: state.clone(),
                content: content_id,
            },
            ctx,
        );

        // --- Drag behaviour on the composite node (the drop target) ---
        let hover_state = state.clone();
        let hover_announce = announce.clone();
        let hover_exts = extensions.clone();
        let leave_state = state.clone();
        let leave_announce = announce.clone();
        let drop_exts = extensions;

        let handlers = HandlerSet::new()
            .on_drag_hover(move |payload, _pos, _ctx| {
                let ok = payload_accepted(
                    payload,
                    &hover_exts,
                    allow_multiple,
                    has_files_cb,
                    has_text_cb,
                    has_urls_cb,
                );
                if ok {
                    hover_state.set(DropZoneVisualState::HoverAccept);
                    hover_announce.set(format!("Drop to add {}", describe(payload)));
                } else {
                    hover_state.set(DropZoneVisualState::HoverReject);
                    hover_announce.set("This item can't be dropped here".to_string());
                }
                // Visuals are state-driven; no framework-drawn feedback.
                DropFeedback::NoFeedback
            })
            .on_drag_leave(move |_ctx| {
                leave_state.set(DropZoneVisualState::Idle);
                leave_announce.set(String::new());
            })
            .on_drop(move |payload, _pos, ctx| {
                let ok = payload_accepted(
                    &payload,
                    &drop_exts,
                    allow_multiple,
                    has_files_cb,
                    has_text_cb,
                    has_urls_cb,
                );
                state.set(DropZoneVisualState::Idle);
                if !ok {
                    announce.set("Item not accepted".to_string());
                    return false;
                }
                let summary = describe(&payload);
                if !payload.files().is_empty() {
                    if let Some(cb) = &on_files {
                        (cb.borrow_mut())(payload.files().to_vec(), ctx);
                    }
                } else if let Some(text) = payload.text() {
                    if let Some(cb) = &on_text {
                        (cb.borrow_mut())(text.to_string(), ctx);
                    }
                } else if !payload.uris().is_empty() {
                    if let Some(cb) = &on_urls {
                        (cb.borrow_mut())(payload.uris().to_vec(), ctx);
                    }
                }
                announce.set(format!("Added {summary}"));
                true
            });
        ctx.apply_self_handlers(handlers);

        self.root_child_id = Some(body);
        self.children()
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The composite node is the drop target and the labelled group; the
        // Live status line lives inside the content column.
        builder.set_role(Role::Group);
        builder.set_name(self.label.clone());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::Point;
    use bastyde_core::ExternalDropData;
    use bastyde_core::widget_tree::WidgetTree;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
    }

    #[test]
    fn builds_with_nonzero_size() {
        let mut tree = tree();
        let id = tree.add(DropZone::new(lit!("Drop files here")));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let b = tree.bounds(id);
        assert!(b.width > 0.0 && b.height > 0.0);
    }

    #[test]
    fn matching_file_drop_fires_callback() {
        let mut tree = tree();
        let got: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let g = got.clone();
        tree.add(
            DropZone::new(lit!("Images"))
                .accept_extensions(["png", "jpg"])
                .on_files_dropped(move |paths, _ctx| *g.borrow_mut() = paths),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = bastyde_core::NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/photo.png")],
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data.clone(), &mut noop);
        tree.end_external_drag(p, data, &mut noop);

        assert_eq!(*got.borrow(), vec![PathBuf::from("/tmp/photo.png")]);
    }

    #[test]
    fn wrong_extension_is_rejected() {
        let mut tree = tree();
        let got: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let g = got.clone();
        tree.add(
            DropZone::new(lit!("Images"))
                .accept_extensions(["png"])
                .on_files_dropped(move |paths, _ctx| *g.borrow_mut() = paths),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = bastyde_core::NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/notes.txt")],
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data.clone(), &mut noop);
        tree.end_external_drag(p, data, &mut noop);

        assert!(got.borrow().is_empty(), "non-png drop must be rejected");
    }

    #[test]
    fn multi_file_rejected_when_single_only() {
        let mut tree = tree();
        let got: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let g = got.clone();
        tree.add(
            DropZone::new(lit!("One file"))
                .allow_multiple(false)
                .on_files_dropped(move |paths, _ctx| *g.borrow_mut() = paths),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = bastyde_core::NoopWindowOps;
        let data = ExternalDropData {
            files: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data.clone(), &mut noop);
        tree.end_external_drag(p, data, &mut noop);

        assert!(got.borrow().is_empty(), "multi-file drop must be rejected");
    }

    #[test]
    fn text_drop_fires_when_handler_set() {
        let mut tree = tree();
        let got: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let g = got.clone();
        tree.add(
            DropZone::new(lit!("Notes")).on_text_dropped(move |t, _ctx| *g.borrow_mut() = Some(t)),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let mut noop = bastyde_core::NoopWindowOps;
        let data = ExternalDropData {
            text: Some("hello".to_string()),
            ..Default::default()
        };
        let p = Point::new(200.0, 150.0);
        tree.begin_external_drag(p, data.clone(), &mut noop);
        tree.end_external_drag(p, data, &mut noop);

        assert_eq!(got.borrow().as_deref(), Some("hello"));
    }

    // --- Hover-time acceptance from advertised formats (Wayland) -------
    // On Wayland the dropped bytes only arrive at drop, so hover accept/reject
    // is decided from the advertised MIME formats alone.

    #[test]
    fn formats_only_hover_accepts_matching_kind() {
        // A file drag advertises text/uri-list (+ text/plain for the path).
        let file_drag = DragPayload::external(ExternalDropData {
            formats: vec!["text/uri-list".into(), "text/plain".into()],
            ..Default::default()
        });
        // Image-style zone: files handler, png filter — accept on hover even
        // though the extension can't be checked until drop.
        assert!(payload_accepted(
            &file_drag,
            &["png".into()],
            true,
            true,
            false,
            false
        ));

        // A pure text drag (no uri-list) onto a files-only zone → reject.
        let text_drag = DragPayload::external(ExternalDropData {
            formats: vec!["text/plain".into()],
            ..Default::default()
        });
        assert!(!payload_accepted(&text_drag, &[], true, true, false, false));
        // …but a text-handling zone accepts it.
        assert!(payload_accepted(&text_drag, &[], true, false, true, false));
    }

    #[test]
    fn formats_only_internal_drag_is_not_accepted() {
        // A non-external payload with no concrete data must not be accepted via
        // the formats path.
        let internal = DragPayload::typed(7_u32);
        assert!(!payload_accepted(&internal, &[], true, true, true, true));
    }
}
