// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Modal dialogs — a trigger button that presents a centered modal panel.
//!
//! Three cooperating types cover the common dialog use-case. [`Dialog`] is the
//! high-level entry point: a `Button` (or custom trigger) that, on activation,
//! presents a `ModalContainer` above a full-viewport dimming [`ModalScrim`].
//! [`DialogContent`] is the convenience body layout — a `VStack` with an
//! optional title, supporting text, scrollable body slot, and a footer slot
//! separated by a `Divider`.
//!
//! ## When to use
//!
//! - `Dialog::new(label).content(|| …)` for the common "button opens dialog" pattern.
//! - `Dialog::new(label).trigger(my_icon_button).content(|| …)` to use a custom widget
//!   as the trigger instead of the default `Button`.
//! - `ModalContainer::new(content)` directly when you need to present a modal from
//!   handler code via `ctx.present_modal(ModalRequest::…)` rather than a persistent
//!   trigger.
//!
//! ## Accessibility
//!
//! `ModalContainer` is a `Role::Dialog` node and announces `set_modal()`.
//! Its accessible name defaults to the `DialogContent` title (via
//! `Widget::accessible_title_hint`) or falls back to the localized
//! `a11y_dialog_name` message; pass `.title(tr!(…))` to the container for an
//! explicit override. The trigger button advertises `HasPopup::Dialog` and
//! `set_expanded` tracks whether the modal is currently open.
//!
//! ```ignore
//! use teksilo_widgets::dialog::{Dialog, DialogContent};
//! use teksilo_i18n::lit;
//!
//! let _d = Dialog::new(lit!("Open settings"))
//!     .content(|| {
//!         DialogContent::new()
//!             .title(lit!("Settings"))
//!             .supporting_text(lit!("Adjust your preferences below."))
//!     });
//! ```

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use teksilo_core::overlay::{OverlayDismissCallback, OverlayId};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{DialogStyleConfig, SharedDialogStyle};
use teksilo_core::widget::{EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{TextRole, TextStyleRole};

use crate::button::{Button, ButtonVariant};
use crate::overlay_trigger::OverlayTrigger;
use crate::primitives::{Divider, TextWidget, VStack};
use teksilo_i18n::LocalizedString;

type DialogFactory = std::rc::Rc<dyn Fn() -> Box<dyn Widget>>;

/// Rounded panel chrome that wraps a modal dialog's content widget.
///
/// All visual dimensions (padding, corner radius, min-width, shadow) are owned
/// by the active [`DialogStyle`](teksilo_core::styles::DialogStyle); per-instance
/// overrides are available via [`Self::padding`] and [`Self::min_width`].
pub struct ModalContainer {
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn Widget>>,
    padding_override: Option<f32>,
    min_width_override: Option<f32>,
    /// Explicit accessible title for the dialog. Set via `.title(...)`
    /// — typically the same string the inner `DialogContent` uses as
    /// its visual title. When `None`, `accessibility()` falls back to
    /// the generic i18n `a11y_dialog_name` string so there's always
    /// a non-empty name for screen readers.
    /// AT name for the `Role::Dialog` node. Kept as a `LocalizedString`
    /// (not eagerly resolved) so an explicit `.title(tr!(...))` follows a
    /// live locale switch — `accessibility()` re-resolves on the AT
    /// re-walk. The content-derived hint path is wrapped as a literal
    /// (the core `accessible_title_hint` trait returns a plain `String`,
    /// since core can't name `LocalizedString`); dialogs rebuild on show
    /// so the hint is still current-locale at present time.
    title: Option<LocalizedString>,
    /// Per-call override for the modal panel chrome. Replaces the
    /// theme-wide `style_slots.dialog` and the IntUI default
    /// `RecipeDialogStyle` for just this container.
    style_override: Option<SharedDialogStyle>,
    /// Build state — the `DialogStyle::make_panel` root.
    root_child_id: Option<WidgetId>,
}

impl ModalContainer {
    /// Wrap `content` inside a modal panel with default chrome.
    pub fn new(content: impl Widget + 'static) -> Self {
        Self::boxed(Box::new(content))
    }

    pub(crate) fn boxed(content: Box<dyn Widget>) -> Self {
        Self {
            content_id: None,
            pending_content: Some(content),
            padding_override: None,
            min_width_override: None,
            title: None,
            style_override: None,
            root_child_id: None,
        }
    }

    /// Override the content padding (logical pixels) from the theme default.
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding_override = Some(padding.max(0.0));
        self
    }

    /// Override the minimum panel width (logical pixels) from the theme default.
    pub fn min_width(mut self, min_width: f32) -> Self {
        self.min_width_override = Some(min_width.max(0.0));
        self
    }

    /// Per-call style override for the modal panel chrome. Replaces the
    /// theme-wide default `DialogStyle` for just this container.
    pub fn style(mut self, style: impl teksilo_core::styles::DialogStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Accessible title for the dialog. Screen readers announce this
    /// as the dialog's name. Should match the inner `DialogContent`'s
    /// visible title string.
    pub fn title(mut self, title: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = title.into();
        self.title = Some(ls);
        self
    }
}

impl std::fmt::Debug for ModalContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModalContainer")
            .field("padding_override", &self.padding_override)
            .field("min_width_override", &self.min_width_override)
            .finish()
    }
}

