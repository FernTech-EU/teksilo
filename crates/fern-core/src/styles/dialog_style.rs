//! Tier-3 style protocol for `Dialog` (`ModalContainer`). See
//! `docs/styling-system.md`.
//!
//! `Dialog` has two themable surfaces, so the trait carries two
//! methods: [`DialogStyle::make_panel`] wraps the `DialogContent`
//! subtree in the modal panel chrome (rounded surface, border, content
//! padding), and [`DialogStyle::make_scrim`] builds the full-window
//! dimming scrim painted behind the panel. The modal-presentation
//! pipeline owns *mounting* both — `DialogStyle` only owns their look.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct DialogStyleConfig {
    /// Pre-built `DialogContent` subtree the panel wraps.
    pub content: WidgetId,
    /// Whether the modal is presented with a dimming scrim behind it.
    /// `ModalContainer` is always modal today, so this is always
    /// `true`; custom styles may branch on it.
    pub has_scrim: bool,
    /// Caller override for the panel content padding — `None` means
    /// "use the recipe default". Custom styles may ignore it.
    pub padding_override: Option<f32>,
    /// Caller override for the panel minimum width — `None` means
    /// "use the recipe default". Custom styles may ignore it.
    pub min_width_override: Option<f32>,
}

pub trait DialogStyle: 'static {
    /// The modal panel surface that wraps `content` — rounded surface
    /// fill, border stroke, and the content-padding inset.
    fn make_panel(&self, cfg: &DialogStyleConfig, ctx: &mut BuildContext) -> WidgetId;
    /// The full-window scrim that dims the content behind the modal
    /// panel. Mounted by the modal-presentation pipeline, not by
    /// `ModalContainer` itself.
    fn make_scrim(&self, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedDialogStyle = Rc<dyn DialogStyle>;