impl Widget for ModalContainer {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(content) = self.pending_content.take() {
            // If the caller didn't set an explicit `.title(...)`,
            // ask the content widget for a suggested title — e.g.
            // `DialogContent::accessible_title_hint` returns its
            // own visible title. This lets dialogs announce their
            // real name without forcing callers to duplicate the
            // string at both the content and the container level.
            if self.title.is_none()
                && let Some(hint) = content.accessible_title_hint()
            {
                // The core `accessible_title_hint` trait can only return a
                // plain `String`, so wrap it as a literal. Resolved fresh
                // at present time (dialogs rebuild on show).
                self.title = Some(LocalizedString::literal(hint));
            }
            self.content_id = Some(ctx.add_boxed(content));
        }

        // The panel chrome (rounded surface + border + content
        // padding) is owned by the active `DialogStyle`; the modal
        // mounting / dismissal pipeline stays on this widget.
        let content_id = self
            .content_id
            .expect("ModalContainer requires content — none was set");
        let style: SharedDialogStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.dialog.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeDialogStyle::default()));
        let cfg = DialogStyleConfig {
            content: content_id,
            has_scrim: true,
            padding_override: self.padding_override,
            min_width_override: self.min_width_override,
        };
        let root_id = style.make_panel(&cfg, ctx);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(240.0, 120.0))
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
        builder.set_role(teksilo_core::accesskit::Role::Dialog);
        let name = self
            .title
            .as_ref()
            .map(|t| t.resolve_now())
            .unwrap_or_else(|| teksilo_i18n::tr_widget!(a11y_dialog_name()).resolve_now());
        builder.set_name(name);
        // ModalContainer is always modal — it's the one path that goes
        // through `ModalRequest` / `ModalPresentation`. A dialog that
        // doesn't block outside interaction would use `Popover` instead.
        builder.set_modal();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Full-viewport dimming scrim painted behind a [`ModalContainer`].
///
/// Mounted by the modal-presentation pipeline (teksilo-app) as a separate
/// `OverlayPlacement::FullViewport` overlay pushed BEFORE the centered
/// modal overlay so it z-orders below the panel. The chrome itself is
/// delegated to the active `DialogStyle::make_scrim`; clicking the
/// scrim dismisses the linked modal when the modal's
/// [`ModalCloseBehavior`] permits click-outside dismissal.
///
/// The dismissal cascade is wired via
/// `OverlayManager::set_parent_overlay` AFTER both overlays are
/// pushed — the scrim's `parent_overlay` is set to the modal's id, so
/// any dismiss of the modal cascades through `dismiss_immediate` and
/// also dismisses the scrim. The scrim's own `dismiss` behavior is
/// `Manual` — it never dismisses itself directly.
pub struct ModalScrim {
    style_override: Option<SharedDialogStyle>,
    /// Filled in by the framework AFTER the modal overlay is pushed
    /// — the scrim is mounted FIRST (so it z-orders below the modal),
    /// so the modal's `OverlayId` isn't yet known at build time. The
    /// scrim's on-tap closure reads through this `Cell` at click time
    /// rather than capturing a value that doesn't exist yet.
    dismiss_target: Rc<Cell<Option<OverlayId>>>,
    /// Whether clicking the scrim should dismiss `dismiss_target`.
    /// Reflects the modal's [`ModalCloseBehavior`]: `true` for
    /// `ClickOutside` and `EscapeOrClickOutside`; `false` for
    /// `EscapeKey` and `Manual` (clicks on the dim are absorbed but
    /// do not dismiss).
    click_to_dismiss: bool,
    root_child_id: Option<WidgetId>,
}

impl ModalScrim {
    /// Build a new scrim; wire it with [`Self::dismiss_target`] and
    /// [`Self::click_to_dismiss`] after construction.
    pub fn new() -> Self {
        Self {
            style_override: None,
            dismiss_target: Rc::new(Cell::new(None)),
            click_to_dismiss: false,
            root_child_id: None,
        }
    }

    /// Per-call style override for the scrim chrome. Replaces the
    /// theme-wide default `DialogStyle` for just this scrim.
    pub fn style(mut self, style: impl teksilo_core::styles::DialogStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Handle to the modal-overlay id the scrim dismisses on click.
    /// The framework fills this AFTER the modal is pushed (see the
    /// in-tree modal pipeline in `teksilo-app`).
    pub fn dismiss_target(mut self, target: Rc<Cell<Option<OverlayId>>>) -> Self {
        self.dismiss_target = target;
        self
    }

    /// Enable click-to-dismiss on the scrim. Should mirror whether the
    /// modal's [`ModalCloseBehavior`] permits click-outside dismissal.
    pub fn click_to_dismiss(mut self, enabled: bool) -> Self {
        self.click_to_dismiss = enabled;
        self
    }
}

impl Default for ModalScrim {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ModalScrim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModalScrim")
            .field("click_to_dismiss", &self.click_to_dismiss)
            .finish()
    }
}

impl Widget for ModalScrim {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let style: SharedDialogStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.dialog.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeDialogStyle::default()));
        let chrome_id = style.make_scrim(ctx);

        if self.click_to_dismiss {
            let target = self.dismiss_target.clone();
            let handlers = HandlerSet::new().on_tap(move |_event, ctx| {
                if let Some(modal_id) = target.get() {
                    ctx.dismiss_overlay(modal_id);
                }
            });
            ctx.apply_self_handlers(handlers);
        }

        self.root_child_id = Some(chrome_id);
        vec![chrome_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // The scrim's actual size is determined by
        // `OverlayPlacement::FullViewport` in `position_overlays`,
        // which overrides the intrinsic size to the full viewport. We
        // still report the child's wanted size so the proposal flows
        // correctly when the framework probes the intrinsic size.
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
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Hidden from the AT: the modal panel above carries the
        // `Role::Dialog` node with the accessible name.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn queue_dialog_request(
    ctx: &mut EventContext,
    factory: &DialogFactory,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    title: &str,
    on_dismiss: Option<OverlayDismissCallback>,
) {
    let factory = factory.clone();
    let mut request = ModalRequest::deferred(move |tree| {
        let content = (factory.as_ref())();
        tree.add(ModalContainer::boxed(content))
    })
    .presentation(presentation)
    .close_behavior(close_behavior)
    .title(title)
    .size(460, 260);
    if let Some(cb) = on_dismiss {
        request = request.on_dismiss(cb);
    }
    ctx.present_modal(request);
}

/// Convenience body layout for a modal dialog: optional title, supporting text,
/// scrollable body slot, and a `Divider`-separated footer row.
pub struct DialogContent {
    title: Option<LocalizedString>,
    supporting_text: Option<LocalizedString>,
    pending_body: Option<PendingChild>,
    pending_footer: Option<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl DialogContent {
    /// Create an empty dialog body with no sections set.
    pub fn new() -> Self {
        Self {
            title: None,
            supporting_text: None,
            pending_body: None,
            pending_footer: None,
            root_child_id: None,
        }
    }

    /// Bold title shown at the top of the content area. Also propagated to
    /// the enclosing `ModalContainer` via `accessible_title_hint`.
    pub fn title(mut self, title: impl Into<LocalizedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Secondary description text shown below the title.
    pub fn supporting_text(mut self, text: impl Into<LocalizedString>) -> Self {
        self.supporting_text = Some(text.into());
        self
    }

    /// Main scrollable content slot (any widget).
    pub fn body(mut self, body: impl Widget + 'static) -> Self {
        self.pending_body = Some(PendingChild::Deferred(Box::new(body)));
        self
    }

    /// Main content slot by pre-registered `WidgetId`.
    pub fn body_id(mut self, id: WidgetId) -> Self {
        self.pending_body = Some(PendingChild::Id(id));
        self
    }

    /// Footer slot separated from the body by a `Divider` (typically action
    /// buttons like "OK" / "Cancel").
    pub fn footer(mut self, footer: impl Widget + 'static) -> Self {
        self.pending_footer = Some(PendingChild::Deferred(Box::new(footer)));
        self
    }

    /// Footer slot by pre-registered `WidgetId`.
    pub fn footer_id(mut self, id: WidgetId) -> Self {
        self.pending_footer = Some(PendingChild::Id(id));
        self
    }
}

impl Default for DialogContent {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DialogContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogContent")
            .field("title", &self.title)
            .field("supporting_text", &self.supporting_text)
            .finish()
    }
}

impl Widget for DialogContent {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut stack = VStack::new().spacing(16.0);

        if self.title.is_some() || self.supporting_text.is_some() {
            let mut header = VStack::new().spacing(8.0);
            if let Some(title) = self.title.clone() {
                header = header.child(
                    TextWidget::new(title)
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary)
                        .single_line(),
                );
            }
            if let Some(text) = self.supporting_text.clone() {
                header = header.child(
                    TextWidget::new(text)
                        .style(TextStyleRole::Body)
                        .color(TextRole::Secondary),
                );
            }
            let header_id = ctx.add(header);
            stack = stack.add_child(header_id);
        }

        if let Some(body) = self.pending_body.take() {
            let body_id = match body {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            stack = stack.add_child(body_id);
        }

        if let Some(footer) = self.pending_footer.take() {
            let divider_id = ctx.add(Divider::new());
            let footer_id = match footer {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            stack = stack.add_child(divider_id).add_child(footer_id);
        }

        let root = ctx.add(stack);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    /// Expose the visible title to an enclosing `ModalContainer`
    /// (or any other shell) so it can use it as its own accessible
    /// name without the caller having to thread the same string
    /// through twice.
    fn accessible_title_hint(&self) -> Option<String> {
        self.title.as_ref().map(|t| t.resolve_now())
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// A trigger button that presents a modal dialog when activated.
///
/// Renders as a `Button` by default; call `.trigger(w)` to replace it with any
/// widget. The content is lazily constructed by a factory closure each time the
/// dialog opens — no persistent widget subtree is kept while the dialog is closed.
pub struct Dialog {
    label: LocalizedString,
    variant: ButtonVariant,
    /// Enabled state, static or reactive; forwarded to the trigger at
    /// build time.
    enabled: Prop<bool>,
    presentation: ModalPresentation,
    close_behavior: ModalCloseBehavior,
    content_factory: Option<DialogFactory>,
    pending_trigger: Option<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl Dialog {
    /// Build a dialog trigger with `label` as the button text and accessible name.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Filled,
            enabled: Prop::Static(true),
            presentation: ModalPresentation::Auto,
            close_behavior: ModalCloseBehavior::EscapeOrClickOutside,
            content_factory: None,
            pending_trigger: None,
            root_child_id: None,
        }
    }

    /// Factory closure that builds the dialog's content each time it opens.
    /// Required — the dialog panics at build time if no factory is set.
    pub fn content<W, F>(mut self, factory: F) -> Self
    where
        W: Widget + 'static,
        F: Fn() -> W + 'static,
    {
        self.content_factory = Some(std::rc::Rc::new(move || {
            Box::new(factory()) as Box<dyn Widget>
        }));
        self
    }

    /// Visual style of the default trigger button. Has no effect when
    /// `.trigger(…)` replaces the button with a custom widget.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Enable or disable the trigger button, statically or reactively
    /// (default `true`).
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Override the modal presentation mode (default `ModalPresentation::Auto`).
    pub fn presentation(mut self, presentation: ModalPresentation) -> Self {
        self.presentation = presentation;
        self
    }

    /// Override how the dialog may be closed (default `EscapeOrClickOutside`).
    pub fn close_behavior(mut self, close_behavior: ModalCloseBehavior) -> Self {
        self.close_behavior = close_behavior;
        self
    }

    /// Replace the default `Button` trigger with a custom widget. The widget
    /// receives the same tap / key / AT-action handlers as the button would.
    pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self {
        self.pending_trigger = Some(PendingChild::Deferred(Box::new(trigger)));
        self
    }

    /// Custom trigger by pre-registered `WidgetId`.
    pub fn trigger_id(mut self, id: WidgetId) -> Self {
        self.pending_trigger = Some(PendingChild::Id(id));
        self
    }
}

impl std::fmt::Debug for Dialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dialog")
            .field("label", &self.label)
            .field("style", &self.variant)
            .field("enabled", &self.enabled.get())
            .finish()
    }
}

impl Widget for Dialog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let label = self.label.clone();
        // Live signal view of the enabled state — the manual gates below
        // run inside event closures dispatched later, so a plain `bool`
        // snapshot captured here would go stale for a `Prop::Bound`
        // value. `.as_signal()` returns the underlying signal when bound,
        // or wraps a static value in a fresh `Signal::new(v)`.
        let enabled = self.enabled.as_signal();
        let close_behavior = self.close_behavior;
        let presentation = self.presentation;
        let style = self.variant;
        let content_factory = self
            .content_factory
            .clone()
            .expect("Dialog requires .content(...) — no content factory was set");

        // Track whether the modal is currently open so the trigger can set
        // aria-expanded correctly. The dismiss callback resets it to false
        // regardless of which close path fires (Escape, click-outside, explicit
        // ctx.dismiss_modal()). Only in-tree presentations fire this callback.
        let is_open: Signal<bool> = ctx.signal(false);
        let dismiss_callback: OverlayDismissCallback = {
            let is_open = is_open.clone();
            std::rc::Rc::new(move || {
                is_open.set(false);
            })
        };

        let root_id = if let Some(trigger) = self.pending_trigger.take() {
            let tap_open = is_open.clone();
            let tap_dismiss = dismiss_callback.clone();
            let key_open = is_open.clone();
            let key_dismiss = dismiss_callback.clone();
            let action_open = is_open.clone();
            let action_dismiss = dismiss_callback.clone();
            let handlers = teksilo_core::widget_builder::HandlerSet::new()
                .focusable(true)
                .cursor(teksilo_core::widget::CursorIcon::Pointer)
                .on_tap({
                    let label = label.clone();
                    let content_factory = content_factory.clone();
                    let enabled = enabled.clone();
                    move |_pos, ctx| {
                        if !enabled.get() {
                            return;
                        }
                        tap_open.set(true);
                        queue_dialog_request(
                            ctx,
                            &content_factory,
                            presentation,
                            close_behavior,
                            &label.resolve_now(),
                            Some(tap_dismiss.clone()),
                        );
                    }
                })
                .on_key({
                    let label = label.clone();
                    let content_factory = content_factory.clone();
                    let enabled = enabled.clone();
                    move |event, ctx| match event {
                        WidgetEvent::KeyUp {
                            key: Key::Enter | Key::Space,
                            ..
                        } if enabled.get() => {
                            key_open.set(true);
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label.resolve_now(),
                                Some(key_dismiss.clone()),
                            );
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                })
                .on_access_action({
                    let label = label.clone();
                    let content_factory = content_factory.clone();
                    let enabled = enabled.clone();
                    move |action, ctx| {
                        if action == teksilo_core::accesskit::Action::Click && enabled.get() {
                            action_open.set(true);
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label.resolve_now(),
                                Some(action_dismiss.clone()),
                            );
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                });
            let overlay_trigger = match trigger {
                PendingChild::Id(id) => OverlayTrigger::from_id(id, handlers),
                PendingChild::Deferred(widget) => OverlayTrigger::new(widget, handlers),
            }
            .enabled(self.enabled.clone())
            .name(label)
            .has_popup(teksilo_core::accesskit::HasPopup::Dialog)
            .expanded_when(is_open.clone());
            ctx.add(overlay_trigger)
        } else {
            let tap_open = is_open.clone();
            let tap_dismiss = dismiss_callback.clone();
            let key_open = is_open.clone();
            let key_dismiss = dismiss_callback.clone();
            let action_open = is_open.clone();
            let action_dismiss = dismiss_callback.clone();
            ctx.add(
                Button::new(label)
                    .variant(style)
                    .enabled(enabled.clone())
                    .has_popup(teksilo_core::accesskit::HasPopup::Dialog)
                    .expanded_when(is_open.clone())
                    .on_tap({
                        let label = self.label.clone();
                        let content_factory = content_factory.clone();
                        let enabled = enabled.clone();
                        move |_pos, ctx| {
                            if !enabled.get() {
                                return;
                            }
                            tap_open.set(true);
                            queue_dialog_request(
                                ctx,
                                &content_factory,
                                presentation,
                                close_behavior,
                                &label.resolve_now(),
                                Some(tap_dismiss.clone()),
                            );
                        }
                    })
                    .on_key({
                        let label = self.label.clone();
                        let content_factory = content_factory.clone();
                        let enabled = enabled.clone();
                        move |event, ctx| match event {
                            WidgetEvent::KeyUp {
                                key: Key::Enter | Key::Space,
                                ..
                            } if enabled.get() => {
                                key_open.set(true);
                                queue_dialog_request(
                                    ctx,
                                    &content_factory,
                                    presentation,
                                    close_behavior,
                                    &label.resolve_now(),
                                    Some(key_dismiss.clone()),
                                );
                                EventResponse::Handled
                            }
                            _ => EventResponse::Ignored,
                        }
                    })
                    .on_access_action({
                        let label = self.label.clone();
                        let content_factory = content_factory.clone();
                        let enabled = enabled.clone();
                        move |action, ctx| {
                            if action == teksilo_core::accesskit::Action::Click && enabled.get() {
                                action_open.set(true);
                                queue_dialog_request(
                                    ctx,
                                    &content_factory,
                                    presentation,
                                    close_behavior,
                                    &label.resolve_now(),
                                    Some(action_dismiss.clone()),
                                );
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
                        }
                    }),
            )
        };

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(140.0, 40.0))
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
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::Size;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_core::{ModalContent, ModalPresentation};
    use teksilo_i18n::lit;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn access_click_opens_centered_dialog_overlay() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(Dialog::new(lit!("Open dialog")).content(|| FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.presentation, ModalPresentation::Auto);
        assert_eq!(
            requests[0].request.close_behavior,
            ModalCloseBehavior::EscapeOrClickOutside,
        );
        assert!(matches!(
            requests[0].request.content,
            ModalContent::Deferred(_)
        ));
    }

    #[test]
    fn dialog_surface_exposes_dialog_role() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(Dialog::new(lit!("Open dialog")).content(|| FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });

        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        let content_id = match request.content {
            ModalContent::Deferred(builder) => builder(&mut tree),
            ModalContent::ExistingWidget(_) => {
                unreachable!("dialog now always uses deferred content")
            }
        };
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let dialog = tree
            .find_by_role(teksilo_core::accesskit::Role::Dialog)
            .unwrap();
        let info = tree.accessibility_node(dialog);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::Dialog);
        assert!(tree.bounds(content_id).width > 0.0);
    }

    #[test]
    fn modal_container_inherits_title_from_dialog_content() {
        // When a ModalContainer wraps a DialogContent and the
        // caller didn't set an explicit title on the container,
        // the title should propagate automatically via
        // `Widget::accessible_title_hint`.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let container = tree.add(ModalContainer::new(
            DialogContent::new()
                .title(lit!("Delete file?"))
                .body(FixedLeaf(100.0, 40.0)),
        ));
        tree.layout(SizeProposal::exact(600.0, 400.0));
        let info = tree.accessibility_node(container);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::Dialog);
        assert_eq!(info.name(), Some("Delete file?"));
    }

    #[test]
    fn modal_container_explicit_title_wins_over_hint() {
        // An explicit `.title(...)` on ModalContainer takes
        // precedence over whatever the content suggests.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let container = tree.add(
            ModalContainer::new(
                DialogContent::new()
                    .title(lit!("Inner title"))
                    .body(FixedLeaf(100.0, 40.0)),
            )
            .title(lit!("Outer title")),
        );
        tree.layout(SizeProposal::exact(600.0, 400.0));
        let info = tree.accessibility_node(container);
        assert_eq!(info.name(), Some("Outer title"));
    }

    /// A panel that directs initial focus to its *second* child.
    ///
    /// The shape real dialogs have: the first focusable descendant is the
    /// close button in the title strip, and focus must land on the first form
    /// field instead — otherwise the dialog opens focused on "dismiss me" and
    /// swallows whatever the user types first.
    #[derive(Debug)]
    struct HintingPanel {
        first: Rc<std::cell::Cell<Option<WidgetId>>>,
        hinted: Rc<std::cell::Cell<Option<WidgetId>>>,
    }

    impl Widget for HintingPanel {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let first = ctx.add(FixedLeaf(40.0, 20.0));
            let hinted = ctx.add(FixedLeaf(40.0, 20.0));
            self.first.set(Some(first));
            self.hinted.set(Some(hinted));
            vec![first, hinted]
        }

        fn initial_focus_hint(&self) -> Option<WidgetId> {
            self.hinted.get()
        }

        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(220.0, 120.0).into()
        }
    }

    /// Wrapping content in a `ModalContainer` must not cost it the ability to
    /// direct initial focus.
    ///
    /// `ModalContainer` does **not** override `initial_focus_hint`, and does not
    /// need to: `WidgetTree::widget_initial_focus_hint` walks the subtree and
    /// finds the content's own hint through the container and its chrome panel.
    /// Nothing pinned that before, which made it look like a missing feature
    /// rather than a load-bearing one — and an app about to move a dozen
    /// hand-chromed panels onto `ModalContainer` is betting on it.
    ///
    /// If that walk is ever flattened to "ask the content root, then give up",
    /// every wrapped dialog silently reopens focused on its close button.
    #[test]
    fn modal_container_lets_its_content_direct_initial_focus() {
        let first = Rc::new(std::cell::Cell::new(None));
        let hinted = Rc::new(std::cell::Cell::new(None));

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let container = tree.add(ModalContainer::new(HintingPanel {
            first: first.clone(),
            hinted: hinted.clone(),
        }));
        tree.layout(SizeProposal::exact(600.0, 400.0));

        let target = tree.widget_initial_focus_hint(container);
        assert_eq!(
            target,
            hinted.get(),
            "the content's hint must survive being wrapped in a ModalContainer"
        );
        assert_ne!(
            target,
            first.get(),
            "…and must not fall back to the first descendant, which is the \
             close button in a real dialog"
        );
    }

    /// The other half: the walk reports a hint, it does not invent one. Content
    /// with nothing to say leaves the pipeline free to fall through to
    /// `first_focusable_descendant`.
    #[test]
    fn modal_container_without_a_hint_reports_none() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let container = tree.add(ModalContainer::new(FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal::exact(600.0, 400.0));

        assert_eq!(tree.widget_initial_focus_hint(container), None);
    }

    #[test]
    fn modal_container_preserves_shell_sizing_defaults() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let container = tree.add(ModalContainer::new(FixedLeaf(220.0, 120.0)));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        // DialogStyle defaults: 24 dp content_padding, 280 dp min_width.
        // Content 220×120 + 48 padding = 268×168, clamped to 280×168.
        let bounds = tree.bounds(container);
        assert!((bounds.width - 280.0).abs() < 0.01);
        assert!((bounds.height - 168.0).abs() < 0.01);
    }

    #[test]
    fn modal_container_custom_padding_changes_layout() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let container = tree.add(
            ModalContainer::new(FixedLeaf(220.0, 120.0))
                .padding(12.0)
                .min_width(200.0),
        );
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });

        let bounds = tree.bounds(container);
        assert!((bounds.width - 244.0).abs() < 0.01);
        assert!((bounds.height - 144.0).abs() < 0.01);
    }

    #[test]
    fn custom_trigger_opens_dialog_overlay() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(
            Dialog::new(lit!("Open dialog"))
                .content(|| FixedLeaf(220.0, 120.0))
                .trigger(FixedLeaf(140.0, 40.0)),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // The OverlayTrigger now routes its handlers onto the trigger
        // child (so real `Button` triggers, which install their own
        // gesture arena, can't consume the tap before the opener
        // fires). Clicking the wrapper hit-tests into the child, which
        // is where the handler lives.
        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.click(trigger);

        assert_eq!(tree.drain_pending_modal_requests().len(), 1);
    }

    #[test]
    fn dialog_content_helper_builds_dialog_sections() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(Dialog::new(lit!("Open dialog")).content(|| {
            DialogContent::new()
                .title(lit!("Review Changes"))
                .supporting_text(lit!("Confirm the staged updates before continuing."))
                .body(FixedLeaf(220.0, 120.0))
                .footer(Button::new(lit!("Close")))
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });

        let request = tree.drain_pending_modal_requests().pop().unwrap().request;
        match request.content {
            ModalContent::Deferred(builder) => {
                builder(&mut tree);
            }
            ModalContent::ExistingWidget(_) => {
                unreachable!("dialog now always uses deferred content")
            }
        }
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert!(tree.find_by_label("Review Changes").is_some());
        assert!(tree.find_by_label("Close").is_some());
    }

    #[test]
    fn dialog_presentation_can_be_overridden() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(
            Dialog::new(lit!("Open dialog"))
                .content(|| FixedLeaf(220.0, 120.0))
                .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request.presentation, ModalPresentation::InTree);
    }

    #[test]
    fn dialog_close_behavior_can_be_overridden() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(
            Dialog::new(lit!("Open dialog"))
                .content(|| FixedLeaf(220.0, 120.0))
                .close_behavior(ModalCloseBehavior::Manual),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let trigger = tree.find_by_label("Open dialog").unwrap();
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });

        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].request.close_behavior,
            ModalCloseBehavior::Manual
        );
    }

    #[test]
    #[should_panic(expected = "Dialog requires .content(...)")]
    fn dialog_without_content_panics_on_build() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(Dialog::new(lit!("Open dialog")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
    }
}
